//! Ownership shared by foreground TEST admission, workers and recovery.

use super::{Gate, Id, Outcome, Refusal, SHUTDOWN, STATE_UNAVAILABLE, Supervisor};
use std::{
    collections::{HashMap, hash_map::Entry},
    io,
    sync::{Arc, Mutex, atomic::Ordering},
};
use tokio::{
    runtime::Runtime,
    task::{AbortHandle, JoinSet},
};

type Entries<T> = Arc<Mutex<HashMap<Id, OwnedEntry<T>>>>;

pub(super) struct Registry<T>(Entries<T>);

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self(Arc::default())
    }
}

struct OwnedEntry<T> {
    owner: Arc<()>,
    value: T,
}

// The TEST producers and recovery adapter are installed in later chunks.
#[allow(dead_code, reason = "TEST producer and recovery wiring is deferred")]
impl<T> Registry<T> {
    /// Reserve the durable intent and register under the same foreground gate
    /// that recovery synchronizes with before observing worker ownership.
    pub(super) fn register(&self, subject: Id, value: T) -> Outcome<Registration<T>> {
        let mut entries = self.0.try_lock().map_err(|_| STATE_UNAVAILABLE)?;
        let Entry::Vacant(entry) = entries.entry(subject) else {
            return Err(Refusal::Busy.into());
        };
        let owner = Arc::new(());
        entry.insert(OwnedEntry {
            owner: owner.clone(),
            value,
        });
        Ok(Registration {
            entries: self.0.clone(),
            subject,
            owner,
        })
    }

    /// Access short-lived in-memory state, including a start's run entry.
    /// The callback must leave I/O and ledger work to the caller.
    pub(super) fn with_entry<R>(
        &self,
        subject: Id,
        f: impl FnOnce(Option<&mut T>) -> R,
    ) -> Outcome<R> {
        let mut entries = self.0.try_lock().map_err(|_| STATE_UNAVAILABLE)?;
        Ok(f(entries.get_mut(&subject).map(|entry| &mut entry.value)))
    }
}

pub(super) struct Registration<T> {
    entries: Entries<T>,
    subject: Id,
    owner: Arc<()>,
}

impl<T> Drop for Registration<T> {
    fn drop(&mut self) {
        let removed = {
            let mut entries = self.entries.lock().unwrap_or_else(|err| err.into_inner());
            match entries.entry(self.subject) {
                Entry::Occupied(entry) if Arc::ptr_eq(&entry.get().owner, &self.owner) => {
                    Some(entry.remove())
                }
                _ => None,
            }
        };
        drop(removed);
    }
}

impl Supervisor {
    #[allow(dead_code, reason = "TEST recovery wiring is deferred")]
    pub(super) fn observe_test_worker<T, R>(
        &self,
        registry: &Registry<T>,
        subject: Id,
        f: impl FnOnce(Option<&mut T>) -> R,
    ) -> Outcome<R> {
        // An ownership observation cannot pass a dispatch that has committed
        // its reservation and still holds the gate to finish the handoff.
        // A busy gate is unconfirmed custody for this bounded sweep turn.
        let _gate = self.gate.try_lock().map_err(|_| STATE_UNAVAILABLE)?;
        registry.with_entry(subject, f)
    }
}

#[derive(Default)]
pub(super) struct TestWorkers {
    tasks: JoinSet<anyhow::Result<()>>,
    failed: bool,
}

impl TestWorkers {
    pub(super) async fn join(mut self) -> anyhow::Result<()> {
        while let Some(done) = self.tasks.join_next().await {
            self.failed |= !matches!(done, Ok(Ok(())));
        }
        anyhow::ensure!(!self.failed, "TEST worker completion unconfirmed");
        Ok(())
    }
}

impl Gate {
    #[allow(dead_code, reason = "TEST producers are installed in later chunks")]
    pub(super) fn enqueue_test_worker<T: Send + 'static>(
        &mut self,
        supervisor: &Arc<Supervisor>,
        registration: Registration<T>,
        initialize: fn() -> io::Result<Runtime>,
        work: impl FnOnce(&Supervisor, &Runtime) -> anyhow::Result<()> + Send + 'static,
    ) -> Outcome<AbortHandle> {
        let workers = &mut self.test_workers;
        while let Some(done) = workers.tasks.try_join_next() {
            workers.failed |= !matches!(done, Ok(Ok(())));
        }
        if workers.failed {
            supervisor.fence();
        }
        if self.closed || supervisor.fenced.load(Ordering::SeqCst) {
            return Err(SHUTDOWN);
        }
        let supervisor = supervisor.clone();
        Ok(workers.tasks.spawn_blocking(move || {
            let _registration = registration;
            let runtime = initialize()?;
            // The worker includes terminal settlement. A failure leaves its
            // durable intent for recovery and still drops the registration.
            work(&supervisor, &runtime)
        }))
    }
}

#[cfg(test)]
mod tests;

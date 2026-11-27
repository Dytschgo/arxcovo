//! Ownership shared by foreground TEST admission, workers and recovery.

use super::{Gate, Id, Outcome, Refusal, SHUTDOWN, STATE_UNAVAILABLE, Supervisor};
use std::{
    collections::{HashMap, hash_map::Entry},
    future::Future,
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
    value: Option<T>,
}

// The TEST producers and recovery adapter are installed in later chunks.
#[allow(dead_code, reason = "TEST producer and recovery wiring is deferred")]
impl<T> Registry<T> {
    /// Reserve the durable intent and register under the same foreground gate
    /// that recovery synchronizes with before observing worker ownership.
    pub(super) fn register(&self, subject: Id, value: T) -> Outcome<Registration<T>> {
        self.reserve(subject, Some(value))
    }

    fn reserve(&self, subject: Id, value: Option<T>) -> Outcome<Registration<T>> {
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

    /// Reserve a fresh owner's slot before its durable admission commits.
    /// Only gated paths fill a slot, so a reservation taken under the gate
    /// stays available until [`Registration::install`] or drop.
    pub(super) fn reserve_owner(&self, subject: Id) -> Outcome<Registration<T>> {
        self.reserve(subject, None)
    }

    /// Fence worker reacquisition for an ordinary or vault release attempt.
    /// Drop removes only this reaper's entry, even across an unwind.
    pub(super) fn fence_orphan(&self, subject: Id) -> Outcome<Registration<T>> {
        self.reserve(subject, None)
    }

    /// Access short-lived in-memory state, including a start's run entry.
    /// The callback must leave I/O and ledger work to the caller.
    pub(super) fn with_entry<R>(
        &self,
        subject: Id,
        f: impl FnOnce(Option<&mut T>) -> R,
    ) -> Outcome<R> {
        let mut entries = self.0.try_lock().map_err(|_| STATE_UNAVAILABLE)?;
        match entries.get_mut(&subject) {
            None => Ok(f(None)),
            Some(entry) => Ok(f(Some(entry.value.as_mut().ok_or(Refusal::Busy)?))),
        }
    }
}

pub(super) struct Registration<T> {
    entries: Entries<T>,
    subject: Id,
    owner: Arc<()>,
}

impl<T> Registration<T> {
    /// Install the owner in this exact reserved slot. Only this
    /// reservation's own drop removes the entry, so it cannot be refused.
    #[allow(dead_code, reason = "TEST producers are installed in later chunks")]
    pub(super) fn install(&self, value: T) -> Outcome<()> {
        let mut entries = self.entries.lock().unwrap_or_else(|err| err.into_inner());
        match entries.get_mut(&self.subject) {
            Some(entry) if Arc::ptr_eq(&entry.owner, &self.owner) => {
                entry.value = Some(value);
                Ok(())
            }
            _ => Err(STATE_UNAVAILABLE),
        }
    }
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
    #[allow(dead_code, reason = "native TEST recovery installation is deferred")]
    pub(super) fn with_test_orphan_fence<T>(
        &self,
        registry: &Registry<T>,
        subject: Id,
        recover: &mut dyn FnMut() -> bool,
    ) -> bool {
        let owner = {
            let Ok(_gate) = self.gate.try_lock() else {
                return false;
            };
            registry.fence_orphan(subject)
        };
        let Ok(_owner) = owner else {
            return false;
        };
        recover()
    }

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

struct OnWorkerExit<F: FnOnce(&Supervisor)> {
    supervisor: Arc<Supervisor>,
    cleanup: Option<F>,
}

impl<F: FnOnce(&Supervisor)> Drop for OnWorkerExit<F> {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup(&self.supervisor);
        }
    }
}

impl Gate {
    /// Drain finished workers. A failed one fences fresh TEST admission, and
    /// a closed or fenced supervisor refuses before any durable commit.
    #[allow(dead_code, reason = "TEST producers are installed in later chunks")]
    pub(super) fn test_workers_open(&mut self, supervisor: &Supervisor) -> Outcome<()> {
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
        Ok(())
    }

    #[allow(dead_code, reason = "TEST producers are installed in later chunks")]
    pub(super) fn enqueue_test_worker<T: Send + 'static>(
        &mut self,
        supervisor: &Arc<Supervisor>,
        registration: Registration<T>,
        initialize: fn() -> io::Result<Runtime>,
        work: impl FnOnce(&Supervisor, &Runtime) -> anyhow::Result<()> + Send + 'static,
    ) -> Outcome<AbortHandle> {
        self.enqueue_test_worker_with_cleanup(supervisor, registration, initialize, |_| {}, work)
    }

    /// Enqueue a tracked TEST worker whose `cleanup` runs on every post-enqueue
    /// failure (pre-entry runtime initialization error, panic, abort before
    /// entry, or worker error) before the registration is released.
    #[allow(dead_code, reason = "TEST producers are installed in later chunks")]
    pub(super) fn enqueue_test_worker_with_cleanup<T: Send + 'static>(
        &mut self,
        supervisor: &Arc<Supervisor>,
        registration: Registration<T>,
        initialize: fn() -> io::Result<Runtime>,
        cleanup: impl FnOnce(&Supervisor) + Send + 'static,
        work: impl FnOnce(&Supervisor, &Runtime) -> anyhow::Result<()> + Send + 'static,
    ) -> Outcome<AbortHandle> {
        self.enqueue_test_blocking(supervisor, registration, cleanup, move |supervisor| {
            let runtime = initialize()?;
            work(supervisor, &runtime)
        })
    }

    pub(super) fn enqueue_test_blocking<T: Send + 'static>(
        &mut self,
        supervisor: &Arc<Supervisor>,
        registration: Registration<T>,
        cleanup: impl FnOnce(&Supervisor) + Send + 'static,
        work: impl FnOnce(&Supervisor) -> anyhow::Result<()> + Send + 'static,
    ) -> Outcome<AbortHandle> {
        self.test_workers_open(supervisor)?;
        let on_exit = OnWorkerExit {
            supervisor: supervisor.clone(),
            cleanup: Some(cleanup),
        };
        let supervisor = supervisor.clone();
        Ok(self.test_workers.tasks.spawn_blocking(move || {
            let _registration = registration;
            let mut on_exit = on_exit;
            // The worker includes terminal settlement. A failure leaves its
            // durable intent for recovery and still drops the registration.
            let result = work(&supervisor);
            if result.is_ok() {
                on_exit.cleanup = None;
            }
            result
        }))
    }

    /// Track a bounded recovery task for an already-committed durable intent
    /// after handoff refusal. Joined by `drain_workers` even when fresh
    /// admission is fenced.
    #[allow(dead_code, reason = "TEST producers are installed in later chunks")]
    pub(super) fn spawn_test_recovery(
        &mut self,
        task: impl Future<Output = anyhow::Result<()>> + Send + 'static,
    ) {
        self.test_workers.tasks.spawn(task);
    }
}

#[cfg(test)]
mod tests;

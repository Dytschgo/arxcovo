//! Foreground handler admission. The SDK detaches request handlers, so joining
//! connection tasks cannot prove that no handler is still between its started
//! activity row and terminal audit. Every foreground handler therefore takes an
//! `Admission` before any activity write and holds it through terminal commit,
//! handing it to the executor worker when one runs. `drain` closes admission
//! with one atomic operation on the same counter, so once it observes zero, no
//! admitted handler remains and none can be admitted later.
use super::*;

/// High bit of `Execution::pending`: admission is closed. Low bits: live tickets.
const CLOSED: usize = !(usize::MAX >> 1);

/// One admitted handler. While `open` (a started row awaits its terminal
/// commit), dropping it latches `completion_unconfirmed`, so a panic, dropped
/// future or failed commit can never acknowledge a drain.
pub struct Admission {
    execution: Arc<Execution>,
    open: bool,
}
impl Admission {
    /// The started activity row has committed.
    pub fn started(&mut self) {
        self.open = true;
    }
    /// The terminal activity row has committed.
    pub fn completed(&mut self) {
        self.open = false;
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        if self.open {
            self.execution
                .completion_unconfirmed
                .store(true, Ordering::Release);
        }
        self.execution.pending.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Execution {
    /// None once `drain` has closed admission: the caller must not write activity.
    pub fn admit(self: &Arc<Self>) -> Option<Admission> {
        let mut value = self.pending.load(Ordering::Acquire);
        loop {
            if value & CLOSED != 0 || value == CLOSED - 1 {
                return None;
            }
            match self.pending.compare_exchange_weak(
                value,
                value + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(Admission {
                        execution: self.clone(),
                        open: false,
                    });
                }
                Err(current) => value = current,
            }
        }
    }
    /// Unit-test-only registration for direct worker tests, regardless of
    /// closure; production handlers can only use `admit`.
    #[cfg(test)]
    pub(crate) fn register(self: &Arc<Self>) -> Admission {
        self.pending.fetch_add(1, Ordering::AcqRel);
        Admission {
            execution: self.clone(),
            open: true,
        }
    }
    /// Closes admission, then waits for every admitted handler to commit its
    /// terminal audit. False when the bounded wait expires or any admitted
    /// completion was lost.
    pub async fn drain(&self) -> bool {
        self.pending.fetch_or(CLOSED, Ordering::AcqRel);
        let idle = || self.pending.load(Ordering::Acquire) & !CLOSED == 0;
        for _ in 0..500 {
            if idle() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        idle() && !self.completion_unconfirmed.load(Ordering::Acquire)
    }
}

/// Unit-test-only handler checkpoints; absent from every non-test build.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Checkpoint {
    /// SDK handler polled, before any admission or activity write.
    Entry,
    /// Started activity row committed, before executor dispatch.
    Started,
}
/// Parks the first handler reaching `point` until released.
#[cfg(test)]
pub(crate) struct Hold {
    point: Checkpoint,
    armed: AtomicBool,
    pub(crate) reached: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
    /// Notified when the held handler's `call_tool` returns.
    pub(crate) left: tokio::sync::Notify,
}
#[cfg(all(test, windows))]
impl Hold {
    pub(crate) fn new(point: Checkpoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            armed: AtomicBool::new(true),
            reached: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
            left: tokio::sync::Notify::new(),
        })
    }
    pub(crate) fn release(&self) {
        self.release.add_permits(1);
    }
}
#[cfg(test)]
pub(crate) struct Visit(Arc<Hold>);
#[cfg(test)]
impl Drop for Visit {
    fn drop(&mut self) {
        self.0.left.notify_one();
    }
}
#[cfg(test)]
impl Execution {
    pub(crate) async fn checkpoint(&self, point: Checkpoint) -> Option<Visit> {
        let hold = self.hold.as_ref()?;
        if hold.point != point || !hold.armed.swap(false, Ordering::AcqRel) {
            return None;
        }
        hold.reached.notify_one();
        if let Ok(permit) = hold.release.acquire().await {
            permit.forget();
        }
        Some(Visit(hold.clone()))
    }
    /// Normal current-user mode with a held handler. Shutdown refuses before
    /// the probe, which panics if a held request were ever executed.
    #[cfg(windows)]
    pub(crate) fn hold_fixture(hold: Arc<Hold>) -> Arc<Self> {
        let mut execution = Self::build(
            Settings {
                mode: Mode::CurrentUser,
                acknowledge: Acknowledge::Shared,
                workspace: PathBuf::from("C:\\workspace"),
                environment: BTreeMap::from([("PATH".into(), "C:\\tools".into())]),
                limits: Limits {
                    memory_bytes: 16777216,
                    pids: 8,
                },
            },
            false,
        );
        execution.identity_mode = IdentityMode::CurrentUser;
        execution.probe = Some(Arc::new(|_, _| panic!("held request must not execute")));
        execution.hold = Some(hold);
        Arc::new(execution)
    }
}

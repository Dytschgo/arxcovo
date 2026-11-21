//! Bounded diagnostics for the disposable C13 foreground processes.
#[cfg(any(test, target_os = "linux"))]
use std::sync::{Mutex, mpsc::SyncSender};
#[cfg(target_os = "linux")]
use std::{
    fs::File,
    io::{self, Write},
    os::fd::AsFd,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::sync_channel,
    },
};

#[cfg(any(test, target_os = "linux"))]
const LIMIT: usize = 24;
#[cfg(any(test, target_os = "linux"))]
type Record = (&'static str, &'static str);

#[cfg(any(test, target_os = "linux"))]
fn fixture(value: &str) -> Option<&'static str> {
    match value {
        "a" => Some("a"),
        "b" => Some("b"),
        "c" => Some("c"),
        "d" => Some("d"),
        _ => None,
    }
}

#[cfg(any(test, target_os = "linux"))]
#[derive(Default)]
struct Records([Option<Record>; LIMIT]);

#[cfg(any(test, target_os = "linux"))]
impl Records {
    fn insert(&mut self, record: Record) -> bool {
        for entry in &mut self.0 {
            match entry {
                Some(prior) if *prior == record => return false,
                None => {
                    *entry = Some(record);
                    return true;
                }
                _ => {}
            }
        }
        false
    }
}

#[cfg(any(test, target_os = "linux"))]
struct Sink {
    records: Mutex<Records>,
    sender: SyncSender<Record>,
}

#[cfg(any(test, target_os = "linux"))]
impl Sink {
    fn new(sender: SyncSender<Record>) -> Self {
        Self {
            records: Mutex::new(Records::default()),
            sender,
        }
    }

    fn offer(&self, record: Record) {
        if self
            .records
            .try_lock()
            .is_ok_and(|mut records| records.insert(record))
        {
            let _ = self.sender.try_send(record);
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn record(stage: &'static str, category: &'static str) {
    static SINK: OnceLock<Sink> = OnceLock::new();
    static STARTED: AtomicBool = AtomicBool::new(false);
    // Concurrent producers can drop diagnostics while initialization is pending.
    if SINK.get().is_none()
        && !STARTED.swap(true, Ordering::Relaxed)
        && let Some(node) = std::env::var("ARX_C13_DIAGNOSTICS")
            .ok()
            .as_deref()
            .and_then(fixture)
        && let Some(sink) = start_writer(node)
    {
        let _ = SINK.set(sink);
    }
    if let Some(sink) = SINK.get() {
        sink.offer((stage, category));
    }
}

#[cfg(target_os = "linux")]
fn start_writer(node: &'static str) -> Option<Sink> {
    let (sender, receiver) = sync_channel::<Record>(LIMIT);
    std::thread::Builder::new()
        .spawn(move || {
            // Own a duplicate so a blocked write cannot hold Rust's stdout lock.
            let Ok(fd) = io::stdout().as_fd().try_clone_to_owned() else {
                return;
            };
            let mut output = File::from(fd);
            for (stage, category) in receiver {
                let line =
                    format!("\nC13 diagnostic node={node} stage={stage} category={category}\n");
                if output.write_all(line.as_bytes()).is_err() {
                    break;
                }
            }
        })
        .ok()?;
    Some(Sink::new(sender))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn record(_stage: &'static str, _category: &'static str) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread, time::Duration};
    use tokio::time::{Instant, timeout_at};

    #[test]
    fn fixture_diagnostic_records_are_deduplicated_and_bounded() {
        assert_eq!(fixture("a"), Some("a"));
        assert_eq!(fixture("d"), Some("d"));
        assert_eq!(fixture("a\ncategory=forged"), None);
        let mut records = Records([Some(("stage", "category")); LIMIT]);
        records.0[LIMIT - 1] = None;
        assert!(!records.insert(("stage", "category")));
        assert!(records.insert(("stage", "busy")));
        assert!(!records.insert(("stage", "clock")));
    }

    #[test]
    fn saturated_or_disconnected_sink_preserves_refusal_and_deadline() {
        const BUDGET: Duration = Duration::from_millis(4);
        for connected in [true, false] {
            let (sender, receiver) = mpsc::sync_channel(1);
            sender.try_send(("fixture", "busy")).unwrap();
            let undrained = connected.then_some(receiver);
            let (done, observed) = mpsc::channel();
            let caller = thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .start_paused(true)
                    .build()
                    .unwrap();
                let sink = Sink::new(sender);
                runtime.block_on(async {
                    let deadline = Instant::now() + BUDGET;
                    let result = timeout_at(deadline, async {
                        Err::<(), _>("refused").inspect_err(|_| sink.offer(("fixture", "refused")))
                    })
                    .await;
                    done.send((result.ok(), deadline - Instant::now())).unwrap();
                });
            });
            let result = observed.recv_timeout(Duration::from_secs(5));
            // Keep the full queue undrained until the caller returns or the watchdog expires.
            drop(undrained);
            caller.join().unwrap();
            assert_eq!(result.unwrap(), (Some(Err("refused")), BUDGET));
        }
    }
}

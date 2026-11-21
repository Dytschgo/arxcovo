//! Bounded diagnostics for the disposable C13 foreground processes.
#[cfg(target_os = "linux")]
#[path = "../../tests/support/c13_diagnostic_endpoint.rs"]
pub(crate) mod endpoint;

#[cfg(target_os = "linux")]
use std::{
    io,
    os::fd::{AsFd, AsRawFd, OwnedFd},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
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

#[cfg(target_os = "linux")]
struct Sink {
    node: &'static str,
    writer: OwnedFd,
    records: Mutex<Records>,
}

#[cfg(target_os = "linux")]
impl Sink {
    fn new(node: &'static str, writer: OwnedFd) -> Self {
        Self {
            node,
            writer,
            records: Mutex::new(Records::default()),
        }
    }

    fn offer(&self, (stage, category): Record) {
        let fd = self.writer.as_raw_fd();
        if endpoint::pipe_writer_identity(fd).is_none() {
            return;
        }
        if !self
            .records
            .try_lock()
            .is_ok_and(|mut records| records.insert((stage, category)))
        {
            return;
        }
        let node = self.node;
        let line = format!("\nC13 diagnostic node={node} stage={stage} category={category}\n");
        // A write well below PIPE_BUF is atomic. Drop on any error, including
        // EAGAIN/EPIPE. The executable uses Rust's default ignored SIGPIPE.
        if line.len() <= 128 {
            // SAFETY: the slice is live and the fixture installed a nonblocking pipe.
            unsafe { libc::write(fd, line.as_ptr().cast(), line.len()) };
        }
    }
}

#[cfg(target_os = "linux")]
static SINK: OnceLock<Option<Sink>> = OnceLock::new();

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn register_publication_writer(writer: Option<OwnedFd>) {
    let sink = writer.and_then(|writer| {
        let node = std::env::var("ARX_C13_DIAGNOSTICS").ok()?;
        (fixture(&node) == Some("d")).then(|| Sink::new("d", writer))
    });
    let _ = SINK.set(sink);
}

#[cfg(target_os = "linux")]
pub(crate) fn record(stage: &'static str, category: &'static str) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if SINK.get().is_none() && !STARTED.swap(true, Ordering::Relaxed) {
        let sink = std::env::var("ARX_C13_DIAGNOSTICS")
            .ok()
            .as_deref()
            .and_then(fixture)
            .filter(|node| *node != "d")
            .and_then(|node| {
                let writer = io::stdout().as_fd().try_clone_to_owned().ok()?;
                Some(Sink::new(node, writer))
            });
        let _ = SINK.set(sink);
    }
    if let Some(Some(sink)) = SINK.get() {
        sink.offer((stage, category));
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn record(_stage: &'static str, _category: &'static str) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::os::fd::FromRawFd;

    #[cfg(target_os = "linux")]
    fn pipe() -> [OwnedFd; 2] {
        let mut fds = [-1; 2];
        // SAFETY: pipe2 writes two distinct owned descriptors on success.
        assert_eq!(
            unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) },
            0
        );
        fds.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sink_delivers_an_allowed_record_once() {
        use std::{fs::File, io::Read};

        let [read, write] = pipe();
        let sink = Sink::new("a", write);
        for _ in 0..2 {
            sink.offer(("admission-worker", "node-busy"));
        }
        drop(sink);
        let mut output = Vec::new();
        File::from(read).read_to_end(&mut output).unwrap();
        assert_eq!(
            output,
            b"\nC13 diagnostic node=a stage=admission-worker category=node-busy\n"
        );
    }

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

    #[cfg(target_os = "linux")]
    #[test]
    fn full_blocking_or_closed_pipe_preserves_refusal_and_deadline() {
        use std::{sync::mpsc, thread, time::Duration};
        use tokio::time::{Instant, timeout_at};

        const BUDGET: Duration = Duration::from_millis(4);
        for (connected, blocking) in [(true, false), (true, true), (false, false)] {
            let [read, write] = pipe();
            let fd = write.as_raw_fd();
            let capacity = unsafe { libc::fcntl(fd, libc::F_GETPIPE_SZ) };
            assert!(capacity > 0);
            let fill = vec![b'x'; capacity as usize];
            assert_eq!(
                unsafe { libc::write(fd, fill.as_ptr().cast(), fill.len()) },
                capacity as isize
            );
            if blocking {
                assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFL, 0) }, 0);
            }
            let undrained = connected.then_some(read);
            let (done, observed) = mpsc::channel();
            let caller = thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .start_paused(true)
                    .build()
                    .unwrap();
                let sink = Sink::new("a", write);
                runtime.block_on(async {
                    let deadline = Instant::now() + BUDGET;
                    let result = timeout_at(deadline, async {
                        Err::<(), _>("refused")
                            .inspect_err(|_| sink.offer(("admission-worker", "refused")))
                    })
                    .await;
                    done.send((result.ok(), deadline - Instant::now())).unwrap();
                });
            });
            let result = observed.recv_timeout(Duration::from_secs(5));
            // Keep the full pipe undrained until the caller returns or the watchdog expires.
            drop(undrained);
            caller.join().unwrap();
            assert_eq!(result.unwrap(), (Some(Err("refused")), BUDGET));
        }
    }
}

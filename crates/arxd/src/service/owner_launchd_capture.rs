//! Private launchd-only capture. No stream formatting, forwarding, or selectors.
use super::super::owner_query::{self, LaunchdObservation, LaunchdTermination as Term, Query};
use std::io;

const CAPS: [usize; 2] = [65_536, 4_096];
const DEADLINE_MS: u64 = 10_000;
trait Transport {
    fn spawn(&mut self) -> Result<(), ()>;
    fn setup(&mut self) -> Result<(), ()>;
    fn read(&mut self, stream: usize, buf: &mut [u8]) -> io::Result<usize>;
    fn wait(&mut self) -> Result<Option<Term>, ()>;
    fn kill(&mut self);
    fn now_ms(&self) -> u64;
    fn pause(&mut self, ms: u64);
}
struct Capture {
    termination: Term,
    bytes: [Vec<u8>; 2],
    complete: [bool; 2],
    truncated: [bool; 2],
}
impl Capture {
    fn query(&self) -> Query {
        owner_query::launchd(LaunchdObservation {
            termination: self.termination,
            stdout: &self.bytes[0],
            stderr: &self.bytes[1],
            stdout_complete: self.complete[0],
            stderr_complete: self.complete[1],
            stdout_truncated: self.truncated[0],
            stderr_truncated: self.truncated[1],
        })
    }
}
fn capture(t: &mut impl Transport) -> Capture {
    let mut result = Capture {
        termination: Term::Incomplete,
        bytes: [Vec::new(), Vec::new()],
        complete: [false; 2],
        truncated: [false; 2],
    };
    let deadline = t.now_ms().saturating_add(DEADLINE_MS);
    if t.spawn().is_err() {
        return result;
    }
    let success = (|| {
        t.setup()?;
        let mut exited = None;
        loop {
            if t.now_ms() >= deadline {
                return Err(());
            }
            let mut failed = false;
            // Touch both streams on each pass, even if one fails or floods.
            for (stream, cap) in CAPS.iter().copied().enumerate() {
                if result.complete[stream] {
                    continue;
                }
                let mut buf = [0; 1024];
                for _ in 0..65 {
                    match t.read(stream, &mut buf) {
                        Ok(0) => {
                            result.complete[stream] = true;
                            break;
                        }
                        Ok(n) if n <= buf.len() => {
                            let remaining = cap - result.bytes[stream].len();
                            result.bytes[stream].extend_from_slice(&buf[..n.min(remaining)]);
                            if n > remaining {
                                result.truncated[stream] = true;
                                failed = true;
                                break;
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        _ => {
                            failed = true;
                            break;
                        }
                    }
                }
            }
            if failed || t.now_ms() >= deadline {
                return Err(());
            }
            if exited.is_none() {
                exited = t.wait()?;
            }
            if let Some(status) = exited
                && result.complete == [true; 2]
            {
                result.termination = status;
                return Ok(());
            }
            t.pause(10.min(deadline.saturating_sub(t.now_ms())));
        }
    })()
    .is_ok();
    if !success {
        result.termination = Term::Incomplete;
        t.kill();
        let reap_deadline = t.now_ms().saturating_add(1000);
        while t.now_ms() < reap_deadline {
            if matches!(t.wait(), Ok(Some(_))) {
                break;
            }
            t.pause(10.min(reap_deadline.saturating_sub(t.now_ms())));
        }
    }
    result
}

#[cfg(unix)]
pub(super) fn native_query() -> Query {
    use std::{
        io::Read,
        os::fd::AsRawFd,
        process::{Child, Command, Stdio},
        time::Instant,
    };
    struct Native {
        child: Option<Child>,
        start: Instant,
    }
    impl Transport for Native {
        fn spawn(&mut self) -> Result<(), ()> {
            self.child = Some(
                Command::new("/bin/launchctl")
                    .args(owner_query::LAUNCHD_QUERY)
                    .env_clear()
                    .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
                    .env("LANG", "C")
                    .env("LC_ALL", "C")
                    .current_dir("/")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|_| ())?,
            );
            Ok(())
        }
        fn setup(&mut self) -> Result<(), ()> {
            let child = self.child.as_ref().ok_or(())?;
            let fds = [
                child.stdout.as_ref().ok_or(())?.as_raw_fd(),
                child.stderr.as_ref().ok_or(())?.as_raw_fd(),
            ];
            for fd in fds {
                let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
                if flags < 0
                    || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
                {
                    return Err(());
                }
            }
            Ok(())
        }
        fn read(&mut self, stream: usize, buf: &mut [u8]) -> io::Result<usize> {
            let child = self
                .child
                .as_mut()
                .ok_or_else(|| io::Error::from(io::ErrorKind::Other))?;
            match stream {
                0 => child
                    .stdout
                    .as_mut()
                    .ok_or_else(|| io::Error::from(io::ErrorKind::Other))?
                    .read(buf),
                _ => child
                    .stderr
                    .as_mut()
                    .ok_or_else(|| io::Error::from(io::ErrorKind::Other))?
                    .read(buf),
            }
        }
        fn wait(&mut self) -> Result<Option<Term>, ()> {
            self.child
                .as_mut()
                .ok_or(())?
                .try_wait()
                .map(|s| s.map(|s| s.code().map_or(Term::Signalled, Term::Exited)))
                .map_err(|_| ())
        }
        fn kill(&mut self) {
            if let Some(c) = &mut self.child {
                let _ = c.kill();
            }
        }
        fn now_ms(&self) -> u64 {
            self.start.elapsed().as_millis().min(u64::MAX as u128) as u64
        }
        fn pause(&mut self, ms: u64) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
    capture(&mut Native {
        child: None,
        start: Instant::now(),
    })
    .query()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    #[derive(Clone, Copy)]
    enum Read {
        Data(usize),
        Eof,
        Block,
        Fail,
        Flood,
    }
    struct Mock {
        reads: [VecDeque<Read>; 2],
        touches: [usize; 2],
        time: u64,
        tick: u64,
        spawn_fail: bool,
        setup_fail: bool,
        wait_fail: bool,
        status: Option<Term>,
        waits: usize,
        exit_after: usize,
        kills: usize,
    }
    impl Default for Mock {
        fn default() -> Self {
            Self {
                reads: [VecDeque::from([Read::Eof]), VecDeque::from([Read::Eof])],
                touches: [0; 2],
                time: 0,
                tick: 0,
                spawn_fail: false,
                setup_fail: false,
                wait_fail: false,
                status: Some(Term::Exited(113)),
                waits: 0,
                exit_after: 0,
                kills: 0,
            }
        }
    }
    impl Transport for Mock {
        fn spawn(&mut self) -> Result<(), ()> {
            if self.spawn_fail { Err(()) } else { Ok(()) }
        }
        fn setup(&mut self) -> Result<(), ()> {
            if self.setup_fail { Err(()) } else { Ok(()) }
        }
        fn read(&mut self, s: usize, buf: &mut [u8]) -> io::Result<usize> {
            assert!(buf.len() <= 1024, "read-size-bound");
            self.touches[s] += 1;
            self.time += self.tick;
            match self.reads[s].pop_front().unwrap_or(Read::Block) {
                Read::Data(n) => {
                    buf[..n].fill(b'x');
                    Ok(n)
                }
                Read::Flood => {
                    self.reads[s].push_front(Read::Flood);
                    buf.fill(b'x');
                    Ok(buf.len())
                }
                Read::Eof => Ok(0),
                Read::Block => Err(io::ErrorKind::WouldBlock.into()),
                Read::Fail => Err(io::ErrorKind::Other.into()),
            }
        }
        fn wait(&mut self) -> Result<Option<Term>, ()> {
            self.waits += 1;
            if self.wait_fail {
                Err(())
            } else if self.waits <= self.exit_after {
                Ok(None)
            } else {
                Ok(self.status)
            }
        }
        fn kill(&mut self) {
            self.kills += 1;
        }
        fn now_ms(&self) -> u64 {
            self.time
        }
        fn pause(&mut self, ms: u64) {
            assert!(ms <= 10, "pause-bound");
            self.time += ms;
        }
    }
    #[test]
    fn pure_capture_exact_caps_nonzero_and_signal() {
        for term in [Term::Exited(0), Term::Exited(113), Term::Signalled] {
            let mut m = Mock {
                status: Some(term),
                ..Default::default()
            };
            m.reads = [
                VecDeque::from(vec![Read::Data(1024); 64]),
                VecDeque::from(vec![Read::Data(1024); 4]),
            ];
            for stream in &mut m.reads {
                stream.push_back(Read::Eof);
            }
            let c = capture(&mut m);
            assert!(c.termination == term, "termination-preserved");
            assert!(c.complete == [true; 2], "both-eof");
            assert!(c.truncated == [false; 2], "exact-cap-not-truncated");
            assert!(
                c.bytes[0].len() == CAPS[0] && c.bytes[1].len() == CAPS[1],
                "exact-caps"
            );
            assert!(
                c.query() == Query::Unknown,
                "capture-maps-through-empty-profile"
            );
            assert!(m.kills == 0, "complete-observation-no-kill");
        }
    }
    #[test]
    fn pure_capture_fair_flood_and_overflow() {
        for (stream, cap) in CAPS.iter().copied().enumerate() {
            let mut m = Mock::default();
            m.reads[stream] = VecDeque::from([Read::Flood]);
            let c = capture(&mut m);
            assert!(
                c.truncated[stream] && c.bytes[stream].len() == cap,
                "overflow-capped"
            );
            assert!(m.touches[0] > 0 && m.touches[1] > 0, "fair-dual-streams");
            assert!(m.touches[0] <= 65 && m.touches[1] <= 65, "bounded-pass");
            assert!(
                c.termination == Term::Incomplete && c.query() == Query::Unknown,
                "overflow-refuses"
            );
            assert!(m.kills == 1, "failure-kills-once");
        }
    }
    #[test]
    fn pure_capture_failures_deadline_and_bounded_reap() {
        for fault in 0..8 {
            let mut m = Mock::default();
            match fault {
                0 => m.spawn_fail = true,
                1 => m.setup_fail = true,
                2 => m.reads[0] = VecDeque::from([Read::Fail]),
                3 => m.wait_fail = true,
                4 => m.reads[1] = VecDeque::from([Read::Block]),
                5 => {
                    m.reads[0] = VecDeque::from(vec![Read::Data(1); 65]);
                    m.reads[0].push_back(Read::Eof);
                    m.tick = 200;
                }
                6 => m.status = None,
                _ => {
                    m.reads[0] = VecDeque::from([Read::Fail]);
                    m.status = None;
                }
            }
            let c = capture(&mut m);
            assert!(
                c.termination == Term::Incomplete && c.query() == Query::Unknown,
                "transport-failure-refuses"
            );
            assert!(
                m.kills == usize::from(fault != 0),
                "cleanup-only-after-spawn"
            );
            assert!(m.waits <= 1102, "bounded-reap-and-wait");
            assert!(m.time <= 14_200, "bounded-deadline-and-cleanup");
            if fault == 4 {
                assert!(!c.complete[1], "exit-not-eof");
            }
        }
    }
    #[test]
    fn pure_capture_eof_before_exit_and_wouldblock_fairness() {
        let mut m = Mock {
            exit_after: 3,
            ..Default::default()
        };
        m.reads[0] = VecDeque::from([Read::Block, Read::Eof]);
        let c = capture(&mut m);
        assert!(
            c.complete == [true; 2] && c.termination == Term::Exited(113),
            "eof-waits-for-status"
        );
        assert!(
            m.touches == [2, 1] && m.waits == 4 && m.kills == 0,
            "wouldblock-does-not-skip-stderr"
        );
    }
}

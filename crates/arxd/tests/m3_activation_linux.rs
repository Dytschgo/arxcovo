#![cfg(all(test, target_os = "linux"))]

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

const OWNER: u32 = 21901;
const DAEMON: u32 = 21902;
const AGENT: u32 = 21903;
const BUILD: u32 = 21904;
const PUBLISH: &str = "cluster::consensus::bound_start::runtime::tests::publish_only_probe::c13_final_publication_only";
const WAIT: Duration = Duration::from_secs(16);
const CAMPAIGN: Duration = Duration::from_secs(102);
const CLEANUP: Duration = Duration::from_secs(17);
static END: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static HELPER_UNCONFIRMED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn phase_end() -> Instant {
    (Instant::now() + WAIT).min(*END.get().expect("C13 campaign deadline"))
}

struct OwnedChild {
    process: Child,
    stop_sent: bool,
}

impl OwnedChild {
    fn new(process: Child) -> Self {
        Self {
            process,
            stop_sent: false,
        }
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        let deadline = phase_end();
        loop {
            if let Some(status) = self.process.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            sleep(Duration::from_millis(20));
        }
    }

    fn interrupt(&mut self) -> io::Result<()> {
        if !self.stop_sent && self.process.try_wait()?.is_none() {
            self.stop_sent = true;
            // SAFETY: a still-owned, unreaped subprocess keeps its PID.
            if unsafe { libc::kill(self.process.id() as i32, libc::SIGINT) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

struct Helper(OwnedChild);

impl Drop for Helper {
    fn drop(&mut self) {
        if !matches!(self.0.process.try_wait(), Ok(Some(_))) {
            let _ = self.0.interrupt();
            HELPER_UNCONFIRMED.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

#[derive(Default)]
struct Campaign {
    processes: Vec<(Node, OwnedChild)>,
    closes: usize,
    confirmed: Vec<bool>,
}

impl Campaign {
    fn start(&mut self, node: &Node, binary: &Path, intent: &str) -> usize {
        assert!(Instant::now() < phase_end(), "C13 campaign budget expired");
        self.processes
            .push((node.clone(), node.start(binary, intent)));
        self.confirmed.push(false);
        self.processes.len() - 1
    }

    fn ready(&mut self, index: usize, conditions: &[u8]) {
        let (node, job) = &mut self.processes[index];
        node.ready(job, conditions);
    }

    fn close(&mut self, index: usize) {
        let (node, job) = &mut self.processes[index];
        job.interrupt().expect("C13 graceful SIGINT");
        assert!(job.stop_sent && job.wait().expect("C13 daemon reap").success());
        assert!(node.closed(), "C13 listener or lock close unconfirmed");
        self.confirmed[index] = true;
        self.closes += 1;
    }
}

impl Drop for Campaign {
    fn drop(&mut self) {
        // Signal every remaining worker first. Observe them independently in one
        // common reserve. A timed-out explicit close never sends a second signal.
        let mut observed = self
            .confirmed
            .iter()
            .map(|clean| clean.then_some(true))
            .collect::<Vec<_>>();
        for (_, job) in &mut self.processes {
            let _ = job.interrupt();
        }
        let end = (Instant::now() + CLEANUP).min(*END.get().unwrap() + CLEANUP);
        loop {
            for (disposition, (node, job)) in observed.iter_mut().zip(&mut self.processes) {
                if disposition.is_none()
                    && let Ok(Some(status)) = job.process.try_wait()
                {
                    *disposition = Some(status.success() && job.stop_sent && node.closed());
                }
            }
            if observed.iter().all(Option::is_some) || Instant::now() >= end {
                break;
            }
            sleep(Duration::from_millis(20));
        }
        let joined = observed.iter().flatten().count();
        let clean = observed.iter().filter(|x| **x == Some(true)).count();
        let helper_unknown =
            usize::from(HELPER_UNCONFIRMED.load(std::sync::atomic::Ordering::SeqCst));
        println!(
            "C13 cleanup spawned={} signaled={} reaped={joined} clean={clean} helper_unknown={helper_unknown}",
            observed.len(),
            self.processes
                .iter()
                .filter(|(_, job)| job.stop_sent)
                .count(),
        );
    }
}

struct Secret(Vec<u8>);
impl Drop for Secret {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: the byte is exclusively borrowed for this write.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

fn transition(uid: u32) -> io::Result<()> {
    // SAFETY: each call takes fixed scalar values or a null pointer with length zero.
    // The owner role gets the one named group as its primary group.
    if unsafe {
        libc::setgroups(0, std::ptr::null()) == 0
            && libc::setresgid(uid, uid, uid) == 0
            && libc::setresuid(uid, uid, uid) == 0
    } {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(Clone)]
struct Node {
    root: PathBuf,
    peer: std::net::SocketAddr,
    enrollment: std::net::SocketAddr,
}

impl Node {
    fn socket(&self) -> PathBuf {
        self.root.join("control/owner.sock")
    }

    fn config(&self) -> PathBuf {
        self.root.join("install.json")
    }

    fn record(&self) -> Vec<u8> {
        fs::read(self.root.join("credentials/identity.json")).expect("published identity")
    }

    fn configure(&self, issuer: Option<std::net::SocketAddr>) {
        let mut config = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(self.config())
            .unwrap();
        serde_json::to_writer(
            &mut config,
            &serde_json::json!({
                "workspace": self.root.join("workspace"),
                "state": self.root.join("state"),
                "credentials": self.root.join("credentials"),
                "wrap": self.root.join("wrap"),
                "custody": self.root.join("private/custody"),
                "history": self.root.join("history"),
                "owner_socket": self.socket(),
                "peer": self.peer.to_string(),
                "enrollment": self.enrollment.to_string(),
                "issuer_peer": issuer.map(|ip| ip.to_string()),
                "founder_cluster": issuer.is_none().then_some([13u8; 16]),
                "owner_uid": OWNER, "daemon_uid": DAEMON, "daemon_gid": DAEMON,
                "agent_uid": AGENT, "build_uid": BUILD, "owner_gid": OWNER,
            }),
        )
        .unwrap();
        config.sync_all().unwrap();
    }

    fn start(&self, binary: &Path, intent: &str) -> OwnedChild {
        let mut command = Command::new(binary);
        command
            .args(["--cluster-config"])
            .arg(self.config())
            .args(["--cluster-intent", intent])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        // SAFETY: transition uses only async-signal-safe system calls.
        unsafe {
            command.pre_exec(|| transition(DAEMON));
        }
        OwnedChild::new(command.spawn().expect("C13 daemon spawn"))
    }

    fn observe(&self, job: &OwnedChild) -> Option<[u8; 3]> {
        const QUERY: &str = r#"
import socket, struct, sys
try:
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(.5)
        s.connect(sys.argv[1])
        pid, uid, _ = struct.unpack('3i', s.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        if pid != int(sys.argv[2]) or uid != 21902:
            sys.exit(2)
        s.sendall(b'ARXO\x04\x00\x00')
        s.shutdown(socket.SHUT_WR)
        out = s.makefile('rb').read(11)
        if len(out) != 10 or out[:7] != b'ARXO\x0f\x00\x03':
            sys.exit(2)
        sys.stdout.buffer.write(out[7:])
except OSError:
    sys.exit(2)
"#;
        let mut command = Command::new("/usr/bin/python3");
        command
            .args(["-I", "-B", "-c", QUERY])
            .arg(self.socket())
            .arg(job.process.id().to_string())
            .env_clear()
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: only the fixed UID/GID transition runs before exec.
        unsafe {
            command.pre_exec(|| transition(OWNER));
        }
        let result = command.output().expect("C13 authenticated status client");
        result
            .status
            .success()
            .then(|| result.stdout.try_into().ok())
            .flatten()
    }

    fn status(&self, job: &OwnedChild, accepted: &[u8]) -> bool {
        self.observe(job).is_some_and(|[phase, ..]| {
            assert_ne!(phase, 6, "C13 daemon reported its first refusal");
            accepted.contains(&phase)
        })
    }

    fn ready(&self, job: &mut OwnedChild, accepted: &[u8]) {
        let end = phase_end();
        while !self.status(job, accepted) {
            assert!(
                job.process.try_wait().expect("C13 child status").is_none(),
                "C13 process failed before requested readiness"
            );
            assert!(Instant::now() < end, "C13 authenticated readiness elapsed");
            sleep(Duration::from_millis(40));
        }
    }

    fn closed(&self) -> bool {
        !self.socket().exists()
            && [
                "state/cluster-owner.lock",
                "state/cluster-state.lock",
                "state/cluster-log.lock",
                "private/custody/custody-lock",
            ]
            .into_iter()
            .all(|file| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(self.root.join(file))
                    .is_ok_and(|lock| lock.try_lock().is_ok())
            })
            && [self.peer, self.enrollment]
                .into_iter()
                .all(|address| std::net::TcpListener::bind(address).is_ok())
    }
}

fn pty() -> (File, File) {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: both descriptors are valid out pointers and null optional values are supported.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    // SAFETY: successful openpty returned two newly owned descriptors.
    (unsafe { File::from_raw_fd(master) }, unsafe {
        File::from_raw_fd(slave)
    })
}

fn owner_command(
    binary: &Path,
    node: &Node,
    uid: u32,
    input: Option<&Secret>,
) -> (ExitStatus, Secret) {
    let (mut master, slave) = pty();
    let fd = slave.as_raw_fd();
    let mut command = Command::new(binary);
    command
        .args([
            "service",
            "cluster-control",
            if input.is_some() { "join" } else { "mint" },
            "--socket",
        ])
        .arg(node.socket())
        .args(["--daemon-uid", "21902"])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: FD is held by the parent through exec. The child uses only fixed syscalls.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 || libc::ioctl(fd, libc::TIOCSCTTY, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            libc::close(fd);
            transition(uid)
        });
    }
    let mut child = Helper(OwnedChild::new(
        command.spawn().expect("C13 actual terminal owner command"),
    ));
    let deadline = phase_end();
    if let Some(paste) = input {
        loop {
            let mut term = std::mem::MaybeUninit::<libc::termios>::uninit();
            // SAFETY: the master is owned and tcgetattr writes term on success.
            let set = unsafe {
                libc::tcgetattr(master.as_raw_fd(), term.as_mut_ptr()) == 0
                    && term.assume_init().c_lflag & libc::ECHO == 0
            };
            if set {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "C13 terminal never protected input"
            );
            sleep(Duration::from_millis(10));
        }
        master
            .write_all(&paste.0)
            .expect("protected terminal paste");
        master.write_all(b"\r").expect("protected terminal submit");
    }
    let status = child.0.wait().expect("C13 terminal child reap");
    drop(slave);
    let mut output = Secret(Vec::new());
    if input.is_none() && status.success() {
        let mut chunk = [0u8; 1024];
        while output.0.len() < 8_801 {
            match master.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => output.0.extend_from_slice(&chunk[..n]),
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(_) => panic!("C13 protected terminal output failure"),
            }
        }
        while output.0.last().is_some_and(|b| matches!(b, b'\n' | b'\r')) {
            output.0.pop();
        }
        assert!(
            output.0.starts_with(b"arxje1-") && output.0.len() <= 8_800,
            "C13 terminal paste was not bounded"
        );
    }
    (status, output)
}

fn publish_final(probe: &Path, node: &Node, paste: &Secret) {
    let (read, mut write) = io::pipe().expect("one-use publication pipe");
    let mut command = Command::new(probe);
    command
        .args(["--exact", PUBLISH, "--ignored", "--nocapture"])
        .env_clear()
        .env("ARX_C13_PUBLISH", "1")
        .env("ARX_C13_CONFIG", node.config())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    // SAFETY: read is the owned private pipe. The child closes unrelated handles.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(read.as_raw_fd(), 3) == -1
                || libc::fcntl(3, libc::F_SETFD, 0) == -1
                || libc::syscall(
                    libc::SYS_close_range,
                    4u32,
                    u32::MAX,
                    libc::CLOSE_RANGE_CLOEXEC,
                ) == -1
            {
                return Err(io::Error::last_os_error());
            }
            transition(DAEMON)
        });
    }
    let mut child = Helper(OwnedChild::new(
        command.spawn().expect("C13 fixed publication helper"),
    ));
    drop(command);
    write
        .write_all(&paste.0)
        .expect("C13 one-use paste transfer");
    drop(write);
    assert!(
        child
            .0
            .wait()
            .expect("C13 publication helper reap")
            .success()
    );
    assert!(!node.socket().exists() && !node.root.join("state/cluster-state.sqlite3").exists());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Position {
    term: u64,
    leader: u64,
    index: u64,
}

impl Position {
    fn decode(bytes: &[u8]) -> Self {
        let word = |i| u64::from_be_bytes(bytes[i..i + 8].try_into().unwrap());
        Self {
            term: word(0),
            leader: word(8),
            index: word(16),
        }
    }

    fn covers(self, previous: Self) -> bool {
        self.index > previous.index || self == previous
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Membership {
    position: Position,
    voters: Vec<Vec<u64>>,
    nodes: Vec<(u64, String)>,
}

fn membership(position: Position, bytes: &[u8]) -> Membership {
    fn take<'a>(bytes: &mut &'a [u8], count: usize) -> &'a [u8] {
        let (head, tail) = bytes.split_at(count);
        *bytes = tail;
        head
    }
    fn word(bytes: &mut &[u8]) -> u64 {
        u64::from_be_bytes(take(bytes, 8).try_into().unwrap())
    }
    let mut bytes = bytes;
    assert_eq!(take(&mut bytes, 1), [1]);
    let voters = (0..take(&mut bytes, 1)[0])
        .map(|_| {
            (0..take(&mut bytes, 1)[0])
                .map(|_| word(&mut bytes))
                .collect()
        })
        .collect();
    let nodes = (0..take(&mut bytes, 1)[0])
        .map(|_| {
            let id = word(&mut bytes);
            let size = usize::from(take(&mut bytes, 1)[0]);
            let address = std::str::from_utf8(take(&mut bytes, size)).unwrap();
            (id, address.to_owned())
        })
        .collect();
    assert!(bytes.is_empty());
    Membership {
        position,
        voters,
        nodes,
    }
}

struct Inventory {
    counts: [usize; 7],
    membership: Vec<Membership>,
    redeems: Vec<(Position, [u8; 32])>,
}

fn checked_blob<'a>(bytes: &'a [u8], header: &[u8], max: usize) -> &'a [u8] {
    use sha2::{Digest, Sha256};
    assert!((header.len() + 32..=max).contains(&bytes.len()) && bytes.starts_with(header));
    let (body, hash) = bytes.split_at(bytes.len() - 32);
    assert!(Sha256::digest(body).as_slice() == hash);
    body
}

fn open_database(path: PathBuf) -> rusqlite::Connection {
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("C13 observed store");
    db.busy_timeout(Duration::from_millis(200)).unwrap();
    db
}

fn inventory(node: &Node) -> Inventory {
    let db = open_database(node.root.join("state/cluster-log.sqlite3"));
    let mut query = db
        .prepare("SELECT idx, body FROM log ORDER BY idx LIMIT 65")
        .unwrap();
    let mut rows = query.query([]).unwrap();
    let mut seen = Inventory {
        counts: [0; 7],
        membership: vec![],
        redeems: vec![],
    };
    while let Some(row) = rows.next().unwrap() {
        let idx: u64 = row.get(0).unwrap();
        let blob: Vec<u8> = row.get(1).unwrap();
        let body = checked_blob(&blob, b"ARXLE2\0\0", 4163);
        let pos = Position::decode(&body[8..32]);
        assert_eq!(pos.index, idx);
        let tag = usize::from(body[32]);
        seen.counts[tag] += 1;
        if tag == 3 {
            assert_eq!(
                usize::from(u16::from_be_bytes(body[33..35].try_into().unwrap())),
                body.len() - 35
            );
            seen.membership.push(membership(pos, &body[35..]));
        } else if tag == 6 {
            seen.redeems.push((pos, body[113..145].try_into().unwrap()));
        }
    }
    assert!(seen.counts.iter().sum::<usize>() < 65);
    seen
}

fn applied(node: &Node) -> (Position, Membership) {
    let db = open_database(node.root.join("state/cluster-state.sqlite3"));
    let blob: Vec<u8> = db
        .query_row("SELECT snapshot FROM sm WHERE singleton=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let body = checked_blob(&blob, b"ARXSM1\0\0\0\x03\x01", 16384);
    let applied = Position::decode(&body[11..35]);
    assert_eq!(body[35], 1);
    let pos = Position::decode(&body[36..60]);
    let size = usize::from(u16::from_be_bytes(body[60..62].try_into().unwrap()));
    assert!(applied.covers(pos));
    (applied, membership(pos, &body[62..62 + size]))
}

fn assert_membership(node: &Node, expected: &[Membership]) {
    assert_eq!(inventory(node).membership, expected);
    assert_eq!(&applied(node).1, expected.last().unwrap());
}

fn publication(node: &Node, id: u64, issuer: u64) -> (Position, [u8; 32]) {
    let record: serde_json::Value = serde_json::from_slice(&node.record()).unwrap();
    assert_eq!(record["node"], id);
    let cluster = [13u8; 16];
    assert_eq!(record["cluster"], serde_json::json!(cluster));
    let origin = &record["origin"];
    assert_eq!(origin["role"], "joiner");
    assert_eq!(origin["leader"], issuer);
    let key = serde_json::from_value(record["key"].clone()).unwrap();
    (
        Position {
            term: origin["term"].as_u64().unwrap(),
            leader: issuer,
            index: origin["index"].as_u64().unwrap(),
        },
        key,
    )
}

fn role_probe(uid: u32, operation: &str, path: &Path, allowed: bool) {
    const PROBE: &str = r#"
import socket, sys
try:
    if sys.argv[1] == 'owner':
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(.5)
            s.connect(sys.argv[2])
    else:
        with open(sys.argv[2], 'rb') as f:
            if not f.read(1):
                sys.exit(3)
except PermissionError:
    sys.exit(2)
except OSError:
    sys.exit(3)
"#;
    let mut command = Command::new("/usr/bin/python3");
    command
        .args(["-I", "-B", "-c", PROBE, operation])
        .arg(path)
        .env_clear();
    // SAFETY: only the fixed fixture identity transition runs before exec.
    unsafe {
        command.pre_exec(move || transition(uid));
    }
    assert_eq!(
        command.status().unwrap().code(),
        Some(if allowed { 0 } else { 2 })
    );
}

fn without_ca(node: &Node) {
    assert!(!node.root.join("private/voter-custody").exists());
    assert!(!node.root.join("private/custody/ca-key").exists());
    assert!(!node.root.join("history/voter").exists());
}

#[test]
#[ignore = "once-only hosted C13 launcher with disposable runner identities"]
fn c13_activation_restarts_original_nodes_and_issues_after_founder_close() {
    assert_eq!(std::env::var("ARX_C13_HOSTED").as_deref(), Ok("1"));
    // SAFETY: this samples the process UID for the launcher precondition.
    assert_eq!(unsafe { libc::geteuid() }, 0);
    let base = PathBuf::from(std::env::var_os("ARX_C13_BASE").expect("staged launcher root"));
    let binary = base.join("arxd");
    let probe = base.join("arxd-publication");
    let ip: std::net::IpAddr = std::env::var("ARX_C13_IP").unwrap().parse().unwrap();
    let nodes = ["a", "b", "c", "d"].map(|name| {
        let accept = |port| std::net::TcpListener::bind((ip, port)).expect("C13 fixture port");
        let peer = accept(0);
        let enrollment = accept(0);
        Node {
            root: base.join(name),
            peer: peer.local_addr().unwrap(),
            enrollment: enrollment.local_addr().unwrap(),
        }
    });
    let [a, b, c, d] = &nodes;
    for (node, issuer) in [
        (a, None),
        (b, Some(a.peer)),
        (c, Some(a.peer)),
        (d, Some(a.peer)),
    ] {
        node.configure(issuer);
        assert_eq!(
            fs::metadata(node.config()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    END.set(Instant::now() + CAMPAIGN).unwrap();
    let mut campaign = Campaign::default();
    let founder = campaign.start(a, &binary, "create-founder");
    campaign.ready(founder, &[4]);
    assert_eq!(inventory(a).counts[1], 0);
    role_probe(OWNER, "owner", &a.socket(), true);
    for uid in [AGENT, BUILD] {
        role_probe(uid, "owner", &a.socket(), false);
        let (denied, _) = owner_command(&binary, a, uid, None);
        assert_eq!(denied.code(), Some(6), "distinct role reached owner Mint");
    }
    assert_eq!(inventory(a).counts[1], 0);
    let mut processes = Vec::with_capacity(2);
    for node in [b, c] {
        let joiner = campaign.start(node, &binary, "join");
        campaign.ready(joiner, &[1]);
        assert!(!node.root.join("state/cluster-state.sqlite3").exists());
        without_ca(node);
        let (mint, paste) = owner_command(&binary, a, OWNER, None);
        assert!(mint.success(), "C13 real owner Mint refused");
        let (join, _) = owner_command(&binary, node, OWNER, Some(&paste));
        assert!(join.success(), "C13 real join or self-admission refused");
        campaign.ready(joiner, &[3]);
        if processes.is_empty() {
            without_ca(node);
        }
        processes.push(joiner);
    }
    campaign.ready(founder, &[5]);
    let [first, second]: [usize; 2] = processes.try_into().ok().unwrap();
    let original = [a.record(), b.record(), c.record()];
    let original_root = fs::read(a.root.join("credentials/ca.der")).unwrap();
    let [_, anchors_before, redeems_before] = a.observe(&campaign.processes[founder].1).unwrap();
    assert_eq!([anchors_before, redeems_before], [2, 2]);
    let transfers = [b, c]
        .into_iter()
        .map(|node| {
            for artifact in [
                "history/voter",
                "history/strict",
                "private/voter-custody/ca-key",
            ] {
                let path = node.root.join(artifact);
                role_probe(BUILD, "read", &path, false);
                role_probe(DAEMON, "read", &path, true);
            }
            assert!(fs::read(node.root.join("credentials/ca.der")).unwrap() == original_root);
            1
        })
        .sum::<usize>();
    let before = inventory(a);
    assert_eq!(before.counts[1..], [2, 0, 5, 1, 0, 2]);
    assert_eq!(before.redeems, [publication(b, 2, 1), publication(c, 3, 1)]);
    let configurations = [
        vec![vec![1]],
        vec![vec![1]],
        vec![vec![1]],
        vec![vec![1], vec![1, 2, 3]],
        vec![vec![1, 2, 3]],
    ];
    for (n, (actual, voters)) in before.membership.iter().zip(configurations).enumerate() {
        assert_eq!(actual.voters, voters);
        assert_eq!(
            actual.nodes,
            [a, b, c]
                .into_iter()
                .take((n + 1).min(3))
                .enumerate()
                .map(|(i, node)| ((i + 1) as u64, node.peer.to_string()))
                .collect::<Vec<_>>()
        );
        if n > 0 {
            assert!(actual.position.index > before.membership[n - 1].position.index);
        }
    }
    for node in [a, b, c] {
        assert_membership(node, &before.membership);
    }
    campaign.close(founder);
    let founder = campaign.start(a, &binary, "recover-founder");
    campaign.ready(founder, &[3, 4, 5]);
    assert_membership(a, &before.membership);
    campaign.close(first);
    let first = campaign.start(b, &binary, "recover-joiner");
    campaign.ready(first, &[3, 4, 5]);
    assert_membership(b, &before.membership);
    campaign.close(second);
    let second = campaign.start(c, &binary, "recover-joiner");
    campaign.ready(second, &[3, 4, 5]);
    assert_membership(c, &before.membership);
    assert!(original == [a.record(), b.record(), c.record()]);
    campaign.close(founder);
    let end = phase_end();
    let survivor = loop {
        if b.status(&campaign.processes[first].1, &[4, 5]) {
            break b;
        }
        if c.status(&campaign.processes[second].1, &[4, 5]) {
            break c;
        }
        assert!([first, second].into_iter().all(|index| {
            campaign.processes[index]
                .1
                .process
                .try_wait()
                .unwrap()
                .is_none()
        }));
        assert!(
            Instant::now() < end,
            "C13 surviving leader did not establish"
        );
        sleep(Duration::from_millis(40));
    };
    let (mint, paste) = owner_command(&binary, survivor, OWNER, None);
    assert!(mint.success(), "C13 survivor Mint refused");
    publish_final(&probe, d, &paste);
    assert!(fs::read(d.root.join("credentials/ca.der")).unwrap() == original_root);
    let issuer_id = if survivor.root == b.root { 2 } else { 3 };
    let final_position = publication(d, 4, issuer_id);
    let survivor_job = &campaign.processes[if issuer_id == 2 { first } else { second }].1;
    let [_, anchors_after, redeems_after] = survivor.observe(survivor_job).unwrap();
    assert_eq!([anchors_after, redeems_after], [1, 1]);
    campaign.close(first);
    campaign.close(second);
    let after = inventory(survivor);
    assert_eq!(
        after.membership, before.membership,
        "membership writes after restart"
    );
    assert_eq!(after.counts[1..], [3, 0, 5, 1, 0, 3]);
    assert_eq!(after.redeems[..2], before.redeems);
    assert_eq!(after.redeems[2], final_position);
    assert!(applied(survivor).0.covers(final_position.0));
    let additions = after
        .membership
        .windows(2)
        .filter(|w| w[1].nodes.len() > w[0].nodes.len())
        .count();
    let joints = after
        .membership
        .iter()
        .filter(|m| m.voters.len() == 2)
        .count();
    let flattens = after
        .membership
        .windows(2)
        .filter(|w| w[0].voters.len() == 2 && w[1].voters.len() == 1)
        .count();
    let [_, mints, _, _, found, removes, redeems] = after.counts;
    assert_eq!(
        (campaign.processes.len(), campaign.closes, transfers),
        (6, 6, 2)
    );
    println!(
        "C13 starts={} closes={} Found={found} Mint={mints} AnchorQuery={} Redeem={} admissions={} AddNodes={additions} joint={joints} flatten={flattens} transfer={transfers} enrollment_proposals={}",
        campaign.processes.len(),
        campaign.closes,
        anchors_before + anchors_after,
        redeems_before + redeems_after,
        processes_admitted(&before, [b, c]),
        mints + found + removes + redeems,
    );
}

fn processes_admitted(before: &Inventory, nodes: [&Node; 2]) -> usize {
    // Each retained Join returned only after its single applied self-admission.
    // Its distinct member/endpoint also appears in the corresponding AddNodes.
    nodes
        .into_iter()
        .zip(&before.membership[1..3])
        .enumerate()
        .filter(|(i, (node, membership))| {
            membership.nodes.last() == Some(&((i + 2) as u64, node.peer.to_string()))
        })
        .count()
}

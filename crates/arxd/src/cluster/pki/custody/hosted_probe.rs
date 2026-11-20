//! Fixed hosted qualification protocol; never compiled into a production executable.
use super::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
    time::{Duration, Instant},
};

const NOW: u64 = 1_000_000;
const MODES: [&str; 8] = [
    "hold",
    "create-founder",
    "create-learner",
    "deny-build",
    "same-uid-control",
    "release",
    "restart-founder",
    "restart-learner",
];

fn bounded(path: &str, max: usize) -> Vec<u8> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .unwrap();
    let mut bytes = Vec::new();
    f.take((max + 1) as u64).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= max);
    bytes
}
fn field(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| {
        assert!(v.len() <= 1024 && v.is_ascii());
        v
    })
}
fn public() -> Vec<u8> {
    let v = hex::decode(field("ARX_CUSTODY_PUBLIC_CONTEXT").unwrap()).unwrap();
    assert_eq!(v.len(), 128);
    v
}
fn contexts(v: &[u8]) -> (Context, Context) {
    assert_eq!(v.len(), 128);
    let member = Context {
        kind: Kind::Member,
        install: v[..16].try_into().unwrap(),
        cluster: v[16..32].try_into().unwrap(),
        root: v[32..64].try_into().unwrap(),
        spki: v[64..96].try_into().unwrap(),
    };
    (
        Context {
            kind: Kind::Ca,
            spki: v[96..128].try_into().unwrap(),
            ..member
        },
        member,
    )
}
fn pack(member: Context, ca: [u8; 32]) -> Vec<u8> {
    [
        member.install.as_slice(),
        member.cluster.as_slice(),
        member.root.as_slice(),
        member.spki.as_slice(),
        ca.as_slice(),
    ]
    .concat()
}
fn sign(ca: &ClusterCa, member: &MemberKey) {
    let csr = ProvenCsr::parse(&member.csr().unwrap()).unwrap();
    let ticket = crate::cluster::consensus::issue::test_ticket(csr.digest());
    let leaf = ca.sign_leaf(&ticket, &csr, NOW).unwrap();
    member
        .check_leaf(ticket.node(), ca.cert_der(), &leaf, Some(NOW))
        .unwrap();
}
fn errno_denied(result: libc::c_long) {
    assert_eq!(result, -1, "unexpected access");
    let e = std::io::Error::last_os_error().raw_os_error().unwrap();
    assert!([libc::EPERM, libc::EACCES, libc::ENOENT].contains(&e));
    println!("ARX_DENIAL errno={e}");
}
fn identity(build: bool) {
    let uid = if build { 21002 } else { 21001 };
    let status = String::from_utf8(bounded("/proc/self/status", 16384)).unwrap();
    for label in ["Uid:", "Gid:"] {
        let line = status.lines().find(|s| s.starts_with(label)).unwrap();
        assert_eq!(
            line.split_whitespace()
                .skip(1)
                .map(|n| n.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            vec![uid; 4]
        );
    }
    let groups = status.lines().find(|s| s.starts_with("Groups:")).unwrap();
    assert!(
        groups
            .split_whitespace()
            .skip(1)
            .all(|g| g == uid.to_string())
    );
    for label in ["CapInh:", "CapPrm:", "CapEff:", "CapAmb:"] {
        let line = status.lines().find(|s| s.starts_with(label)).unwrap();
        assert_eq!(
            u64::from_str_radix(line.split_whitespace().nth(1).unwrap(), 16).unwrap(),
            0
        );
    }
    assert!(status.lines().any(|s| s == "NoNewPrivs:\t1"));
    let mut limit = libc::rlimit {
        rlim_cur: 1,
        rlim_max: 1,
    };
    // SAFETY: valid writable rlimit pointer.
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
    assert_eq!((limit.rlim_cur, limit.rlim_max), (0, 0));
    // SAFETY: same valid pointer.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0
    );
    assert_eq!((limit.rlim_cur, limit.rlim_max), (128, 128));
    let executable = fs::metadata("/opt/arx/arxd-tests").unwrap();
    assert_eq!(executable.uid(), 0);
    assert_eq!(executable.mode() & 0o222, 0);
    for path in [
        "/opt/arx/arxd-tests",
        "/etc/hosts",
        "/etc/hostname",
        "/etc/resolv.conf",
    ] {
        assert!(OpenOptions::new().write(true).open(path).is_err());
    }
    assert!(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open("/dev/arx-probe")
            .is_err()
    );
    let fds = fs::read_dir("/proc/self/fd").unwrap();
    let inspection_target = format!("/proc/{}/fd", std::process::id());
    for entry in fds {
        let fd: i32 = entry
            .unwrap()
            .file_name()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            fd <= 2
                || fs::read_link(format!("/proc/self/fd/{fd}")).unwrap()
                    == Path::new(&inspection_target),
            "inherited descriptor"
        );
    }
    for (path, expected) in [
        ("/sys/fs/cgroup/memory.max", "268435456"),
        ("/sys/fs/cgroup/memory.swap.max", "0"),
        ("/sys/fs/cgroup/pids.max", "64"),
        ("/sys/fs/cgroup/cpu.max", "100000 100000"),
    ] {
        assert_eq!(
            String::from_utf8(bounded(path, 128)).unwrap().trim(),
            expected
        );
    }
    for (path, allowed) in [
        ("/workspace", build),
        ("/control", !build),
        ("/wrap", !build),
        ("/store", !build),
    ] {
        let probe = format!("{path}/permission-probe");
        let result = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&probe);
        assert_eq!(result.is_ok(), allowed);
        if result.is_ok() {
            fs::remove_file(probe).unwrap();
        }
    }
    let mounts = String::from_utf8(bounded("/proc/self/mountinfo", 65536)).unwrap();
    for path in ["/wrap", "/store"] {
        let fields: Vec<_> = mounts
            .lines()
            .find(|s| s.split_whitespace().nth(4) == Some(path))
            .unwrap()
            .split_whitespace()
            .collect();
        let separator = fields.iter().position(|f| *f == "-").unwrap();
        assert_eq!(fields[separator + 1], "ext4");
        assert!(
            ["nosuid", "nodev", "noexec"]
                .iter()
                .all(|flag| fields[5].split(',').any(|f| f == *flag))
        );
        let metadata = fs::metadata(path).unwrap();
        assert_eq!(metadata.uid(), 21001);
        assert_eq!(metadata.mode() & 0o777, 0o700);
    }
    // The launcher attributes host sources; this view independently constrains writable targets.
    for line in mounts.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let target = fields[4];
        if fields[5].split(',').any(|s| s == "rw") {
            assert!(
                [
                    "/wrap",
                    "/store",
                    "/tmp",
                    "/workspace",
                    "/control",
                    "/dev",
                    "/dev/pts",
                    "/dev/shm",
                    "/dev/mqueue",
                    "/proc",
                    "/proc/interrupts",
                    "/proc/kcore",
                    "/proc/keys",
                    "/proc/latency_stats",
                    "/proc/timer_list",
                    "/etc/hosts",
                    "/etc/hostname",
                    "/etc/resolv.conf"
                ]
                .contains(&target),
                "unexpected writable mount"
            );
        }
    }
    for (path, cap) in [
        ("/tmp", 8),
        ("/workspace", 8),
        ("/control", 1),
        ("/dev/shm", 1),
        ("/dev", 64),
    ] {
        let c = std::ffi::CString::new(path).unwrap();
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: valid path and output storage, initialized only after success.
        assert_eq!(unsafe { libc::statvfs(c.as_ptr(), stat.as_mut_ptr()) }, 0);
        let stat = unsafe { stat.assume_init() };
        let capacity = stat.f_blocks * stat.f_frsize;
        assert!(capacity <= cap * 1024 * 1024);
        if path != "/dev" {
            assert_eq!(capacity, cap * 1024 * 1024);
        }
        println!(
            "ARX_MOUNT slot={path} capacity={capacity} used={}",
            (stat.f_blocks - stat.f_bfree) * stat.f_frsize
        );
        if path == "/dev/shm" {
            let line = mounts
                .lines()
                .find(|s| s.split_whitespace().nth(4) == Some(path))
                .unwrap();
            for flag in ["nosuid", "nodev", "noexec"] {
                assert!(
                    line.split_whitespace()
                        .nth(5)
                        .unwrap()
                        .split(',')
                        .any(|f| f == flag)
                );
            }
        }
    }
    assert!(
        fs::read_dir("/sys/class/net")
            .unwrap()
            .all(|e| e.unwrap().file_name() == "lo")
    );
    for address in [
        "192.0.2.1:9",
        "169.254.169.254:80",
        "172.17.0.1:9",
        "[2001:db8::1]:9",
    ] {
        assert!(
            std::net::TcpStream::connect_timeout(
                &address.parse().unwrap(),
                Duration::from_millis(50)
            )
            .is_err()
        );
    }
    println!("ARX_FACTS uid={uid} caps=0 nnp=1 core=0");
}

#[test]
fn hosted_custody_probe() {
    let Some(mode) = field("ARX_CUSTODY_HOSTED_MODE") else {
        return;
    };
    assert!(MODES.contains(&mode.as_str()));
    for (k, _) in std::env::vars() {
        if k.starts_with("ARX_CUSTODY_") {
            assert!(
                [
                    "ARX_CUSTODY_HOSTED_MODE",
                    "ARX_CUSTODY_TARGET_PID",
                    "ARX_CUSTODY_TARGET_FDS",
                    "ARX_CUSTODY_PUBLIC_CONTEXT"
                ]
                .contains(&k.as_str())
            );
        }
    }
    let started = Instant::now();
    if mode == "hold" {
        std::thread::sleep(Duration::from_secs(130));
        return;
    }
    if mode != "deny-build" {
        // SAFETY: fixed process-local controls before any private file access.
        unsafe {
            libc::umask(0o077);
            assert_eq!(libc::prctl(libc::PR_SET_DUMPABLE, 0), 0);
            assert_eq!(libc::prctl(libc::PR_GET_DUMPABLE), 0);
        }
    }
    identity(mode == "deny-build");
    if mode == "release" {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open("/control/release")
            .unwrap()
            .write_all(b"1")
            .unwrap();
    } else if mode == "deny-build" {
        let pid: i32 = field("ARX_CUSTODY_TARGET_PID").unwrap().parse().unwrap();
        assert!((2..=4194304).contains(&pid));
        let fds: Vec<i32> = field("ARX_CUSTODY_TARGET_FDS")
            .unwrap()
            .split(',')
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(fds.len(), 3);
        assert!(fds.iter().all(|fd| (3..=127).contains(fd)));
        fs::write("/workspace/positive", b"probe").unwrap();
        assert_eq!(fs::read("/workspace/positive").unwrap(), b"probe");
        for path in [
            "/wrap",
            "/store",
            "/wrap/seal-key",
            "/wrap/install-context",
            "/store/custody-lock",
            "/store/ca-key",
            "/store/member-key",
            "/opt/arx/arxd-tests",
        ] {
            assert!(OpenOptions::new().write(true).open(path).is_err());
            if path != "/opt/arx/arxd-tests" {
                assert!(File::open(path).is_err());
            }
        }
        assert!(fs::read_dir(format!("/proc/{pid}/fd")).is_err());
        assert!(File::open(format!("/proc/{pid}/mem")).is_err());
        for fd in fds {
            let path = format!("/proc/{pid}/fd/{fd}");
            assert!(fs::read_link(&path).is_err());
            assert!(File::open(path).is_err());
        }
        let mut byte = 0u8;
        let local = libc::iovec {
            iov_base: (&mut byte as *mut u8).cast(),
            iov_len: 1,
        };
        let remote = libc::iovec {
            iov_base: std::ptr::dangling_mut(),
            iov_len: 1,
        };
        // SAFETY: local buffer is live; remote pointer belongs to target and is never dereferenced locally.
        errno_denied(unsafe { libc::process_vm_readv(pid, &local, 1, &remote, 1, 0) });
        // SAFETY: fixed non-mutating attach attempt must be refused by kernel policy.
        errno_denied(unsafe {
            libc::ptrace(
                libc::PTRACE_ATTACH,
                pid,
                std::ptr::null_mut::<libc::c_void>(),
                std::ptr::null_mut::<libc::c_void>(),
            )
        });
        // SAFETY: fixed target PID; zero and SIGCONT cannot terminate the target.
        unsafe {
            errno_denied(libc::kill(pid, 0).into());
            errno_denied(libc::kill(pid, libc::SIGCONT).into());
            errno_denied(libc::setresuid(21001, 21001, 21001).into());
            errno_denied(libc::setresgid(21001, 21001, 21001).into());
        }
    } else if mode == "same-uid-control" {
        let (ca, member) = contexts(&public());
        let key = codec::Secret(bounded("/wrap/seal-key", 32));
        let key = codec::wrapping(&key.0).unwrap();
        for (path, ctx) in [("/store/ca-key", ca), ("/store/member-key", member)] {
            assert!(codec::open(&key, ctx, &bounded(path, 16384)).is_ok());
        }
    } else {
        // SAFETY: fixed process-local privacy controls, applied before secrets are opened.
        unsafe {
            libc::umask(0o077);
            assert_eq!(libc::prctl(libc::PR_SET_DUMPABLE, 0), 0);
            assert_eq!(libc::prctl(libc::PR_GET_DUMPABLE), 0);
        }
        let learner = mode.ends_with("learner");
        let create = mode.starts_with("create-");
        let wrap = Path::new("/wrap");
        let store = Path::new("/store");
        if create {
            let mut owner = linux::Custody::create_fresh_installation(wrap, store).unwrap();
            let ca = if learner {
                None
            } else {
                Some(owner.create_founder_ca([1; 16], Some(NOW)).unwrap())
            };
            let root = ca
                .as_ref()
                .map_or_else(|| contexts(&public()).1.root, |c| c.fingerprint());
            let (member, _) = owner.create_member([1; 16], root, learner).unwrap();
            if let Some(ca) = &ca {
                sign(ca, &member);
            }
            let context = pack(
                member.context,
                ca.as_ref().map_or([0; 32], |c| c.spki_digest().unwrap()),
            );
            let files: Vec<_> = [
                "/wrap/seal-key",
                if learner {
                    "/store/custody-lock"
                } else {
                    "/store/ca-key"
                },
                "/store/member-key",
            ]
            .iter()
            .map(|p| {
                OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
                    .open(p)
                    .unwrap()
            })
            .collect();
            println!(
                "ARX_READY {} {},{},{} {}",
                std::process::id(),
                files[0].as_raw_fd(),
                files[1].as_raw_fd(),
                files[2].as_raw_fd(),
                hex::encode(context)
            );
            std::io::stdout().flush().unwrap();
            while !Path::new("/control/release").exists() {
                assert!(
                    started.elapsed() < Duration::from_secs(30),
                    "holder expired"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(bounded("/control/release", 1), b"1");
            drop((owner, member, ca, files));
        } else {
            let (ca_ctx, member_ctx) = contexts(&public());
            let owner = linux::Custody::open_existing(wrap, store, member_ctx.install).unwrap();
            let mut wrong = member_ctx;
            wrong.cluster[0] ^= 1;
            assert!(owner.load_member(wrong, learner).is_err());
            let member = owner.load_member(member_ctx, learner).unwrap();
            assert_eq!(member.context.spki, member_ctx.spki);
            ProvenCsr::parse(&member.csr().unwrap()).unwrap();
            if learner {
                assert!(!Path::new("/store/ca-key").exists());
            } else {
                let ca = owner.load_ca(ca_ctx, Some(NOW)).unwrap();
                assert_eq!(ca.fingerprint(), ca_ctx.root);
                sign(&ca, &member);
            }
        }
        if learner {
            assert!(!Path::new("/store/ca-key").exists());
        }
    }
    println!("ARX_PASS selected=1 passed=1");
}

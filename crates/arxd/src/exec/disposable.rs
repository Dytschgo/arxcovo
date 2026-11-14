//! Explicit opt-in only: the cross-account test creates/removes one OS account.
//! Never run on an owner PC, self-hosted runner, or persistent machine.
use super::*;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, chown},
    process::Command,
};

struct Fixture {
    name: String,
    base: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only our generated account and exact generated fixture directory.
        let _ = Command::new("/usr/sbin/userdel").arg(&self.name).status();
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn checked(tool: &str, args: &[&str]) -> Vec<u8> {
    let out = Command::new(tool).args(args).output().unwrap();
    assert!(out.status.success(), "fixture tool failed: {tool}");
    out.stdout
}

fn accessible_control_directory(path: &Path) -> std::io::Result<()> {
    // Start private regardless of the caller's umask, then explicitly grant
    // traversal for the generated build identity. These contain fixture data.
    fs::DirBuilder::new().mode(0o700).create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[test]
fn positive_control_directories_are_traversable_without_provisioning() {
    use std::{io::Read, os::unix::process::CommandExt, process::Stdio, time::Instant};
    const TEST: &str = "exec::disposable::restrictive_umask_control_fixture";
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            TEST,
            "--ignored",
            "--nocapture",
            "--color",
            "never",
        ])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // umask is process-global: change it only in the isolated child before exec,
    // never in the parallel test runner. This syscall is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o077);
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("isolated umask fixture timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(4097)
        .read_to_end(&mut output)
        .unwrap();
    assert!(status.success(), "isolated umask fixture failed");
    assert!(output.len() <= 4096);
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains(&format!("test {TEST} ... ok")));
    assert!(output.contains("1 passed"));
}

#[test]
#[ignore = "subprocess-only non-privileged umask fixture; exercised by parent"]
fn restrictive_umask_control_fixture() {
    assert_eq!(unsafe { libc::umask(0o077) }, 0o077);
    let base = tempfile::tempdir().unwrap();
    for name in ["outside", "plain"] {
        let path = base.path().join(name);
        accessible_control_directory(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(accessible_control_directory(&path).is_err());
    }
}

#[test]
#[ignore = "root + generated account + delegated cgroup; disposable hosted CI only"]
fn disposable_linux_cross_account() {
    for (key, value) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("ARX_DISPOSABLE_EXEC_TEST", "1"),
    ] {
        assert_eq!(
            std::env::var(key).as_deref(),
            Ok(value),
            "disposable CI gate {key}"
        );
    }
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert_eq!(unsafe { libc::getuid() }, 0);
    let delegation = std::env::var_os("ARX_EXEC_CGROUP_PARENT")
        .expect("pre-provision root-only delegated memory+pids subtree");
    let name = format!("arxexec{}", std::process::id());
    let base = PathBuf::from(format!("/var/lib/{name}"));
    // Fail if fixture/account already exists; never reuse or delete existing data.
    fs::create_dir(&base).unwrap();
    checked(
        "/usr/sbin/useradd",
        &[
            "--no-create-home",
            "--user-group",
            "--shell",
            "/usr/sbin/nologin",
            &name,
        ],
    );
    let fixture = Fixture {
        name: name.clone(),
        base: base.clone(),
    };
    let uid: u32 = String::from_utf8(checked("/usr/bin/id", &["-u", &name]))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let gid: u32 = String::from_utf8(checked("/usr/bin/id", &["-g", &name]))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // Re-scan after account creation, before any Runner::run. No delegation
    // anywhere may belong to this generated build UID/GID.
    checked(
        "/usr/bin/python3",
        &[
            "-I",
            &std::env::var("ARX_EXEC_GUARD_DRIVER").unwrap(),
            "fixture-check",
            &std::env::var("ARX_EXEC_RUN_ID").unwrap(),
        ],
    );
    let workspace = base.join("workspace");
    let private = base.join("private");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    chown(&workspace, Some(uid), Some(gid)).unwrap();
    fs::write(private.join("denied"), b"fixture-only").unwrap();
    let config = OwnerConfig {
        account: name,
        uid,
        gid,
        workspace: workspace.clone(),
        protected_directories: vec![private.clone()],
        environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
        admin: false,
        cgroup_parent: Some(delegation.into()),
        memory_bytes: 64 * 1024 * 1024,
        pids: 32,
    };
    let runner = Runner::new(config.clone()).unwrap();
    let mut spec = RunSpec {
        executable: "/usr/bin/id".into(),
        arguments: vec!["-u".into()],
        cwd: PathBuf::new(),
        timeout: Duration::from_secs(10),
        output_bytes: 1024,
    };
    let run = runner.run(&spec, &Cancellation::default()).unwrap();
    assert_eq!(
        run.identity,
        ObservedIdentity {
            uid,
            gid,
            daemon_was_root: true
        }
    );
    assert_eq!(run.stdout.head, format!("{uid}\n").as_bytes());
    assert!(run.tree_cleanup_confirmed);
    assert_eq!(run.exit_code, Some(0));
    // Actual membership and configured controls, plus enforced fork exhaustion.
    // This ignored runtime evidence is unrun until independently reviewed.
    let parent = config.cgroup_parent.clone().unwrap();
    let controls = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            for entry in fs::read_dir(&parent).unwrap() {
                let path = entry.unwrap().path();
                if path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("arx-")
                {
                    if fs::read_to_string(path.join("cgroup.procs"))
                        .unwrap()
                        .trim()
                        .is_empty()
                    {
                        continue; // wait for child join after all limits are configured
                    }
                    assert_eq!(
                        fs::read_to_string(path.join("pids.max")).unwrap().trim(),
                        "32"
                    );
                    assert_eq!(
                        fs::read_to_string(path.join("memory.max")).unwrap().trim(),
                        "67108864"
                    );
                    assert_eq!(
                        fs::read_to_string(path.join("memory.swap.max"))
                            .unwrap()
                            .trim(),
                        "0"
                    );
                    return;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "run cgroup not observed"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    spec.executable = "/usr/bin/python3".into();
    spec.arguments = vec![
        "-c".into(),
        r#"import os, errno, time
print(open('/proc/self/cgroup').read(), flush=True)
children=[]
try:
    for i in range(64):
        pid=os.fork()
        if pid == 0:
            time.sleep(1)
            os._exit(0)
        children.append(pid)
except OSError as e:
    assert e.errno == errno.EAGAIN
else:
    raise AssertionError('pids limit not enforced')
assert 0 < len(children) < 32
for pid in children:
    os.waitpid(pid, 0)
"#
        .into(),
    ];
    let run = runner.run(&spec, &Cancellation::default()).unwrap();
    controls.join().unwrap();
    assert_eq!(run.exit_code, Some(0));
    let membership = String::from_utf8(run.stdout.head).unwrap();
    let expected = config
        .cgroup_parent
        .as_ref()
        .unwrap()
        .strip_prefix("/sys/fs/cgroup")
        .unwrap();
    assert!(
        membership
            .trim()
            .starts_with(&format!("0::/{}/arx-", expected.display()))
    );
    assert!(run.tree_cleanup_confirmed);
    spec.executable = "/usr/bin/env".into();
    spec.arguments.clear();
    let run = runner.run(&spec, &Cancellation::default()).unwrap();
    assert_eq!(run.stdout.head, b"PATH=/usr/bin:/bin\n");
    assert!(run.stdout.tail.is_empty());
    spec.executable = "/bin/cat".into();
    spec.arguments = vec![private.join("denied").to_str().unwrap().into()];
    assert_ne!(
        runner
            .run(&spec, &Cancellation::default())
            .unwrap()
            .exit_code,
        Some(0)
    );
    let outside = base.join("outside");
    accessible_control_directory(&outside).unwrap();
    // Positive controls: target is traversable by the build identity, and a
    // normal workspace subdirectory succeeds with the same executable.
    spec.executable = "/bin/sh".into();
    spec.arguments = vec!["-c".into(), format!("cd {}", outside.display())];
    assert_eq!(
        runner
            .run(&spec, &Cancellation::default())
            .unwrap()
            .exit_code,
        Some(0)
    );
    accessible_control_directory(&workspace.join("plain")).unwrap();
    spec.cwd = "plain".into();
    spec.executable = "/usr/bin/true".into();
    spec.arguments.clear();
    assert_eq!(
        runner
            .run(&spec, &Cancellation::default())
            .unwrap()
            .exit_code,
        Some(0)
    );
    std::os::unix::fs::symlink(&outside, workspace.join("link")).unwrap();
    spec.cwd = "link".into();
    assert!(runner.run(&spec, &Cancellation::default()).is_err());
    // This in-workspace target also catches removal of NO_SYMLINKS alone:
    // BENEATH would otherwise allow it.
    std::os::unix::fs::symlink("plain", workspace.join("inside-link")).unwrap();
    spec.cwd = "inside-link".into();
    assert!(runner.run(&spec, &Cancellation::default()).is_err());
    spec.cwd = PathBuf::new();
    spec.executable = "/bin/sh".into();
    spec.arguments = vec!["-c".into(), "i=0; while [ $i -lt 10000 ]; do printf 0123456789; printf abcdefghij >&2; i=$((i+1)); done".into()];
    let run = runner.run(&spec, &Cancellation::default()).unwrap();
    assert_eq!(run.exit_code, Some(0));
    assert_eq!(run.stdout.dropped_bytes, 100000 - 1024);
    assert_eq!(run.stderr.dropped_bytes, 100000 - 1024);
    spec.arguments = vec!["-c".into(), "setsid /bin/sleep 60 & wait".into()];
    spec.timeout = Duration::from_millis(150);
    let run = runner.run(&spec, &Cancellation::default()).unwrap();
    assert_eq!(run.outcome, Outcome::TimedOut);
    assert!(run.tree_cleanup_confirmed);
    spec.timeout = Duration::from_secs(5);
    let cancel = Cancellation::default();
    let signal = cancel.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        signal.cancel();
    });
    let run = runner.run(&spec, &cancel).unwrap();
    thread.join().unwrap();
    assert_eq!(run.outcome, Outcome::Cancelled);
    assert!(run.tree_cleanup_confirmed);
    let signal = runner.shutdown_handle();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        signal.cancel();
    });
    let run = runner.run(&spec, &Cancellation::default()).unwrap();
    thread.join().unwrap();
    assert_eq!(run.outcome, Outcome::Shutdown);
    assert!(run.tree_cleanup_confirmed);
    let mut changed = config.clone();
    changed.uid += 1;
    assert!(
        Runner::new(changed)
            .unwrap()
            .run(&spec, &Cancellation::default())
            .is_err()
    );
    let mut absent = config;
    absent.cgroup_parent = None;
    assert!(
        Runner::new(absent)
            .unwrap()
            .run(&spec, &Cancellation::default())
            .is_err()
    );
    drop(fixture);
}

#[test]
#[ignore = "production setup + fixed packaged service; fresh disposable hosted Linux only"]
fn disposable_linux_a1_a2() {
    use hierarchy::{linux::LinuxFs, setup::Clock};
    use std::time::Instant;
    for (key, value) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("ARX_DISPOSABLE_EXEC_TEST", "1"),
    ] {
        assert_eq!(std::env::var(key).as_deref(), Ok(value));
    }
    assert_eq!(unsafe { libc::getuid() }, 0);
    assert_eq!(unsafe { libc::geteuid() }, 0);
    // The selected ignored test is the only running test; umask changes no rights.
    assert_eq!(unsafe { libc::umask(0o077) }, 0o077);
    struct Monotonic(Instant);
    impl Clock for Monotonic {
        fn now(&self) -> Duration {
            self.0.elapsed()
        }
        fn sleep(&self, duration: Duration) {
            std::thread::sleep(duration);
        }
    }
    let clock = Monotonic(Instant::now());
    assert!(!Path::new("identity-ack").exists());
    assert!(hierarchy::fake::prepare_a1a2(
        &LinuxFs::system(),
        &clock,
        &Cancellation::default()
    ));
    let socket = std::env::var_os("NOTIFY_SOCKET").unwrap();
    crate::service::unix::Notify::send_to(&socket, b"READY=1").unwrap();
    hold_identity_ack();
}

fn hold_identity_ack() {
    use std::{io::Read, os::unix::fs::OpenOptionsExt, time::Instant};
    // Outer checks MainPID/executable/placement now, then releases us. The libtest
    // summary cannot exist until after this function returns.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open("identity-ack")
        {
            Ok(file) => {
                use std::os::unix::fs::MetadataExt;
                let info = file.metadata().unwrap();
                assert!(
                    info.is_file()
                        && info.uid() == 0
                        && info.mode() & 0o777 == 0o600
                        && info.nlink() == 1
                );
                let mut bytes = Vec::new();
                file.take(2).read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"1");
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => panic!("ack refused"),
        }
        assert!(Instant::now() < deadline, "identity hold timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// Dormant A3 observations only. No setup, service mutation or Runner call.
// The driver establishes profile presence/absence and runtime attribution.
// Its preparation default selects neither of these tests.
fn assert_a3_hardening(expected: hierarchy::parse::Hardening) {
    use hierarchy::{linux::LinuxFs, setup::CgroupFs};
    for (key, value) in [
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("ARX_DISPOSABLE_EXEC_TEST", "1"),
    ] {
        assert_eq!(std::env::var(key).as_deref(), Ok(value));
    }
    assert_eq!(unsafe { libc::getuid() }, 0);
    assert_eq!(unsafe { libc::geteuid() }, 0);
    let fs = LinuxFs::system();
    let membership = fs.proc_cgroup().unwrap();
    assert_eq!(membership.magic, hierarchy::setup::PROC_MAGIC);
    assert!(hierarchy::locate(&membership.bytes).is_some());
    let status = fs.proc_status().unwrap();
    assert_eq!(status.magic, hierarchy::setup::PROC_MAGIC);
    assert_eq!(hierarchy::parse::hardening(&status.bytes), expected);
}

#[test]
#[ignore = "A3 with unchanged packaged drop-in; fresh disposable hosted Linux only; source selection required"]
fn disposable_linux_a3_standard() {
    assert!(!Path::new("identity-ack").exists());
    assert_a3_hardening(hierarchy::parse::Hardening::Standard);
    let socket = std::env::var_os("NOTIFY_SOCKET").unwrap();
    crate::service::unix::Notify::send_to(&socket, b"READY=1").unwrap();
    hold_identity_ack();
}

#[test]
#[ignore = "A3 without standard drop-in; fresh disposable hosted Linux only; source selection required"]
fn disposable_linux_a3_none() {
    assert!(!Path::new("identity-ack").exists());
    assert_a3_hardening(hierarchy::parse::Hardening::None);
    let socket = std::env::var_os("NOTIFY_SOCKET").unwrap();
    crate::service::unix::Notify::send_to(&socket, b"READY=1").unwrap();
    hold_identity_ack();
}

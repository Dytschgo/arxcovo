//! Explicit opt-in only: this test creates/removes one generated OS account.
//! Never run on an owner PC, self-hosted runner, or persistent machine.
use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, chown},
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
    std::os::unix::fs::symlink(&private, workspace.join("link")).unwrap();
    spec.cwd = "link".into();
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

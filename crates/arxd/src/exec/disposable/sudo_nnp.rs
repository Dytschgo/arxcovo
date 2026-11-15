//! SOURCE ONLY. No delivery pins or qualifying host/policy attestation exists.
use super::*;

// This is deliberately not an environment/config selector. A later separately
// reviewed delivery slice must replace this refusal with verified owned inputs.
struct Prepared {
    accounts: [OwnerConfig; 2], // rule, then no-rule; driver-owned and pinned
    arxd: PathBuf,
    reentry: PathBuf,
}
fn prepared_delivery() -> Option<Prepared> {
    None
}

#[derive(Clone, Copy)]
struct Facts {
    identity: bool,
    exited: bool,
    bounded: bool,
    records: bool,
    rights: bool,
    c1: bool,
    alias_skipped: bool,
    c3_unopened: bool,
    path: bool,
    cloexec: bool,
    regular: bool,
    exec_eof: bool,
    nonzero: bool,
    both_policy_cases: bool,
    cleanup: bool,
}
fn accepts(f: Facts) -> bool {
    f.identity
        && f.exited
        && f.bounded
        && f.records
        && f.rights
        && f.c1
        && f.alias_skipped
        && f.c3_unopened
        && f.path
        && f.cloexec
        && f.regular
        && f.exec_eof
        && f.nonzero
        && f.both_policy_cases
        && f.cleanup
}

fn output(run: &RunResult) -> Option<Vec<u8>> {
    if run.stdout.dropped_bytes != 0
        || run.stderr.dropped_bytes != 0
        || !run.stderr.head.is_empty()
        || !run.stderr.tail.is_empty()
        || run.stdout.head.len() + run.stdout.tail.len() > 4096
    {
        return None;
    }
    Some([run.stdout.head.as_slice(), run.stdout.tail.as_slice()].concat())
}
fn probe_records(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    if !text.ends_with('\n') || bytes.len() > 4096 {
        return false;
    }
    let lines: Vec<_> = text.lines().collect();
    if lines.len() != crate::development::TOOLS.len() + 1 {
        return false;
    }
    for (line, name) in lines.iter().zip(crate::development::TOOLS) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        let Some(object) = value.as_object() else {
            return false;
        };
        if object.len() != 3
            || value["name"] != *name
            || !value["found"].is_boolean()
            || !(value["version"].is_null() || value["version"].is_string())
        {
            return false;
        }
    }
    lines.last() == Some(&r#"{"rights":{"no_new_privs":true,"sudo":"exit_nonzero"}}"#)
}
fn reentry_records(bytes: &[u8]) -> bool {
    if bytes.len() > 4096 {
        return false;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let prefix = concat!(
        "\nrunning 1 test\ntest unix_shim::rights::linux::tests::e4_sudo_nnp_reentry ... ",
        "ARX_A7_A11_REENTRY_OK\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
    );
    let Some(rest) = text.strip_prefix(prefix) else {
        return false;
    };
    let Some((filtered, elapsed)) = rest.split_once(" filtered out; finished in ") else {
        return false;
    };
    let Some(elapsed) = elapsed.strip_suffix("s\n\n") else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit());
    digits(filtered)
        && elapsed
            .split_once('.')
            .is_some_and(|(a, b)| digits(a) && digits(b))
}
fn valid_run(run: &RunResult, config: &OwnerConfig, residue: bool) -> bool {
    run.identity
        == ObservedIdentity {
            uid: config.uid,
            gid: config.gid,
            daemon_was_root: true,
        }
        && run.outcome == Outcome::Exited
        && run.exit_code == Some(0)
        && run.tree_boundary == TreeBoundary::LinuxCgroupV2
        && run.tree_cleanup_confirmed
        && !residue
}

#[test]
#[ignore = "A7/A11 source only; unattested host/policy and no native grant; unselected"]
fn disposable_linux_a7_a11() {
    // Refuse BEFORE any host observation, LinuxFs, account or process operation.
    let prepared = prepared_delivery().expect("A7/A11 host/policy/delivery unattested");
    use hierarchy::{linux::LinuxFs, setup::Clock};
    struct Monotonic(std::time::Instant);
    impl Clock for Monotonic {
        fn now(&self) -> Duration {
            self.0.elapsed()
        }
        fn sleep(&self, duration: Duration) {
            std::thread::sleep(duration);
        }
    }
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
    assert_ne!(prepared.accounts[0].uid, prepared.accounts[1].uid);
    assert_ne!(prepared.accounts[0].gid, prepared.accounts[1].gid);
    assert!(hierarchy::fake::prepare_a1a2(
        &LinuxFs::system(),
        &Monotonic(std::time::Instant::now()),
        &Cancellation::default()
    ));
    let both_policy_cases = prepared.accounts.len() == 2;
    let mut cases = 0;
    for config in prepared.accounts {
        assert!(config.uid >= 1000 && config.gid >= 1000 && !config.admin);
        assert_eq!(config.memory_bytes, 64 * 1024 * 1024);
        assert_eq!(config.pids, 32);
        let runner = Runner::new(config.clone()).unwrap();
        let mut spec = RunSpec {
            executable: prepared.arxd.clone(),
            arguments: vec!["tool-probe".into()],
            cwd: PathBuf::new(),
            timeout: Duration::from_secs(20),
            output_bytes: 4096,
        };
        let probe = runner
            .run_phased(&spec, &Cancellation::default(), &Cancellation::default())
            .unwrap();
        assert!(valid_run(&probe, &config, runner.cleanup_residue()));
        assert!(probe_records(&output(&probe).unwrap()));
        spec.executable = prepared.reentry.clone();
        spec.arguments = vec![
            "--exact".into(),
            "unix_shim::rights::linux::tests::e4_sudo_nnp_reentry".into(),
            "--ignored".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
            "--color=never".into(),
        ];
        let reentry = runner
            .run_phased(&spec, &Cancellation::default(), &Cancellation::default())
            .unwrap();
        assert!(valid_run(&reentry, &config, runner.cleanup_residue()));
        let bytes = output(&reentry).unwrap();
        let observed = reentry_records(&bytes);
        assert!(observed);
        assert!(accepts(Facts {
            identity: valid_run(&reentry, &config, runner.cleanup_residue()),
            exited: reentry.exit_code == Some(0),
            bounded: output(&reentry).is_some(),
            records: probe_records(&output(&probe).unwrap()),
            rights: observed,
            c1: observed,
            alias_skipped: observed,
            c3_unopened: observed,
            path: observed,
            cloexec: observed,
            regular: observed,
            exec_eof: observed,
            nonzero: observed,
            both_policy_cases,
            cleanup: reentry.tree_cleanup_confirmed && !runner.cleanup_residue()
        }));
        cases += 1;
    }
    assert_eq!(cases, 2);
    crate::service::unix::Notify::send_to(&std::env::var_os("NOTIFY_SOCKET").unwrap(), b"READY=1")
        .unwrap();
    hold_identity_ack();
}

// Pure discriminators, also compiled in isolation by the source-only check.
#[test]
fn a7_a11_discriminators() {
    let good = Facts {
        identity: true,
        exited: true,
        bounded: true,
        records: true,
        rights: true,
        c1: true,
        alias_skipped: true,
        c3_unopened: true,
        path: true,
        cloexec: true,
        regular: true,
        exec_eof: true,
        nonzero: true,
        both_policy_cases: true,
        cleanup: true,
    };
    assert!(accepts(good));
    for index in 0..15 {
        let mut bad = good;
        match index {
            0 => bad.identity = false,
            1 => bad.exited = false,
            2 => bad.bounded = false,
            3 => bad.records = false,
            4 => bad.rights = false,
            5 => bad.c1 = false,
            6 => bad.alias_skipped = false,
            7 => bad.c3_unopened = false,
            8 => bad.path = false,
            9 => bad.cloexec = false,
            10 => bad.regular = false,
            11 => bad.exec_eof = false,
            12 => bad.nonzero = false,
            13 => bad.both_policy_cases = false,
            _ => bad.cleanup = false,
        }
        assert!(!accepts(bad), "assertion discriminator {index}");
    }
}
#[test]
fn a7_a11_delivery_stays_closed() {
    assert!(prepared_delivery().is_none());
}

#[test]
fn a7_a11_record_discriminators() {
    let mut probe = Vec::new();
    for name in crate::development::TOOLS {
        probe.extend_from_slice(
            format!("{{\"name\":\"{name}\",\"found\":false,\"version\":null}}\n").as_bytes(),
        );
    }
    let rights = b"{\"rights\":{\"no_new_privs\":true,\"sudo\":\"exit_nonzero\"}}\n";
    probe.extend_from_slice(rights);
    assert!(probe_records(&probe));
    for result in ["exit_zero", "unknown", "absent", "untrusted"] {
        let bad = String::from_utf8(probe.clone())
            .unwrap()
            .replace("exit_nonzero", result);
        assert!(!probe_records(bad.as_bytes()));
    }
    assert!(!probe_records(&probe[..probe.len() - 1]));
    assert!(!probe_records(&[probe.as_slice(), b"extra\n"].concat()));
    let tool = probe.split_inclusive(|b| *b == b'\n').next().unwrap();
    assert!(!probe_records(
        &[&probe[..probe.len() - rights.len()], tool, rights].concat()
    ));
    assert!(!probe_records(&vec![b'x'; 4097]));
    assert!(!probe_records(rights));
    let reentry = concat!(
        "\nrunning 1 test\ntest unix_shim::rights::linux::tests::e4_sudo_nnp_reentry ... ",
        "ARX_A7_A11_REENTRY_OK\nok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; ",
        "500 filtered out; finished in 0.01s\n\n"
    );
    assert!(reentry_records(reentry.as_bytes()));
    assert!(!reentry_records(&[reentry.as_bytes(), b"extra\n"].concat()));
    assert!(!reentry_records(
        reentry.replace("0 failed", "1 failed").as_bytes()
    ));
    assert!(!reentry_records(reentry.trim_end().as_bytes()));
}

#[test]
fn a7_a11_run_discriminators() {
    let config = OwnerConfig {
        account: "fixture".into(),
        uid: 2000,
        gid: 2001,
        workspace: "/fixture".into(),
        protected_directories: vec![],
        environment: BTreeMap::new(),
        admin: false,
        cgroup_parent: None,
        memory_bytes: 67108864,
        pids: 32,
    };
    let mut run = RunResult {
        outcome: Outcome::Exited,
        exit_code: Some(0),
        identity: ObservedIdentity {
            uid: 2000,
            gid: 2001,
            daemon_was_root: true,
        },
        tree_boundary: TreeBoundary::LinuxCgroupV2,
        tree_cleanup_confirmed: true,
        stdout: CapturedOutput {
            head: b"ok".to_vec(),
            tail: vec![],
            dropped_bytes: 0,
        },
        stderr: CapturedOutput {
            head: vec![],
            tail: vec![],
            dropped_bytes: 0,
        },
    };
    assert!(valid_run(&run, &config, false));
    assert!(!valid_run(&run, &config, true));
    run.identity.uid += 1;
    assert!(!valid_run(&run, &config, false));
    run.identity.uid -= 1;
    run.identity.gid += 1;
    assert!(!valid_run(&run, &config, false));
    run.identity.gid -= 1;
    run.identity.daemon_was_root = false;
    assert!(!valid_run(&run, &config, false));
    run.identity.daemon_was_root = true;
    run.exit_code = Some(1);
    assert!(!valid_run(&run, &config, false));
    run.exit_code = None;
    assert!(!valid_run(&run, &config, false));
    run.exit_code = Some(0);
    run.outcome = Outcome::TimedOut;
    assert!(!valid_run(&run, &config, false));
    run.outcome = Outcome::Exited;
    run.tree_cleanup_confirmed = false;
    assert!(!valid_run(&run, &config, false));
    run.tree_cleanup_confirmed = true;
    run.tree_boundary = TreeBoundary::MacProcessGroupBestEffort;
    assert!(!valid_run(&run, &config, false));
    assert!(output(&run).is_some());
    run.stdout.dropped_bytes = 1;
    assert!(output(&run).is_none());
    run.stdout.dropped_bytes = 0;
    run.stderr.dropped_bytes = 1;
    assert!(output(&run).is_none());
    run.stderr.dropped_bytes = 0;
    run.stderr.head.push(1);
    assert!(output(&run).is_none());
    run.stderr.head.clear();
    run.stderr.tail.push(1);
    assert!(output(&run).is_none());
    run.stderr.tail.clear();
    run.stdout.head = vec![0; 4097];
    assert!(output(&run).is_none());
}

use super::*;
fn absolute(s: &str) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(format!("C:/{s}"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(format!("/{s}"))
    }
}
pub(super) fn config() -> OwnerConfig {
    OwnerConfig {
        account: "arxbuild".into(),
        uid: 12345,
        gid: 12345,
        workspace: absolute("workspace"),
        protected_directories: vec![absolute("private")],
        environment: BTreeMap::new(),
        admin: false,
        cgroup_parent: None,
        memory_bytes: 64 * 1024 * 1024,
        pids: 32,
    }
}
pub(super) fn spec() -> RunSpec {
    RunSpec {
        executable: absolute("bin/true"),
        arguments: vec![],
        cwd: PathBuf::new(),
        timeout: Duration::from_secs(1),
        output_bytes: 10,
    }
}
#[test]
fn argv_and_deadline_limits() {
    let mut s = spec();
    assert!(validate_spec(&s).is_ok());
    s.arguments = vec!["a".repeat(8193)];
    assert!(validate_spec(&s).is_err());
    s.arguments = vec!["abc\0def".into()];
    assert!(validate_spec(&s).is_err());
    s.arguments = vec!["a".into(); 129];
    assert!(validate_spec(&s).is_err());
    s.arguments = vec!["a".repeat(8192); 9];
    assert!(validate_spec(&s).is_err());
    s.arguments.clear();
    s.timeout = Duration::ZERO;
    assert!(validate_spec(&s).is_err());
    s.timeout = Duration::from_secs(3601);
    assert!(validate_spec(&s).is_err());
    s.timeout = Duration::from_secs(1);
    s.output_bytes = 1024 * 1024 + 1;
    assert!(validate_spec(&s).is_err());
}
#[test]
fn paths_refuse_traversal_and_overlap() {
    let mut s = spec();
    for cwd in ["../escape", "a/../b", "./a", "a/./b"] {
        s.cwd = cwd.into();
        assert!(validate_spec(&s).is_err(), "{cwd}");
    }
    s.cwd = absolute("tmp");
    assert!(validate_spec(&s).is_err());
    let mut c = config();
    c.protected_directories = vec![c.workspace.join("keys")];
    assert!(validate_config(&c).is_err());
    c.protected_directories = vec![absolute("workspace")];
    c.workspace = absolute("workspace/build");
    assert!(validate_config(&c).is_err());
}
#[test]
fn owner_policy_never_allows_root_admin_or_credentials() {
    let mut c = config();
    assert!(Runner::new(c.clone()).is_ok());
    c.uid = 0;
    assert!(Runner::new(c.clone()).is_err());
    c.uid = 12345;
    c.admin = true;
    assert!(matches!(
        Runner::new(c.clone()),
        Err(ExecError::Unavailable(_))
    ));
    c.admin = false;
    c.environment
        .insert("AWS_SECRET_ACCESS_KEY".into(), "not-a-secret".into());
    assert!(Runner::new(c).is_err());
    let mut c = config();
    c.gid = 0;
    assert!(Runner::new(c).is_err());
    let mut c = config();
    c.pids = 0;
    assert!(Runner::new(c).is_err());
    let mut c = config();
    c.memory_bytes = 1;
    assert!(Runner::new(c).is_err());
}
#[test]
fn output_retains_head_tail_and_counts_drops_across_chunks() {
    let mut c = output::Capture::new(7);
    c.push(b"abcde");
    c.push(b"fghijklmnop");
    assert_eq!(
        c.finish(),
        CapturedOutput {
            head: b"abc".to_vec(),
            tail: b"mnop".to_vec(),
            dropped_bytes: 9
        }
    );
    let mut c = output::Capture::new(10);
    c.push(b"abc");
    assert_eq!(c.finish().dropped_bytes, 0);
}
#[test]
fn timeout_cancellation_and_shutdown_priority() {
    let t = Duration::from_secs(1);
    assert_eq!(stop_reason(t - t, t, false, false), None);
    assert_eq!(stop_reason(t, t, false, false), Some(Outcome::TimedOut));
    assert_eq!(stop_reason(t, t, true, false), Some(Outcome::Cancelled));
    assert_eq!(stop_reason(t, t, true, true), Some(Outcome::Shutdown));
    let token = Cancellation::default();
    let clone = token.clone();
    token.cancel();
    assert!(clone.is_cancelled());
}
#[cfg(windows)]
#[test]
fn windows_library_is_unavailable_and_cannot_spawn() {
    assert!(matches!(
        Runner::new(config())
            .unwrap()
            .run(&spec(), &Cancellation::default()),
        Err(ExecError::Unavailable(_))
    ));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn nofollow_walk_rejects_symlink_directory() {
    use std::os::unix::fs::symlink;
    let dir = std::env::temp_dir().join(format!(
        "arx-nofollow-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    // Resolve the trusted OS temp prefix only, never a production workspace.
    let base = dir.canonicalize().unwrap();
    std::fs::create_dir(base.join("real")).unwrap();
    symlink(base.join("real"), base.join("link")).unwrap();
    let open = |p: &Path| {
        use std::os::unix::ffi::OsStrExt;
        let names: Vec<_> = p
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(std::ffi::CString::new(s.as_bytes()).unwrap()),
                _ => None,
            })
            .collect();
        unix::walk(&names)
    };
    assert!(open(&base.join("real")).is_ok());
    assert!(open(&base.join("link")).is_err());
    std::fs::remove_dir_all(&base).unwrap();
}

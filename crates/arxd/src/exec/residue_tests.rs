use super::*;

#[test]
fn independent_owned_domains_keep_distinct_monotone_runner_flags() {
    // These runners represent distinct test domains, never replacements
    // for one service subtree. No execution or account lookup occurs.
    let domains = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let runners = domains.each_ref().map(|domain| {
        let mut config = super::tests::config();
        config.workspace = domain.path().join("workspace");
        config.protected_directories = vec![domain.path().join("private")];
        config.cgroup_parent = Some(domain.path().join("runs"));
        for path in [
            &config.workspace,
            &config.protected_directories[0],
            config.cgroup_parent.as_ref().unwrap(),
        ] {
            std::fs::create_dir(path).unwrap();
        }
        Runner::new(config).unwrap()
    });
    assert_ne!(
        runners[0].config.cgroup_parent,
        runners[1].config.cgroup_parent
    );
    assert!(!Arc::ptr_eq(&runners[0].residue, &runners[1].residue));
    assert!(runners.iter().all(|r| !r.cleanup_residue()));
    let group_flag = runners[0].residue.clone();
    std::thread::spawn(move || group_flag.store(true, Ordering::Release))
        .join()
        .unwrap();
    assert!(runners[0].cleanup_residue());
    assert!(!runners[1].cleanup_residue());
    runners[0].shutdown_handle().cancel();
    assert!(runners[0].cleanup_residue());
}

#[test]
fn cleanup_source_pins_release_sites_and_legacy_errno_order() {
    // A portable source guard, not evidence of Linux syscall behavior.
    let source = include_str!("cgroup.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap();
    let helper = source
        .split("fn release_leaf(")
        .nth(1)
        .unwrap()
        .split("fn text(")
        .next()
        .unwrap();
    let create = source
        .split("pub fn create(")
        .nth(1)
        .unwrap()
        .split("pub fn kill_and_confirm(")
        .next()
        .unwrap();
    let kill = source
        .split("pub fn kill_and_confirm(")
        .nth(1)
        .unwrap()
        .split("impl Drop for Group")
        .next()
        .unwrap();
    assert_eq!(source.matches("libc::unlinkat(").count(), 2);
    assert_eq!(helper.matches("libc::unlinkat(").count(), 1);
    assert_eq!(kill.matches("libc::unlinkat(").count(), 1);
    assert!(!create.contains("libc::unlinkat("));
    assert_eq!(
        create.matches("release_leaf(fd, &name, &residue);").count(),
        2
    );
    assert!(create.contains("if raw < 0 {\n            release_leaf(fd, &name, &residue);\n            return Err(std::io::Error::last_os_error().into());"));
    assert!(create.contains("Err(e) => {\n                release_leaf(fd, &name, &residue);\n                Err(e.into())"));
    assert_eq!(helper.matches("libc::").count(), 2); // unlinkat + AT_REMOVEDIR
    assert!(helper.contains("residue.store(true, Ordering::Release)"));
    assert!(source.contains(
        "if !self.kill_and_confirm() {\n            self.residue.store(true, Ordering::Release);"
    ));
    let unix = include_str!("unix.rs");
    assert!(unix.contains("#[cfg(target_os = \"linux\")] residue: Arc<AtomicBool>"));
    assert!(unix.contains("Group::create(c, residue)"));
    let runner = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap();
    assert_eq!(runner.matches("self.residue.clone()").count(), 1);
    assert_eq!(
        runner.matches("Arc::new(AtomicBool::new(false))").count(),
        1
    );
    let unix = unix.split("#[cfg(test)]").next().unwrap();
    assert_eq!(unix.matches("residue").count(), 2);
    for production in [runner, unix, source] {
        let compact: String = production.split_whitespace().collect();
        assert!(!compact.contains(".store(false,"));
    }
}

#![cfg(target_os = "linux")]

#[test]
#[ignore = "single hosted C14 grant with the qualified A10 broker"]
fn c14_target_authorized_build_and_lost_reply() {
    assert_eq!(std::env::var("ARX_C14_HOSTED").as_deref(), Ok("1"));
    let root = std::path::PathBuf::from(std::env::var_os("ARX_C14_ROOT").expect("hosted root"));
    assert_eq!(root.parent(), Some(std::path::Path::new("/var/lib")));
    // SAFETY: checking the staged harness identity has no side effects.
    assert_eq!(unsafe { libc::geteuid() }, 0);
    let status = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-B"])
        .arg(root.join("probe.py"))
        .arg("--campaign")
        .arg(&root)
        .arg(std::env::var("ARX_C14_IP").expect("hosted route"))
        .env_clear()
        .stdin(std::process::Stdio::null())
        .status()
        .expect("staged C14 harness");
    assert!(status.success(), "C14 campaign refused");
}

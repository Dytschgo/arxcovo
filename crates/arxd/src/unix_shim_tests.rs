// Included only inside the Unix unit-test module. Hooks change only in a child.
#[test]
#[ignore = "reexecuted by s7_silent_hook_subprocess"]
fn s7_panic_fixture() {
    if std::env::var_os("ARX_SHIM_TEST_CHILD").is_none() {
        return;
    }
    std::process::exit(super::silent_entry(|| panic!("ARX-SHIM-PANIC-MARKER")));
}

#[test]
fn s7_silent_hook_subprocess() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    for backtrace in [None, Some("1"), Some("full")] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "unix_shim::tests::s7_panic_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .env("ARX_SHIM_TEST_CHILD", "1");
        if let Some(value) = backtrace {
            command.env("RUST_BACKTRACE", value);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(125));
        assert!(output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("ARX-SHIM-PANIC-MARKER"));
    }
}

#[test]
fn version_validation() {
    for bytes in [b"a/b".as_slice(), b"a\\b", b"a\0b", b"", b"\x7f", b"\xff"] {
        assert_eq!(super::version(bytes), None);
    }
    assert_eq!(
        super::version(b"  good 1.0 \r\nother"),
        Some("good 1.0".into())
    );
    assert_eq!(super::version(&[b'x'; 129]), None);
}

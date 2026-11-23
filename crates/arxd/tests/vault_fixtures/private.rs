pub fn private_dir() -> tempfile::TempDir {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let dir = tempfile::tempdir_in(root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    {
        let status = std::process::Command::new("icacls")
            .arg(dir.path())
            .args(["/inheritance:r", "/grant:r"])
            .arg(format!("*{}:(OI)(CI)F", identity()))
            .args(["*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "private fixture ACL");
    }
    dir
}

#[cfg(windows)]
pub fn identity() -> String {
    let output = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    let (_, sid) = text.trim().rsplit_once(',').unwrap();
    sid.trim_matches('"').to_owned()
}

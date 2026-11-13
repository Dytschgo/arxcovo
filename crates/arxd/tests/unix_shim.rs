#![cfg(unix)]
use std::{
    os::unix::{
        fs::{PermissionsExt, symlink},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

trait BoundedOutput {
    fn pipe_output(&mut self) -> Output;
}
impl BoundedOutput for Command {
    fn pipe_output(&mut self) -> Output {
        let mut child = self
            .process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                // Only our still-unreaped fixture group, with no escaping descendants.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
                panic!("shim subprocess exceeded fixture deadline");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }
}

fn ordinary() -> bool {
    unsafe { libc::geteuid() != 0 }
}
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn command(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_arxd"));
    c.current_dir(dir).env_clear();
    c
}
fn exec(dir: &Path, mode: &str, target: &Path, cwd: &str) -> Output {
    command(dir)
        .args(["exec-child", mode])
        .arg(target)
        .arg(cwd)
        .pipe_output()
}
fn silent(o: Output, code: i32) {
    assert_eq!(o.status.code(), Some(code));
    assert!(o.stdout.is_empty());
    assert!(o.stderr.is_empty());
}

#[test]
fn s1_target_bytes_and_exit() {
    if !ordinary() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let target = script(
        d.path(),
        "target",
        "test \"$ARX_SHIM_TEST_VALUE\" = yes || exit 2; if read x; then exit 3; fi; printf TARGETBYTES; printf TARGETBYTES >&2; exit 23",
    );
    let o = command(d.path())
        .env("ARX_SHIM_TEST_VALUE", "yes")
        .stdin(Stdio::piped())
        .args(["exec-child", "path"])
        .arg(&target)
        .arg("")
        .pipe_output();
    assert_eq!(o.status.code(), Some(23));
    assert_eq!(o.stdout, b"TARGETBYTES");
    assert_eq!(o.stderr, b"TARGETBYTES");
    std::fs::create_dir(d.path().join("child")).unwrap();
    let target = script(
        d.path(),
        "args",
        "test \"$#\" = 4 && test \"$1\" = '-x' && test \"$2\" = '--' && test \"$3\" = '' && test \"$4\" = 'ü' && test \"${PWD##*/}\" = child",
    );
    silent(
        command(d.path())
            .args(["exec-child", "path"])
            .arg(target)
            .args(["child", "-x", "--", "", "ü"])
            .pipe_output(),
        0,
    );
}
#[test]
fn s2_silent_refusals() {
    if !ordinary() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let t = script(d.path(), "target", "exit 0");
    silent(exec(d.path(), "path", &d.path().join("missing"), ""), 127);
    std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o600)).unwrap();
    silent(exec(d.path(), "path", &t, ""), 126);
    std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir(d.path().join("real")).unwrap();
    symlink("real", d.path().join("link")).unwrap();
    for cwd in ["missing", "link", "..", "/", "real/../real", "./real"] {
        silent(exec(d.path(), "path", &t, cwd), 125);
    }
    for args in [
        vec!["exec-child"],
        vec!["exec-child-extra"],
        vec!["tool-probe", "extra"],
        vec!["tool-probe-extra"],
        vec!["exec-child", "bad", "target", ""],
    ] {
        silent(command(d.path()).args(args).pipe_output(), 125);
    }
    silent(exec(d.path(), "path", Path::new("relative"), ""), 125);
    silent(
        command(d.path())
            .args(["exec-child", "path"])
            .arg(&t)
            .arg("")
            .args(vec!["x"; 129])
            .pipe_output(),
        125,
    );
    silent(
        command(d.path())
            .args(["exec-child", "path"])
            .arg(&t)
            .arg("")
            .arg("x".repeat(8193))
            .pipe_output(),
        125,
    );
}
#[test]
fn s3_signal_is_137() {
    if !ordinary() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let t = script(d.path(), "target", "kill -KILL $$");
    silent(exec(d.path(), "path", &t, ""), 137);
}
#[test]
fn s4_manual_path_no_fallback() {
    if !ordinary() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    script(d.path(), "cargo", "exit 19");
    let run = |path: &std::ffi::OsStr, mode, target: &Path| {
        command(d.path())
            .env("PATH", path)
            .args(["exec-child", mode])
            .arg(target)
            .arg("")
            .pipe_output()
    };
    silent(run(d.path().as_os_str(), "tool", Path::new("cargo")), 19);
    silent(
        run(
            std::ffi::OsStr::new(":.:relative"),
            "tool",
            Path::new("cargo"),
        ),
        127,
    );
    silent(
        run(d.path().as_os_str(), "tool", Path::new("unlisted")),
        125,
    );
    silent(
        run(d.path().as_os_str(), "path", &d.path().join("absent/cargo")),
        127,
    );
    let prefix = ":".repeat(64);
    silent(
        run(
            std::ffi::OsStr::new(&format!("{prefix}{}", d.path().display())),
            "tool",
            Path::new("cargo"),
        ),
        127,
    );
    silent(
        run(
            std::ffi::OsStr::new(&format!("{}{}", ":".repeat(63), d.path().display())),
            "tool",
            Path::new("cargo"),
        ),
        19,
    );
    silent(
        run(
            std::ffi::OsStr::new(&format!(
                "{}/../{}",
                d.path().display(),
                d.path().file_name().unwrap().to_str().unwrap()
            )),
            "tool",
            Path::new("cargo"),
        ),
        127,
    );
    std::fs::set_permissions(
        d.path().join("cargo"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    silent(run(d.path().as_os_str(), "tool", Path::new("cargo")), 126);
    let next = d.path().join("next");
    std::fs::create_dir(&next).unwrap();
    script(&next, "cargo", "exit 21");
    silent(
        run(
            std::ffi::OsStr::new(&format!("{}:{}", d.path().display(), next.display())),
            "tool",
            Path::new("cargo"),
        ),
        21,
    );
}
#[test]
fn s6_probe_bounded_order_root_cwd_and_deadline() {
    if !ordinary() {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    script(d.path(), "cargo", "printf 'bad/version\\n'");
    script(d.path(), "rustc", "printf 'bad\\000version\\n'");
    // exec replaces the shell: the timeout leaves no sleeping descendant.
    script(
        d.path(),
        "rustup",
        "echo $$ > \"$0.pid\"; exec /bin/sleep 10",
    );
    script(
        d.path(),
        "git",
        "if test \"$PWD\" = /; then printf 'root-cwd\\n'; else printf 'wrong-cwd\\n'; fi",
    );
    script(
        d.path(),
        "python3",
        "i=0; while test $i -lt 5000; do printf x; i=$((i+1)); done",
    );
    let start = Instant::now();
    let mut probe = command(d.path());
    probe.env("PATH", d.path()).arg("tool-probe");
    #[cfg(target_os = "linux")]
    unsafe {
        probe.pre_exec(|| {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let o = probe.pipe_output();
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stderr.is_empty());
    assert!(o.stdout.len() <= 4096);
    assert!(start.elapsed() >= Duration::from_secs(5));
    assert!(start.elapsed() < Duration::from_secs(9));
    let rows: Vec<serde_json::Value> = String::from_utf8(o.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), arxd::development::TOOLS.len() + 1);
    let rights = rows.last().unwrap();
    assert!(rights.get("rights").is_some());
    assert!(rights.get("name").is_none());
    assert_eq!(rights["rights"].as_object().unwrap().len(), 2);
    #[cfg(target_os = "linux")]
    {
        assert_eq!(rights["rights"]["no_new_privs"], true);
        assert_ne!(rights["rights"]["sudo"], "exit_zero");
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert!(rights["rights"]["no_new_privs"].is_null());
        assert_eq!(rights["rights"]["sudo"], "unknown");
    }

    for (row, name) in rows.iter().zip(arxd::development::TOOLS) {
        assert_eq!(row["name"], *name);
    }
    for row in &rows[..3] {
        assert_eq!(row["found"], true);
        assert!(row["version"].is_null());
    }
    assert_eq!(rows[3]["version"], "root-cwd");
    assert!(rows[4]["version"].is_null());
    assert_eq!(rows[5]["found"], false);
    let pid: i32 = std::fs::read_to_string(d.path().join("rustup.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "timed-out child reaped");
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

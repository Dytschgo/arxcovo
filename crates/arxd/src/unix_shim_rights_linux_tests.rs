use super::*;
use std::{
    io::{Seek, SeekFrom, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    },
    path::Path,
};

fn ordinary() -> bool {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("E4_SKIP_ROOT");
        false
    } else {
        true
    }
}
fn fixture() -> (tempfile::TempDir, Settings) {
    let root = tempfile::tempdir().unwrap();
    std::fs::copy(
        std::env::current_exe().unwrap(),
        root.path().join("base-elf"),
    )
    .unwrap();
    for part in ["usr/bin", "usr/local/bin", "bin"] {
        std::fs::create_dir_all(root.path().join(part)).unwrap();
    }
    let settings = Settings {
        anchor: CString::new(root.path().as_os_str().as_bytes()).unwrap(),
        owner: unsafe { libc::geteuid() },
        deadline: Duration::from_millis(200),
        argv: [
            "sudo",
            "--exact",
            "unix_shim::rights::linux::tests::e4_helper",
            "--ignored",
            "--nocapture",
        ]
        .iter()
        .map(|v| CString::new(*v).unwrap())
        .collect(),
        env: vec![CString::new(format!("E4_ROOT={}", root.path().display())).unwrap()],
    };
    (root, settings)
}
fn fake(root: &Path, relative: &str, mode: u8) {
    let path = root.join(relative);
    std::fs::copy(root.join("base-elf"), &path).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&[mode])
        .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
fn no_markers(root: &Path) {
    for label in ["A", "B", "3", "S", "K"] {
        assert!(!root.join(label).exists(), "no fixture executed");
    }
}
fn call(s: &Settings) -> &'static str {
    run(s, || {})
}
#[test]
#[ignore = "fixed ELF reentry only"]
fn e4_helper() {
    let Some(root) = std::env::var_os("E4_ROOT") else {
        return;
    };
    // Read the bound executable itself, including after replacement/unlink.
    let mut file = std::fs::File::open("/proc/self/exe").unwrap();
    file.seek(SeekFrom::End(-1)).unwrap();
    let mut mode = [0];
    file.read_exact(&mut mode).unwrap();
    let label = std::str::from_utf8(&mode).unwrap();
    let mut marker = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(&root).join(label))
        .unwrap();
    marker.write_all(b"1").unwrap();
    assert_eq!(std::env::current_dir().unwrap(), Path::new("/"));
    assert_eq!(std::env::vars_os().count(), 1);
    let mut input = [0];
    assert_eq!(std::io::stdin().read(&mut input).unwrap(), 0);
    std::fs::write(Path::new(&root).join("pid"), std::process::id().to_string()).unwrap();
    match mode[0] {
        b'3' => std::process::exit(3),
        b'S' => std::thread::sleep(Duration::from_secs(10)),
        b'K' => unsafe {
            libc::kill(libc::getpid(), libc::SIGKILL);
        },
        _ => (),
    }
}
#[test]
fn s8_outcomes_and_reap() {
    if !ordinary() {
        return;
    }
    for (mode, expected) in [
        (b'A', "exit_zero"),
        (b'3', "exit_nonzero"),
        (b'S', "unknown"),
        (b'K', "unknown"),
    ] {
        let (root, settings) = fixture();
        fake(root.path(), "usr/bin/sudo", mode);
        let started = Instant::now();
        assert_eq!(call(&settings), expected, "fixed outcome");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "bounded fixture completion"
        );
        assert_eq!(
            std::fs::read(root.path().join((mode as char).to_string())).unwrap(),
            b"1"
        );
        let pid: i32 = std::fs::read_to_string(root.path().join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
            -1,
            "child reaped"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "child gone");
    }
    let (root, settings) = fixture();
    assert_eq!(call(&settings), "absent");
    let path = root.path().join("usr/bin/sudo");
    // A script must not run even though its interpreter exists.
    std::fs::write(
        &path,
        format!("#!/bin/sh\nprintf 1 > '{}/A'\n", root.path().display()),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(call(&settings), "unknown");
    no_markers(root.path());
    std::fs::remove_file(&path).unwrap();
    fake(root.path(), "usr/bin/sudo", b'A');
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(call(&settings), "unknown");
    no_markers(root.path());
}
#[test]
fn s9_all_candidates_before_exec() {
    if !ordinary() {
        return;
    }
    for case in 0..9 {
        let (root, mut settings) = fixture();
        fake(root.path(), "usr/bin/sudo", b'A');
        fake(root.path(), "bin/sudo", b'B');
        match case {
            0 => std::fs::set_permissions(
                root.path().join("bin/sudo"),
                std::fs::Permissions::from_mode(0o720),
            )
            .unwrap(),
            1 => settings.owner = settings.owner.wrapping_add(1),
            2 => {
                std::fs::remove_file(root.path().join("bin/sudo")).unwrap();
                symlink("../usr/bin/sudo", root.path().join("bin/sudo")).unwrap();
            }
            3 => std::fs::set_permissions(
                root.path().join("usr"),
                std::fs::Permissions::from_mode(0o702),
            )
            .unwrap(),
            4 => std::fs::set_permissions(
                root.path().join("usr/local/bin"),
                std::fs::Permissions::from_mode(0o720),
            )
            .unwrap(),
            5 => {
                std::fs::rename(root.path().join("usr"), root.path().join("trusted")).unwrap();
                symlink("trusted", root.path().join("usr")).unwrap();
            }
            6 | 7 => {
                std::fs::remove_dir_all(root.path().join("bin")).unwrap();
                symlink(
                    if case == 6 {
                        "trusted/usr/bin"
                    } else {
                        "usr/bin/"
                    },
                    root.path().join("bin"),
                )
                .unwrap();
            }
            8 => std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o720))
                .unwrap(),
            _ => unreachable!(),
        }
        assert_eq!(call(&settings), "untrusted", "fixed trust case");
        no_markers(root.path());
    }
}
#[test]
fn s9b_exact_alias() {
    if !ordinary() {
        return;
    }
    for target in ["usr/bin", "/usr/bin"] {
        let (root, settings) = fixture();
        fake(root.path(), "usr/bin/sudo", b'A');
        std::fs::remove_dir(root.path().join("bin")).unwrap();
        symlink(target, root.path().join("bin")).unwrap();
        assert_eq!(call(&settings), "exit_zero");
        assert_eq!(std::fs::read(root.path().join("A")).unwrap(), b"1");
        let anchor = directory(libc::AT_FDCWD, settings.anchor(), settings.owner()).unwrap();
        assert_eq!(
            alias(&anchor, settings.owner().wrapping_add(1)).err(),
            Some("untrusted")
        );
    }
}
#[test]
fn s9c_bound_object_survives_file_and_directory_swap() {
    if !ordinary() {
        return;
    }
    for directory_swap in [false, true] {
        let (root, settings) = fixture();
        fake(root.path(), "usr/bin/sudo", b'A');
        std::fs::create_dir(root.path().join("replacement")).unwrap();
        fake(root.path(), "replacement/sudo", b'B');
        assert_eq!(
            run(&settings, || {
                if directory_swap {
                    std::fs::rename(root.path().join("usr/bin"), root.path().join("old")).unwrap();
                    std::fs::rename(root.path().join("replacement"), root.path().join("usr/bin"))
                        .unwrap();
                } else {
                    std::fs::rename(
                        root.path().join("replacement/sudo"),
                        root.path().join("usr/bin/sudo"),
                    )
                    .unwrap();
                }
            }),
            "exit_zero"
        );
        assert_eq!(std::fs::read(root.path().join("A")).unwrap(), b"1");
        assert!(!root.path().join("B").exists());
    }
}
#[test]
#[ignore = "isolated PATH fixture only"]
fn e4_path_helper() {
    let Some(root) = std::env::var_os("E4_PATH_ROOT") else {
        return;
    };
    let settings = Settings {
        anchor: CString::new(root.as_os_str().as_bytes()).unwrap(),
        owner: unsafe { libc::geteuid() },
        ..Settings::default()
    };
    assert_eq!(call(&settings), "absent");
}
#[test]
fn s10_path_is_irrelevant() {
    if !ordinary() {
        return;
    }
    let (root, _) = fixture();
    fake(root.path(), "sudo", b'A');
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "unix_shim::rights::linux::tests::e4_path_helper",
            "--ignored",
        ])
        .env_clear()
        .env("PATH", root.path())
        .env("E4_PATH_ROOT", root.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "fixed PATH isolation");
    no_markers(root.path());
}
#[test]
fn s12_production_constants() {
    assert_eq!(ANCHOR, c"/");
    assert_eq!(OWNER, 0);
    assert_eq!(
        CANDIDATES,
        &[
            &[c"usr", c"bin", c"sudo"][..],
            &[c"usr", c"local", c"bin", c"sudo"][..],
            &[c"bin", c"sudo"][..]
        ]
    );
    assert_eq!(ALIASES, &[b"usr/bin".as_slice(), b"/usr/bin"]);
    assert_eq!(ARGV, &[c"sudo", c"-n", c"true"]);
    assert_eq!(ENV.len(), 0);
    assert_eq!(DEADLINE, Duration::from_secs(5));
    assert_eq!(NULL, c"/dev/null");
    assert_eq!(CWD, c"/");
    let settings = Settings::default();
    assert_eq!(settings.anchor(), ANCHOR);
    assert_eq!(settings.owner(), OWNER);
    assert_eq!(settings.deadline(), DEADLINE);
    assert_eq!(settings.argv.len(), 3);
    assert!(settings.env.is_empty());
}
#[test]
fn s11_nnp_parser() {
    assert_eq!(parse_nnp(b"NoNewPrivs:\t0\n"), Some(false));
    assert_eq!(parse_nnp(b"NoNewPrivs:\t1\n"), Some(true));
    for value in [
        b"NoNewPrivs:\t2\n".as_slice(),
        b"NoNewPrivs:\t1\nNoNewPrivs:\t0\n",
        b"NoNewPrivs: 1\n",
        b"",
        &[b'x'; 8193],
    ] {
        assert_eq!(parse_nnp(value), None);
    }
}

#[test]
#[ignore = "NNP-only fixed reentry, never system sudo"]
fn e4_nnp_helper() {
    match std::env::var("E4_NNP").as_deref() {
        Ok("true") => assert_eq!(nnp(), Some(true)),
        Ok("false") => assert_eq!(nnp(), Some(false)),
        _ => {}
    }
}
#[test]
fn s11_nnp_observed_in_child() {
    use std::os::unix::process::CommandExt;
    if !ordinary() {
        return;
    }
    for enabled in [false, true] {
        if !enabled && nnp() == Some(true) {
            eprintln!("E4_SKIP_INHERITED_NNP");
            continue;
        }
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "unix_shim::rights::linux::tests::e4_nnp_helper",
                "--ignored",
            ])
            .env_clear()
            .env("E4_NNP", if enabled { "true" } else { "false" });
        if enabled {
            unsafe {
                command.pre_exec(|| {
                    if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0 {
                        Ok(())
                    } else {
                        Err(std::io::Error::last_os_error())
                    }
                });
            }
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "fixed NNP observation");
    }
}

#[test]
fn s9_final_object_owner_check() {
    if !ordinary() {
        return;
    }
    let (root, settings) = fixture();
    fake(root.path(), "usr/bin/sudo", b'A');
    let anchor = directory(libc::AT_FDCWD, settings.anchor(), settings.owner()).unwrap();
    let value = candidate(&anchor, CANDIDATES[0], settings.owner()).unwrap();
    let metadata = stat(&value).unwrap();
    assert!(
        !trusted(&metadata, libc::S_IFREG, settings.owner().wrapping_add(1)),
        "foreign file owner refused"
    );
    no_markers(root.path());
}

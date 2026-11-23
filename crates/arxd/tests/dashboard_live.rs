//! Test-only launcher for the live-backend dashboard acceptance suite
//! (docs/m6-acceptance.md). `live_dashboard_server` is ignored by default:
//! the Playwright config `ui/tests/live/playwright.live.config.ts` starts it
//! as its web server and stops it when the run ends. It builds a hermetic,
//! private state root, provisions a synthetic owner password through the real
//! `arxd dashboard-password` CLI, seeds owner-visible activity and a VM catalog
//! through the public store and controller, and then runs the real foreground
//! daemon with only the loopback dashboard listener.
#![cfg(any(windows, target_os = "linux"))]

use arxd::{
    store::Activity,
    vm::{Clock, ClockReading, VmController, VmPolicy},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

/// Seeded `run_command` rows: more than one 20-entry activity page.
const ACTIVITY_ROWS: usize = 27;
const VM_RUNNING: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const VM_STOPPED: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const STATE_PREFIX: &str = "arx-live-";

struct Wall;

impl Clock for Wall {
    fn now(&self) -> ClockReading {
        ClockReading {
            utc_ms: now_ms(),
            monotonic: Instant::now(),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

/// Owner-private directory: 0700 on Linux; on Windows TokenUser owns it with
/// a protected, inheritable TokenUser-only DACL (the product's private check).
fn private_dir(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new().mode(0o700).create(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    {
        std::fs::create_dir(path).unwrap();
        win::protect(path);
    }
}

#[cfg(windows)]
mod win {
    use std::{os::windows::ffi::OsStrExt, path::Path, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    fn token_user() -> Vec<usize> {
        // SAFETY: the token handle is closed once; the buffer is sized by the
        // first query and outlives every use of the SID inside it.
        unsafe {
            let mut handle = ptr::null_mut();
            assert_ne!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle),
                0
            );
            let mut len = 0;
            GetTokenInformation(handle, TokenUser, ptr::null_mut(), 0, &mut len);
            let mut buffer = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
            let ok =
                GetTokenInformation(handle, TokenUser, buffer.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(handle);
            assert_ne!(ok, 0);
            buffer
        }
    }

    pub(super) fn protect(path: &Path) {
        let user = token_user();
        // SAFETY: TOKEN_USER heads the buffer, which outlives the SID; every
        // string is NUL-terminated and every allocation is freed once.
        unsafe {
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let mut text = ptr::null_mut();
            assert_ne!(ConvertSidToStringSidW(sid, &mut text), 0);
            let len = (0..184).find(|i| *text.add(*i) == 0).unwrap();
            let sid_text = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
            LocalFree(text.cast());
            let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid_text})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut()
                ),
                0
            );
            let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
                0
            );
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // DACL first: changing the owner needs the WRITE_OWNER it grants.
            let dacl = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            );
            LocalFree(descriptor);
            assert_eq!(dacl, 0, "protect DACL {}", path.display());
            let owner = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                sid,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(owner, 0, "protect owner {}", path.display());
        }
    }
}

/// Seed owner-visible activity and a controller-current VM catalog. Returns
/// the catalog directory for `dashboard.vm_state`.
fn seed(state: &Path) -> PathBuf {
    let activity = Activity::open(state).unwrap();
    for n in 0..ACTIVITY_ROWS {
        let id = activity.start("live-fixture-agent", "run_command").unwrap();
        let outcome = if n % 5 == 4 { "failed" } else { "succeeded" };
        activity.finish(id, outcome).unwrap();
    }
    drop(activity);

    let vm = state.join("vm");
    private_dir(&vm);
    // The controller creates the current schema; the dashboard only reads it.
    drop(VmController::open(&vm, VmPolicy::default(), Wall).unwrap());
    let expires = now_ms() + 6 * 24 * 60 * 60 * 1000;
    let created = now_ms() - 2 * 60 * 60 * 1000;
    let db = rusqlite::Connection::open(vm.join("activity.sqlite3")).unwrap();
    db.execute_batch(&format!(
        "BEGIN;
         INSERT INTO vm_templates VALUES(unhex('{t1}'),1,unhex('{d1}'),zeroblob(32),zeroblob(32),1024);
         INSERT INTO vm_templates VALUES(unhex('{t2}'),2,unhex('{d2}'),zeroblob(32),zeroblob(32),1024);
         INSERT INTO vm_instances(id,template,principal,expires_ms,state,dashboard_cpus,dashboard_memory)
            VALUES(unhex('{VM_RUNNING}'),unhex('{t1}'),'live-fixture-agent',{expires},2,4,8589934592);
         INSERT INTO vm_instances(id,template,principal,expires_ms,state,dashboard_cpus,dashboard_memory)
            VALUES(unhex('{VM_STOPPED}'),unhex('{t2}'),'live-fixture-agent',{expires},1,2,4294967296);
         INSERT INTO vm_snapshots(vm,id,ready,dashboard_created_ms)
            VALUES(unhex('{VM_RUNNING}'),unhex('{s1}'),1,{created});
         INSERT INTO vm_disk_claims(activity_id,vm,snapshot,persistent,staging)
            VALUES(900001,unhex('{VM_RUNNING}'),NULL,21474836480,0);
         INSERT INTO vm_disk_claims(activity_id,vm,snapshot,persistent,staging)
            VALUES(900002,unhex('{VM_RUNNING}'),unhex('{s1}'),1073741824,0);
         COMMIT;",
        t1 = "c3".repeat(16),
        t2 = "d4".repeat(16),
        d1 = "01".repeat(32),
        d2 = "02".repeat(32),
        s1 = "e5".repeat(16),
    ))
    .unwrap();
    vm
}

fn required(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} is required; run through playwright.live.config.ts"))
}

/// Remove state roots left by earlier runs that were stopped by a tree kill.
/// Only this launcher's own prefix under the dedicated root is touched.
fn remove_stale(root: &Path) {
    for entry in std::fs::read_dir(root).unwrap().flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(STATE_PREFIX)
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[test]
fn live_fixture_seeds_activity_and_vm_catalog() {
    let temp = tempfile::Builder::new()
        .prefix(STATE_PREFIX)
        .tempdir()
        .unwrap();
    let state = temp.path().join("state");
    private_dir(&state);
    let vm = seed(&state);
    let db = rusqlite::Connection::open(vm.join("activity.sqlite3")).unwrap();
    let count = |sql: &str| db.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap();
    assert_eq!(count("SELECT count(*) FROM vm_instances"), 2);
    assert_eq!(count("SELECT count(*) FROM vm_snapshots WHERE ready=1"), 1);
    drop(db);
    // Reopening proves the seeded rows satisfy the current controller schema.
    drop(VmController::open(&vm, VmPolicy::default(), Wall).unwrap());
    let activity = Activity::open(&state).unwrap();
    let reader = activity
        .start("live-fixture-agent", "list_activity")
        .unwrap();
    let page = activity
        .list(
            "live-fixture-agent",
            reader,
            &arxd::store::ListActivity {
                limit: 50,
                before_id: None,
                include_reads: false,
            },
        )
        .unwrap();
    assert_eq!(page.activities.len(), ACTIVITY_ROWS);
}

#[test]
#[ignore = "launcher for the live Playwright suite; see docs/m6-acceptance.md"]
fn live_dashboard_server() {
    let root = PathBuf::from(required("ARX_LIVE_ROOT"));
    let dist = PathBuf::from(required("ARX_LIVE_DIST"));
    let port: u16 = required("ARX_LIVE_PORT").parse().unwrap();
    let password = zeroize::Zeroizing::new(required("ARX_LIVE_PASSWORD"));
    assert!(dist.join("index.html").is_file(), "build ui/dist first");
    std::fs::create_dir_all(&root).unwrap();
    remove_stale(&root);
    let temp = tempfile::Builder::new()
        .prefix(STATE_PREFIX)
        .tempdir_in(&root)
        .unwrap();
    let state = temp.path().join("state");
    private_dir(&state);
    let vm = seed(&state);

    let binary = env!("CARGO_BIN_EXE_arxd");
    let mut setup = Command::new(binary)
        .args(["dashboard-password", "--state-dir"])
        .arg(&state)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    {
        let mut stdin = setup.stdin.take().unwrap();
        stdin.write_all(password.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    assert!(setup.wait().unwrap().success(), "password CLI refused");

    let config = state.join("dashboard.json");
    std::fs::write(
        &config,
        serde_json::json!({
            "state_dir": state,
            "agents": [],
            "dashboard": {
                "listen": format!("127.0.0.1:{port}"),
                "dist": dist,
                "vm_state": vm,
            }
        })
        .to_string(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut daemon = Command::new(binary)
        .arg("--config")
        .arg(&config)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            daemon.try_wait().unwrap().is_none(),
            "arxd refused the live fixture"
        );
        assert!(Instant::now() < deadline, "arxd did not listen in time");
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("arx-live: dashboard ready at http://localhost:{port}/");
    // Playwright stops the whole process tree when the run ends.
    let status = daemon.wait().unwrap();
    panic!("arxd exited before the suite finished: {status}");
}

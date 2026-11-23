//! Test-only launcher for the live-backend dashboard acceptance suite
//! (docs/m6-acceptance.md). `live_dashboard_server` is ignored by default:
//! the Playwright config `ui/tests/live/playwright.live.config.ts` starts it
//! as its web server, and its global teardown asks it to stop. The launcher
//! allocates one fresh, randomly named run directory inside an owner-private
//! root, provisions a synthetic owner password through the real
//! `arxd dashboard-password` CLI, seeds owner-visible activity and a VM catalog
//! through the public store and controller, and runs the real foreground
//! daemon with only the loopback dashboard listener. On stop it kills the
//! daemon and removes exactly its own run directory, never anything else.
#![cfg(any(windows, target_os = "linux"))]

use arxd::{
    store::Activity,
    vm::{Clock, ClockReading, VmController, VmPolicy},
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

/// Seeded `run_command` rows: more than one 20-entry activity page.
const ACTIVITY_ROWS: usize = 27;
const VM_RUNNING: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const VM_STOPPED: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const RUN_PREFIX: &str = "run-";

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

/// Apply owner-only protection to an existing directory: 0700 on Linux; on
/// Windows TokenUser owns it with a protected, inheritable TokenUser-only DACL.
fn protect(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    win::protect(path, "");
}

fn private_dir(path: &Path) {
    std::fs::create_dir(path).unwrap();
    protect(path);
}

/// The same owner and permission rule the product applies to private state:
/// owned by the current user, and no access for anyone but the user (plus
/// SYSTEM and Administrators on Windows).
fn is_private(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path).is_ok_and(|m| {
            m.is_dir() && m.mode() & 0o077 == 0 && m.uid() == unsafe { libc::geteuid() }
        })
    }
    #[cfg(windows)]
    {
        std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) && win::is_private(path)
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

    fn user_sid(user: &[usize]) -> PSID {
        // SAFETY: TOKEN_USER heads the buffer, which outlives the SID.
        unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    /// Owner TokenUser and a protected DACL granting TokenUser full control,
    /// followed by `extra` SDDL ACEs (tests use it to widen access).
    pub(super) fn protect(path: &Path, extra: &str) {
        let user = token_user();
        let sid = user_sid(&user);
        // SAFETY: every string is NUL-terminated and every allocation is
        // freed once; the SID outlives the calls.
        unsafe {
            let mut text = ptr::null_mut();
            assert_ne!(ConvertSidToStringSidW(sid, &mut text), 0);
            let len = (0..184).find(|i| *text.add(*i) == 0).unwrap();
            let sid_text = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
            LocalFree(text.cast());
            let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid_text}){extra}")
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
            let name = wide(path);
            // DACL first: changing the owner needs the WRITE_OWNER it grants.
            let dacl = SetNamedSecurityInfoW(
                name.as_ptr(),
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
                name.as_ptr(),
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

    /// Mirrors arxd's private-state rule: owner is TokenUser and every allow
    /// ACE names TokenUser, SYSTEM or Builtin Administrators.
    pub(super) fn is_private(path: &Path) -> bool {
        let user = token_user();
        let me = user_sid(&user);
        let name = wide(path);
        // SAFETY: the descriptor owns the returned owner and DACL pointers
        // and is freed once after the last read through them.
        unsafe {
            let (mut owner, mut acl, mut descriptor) =
                (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            if GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut acl,
                ptr::null_mut(),
                &mut descriptor,
            ) != 0
            {
                return false;
            }
            let mut private = !acl.is_null() && EqualSid(owner, me) != 0;
            if private {
                for i in 0..(*acl).AceCount {
                    let mut ace = ptr::null_mut();
                    if GetAce(acl, u32::from(i), &mut ace) == 0 {
                        private = false;
                        break;
                    }
                    let kind = (*ace.cast::<ACE_HEADER>()).AceType;
                    if kind == 1 {
                        continue; // a deny ACE grants nothing
                    }
                    let sid = ptr::addr_of!((*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart)
                        as *mut core::ffi::c_void;
                    if kind != 0
                        || (EqualSid(sid, me) == 0
                            && IsWellKnownSid(sid, WinLocalSystemSid) == 0
                            && IsWellKnownSid(sid, WinBuiltinAdministratorsSid) == 0)
                    {
                        private = false;
                        break;
                    }
                }
            }
            LocalFree(descriptor);
            private
        }
    }
}

/// Allocate this run's own directory: a fresh, randomly named child of an
/// owner-private `root`. Creates `root` (one level, owner-only) only when it
/// does not exist yet; refuses an existing root that is not a private, plain
/// directory. Dropping or closing the guard removes this directory only.
fn run_dir(root: &Path) -> Result<tempfile::TempDir, String> {
    if !root.is_absolute() {
        return Err("ARX_LIVE_ROOT must be absolute".into());
    }
    if std::fs::symlink_metadata(root).is_err() {
        std::fs::create_dir(root).map_err(|_| "cannot create ARX_LIVE_ROOT")?;
        protect(root);
    }
    arxd::config::checked_path(root).map_err(|_| "ARX_LIVE_ROOT path refused")?;
    if !is_private(root) {
        return Err("ARX_LIVE_ROOT must be an owner-private directory".into());
    }
    let dir = tempfile::Builder::new()
        .prefix(RUN_PREFIX)
        .rand_bytes(16)
        .tempdir_in(root)
        .map_err(|_| "cannot create the run directory")?;
    protect(dir.path());
    if !is_private(dir.path()) {
        return Err("run directory is not private".into());
    }
    Ok(dir)
}

/// Kills and reaps the daemon when dropped, before the run directory goes.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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

/// A private test parent standing in for `ARX_LIVE_ROOT`.
fn private_parent() -> tempfile::TempDir {
    let parent = tempfile::tempdir().unwrap();
    protect(parent.path());
    parent
}

#[test]
fn live_fixture_seeds_activity_and_vm_catalog() {
    let parent = private_parent();
    let state = parent.path().join("state");
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
fn run_cleanup_removes_only_its_own_directory() {
    let parent = private_parent();
    let root = parent.path().join("live-root");
    // A missing root is created owner-private.
    let first = run_dir(&root).unwrap();
    assert!(is_private(&root));
    // Siblings, including another run's directory and a look-alike, survive.
    let other_run = run_dir(&root).unwrap();
    let lookalike = root.join(format!("{RUN_PREFIX}not-mine"));
    std::fs::create_dir(&lookalike).unwrap();
    std::fs::write(lookalike.join("keep"), b"keep").unwrap();
    let mine = first.path().to_owned();
    assert_ne!(mine, other_run.path());
    std::fs::create_dir(mine.join("state")).unwrap();
    std::fs::write(mine.join("state").join("data"), b"data").unwrap();
    first.close().unwrap();
    assert!(!mine.exists());
    assert!(other_run.path().is_dir());
    assert_eq!(std::fs::read(lookalike.join("keep")).unwrap(), b"keep");
}

#[test]
fn run_dir_refuses_a_root_that_is_not_private() {
    let parent = private_parent();
    let root = parent.path().join("shared-root");
    std::fs::create_dir(&root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(windows)]
    win::protect(&root, "(A;OICI;FR;;;WD)");
    assert!(!is_private(&root));
    assert!(run_dir(&root).is_err());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    assert!(run_dir(Path::new("relative-root")).is_err());
}

#[test]
#[ignore = "launcher for the live Playwright suite; see docs/m6-acceptance.md"]
fn live_dashboard_server() {
    let root = PathBuf::from(required("ARX_LIVE_ROOT"));
    let dist = PathBuf::from(required("ARX_LIVE_DIST"));
    let port: u16 = required("ARX_LIVE_PORT").parse().unwrap();
    let token = required("ARX_LIVE_RUN");
    assert!(
        token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "ARX_LIVE_RUN must be 32 hex digits"
    );
    let password = zeroize::Zeroizing::new(required("ARX_LIVE_PASSWORD"));
    assert!(dist.join("index.html").is_file(), "build ui/dist first");
    let run = run_dir(&root).unwrap_or_else(|reason| panic!("{reason}"));
    let state = run.path().join("state");
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
    // Declared after `run`, so it is dropped (killed and reaped) first, even
    // when an assertion below panics.
    let mut daemon = Daemon(
        Command::new(binary)
            .arg("--config")
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "arxd refused the live fixture"
        );
        assert!(Instant::now() < deadline, "arxd did not listen in time");
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("arx-live: dashboard ready at http://localhost:{port}/");
    // The Playwright global teardown creates this run's stop file.
    let stop = root.join(format!("stop-{token}"));
    while !stop.exists() {
        if let Some(status) = daemon.0.try_wait().unwrap() {
            panic!("arxd exited before the suite finished: {status}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(daemon);
    run.close().unwrap();
    // Removing the stop file tells the teardown that cleanup is complete.
    std::fs::remove_file(&stop).unwrap();
    println!("arx-live: stopped arxd and removed this run's directory");
}

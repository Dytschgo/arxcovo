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
    path::{Component, Path, PathBuf},
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
/// Returns whether every step succeeded; it never panics.
fn try_protect(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).is_ok()
    }
    #[cfg(windows)]
    {
        win::protect(path, "")
    }
}

/// Fixture setup: protection that must succeed.
fn protect(path: &Path) {
    assert!(try_protect(path), "protect {}", path.display());
}

fn private_dir(path: &Path) {
    create_private_dir(path).unwrap();
    assert!(is_private(path), "private fixture directory");
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

    thread_local! {
        /// Test fault injection: makes every identity lookup fail.
        pub(super) static IDENTITY_FAULT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// The current token's TOKEN_USER buffer, or None when it cannot be read.
    /// Never panics, so callers on the launcher's write path can roll back.
    fn token_user() -> Option<Vec<usize>> {
        if IDENTITY_FAULT.get() {
            return None;
        }
        // SAFETY: the token handle is closed once; the buffer is sized by the
        // first query and outlives every use of the SID inside it.
        unsafe {
            let mut handle = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) == 0 {
                return None;
            }
            let mut len = 0;
            GetTokenInformation(handle, TokenUser, ptr::null_mut(), 0, &mut len);
            let mut buffer = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
            let ok =
                GetTokenInformation(handle, TokenUser, buffer.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(handle);
            (ok != 0 && len != 0).then_some(buffer)
        }
    }

    fn user_sid(user: &[usize]) -> PSID {
        // SAFETY: TOKEN_USER heads the buffer, which outlives the SID.
        unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    /// The SID's string form, or None when it cannot be converted.
    fn user_text(sid: PSID) -> Option<String> {
        // SAFETY: a successful conversion returns a NUL-terminated string
        // that is read within its bound and freed once.
        unsafe {
            let mut text = ptr::null_mut();
            if ConvertSidToStringSidW(sid, &mut text) == 0 {
                return None;
            }
            let sid_text = (0..184)
                .find(|i| *text.add(*i) == 0)
                .and_then(|len| String::from_utf16(std::slice::from_raw_parts(text, len)).ok());
            LocalFree(text.cast());
            sid_text
        }
    }

    /// Create a new directory whose owner and protected TokenUser-only DACL
    /// are part of its creation, so it never exists with inherited access.
    pub(super) fn create_private(path: &Path) -> std::io::Result<()> {
        use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;
        let identity = || std::io::Error::other("identity unavailable");
        let user = token_user().ok_or_else(identity)?;
        let sid_text = user_text(user_sid(&user)).ok_or_else(identity)?;
        let sddl: Vec<u16> = format!("O:{sid_text}D:P(A;OICI;FA;;;{sid_text})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let name = wide(path);
        // SAFETY: both strings are NUL-terminated; the descriptor is used
        // only while live and freed once.
        unsafe {
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            let created = CreateDirectoryW(name.as_ptr(), &attributes);
            let error = std::io::Error::last_os_error();
            LocalFree(descriptor);
            if created == 0 { Err(error) } else { Ok(()) }
        }
    }

    /// The owner is TokenUser, SYSTEM or Builtin Administrators.
    pub(super) fn trusted_owner(path: &Path) -> bool {
        let Some(user) = token_user() else {
            return false;
        };
        let me = user_sid(&user);
        let name = wide(path);
        // SAFETY: the descriptor owns the owner pointer and is freed once.
        unsafe {
            let (mut owner, mut descriptor) = (ptr::null_mut(), ptr::null_mut());
            if GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            ) != 0
            {
                return false;
            }
            let trusted = !owner.is_null()
                && (EqualSid(owner, me) != 0
                    || IsWellKnownSid(owner, WinLocalSystemSid) != 0
                    || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0);
            LocalFree(descriptor);
            trusted
        }
    }

    /// Owner TokenUser and a protected DACL granting TokenUser full control,
    /// followed by `extra` SDDL ACEs (tests use it to widen access). Returns
    /// whether both the DACL and the owner were applied; never panics.
    pub(super) fn protect(path: &Path, extra: &str) -> bool {
        let Some(user) = token_user() else {
            return false;
        };
        let sid = user_sid(&user);
        let Some(sid_text) = user_text(sid) else {
            return false;
        };
        let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid_text}){extra}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let name = wide(path);
        // SAFETY: every string is NUL-terminated and every allocation is
        // freed once; the SID and ACL outlive the calls that use them.
        unsafe {
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return false;
            }
            let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
            // DACL first: changing the owner needs the WRITE_OWNER it grants.
            let dacl =
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) != 0
                    && SetNamedSecurityInfoW(
                        name.as_ptr(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        acl,
                        ptr::null_mut(),
                    ) == 0;
            LocalFree(descriptor);
            dacl && SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                sid,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ) == 0
        }
    }

    /// Mirrors arxd's private-state rule: owner is TokenUser and every allow
    /// ACE names TokenUser, SYSTEM or Builtin Administrators.
    pub(super) fn is_private(path: &Path) -> bool {
        let Some(user) = token_user() else {
            return false;
        };
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

/// Create a directory that is owner-only from the moment it exists: mode
/// 0700 on Linux (the umask can only remove bits), or a creation-time owner
/// and protected DACL on Windows. Never opens or follows an existing entry.
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(windows)]
    win::create_private(path)
}

/// The parent of `ARX_LIVE_ROOT` may be shared scratch, but it must not be
/// controlled by another account: owned by the current user or the system
/// (root; SYSTEM or Administrators on Windows), and on Linux not writable by
/// group or others unless the sticky bit protects its entries (as on /tmp).
fn trusted_parent(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path).is_ok_and(|m| {
            m.is_dir()
                && (m.uid() == unsafe { libc::geteuid() } || m.uid() == 0)
                && (m.mode() & 0o022 == 0 || m.mode() & 0o1000 != 0)
        })
    }
    #[cfg(windows)]
    {
        std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) && win::trusted_owner(path)
    }
}

/// Whether `root` is spelled exactly as its normalized form, judged from
/// the string alone (no filesystem access): rebuilding it from its parsed
/// components must give the same string, which refuses `.` segments, doubled
/// or trailing separators and, on Windows, `/` separators. `..` segments are
/// refused outright. On Windows only a plain drive path (`C:\...`) is
/// accepted, and no segment may contain `:` (alternate data streams) or end in
/// `.` or a space, which Windows would silently strip and alias.
fn normalized_spelling(root: &Path) -> bool {
    let Some(raw) = root.to_str() else {
        return false;
    };
    let mut rebuilt = PathBuf::new();
    for component in root.components() {
        match component {
            Component::ParentDir | Component::CurDir => return false,
            #[cfg(windows)]
            Component::Prefix(prefix) if !matches!(prefix.kind(), std::path::Prefix::Disk(_)) => {
                return false;
            }
            Component::Normal(segment) => {
                let Some(segment) = segment.to_str() else {
                    return false;
                };
                if cfg!(windows)
                    && (segment.contains(':') || segment.ends_with('.') || segment.ends_with(' '))
                {
                    return false;
                }
            }
            _ => {}
        }
        rebuilt.push(component);
    }
    rebuilt.as_os_str() == raw
}

/// Why `prepare_run` did not return a run directory.
#[derive(Debug)]
enum PrepareError {
    /// The root was refused. The directory contents are exactly as they
    /// were; when refused before the first write, nothing was touched at all.
    Refused(String),
    /// A step after the first write failed and cleanup could not remove
    /// everything this call created. These paths still exist.
    CleanupFailed {
        reason: String,
        leftovers: Vec<PathBuf>,
    },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(reason) => write!(f, "ARX_LIVE_ROOT refused: {reason}"),
            Self::CleanupFailed { reason, leftovers } => write!(
                f,
                "ARX_LIVE_ROOT cleanup failed after: {reason}; remove by hand: {}",
                leftovers
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// Seams on the write phase. The launcher uses `LIVE`; tests inject failures
/// to drive every rollback and cleanup-failure path.
#[derive(Clone, Copy)]
struct Hooks {
    /// Verification of each created object.
    verify: fn(&Path) -> bool,
    /// Runs just before the root is created.
    before_create_root: fn(),
    /// Runs once the root exists and has verified (or already existed).
    after_root_ready: fn(),
    /// Runs just before a root created by this call is removed.
    before_remove_root: fn(&Path),
    /// Removes the run directory.
    close_run: fn(tempfile::TempDir) -> std::io::Result<()>,
}

fn noop() {}
fn noop_path(_: &Path) {}

const LIVE: Hooks = Hooks {
    verify: is_private,
    before_create_root: noop,
    after_root_ready: noop,
    before_remove_root: noop_path,
    close_run: tempfile::TempDir::close,
};

/// Allocate this run's own directory, a fresh randomly named child of an
/// owner-private root, from the raw `ARX_LIVE_ROOT` value.
///
/// Invariant:
/// (a) a refusal before the first write touches nothing, parent timestamps
///     included;
/// (b) after a write, every failure removes what this call created, newest
///     first, so the directory contents are exactly as they were (only the
///     timestamps of the directories that held a created-then-removed entry
///     may change). If a removal fails, the result is `CleanupFailed` naming
///     every path that remains, never a refusal.
///
/// 1. **Read-only phase.** Every check that can refuse runs before the first
///    write: unset, non-UTF-8 or relative value; non-normalized spelling
///    (string only, no filesystem access); parent chain missing or crossing a
///    link or reparse point (arxd's `checked_path`); parent controlled by
///    another account; an existing root that is a link, not a directory or
///    not owner-private.
/// 2. **Write phase.** At most two objects are created, in order: the root
///    (only if it was absent, owner-only at creation) and the run directory.
///    Every helper on this path returns errors instead of panicking.
fn prepare_run(raw: Option<&std::ffi::OsStr>) -> Result<tempfile::TempDir, PrepareError> {
    prepare_run_with(raw, &LIVE)
}

fn prepare_run_with(
    raw: Option<&std::ffi::OsStr>,
    hooks: &Hooks,
) -> Result<tempfile::TempDir, PrepareError> {
    let refused = |reason: &str| PrepareError::Refused(reason.to_owned());
    // ---- read-only phase -------------------------------------------------
    let Some(raw) = raw else {
        return Err(refused("it is not set"));
    };
    let root = Path::new(raw);
    if !root.is_absolute() || root.file_name().is_none() || !normalized_spelling(root) {
        return Err(refused(
            "an absolute, normalized directory path is required",
        ));
    }
    let Some(parent) = root.parent() else {
        return Err(refused(
            "an absolute, normalized directory path is required",
        ));
    };
    if arxd::config::checked_path(parent).is_err() {
        return Err(refused(
            "the parent path is missing, aliased or crosses a link or reparse point",
        ));
    }
    if !trusted_parent(parent) {
        return Err(refused(
            "the parent directory is controlled by another account",
        ));
    }
    let existed = match std::fs::symlink_metadata(root) {
        Ok(_) if arxd::config::checked_path(root).is_ok() && is_private(root) => true,
        Ok(_) => return Err(refused("it must be a plain owner-private directory")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err(refused("cannot inspect the path")),
    };
    // ---- write phase -------------------------------------------------------
    let exists = |path: &Path| std::fs::symlink_metadata(path).is_ok();
    // Remove what this call created (newest first) and report exactly what
    // could not be removed.
    let undo = |reason: &str, run: Option<PathBuf>| {
        let mut leftovers: Vec<PathBuf> = run.into_iter().filter(|path| exists(path)).collect();
        if !existed {
            (hooks.before_remove_root)(root);
            if std::fs::remove_dir(root).is_err() && exists(root) {
                leftovers.push(root.to_owned());
            }
        }
        if leftovers.is_empty() {
            refused(reason)
        } else {
            PrepareError::CleanupFailed {
                reason: reason.to_owned(),
                leftovers,
            }
        }
    };
    if !existed {
        (hooks.before_create_root)();
        if create_private_dir(root).is_err() {
            // The creation failed, so nothing was written.
            return Err(refused("cannot create the directory"));
        }
        if arxd::config::checked_path(root).is_err() || !(hooks.verify)(root) {
            return Err(undo(
                "the new directory did not verify as owner-private",
                None,
            ));
        }
    }
    (hooks.after_root_ready)();
    let dir = match tempfile::Builder::new()
        .prefix(RUN_PREFIX)
        .rand_bytes(16)
        .tempdir_in(root)
    {
        Ok(dir) => dir,
        Err(_) => return Err(undo("cannot create the run directory", None)),
    };
    // The root's inheritable owner-only DACL (or tempfile's 0700) already
    // applies; re-apply (owner included) and verify before any use.
    if !try_protect(dir.path()) || !(hooks.verify)(dir.path()) {
        let run = dir.path().to_owned();
        // A failed close may or may not have removed the entry; `undo`
        // reports it if it still exists.
        let _ = (hooks.close_run)(dir);
        return Err(undo(
            "the run directory did not verify as owner-private",
            Some(run),
        ));
    }
    Ok(dir)
}

/// Test convenience for an already-built path.
fn run_dir(root: &Path) -> Result<tempfile::TempDir, PrepareError> {
    prepare_run(Some(root.as_os_str()))
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

/// A private test parent standing in for `ARX_LIVE_ROOT`'s parent.
fn private_parent() -> tempfile::TempDir {
    let parent = tempfile::tempdir().unwrap();
    protect(parent.path());
    parent
}

/// The parent's normalized spelling (`TMP` may use `/` on Windows hosts).
fn base(parent: &tempfile::TempDir) -> PathBuf {
    let base = std::path::absolute(parent.path()).unwrap();
    assert!(normalized_spelling(&base), "normalized test base");
    base
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
    let root = base(&parent).join("live-root");
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

/// Link `link` to the existing directory `target`: a symlink on Linux, a
/// junction (no privilege needed) on Windows.
fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    assert!(
        Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
}

/// One entry of a snapshot: its relative path (with `/`), the metadata that
/// must never change, and its timestamps.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    path: String,
    stable: String,
    times: String,
}

/// `base` itself (as ".") and every entry below it, recursively and without
/// following links. `stable` holds the kind, length, permissions or
/// attributes and the owner-private flag; `times` holds the modification time
/// (and the status-change time on Linux).
fn snapshot(base: &Path) -> Vec<Entry> {
    fn describe(base: &Path, path: &Path) -> Entry {
        let meta = std::fs::symlink_metadata(path).unwrap();
        let kind = if meta.file_type().is_symlink() {
            'l'
        } else if meta.is_dir() {
            'd'
        } else {
            'f'
        };
        #[cfg(unix)]
        let (bits, times) = {
            use std::os::unix::fs::MetadataExt;
            (
                format!("mode={:o} uid={}", meta.mode(), meta.uid()),
                format!(
                    "mtime={}.{} ctime={}.{}",
                    meta.mtime(),
                    meta.mtime_nsec(),
                    meta.ctime(),
                    meta.ctime_nsec()
                ),
            )
        };
        #[cfg(windows)]
        let (bits, times) = {
            use std::os::windows::fs::MetadataExt;
            (
                format!("attributes={:x}", meta.file_attributes()),
                format!("mtime={}", meta.last_write_time()),
            )
        };
        let relative = path.strip_prefix(base).unwrap().to_string_lossy();
        Entry {
            path: if relative.is_empty() {
                ".".to_owned()
            } else {
                relative.replace('\\', "/")
            },
            stable: format!(
                "{kind} len={} {bits} private={}",
                if kind == 'f' { meta.len() } else { 0 },
                kind == 'd' && is_private(path)
            ),
            times,
        }
    }
    fn walk(base: &Path, dir: &Path, out: &mut Vec<Entry>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let described = describe(base, &path);
            let descend = described.stable.starts_with('d');
            out.push(described);
            if descend {
                walk(base, &path, out);
            }
        }
    }
    let mut out = vec![describe(base, base)];
    walk(base, base, &mut out);
    out.sort();
    out
}

/// Clear the timestamps of the directories that may hold a created and
/// then removed entry: the root's parent (".") and the root.
fn without_parent_times(entries: &[Entry]) -> Vec<Entry> {
    entries
        .iter()
        .map(|entry| Entry {
            times: if entry.path == "." || entry.path == "live-root" {
                String::new()
            } else {
                entry.times.clone()
            },
            ..entry.clone()
        })
        .collect()
}

/// What a refused row must leave behind.
enum Expect {
    /// Refused before the first write: identical, timestamps included.
    Untouched,
    /// Refused after a write and fully rolled back: identical contents; only
    /// the timestamps of "." and "live-root" may differ.
    Restored,
    /// Cleanup failed: `CleanupFailed` names exactly these paths (relative,
    /// matched by prefix), they exist, and everything else is restored.
    Leftovers(&'static [&'static str]),
}

type Setup = Box<dyn Fn(&Path) -> Option<std::ffi::OsString>>;
type Row = (&'static str, Setup, Hooks, Expect);

fn row(
    name: &'static str,
    setup: impl Fn(&Path) -> Option<std::ffi::OsString> + 'static,
    hooks: Hooks,
    expect: Expect,
) -> Row {
    (name, Box::new(setup), hooks, expect)
}

/// `base` followed by the raw, unnormalized text `tail`.
fn at(base: &Path, tail: &str) -> Option<std::ffi::OsString> {
    Some(format!("{}{tail}", base.display()).into())
}

fn named(path: &Path, name: &str) -> bool {
    path.file_name()
        .is_some_and(|file| file.to_string_lossy().starts_with(name))
}

// Injected faults for the write phase.
fn fail_root(path: &Path) -> bool {
    !named(path, "live-root") && is_private(path)
}

fn fail_run(path: &Path) -> bool {
    !named(path, RUN_PREFIX) && is_private(path)
}

/// A real removal failure: the root is no longer empty.
fn block_removal(root: &Path) {
    std::fs::write(root.join("blocker"), b"blocker").unwrap();
}

/// A close that fails and leaves the run directory in place.
fn close_and_keep(dir: tempfile::TempDir) -> std::io::Result<()> {
    let _ = dir.keep();
    Err(std::io::Error::other("injected close failure"))
}

/// A close that reports failure although the directory was removed.
fn close_then_fail(dir: tempfile::TempDir) -> std::io::Result<()> {
    dir.close()?;
    Err(std::io::Error::other("injected close failure"))
}

#[cfg(windows)]
fn lose_identity() {
    win::IDENTITY_FAULT.set(true);
}

/// Invariant A: for every refused `ARX_LIVE_ROOT`, (a) a refusal before the
/// first write touches nothing, parent timestamps included, and (b) after a
/// write every failure restores the directory contents exactly, or reports
/// `CleanupFailed` naming precisely what remains. The rows follow
/// `prepare_run_with` in order. Each runs in a fresh private base whose full
/// recursive snapshot is compared before and after.
#[test]
fn every_refused_root_leaves_the_disk_unchanged() {
    use Expect::*;
    let s = std::path::MAIN_SEPARATOR;
    let root = move |b: &Path| at(b, &format!("{s}live-root"));
    let existing_root = move |b: &Path| {
        private_dir(&b.join("live-root"));
        at(b, &format!("{s}live-root"))
    };
    let mut rows = vec![
        // (a) Read-only refusals. Value checks: no filesystem access at all.
        row("unset", |_| None, LIVE, Untouched),
        row("empty", |_| Some("".into()), LIVE, Untouched),
        row(
            "relative",
            |_| Some("relative-root".into()),
            LIVE,
            Untouched,
        ),
        row(
            "parent segment",
            move |b| at(b, &format!("{s}sub{s}..{s}live-root")),
            LIVE,
            Untouched,
        ),
        row(
            "trailing parent segment",
            move |b| at(b, &format!("{s}sub{s}..")),
            LIVE,
            Untouched,
        ),
        row(
            "dot segment",
            move |b| at(b, &format!("{s}.{s}live-root")),
            LIVE,
            Untouched,
        ),
        row(
            "trailing dot segment",
            move |b| at(b, &format!("{s}live-root{s}.")),
            LIVE,
            Untouched,
        ),
        row(
            "trailing separator",
            move |b| at(b, &format!("{s}live-root{s}")),
            LIVE,
            Untouched,
        ),
        row(
            "doubled separator",
            move |b| at(b, &format!("{s}{s}live-root")),
            LIVE,
            Untouched,
        ),
        // Parent chain and parent owner.
        row(
            "missing parent",
            move |b| at(b, &format!("{s}absent{s}live-root")),
            LIVE,
            Untouched,
        ),
        row(
            "missing root below a link",
            move |b| {
                link_dir(&b.join("sub"), &b.join("link"));
                at(b, &format!("{s}link{s}live-root"))
            },
            LIVE,
            Untouched,
        ),
        // Existing root.
        row(
            "link as the root",
            move |b| {
                link_dir(&b.join("sub"), &b.join("link"));
                at(b, &format!("{s}link"))
            },
            LIVE,
            Untouched,
        ),
        row(
            "root is a file",
            move |b| {
                std::fs::write(b.join("live-root"), b"file").unwrap();
                at(b, &format!("{s}live-root"))
            },
            LIVE,
            Untouched,
        ),
        row(
            "existing root that is not private",
            move |b| {
                let root = b.join("live-root");
                std::fs::create_dir(&root).unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                }
                #[cfg(windows)]
                assert!(win::protect(&root, "(A;OICI;FR;;;WD)"));
                at(b, &format!("{s}live-root"))
            },
            LIVE,
            Untouched,
        ),
        // (b) Write phase, fully rolled back.
        row(
            "new root fails verification",
            root,
            Hooks {
                verify: fail_root,
                ..LIVE
            },
            Restored,
        ),
        row(
            "run directory fails verification under a new root",
            root,
            Hooks {
                verify: fail_run,
                ..LIVE
            },
            Restored,
        ),
        row(
            "run directory fails verification under an existing root",
            existing_root,
            Hooks {
                verify: fail_run,
                ..LIVE
            },
            Restored,
        ),
        row(
            "run directory close reports failure after removing it",
            root,
            Hooks {
                verify: fail_run,
                close_run: close_then_fail,
                ..LIVE
            },
            Restored,
        ),
        // (b) Write phase, cleanup failures reported with their leftovers.
        row(
            "root removal fails after the new root fails verification",
            root,
            Hooks {
                verify: fail_root,
                before_remove_root: block_removal,
                ..LIVE
            },
            Leftovers(&["live-root"]),
        ),
        row(
            "root removal fails after the run directory fails verification",
            root,
            Hooks {
                verify: fail_run,
                before_remove_root: block_removal,
                ..LIVE
            },
            Leftovers(&["live-root"]),
        ),
        row(
            "run directory close fails under a new root",
            root,
            Hooks {
                verify: fail_run,
                close_run: close_and_keep,
                ..LIVE
            },
            Leftovers(&["live-root/run-", "live-root"]),
        ),
        row(
            "run directory close fails under an existing root",
            existing_root,
            Hooks {
                verify: fail_run,
                close_run: close_and_keep,
                ..LIVE
            },
            Leftovers(&["live-root/run-"]),
        ),
    ];
    // A parent owned by another account needs privileges to create, so the
    // unsafe-parent row uses Linux mode bits; Windows owner trust is covered
    // by the same `trusted_parent` call on the real parent of every row.
    #[cfg(unix)]
    rows.push(row(
        "parent writable by others without the sticky bit",
        move |b| {
            use std::os::unix::fs::PermissionsExt;
            let open = b.join("open");
            std::fs::create_dir(&open).unwrap();
            std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
            at(b, &format!("{s}open{s}live-root"))
        },
        LIVE,
        Untouched,
    ));
    // Windows-only spellings, and identity lookups (which Linux does not
    // need) failing on the write path instead of panicking.
    #[cfg(windows)]
    rows.extend([
        row("forward slash", |b| at(b, "/live-root"), LIVE, Untouched),
        row(
            "only forward slashes",
            |b| {
                Some(
                    format!("{}/live-root", b.display())
                        .replace('\\', "/")
                        .into(),
                )
            },
            LIVE,
            Untouched,
        ),
        row(
            "trailing dot alias",
            |b| at(b, r"\live-root."),
            LIVE,
            Untouched,
        ),
        row(
            "trailing space alias",
            |b| at(b, r"\live-root "),
            LIVE,
            Untouched,
        ),
        row(
            "alternate data stream",
            |b| at(b, r"\live-root:stream"),
            LIVE,
            Untouched,
        ),
        row(
            "verbatim prefix",
            |b| Some(format!(r"\\?\{}\live-root", b.display()).into()),
            LIVE,
            Untouched,
        ),
        row(
            "identity unavailable when creating the root",
            root,
            Hooks {
                before_create_root: lose_identity,
                ..LIVE
            },
            Untouched,
        ),
        row(
            "identity lost after a new root is ready",
            root,
            Hooks {
                after_root_ready: lose_identity,
                ..LIVE
            },
            Restored,
        ),
        row(
            "identity lost under an existing root",
            existing_root,
            Hooks {
                after_root_ready: lose_identity,
                ..LIVE
            },
            Restored,
        ),
    ]);
    for (name, setup, hooks, expect) in rows {
        let parent = private_parent();
        let base = base(&parent);
        std::fs::create_dir(base.join("sub")).unwrap();
        let raw = setup(&base);
        let before = snapshot(&base);
        let result = prepare_run_with(raw.as_deref(), &hooks);
        #[cfg(windows)]
        win::IDENTITY_FAULT.set(false);
        let after = snapshot(&base);
        match (expect, result) {
            (_, Ok(_)) => panic!("{name}: accepted"),
            (Untouched, Err(PrepareError::Refused(_))) => {
                assert_eq!(after, before, "{name}: touched before the first write");
            }
            (Restored, Err(PrepareError::Refused(_))) => assert_eq!(
                without_parent_times(&after),
                without_parent_times(&before),
                "{name}: contents not restored"
            ),
            (Leftovers(expected), Err(PrepareError::CleanupFailed { leftovers, .. })) => {
                let reported: Vec<String> = leftovers
                    .iter()
                    .map(|path| {
                        assert!(path.exists(), "{name}: reported leftover does not exist");
                        path.strip_prefix(&base)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    })
                    .collect();
                assert_eq!(reported.len(), expected.len(), "{name}: leftovers");
                for (path, prefix) in reported.iter().zip(expected) {
                    assert!(
                        path.starts_with(prefix),
                        "{name}: leftover {path} vs {prefix}"
                    );
                }
                // Everything outside the reported leftovers is restored.
                let outside = |entries: &[Entry]| {
                    without_parent_times(entries)
                        .into_iter()
                        .filter(|entry| {
                            !reported.iter().any(|left| {
                                entry.path == *left || entry.path.starts_with(&format!("{left}/"))
                            })
                        })
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    outside(&after),
                    outside(&before),
                    "{name}: more than the leftovers"
                );
            }
            (_, Err(error)) => panic!("{name}: unexpected outcome {error}"),
        }
    }
    // Positive control: the normalized spelling is accepted and created.
    let parent = private_parent();
    let root = base(&parent).join("live-root");
    let run = prepare_run(Some(root.as_os_str())).unwrap();
    assert!(is_private(&root) && is_private(run.path()));
    run.close().unwrap();
}

#[test]
#[ignore = "launcher for the live Playwright suite; see docs/m6-acceptance.md"]
fn live_dashboard_server() {
    // Read every input before the first filesystem write.
    let raw_root = std::env::var_os("ARX_LIVE_ROOT");
    let dist = PathBuf::from(required("ARX_LIVE_DIST"));
    let port: u16 = required("ARX_LIVE_PORT").parse().unwrap();
    let token = required("ARX_LIVE_RUN");
    assert!(
        token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "ARX_LIVE_RUN must be 32 hex digits"
    );
    let password = zeroize::Zeroizing::new(required("ARX_LIVE_PASSWORD"));
    assert!(dist.join("index.html").is_file(), "build ui/dist first");
    let run = prepare_run(raw_root.as_deref()).unwrap_or_else(|error| panic!("{error}"));
    let root = PathBuf::from(raw_root.expect("accepted root"));
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

//! Retained store construction; no production startup caller.

use rusqlite::{Connection, ffi};
use std::{ffi::CStr, fs::File, path::Path};

type IdentityAbi = ffi::ArxIdentityV1;
const _: () = {
    assert!(size_of::<IdentityAbi>() == 80);
    assert!(align_of::<IdentityAbi>() == 4);
    assert!(std::mem::offset_of!(IdentityAbi, magic) == 0);
    assert!(std::mem::offset_of!(IdentityAbi, abi_version) == 4);
    assert!(std::mem::offset_of!(IdentityAbi, struct_bytes) == 8);
    assert!(std::mem::offset_of!(IdentityAbi, status) == 12);
    assert!(std::mem::offset_of!(IdentityAbi, os_errno) == 16);
    assert!(std::mem::offset_of!(IdentityAbi, vfs_kind) == 20);
    assert!(std::mem::offset_of!(IdentityAbi, dev_lo) == 24);
    assert!(std::mem::offset_of!(IdentityAbi, dev_hi) == 28);
    assert!(std::mem::offset_of!(IdentityAbi, ino_lo) == 32);
    assert!(std::mem::offset_of!(IdentityAbi, ino_hi) == 36);
    assert!(std::mem::offset_of!(IdentityAbi, nlink_lo) == 40);
    assert!(std::mem::offset_of!(IdentityAbi, nlink_hi) == 44);
    assert!(std::mem::offset_of!(IdentityAbi, mode) == 48);
    assert!(std::mem::offset_of!(IdentityAbi, flags) == 52);
    assert!(std::mem::offset_of!(IdentityAbi, reserved) == 56);
};

#[derive(Debug, PartialEq, Eq)]
struct ObjectId(u64, u64);

#[cfg(any(unix, test))]
fn initial() -> IdentityAbi {
    IdentityAbi {
        magic: 0x41525849,
        abi_version: 1,
        struct_bytes: 80,
        status: u32::MAX,
        os_errno: 0,
        vfs_kind: 0,
        dev_lo: 0,
        dev_hi: 0,
        ino_lo: 0,
        ino_hi: 0,
        nlink_lo: 0,
        nlink_hi: 0,
        mode: 0,
        flags: 0,
        reserved: [0; 6],
    }
}
fn words(lo: u32, hi: u32) -> u64 {
    (u64::from(hi) << 32) | u64::from(lo)
}

// Format validation is deliberately distinct from real native identity evidence.
#[cfg(any(unix, test))]
fn decode(rc: i32, a: &IdentityAbi, kind: u32) -> Result<ObjectId, ()> {
    if rc != ffi::SQLITE_OK
        || a.magic != 0x41525849
        || a.abi_version != 1
        || a.struct_bytes != 80
        || a.status != 0
        || a.os_errno != 0
        || a.vfs_kind != kind
        || !(1..=2).contains(&kind)
        || a.flags != 0
        || a.reserved != [0; 6]
        || words(a.nlink_lo, a.nlink_hi) != 1
        || a.mode & 0o170000 != 0o100000
        || a.mode & 0o077 != 0
    {
        return Err(());
    }
    Ok(ObjectId(
        words(a.dev_lo, a.dev_hi),
        words(a.ino_lo, a.ino_hi),
    ))
}

fn qualify(db: &mut Connection) -> Result<(), ()> {
    // Exclusive borrow encloses all public SQLite handle operations. No callbacks/SQL.
    unsafe {
        if ffi::sqlite3_libversion_number() != 3050002
            || CStr::from_ptr(ffi::sqlite3_sourceid()).to_bytes() != b"2025-06-28 14:00:48 2af157d77fb1304a74176eaee7fbc7c7e932d946bf25325e9c26c91db19e3079"
            || ffi::sqlite3_arxcovo_bound_abi_v1() != 1
            || ffi::sqlite3_arxcovo_qualify_main_v1(db.handle()) != ffi::SQLITE_OK {
            return Err(());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn guard_id(file: &File) -> Result<ObjectId, ()> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata().map_err(|_| ())?;
    if !m.is_file()
        || m.nlink() != 1
        || m.mode() & 0o077 != 0
        || m.uid() != unsafe { libc::geteuid() }
    {
        return Err(());
    }
    // MetadataExt on macOS exposes signed dev_t through u64; reject negative origin.
    #[cfg(target_os = "macos")]
    if m.dev() > i32::MAX as u64 {
        return Err(());
    }
    Ok(ObjectId(m.dev(), m.ino()))
}
#[cfg(windows)]
fn windows_id(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<ObjectId, ()> {
    use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::*};
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(());
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        if GetFileType(handle) != FILE_TYPE_DISK
            || GetFileInformationByHandle(handle, &mut info) == 0
        {
            return Err(());
        }
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.nNumberOfLinks != 1
    {
        return Err(());
    }
    Ok(ObjectId(
        u64::from(info.dwVolumeSerialNumber),
        words(info.nFileIndexLow, info.nFileIndexHigh),
    ))
}
#[cfg(windows)]
fn guard_id(file: &File) -> Result<ObjectId, ()> {
    use std::os::windows::io::AsRawHandle;
    windows_id(file.as_raw_handle())
}

fn actual_id(db: &mut Connection) -> Result<ObjectId, ()> {
    qualify(db)?;
    #[cfg(unix)]
    {
        let mut a = initial();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                ffi::ARX_FCNTL_IDENTITY_V1,
                std::ptr::from_mut(&mut a).cast(),
            )
        };
        let kind = if cfg!(target_os = "macos") { 2 } else { 1 };
        decode(rc, &a, kind)
    }
    #[cfg(windows)]
    {
        let mut borrowed: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                29,
                std::ptr::from_mut(&mut borrowed).cast(),
            )
        };
        if rc != ffi::SQLITE_OK {
            return Err(());
        }
        // Borrowed only for this measurement; never close, adopt or return HANDLE.
        windows_id(borrowed)
    }
}

fn verify(db: &mut Connection, original: &File, path: &Path) -> Result<ObjectId, ()> {
    crate::config::checked_path(path).map_err(|_| ())?;
    crate::security::private(path).map_err(|_| ())?;
    crate::security::private(path.parent().ok_or(())?).map_err(|_| ())?;
    let before = guard_id(original)?;
    let actual = actual_id(db)?;
    if actual != before || guard_id(original)? != before {
        return Err(());
    }
    // Recheck path object without treating it as SQLite identity proof.
    if guard_id(&File::open(path).map_err(|_| ())?)? != before {
        return Err(());
    }
    crate::config::checked_path(path).map_err(|_| ())?;
    crate::security::private(path).map_err(|_| ())?;
    Ok(actual)
}

fn immutable(path: &Path) -> rusqlite::Result<Connection> {
    let text = path
        .to_str()
        .ok_or(rusqlite::Error::InvalidPath(path.to_owned()))?
        .replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let mut uri = String::from("file:");
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/:_- .".contains(&b) && b != b' ' {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

use super::{
    enrollment::ClusterId,
    log_store::VoteLogStore,
    startup_plan::{Guard, Owner},
    state_store::{Init, StateStore, StoreError},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoundFailure {
    Intent,
    Layout,
    Inspection(super::startup_plan::Refusal),
    Unavailable,
    Identity,
    Open(StoreError),
    Init(StoreError),
    Unwind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoundAttempt {
    Ready,
    Begun,
    Failed(BoundFailure),
    Complete,
}
#[derive(Clone, Copy)]
pub(super) enum BoundIntent {
    Fresh(Init),
    UntouchedGenesis(Init),
    Recovery(ClusterId),
    Inspect(ClusterId),
}
#[derive(Clone, Copy)]
enum StoreKind {
    State,
    VoteLog,
}
impl StoreKind {
    fn stem(self) -> &'static str {
        match self {
            Self::State => "cluster-state",
            Self::VoteLog => "cluster-log",
        }
    }
}
// Duplicates of the original guard share the Windows lock's handle lifetime.
// Release the cooperative store lock explicitly after its connection has dropped,
// while preserving the original Owner's identity guard.
struct StoreLock(File);
impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub(super) struct BoundLease {
    _lock: StoreLock,
    _guard: File,
    _identity: ObjectId,
}
pub(super) struct BoundDb {
    pub(super) db: Connection,
    pub(super) dir: PathBuf,
    pub(super) lease: BoundLease,
}
// Connection always drops before either cooperative lock or retained original.
#[derive(Default)]
struct Pending {
    db: Option<Connection>,
    bound: Option<BoundDb>,
    lock: Option<StoreLock>,
    guard: Option<File>,
}
pub(super) struct BoundStores<'owner> {
    state: Option<StateStore>,
    log: Option<VoteLogStore>,
    inspection_state: Option<BoundDb>,
    pending: Pending,
    staged: Vec<Guard>,
    owner: &'owner mut Owner,
    failure: Option<BoundFailure>,
    #[cfg(test)]
    accesses: usize,
    #[cfg(test)]
    opens: Vec<usize>,
}
// Owned slots outlive the short Owner borrow, including partial acquisitions.
pub(super) struct OwnedStores {
    state: Option<StateStore>,
    log: Option<VoteLogStore>,
    _inspection_state: Option<BoundDb>,
    _pending: Pending,
    _staged: Vec<Guard>,
    failure: Option<BoundFailure>,
    #[cfg(test)]
    accesses: usize,
    #[cfg(test)]
    opens: Vec<usize>,
}
impl OwnedStores {
    #[cfg(test)]
    pub(super) fn io_counts(&self) -> (usize, usize) {
        (self.accesses, self.opens.len())
    }
    #[cfg(test)]
    pub(super) fn pointers(&self) -> (usize, usize) {
        (
            self.log.as_ref().unwrap().bound_pointer(),
            self.state.as_ref().unwrap().bound_pointer(),
        )
    }
    pub(super) fn take_pair(&mut self) -> Result<(StateStore, VoteLogStore), BoundFailure> {
        self.failure.map_or(Ok(()), Err)?;
        if self.state.is_none() || self.log.is_none() {
            return Err(BoundFailure::Unavailable);
        }
        // All validation precedes these infallible moves.
        Ok((self.state.take().unwrap(), self.log.take().unwrap()))
    }
}
// Deliberately dormant until separately authorized startup integration. Tests exercise
// this concrete entry point; no public capability or live caller is introduced.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "store-only slice has no live caller")
)]
impl<'owner> BoundStores<'owner> {
    pub(super) fn acquire(owner: &'owner mut Owner, intent: BoundIntent) -> Self {
        let mut stores = Self {
            state: None,
            log: None,
            inspection_state: None,
            pending: Pending::default(),
            staged: Vec::new(),
            owner,
            failure: None,
            #[cfg(test)]
            accesses: 0,
            #[cfg(test)]
            opens: Vec::new(),
        };
        match stores.owner.bound_attempt {
            BoundAttempt::Failed(f) => {
                stores.failure = Some(f);
                return stores;
            }
            BoundAttempt::Begun | BoundAttempt::Complete => {
                stores.failure = Some(BoundFailure::Unavailable);
                return stores;
            }
            BoundAttempt::Ready => {}
        }
        let valid = match intent {
            BoundIntent::Fresh(_) | BoundIntent::UntouchedGenesis(_) => {
                stores.owner.plan.mode == super::startup_plan::Mode::CreateFounder
            }
            BoundIntent::Recovery(_) => stores.owner.plan.mode.recovery(),
            BoundIntent::Inspect(_) => true,
        };
        if !valid {
            stores.failure = Some(BoundFailure::Intent);
            return stores;
        }
        stores.owner.bound_attempt = BoundAttempt::Begun;
        let result = catch_unwind(AssertUnwindSafe(|| stores.construct(intent)));
        match result {
            Ok(Ok(())) => stores.owner.bound_attempt = BoundAttempt::Complete,
            Ok(Err(f)) => stores.fail(f),
            Err(_) => stores.fail(BoundFailure::Unwind),
        }
        stores
    }
    pub(super) fn into_owned(self) -> OwnedStores {
        OwnedStores {
            state: self.state,
            log: self.log,
            _inspection_state: self.inspection_state,
            _pending: self.pending,
            _staged: self.staged,
            failure: self.failure,
            #[cfg(test)]
            accesses: self.accesses,
            #[cfg(test)]
            opens: self.opens,
        }
    }
    /// One Begun interval covers readonly inspection and writable acquisition.
    /// The caller's validation runs while every acquired slot remains owned.
    pub(super) fn operational<T>(
        owner: &'owner mut Owner,
        intent: BoundIntent,
        inspect: impl FnOnce(&mut Self) -> Result<T, BoundFailure>,
        validate: impl FnOnce(&T, &StateStore, &mut VoteLogStore) -> Result<(), BoundFailure>,
    ) -> (OwnedStores, Option<T>) {
        let mut stores = Self {
            state: None,
            log: None,
            inspection_state: None,
            pending: Pending::default(),
            staged: Vec::new(),
            owner,
            failure: None,
            #[cfg(test)]
            accesses: 0,
            #[cfg(test)]
            opens: Vec::new(),
        };
        let valid = match intent {
            BoundIntent::Fresh(_) | BoundIntent::UntouchedGenesis(_) => {
                stores.owner.plan.mode == super::startup_plan::Mode::CreateFounder
            }
            BoundIntent::Recovery(_) => stores.owner.plan.mode.recovery(),
            BoundIntent::Inspect(_) => false,
        };
        match stores.owner.bound_attempt {
            BoundAttempt::Failed(f) => stores.failure = Some(f),
            BoundAttempt::Begun | BoundAttempt::Complete => {
                stores.failure = Some(BoundFailure::Unavailable)
            }
            BoundAttempt::Ready if !valid => stores.fail(BoundFailure::Intent),
            BoundAttempt::Ready => {}
        }
        if stores.failure.is_some() {
            return (stores.into_owned(), None);
        }
        stores.owner.bound_attempt = BoundAttempt::Begun;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let observed = inspect(&mut stores)?;
            // Successful readonly observations are retired in connection/lease order
            // under the continuously held original outer lock.
            stores.inspection_state = None;
            stores.pending = Pending::default();
            stores.construct(intent)?;
            validate(
                &observed,
                stores.state.as_ref().unwrap(),
                stores.log.as_mut().unwrap(),
            )?;
            Ok(observed)
        }));
        let observed = match result {
            Ok(Ok(value)) => {
                stores.owner.bound_attempt = BoundAttempt::Complete;
                Some(value)
            }
            Ok(Err(f)) => {
                stores.fail(f);
                None
            }
            Err(_) => {
                stores.fail(BoundFailure::Unwind);
                None
            }
        };
        (stores.into_owned(), observed)
    }
    pub(super) fn validate_owner_for_start(
        &mut self,
        validate: impl FnOnce(&Owner) -> Result<(), super::startup_plan::Refusal>,
    ) -> Result<(), BoundFailure> {
        self.access();
        validate(self.owner).map_err(BoundFailure::Inspection)
    }
    pub(super) fn inspect_for_start<T>(
        &mut self,
        cluster: ClusterId,
        read: impl FnOnce(&Connection, &File, &Connection, &File) -> Result<T, StoreError>,
    ) -> Result<T, BoundFailure> {
        self.construct(BoundIntent::Inspect(cluster))?;
        self.inspect(read)
    }
    pub(super) fn inspect<T>(
        &mut self,
        read: impl FnOnce(&Connection, &File, &Connection, &File) -> Result<T, StoreError>,
    ) -> Result<T, BoundFailure> {
        self.status()?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let bound = self
                .pending
                .bound
                .as_ref()
                .ok_or(BoundFailure::Unavailable)?;
            if !bound
                .db
                .is_readonly("main")
                .map_err(|_| BoundFailure::Unavailable)?
            {
                return Err(BoundFailure::Unavailable);
            }
            let state = self
                .inspection_state
                .as_ref()
                .ok_or(BoundFailure::Unavailable)?;
            if !state
                .db
                .is_readonly("main")
                .map_err(|_| BoundFailure::Unavailable)?
            {
                return Err(BoundFailure::Unavailable);
            }
            let value = read(
                &state.db,
                &state.lease._guard,
                &bound.db,
                &bound.lease._guard,
            )
            .map_err(BoundFailure::Init)?;
            self.owner
                .inspect_validate()
                .map_err(BoundFailure::Inspection)?;
            Ok(value)
        }));
        match result {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(f)) => {
                self.fail(f);
                Err(f)
            }
            Err(_) => {
                self.fail(BoundFailure::Unwind);
                Err(BoundFailure::Unwind)
            }
        }
    }
    // Closed store-only access; no raw connections, adapters or Engine escape.
    fn validate_pair(
        &mut self,
        validate: impl FnOnce(&StateStore, &mut VoteLogStore) -> Result<(), StoreError>,
    ) -> Result<(), BoundFailure> {
        self.status()?;
        validate(
            self.state.as_ref().ok_or(BoundFailure::Unavailable)?,
            self.log.as_mut().ok_or(BoundFailure::Unavailable)?,
        )
        .map_err(BoundFailure::Init)
    }
    fn fail(&mut self, f: BoundFailure) {
        self.failure = Some(f);
        self.owner.bound_attempt = BoundAttempt::Failed(f);
    }
    pub(super) fn status(&self) -> Result<(), BoundFailure> {
        self.failure.map_or(Ok(()), Err)
    }
    pub(super) fn first_failure(&self) -> Option<BoundFailure> {
        self.failure
    }
    fn access(&mut self) {
        #[cfg(test)]
        {
            self.accesses += 1;
        }
    }
    fn original(&mut self, name: &str, fresh: bool) -> Result<File, BoundFailure> {
        if fresh {
            let g = self
                .owner
                .bound_create(name)
                .map_err(|_| BoundFailure::Layout)?;
            self.staged.push(g);
            self.staged
                .last()
                .unwrap()
                .bound_duplicate()
                .map_err(|_| BoundFailure::Layout)
        } else {
            self.owner
                .bound_guard(name)
                .map_err(|_| BoundFailure::Layout)
        }
    }
    fn open(&mut self, kind: StoreKind, fresh: bool, readonly: bool) -> Result<(), BoundFailure> {
        self.access();
        let stem = kind.stem();
        self.pending.lock = Some(StoreLock(
            self.original(&format!("{stem}.lock"), fresh).map_err(|f| {
                if readonly {
                    BoundFailure::Inspection(super::startup_plan::Refusal::Ownership)
                } else {
                    f
                }
            })?,
        ));
        self.pending
            .lock
            .as_ref()
            .unwrap()
            .0
            .try_lock()
            .map_err(|_| {
                if readonly {
                    BoundFailure::Inspection(super::startup_plan::Refusal::Ownership)
                } else {
                    BoundFailure::Open(StoreError::Io)
                }
            })?;
        self.pending.guard = Some(self.original(&format!("{stem}.sqlite3"), fresh)?);
        self.validate_open_layout(readonly)?;
        let path = self.owner.plan.state.join(format!("{stem}.sqlite3"));
        #[cfg(test)]
        {
            if matches!(kind, StoreKind::VoteLog) {
                tests::boundary("second", self)?;
            }
            tests::boundary("open", self)?;
        }
        #[cfg(test)]
        let path = tests::sqlite_open_path(&path);
        let flags = if readonly {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
        };
        self.pending.db = Some(
            if readonly {
                immutable(&path)
            } else {
                Connection::open_with_flags(&path, flags)
            }
            .map_err(|_| BoundFailure::Open(StoreError::Sql))?,
        );
        #[cfg(test)]
        self.opens
            .push(unsafe { self.pending.db.as_ref().unwrap().handle() } as usize);
        let identity = verify(
            self.pending.db.as_mut().unwrap(),
            self.pending.guard.as_ref().unwrap(),
            &path,
        )
        .map_err(|_| BoundFailure::Identity)?;
        self.validate_open_layout(readonly)?;
        self.pending.bound = Some(BoundDb {
            db: self.pending.db.take().unwrap(),
            dir: self.owner.plan.state.clone(),
            lease: BoundLease {
                _lock: self.pending.lock.take().unwrap(),
                _guard: self.pending.guard.take().unwrap(),
                _identity: identity,
            },
        });
        Ok(())
    }
    fn validate_open_layout(&self, readonly: bool) -> Result<(), BoundFailure> {
        if readonly {
            self.owner
                .inspect_validate()
                .map_err(BoundFailure::Inspection)
        } else {
            self.owner
                .bound_validate(&self.staged, true)
                .map_err(|_| BoundFailure::Layout)
        }
    }
    fn require_v2(&self) -> Result<(), BoundFailure> {
        #[cfg(test)]
        tests::application_sql();
        let v: u32 = self
            .pending
            .bound
            .as_ref()
            .unwrap()
            .db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| BoundFailure::Init(StoreError::Sql))?;
        if v != 2 {
            return Err(BoundFailure::Init(StoreError::Unsupported));
        }
        Ok(())
    }
    fn construct(&mut self, intent: BoundIntent) -> Result<(), BoundFailure> {
        self.access();
        let fresh = matches!(intent, BoundIntent::Fresh(_));
        if fresh {
            self.owner
                .fresh_baseline()
                .map_err(|_| BoundFailure::Layout)?;
        } else {
            self.owner.recheck().map_err(|f| {
                if matches!(intent, BoundIntent::Inspect(_)) {
                    BoundFailure::Inspection(f)
                } else {
                    BoundFailure::Layout
                }
            })?;
        }
        let init = match intent {
            BoundIntent::Fresh(i) | BoundIntent::UntouchedGenesis(i) => i,
            BoundIntent::Recovery(cluster) | BoundIntent::Inspect(cluster) => Init {
                cluster,
                first_free: std::num::NonZeroU64::new(1).unwrap(),
            },
        };
        if matches!(intent, BoundIntent::Inspect(_)) {
            self.owner
                .inspect_validate()
                .map_err(BoundFailure::Inspection)?;
            self.open(StoreKind::State, false, true)?;
            self.inspection_state = self.pending.bound.take();
            self.open(StoreKind::VoteLog, false, true)?;
            #[cfg(test)]
            tests::pair_final();
            for (bound, kind) in [
                (self.inspection_state.as_mut().unwrap(), StoreKind::State),
                (self.pending.bound.as_mut().unwrap(), StoreKind::VoteLog),
            ] {
                verify(
                    &mut bound.db,
                    &bound.lease._guard,
                    &bound.dir.join(format!("{}.sqlite3", kind.stem())),
                )
                .map_err(|_| BoundFailure::Identity)?;
            }
            self.owner
                .inspect_validate()
                .map_err(BoundFailure::Inspection)?;
            return Ok(());
        }
        self.open(StoreKind::State, fresh, false)?;
        if !fresh {
            self.require_v2()?;
        }
        let initialized = StateStore::initialize_bound(self.pending.bound.as_mut().unwrap(), init)
            .map_err(BoundFailure::Init)?;
        self.state = Some(StateStore::from_initialized(
            self.pending.bound.take().unwrap(),
            initialized,
        ));
        // TRUNCATE may leave a zero journal; validate it before the next acquisition.
        self.owner
            .bound_validate(&self.staged, true)
            .map_err(|_| BoundFailure::Layout)?;
        self.open(StoreKind::VoteLog, fresh, false)?;
        if !fresh {
            self.require_v2()?;
        }
        let initialized =
            VoteLogStore::initialize_bound(self.pending.bound.as_mut().unwrap(), init.cluster)
                .map_err(BoundFailure::Init)?;
        self.log = Some(VoteLogStore::from_initialized(
            self.pending.bound.take().unwrap(),
            initialized,
        ));
        #[cfg(test)]
        tests::boundary("preflight", self)?;
        if !matches!(intent, BoundIntent::Recovery(_)) {
            let state = self.state.as_ref().unwrap();
            let log = self.log.as_mut().unwrap();
            if *state.state()
                != super::enrollment::EnrollmentState::new(init.cluster, init.first_free)
                || state.applied().is_some()
                || state.membership().is_some()
                || log.vote().map_err(BoundFailure::Init)?.is_some()
                || log
                    .log_state()
                    .map_err(BoundFailure::Init)?
                    .purged
                    .is_some()
                || log.live().map_err(BoundFailure::Init)? != 0
            {
                return Err(BoundFailure::Layout);
            }
        }
        self.validate_pair(|state, log| {
            let metadata = log.log_state()?;
            let valid = if let Some(applied) = state.applied() {
                if applied.index > i64::MAX as u64 {
                    return Err(StoreError::Corrupt);
                }
                let end = applied.index.checked_add(1).ok_or(StoreError::Corrupt)?;
                match metadata.purged {
                    Some(p) if p.index > applied.index => false,
                    Some(p) if p.index == applied.index => p == applied,
                    _ if metadata.last.is_none_or(|last| last.index < applied.index) => {
                        metadata.last < Some(applied)
                    }
                    _ => {
                        let rows = log.entries(applied.index, end, 1)?;
                        rows.len() == 1 && rows[0].0 == applied
                    }
                }
            } else {
                metadata.purged.is_none()
            };
            valid.then_some(()).ok_or(StoreError::Corrupt)
        })?;
        if fresh {
            self.owner
                .bound_install(&mut self.staged)
                .map_err(|_| BoundFailure::Layout)?;
        } else {
            self.owner.recheck().map_err(|_| BoundFailure::Layout)?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::fs;
    thread_local! { static FAULT: std::cell::RefCell<Option<(&'static str, bool)>> = const {std::cell::RefCell::new(None)}; }
    fn probe(dir: &Path, stem: &str, held: bool) {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join(format!("{stem}.lock")))
            .unwrap();
        assert_eq!(file.try_lock().is_err(), held, "{stem}");
    }
    pub(super) fn boundary(
        name: &'static str,
        stores: &mut BoundStores<'_>,
    ) -> Result<(), BoundFailure> {
        if let Some((point, panic)) = FAULT.with(|f| *f.borrow())
            && point == name
        {
            REACHED.with(|r| r.borrow_mut().push(name));
            probe(&stores.owner.plan.state, "cluster-owner", true);
            probe(&stores.owner.plan.state, "cluster-state", true);
            if name == "second" || name == "preflight" {
                probe(&stores.owner.plan.state, "cluster-log", true);
            }
            if panic {
                panic!("ordinary injected unwind");
            }
            return Err(BoundFailure::Init(StoreError::Sql));
        }
        Ok(())
    }
    thread_local! {
        static SQL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        static REACHED: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    pub(in crate::cluster) fn application_sql() {
        SQL.with(|n| n.set(n.get() + 1));
    }
    pub(in crate::cluster) fn configure(db: &Connection, stem: &str) -> Result<(), StoreError> {
        application_sql();
        if FAULT.with(|f| *f.borrow())
            == Some((
                if stem == "cluster-state" {
                    "state-pragma"
                } else {
                    "log-pragma"
                },
                false,
            ))
        {
            // The real configure_db PRAGMA batch must refuse synchronous changes
            // in this transaction. Do not manufacture its return value.
            db.execute_batch("BEGIN")
                .expect("PRAGMA failure fixture must begin its transaction");
        }
        Ok(())
    }
    pub(in crate::cluster) fn initialization(
        db: &Connection,
        dir: &Path,
        stem: &str,
    ) -> Result<(), StoreError> {
        let state = stem == "cluster-state";
        let phase = if state { "state-init" } else { "log-init" };
        if let Some((point, panic)) = FAULT.with(|f| *f.borrow()) {
            if point == phase || (state && point == "borrowed-init") {
                REACHED.with(|r| r.borrow_mut().push(point));
                probe(dir, "cluster-owner", true);
                probe(dir, "cluster-state", true);
                if !state {
                    probe(dir, "cluster-log", true);
                }
                if panic {
                    panic!("borrowed initialization unwind");
                }
                db.execute_batch("SELECT * FROM arxcovo_missing_injected_table")?;
            }
            if point == if state { "state-schema" } else { "log-schema" } {
                // Real initializer CREATE TABLE now fails on this same connection.
                db.execute_batch("PRAGMA query_only=ON")
                    .expect("schema failure fixture must enable query_only");
            }
        }
        Ok(())
    }
    pub(in crate::cluster) fn staging(owner: &Owner) {
        if FAULT.with(|f| *f.borrow()) == Some(("staging", false)) {
            probe(&owner.plan.state, "cluster-state", true);
            probe(&owner.plan.state, "cluster-log", true);
            fs::write(
                owner.plan.state.join("unexpected-install-entry"),
                b"fixture",
            )
            .unwrap();
        }
    }
    pub(super) fn sqlite_open_path(path: &Path) -> PathBuf {
        let fault = FAULT.with(|f| *f.borrow()).map(|(point, _)| point);
        let log = path.file_name().unwrap() == "cluster-log.sqlite3";
        let fault = match fault {
            Some("log-mismatch") if log => Some("pair-mismatch"),
            Some("state-mismatch") if !log => Some("pair-mismatch"),
            Some("log-open") if log => Some("sqlite-open"),
            other => other,
        };
        match fault {
            Some("pair-mismatch") => {
                let other = path.with_file_name("different.sqlite3");
                #[cfg(windows)]
                {
                    fs::copy(path, &other).unwrap();
                    other
                }
                #[cfg(unix)]
                {
                    fs::rename(path, &other).unwrap();
                    fs::copy(&other, path).unwrap();
                    path.to_owned()
                }
            }
            Some("sqlite-open") => path.with_file_name("absent-sqlite-open.sqlite3"),
            Some("mismatch") => {
                #[cfg(windows)]
                {
                    // Windows denies renaming the retained original. Open a real
                    // different private file instead, preserving the original guard.
                    let other = path.with_file_name("different.sqlite3");
                    fs::write(&other, b"").unwrap();
                    other
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::rename(path, path.with_file_name("retained-original.sqlite3")).unwrap();
                    fs::write(path, b"").unwrap();
                    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
                    path.to_owned()
                }
            }
            _ => path.to_owned(),
        }
    }
    thread_local! { static PAIR_MUTATION: std::cell::RefCell<Option<PathBuf>> = const {std::cell::RefCell::new(None)}; }
    pub(super) fn pair_final() {
        PAIR_MUTATION.with(|p| {
            if let Some(path) = p.borrow().as_ref() {
                fs::OpenOptions::new()
                    .write(true)
                    .open(path)
                    .unwrap()
                    .set_len(1)
                    .unwrap();
            }
        });
    }
    #[test]
    fn inspection_pair_gate_positive_and_failure_lifetimes() {
        for fault in [
            "healthy",
            "state-mismatch",
            "log-mismatch",
            "log-open",
            "second-panic",
        ] {
            let l = seeded();
            let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
            let before = l.snapshot();
            SQL.with(|n| n.set(0));
            FAULT.with(|f| {
                *f.borrow_mut() = match fault {
                    "healthy" => None,
                    "second-panic" => Some(("second", true)),
                    other => Some((other, false)),
                }
            });
            let mut stores = BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
            FAULT.with(|f| *f.borrow_mut() = None);
            assert_eq!(SQL.with(|n| n.get()), 0);
            if fault == "healthy" {
                assert_eq!(stores.status(), Ok(()));
                assert_eq!(stores.opens.len(), 2);
                stores
                    .inspect(|state, sf, log, lf| {
                        for (db, file, stem) in
                            [(state, sf, "cluster-state"), (log, lf, "cluster-log")]
                        {
                            assert!(db.is_readonly("main").unwrap());
                            assert_eq!(
                                guard_id(file).unwrap(),
                                guard_id(&File::open(stores_path(&l, stem)).unwrap()).unwrap()
                            );
                            application_sql();
                            assert_eq!(
                                db.pragma_query_value::<u32, _>(None, "user_version", |r| r
                                    .get(0))?,
                                2
                            );
                        }
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(SQL.with(|n| n.get()), 2);
            } else {
                let expected = match fault {
                    "log-open" => BoundFailure::Open(StoreError::Sql),
                    "second-panic" => BoundFailure::Unwind,
                    _ => BoundFailure::Identity,
                };
                assert_eq!(stores.status(), Err(expected));
                if fault != "state-mismatch" {
                    assert!(stores.inspection_state.is_some());
                }
                if fault.ends_with("mismatch") {
                    let db = stores.pending.db.as_mut().unwrap();
                    assert_ne!(
                        actual_id(db).unwrap(),
                        guard_id(stores.pending.guard.as_ref().unwrap()).unwrap()
                    );
                }
                assert_eq!(
                    stores.inspect::<()>(|_, _, _, _| panic!("rejected pair queried")),
                    Err(expected)
                );
            }
            probe(&stores.owner.plan.state, "cluster-state", true);
            if fault != "state-mismatch" {
                probe(&stores.owner.plan.state, "cluster-log", true);
            }
            let failure = stores.first_failure();
            drop(stores);
            probe(&owner.plan.state, "cluster-state", false);
            probe(&owner.plan.state, "cluster-log", false);
            probe(&owner.plan.state, "cluster-owner", true);
            let again = BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
            assert_eq!(
                again.status(),
                Err(failure.unwrap_or(BoundFailure::Unavailable))
            );
            assert_eq!(again.accesses, 0);
            drop(again);
            if !fault.ends_with("mismatch") {
                assert_eq!(l.snapshot(), before);
            }
        }
    }
    #[test]
    fn inspection_callback_unwind_retains_complete_pair_and_first_failure() {
        let l = seeded();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let mut stores = BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
        assert_eq!(stores.status(), Ok(()));
        assert_eq!(
            stores.inspect::<()>(|_, _, _, _| panic!("ordinary callback unwind")),
            Err(BoundFailure::Unwind)
        );
        probe(&stores.owner.plan.state, "cluster-state", true);
        probe(&stores.owner.plan.state, "cluster-log", true);
        drop(stores);
        probe(&owner.plan.state, "cluster-state", false);
        probe(&owner.plan.state, "cluster-log", false);
        let again = BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
        assert_eq!(again.status(), Err(BoundFailure::Unwind));
        assert_eq!(again.accesses, 0);
    }
    #[test]
    fn inspection_rejects_new_sidecars_without_repair() {
        for stem in ["cluster-state", "cluster-log"] {
            for suffix in ["-wal", "-journal"] {
                let l = seeded();
                let path = stores_path(&l, stem).with_file_name(format!("{stem}.sqlite3{suffix}"));
                if path.exists() {
                    fs::remove_file(&path).unwrap();
                }
                assert!(!path.exists());
                let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
                create_private_new(&path);
                let before = l.snapshot();
                SQL.with(|n| n.set(0));
                let stores = BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
                assert_eq!(
                    stores.status(),
                    Err(BoundFailure::Inspection(
                        super::super::startup_plan::Refusal::Layout
                    ))
                );
                assert_eq!(SQL.with(|n| n.get()), 0);
                drop(stores);
                assert_eq!(l.snapshot(), before);
            }
        }
    }
    #[test]
    fn inspection_exact_optional_masks_and_callback_layout_refusal() {
        for mask in 0..16 {
            let l = seeded();
            for (i, name) in [
                "cluster-state.sqlite3-wal",
                "cluster-state.sqlite3-journal",
                "cluster-log.sqlite3-wal",
                "cluster-log.sqlite3-journal",
            ]
            .iter()
            .enumerate()
            {
                let path = l.root.path().join("state").join(name);
                if path.exists() {
                    fs::remove_file(&path).unwrap();
                }
                if mask & (1 << i) != 0 {
                    l.write(&format!("state/{name}"), b"");
                }
            }
            let before = l.snapshot();
            let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
            let mut stores = BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
            assert_eq!(stores.status(), Ok(()), "mask {mask}");
            stores.inspect(|_, _, _, _| Ok(())).unwrap();
            drop(stores);
            assert_eq!(l.snapshot(), before, "mask {mask}");
        }
        let l = seeded();
        let path = stores_path(&l, "cluster-log").with_file_name("cluster-log.sqlite3-wal");
        assert!(!path.exists());
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let mut stores = BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
        assert_eq!(stores.status(), Ok(()));
        assert_eq!(
            stores.inspect(|_, _, _, _| {
                create_private_new(&path);
                Ok(())
            }),
            Err(BoundFailure::Inspection(
                super::super::startup_plan::Refusal::Layout
            ))
        );
        let before = l.snapshot();
        drop(stores);
        assert_eq!(l.snapshot(), before);
    }
    fn create_private_new(path: &Path) {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).unwrap();
    }
    fn stores_path(l: &super::super::startup_plan::tests::Layout, stem: &str) -> PathBuf {
        l.root.path().join(format!("state/{stem}.sqlite3"))
    }
    #[test]
    fn inspection_sidecar_policy_original_zero_and_exact_layout() {
        for stem in ["cluster-state", "cluster-log"] {
            for suffix in ["-wal", "-journal"] {
                for change in [
                    "none",
                    "grow",
                    "remove",
                    "replace",
                    "pair-grow",
                    "callback-grow",
                ] {
                    let l = seeded();
                    let path =
                        stores_path(&l, stem).with_file_name(format!("{stem}.sqlite3{suffix}"));
                    l.write(&format!("state/{stem}.sqlite3{suffix}"), b"");
                    let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
                    let before = l.snapshot();
                    if change == "grow" {
                        fs::OpenOptions::new()
                            .write(true)
                            .open(&path)
                            .unwrap()
                            .set_len(1)
                            .unwrap();
                    }
                    if change == "remove" || change == "replace" {
                        #[cfg(windows)]
                        {
                            let original = l.snapshot();
                            fs::remove_file(&path).unwrap_err();
                            if change == "replace" {
                                let replacement = path.with_extension("replacement");
                                l.write(&format!("state/{stem}.replacement"), b"");
                                fs::rename(&replacement, &path).unwrap_err();
                                fs::remove_file(&replacement).unwrap();
                            }
                            assert_eq!(l.snapshot(), original);
                            assert_eq!(owner.inspect_validate(), Ok(()));
                            continue;
                        }
                        #[cfg(unix)]
                        {
                            if change == "replace" {
                                let replacement = path.with_extension("replacement");
                                l.write(&format!("state/{stem}.replacement"), b"");
                                fs::rename(&replacement, &path).unwrap();
                            } else {
                                fs::remove_file(&path).unwrap();
                            }
                            assert!(owner.inspect_validate().is_err());
                            continue;
                        }
                    }

                    SQL.with(|n| n.set(0));
                    PAIR_MUTATION.with(|p| {
                        *p.borrow_mut() = (change == "pair-grow").then_some(path.clone())
                    });
                    let mut stores =
                        BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
                    FAULT.with(|f| *f.borrow_mut() = None);
                    PAIR_MUTATION.with(|p| *p.borrow_mut() = None);
                    assert_eq!(SQL.with(|n| n.get()), 0);
                    if change == "grow" || change == "pair-grow" {
                        assert_eq!(
                            stores.status(),
                            Err(BoundFailure::Inspection(
                                super::super::startup_plan::Refusal::Store
                            ))
                        );
                    } else {
                        assert_eq!(stores.status(), Ok(()));
                        let result = stores.inspect(|s, _, l, _| {
                            assert!(
                                s.is_readonly("main").unwrap() && l.is_readonly("main").unwrap()
                            );
                            if change == "callback-grow" {
                                fs::OpenOptions::new()
                                    .write(true)
                                    .open(&path)
                                    .unwrap()
                                    .set_len(1)
                                    .unwrap();
                            }
                            Ok(())
                        });
                        assert_eq!(
                            result,
                            if change == "callback-grow" {
                                Err(BoundFailure::Inspection(
                                    super::super::startup_plan::Refusal::Store,
                                ))
                            } else {
                                Ok(())
                            }
                        );
                    }
                    drop(stores);
                    if change == "none" {
                        assert_eq!(l.snapshot(), before);
                    }
                }
            }
            for suffix in ["-wal", "-journal", "-shm"] {
                let l = seeded();
                l.write(
                    &format!("state/{stem}.sqlite3{suffix}"),
                    if suffix == "-shm" { b"" } else { b"pending" },
                );
                let before = l.snapshot();
                assert!(matches!(
                    Owner::acquire(l.plan("create-founder")),
                    Err(super::super::startup_plan::Refusal::Store)
                ));
                assert_eq!(l.snapshot(), before);
            }
        }
    }
    fn init() -> Init {
        Init {
            cluster: [7; 16],
            first_free: std::num::NonZeroU64::new(1).unwrap(),
        }
    }
    fn layout() -> super::super::startup_plan::tests::Layout {
        super::super::startup_plan::tests::Layout::new()
    }
    #[test]
    fn retained_fresh_same_connections_and_spent_complete() {
        let l = layout();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        for name in [
            "cluster-state.lock",
            "cluster-log.lock",
            "cluster-state.sqlite3",
            "cluster-log.sqlite3",
        ] {
            assert!(!owner.plan.state.join(name).exists());
        }
        let stores = BoundStores::acquire(&mut owner, BoundIntent::Fresh(init()));
        assert_eq!(stores.status(), Ok(()));
        assert_eq!(stores.opens.len(), 2);
        assert_eq!(
            stores.state.as_ref().unwrap().bound_pointer(),
            stores.opens[0]
        );
        assert_eq!(
            stores.log.as_ref().unwrap().bound_pointer(),
            stores.opens[1]
        );
        let state = stores.state.as_ref().unwrap().snapshot();
        probe(&stores.owner.plan.state, "cluster-state", true);
        probe(&stores.owner.plan.state, "cluster-log", true);
        drop(stores);
        probe(&owner.plan.state, "cluster-state", false);
        probe(&owner.plan.state, "cluster-log", false);
        let again = BoundStores::acquire(&mut owner, BoundIntent::Fresh(init()));
        assert_eq!(again.first_failure(), Some(BoundFailure::Unavailable));
        assert_eq!(again.accesses, 0);
        drop(again);
        drop(owner);
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let stores = BoundStores::acquire(&mut owner, BoundIntent::UntouchedGenesis(init()));
        assert_eq!(stores.status(), Ok(()));
        assert_eq!(stores.state.as_ref().unwrap().snapshot(), state);
    }
    #[test]
    fn retained_errors_and_unwinds_keep_locks_and_original_first_failure() {
        for point in [
            "state-init",
            "borrowed-init",
            "second",
            "log-init",
            "preflight",
        ] {
            for unwind in [false, true] {
                let l = layout();
                let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
                REACHED.with(|r| r.borrow_mut().clear());
                FAULT.with(|f| *f.borrow_mut() = Some((point, unwind)));
                let stores = BoundStores::acquire(&mut owner, BoundIntent::Fresh(init()));
                FAULT.with(|f| *f.borrow_mut() = None);
                let first = if unwind {
                    BoundFailure::Unwind
                } else {
                    BoundFailure::Init(StoreError::Sql)
                };
                assert_eq!(stores.first_failure(), Some(first), "{point}");
                REACHED.with(|r| assert_eq!(*r.borrow(), vec![point]));
                probe(&stores.owner.plan.state, "cluster-owner", true);
                probe(&stores.owner.plan.state, "cluster-state", true);
                if matches!(point, "second" | "log-init" | "preflight") {
                    probe(&stores.owner.plan.state, "cluster-log", true);
                }
                drop(stores);
                probe(&owner.plan.state, "cluster-state", false);
                if matches!(point, "second" | "log-init" | "preflight") {
                    probe(&owner.plan.state, "cluster-log", false);
                }
                // Restore a valid-looking pair on disk without replacing the original Owner.
                drop(StateStore::open(&owner.plan.state, init()).unwrap());
                drop(VoteLogStore::open(&owner.plan.state, init().cluster).unwrap());
                let bytes = l.snapshot();
                let again = BoundStores::acquire(&mut owner, BoundIntent::Fresh(init()));
                assert_eq!(again.first_failure(), Some(first));
                assert_eq!(again.accesses, 0);
                assert!(again.opens.is_empty());
                drop(again);
                assert_eq!(l.snapshot(), bytes);
            }
        }
    }
    #[test]
    fn retained_unexpected_begun_and_pure_intent_refuse_without_access() {
        let l = layout();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let s = BoundStores::acquire(&mut owner, BoundIntent::Recovery(init().cluster));
        assert_eq!(s.status(), Err(BoundFailure::Intent));
        assert_eq!(s.accesses, 0);
        drop(s);
        assert_eq!(owner.bound_attempt, BoundAttempt::Ready);
        owner.bound_attempt = BoundAttempt::Begun;
        let s = BoundStores::acquire(&mut owner, BoundIntent::Fresh(init()));
        assert_eq!(s.status(), Err(BoundFailure::Unavailable));
        assert_eq!(s.accesses, 0);
    }
    #[test]
    fn retained_readonly_inspection_preserves_bytes_and_query_failure() {
        let l = layout();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        assert_eq!(
            BoundStores::acquire(&mut owner, BoundIntent::Fresh(init())).status(),
            Ok(())
        );
        drop(owner);
        let before = l.snapshot();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let mut stores = BoundStores::acquire(&mut owner, BoundIntent::Inspect(init().cluster));
        assert_eq!(stores.status(), Ok(()));
        assert_eq!(
            stores.inspect(|db, _, _, _| Ok(db.pragma_query_value::<u32, _>(
                None,
                "user_version",
                |r| r.get(0)
            )?)),
            Ok(2)
        );
        assert_eq!(
            stores.inspect(|db, _, _, _| {
                db.execute_batch("CREATE TABLE forbidden(x)")?;
                Ok(())
            }),
            Err(BoundFailure::Init(StoreError::Sql))
        );
        assert_eq!(
            stores.inspect(|_, _, _, _| panic!("must not access")),
            Err::<(), _>(BoundFailure::Init(StoreError::Sql))
        );
        drop(stores);
        assert_eq!(before, l.snapshot());
    }

    // These fixtures mutate the owned operation, never fabricate its error.
    fn real_failure(
        owner: &mut Owner,
        point: &'static str,
        intent: BoundIntent,
        expected: BoundFailure,
        both: bool,
    ) -> usize {
        SQL.with(|n| n.set(0));
        FAULT.with(|f| *f.borrow_mut() = Some((point, false)));
        let stores = BoundStores::acquire(owner, intent);
        FAULT.with(|f| *f.borrow_mut() = None);
        assert_eq!(stores.status(), Err(expected), "{point}");
        probe(&stores.owner.plan.state, "cluster-owner", true);
        probe(&stores.owner.plan.state, "cluster-state", true);
        if both {
            probe(&stores.owner.plan.state, "cluster-log", true);
        }
        match point {
            "sqlite-open" => {
                assert!(stores.pending.guard.is_some());
                assert!(stores.pending.db.is_none());
                assert!(stores.opens.is_empty());
            }
            "mismatch" => {
                assert!(stores.pending.guard.is_some());
                assert!(stores.pending.db.is_some());
                assert_eq!(stores.opens.len(), 1);
                let actual = guard_id(
                    &File::open(sqlite_open_path_for_witness(&stores.owner.plan.state)).unwrap(),
                )
                .unwrap();
                assert_ne!(
                    actual,
                    guard_id(stores.pending.guard.as_ref().unwrap()).unwrap()
                );
            }
            "staging" => {
                assert!(stores.state.is_some() && stores.log.is_some());
                assert_eq!(stores.opens.len(), 2);
            }
            _ => {
                assert!(stores.pending.bound.is_some());
                assert_eq!(stores.state.is_some(), both);
            }
        }
        let sql = SQL.with(|n| n.get());
        drop(stores);
        probe(&owner.plan.state, "cluster-owner", true);
        probe(&owner.plan.state, "cluster-state", false);
        if both {
            probe(&owner.plan.state, "cluster-log", false);
        }
        let before = super::super::startup_plan::tests::objects(&owner.plan.state);
        let retry = BoundStores::acquire(owner, intent);
        assert_eq!(retry.first_failure(), Some(expected));
        assert_eq!(retry.accesses, 0);
        assert!(retry.opens.is_empty());
        drop(retry);
        assert_eq!(
            before,
            super::super::startup_plan::tests::objects(&owner.plan.state)
        );
        assert_eq!(SQL.with(|n| n.get()), sql);
        sql
    }
    fn sqlite_open_path_for_witness(dir: &Path) -> PathBuf {
        #[cfg(windows)]
        {
            dir.join("different.sqlite3")
        }
        #[cfg(unix)]
        {
            dir.join("cluster-state.sqlite3")
        }
    }
    #[test]
    fn retained_actual_sqlite_open_failure_keeps_guard_and_lock() {
        let l = layout();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        assert_eq!(
            real_failure(
                &mut owner,
                "sqlite-open",
                BoundIntent::Fresh(init()),
                BoundFailure::Open(StoreError::Sql),
                false
            ),
            0
        );
    }
    #[test]
    fn retained_actual_configure_and_schema_failures_keep_owned_connections() {
        for point in ["state-pragma", "log-pragma", "state-schema", "log-schema"] {
            let l = layout();
            let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
            assert!(
                real_failure(
                    &mut owner,
                    point,
                    BoundIntent::Fresh(init()),
                    BoundFailure::Init(StoreError::Sql),
                    point.starts_with("log")
                ) > 0
            );
        }
    }
    #[test]
    fn retained_actual_install_refusal_keeps_both_stores_and_original_baseline() {
        let l = layout();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let baseline = super::super::startup_plan::tests::baseline(&owner);
        real_failure(
            &mut owner,
            "staging",
            BoundIntent::Fresh(init()),
            BoundFailure::Layout,
            true,
        );
        assert_eq!(
            super::super::startup_plan::tests::baseline(&owner),
            baseline
        );
    }
    #[test]
    fn retained_constructor_mismatch_refuses_before_any_application_sql() {
        let l = layout();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        assert_eq!(
            real_failure(
                &mut owner,
                "mismatch",
                BoundIntent::Fresh(init()),
                BoundFailure::Identity,
                false
            ),
            0
        );
        drop(owner);
        // Also exercise the read-only constructor before any trusted version query.
        let l = seeded();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        assert_eq!(
            real_failure(
                &mut owner,
                "mismatch",
                BoundIntent::Inspect(init().cluster),
                BoundFailure::Identity,
                false
            ),
            0
        );
    }

    #[test]
    fn operational_failures_and_unwind_retain_actual_slots_without_retry() {
        for (point, panic) in [
            ("sqlite-open", false),
            ("state-pragma", false),
            ("state-schema", false),
            ("log-init", false),
            ("log-init", true),
        ] {
            let l = layout();
            let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
            FAULT.with(|f| *f.borrow_mut() = Some((point, panic)));
            let (mut slots, observed) = BoundStores::operational(
                &mut owner,
                BoundIntent::Fresh(init()),
                |_| Ok(()),
                |_, _, _| Ok(()),
            );
            FAULT.with(|f| *f.borrow_mut() = None);
            assert!(observed.is_none());
            assert!(slots.take_pair().is_err());
            probe(&owner.plan.state, "cluster-state", true);
            if point.starts_with("log") {
                probe(&owner.plan.state, "cluster-log", true);
            }
            let first = slots.failure;
            let (mut retry, _) = BoundStores::operational(
                &mut owner,
                BoundIntent::Fresh(init()),
                |_| -> Result<(), BoundFailure> { panic!("must not retry") },
                |_, _, _| Ok(()),
            );
            assert_eq!(retry.failure, first);
            assert!(retry.take_pair().is_err());
            drop(retry);
            drop(slots);
            probe(&owner.plan.state, "cluster-state", false);
        }
    }
    fn seeded() -> super::super::startup_plan::tests::Layout {
        let l = layout();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        assert_eq!(
            BoundStores::acquire(&mut owner, BoundIntent::Fresh(init())).status(),
            Ok(())
        );
        l
    }
    fn recovery(l: &super::super::startup_plan::tests::Layout) -> Owner {
        // Layout-only synthetic public fixtures; no credential parser or live startup.
        for name in ["identity.json", "ca.der", "receipt"] {
            l.write(&format!("credentials/{name}"), b"public-layout-fixture");
        }
        Owner::acquire(l.plan("recover-founder")).unwrap()
    }
    #[test]
    fn retained_existing_pairs_recovery_and_same_connection_readback() {
        let l = seeded();
        let dir = l.root.path().join("state");
        let position = super::super::state_store::LogPos {
            term: 1,
            leader: 1,
            index: 0,
        };
        let entry = super::super::state_store::Entry::Blank;
        {
            let mut state = StateStore::open(&dir, init()).unwrap();
            state.apply(position, &entry).unwrap();
            let mut log = VoteLogStore::open(&dir, init().cluster).unwrap();
            log.append(&[(position, entry)]).unwrap();
        }
        let mut owner = recovery(&l);
        let mut stores = BoundStores::acquire(&mut owner, BoundIntent::Recovery(init().cluster));
        assert_eq!(stores.status(), Ok(()));
        assert_eq!(
            stores.state.as_ref().unwrap().bound_pointer(),
            stores.opens[0]
        );
        assert_eq!(
            stores.log.as_ref().unwrap().bound_pointer(),
            stores.opens[1]
        );
        stores
            .validate_pair(|state, log| {
                assert_eq!(state.applied(), Some(position));
                assert_eq!(log.entries(0, 1, 1)?.len(), 1);
                Ok(())
            })
            .unwrap();
    }
    #[test]
    fn retained_nonfresh_version_zero_refused_preserving_bytes_and_layout() {
        for recover in [false, true] {
            for stem in ["cluster-state", "cluster-log"] {
                let l = seeded();
                let path = l.root.path().join(format!("state/{stem}.sqlite3"));
                let db = open(&path);
                db.pragma_update(None, "user_version", 0).unwrap();
                drop(db);
                let mut owner = if recover {
                    recovery(&l)
                } else {
                    Owner::acquire(l.plan("create-founder")).unwrap()
                };
                let before = l.snapshot();
                let objects = super::super::startup_plan::tests::objects(&owner.plan.state);
                let intent = if recover {
                    BoundIntent::Recovery(init().cluster)
                } else {
                    BoundIntent::UntouchedGenesis(init())
                };
                let stores = BoundStores::acquire(&mut owner, intent);
                assert_eq!(
                    stores.status(),
                    Err(BoundFailure::Init(StoreError::Unsupported)),
                    "{recover} {stem}"
                );
                drop(stores);
                assert_eq!(l.snapshot(), before);
                assert_eq!(
                    super::super::startup_plan::tests::objects(&owner.plan.state),
                    objects
                );
            }
        }
    }
    #[test]
    fn retained_genesis_requires_entire_enrollment_image() {
        let l = seeded();
        let mut other = init();
        other.first_free = std::num::NonZeroU64::new(2).unwrap();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let before = l.snapshot();
        let stores = BoundStores::acquire(&mut owner, BoundIntent::UntouchedGenesis(other));
        assert_eq!(stores.status(), Err(BoundFailure::Layout));
        drop(stores);
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn retained_recovery_incompatible_purged_metadata_refused() {
        let l = seeded();
        let dir = l.root.path().join("state");
        let mut log = VoteLogStore::open(&dir, init().cluster).unwrap();
        log.purge(super::super::state_store::LogPos {
            term: 1,
            leader: 1,
            index: 1,
        })
        .unwrap();
        drop(log);
        let mut owner = recovery(&l);
        let before = l.snapshot();
        let stores = BoundStores::acquire(&mut owner, BoundIntent::Recovery(init().cluster));
        assert_eq!(
            stores.status(),
            Err(BoundFailure::Init(StoreError::Corrupt))
        );
        probe(&stores.owner.plan.state, "cluster-state", true);
        probe(&stores.owner.plan.state, "cluster-log", true);
        drop(stores);
        probe(&owner.plan.state, "cluster-state", false);
        probe(&owner.plan.state, "cluster-log", false);
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn fresh_creation_rejects_existing_leaf_and_unsafe_names() {
        let l = layout();
        let owner = Owner::acquire(l.plan("create-founder")).unwrap();
        for name in [
            "../escape",
            "cluster-state.sqlite3/child",
            "cluster-state.sqlite3:stream",
            "unknown",
        ] {
            assert!(owner.bound_create(name).is_err(), "{name}");
        }
        let path = owner.plan.state.join("cluster-state.lock");
        fs::write(&path, b"sentinel").unwrap();
        assert!(owner.bound_create("cluster-state.lock").is_err());
        assert_eq!(fs::read(path).unwrap(), b"sentinel");
    }
    #[test]
    fn fresh_creation_rejects_reparse_leaf_and_parent() {
        let l = layout();
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        let target = l.root.path().join("workspace");
        let link = owner.plan.state.join("cluster-state.lock");
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&link)
                .arg(&target)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "junction fixture prerequisite unavailable"
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(owner.bound_create("cluster-state.lock").is_err());
        owner.plan.state = link;
        assert!(owner.bound_create("cluster-log.lock").is_err());
        assert!(!target.join("cluster-log.lock").exists());
    }
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, File) {
        let dir = crate::service::application::tests::private_temp();
        let p = dir.path().join("proof.sqlite3");
        let f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&p)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        }
        (dir, p, f)
    }
    fn open(p: &Path) -> Connection {
        Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap()
    }
    #[test]
    fn abi_format_refusals_are_not_identity_acceptance() {
        let a = initial();
        assert!(decode(0, &a, 1).is_err());
        let mut valid = initial();
        valid.status = 0;
        valid.vfs_kind = 1;
        valid.nlink_lo = 1;
        valid.mode = 0o100600;
        assert_eq!(decode(0, &valid, 1), Ok(ObjectId(0, 0)));
        for index in 0..14 {
            let mut b = valid;
            match index {
                0 => b.magic = 0,
                1 => b.abi_version = 2,
                2 => b.struct_bytes = 79,
                3 => b.status = 1,
                4 => b.status = 2,
                5 => b.status = 3,
                6 => b.status = u32::MAX,
                7 => b.os_errno = 1,
                8 => b.vfs_kind = 3,
                9 => b.flags = 1,
                10 => b.reserved[5] = 1,
                11 => b.nlink_lo = 2,
                12 => b.mode = 0o040700,
                _ => b.mode = 0o100644,
            }
            assert!(decode(0, &b, 1).is_err(), "field {index}");
        }
        assert!(decode(ffi::SQLITE_NOTFOUND, &valid, 1).is_err());
        assert!(decode(0, &valid, 0).is_err());
        assert_eq!(words(u32::MAX, 0x80000000), 0x80000000ffffffff);
    }
    #[test]
    fn stable_actual_connection_identity() {
        let (_d, p, f) = fixture();
        let mut db = open(&p);
        let pointer = unsafe { db.handle() };
        let id = verify(&mut db, &f, &p).unwrap();
        assert_eq!(id, guard_id(&f).unwrap());
        assert_eq!(verify(&mut db, &f, &p).unwrap(), id);
        assert_eq!(unsafe { db.handle() }, pointer);
    }
    #[test]
    fn original_guard_vs_other_actual_sqlite_file_refuses() {
        let (_a, p, f) = fixture();
        let (_b, q, g) = fixture();
        assert_ne!(guard_id(&f).unwrap(), guard_id(&g).unwrap());
        let mut db = open(&q);
        assert!(verify(&mut db, &f, &p).is_err());
        assert!(verify(&mut db, &g, &q).is_ok());
    }
    #[test]
    fn real_hardlink_refuses() {
        let (d, p, f) = fixture();
        let mut db = open(&p);
        assert!(verify(&mut db, &f, &p).is_ok());
        fs::hard_link(&p, d.path().join("alias")).unwrap();
        assert!(verify(&mut db, &f, &p).is_err());
    }
    #[test]
    fn real_directory_type_refuses() {
        let (d, _, _) = fixture();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            };
            options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let directory = options.open(d.path()).unwrap();
        assert!(guard_id(&directory).is_err());
    }
    #[test]
    fn read_only_actual_connection_identity() {
        let (_d, p, f) = fixture();
        let mut db =
            Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let pointer = unsafe { db.handle() };
        assert_eq!(verify(&mut db, &f, &p).unwrap(), guard_id(&f).unwrap());
        assert_eq!(unsafe { db.handle() }, pointer);
        assert_eq!(fs::read(&p).unwrap(), b"");
    }
    #[test]
    fn memory_vfs_and_unknown_opcode_refuse() {
        let mut memory = Connection::open_in_memory().unwrap();
        assert!(qualify(&mut memory).is_err());
        let (_d, p, _) = fixture();
        let db = open(&p);
        let mut a = initial();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                0x41525832,
                std::ptr::from_mut(&mut a).cast(),
            )
        };
        assert_eq!(rc, ffi::SQLITE_NOTFOUND);
        assert_eq!(a.status, u32::MAX);
    }
    #[cfg(unix)]
    #[test]
    fn actual_unix_swap_and_privacy_refuse() {
        use std::os::unix::fs::PermissionsExt;
        let (d, p, f) = fixture();
        fs::rename(&p, d.path().join("original")).unwrap();
        fs::write(&p, b"").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        let mut db = open(&p);
        assert!(verify(&mut db, &f, &p).is_err());
        let current = File::open(&p).unwrap();
        assert!(verify(&mut db, &current, &p).is_ok());
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(verify(&mut db, &current, &p).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn actual_unix_abi_header_and_unsupported_alias_refuse() {
        let (_d, p, _) = fixture();
        let db = open(&p);
        let mut a = initial();
        a.magic = 0;
        let rc = unsafe {
            ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                ffi::ARX_FCNTL_IDENTITY_V1,
                std::ptr::from_mut(&mut a).cast(),
            )
        };
        assert_eq!(rc, ffi::SQLITE_MISMATCH);
        assert_eq!(a.status, u32::MAX);
        let mut alias = Connection::open_with_flags_and_vfs(
            &p,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
            "unix-none",
        )
        .unwrap();
        assert!(qualify(&mut alias).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_swap_denial_is_prevention_only() {
        let (d, p, f) = fixture();
        let mut db = open(&p);
        let error = fs::rename(&p, d.path().join("moved")).unwrap_err();
        assert_eq!(
            error.raw_os_error(),
            Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32),
            "{error:?}"
        );
        assert!(verify(&mut db, &f, &p).is_ok());
    }
    #[cfg(windows)]
    #[test]
    fn windows_actual_alias_vfs_refuses() {
        let (_d, p, _) = fixture();
        let mut alias = Connection::open_with_flags_and_vfs(
            &p,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
            "win32-longpath",
        )
        .unwrap();
        assert!(qualify(&mut alias).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_real_privacy_change_refuses() {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{Authorization::*, *},
        };
        let (_d, p, f) = fixture();
        let mut db = open(&p);
        assert!(verify(&mut db, &f, &p).is_ok());
        // Change only this disposable file to an Everyone grant; no admin needed.
        let text: Vec<u16> = "D:P(A;;FA;;;WD)".encode_utf16().chain(Some(0)).collect();
        unsafe {
            let mut sd = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    1,
                    &mut sd,
                    ptr::null_mut()
                ),
                0
            );
            let (mut present, mut defaulted) = (0, 0);
            let mut acl = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted),
                0
            );
            let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
            let status = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            );
            LocalFree(sd);
            assert_eq!(status, 0);
        }
        assert!(crate::security::private(&p).is_err());
        assert!(verify(&mut db, &f, &p).is_err());
    }
}

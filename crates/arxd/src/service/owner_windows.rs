//! Callable ADR0012 adapter. No CLI, SCM, installation or dispatch wiring.
use super::{
    config::{MAX_CONFIG_BYTES, ServiceConfig},
    layout::{self, Error, Layout, Platform, Policy},
    native,
    owner_scm::Deregistered,
    owner_transactions::{
        self, BatchObservation, BatchRight, Configure, Failure, OwnerRecord, OwnerStore,
        RightReport,
    },
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authentication::Identity::*, Authorization::*, *},
    Storage::FileSystem::*,
    System::Threading::*,
};

// FILE_RENAME_INFO_EX flags (not exported by windows-sys 0.61).
const RENAME_REPLACE_IF_EXISTS: u32 = 0x1;
const RENAME_POSIX_SEMANTICS: u32 = 0x2;
const PRIVATE_DESCRIPTOR: &str = "O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)";
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn digest(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}
struct Local(*mut core::ffi::c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn descriptor(text: &str) -> Result<Local, Failure> {
    let mut sd = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(text).as_ptr(),
            1,
            &mut sd,
            null_mut(),
        )
    } == 0
    {
        return Err(Failure::Io);
    }
    Ok(Local(sd))
}
/// Fixed tokens only, including native session preconditions.
#[derive(Debug)]
pub enum OpenError {
    Layout(Error),
    Store(Failure),
}
impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Layout(e) => e.fmt(f),
            Self::Store(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for OpenError {}
impl From<Error> for OpenError {
    fn from(e: Error) -> Self {
        Self::Layout(e)
    }
}
impl From<Failure> for OpenError {
    fn from(e: Failure) -> Self {
        Self::Store(e)
    }
}

// No runtime identity or layout selector. Fixture policy exists only in test builds.
struct ProtectionPolicy {
    #[cfg(test)]
    fixture_owner: Option<String>,
}
impl ProtectionPolicy {
    fn production() -> Self {
        Self {
            #[cfg(test)]
            fixture_owner: None,
        }
    }
    fn check(&self, f: &File, directory: bool) -> Result<(), Failure> {
        structural(f, directory)?;
        let p = native::windows_protection(f).map_err(|_| Failure::RecoveryRequired)?;
        #[cfg(test)]
        if let Some(who) = &self.fixture_owner {
            use layout::{Ace, Protection};
            return match p {
                Protection::Windows {
                    owner,
                    protected: true,
                    dacl_present: true,
                    aces,
                } if owner == *who
                    && !aces.is_empty()
                    && aces
                        .iter()
                        .all(|a| matches!(a,Ace::Allow{sid,..} if sid==who)) =>
                {
                    Ok(())
                }
                _ => Err(Failure::RecoveryRequired),
            };
        }
        layout::check_protection(
            Platform::Windows,
            if directory {
                Policy::PrivateDirectory
            } else {
                Policy::PrivateFile
            },
            true,
            &p,
        )
        .map_err(|_| Failure::RecoveryRequired)
    }
    fn creation_descriptor(&self) -> String {
        #[cfg(test)]
        if let Some(who) = &self.fixture_owner {
            return format!("O:{who}D:P(A;;FA;;;{who})");
        }
        PRIVATE_DESCRIPTOR.into()
    }
}
fn raw_open(
    path: &Path,
    access: u32,
    share: u32,
    create: u32,
    sd: Option<&Local>,
) -> Result<File, Failure> {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.map_or(null_mut(), |s| s.0),
        bInheritHandle: 0,
    };
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            share,
            if sd.is_some() { &sa } else { null() },
            create,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(match unsafe { GetLastError() } {
            ERROR_SHARING_VIOLATION => Failure::Busy,
            _ => Failure::Io,
        });
    }
    Ok(unsafe { File::from_raw_handle(h) })
}
fn structural(f: &File, directory: bool) -> Result<(), Failure> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut info) } == 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && info.nNumberOfLinks != 1)
    {
        return Err(Failure::RecoveryRequired);
    }
    Ok(())
}
fn identity(f: &File) -> Result<(u64, [u8; 16]), Failure> {
    let mut id = FILE_ID_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            f.as_raw_handle(),
            FileIdInfo,
            (&mut id as *mut FILE_ID_INFO).cast(),
            std::mem::size_of_val(&id) as u32,
        )
    } == 0
    {
        return Err(Failure::Io);
    }
    Ok((id.VolumeSerialNumber, id.FileId.Identifier))
}
fn read(f: &mut File, cap: usize, policy: &ProtectionPolicy) -> Result<Vec<u8>, Failure> {
    policy.check(f, false)?;
    f.seek(SeekFrom::Start(0)).map_err(|_| Failure::Io)?;
    let mut b = Vec::new();
    Read::by_ref(f)
        .take((cap + 1) as u64)
        .read_to_end(&mut b)
        .map_err(|_| Failure::Io)?;
    policy.check(f, false)?;
    if b.len() > cap {
        return Err(Failure::RecoveryRequired);
    }
    Ok(b)
}
fn optional(path: &Path, access: u32, share: u32) -> Result<Option<File>, Failure> {
    // Capture absence from the same CreateFile call; never use exists()/metadata.
    match raw_open(path, access, share, OPEN_EXISTING, None) {
        Ok(f) => Ok(Some(f)),
        Err(e) => {
            if unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}
fn discard(f: &File) -> Result<(), Failure> {
    let flags: u32 = FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS;
    if unsafe {
        SetFileInformationByHandle(
            f.as_raw_handle(),
            FileDispositionInfoEx,
            (&flags as *const u32).cast(),
            4,
        )
    } == 0
    {
        Err(Failure::Io)
    } else {
        Ok(())
    }
}
fn pin_chain(path: &Path, final_policy: Policy) -> Result<Vec<File>, OpenError> {
    walk_chain(
        path,
        final_policy,
        &|f| {
            layout::check_protection(
                Platform::Windows,
                final_policy,
                true,
                &native::windows_protection(f)?,
            )
        },
        &layout::check_windows_ancestor,
    )
}

fn pin_private_dirs<P: AsRef<Path>>(
    paths: impl IntoIterator<Item = P>,
    policy: &ProtectionPolicy,
    check_ancestor: impl Fn(&layout::Protection) -> Result<(), Error>,
) -> Result<Vec<File>, OpenError> {
    let mut pins = Vec::new();
    for path in paths {
        pins.extend(
            walk_chain(
                path.as_ref(),
                Policy::PrivateDirectory,
                &|f| policy.check(f, true).map_err(|_| Error::Permissions),
                &check_ancestor,
            )
            .map_err(|error| match error {
                // The private-directory admission contract uses Permissions; the
                // binary pin walk retains its original structural store error.
                OpenError::Store(Failure::RecoveryRequired) => Error::Permissions.into(),
                other => other,
            })?,
        );
    }
    Ok(pins)
}

fn walk_chain(
    path: &Path,
    final_policy: Policy,
    check_leaf: &impl Fn(&File) -> Result<(), Error>,
    check_ancestor: &impl Fn(&layout::Protection) -> Result<(), Error>,
) -> Result<Vec<File>, OpenError> {
    native::components(path, true)?;
    let mut paths = path.ancestors().collect::<Vec<_>>();
    paths.reverse();
    let mut pins = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        if p.as_os_str().is_empty() {
            continue;
        }
        let leaf = i == paths.len() - 1;
        if i == 0
            && unsafe {
                GetDriveTypeW(
                    p.as_os_str()
                        .encode_wide()
                        .chain(Some(0))
                        .collect::<Vec<_>>()
                        .as_ptr(),
                )
            } != 3
        {
            return Err(Error::Layout.into());
        }
        let directory = !leaf || final_policy != Policy::BinaryIntegrity;
        let f = raw_open(
            p,
            READ_CONTROL | FILE_READ_DATA | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ
                | if leaf && directory {
                    FILE_SHARE_WRITE
                } else {
                    0
                },
            OPEN_EXISTING,
            None,
        )?;
        structural(&f, directory)?;
        if leaf {
            check_leaf(&f)?;
        } else {
            check_ancestor(&native::windows_protection(&f)?)?;
        }
        pins.push(f);
    }
    Ok(pins)
}

/// Holds authority for the entire transaction; lock closes after staging/pins.
pub struct WindowsOwnerSession {
    store: WindowsOwnerStore,
    right: WindowsBatchRight,
}
/// Constructible only as part of a validated owner session.
struct WindowsOwnerStore {
    staged: Option<Staged>,
    root: PathBuf,
    layout: Layout,
    policy: ProtectionPolicy,
    _pins: Vec<File>,
    _lock: File,
}
struct Staged {
    file: File,
    target: PathBuf,
    expected: Option<[u8; 32]>,
    bytes: Vec<u8>,
    old: Option<File>,
    cap: usize,
}
/// Binary-only authority; construction never enables privilege or opens a store.
pub struct OwnerGuard {
    layout: Layout,
    pins: Vec<File>,
}
impl OwnerGuard {
    pub fn acquire() -> Result<Self, OpenError> {
        owner_required()?;
        let layout = Layout::system()?;
        let mut pins = pin_chain(layout.binary(), Policy::BinaryIntegrity)?;
        let image = raw_open(
            &std::env::current_exe().map_err(|_| Error::Layout)?,
            READ_CONTROL | FILE_READ_DATA,
            FILE_SHARE_READ,
            OPEN_EXISTING,
            None,
        )?;
        if identity(&image)? != identity(pins.last().ok_or(Error::Layout)?)? {
            return Err(Error::Layout.into());
        }
        pins.push(image);
        Ok(Self { layout, pins })
    }
}
impl WindowsOwnerSession {
    pub fn open(guard: OwnerGuard) -> Result<Self, OpenError> {
        let OwnerGuard { layout, mut pins } = guard;
        let policy = ProtectionPolicy::production();
        pins.extend(pin_private_dirs(
            layout.private_dirs(),
            &policy,
            layout::check_windows_ancestor,
        )?);
        enable_restore()?;
        let root = layout.config().parent().ok_or(Error::Layout)?.to_owned();
        let lock = optional(&root.join("owner.lock"), READ_CONTROL | FILE_READ_DATA, 0)?
            .ok_or(Error::Layout)?;
        let store = WindowsOwnerStore::with_lock(root, layout, policy, pins, lock)?;
        let mut session = Self {
            store,
            right: WindowsBatchRight { _private: () },
        };
        owner_transactions::recover(&mut session.store, &mut session.right)?;
        Ok(session)
    }
    pub fn read_config(&mut self) -> Result<Option<Vec<u8>>, Failure> {
        self.store.read_config()
    }
    pub fn observe_batch(&mut self, sid: &str) -> Result<BatchObservation, Failure> {
        owner_transactions::observe_batch(sid, &mut self.store, &mut self.right)
    }
    pub fn configure(&mut self, req: &Configure) -> Result<(), Failure> {
        owner_transactions::configure(req, &mut self.store, &mut self.right)
    }
    pub fn uninstall(guard: OwnerGuard, proof: Deregistered) -> Result<RightReport, OpenError> {
        let OwnerGuard { layout, mut pins } = guard;
        let policy = ProtectionPolicy::production();
        pins.extend(pin_private_dirs(
            layout.private_dirs(),
            &policy,
            layout::check_windows_ancestor,
        )?);
        enable_restore()?;
        let root = layout.config().parent().ok_or(Error::Layout)?.to_owned();
        let lock = optional(&root.join("owner.lock"), READ_CONTROL | FILE_READ_DATA, 0)?
            .ok_or(Error::Layout)?;
        let mut store = WindowsOwnerStore::with_lock(root, layout, policy, pins, lock)?;
        Ok(uninstall_locked(
            proof,
            &mut store,
            &mut WindowsBatchRight { _private: () },
        )?)
    }
}
fn uninstall_locked(
    _proof: Deregistered,
    store: &mut impl OwnerStore,
    right: &mut impl BatchRight,
) -> Result<RightReport, Failure> {
    owner_transactions::recover(store, right)?;
    owner_transactions::uninstall_right(store, right)
}
impl WindowsOwnerStore {
    #[cfg(test)]
    fn locked(
        root: PathBuf,
        layout: Layout,
        policy: ProtectionPolicy,
        pins: Vec<File>,
    ) -> Result<Self, Failure> {
        let lock = raw_open(
            &root.join("owner.lock"),
            READ_CONTROL | FILE_READ_DATA,
            0,
            OPEN_EXISTING,
            None,
        )?;
        Self::with_lock(root, layout, policy, pins, lock)
    }
    fn with_lock(
        root: PathBuf,
        layout: Layout,
        policy: ProtectionPolicy,
        pins: Vec<File>,
        lock: File,
    ) -> Result<Self, Failure> {
        policy.check(&lock, false)?;
        if lock.metadata().map_err(|_| Failure::Io)?.len() != 0 {
            return Err(Failure::RecoveryRequired);
        }
        let mut s = Self {
            staged: None,
            root,
            layout,
            policy,
            _pins: pins,
            _lock: lock,
        };
        s.cleanup("owner-record.json.staging")?;
        s.cleanup("arxd.json.staging")?;
        Ok(s)
    }
    /// Exact protected bytes only; parsing the config mirror grants no authority.
    pub fn read_config(&mut self) -> Result<Option<Vec<u8>>, Failure> {
        let Some(mut file) = optional(
            &self.root.join("arxd.json"),
            GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
        )?
        else {
            return Ok(None);
        };
        read(&mut file, MAX_CONFIG_BYTES, &self.policy).map(Some)
    }
    fn cleanup(&mut self, name: &str) -> Result<(), Failure> {
        if let Some(f) = optional(
            &self.root.join(name),
            READ_CONTROL | FILE_READ_DATA | DELETE,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
        )? {
            self.policy.check(&f, false)?;
            discard(&f)?;
        }
        Ok(())
    }
    fn target(
        &self,
        path: &Path,
        expected: Option<[u8; 32]>,
        cap: usize,
    ) -> Result<Option<File>, Failure> {
        let mut f = optional(
            path,
            GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
        )?;
        match (expected, f.as_mut()) {
            (None, None) => {}
            (Some(h), Some(f)) => {
                if digest(&read(f, cap, &self.policy)?) != h {
                    return Err(Failure::Conflict);
                }
            }
            _ => return Err(Failure::Conflict),
        }
        Ok(f)
    }
    fn stage(
        &self,
        name: &str,
        bytes: &[u8],
        expected: Option<[u8; 32]>,
        cap: usize,
    ) -> Result<Staged, Failure> {
        if bytes.len() > cap {
            return Err(Failure::RecoveryRequired);
        }
        let target = self.root.join(name);
        let sd = descriptor(&self.policy.creation_descriptor())?;
        let mut file = raw_open(
            &self.root.join(format!("{name}.staging")),
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL | DELETE,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            CREATE_NEW,
            Some(&sd),
        )?;
        let prepared = (|| {
            self.policy.check(&file, false)?;
            file.write_all(bytes).map_err(|_| Failure::Io)?;
            file.sync_all().map_err(|_| Failure::Io)?;
            self.policy.check(&file, false)?;
            self.target(&target, expected, cap)
        })();
        match prepared {
            Ok(old) => Ok(Staged {
                file,
                target,
                expected,
                bytes: bytes.into(),
                old,
                cap,
            }),
            Err(e) => {
                self.policy.check(&file, false)?;
                discard(&file)?;
                Err(e)
            }
        }
    }
    fn publish(&self, mut s: Staged) -> Result<(), Failure> {
        let mut rename_attempted = false;
        let result = (|| {
            self.policy.check(&s.file, false)?;
            if read(&mut s.file, s.cap, &self.policy)? != s.bytes {
                return Err(Failure::Conflict);
            }
            let staging_path = self.root.join(format!(
                "{}.staging",
                s.target
                    .file_name()
                    .and_then(|v| v.to_str())
                    .ok_or(Failure::Io)?
            ));
            let source = raw_open(
                &staging_path,
                READ_CONTROL | FILE_READ_DATA,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                OPEN_EXISTING,
                None,
            )?;
            self.policy.check(&source, false)?;
            if identity(&source)? != identity(&s.file)? {
                return Err(Failure::Conflict);
            }
            let fresh = self.target(&s.target, s.expected, s.cap)?;
            if fresh.as_ref().map(identity).transpose()?
                != s.old.as_ref().map(identity).transpose()?
            {
                return Err(Failure::Conflict);
            }
            let id = identity(&s.file)?;
            let name: Vec<u16> = s.target.as_os_str().encode_wide().collect();
            let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
            let size = offset + name.len() * 2;
            let mut buf = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
            let info = buf.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            unsafe {
                (*info).Anonymous.Flags = if s.expected.is_some() {
                    RENAME_REPLACE_IF_EXISTS | RENAME_POSIX_SEMANTICS
                } else {
                    0
                };
                (*info).RootDirectory = null_mut();
                (*info).FileNameLength = (name.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    std::ptr::addr_of_mut!((*info).FileName).cast(),
                    name.len(),
                );
                rename_attempted = true;
                if SetFileInformationByHandle(
                    s.file.as_raw_handle(),
                    FileRenameInfoEx,
                    info.cast(),
                    size as u32,
                ) == 0
                {
                    return Err(Failure::Io);
                }
            }
            // Once rename starts every error has unknown publication outcome.
            (|| {
                let mut published = raw_open(
                    &s.target,
                    GENERIC_READ | READ_CONTROL,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    OPEN_EXISTING,
                    None,
                )?;
                if identity(&published)? != id
                    || read(&mut published, s.cap, &self.policy)? != s.bytes
                {
                    return Err(Failure::Io);
                }
                Ok(())
            })()
            .map_err(|_| Failure::Io)
        })();
        if result.is_err() && !rename_attempted {
            self.policy.check(&s.file, false)?;
            discard(&s.file)?;
        }
        result
    }
}
impl OwnerStore for WindowsOwnerStore {
    fn record(&mut self) -> Result<Option<(OwnerRecord, [u8; 32])>, Failure> {
        let Some(mut f) = optional(
            &self.root.join("owner-record.json"),
            GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
        )?
        else {
            return Ok(None);
        };
        let b = read(&mut f, 512, &self.policy)?;
        Ok(Some((OwnerRecord::decode(&b)?, digest(&b))))
    }
    fn replace_record(
        &mut self,
        expected: Option<[u8; 32]>,
        next: &OwnerRecord,
    ) -> Result<[u8; 32], Failure> {
        let b = next.encode()?;
        OwnerRecord::decode(&b)?;
        let staged = self.stage("owner-record.json", &b, expected, 512)?;
        self.publish(staged)?;
        Ok(digest(&b))
    }
    fn stage_config(&mut self, bytes: &[u8], expected: Option<[u8; 32]>) -> Result<(), Failure> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(Failure::RecoveryRequired);
        }
        if self.staged.is_some() {
            return Err(Failure::Conflict);
        }
        // NativeResolver is fixed; owner-only parse rechecks account pins and
        // metadata. Service startup does not call this path.
        ServiceConfig::parse(bytes, &self.layout, &native::NativeResolver)
            .map_err(|_| Failure::InvalidPlan)?;
        self.staged = Some(self.stage("arxd.json", bytes, expected, MAX_CONFIG_BYTES)?);
        Ok(())
    }
    fn publish_config(&mut self) -> Result<(), Failure> {
        let s = self.staged.take().ok_or(Failure::InvalidPlan)?;
        self.publish(s)
    }
}

fn owner_required() -> Result<(), Error> {
    unsafe {
        let mut t = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut t) == 0 {
            return Err(Error::OwnerRequired);
        }
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0;
        let ok = GetTokenInformation(
            t,
            TokenElevation,
            (&mut e as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of_val(&e) as u32,
            &mut len,
        );
        CloseHandle(t);
        let mut sid = [0u32; 17];
        let mut size = 68;
        let mut enabled = 0;
        if ok == 0
            || e.TokenIsElevated == 0
            || CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                null_mut(),
                sid.as_mut_ptr().cast(),
                &mut size,
            ) == 0
            || CheckTokenMembership(null_mut(), sid.as_mut_ptr().cast(), &mut enabled) == 0
            || enabled == 0
        {
            return Err(Error::OwnerRequired);
        }
    }
    Ok(())
}
fn enable_restore() -> Result<(), Error> {
    unsafe {
        let mut t = null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
            &mut t,
        ) == 0
        {
            return Err(Error::OwnerRequired);
        }
        let mut p = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            ..Default::default()
        };
        let found = LookupPrivilegeValueW(
            null(),
            wide("SeRestorePrivilege").as_ptr(),
            &mut p.Privileges[0].Luid,
        );
        p.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
        SetLastError(0);
        let ok = found != 0 && AdjustTokenPrivileges(t, 0, &p, 0, null_mut(), null_mut()) != 0;
        let error = GetLastError();
        CloseHandle(t);
        if !ok || error != 0 {
            return Err(Error::OwnerRequired);
        }
    }
    Ok(())
}

/// Narrow direct-right adapter; obtainable only through the locked session.
struct WindowsBatchRight {
    _private: (),
}
struct Lsa(LSA_HANDLE);
impl Drop for Lsa {
    fn drop(&mut self) {
        unsafe {
            LsaClose(self.0);
        }
    }
}
struct LsaMemory(*mut core::ffi::c_void);
impl Drop for LsaMemory {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                LsaFreeMemory(self.0);
            }
        }
    }
}
fn sid(value: &str) -> Result<Local, Failure> {
    // Reuse the engine's canonical ordinary-SID validation without exposing data.
    if value.len() > 52 {
        return Err(Failure::InvalidPlan);
    }
    let record = serde_json::json!({"version":1,"sid":value,"state":"none"});
    OwnerRecord::decode(&serde_json::to_vec(&record).map_err(|_| Failure::InvalidPlan)?)
        .map_err(|_| Failure::InvalidPlan)?;
    let mut s = null_mut();
    if unsafe { ConvertStringSidToSidW(wide(value).as_ptr(), &mut s) } == 0 {
        return Err(Failure::InvalidPlan);
    }
    let s = Local(s);
    if unsafe { IsValidSid(s.0) } == 0 {
        return Err(Failure::InvalidPlan);
    }
    Ok(s)
}
fn policy(mutate: bool) -> Result<Lsa, Failure> {
    let attrs = LSA_OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<LSA_OBJECT_ATTRIBUTES>() as u32,
        ..Default::default()
    };
    let mut h = 0;
    if unsafe {
        LsaOpenPolicy(
            null(),
            &attrs,
            (POLICY_LOOKUP_NAMES | if mutate { POLICY_CREATE_ACCOUNT } else { 0 }) as u32,
            &mut h,
        )
    } != 0
    {
        return Err(Failure::Io);
    }
    Ok(Lsa(h))
}
// SAFETY: pointer/count must describe an OS-returned allocation or a live
// synthetic array; each nonempty Buffer must have MaximumLength readable bytes.
unsafe fn observed(status: i32, p: *const LSA_UNICODE_STRING, count: u32) -> Result<bool, Failure> {
    if status == STATUS_OBJECT_NAME_NOT_FOUND {
        return Ok(false);
    }
    if status != 0 || count > 64 || (count > 0 && p.is_null()) {
        return Err(Failure::Io);
    }
    let mut present = false;
    for i in 0..count as usize {
        let s = unsafe { &*p.add(i) };
        if s.Length > 128
            || s.Length % 2 != 0
            || s.MaximumLength < s.Length
            || s.MaximumLength % 2 != 0
            || (s.Length > 0 && s.Buffer.is_null())
        {
            return Err(Failure::Io);
        }
        let name = if s.Length == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(s.Buffer, (s.Length / 2) as usize) }
        };
        present |= name.iter().copied().eq("SeBatchLogonRight".encode_utf16());
    }
    Ok(present)
}
impl BatchRight for WindowsBatchRight {
    fn observe(&mut self, value: &str) -> Result<bool, Failure> {
        let s = sid(value)?;
        let p = policy(false)?;
        let mut names = null_mut();
        let mut count = 0;
        let status = unsafe { LsaEnumerateAccountRights(p.0, s.0, &mut names, &mut count) };
        let _memory = LsaMemory(names.cast());
        unsafe { observed(status, names, count) }
    }
    fn grant(&mut self, value: &str) -> Result<(), Failure> {
        mutate(value, true)
    }
    fn revoke(&mut self, value: &str) -> Result<(), Failure> {
        mutate(value, false)
    }
}
fn mutate(value: &str, add: bool) -> Result<(), Failure> {
    let s = sid(value)?;
    let p = policy(true)?;
    let mut name = wide("SeBatchLogonRight");
    let right = LSA_UNICODE_STRING {
        Length: ((name.len() - 1) * 2) as u16,
        MaximumLength: (name.len() * 2) as u16,
        Buffer: name.as_mut_ptr(),
    };
    let status = unsafe {
        if add {
            LsaAddAccountRights(p.0, s.0, &right, 1)
        } else {
            LsaRemoveAccountRights(p.0, s.0, false, &right, 1)
        }
    };
    mutation_status(status)
}
fn mutation_status(status: i32) -> Result<(), Failure> {
    if status == 0 {
        Ok(())
    } else {
        Err(Failure::Io)
    }
}
#[cfg(test)]
#[path = "owner_windows_tests.rs"]
mod tests;

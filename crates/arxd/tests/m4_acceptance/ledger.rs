use super::*;
use arxd::{
    config::EnrolledVmStore,
    vm::{paths::LocalPath, qemu_img::inspect_header},
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions, read_dir},
    io::{self, Read},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Wdk::{
    Foundation::OBJECT_ATTRIBUTES,
    Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    },
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES, ERROR_SHARING_VIOLATION, HANDLE,
        INVALID_HANDLE_VALUE, LocalFree, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
        RtlNtStatusToDosError, UNICODE_STRING,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetTokenInformation, IsWellKnownSid,
        OWNER_SECURITY_INFORMATION, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
        WinBuiltinAdministratorsSid, WinLocalSystemSid,
    },
    Storage::FileSystem::*,
    System::{
        IO::IO_STATUS_BLOCK,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

pub(super) type Id = [u8; 16];

const MAX_ARTIFACT: u64 = 200 * 1024 * 1024 * 1024;
const GENERATION_LEASE: &str = "generation.lease";
const MAX_ROOT_ENTRIES: usize = 256;
type Hash = [u8; 32];

/// Custody contract for quiet golden inventory scans.
///
/// Native acceptance and startup preflight require `Native` custody for every
/// retained golden. Pre-7c historic rows upgraded from store v15 lack
/// `vm_native_generations` / `vm_native_slots` rows and `generation.lease`;
/// they may be inspected read-only under `Historic`, never silently bound,
/// and never satisfy native TEST eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GoldenCustody {
    Native,
    Historic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct InventorySummary {
    pub generations: usize,
    pub artifacts: usize,
    pub leases: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct NativeBinding {
    volume: [u8; 8],
    production: Id,
    directory: Id,
    lease: Id,
}

pub(super) fn safe<T, E>(result: std::result::Result<T, E>, label: &'static str) -> Result<T> {
    result.map_err(|_| Failure::Check(label))
}

pub(super) fn id(row: &Value, key: &'static str) -> Result<Id> {
    let hex = opaque(&row[key], 32).ok_or(Failure::Check(key))?;
    let mut id = [0; 16];
    safe(hex::decode_to_slice(hex, &mut id), key)?;
    check(id != [0; 16], key)?;
    Ok(id)
}

pub(super) fn digest(file: &File, max: u64) -> Result<(u64, Hash)> {
    let mut sha = Sha256::new();
    let copied = safe(io::copy(&mut file.take(max + 1), &mut sha), "hash_read")?;
    check(copied > 0 && copied <= max, "hash_bound")?;
    Ok((copied, sha.finalize().into()))
}

pub(super) fn config<T: for<'a> Deserialize<'a>>(file: &File) -> Result<T> {
    let mut bytes = Vec::new();
    safe(file.take(65_537).read_to_end(&mut bytes), "input_read")?;
    check(bytes.len() <= 65_536, "input_bound")?;
    safe(serde_json::from_slice(&bytes), "input_format")
}

#[derive(PartialEq, Eq)]
struct Identity {
    volume: [u8; 8],
    file: Id,
}

fn identity(handle: windows_sys::Win32::Foundation::HANDLE, directory: bool) -> Result<Identity> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    let mut id = FILE_ID_INFO::default();
    // Each output is a fixed-size structure from a live file handle.
    let valid = unsafe {
        GetFileType(handle) == FILE_TYPE_DISK
            && GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            ) != 0
            && GetFileInformationByHandleEx(
                handle,
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            ) != 0
            && GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            ) != 0
    };
    check(
        valid
            && tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
            && standard.Directory == directory
            && !standard.DeletePending
            && (directory || standard.NumberOfLinks == 1),
        "unsafe_file_identity",
    )?;
    Ok(Identity {
        volume: id.VolumeSerialNumber.to_be_bytes(),
        file: id.FileId.Identifier,
    })
}

fn nt_open_relative(
    parent: &File,
    name: &str,
    access: u32,
    share: u32,
    kind: u32,
) -> io::Result<File> {
    if name.is_empty() || name.contains(['\\', '/', ':']) {
        return Err(io::Error::other("not a single path component"));
    }
    let mut units: Vec<u16> = name.encode_utf16().collect();
    let bytes = u16::try_from(units.len() * 2).map_err(io::Error::other)?;
    let object = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: units.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &object,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        SecurityDescriptor: null(),
        SecurityQualityOfService: null(),
    };
    let options = FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT | kind;
    let mut handle = null_mut();
    // Zero is a valid initial state for this output-only status block.
    let mut status_block: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // The name buffer, attributes and parent handle outlive this call.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status_block,
            null(),
            FILE_ATTRIBUTE_NORMAL,
            share,
            FILE_OPEN,
            options,
            null(),
            0,
        )
    };
    if status < 0 {
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::other("invalid handle returned"));
    }
    // NtCreateFile succeeded and transferred ownership of a fresh handle.
    Ok(File::from(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

fn probe_relative(parent: &File, name: &str) -> io::Result<Option<File>> {
    match nt_open_relative(
        parent,
        name,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        0,
    ) {
        Ok(file) => Ok(Some(file)),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                || error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn listing_bounded(
    dir: &File,
    max_entries: usize,
    bound_err: &'static str,
) -> Result<Vec<(String, u32)>> {
    let mut buffer = vec![0u64; 8192];
    let capacity = buffer.len() * size_of::<u64>();
    let header = size_of::<FILE_FULL_DIR_INFO>();
    let name_offset = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName);
    let mut found = Vec::new();
    let mut class = FileFullDirectoryRestartInfo;
    loop {
        // The aligned buffer receives a chain of variable-length directory records.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle(),
                class,
                buffer.as_mut_ptr().cast(),
                capacity as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(found);
            }
            return Err(Failure::Check("media_inventory"));
        }
        class = FileFullDirectoryInfo;
        let bytes = buffer.as_ptr().cast::<u8>();
        let mut offset = 0usize;
        loop {
            check(offset + header <= capacity, "media_inventory")?;
            // The record starts inside the buffer at an 8-byte boundary.
            let entry = unsafe { &*bytes.add(offset).cast::<FILE_FULL_DIR_INFO>() };
            let length = entry.FileNameLength as usize;
            check(
                offset + name_offset + length <= capacity && length.is_multiple_of(2),
                "media_inventory",
            )?;
            // The UTF-16 name slice lies within the validated buffer bounds.
            let units = unsafe {
                std::slice::from_raw_parts(
                    bytes.add(offset + name_offset).cast::<u16>(),
                    length / 2,
                )
            };
            let name = safe(String::from_utf16(units), "media_inventory")?;
            if name != "." && name != ".." {
                check(found.len() < max_entries, bound_err)?;
                found.push((name, entry.FileAttributes));
            }
            let next = entry.NextEntryOffset as usize;
            if next == 0 {
                break;
            }
            offset = offset
                .checked_add(next)
                .ok_or(Failure::Check("media_inventory"))?;
        }
    }
}

fn has_exact_leaf(file: &File, name: &str) -> bool {
    // The normalized path from the held handle supplies NTFS's stored case.
    let mut path = vec![0u16; 32_768];
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            path.as_mut_ptr(),
            path.len() as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    count != 0
        && count < path.len()
        && path[..count]
            .rsplit(|unit| *unit == u16::from(b'\\'))
            .next()
            .is_some_and(|leaf| leaf.iter().copied().eq(name.encode_utf16()))
}

fn current_user_sid_buffer() -> Result<Vec<usize>> {
    let mut raw_token: HANDLE = null_mut();
    check(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } != 0
            && !raw_token.is_null()
            && raw_token != INVALID_HANDLE_VALUE,
        "owner_token",
    )?;
    // OpenProcessToken succeeded and transferred a fresh token handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
    let mut needed = 0u32;
    unsafe {
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut needed);
    }
    check(needed > 0 && needed <= 65_536, "owner_token")?;
    let words = (needed as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0usize; words];
    let mut written = 0u32;
    check(
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * size_of::<usize>()) as u32,
                &mut written,
            )
        } != 0
            && written as usize >= size_of::<TOKEN_USER>(),
        "owner_token",
    )?;
    Ok(buffer)
}

fn user_sid(buffer: &[usize]) -> PSID {
    // TokenUser data begins with a TOKEN_USER whose Sid points into `buffer`.
    unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

/// Owner and DACL of the held object itself: owned by the current owner user,
/// with allow entries only for that user, SYSTEM and Administrators.
fn held_private(file: &File, owner: PSID) -> bool {
    let mut actual_owner = null_mut();
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    // The descriptor owns the returned owner and DACL pointers until LocalFree.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut actual_owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return false;
    }
    let private = unsafe { private_descriptor(actual_owner, dacl, owner) };
    unsafe { LocalFree(descriptor) };
    private
}

unsafe fn private_descriptor(actual_owner: PSID, dacl: *const ACL, owner: PSID) -> bool {
    // Caller keeps the owner SID and security descriptor alive for this call.
    unsafe {
        !actual_owner.is_null()
            && EqualSid(actual_owner, owner) != 0
            && !dacl.is_null()
            && (0..u32::from((*dacl).AceCount)).all(|index| {
                let mut ace = null_mut();
                if GetAce(dacl, index, &mut ace) == 0 {
                    return false;
                }
                let header = &*ace.cast::<ACE_HEADER>();
                match header.AceType {
                    // A deny entry grants nothing.
                    1 => true,
                    0 => {
                        let allow = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                        let sid = std::ptr::addr_of!(allow.SidStart).cast_mut().cast();
                        EqualSid(sid, owner) != 0
                            || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                            || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
                    }
                    _ => false,
                }
            })
    }
}

fn verify_native_lease(
    root_handle: &File,
    root_id: &Identity,
    dir: &File,
    dir_id: &Identity,
    binding: &NativeBinding,
    owner: PSID,
) -> Result<()> {
    let lease = match nt_open_relative(
        dir,
        GENERATION_LEASE,
        FILE_GENERIC_READ | READ_CONTROL,
        0,
        FILE_NON_DIRECTORY_FILE,
    ) {
        Ok(file) => file,
        Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => {
            return Err(Failure::Check("native_lease_busy"));
        }
        Err(_) => return Err(Failure::Check("native_lease_open")),
    };
    let handle = lease.as_raw_handle();
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    let mut id = FILE_ID_INFO::default();
    let file_type = unsafe { GetFileType(handle) };
    let queried = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        ) != 0
            && GetFileInformationByHandleEx(
                handle,
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            ) != 0
            && GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            ) != 0
    };
    check(queried, "native_lease_identity")?;
    check(
        tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
        "native_lease_reparse",
    )?;
    check(
        file_type == FILE_TYPE_DISK && !standard.Directory,
        "native_lease_kind",
    )?;
    check(!standard.DeletePending, "native_lease_delete_pending")?;
    check(standard.NumberOfLinks == 1, "native_lease_links")?;
    check(standard.EndOfFile == 0, "native_lease_length")?;
    check(
        has_exact_leaf(&lease, GENERATION_LEASE),
        "native_lease_case",
    )?;
    let lease_id = Identity {
        volume: id.VolumeSerialNumber.to_be_bytes(),
        file: id.FileId.Identifier,
    };
    check(
        lease_id.volume == binding.volume
            && dir_id.volume == binding.volume
            && dir_id.file == binding.directory
            && lease_id.file == binding.lease
            && lease_id.file != dir_id.file
            && lease_id.file != binding.production,
        "native_lease_identity",
    )?;
    check(held_private(&lease, owner), "native_lease_acl")?;
    // Bounded nonblocking lock check: mirrors ProductionWorkspace's exclusive
    // lease acquisition without ever waiting on a live holder.
    safe(lease.try_lock(), "native_lease_locked")?;
    let pinned_dir = identity(dir.as_raw_handle(), true)?;
    let pinned_root = identity(root_handle.as_raw_handle(), true)?;
    let unlocked = lease.unlock();
    check(
        pinned_dir == *dir_id && pinned_root == *root_id,
        "native_lease_identity",
    )?;
    safe(unlocked, "native_lease_locked")?;
    Ok(())
}

pub(super) fn open(path: &Path, directory: bool, share: u32) -> Result<File> {
    safe(arxd::config::checked_path(path), "unsafe_local_path")?;
    let file = safe(
        OpenOptions::new()
            .read(true)
            .share_mode(share)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT
                    | if directory {
                        FILE_FLAG_BACKUP_SEMANTICS
                    } else {
                        0
                    },
            )
            .open(path),
        "local_open",
    )?;
    identity(file.as_raw_handle(), directory)?;
    Ok(file)
}

/// Hold the entire root chain without delete sharing while opening children.
pub(super) fn pin_root(root: &Path) -> Result<Vec<File>> {
    let chain: Vec<_> = root.ancestors().filter(|path| path.has_root()).collect();
    chain
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .rev()
        .map(|path| open(path, true, FILE_SHARE_READ | FILE_SHARE_WRITE))
        .collect()
}

pub(super) struct Ledger {
    db: Connection,
    root: PathBuf,
    backing_root: PathBuf,
    _pins: Vec<File>,
    database: File,
}

pub(super) struct LiveLedger {
    ledger: Ledger,
    store: EnrolledVmStore,
}

impl LiveLedger {
    pub fn open(configured: &Path, expected: &ExpectedVmStore) -> Result<Self> {
        let store = safe(
            EnrolledVmStore::open(configured, expected),
            "foreground_store_binding",
        )?;
        let ledger = safe(
            store.read(|root| Ledger::open_at(root, configured)),
            "foreground_store_binding",
        )??;
        Ok(Self { ledger, store })
    }

    pub fn read<T>(&self, batch: impl FnOnce(&Ledger) -> Result<T>) -> Result<T> {
        safe(
            self.store.read(|_| batch(&self.ledger)),
            "foreground_store_binding",
        )?
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Leaf {
    generation: Id,
    role: &'static str,
    key: Id,
    volume: [u8; 8],
    directory: Id,
    file: Id,
    digest: Option<Hash>,
    bytes: Option<u64>,
}

pub(super) type Pair = [Leaf; 2];

impl Leaf {
    fn path(&self, root: &Path) -> PathBuf {
        root.join(hex::encode(self.generation)).join(format!(
            "{}-{}",
            self.role,
            hex::encode(self.key)
        ))
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct Claim {
    pub active: i64,
    pub snapshot: i64,
    pub staged: i64,
    pub generations: i64,
}

impl Claim {
    pub fn evidence(&self) -> Value {
        json!({"persistent":self.active,"snapshot":self.snapshot,"staging":self.staged,"generations":self.generations})
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct PairProof {
    pub contents: [(u64, Hash); 2],
    pub allocated: [u64; 2],
}

impl PairProof {
    pub fn evidence(&self) -> Value {
        json!({
            "disk":{"bytes":self.contents[0].0,"sha256":hex::encode(self.contents[0].1),"allocated_bytes":self.allocated[0]},
            "vars":{"bytes":self.contents[1].0,"sha256":hex::encode(self.contents[1].1),"allocated_bytes":self.allocated[1]},
        })
    }
}

// The store constant is private. Read its declaration from the same crate
// source used to build this test, and refuse if it is no longer readable.
fn built_vm_schema_version() -> Result<u32> {
    include_str!("../../src/store/vm.rs")
        .lines()
        .find_map(|line| line.trim().strip_prefix("const VM_SCHEMA_VERSION: u32 ="))
        .and_then(|value| value.trim().strip_suffix(';'))
        .and_then(|value| value.parse().ok())
        .ok_or(Failure::Check("built_ledger_version"))
}

impl Ledger {
    fn root_handle(&self) -> Result<&File> {
        self._pins.last().ok_or(Failure::Check("root_pin"))
    }

    /// The real foreground sweeps at startup. Check a stopped store before
    /// launching it, so a wrong profile cannot sweep someone else's VMs.
    /// Requires native custody for any retained sealed golden.
    pub fn preflight(state: &Path, recipes: [(&str, &str); 2]) -> Result<()> {
        Self::preflight_with(state, recipes, GoldenCustody::Native).map(|_| ())
    }

    /// Explicit read-only observation of pre-7c historic golden layouts
    /// upgraded from store v15 without native custody rows or
    /// `generation.lease`. Never supplies native eligibility and never binds
    /// silently.
    #[cfg(test)]
    pub(super) fn observe_historic_preflight(
        state: &Path,
        recipes: [(&str, &str); 2],
    ) -> Result<InventorySummary> {
        Self::preflight_with(state, recipes, GoldenCustody::Historic)
    }

    fn preflight_with(
        state: &Path,
        recipes: [(&str, &str); 2],
        custody: GoldenCustody,
    ) -> Result<InventorySummary> {
        let _state = pin_root(state)?;
        match state.join("activity.sqlite3").symlink_metadata() {
            Ok(_) => {
                for suffix in ["-journal", "-wal", "-shm"] {
                    match state
                        .join(format!("activity.sqlite3{suffix}"))
                        .symlink_metadata()
                    {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        _ => return Err(Failure::Check("unsettled_startup_ledger")),
                    }
                }
                let ledger = Self::open(state)?;
                ledger.quiet()?;
                let templates = ledger.goldens()?;
                check(
                    templates.len() <= recipes.len(),
                    "isolated_golden_inventory",
                )?;
                let mut seen_guests = BTreeSet::new();
                let mut selected = Vec::new();
                let mut total_charge: i64 = 0;
                for template in templates {
                    let guest = safe(
                        ledger.db.query_row(
                            "SELECT guest FROM vm_templates WHERE id=?1",
                            [template],
                            |row| row.get::<_, i64>(0),
                        ),
                        "golden_guest",
                    )?;
                    check(seen_guests.insert(guest), "isolated_golden_inventory")?;
                    let (recipe, pin) = match guest {
                        1 => recipes[0],
                        2 => recipes[1],
                        _ => return Err(Failure::Check("golden_guest")),
                    };
                    let (pair, charge) = ledger.golden(template, recipe, pin, guest)?;
                    check(charge > 0, "remaining_charges")?;
                    total_charge = total_charge
                        .checked_add(charge)
                        .ok_or(Failure::Check("remaining_charges"))?;
                    ledger.inspect(&pair, None, true)?;
                    selected.push(pair);
                }
                let summary =
                    ledger.inventory_with(&selected.iter().collect::<Vec<_>>(), custody)?;
                check(ledger.aggregate()? == total_charge, "remaining_charges")?;
                Ok(summary)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                check(
                    safe(read_dir(state), "fresh_state")?.next().is_none(),
                    "fresh_state",
                )?;
                Ok(InventorySummary {
                    generations: 0,
                    artifacts: 0,
                    leases: 0,
                })
            }
            Err(_) => Err(Failure::Check("ledger_preflight")),
        }
    }

    pub fn open(state: &Path) -> Result<Self> {
        Self::open_at(state, state)
    }

    fn open_at(state: &Path, backing_root: &Path) -> Result<Self> {
        let pins = pin_root(state)?;
        let path = state.join("activity.sqlite3");
        let database = open(&path, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
        check(
            safe(database.metadata(), "ledger_metadata")?.len() <= 4 * 1024 * 1024,
            "ledger_size_bound",
        )?;
        for suffix in ["-journal", "-wal", "-shm"] {
            let sidecar = state.join(format!("activity.sqlite3{suffix}"));
            match sidecar.symlink_metadata() {
                Ok(_) => {
                    open(
                        &sidecar,
                        false,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    )?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(Failure::Check("ledger_sidecar")),
            }
        }
        let db = safe(
            Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_NOFOLLOW
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ),
            "ledger_open",
        )?;
        let mut handle: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        // SQLite retains ownership of this borrowed VFS handle.
        let rc = unsafe {
            rusqlite::ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                rusqlite::ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                std::ptr::from_mut(&mut handle).cast(),
            )
        };
        check(
            rc == rusqlite::ffi::SQLITE_OK
                && identity(handle, false)? == identity(database.as_raw_handle(), false)?,
            "ledger_identity",
        )?;
        safe(db.busy_timeout(Duration::from_secs(1)), "ledger_timeout")?;
        check(
            safe(
                db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0)),
                "ledger_version",
            )? == built_vm_schema_version()?,
            "ledger_version",
        )?;
        Ok(Self {
            db,
            root: state.to_path_buf(),
            backing_root: backing_root.to_path_buf(),
            _pins: pins,
            database,
        })
    }

    pub fn quiet(&self) -> Result<()> {
        check(
            safe(
                self.db.query_row(
                    "SELECT (SELECT count(*) FROM vm_instances)
                  + (SELECT count(*) FROM vm_static_instances)
                  + (SELECT count(*) FROM vm_restore_points)
                  + (SELECT count(*) FROM vm_snapshot_work)
                  + (SELECT count(*) FROM vm_build_intents WHERE state<>'sealed')
                  + (SELECT count(*) FROM vm_operations WHERE status=0)
                  + (SELECT count(*) FROM vm_disk_claims)
                  + (SELECT count(*) FROM vm_native_generations
                        WHERE state<>'bound' OR disposal IS NOT NULL)
                  + (SELECT count(*) FROM vm_native_slots
                        WHERE state<>'verified' OR disposal IS NOT NULL)",
                    [],
                    |r| r.get::<_, i64>(0),
                ),
                "ledger_quiet",
            )? == 0,
            "ledger_not_isolated",
        )
    }

    pub fn goldens(&self) -> Result<Vec<Id>> {
        let mut statement = safe(
            self.db
                .prepare("SELECT id FROM vm_templates ORDER BY id LIMIT 3"),
            "ledger_goldens",
        )?;
        let ids = safe(statement.query_map([], |r| r.get(0)), "ledger_goldens")?;
        safe(ids.collect(), "ledger_goldens")
    }

    pub fn golden(&self, template: Id, recipe: &str, pin: &str, guest: i64) -> Result<(Pair, i64)> {
        let (generation, charge) = safe(
            self.db.query_row(
                "SELECT g.id,t.bytes FROM vm_templates t
             JOIN vm_build_intents b ON b.template_id=t.id
             JOIN vm_generations g ON g.build_id=b.id AND g.active=1
             JOIN vm_artifacts d ON d.generation_id=g.id AND d.role='disk' AND d.state='sealed'
             JOIN vm_artifacts v ON v.generation_id=g.id AND v.role='vars' AND v.state='sealed'
             WHERE t.id=?1 AND b.state='sealed' AND b.health_verified=1 AND b.recipe_id=?2
               AND b.recipe_digest=t.recipe_digest AND lower(hex(t.recipe_digest))=?3 AND t.guest=?4
               AND d.content_digest=t.disk_digest AND v.content_digest=t.vars_digest",
                params![template, recipe, pin, guest],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ),
            "sealed_recipe_pins",
        )?;
        Ok((self.pair(generation, "sealed")?, charge))
    }

    pub fn vm_pair(&self, vm: Id) -> Result<Pair> {
        let generation = safe(
            self.db.query_row(
                "SELECT id FROM vm_generations WHERE vm_id=?1 AND active=1",
                [vm],
                |r| r.get(0),
            ),
            "active_generation",
        )?;
        self.pair(generation, "active")
    }

    pub fn point(&self, vm: Id, snapshot: Id, template: Id, provenance: Id) -> Result<(Pair, i64)> {
        let (generation, charge) = safe(
            self.db.query_row(
                "SELECT p.generation,p.charge FROM vm_restore_points p
             JOIN vm_instances i ON i.id=p.vm JOIN vm_templates t ON t.id=i.template
             WHERE p.id=?1 AND p.vm=?2 AND p.state='ready' AND t.id=?3 AND p.provenance=?4",
                params![snapshot, vm, template, provenance],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ),
            "snapshot_ledger",
        )?;
        Ok((self.pair(generation, "sealed")?, charge))
    }

    fn pair(&self, generation: Id, state: &str) -> Result<Pair> {
        let leaf = |role| {
            safe(
                self.db.query_row(
                    "SELECT object_key,volume_id,directory_id,file_id,content_digest,bytes
             FROM vm_artifacts WHERE generation_id=?1 AND role=?2 AND state=?3",
                    params![generation, role, state],
                    |r| {
                        Ok(Leaf {
                            generation,
                            role,
                            key: r.get(0)?,
                            volume: r.get(1)?,
                            directory: r.get(2)?,
                            file: r.get(3)?,
                            digest: r.get(4)?,
                            bytes: r.get(5)?,
                        })
                    },
                ),
                "ledger_pair",
            )
        };
        let pair = [leaf("disk")?, leaf("vars")?];
        check(
            pair[0].key != pair[1].key && pair[0].file != pair[1].file,
            "private_pair",
        )?;
        Ok(pair)
    }

    pub fn generation(pair: &Pair) -> Id {
        pair[0].generation
    }

    pub fn inspect(&self, pair: &Pair, backing: Option<&Pair>, sealed: bool) -> Result<PairProof> {
        // The configured spelling is an expected header value only.
        let backup = backing.map(|pair| pair[0].path(&self.backing_root));
        let backup = backup
            .as_ref()
            .map(|path| {
                let text = path.to_str().ok_or(Failure::Check("backing_path"))?;
                safe(LocalPath::parse(text), "backing_path")
            })
            .transpose()?;
        let dir = open(
            &self.root.join(hex::encode(pair[0].generation)),
            true,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )?;
        let dir_id = identity(dir.as_raw_handle(), true)?;
        let mut proof = PairProof {
            contents: [(0, [0; 32]); 2],
            allocated: [0; 2],
        };
        for (index, leaf) in pair.iter().enumerate() {
            let mut file = open(&leaf.path(&self.root), false, FILE_SHARE_READ)?;
            let id = identity(file.as_raw_handle(), false)?;
            check(
                id.file == leaf.file
                    && id.volume == leaf.volume
                    && dir_id.volume == id.volume
                    && dir_id.file == leaf.directory,
                "artifact_identity",
            )?;
            // NTFS allocated length is independent of the qcow2 virtual size.
            let mut standard = FILE_STANDARD_INFO::default();
            check(
                unsafe {
                    GetFileInformationByHandleEx(
                        file.as_raw_handle(),
                        FileStandardInfo,
                        (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                        size_of::<FILE_STANDARD_INFO>() as u32,
                    )
                } != 0,
                "allocated_length",
            )?;
            proof.allocated[index] =
                safe(u64::try_from(standard.AllocationSize), "allocated_length")?;
            let contents = digest(&file, MAX_ARTIFACT)?;
            if sealed {
                check(
                    leaf.bytes == Some(contents.0) && leaf.digest == Some(contents.1),
                    "sealed_content_pin",
                )?;
            }
            if index == 0 {
                safe(std::io::Seek::rewind(&mut file), "disk_rewind")?;
                let mut cluster = vec![0; 65_536];
                safe(file.read_exact(&mut cluster), "qcow_header")?;
                safe(inspect_header(&cluster, backup.as_ref()), "qcow_lineage")?;
            }
            proof.contents[index] = contents;
        }
        Ok(proof)
    }

    pub fn distinct(&self, first: &Pair, second: &Pair) -> Result<()> {
        check(
            first[0].generation != second[0].generation
                && first.iter().all(|a| {
                    second
                        .iter()
                        .all(|b| a.key != b.key && (a.volume != b.volume || a.file != b.file))
                }),
            "aliased_pairs",
        )
    }

    /// Confirm a retired generation has left both the ledger and the physical
    /// custody root: no generation/artifact/native rows remain, and the
    /// generation directory, its `generation.lease`, and its media leaves are
    /// all physically absent. Empty or lease-only retired directories are never
    /// waived.
    pub fn gone(&self, pair: &Pair) -> Result<bool> {
        let generation = pair[0].generation;
        let retained = safe(
            self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM vm_generations WHERE id=?1)",
                [generation],
                |r| r.get::<_, bool>(0),
            ),
            "retirement_ledger",
        )?;
        if retained {
            return Ok(false);
        }
        let ledger_residue = safe(
            self.db.query_row(
                "SELECT (SELECT count(*) FROM vm_artifacts WHERE generation_id=?1)
                      + (SELECT count(*) FROM vm_native_generations WHERE generation=?1)
                      + (SELECT count(*) FROM vm_native_slots WHERE generation=?1)",
                [generation],
                |r| r.get::<_, i64>(0),
            ),
            "retirement_ledger",
        )?;
        check(ledger_residue == 0, "retired_ledger_residue")?;
        let gen_name = hex::encode(generation);
        let root = self.root_handle()?;
        let _ = identity(root.as_raw_handle(), true)?;
        match safe(probe_relative(root, &gen_name), "retired_directory")? {
            None => Ok(true),
            Some(_) => {
                if let Ok(dir) = nt_open_relative(
                    root,
                    &gen_name,
                    FILE_GENERIC_READ | READ_CONTROL,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    FILE_DIRECTORY_FILE,
                ) {
                    if matches!(probe_relative(&dir, GENERATION_LEASE), Ok(Some(_))) {
                        return Err(Failure::Check("retired_lease_still_present"));
                    }
                    for leaf in pair {
                        let leaf_name = format!("{}-{}", leaf.role, hex::encode(leaf.key));
                        if matches!(probe_relative(&dir, &leaf_name), Ok(Some(_))) {
                            return Err(Failure::Check("retired_media_still_present"));
                        }
                    }
                }
                Err(Failure::Check("retired_directory_still_present"))
            }
        }
    }

    pub fn release(&self, pair: &Pair, start: Id, clean: bool) -> Result<()> {
        let (epoch, disposition) = safe(
            self.db.query_row(
                "SELECT epoch,disposition FROM vm_snapshot_release WHERE generation=?1",
                [Self::generation(pair)],
                |r| Ok((r.get::<_, Id>(0)?, r.get::<_, Option<String>>(1)?)),
            ),
            "boot_release_epoch",
        )?;
        check(
            epoch == start && disposition.as_deref() == clean.then_some("clean"),
            "boot_release_epoch",
        )
    }

    pub fn charge(&self, vm: Id) -> Result<Claim> {
        safe(self.db.query_row(
            "SELECT (SELECT coalesce(sum(persistent),0) FROM vm_disk_claims WHERE vm=?1),
                    (SELECT coalesce(sum(charge),0) FROM vm_restore_points WHERE vm=?1),
                    (SELECT coalesce(sum(staging),0) FROM vm_disk_claims WHERE vm=?1)
                      + (SELECT coalesce(sum(staging+persistent),0) FROM vm_snapshot_work WHERE vm=?1),
                    (SELECT count(*) FROM vm_generations WHERE vm_id=?1)",
            [vm], |r| Ok(Claim { active:r.get(0)?,snapshot:r.get(1)?,staged:r.get(2)?,generations:r.get(3)? }),
        ), "ledger_charge")
    }

    pub fn effect(&self, vm: Id) -> Result<Value> {
        let (vms, operations, media) = safe(
            self.db.query_row(
                "SELECT (SELECT count(*) FROM vm_instances),
                    (SELECT count(*) FROM vm_operations),
                    (SELECT count(*) FROM vm_artifacts)",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            ),
            "clone_retry_ledger",
        )?;
        Ok(json!({"vms":vms,"operations":operations,"media":media,
            "generation":hex::encode(Self::generation(&self.vm_pair(vm)?)),
            "charges":self.charge(vm)?.evidence()}))
    }

    pub fn metadata_bytes(&self) -> Result<u64> {
        Ok(safe(self.database.metadata(), "ledger_metadata")?.len())
    }

    pub fn aggregate(&self) -> Result<i64> {
        safe(self.db.query_row(
            "SELECT coalesce(sum(bytes),0) FROM (
                SELECT bytes FROM vm_templates UNION ALL SELECT persistent+staging FROM vm_disk_claims
                UNION ALL SELECT staging_bytes FROM vm_build_intents
                UNION ALL SELECT persistent+staging FROM vm_static_instances
                UNION ALL SELECT charge FROM vm_restore_points
                UNION ALL SELECT persistent+staging FROM vm_snapshot_work)",
            [], |r| r.get(0),
        ), "aggregate_charge")
    }

    pub fn activity(
        &self,
        operation: Id,
        request: Id,
        principal: &str,
        action: i64,
    ) -> Result<i64> {
        safe(self.db.query_row(
            "SELECT a.id FROM vm_operations o JOIN activity a ON a.id=o.activity_id
             WHERE o.operation_id=?1 AND o.request_key=?2 AND o.principal=?3
               AND a.actor='agent' AND a.principal=o.principal AND a.action=?4 AND o.action=a.action
               AND a.status=o.status AND a.status<>0 AND a.cleanup_confirmed IS o.cleanup_confirmed",
            params![operation, request, principal, action], |r| r.get(0),
        ), "operation_activity_correlation")
    }

    pub fn expiry(
        &self,
        vm: Id,
        principal: &str,
        activity: i64,
        deadline: i64,
        expired: i64,
    ) -> Result<()> {
        let found = safe(
            self.db
                .query_row(
                    "SELECT a.id FROM vm_tombstones t JOIN activity a ON a.id=t.activity_id
             WHERE t.id=?1 AND t.principal=?2 AND t.activity_id=?3 AND t.expired_at_ms=?4
               AND a.actor='daemon' AND a.principal='arxd' AND a.vm_owner=t.principal
               AND a.vm=t.id AND a.action=7 AND a.status=1 AND a.cleanup_confirmed=1
               AND a.started_ms>=?5 AND a.finished_ms>=a.started_ms",
                    params![vm, principal, activity, expired, deadline],
                    |r| r.get::<_, i64>(0),
                )
                .optional(),
            "daemon_expiry",
        )?;
        check(found == Some(activity), "daemon_expiry")
    }

    pub fn reconciled(
        &self,
        vm: Id,
        generation: Id,
        cause: &str,
        restore: Option<Id>,
        expiry: Option<i64>,
    ) -> Result<i64> {
        safe(
            self.db.query_row(
                "SELECT a.id FROM vm_snapshot_reconciliation r
             JOIN activity a ON a.id=r.activity_id
             LEFT JOIN vm_operations o ON o.activity_id=r.source_activity
             WHERE r.vm=?1 AND r.generation=?2 AND r.cause=?3
               AND o.operation_id IS ?4 AND r.expiry_activity IS ?5
               AND a.vm=r.vm AND a.action=19 AND a.actor='daemon' AND a.principal='arxd'
               AND a.status=1 AND a.cleanup_confirmed=1",
                params![vm, generation, cause, restore, expiry],
                |r| r.get(0),
            ),
            "snapshot_reconciliation",
        )
    }
    fn generation_custody(
        &self,
        pair: &Pair,
        root_id: &Identity,
        custody: GoldenCustody,
    ) -> Result<Option<NativeBinding>> {
        let generation = pair[0].generation;
        let row = safe(
            self.db
                .query_row(
                    "SELECT host=(SELECT id FROM vm_state_host WHERE singleton=1),
                            origin, protocol, volume, production, state, directory, lease, disposal
                     FROM vm_native_generations
                     WHERE generation=?1",
                    [generation],
                    |r| {
                        Ok((
                            r.get::<_, bool>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, [u8; 8]>(3)?,
                            r.get::<_, Id>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, Option<Id>>(6)?,
                            r.get::<_, Option<Id>>(7)?,
                            r.get::<_, Option<String>>(8)?,
                        ))
                    },
                )
                .optional(),
            "native_custody_scope",
        )?;
        match (custody, row) {
            (GoldenCustody::Native, None) => Err(Failure::Check("historic_golden_not_native")),
            (GoldenCustody::Historic, None) => {
                let slot_rows = safe(
                    self.db.query_row(
                        "SELECT count(*) FROM vm_native_slots WHERE generation=?1",
                        [generation],
                        |r| r.get::<_, usize>(0),
                    ),
                    "historic_custody_mismatch",
                )?;
                check(slot_rows == 0, "historic_custody_mismatch")?;
                Ok(None)
            }
            (GoldenCustody::Historic, Some(_)) => {
                Err(Failure::Check("native_golden_in_historic_scan"))
            }
            (
                GoldenCustody::Native,
                Some((
                    same_host,
                    origin,
                    protocol,
                    volume,
                    production,
                    state,
                    directory,
                    lease,
                    disposal,
                )),
            ) => {
                let (Some(directory), Some(lease)) = (directory, lease) else {
                    return Err(Failure::Check("native_custody_scope"));
                };
                check(
                    same_host
                        && origin == "build"
                        && protocol == 1
                        && state == "bound"
                        && disposal.is_none()
                        && volume == root_id.volume
                        && production == root_id.file
                        && directory == pair[0].directory
                        && directory == pair[1].directory
                        && volume == pair[0].volume
                        && volume == pair[1].volume
                        && lease != root_id.file
                        && lease != directory
                        && lease != pair[0].file
                        && lease != pair[1].file,
                    "native_custody_scope",
                )?;
                let slot_count = safe(
                    self.db.query_row(
                        "SELECT count(*) FROM vm_native_slots WHERE generation=?1",
                        [generation],
                        |r| r.get::<_, usize>(0),
                    ),
                    "native_slot_custody",
                )?;
                check(slot_count == 2, "native_slot_custody")?;
                for leaf in pair {
                    let slot = safe(
                        self.db
                            .query_row(
                                "SELECT state, file, bytes, content_digest, disposal
                                 FROM vm_native_slots
                                 WHERE generation=?1 AND role=?2 AND object_key=?3",
                                params![generation, leaf.role, leaf.key],
                                |r| {
                                    Ok((
                                        r.get::<_, String>(0)?,
                                        r.get::<_, Option<Id>>(1)?,
                                        r.get::<_, Option<u64>>(2)?,
                                        r.get::<_, Option<Hash>>(3)?,
                                        r.get::<_, Option<String>>(4)?,
                                    ))
                                },
                            )
                            .optional(),
                        "native_slot_custody",
                    )?;
                    let Some((slot_state, file, bytes, digest, slot_disposal)) = slot else {
                        return Err(Failure::Check("native_slot_custody"));
                    };
                    check(
                        slot_state == "verified"
                            && slot_disposal.is_none()
                            && file == Some(leaf.file)
                            && bytes.is_some()
                            && bytes == leaf.bytes
                            && digest.is_some()
                            && digest == leaf.digest,
                        "native_slot_custody",
                    )?;
                }
                Ok(Some(NativeBinding {
                    volume,
                    production,
                    directory,
                    lease,
                }))
            }
        }
    }

    /// Confirm the exact native golden inventory: exact generation directory
    /// set, exact disk/VARS golden pairs, and one separately counted quiescent
    /// `generation.lease` per native-bound golden. Empty, extra, or lease-only
    /// directories are never waived, and historic rows cannot pass native
    /// eligibility.
    pub fn inventory(&self, pairs: &[&Pair]) -> Result<InventorySummary> {
        self.inventory_with(pairs, GoldenCustody::Native)
    }

    pub(super) fn inventory_with(
        &self,
        pairs: &[&Pair],
        custody: GoldenCustody,
    ) -> Result<InventorySummary> {
        let root_handle = self.root_handle()?;
        let root_id = identity(root_handle.as_raw_handle(), true)?;
        let _ = identity(self.database.as_raw_handle(), false)?;
        let owner_buf = current_user_sid_buffer()?;
        let owner = user_sid(&owner_buf);

        // Enumerate the held root and refuse any activity sidecar before
        // querying SQLite, so a live rollback journal is classified cleanly as
        // `unsettled_ledger_sidecar` rather than a read-only SQLite fault.
        let root_entries = listing_bounded(root_handle, MAX_ROOT_ENTRIES, "media_inventory_bound")?;
        for (name, _) in &root_entries {
            if matches!(
                name.as_str(),
                "activity.sqlite3-journal" | "activity.sqlite3-wal" | "activity.sqlite3-shm"
            ) {
                return Err(Failure::Check("unsettled_ledger_sidecar"));
            }
        }

        struct ExpectedGeneration<'a> {
            pair: &'a Pair,
            disk_name: String,
            vars_name: String,
            lease: Option<NativeBinding>,
        }

        let mut expected_gens: BTreeMap<String, ExpectedGeneration<'_>> = BTreeMap::new();
        for &pair in pairs {
            check(
                pair[0].generation == pair[1].generation
                    && pair[0].role == "disk"
                    && pair[1].role == "vars"
                    && pair[0].key != pair[1].key
                    && pair[0].file != pair[1].file
                    && pair[0].volume == root_id.volume
                    && pair[1].volume == root_id.volume
                    && pair[0].directory == pair[1].directory
                    && pair[0].directory != root_id.file,
                "private_pair",
            )?;
            let gen_name = hex::encode(pair[0].generation);
            let lease = self.generation_custody(pair, &root_id, custody)?;
            let inserted = expected_gens.insert(
                gen_name,
                ExpectedGeneration {
                    pair,
                    disk_name: format!("disk-{}", hex::encode(pair[0].key)),
                    vars_name: format!("vars-{}", hex::encode(pair[1].key)),
                    lease,
                },
            );
            check(inserted.is_none(), "duplicate_generation")?;
        }

        let artifact_count = safe(
            self.db
                .query_row("SELECT count(*) FROM vm_artifacts", [], |r| {
                    r.get::<_, usize>(0)
                }),
            "artifact_inventory",
        )?;
        check(artifact_count == pairs.len() * 2, "artifact_inventory")?;

        let generation_count = safe(
            self.db
                .query_row("SELECT count(*) FROM vm_generations", [], |r| {
                    r.get::<_, usize>(0)
                }),
            "generation_inventory",
        )?;
        check(generation_count == pairs.len(), "generation_inventory")?;

        let template_count = safe(
            self.db
                .query_row("SELECT count(*) FROM vm_templates", [], |r| {
                    r.get::<_, usize>(0)
                }),
            "template_inventory",
        )?;
        check(template_count == pairs.len(), "template_inventory")?;

        let native_gen_count = safe(
            self.db
                .query_row("SELECT count(*) FROM vm_native_generations", [], |r| {
                    r.get::<_, usize>(0)
                }),
            "native_generation_inventory",
        )?;
        let native_slot_count = safe(
            self.db
                .query_row("SELECT count(*) FROM vm_native_slots", [], |r| {
                    r.get::<_, usize>(0)
                }),
            "native_slot_inventory",
        )?;
        match custody {
            GoldenCustody::Native => {
                check(
                    native_gen_count == pairs.len(),
                    "native_generation_inventory",
                )?;
                check(
                    native_slot_count == pairs.len() * 2,
                    "native_slot_inventory",
                )?;
            }
            GoldenCustody::Historic => {
                check(
                    native_gen_count == 0 && native_slot_count == 0,
                    "historic_custody_mismatch",
                )?;
            }
        }

        let mut seen_gens: BTreeSet<String> = BTreeSet::new();
        let mut seen_artifacts = 0usize;
        let mut seen_leases = 0usize;

        for (name, attrs) in root_entries {
            // The TEST foreground opens the vault on this same root. Its
            // DELETE-mode rollback journal is supported; WAL/SHM are refused.
            if matches!(
                name.as_str(),
                "activity.sqlite3"
                    | "activity.lock"
                    | "vault.sqlite3"
                    | "vault.lock"
                    | "vault.sqlite3-journal"
            ) {
                check(
                    attrs & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0,
                    "unsafe_file_identity",
                )?;
                let meta = safe(
                    nt_open_relative(
                        root_handle,
                        &name,
                        FILE_GENERIC_READ | READ_CONTROL,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        FILE_NON_DIRECTORY_FILE,
                    ),
                    "local_open",
                )?;
                let meta_id = identity(meta.as_raw_handle(), false)?;
                check(
                    meta_id.volume == root_id.volume && has_exact_leaf(&meta, &name),
                    "unsafe_file_identity",
                )?;
                continue;
            }
            // Any non-allowlisted root entry must belong to the exact expected
            // generation directory set before opening it.
            let name_json = json!(name);
            check(opaque(&name_json, 32).is_some(), "unattributed_media")?;
            let Some(expected) = expected_gens.get(&name) else {
                return Err(Failure::Check("unattributed_media"));
            };
            check(
                attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0 && attrs & FILE_ATTRIBUTE_DIRECTORY != 0,
                "unsafe_file_identity",
            )?;
            check(seen_gens.insert(name.clone()), "duplicate_generation")?;

            let dir = safe(
                nt_open_relative(
                    root_handle,
                    &name,
                    FILE_GENERIC_READ | READ_CONTROL,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    FILE_DIRECTORY_FILE,
                ),
                "local_open",
            )?;
            let dir_id = identity(dir.as_raw_handle(), true)?;
            check(
                dir_id.volume == root_id.volume
                    && dir_id.file == expected.pair[0].directory
                    && has_exact_leaf(&dir, &name),
                "generation_directory_identity",
            )?;
            if expected.lease.is_some() {
                check(held_private(&dir, owner), "generation_directory_acl")?;
            }

            let max_children = if expected.lease.is_some() { 3 } else { 2 };
            let children = listing_bounded(&dir, max_children, "unattributed_media")?;
            let mut seen_disk = false;
            let mut seen_vars = false;
            let mut seen_lease = false;

            for (child_name, child_attrs) in children {
                if child_name == GENERATION_LEASE {
                    let Some(binding) = expected.lease else {
                        return Err(Failure::Check("unattributed_media"));
                    };
                    check(!seen_lease, "unattributed_media")?;
                    check(
                        child_attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0,
                        "native_lease_reparse",
                    )?;
                    check(
                        child_attrs & FILE_ATTRIBUTE_DIRECTORY == 0,
                        "native_lease_kind",
                    )?;
                    verify_native_lease(root_handle, &root_id, &dir, &dir_id, &binding, owner)?;
                    seen_lease = true;
                    seen_leases += 1;
                    continue;
                }
                if child_name.eq_ignore_ascii_case(GENERATION_LEASE) {
                    if expected.lease.is_some() {
                        check(
                            child_attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0,
                            "native_lease_reparse",
                        )?;
                        return Err(Failure::Check("native_lease_case"));
                    }
                    return Err(Failure::Check("unattributed_media"));
                }
                check(
                    child_attrs & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0,
                    "unattributed_media",
                )?;
                let leaf = if child_name == expected.disk_name {
                    check(!seen_disk, "unattributed_media")?;
                    seen_disk = true;
                    &expected.pair[0]
                } else if child_name == expected.vars_name {
                    check(!seen_vars, "unattributed_media")?;
                    seen_vars = true;
                    &expected.pair[1]
                } else {
                    return Err(Failure::Check("unattributed_media"));
                };
                let leaf_file = safe(
                    nt_open_relative(
                        &dir,
                        &child_name,
                        FILE_GENERIC_READ | READ_CONTROL,
                        FILE_SHARE_READ,
                        FILE_NON_DIRECTORY_FILE,
                    ),
                    "local_open",
                )?;
                let leaf_id = identity(leaf_file.as_raw_handle(), false)?;
                check(
                    leaf_id.volume == leaf.volume
                        && leaf_id.file == leaf.file
                        && has_exact_leaf(&leaf_file, &child_name),
                    "artifact_identity",
                )?;
                seen_artifacts += 1;
            }
            check(seen_disk && seen_vars, "physical_inventory")?;
            if expected.lease.is_some() {
                check(seen_lease, "native_lease_missing")?;
            } else {
                check(!seen_lease, "unattributed_media")?;
            }
        }

        let expected_leases = match custody {
            GoldenCustody::Native => pairs.len(),
            GoldenCustody::Historic => 0,
        };
        check(seen_gens.len() == expected_gens.len(), "physical_inventory")?;
        check(seen_artifacts == pairs.len() * 2, "physical_inventory")?;
        check(seen_leases == expected_leases, "native_lease_inventory")?;
        Ok(InventorySummary {
            generations: seen_gens.len(),
            artifacts: seen_artifacts,
            leases: seen_leases,
        })
    }
}

#[test]
fn startup_preflight_refuses_occupied_or_foreign_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    let state = temporary.path();
    let recipes = [("ubuntu", "unused"), ("windows", "unused")];
    assert_eq!(Ledger::preflight(state, recipes), Ok(()));

    let orphan = state.join("untracked");
    std::fs::write(&orphan, b"fixture").unwrap();
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("fresh_state"))
    );

    let database = Connection::open(state.join("activity.sqlite3")).unwrap();
    database
        .execute_batch(
            "CREATE TABLE vm_instances (id INTEGER);
         CREATE TABLE vm_static_instances (id INTEGER, persistent INTEGER, staging INTEGER);
         CREATE TABLE vm_restore_points (id INTEGER, charge INTEGER);
         CREATE TABLE vm_snapshot_work (id INTEGER, persistent INTEGER, staging INTEGER);
         CREATE TABLE vm_build_intents (state TEXT, staging_bytes INTEGER);
         CREATE TABLE vm_operations (status INTEGER);
         CREATE TABLE vm_disk_claims (id INTEGER, persistent INTEGER, staging INTEGER);
         CREATE TABLE vm_templates (id BLOB, bytes INTEGER);
         CREATE TABLE vm_generations (id BLOB);
         CREATE TABLE vm_artifacts (id BLOB);
         CREATE TABLE vm_native_generations (generation BLOB, state TEXT, disposal TEXT);
         CREATE TABLE vm_native_slots (generation BLOB, state TEXT, disposal TEXT);",
        )
        .unwrap();
    database
        .pragma_update(None, "user_version", built_vm_schema_version().unwrap())
        .unwrap();
    drop(database);

    let sidecar = state.join("activity.sqlite3-journal");
    std::fs::write(&sidecar, b"fixture").unwrap();
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("unsettled_startup_ledger"))
    );
    std::fs::remove_file(sidecar).unwrap();

    // The same foreign file must also fail with an existing, idle ledger.
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&orphan).unwrap();
    let dir = state.join("aa".repeat(16));
    std::fs::create_dir(&dir).unwrap();
    let orphan = dir.join("untracked");
    std::fs::write(&orphan, b"fixture").unwrap();
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&orphan).unwrap();
    // Empty 32-hex directories are never waived by a wildcard.
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_dir(&dir).unwrap();
    assert_eq!(Ledger::preflight(state, recipes), Ok(()));

    let database = Connection::open(state.join("activity.sqlite3")).unwrap();
    database
        .execute("INSERT INTO vm_instances VALUES (1)", [])
        .unwrap();
    drop(database);
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("ledger_not_isolated"))
    );
}

#[test]
fn ledger_accepts_the_store_version_written_by_the_built_crate() {
    use arxd::vm::{Clock, ClockReading, VmController, VmPolicy};

    struct StoreClock;

    impl Clock for StoreClock {
        fn now(&self) -> ClockReading {
            ClockReading {
                utc_ms: 1_800_000_000_000,
                monotonic: std::time::Instant::now(),
            }
        }
    }

    let temporary = private_fixture::private_dir();
    let state = temporary.path();
    let recipes = [("ubuntu", "unused"), ("windows", "unused")];
    let controller = VmController::open(state, VmPolicy::default(), StoreClock).unwrap();

    // Check the reader against a real freshly migrated store. Read the
    // version from that store rather than stamping it with the parser.
    let path = state.join("activity.sqlite3");
    let database = Connection::open(&path).unwrap();
    let version: u32 = database
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(built_vm_schema_version(), Ok(version));
    drop(database);
    assert!(Ledger::open(state).is_ok());
    drop(controller);
    assert_eq!(Ledger::preflight(state, recipes), Ok(()));

    for name in ["vault.sqlite3", "vault.lock", "vault.sqlite3-journal"] {
        std::fs::write(state.join(name), b"vault fixture").unwrap();
    }
    assert_eq!(Ledger::preflight(state, recipes), Ok(()));
    for name in [
        "vault.sqlite3-backup",
        "vault.sqlite3-wal",
        "vault.sqlite3-shm",
    ] {
        let unknown = state.join(name);
        std::fs::write(&unknown, b"foreign fixture").unwrap();
        assert_eq!(
            Ledger::preflight(state, recipes),
            Err(Failure::Check("unattributed_media"))
        );
        std::fs::remove_file(unknown).unwrap();
    }
    let vault_lock = state.join("vault.lock");
    std::fs::remove_file(&vault_lock).unwrap();
    std::fs::create_dir(&vault_lock).unwrap();
    assert!(Ledger::preflight(state, recipes).is_err());
    std::fs::remove_dir(vault_lock).unwrap();

    for unsupported in [0, version - 1, version + 1] {
        let database = Connection::open(&path).unwrap();
        database
            .pragma_update(None, "user_version", unsupported)
            .unwrap();
        drop(database);
        let original = std::fs::read(&path).unwrap();
        assert!(matches!(
            Ledger::open(state),
            Err(Failure::Check("ledger_version"))
        ));
        assert_eq!(
            Ledger::preflight(state, recipes),
            Err(Failure::Check("ledger_version"))
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

#[cfg(test)]
mod inventory_fixtures {
    use super::*;
    use arxd::vm::{Clock, ClockReading, VmController, VmPolicy};
    use std::process::Stdio;

    const UBUNTU_RECIPE: &str = "ubuntu-test-v1";
    const WINDOWS_RECIPE: &str = "3c2fe2069942ce6c6cebe5c3e700e758";

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> ClockReading {
            ClockReading {
                utc_ms: 1_800_000_000_000,
                monotonic: std::time::Instant::now(),
            }
        }
    }

    pub(super) struct SeededGolden {
        pub template: Id,
        pub generation: Id,
        pub recipe_digest: Hash,
        pub charge: i64,
        pub dir: PathBuf,
        pub lease: PathBuf,
    }

    pub(super) fn qcow2_unbacked(tag: u8) -> Vec<u8> {
        let mut cluster = vec![0u8; 65_536];
        cluster[0..4].copy_from_slice(b"QFI\xfb");
        cluster[4..8].copy_from_slice(&3u32.to_be_bytes());
        cluster[20..24].copy_from_slice(&16u32.to_be_bytes());
        cluster[24..32].copy_from_slice(&(64 * 1024 * 1024u64).to_be_bytes());
        cluster[96..100].copy_from_slice(&4u32.to_be_bytes());
        cluster[100..104].copy_from_slice(&104u32.to_be_bytes());
        cluster[4096] = tag;
        cluster
    }

    fn sha256(bytes: &[u8]) -> Hash {
        Sha256::digest(bytes).into()
    }

    pub(super) fn open_v16_store() -> (tempfile::TempDir, PathBuf) {
        let dir = private_fixture::private_dir();
        let state = fixture_drive_path(dir.path()).join("state");
        std::fs::create_dir(&state).unwrap();
        fixture_set_owner(&[dir.path(), &state]);
        let controller = VmController::open(&state, VmPolicy::default(), FixedClock).unwrap();
        drop(controller);
        (dir, state)
    }

    pub(super) fn seed_golden(state: &Path, guest: i64, tag: u8, native: bool) -> SeededGolden {
        let recipe_id = if guest == 1 {
            UBUNTU_RECIPE
        } else {
            WINDOWS_RECIPE
        };
        let template: Id = [tag; 16];
        let generation: Id = [tag.wrapping_add(0x10); 16];
        let disk_key: Id = [tag.wrapping_add(0x20); 16];
        let vars_key: Id = [tag.wrapping_add(0x30); 16];
        let recipe_digest: Hash = [tag.wrapping_add(0x40); 32];

        let gen_dir = state.join(hex::encode(generation));
        std::fs::create_dir(&gen_dir).unwrap();
        fixture_set_owner(&[&gen_dir]);

        let disk_bytes = qcow2_unbacked(tag);
        let vars_bytes = vec![tag; 4096];
        let disk_digest = sha256(&disk_bytes);
        let vars_digest = sha256(&vars_bytes);
        let disk_path = gen_dir.join(format!("disk-{}", hex::encode(disk_key)));
        let vars_path = gen_dir.join(format!("vars-{}", hex::encode(vars_key)));
        std::fs::write(&disk_path, &disk_bytes).unwrap();
        std::fs::write(&vars_path, &vars_bytes).unwrap();
        fixture_set_owner(&[&disk_path, &vars_path]);

        let root_file = open(state, true, FILE_SHARE_READ | FILE_SHARE_WRITE).unwrap();
        let root_id = identity(root_file.as_raw_handle(), true).unwrap();
        let dir_file = open(&gen_dir, true, FILE_SHARE_READ | FILE_SHARE_WRITE).unwrap();
        let dir_id = identity(dir_file.as_raw_handle(), true).unwrap();
        let disk_file = open(&disk_path, false, FILE_SHARE_READ).unwrap();
        let disk_id = identity(disk_file.as_raw_handle(), false).unwrap();
        let vars_file = open(&vars_path, false, FILE_SHARE_READ).unwrap();
        let vars_id = identity(vars_file.as_raw_handle(), false).unwrap();
        drop((root_file, dir_file, disk_file, vars_file));

        let lease_path = gen_dir.join(GENERATION_LEASE);
        let lease_id = if native {
            std::fs::write(&lease_path, b"").unwrap();
            fixture_set_owner(&[&lease_path]);
            let lease_file = open(&lease_path, false, FILE_SHARE_READ).unwrap();
            let id = identity(lease_file.as_raw_handle(), false).unwrap();
            drop(lease_file);
            Some(id)
        } else {
            None
        };

        let charge = i64::try_from(disk_bytes.len() + vars_bytes.len()).unwrap();
        let db = Connection::open(state.join("activity.sqlite3")).unwrap();
        db.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        db.execute(
            "INSERT INTO vm_templates(id,guest,recipe_digest,disk_digest,vars_digest,bytes)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                template,
                guest,
                recipe_digest,
                disk_digest,
                vars_digest,
                charge
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO vm_build_intents(id,operation_id,recipe_id,recipe_digest,staging_bytes,template_id,health_verified,state)
             VALUES(?1,NULL,?2,?3,0,?1,1,'sealed')",
            params![template, recipe_id, recipe_digest],
        )
        .unwrap();
        // Insert generation with active=0 first so vm_native_origin trigger
        // accepts native custody, then activate it as the sealed golden.
        db.execute(
            "INSERT INTO vm_generations(id,build_id,vm_id,static_id,generation,active)
             VALUES(?1,?2,NULL,NULL,1,0)",
            params![generation, template],
        )
        .unwrap();
        db.execute(
            "INSERT INTO vm_artifacts(generation_id,role,object_key,state,volume_id,directory_id,file_id,content_digest,bytes)
             VALUES(?1,'disk',?2,'planned',NULL,NULL,NULL,NULL,NULL),
                   (?1,'vars',?3,'planned',NULL,NULL,NULL,NULL,NULL)",
            params![generation, disk_key, vars_key],
        )
        .unwrap();
        if let Some(lease_id) = lease_id {
            db.execute(
                "INSERT INTO vm_native_generations(generation,host,origin,protocol,volume,production,state,directory,lease,disposal)
                 SELECT ?1,id,'build',1,?2,?3,'bound',?4,?5,NULL FROM vm_state_host WHERE singleton=1",
                params![generation, root_id.volume, root_id.file, dir_id.file, lease_id.file],
            )
            .unwrap();
            db.execute(
                "INSERT INTO vm_native_slots(generation,role,object_key,state,file,bytes,content_digest,disposal)
                 VALUES(?1,'disk',?2,'verified',?3,?4,?5,NULL),
                       (?1,'vars',?6,'verified',?7,?8,?9,NULL)",
                params![
                    generation,
                    disk_key,
                    disk_id.file,
                    disk_bytes.len() as i64,
                    disk_digest,
                    vars_key,
                    vars_id.file,
                    vars_bytes.len() as i64,
                    vars_digest,
                ],
            )
            .unwrap();
        }
        db.execute(
            "UPDATE vm_artifacts SET state='sealed',volume_id=?3,directory_id=?4,file_id=?5,content_digest=?6,bytes=?7
             WHERE generation_id=?1 AND role='disk' AND object_key=?2",
            params![
                generation,
                disk_key,
                disk_id.volume,
                dir_id.file,
                disk_id.file,
                disk_digest,
                disk_bytes.len() as i64,
            ],
        )
        .unwrap();
        db.execute(
            "UPDATE vm_artifacts SET state='sealed',volume_id=?3,directory_id=?4,file_id=?5,content_digest=?6,bytes=?7
             WHERE generation_id=?1 AND role='vars' AND object_key=?2",
            params![
                generation,
                vars_key,
                vars_id.volume,
                dir_id.file,
                vars_id.file,
                vars_digest,
                vars_bytes.len() as i64,
            ],
        )
        .unwrap();
        db.execute(
            "UPDATE vm_generations SET active=1 WHERE id=?1",
            [generation],
        )
        .unwrap();
        drop(db);

        SeededGolden {
            template,
            generation,
            recipe_digest,
            charge,
            dir: gen_dir,
            lease: lease_path,
        }
    }

    pub(super) fn recipes_for(ubuntu: &SeededGolden, windows: &SeededGolden) -> (String, String) {
        (
            hex::encode(ubuntu.recipe_digest),
            hex::encode(windows.recipe_digest),
        )
    }

    pub(super) fn run_icacls(args: &[&std::ffi::OsStr]) {
        let status = std::process::Command::new("icacls")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "icacls failed");
    }

    pub(super) fn restore_private_acl(path: &Path) {
        run_icacls(&[path.as_os_str(), std::ffi::OsStr::new("/reset")]);
        fixture_set_owner(&[path]);
    }
}

#[test]
fn native_two_golden_inventory_accepts_exact_layout_and_counts_leases_separately() {
    use inventory_fixtures::*;

    let (_root, state) = open_v16_store();
    let ubuntu = seed_golden(&state, 1, 0x11, true);
    let windows = seed_golden(&state, 2, 0x22, true);
    let (u_pin, w_pin) = recipes_for(&ubuntu, &windows);
    let recipes = [
        ("ubuntu-test-v1", u_pin.as_str()),
        ("3c2fe2069942ce6c6cebe5c3e700e758", w_pin.as_str()),
    ];

    assert_eq!(Ledger::preflight(&state, recipes), Ok(()));

    let expected = fixture_expected_store(&state);
    let live = LiveLedger::open(&state, &expected).unwrap();
    let summary = live
        .read(|ledger| {
            let (u_pair, u_charge) =
                ledger.golden(ubuntu.template, recipes[0].0, recipes[0].1, 1)?;
            let (w_pair, w_charge) =
                ledger.golden(windows.template, recipes[1].0, recipes[1].1, 2)?;
            assert_eq!(u_charge, ubuntu.charge);
            assert_eq!(w_charge, windows.charge);
            ledger.distinct(&u_pair, &w_pair)?;
            ledger.inspect(&u_pair, None, true)?;
            ledger.inspect(&w_pair, None, true)?;
            ledger.quiet()?;
            let summary = ledger.inventory(&[&u_pair, &w_pair])?;
            assert_eq!(ledger.aggregate()?, u_charge + w_charge);
            Ok(summary)
        })
        .unwrap();
    assert_eq!(
        summary,
        InventorySummary {
            generations: 2,
            artifacts: 4,
            leases: 2,
        }
    );
    drop(live);

    // Populated two-golden store must still reject old, zero, and newer schema
    // versions without mutating the database bytes.
    let db_path = state.join("activity.sqlite3");
    let current = built_vm_schema_version().unwrap();
    for unsupported in [0, current - 1, current + 1] {
        let db = Connection::open(&db_path).unwrap();
        db.pragma_update(None, "user_version", unsupported).unwrap();
        drop(db);
        let bytes = std::fs::read(&db_path).unwrap();
        assert_eq!(
            Ledger::preflight(&state, recipes),
            Err(Failure::Check("ledger_version"))
        );
        assert_eq!(std::fs::read(&db_path).unwrap(), bytes);
    }
    let db = Connection::open(&db_path).unwrap();
    db.pragma_update(None, "user_version", current).unwrap();
    drop(db);
    assert_eq!(Ledger::preflight(&state, recipes), Ok(()));
}

#[test]
fn native_inventory_refuses_extras_empty_dirs_lease_only_dirs_and_retired_residue() {
    use inventory_fixtures::*;

    let (_root, state) = open_v16_store();
    let ubuntu = seed_golden(&state, 1, 0x31, true);
    let windows = seed_golden(&state, 2, 0x42, true);
    let (u_pin, w_pin) = recipes_for(&ubuntu, &windows);
    let recipes = [
        ("ubuntu-test-v1", u_pin.as_str()),
        ("3c2fe2069942ce6c6cebe5c3e700e758", w_pin.as_str()),
    ];
    let load_pairs = |ledger: &Ledger| {
        let (u, _) = ledger
            .golden(ubuntu.template, recipes[0].0, recipes[0].1, 1)
            .unwrap();
        let (w, _) = ledger
            .golden(windows.template, recipes[1].0, recipes[1].1, 2)
            .unwrap();
        (u, w)
    };
    assert_eq!(Ledger::preflight(&state, recipes), Ok(()));

    // 1. Extra root file.
    let stray = state.join("stray.tmp");
    std::fs::write(&stray, b"extra").unwrap();
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&stray).unwrap();

    // 2. Activity sidecars refused by both startup preflight and inventory.
    for suffix in ["-journal", "-wal", "-shm"] {
        let ledger = Ledger::open(&state).unwrap();
        let (u, w) = load_pairs(&ledger);
        let sidecar = state.join(format!("activity.sqlite3{suffix}"));
        std::fs::write(&sidecar, b"sidecar").unwrap();
        assert_eq!(
            Ledger::preflight(&state, recipes),
            Err(Failure::Check("unsettled_startup_ledger"))
        );
        assert_eq!(
            ledger.inventory(&[&u, &w]),
            Err(Failure::Check("unsettled_ledger_sidecar"))
        );
        drop(ledger);
        std::fs::remove_file(&sidecar).unwrap();
    }

    // 3. Empty extra 32-hex directory and lease-only 32-hex directory: never waived.
    let extra_dir = state.join("bb".repeat(16));
    std::fs::create_dir(&extra_dir).unwrap();
    fixture_set_owner(&[&extra_dir]);
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    let extra_lease = extra_dir.join(GENERATION_LEASE);
    std::fs::write(&extra_lease, b"").unwrap();
    fixture_set_owner(&[&extra_lease]);
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&extra_lease).unwrap();
    std::fs::remove_dir(&extra_dir).unwrap();

    // 4. Extra sibling inside a golden generation directory.
    let extra_seed = ubuntu.dir.join(format!("seed-{}", "cc".repeat(16)));
    std::fs::write(&extra_seed, b"transient").unwrap();
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&extra_seed).unwrap();

    // 5. Retired generation lifecycle: directory, lease, and media residue all refused.
    let retired_gen: Id = [0x99; 16];
    let retired_disk_key: Id = [0x9a; 16];
    let retired_vars_key: Id = [0x9b; 16];
    let retired_dir = state.join(hex::encode(retired_gen));
    std::fs::create_dir(&retired_dir).unwrap();
    fixture_set_owner(&[&retired_dir]);
    let retired_lease = retired_dir.join(GENERATION_LEASE);
    let retired_disk = retired_dir.join(format!("disk-{}", hex::encode(retired_disk_key)));
    let retired_vars = retired_dir.join(format!("vars-{}", hex::encode(retired_vars_key)));
    std::fs::write(&retired_lease, b"").unwrap();
    std::fs::write(&retired_disk, b"retired-disk").unwrap();
    std::fs::write(&retired_vars, b"retired-vars").unwrap();
    fixture_set_owner(&[&retired_lease, &retired_disk, &retired_vars]);
    let rdir_id = identity(
        open(&retired_dir, true, FILE_SHARE_READ | FILE_SHARE_WRITE)
            .unwrap()
            .as_raw_handle(),
        true,
    )
    .unwrap();
    let rdisk_id = identity(
        open(&retired_disk, false, FILE_SHARE_READ)
            .unwrap()
            .as_raw_handle(),
        false,
    )
    .unwrap();
    let rvars_id = identity(
        open(&retired_vars, false, FILE_SHARE_READ)
            .unwrap()
            .as_raw_handle(),
        false,
    )
    .unwrap();
    let retired_pair: Pair = [
        Leaf {
            generation: retired_gen,
            role: "disk",
            key: retired_disk_key,
            volume: rdisk_id.volume,
            directory: rdir_id.file,
            file: rdisk_id.file,
            digest: None,
            bytes: None,
        },
        Leaf {
            generation: retired_gen,
            role: "vars",
            key: retired_vars_key,
            volume: rvars_id.volume,
            directory: rdir_id.file,
            file: rvars_id.file,
            digest: None,
            bytes: None,
        },
    ];

    // While vm_generations still holds the row, gone() reports false (pending sweep).
    let db = Connection::open(state.join("activity.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO vm_generations(id,build_id,vm_id,static_id,generation,active)
         VALUES(?1,?2,NULL,NULL,2,0)",
        params![retired_gen, ubuntu.template],
    )
    .unwrap();
    drop(db);
    let ledger = Ledger::open(&state).unwrap();
    assert_eq!(ledger.gone(&retired_pair), Ok(false));
    let (u, w) = load_pairs(&ledger);
    assert_eq!(
        ledger.inventory(&[&u, &w]),
        Err(Failure::Check("generation_inventory"))
    );
    drop(ledger);

    // Once the ledger row is retired, remaining lease, media, or empty dir MUST fail.
    let db = Connection::open(state.join("activity.sqlite3")).unwrap();
    db.execute("DELETE FROM vm_generations WHERE id=?1", [retired_gen])
        .unwrap();
    drop(db);
    let ledger = Ledger::open(&state).unwrap();
    assert_eq!(
        ledger.gone(&retired_pair),
        Err(Failure::Check("retired_lease_still_present"))
    );
    let (u, w) = load_pairs(&ledger);
    assert_eq!(
        ledger.inventory(&[&u, &w]),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&retired_lease).unwrap();
    assert_eq!(
        ledger.gone(&retired_pair),
        Err(Failure::Check("retired_media_still_present"))
    );
    std::fs::remove_file(&retired_disk).unwrap();
    std::fs::remove_file(&retired_vars).unwrap();
    assert_eq!(
        ledger.gone(&retired_pair),
        Err(Failure::Check("retired_directory_still_present"))
    );
    assert_eq!(
        ledger.inventory(&[&u, &w]),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_dir(&retired_dir).unwrap();
    assert_eq!(ledger.gone(&retired_pair), Ok(true));
    assert_eq!(
        ledger.inventory(&[&u, &w]),
        Ok(InventorySummary {
            generations: 2,
            artifacts: 4,
            leases: 2,
        })
    );
    drop(ledger);
    assert_eq!(Ledger::preflight(&state, recipes), Ok(()));
}

#[test]
fn native_inventory_refuses_every_invalid_or_busy_lease_substitution() {
    use inventory_fixtures::*;

    let (outer, state) = open_v16_store();
    let ubuntu = seed_golden(&state, 1, 0x51, true);
    let windows = seed_golden(&state, 2, 0x62, true);
    let (u_pin, w_pin) = recipes_for(&ubuntu, &windows);
    let recipes = [
        ("ubuntu-test-v1", u_pin.as_str()),
        ("3c2fe2069942ce6c6cebe5c3e700e758", w_pin.as_str()),
    ];
    let expected_ok = || assert_eq!(Ledger::preflight(&state, recipes), Ok(()));
    expected_ok();

    // 1. Locked lease (exact runtime ProductionWorkspace lock mode).
    {
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&ubuntu.lease)
            .unwrap();
        held.try_lock().unwrap();
        assert_eq!(
            Ledger::preflight(&state, recipes),
            Err(Failure::Check("native_lease_busy"))
        );
        held.unlock().unwrap();
    }
    expected_ok();

    // 2. Open (unlocked) reader handle on the lease.
    {
        let reader = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&ubuntu.lease)
            .unwrap();
        assert_eq!(
            Ledger::preflight(&state, recipes),
            Err(Failure::Check("native_lease_busy"))
        );
        drop(reader);
    }
    expected_ok();

    // 3. Open (unlocked) writer handle on the lease.
    {
        let writer = OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&ubuntu.lease)
            .unwrap();
        assert_eq!(
            Ledger::preflight(&state, recipes),
            Err(Failure::Check("native_lease_busy"))
        );
        drop(writer);
    }
    expected_ok();

    // 4. Nonprivate ACL (Everyone read, then world-writable).
    run_icacls(&[
        ubuntu.lease.as_os_str(),
        std::ffi::OsStr::new("/grant"),
        std::ffi::OsStr::new("*S-1-1-0:(R)"),
    ]);
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("native_lease_acl"))
    );
    run_icacls(&[
        ubuntu.lease.as_os_str(),
        std::ffi::OsStr::new("/grant"),
        std::ffi::OsStr::new("*S-1-1-0:(M)"),
    ]);
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("native_lease_acl"))
    );
    restore_private_acl(&ubuntu.lease);
    expected_ok();

    // 4b. Nonprivate ACL on the generation directory itself (non-inheriting
    // grant so ubuntu.lease stays private and only generation_directory_acl trips).
    run_icacls(&[
        ubuntu.dir.as_os_str(),
        std::ffi::OsStr::new("/grant"),
        std::ffi::OsStr::new("*S-1-1-0:(R)"),
    ]);
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("generation_directory_acl"))
    );
    restore_private_acl(&ubuntu.dir);
    expected_ok();

    // 5. Populated (non-zero length) lease keeping the exact same file ID and ACL.
    {
        let mut f = OpenOptions::new().write(true).open(&ubuntu.lease).unwrap();
        std::io::Write::write_all(&mut f, b"x").unwrap();
        f.sync_all().unwrap();
    }
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("native_lease_length"))
    );
    {
        let f = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&ubuntu.lease)
            .unwrap();
        f.sync_all().unwrap();
    }
    expected_ok();

    // 6. Hard-linked lease (NumberOfLinks == 2, same file ID, zero length, private ACL).
    let sibling_link = fixture_drive_path(outer.path()).join("lease-hardlink");
    std::fs::hard_link(&ubuntu.lease, &sibling_link).unwrap();
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("native_lease_links"))
    );
    std::fs::remove_file(&sibling_link).unwrap();
    expected_ok();

    // 7. Case-changed lease (in-place rename preserves file ID, zero length, ACL, 1 link).
    let wrong_case = ubuntu.dir.join("Generation.Lease");
    std::fs::rename(&ubuntu.lease, &wrong_case).unwrap();
    {
        let f = open(&wrong_case, false, FILE_SHARE_READ).unwrap();
        assert!(has_exact_leaf(&f, "Generation.Lease"));
    }
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("native_lease_case"))
    );
    std::fs::rename(&wrong_case, &ubuntu.lease).unwrap();
    expected_ok();

    // 8. Missing lease, then reparse point lease, then wrong-identity recreated lease.
    let saved_lease = fixture_drive_path(outer.path()).join("saved-ubuntu-lease");
    std::fs::rename(&ubuntu.lease, &saved_lease).unwrap();
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("native_lease_missing"))
    );

    let junction_target = fixture_drive_path(outer.path()).join("junction-target");
    std::fs::create_dir(&junction_target).unwrap();
    assert!(
        std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&ubuntu.lease)
            .arg(&junction_target)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("native_lease_reparse"))
    );
    std::fs::remove_dir(&ubuntu.lease).unwrap();
    std::fs::remove_dir(&junction_target).unwrap();

    // Fresh empty private lease with a different NTFS file ID.
    std::fs::write(&ubuntu.lease, b"").unwrap();
    fixture_set_owner(&[&ubuntu.lease]);
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("native_lease_identity"))
    );
    std::fs::remove_file(&ubuntu.lease).unwrap();

    // Restore original lease handle/file ID by moving back.
    std::fs::rename(&saved_lease, &ubuntu.lease).unwrap();
    expected_ok();

    // 9. Reparse point on the generation directory itself.
    let moved_gen = fixture_drive_path(outer.path()).join("moved-gen");
    std::fs::rename(&ubuntu.dir, &moved_gen).unwrap();
    assert!(
        std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&ubuntu.dir)
            .arg(&moved_gen)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    assert!(Ledger::preflight(&state, recipes).is_err());
    std::fs::remove_dir(&ubuntu.dir).unwrap();
    std::fs::rename(&moved_gen, &ubuntu.dir).unwrap();
    expected_ok();

    // 10. Wrong volume / production / directory / lease binding in vm_native_generations
    // and unsettled disposal checkpoint.
    let db_path = state.join("activity.sqlite3");
    let db = Connection::open(&db_path).unwrap();
    // Simulate a native generation whose disposal checkpoint is unsettled.
    db.execute(
        "UPDATE vm_native_slots SET disposal='absent' WHERE generation=?1",
        [ubuntu.generation],
    )
    .unwrap();
    db.execute(
        "UPDATE vm_native_generations SET disposal='lease' WHERE generation=?1",
        [ubuntu.generation],
    )
    .unwrap();
    drop(db);
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("ledger_not_isolated"))
    );
    let ledger = Ledger::open(&state).unwrap();
    let (u_pair, _) = ledger
        .golden(ubuntu.template, recipes[0].0, recipes[0].1, 1)
        .unwrap();
    let (w_pair, _) = ledger
        .golden(windows.template, recipes[1].0, recipes[1].1, 2)
        .unwrap();
    assert_eq!(
        ledger.inventory(&[&u_pair, &w_pair]),
        Err(Failure::Check("native_custody_scope"))
    );
    drop(ledger);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "UPDATE vm_native_generations SET disposal=NULL WHERE generation=?1",
        [ubuntu.generation],
    )
    .unwrap();
    db.execute(
        "UPDATE vm_native_slots SET disposal=NULL WHERE generation=?1",
        [ubuntu.generation],
    )
    .unwrap();
    drop(db);
    expected_ok();

    // Wrong production root binding in a separate native store.
    let (_outer2, bad_scope_state) = open_v16_store();
    let bad_u = seed_golden(&bad_scope_state, 1, 0xb1, true);
    let bad_w = seed_golden(&bad_scope_state, 2, 0xb2, true);
    let (bu_pin, bw_pin) = recipes_for(&bad_u, &bad_w);
    let bad_recipes = [
        ("ubuntu-test-v1", bu_pin.as_str()),
        ("3c2fe2069942ce6c6cebe5c3e700e758", bw_pin.as_str()),
    ];
    assert_eq!(Ledger::preflight(&bad_scope_state, bad_recipes), Ok(()));
    let moved_state = fixture_drive_path(_outer2.path()).join("state-relocated");
    // Copying the store into a new directory changes the production root file ID,
    // which vm_native_generations.production pins.
    std::fs::create_dir(&moved_state).unwrap();
    fixture_set_owner(&[&moved_state]);
    std::fs::copy(
        bad_scope_state.join("activity.sqlite3"),
        moved_state.join("activity.sqlite3"),
    )
    .unwrap();
    std::fs::rename(&bad_u.dir, moved_state.join(hex::encode(bad_u.generation))).unwrap();
    std::fs::rename(&bad_w.dir, moved_state.join(hex::encode(bad_w.generation))).unwrap();
    assert_eq!(
        Ledger::preflight(&moved_state, bad_recipes),
        Err(Failure::Check("native_custody_scope"))
    );
}

#[test]
fn historic_goldens_are_separated_from_native_custody_and_never_bound_silently() {
    use inventory_fixtures::*;

    let (_root, state) = open_v16_store();
    let ubuntu = seed_golden(&state, 1, 0x71, false);
    let windows = seed_golden(&state, 2, 0x82, false);
    let (u_pin, w_pin) = recipes_for(&ubuntu, &windows);
    let recipes = [
        ("ubuntu-test-v1", u_pin.as_str()),
        ("3c2fe2069942ce6c6cebe5c3e700e758", w_pin.as_str()),
    ];
    let db_path = state.join("activity.sqlite3");
    let before_bytes = std::fs::read(&db_path).unwrap();

    // Historic layout MUST fail native preflight and native inventory without
    // modifying the database (never silently binds).
    assert_eq!(
        Ledger::preflight(&state, recipes),
        Err(Failure::Check("historic_golden_not_native"))
    );
    let ledger = Ledger::open(&state).unwrap();
    let (u_pair, _) = ledger
        .golden(ubuntu.template, recipes[0].0, recipes[0].1, 1)
        .unwrap();
    let (w_pair, _) = ledger
        .golden(windows.template, recipes[1].0, recipes[1].1, 2)
        .unwrap();
    assert_eq!(
        ledger.inventory(&[&u_pair, &w_pair]),
        Err(Failure::Check("historic_golden_not_native"))
    );
    drop(ledger);
    assert_eq!(std::fs::read(&db_path).unwrap(), before_bytes);

    // Explicit historic observation contract accepts the old layout with 0 leases.
    assert_eq!(
        Ledger::observe_historic_preflight(&state, recipes),
        Ok(InventorySummary {
            generations: 2,
            artifacts: 4,
            leases: 0,
        })
    );

    // Historic observation still enforces exact directory set and forbids leases.
    let stray_dir = state.join("dd".repeat(16));
    std::fs::create_dir(&stray_dir).unwrap();
    assert_eq!(
        Ledger::observe_historic_preflight(&state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_dir(&stray_dir).unwrap();

    std::fs::write(&ubuntu.lease, b"").unwrap();
    assert_eq!(
        Ledger::observe_historic_preflight(&state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&ubuntu.lease).unwrap();
    assert!(Ledger::observe_historic_preflight(&state, recipes).is_ok());

    // Mixed store (one native golden + one historic golden) is refused under
    // BOTH native and historic contracts.
    let (_root2, mixed) = open_v16_store();
    let u_native = seed_golden(&mixed, 1, 0x91, true);
    let w_historic = seed_golden(&mixed, 2, 0xa2, false);
    let (mu_pin, mw_pin) = recipes_for(&u_native, &w_historic);
    let mixed_recipes = [
        ("ubuntu-test-v1", mu_pin.as_str()),
        ("3c2fe2069942ce6c6cebe5c3e700e758", mw_pin.as_str()),
    ];
    assert_eq!(
        Ledger::preflight(&mixed, mixed_recipes),
        Err(Failure::Check("historic_golden_not_native"))
    );
    assert_eq!(
        Ledger::observe_historic_preflight(&mixed, mixed_recipes),
        Err(Failure::Check("native_golden_in_historic_scan"))
    );
}

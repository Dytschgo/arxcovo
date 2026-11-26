//! Candidate Windows mechanisms. The runtime has no production issuer.
#![allow(
    dead_code,
    reason = "private native adapter awaits owner qualification and slice 4 wiring"
)]

use super::{
    guest::GuestChannel,
    paths::{ArtifactPin, ArtifactRole, FileIdentity, LocalPath},
    qemu_args::{CommandPlan, InstallerArtifact, PlanKind, SerialName},
    qmp::{ChannelError, Qmp},
    runtime::{
        FRESHNESS, InstallerFailure, InstallerFailureReason, InstallerStage, NativeRefusal,
        StopDisposition, StopEvidence, fresh_observation,
    },
    selection_epoch::Invalidation,
};
use crate::{security::vm_acl, service::Shutdown};
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
    time::Duration,
};
use windows_sys::Wdk::{
    Foundation::OBJECT_ATTRIBUTES,
    Storage::FileSystem::{
        FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
        FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authentication::Identity::*, Authorization::ConvertStringSidToSidW, *},
    Storage::FileSystem::*,
    System::{
        IO::{CancelSynchronousIo, IO_STATUS_BLOCK},
        JobObjects::*,
        Pipes::*,
        RemoteDesktop::*,
        SystemInformation::*,
        Threading::*,
    },
};

mod desktop;
pub(super) mod offline_read;
pub mod qualification;
use qualification::WorkspaceCustody;

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn succeeded(ok: windows_sys::core::BOOL) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Takes a fresh, uniquely owned Win32 handle.
unsafe fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        // The caller transfers ownership of the valid handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}

struct PathPin {
    _ancestors: Vec<File>,
    leaf: File,
    identity: FileIdentity,
    allocated: u64,
    path: LocalPath,
    mutable: bool,
    verified_content: Option<ArtifactPin>,
}

const MAX_SETUP_VARS_BYTES: u64 = 16 * 1024 * 1024;

impl PathPin {
    fn open(path: &LocalPath, directory: bool, mutable: bool) -> Result<Self, NativeRefusal> {
        Self::pin(path, directory, mutable, false).map_err(|_| NativeRefusal::PathCustodyUnverified)
    }

    fn create(path: &LocalPath) -> Result<Self, NativeRefusal> {
        Self::pin(path, false, true, true).map_err(|_| NativeRefusal::PathCustodyUnverified)
    }

    fn pin(path: &LocalPath, directory: bool, mutable: bool, create: bool) -> io::Result<Self> {
        Self::pin_observed(path, directory, mutable, create, &mut |_| {})
    }

    /// Walks from the volume root, opening each component relative to the
    /// already-checked parent handle that the pin keeps open. `observe` runs
    /// after each directory is pinned; tests use it to race the walk.
    fn pin_observed(
        path: &LocalPath,
        directory: bool,
        mutable: bool,
        create: bool,
        observe: &mut dyn FnMut(&str),
    ) -> io::Result<Self> {
        let raw = path.as_str();
        let root = &raw[..3];
        // A drive alias for a network, optical, or removable device is refused.
        if unsafe { GetDriveTypeW(wide(root).as_ptr()) } != 3 {
            return Err(io::Error::other("expected a fixed local volume"));
        }
        // The volume root is the only open by absolute name, and it must be
        // the volume's own root object.
        let root_file = open_directory(root)?;
        let (root_identity, root_allocated) = file_identity(&root_file, true)?;
        verify_ntfs(&root_file)?;
        verify_volume_root(root, root_identity)?;
        if raw.len() == 3 {
            if create || !directory {
                return Err(io::Error::other("the volume root is a directory"));
            }
            return Ok(Self {
                _ancestors: Vec::new(),
                leaf: root_file,
                identity: root_identity,
                allocated: root_allocated,
                path: path.clone(),
                mutable,
                verified_content: None,
            });
        }
        observe(root);
        let names: Vec<&str> = raw[3..].split('\\').collect();
        let mut ancestors = vec![root_file];
        let mut end = 2;
        for (index, name) in names.iter().enumerate() {
            end += 1 + name.len();
            let last = index + 1 == names.len();
            let node = if last && !directory {
                Node::File
            } else {
                Node::Directory
            };
            let parent = ancestors.last().expect("the volume root is pinned");
            let file = open_relative(parent, name, node, last && mutable, last && create)?;
            let (identity, allocated) = file_identity(&file, node == Node::Directory)?;
            if identity.volume != root_identity.volume {
                return Err(io::Error::other("unexpected volume transition"));
            }
            if last {
                // Reopen through the same held parent and compare the object.
                let check = open_relative(parent, name, node, mutable, false)?;
                if file_identity(&check, directory)?.0 != identity {
                    return Err(io::Error::other("path binding changed"));
                }
                return Ok(Self {
                    _ancestors: ancestors,
                    leaf: file,
                    identity,
                    allocated,
                    path: path.clone(),
                    mutable,
                    verified_content: None,
                });
            }
            ancestors.push(file);
            observe(&raw[..end]);
        }
        Err(io::Error::other("path has no leaf"))
    }

    /// Verify the recipe pin through the retained object before launch.
    fn verify_content(&mut self, expected: ArtifactPin, limit: u64) -> Result<(), NativeRefusal> {
        use std::io::{Seek, SeekFrom};
        self.verified_content = None;
        let verified = self
            .leaf
            .seek(SeekFrom::Start(0))
            .and_then(|_| expected.verify(&self.leaf, limit));
        if !matches!(verified, Ok(true)) {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        self.verified_content = Some(expected);
        Ok(())
    }

    fn reopen_without_writer(&self) -> io::Result<File> {
        let parent = self
            ._ancestors
            .last()
            .ok_or_else(|| io::Error::other("no held parent"))?;
        let name = self.path.as_str().rsplit('\\').next().unwrap_or_default();
        let file = open_relative(parent, name, Node::File, false, false)?;
        if file_identity(&file, false)?.0 != self.identity {
            return Err(io::Error::other("pinned identity changed"));
        }
        Ok(file)
    }
}

fn bind_installer_pin(
    expected: &InstallerArtifact<'_>,
    pins: &[PathPin],
) -> Result<(usize, Option<ArtifactPin>), NativeRefusal> {
    let index = pins
        .iter()
        .position(|pin| {
            pin.path
                .as_str()
                .eq_ignore_ascii_case(expected.path.as_str())
                && pin.identity == expected.identity
                && pin.mutable == expected.role.writable()
        })
        .ok_or(NativeRefusal::PathCustodyUnverified)?;
    if expected.role == ArtifactRole::WindowsSetupVars {
        let digest = expected.pin.ok_or(NativeRefusal::PathCustodyUnverified)?;
        return Ok((index, Some(digest)));
    }
    if expected
        .pin
        .is_some_and(|digest| pins[index].verified_content != Some(digest))
    {
        return Err(NativeRefusal::PathCustodyUnverified);
    }
    Ok((index, None))
}

/// The ordinary pin retains the object and ancestors while this additional
/// handle excludes writers during verification and suspended creation.
fn guard_setup_vars(pin: &PathPin, digest: ArtifactPin) -> Result<File, NativeRefusal> {
    let guard = pin
        .reopen_without_writer()
        .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
    if !matches!(digest.verify(&guard, MAX_SETUP_VARS_BYTES), Ok(true)) {
        return Err(NativeRefusal::PathCustodyUnverified);
    }
    Ok(guard)
}

/// No-follow read open of a directory by absolute name. Only volume roots
/// are opened this way.
fn open_directory(path: &str) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | READ_CONTROL)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Node {
    Directory,
    File,
}

pub(crate) fn hold_child_directory(parent: &File, name: &str) -> io::Result<File> {
    open_relative(parent, name, Node::Directory, false, false)
}

pub(crate) fn read_private_child(parent: &File, name: &str, volume: u64) -> io::Result<File> {
    let file = open_relative(parent, name, Node::File, false, false)?;
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
        || file_identity(&file, false)?.0.volume != volume
    {
        return Err(io::Error::other("invalid private file"));
    }
    qualification::require_held_private(&file)?;
    Ok(file)
}

/// Opens one name relative to a held, already-checked directory handle. The
/// parent's current name is never parsed again, a reparse point on the named
/// object is opened rather than followed, and any reparse encountered fails.
fn open_relative(
    parent: &File,
    name: &str,
    node: Node,
    mutable: bool,
    create: bool,
) -> io::Result<File> {
    // Every branch requests data access, so the pin participates in sharing
    // checks: no branch shares delete, so rename and delete of a pinned
    // object are refused. A writable leaf shares write for its consumer.
    let access = if create {
        FILE_GENERIC_WRITE | FILE_GENERIC_READ
    } else {
        FILE_GENERIC_READ | READ_CONTROL
    };
    let share = FILE_SHARE_READ | if mutable { FILE_SHARE_WRITE } else { 0 };
    let kind = match node {
        Node::Directory => FILE_DIRECTORY_FILE,
        Node::File => FILE_NON_DIRECTORY_FILE,
    };
    nt_open_relative(
        parent,
        name,
        access,
        share,
        if create { FILE_CREATE } else { FILE_OPEN },
        kind,
    )
}

/// Exclusive no-follow delete open of one name relative to a held, checked
/// parent directory. Unlike `open_relative` (which pins objects without
/// `FILE_SHARE_DELETE`), this helper requests read and delete access with
/// `share = 0`, excluding any existing or competing reader, writer or deleter
/// through identity verification, durable intent commit and `delete_on_close`.
fn open_relative_delete(parent: &File, name: &str, node: Node) -> io::Result<File> {
    let kind = match node {
        Node::Directory => FILE_DIRECTORY_FILE,
        Node::File => FILE_NON_DIRECTORY_FILE,
    };
    nt_open_relative(
        parent,
        name,
        FILE_GENERIC_READ | READ_CONTROL | DELETE,
        0,
        FILE_OPEN,
        kind,
    )
}

/// Attribute-only, no-follow open of one name of either kind. It requests
/// no data access, so it neither conflicts with nor grants a sharing mode.
/// Callers use it only to classify presence and never to adopt the object.
fn probe_relative(parent: &File, name: &str) -> io::Result<File> {
    nt_open_relative(
        parent,
        name,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        FILE_OPEN,
        0,
    )
}

fn nt_open_relative(
    parent: &File,
    name: &str,
    access: u32,
    share: u32,
    disposition: u32,
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
            disposition,
            options,
            null(),
            0,
        )
    };
    if status < 0 {
        // Translate the NTSTATUS for the typed custody refusal.
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    // NtCreateFile succeeded and transferred a fresh handle.
    Ok(File::from(unsafe { owned(handle) }?))
}

fn verify_ntfs(root: &File) -> io::Result<()> {
    let mut fs = [0u16; 16];
    // The output buffer is sized in UTF-16 units and other outputs are unused.
    succeeded(unsafe {
        GetVolumeInformationByHandleW(
            root.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            fs.as_mut_ptr(),
            fs.len() as u32,
        )
    })?;
    if &fs[..5] != wide("NTFS").as_slice() {
        return Err(io::Error::other("expected NTFS"));
    }
    Ok(())
}

/// Typed outcome for handle-identity queries so custody checks can separate
/// measured file-state violations from OS or I/O failures.
#[derive(Debug)]
enum FileIdentityError {
    /// Measured forbidden attribute, wrong node type, delete-pending, or extra hard link.
    Unsafe,
    /// Win32 API or I/O failure while querying the handle.
    Io(io::Error),
}

impl FileIdentityError {
    fn into_io(self) -> io::Error {
        match self {
            Self::Unsafe => io::Error::other("unsafe file type or alias"),
            Self::Io(error) => error,
        }
    }
}

/// Typed outcome for volume-root and drive-binding checks feeding the
/// selection epoch. Only a positive measured mismatch confirms drift.
#[derive(Debug)]
pub(in crate::vm::windows) enum CustodyCheckError {
    /// Positive measured mismatch against the qualified volume or object identity.
    Mismatch,
    /// Win32 API or I/O failure; fails closed without claiming drift.
    Unavailable(io::Error),
}

impl CustodyCheckError {
    pub(in crate::vm::windows) fn into_invalidation(self, reason: NativeRefusal) -> Invalidation {
        match self {
            Self::Mismatch => Invalidation::Confirmed(reason),
            Self::Unavailable(_) => Invalidation::Indeterminate(reason),
        }
    }

    fn into_io(self) -> io::Error {
        match self {
            Self::Mismatch => io::Error::other("drive letter is not a volume root"),
            Self::Unavailable(error) => error,
        }
    }
}

impl From<FileIdentityError> for CustodyCheckError {
    fn from(error: FileIdentityError) -> Self {
        match error {
            FileIdentityError::Unsafe => Self::Mismatch,
            FileIdentityError::Io(io_error) => Self::Unavailable(io_error),
        }
    }
}

/// A SUBST or other drive-letter alias for a folder is not a volume mount
/// point. The pinned drive-letter root must be the volume's own root object.
pub(crate) fn verify_volume_root(root: &str, pinned: FileIdentity) -> io::Result<()> {
    verify_volume_root_typed(root, pinned).map_err(CustodyCheckError::into_io)
}

pub(in crate::vm::windows) fn verify_volume_root_typed(
    root: &str,
    pinned: FileIdentity,
) -> Result<(), CustodyCheckError> {
    verify_volume_root_with(
        root,
        pinned,
        |mount_point| {
            let mut name = [0u16; 64];
            // The fixed buffer exceeds the 49-unit `\\?\Volume{GUID}\` name.
            succeeded(unsafe {
                GetVolumeNameForVolumeMountPointW(
                    wide(mount_point).as_ptr(),
                    name.as_mut_ptr(),
                    name.len() as u32,
                )
            })?;
            let end = name
                .iter()
                .position(|&unit| unit == 0)
                .ok_or_else(|| io::Error::other("unterminated volume name"))?;
            let volume = String::from_utf16(&name[..end]).map_err(io::Error::other)?;
            if !volume.starts_with(r"\\?\Volume{") || !volume.ends_with(r"}\") {
                return Err(io::Error::other("unexpected volume name"));
            }
            Ok(volume)
        },
        |volume| {
            let dir = open_directory(volume).map_err(FileIdentityError::Io)?;
            Ok(file_identity_typed(&dir, true)?.0)
        },
    )
}

fn verify_volume_root_with(
    root: &str,
    pinned: FileIdentity,
    resolve: impl FnOnce(&str) -> io::Result<String>,
    read_root: impl FnOnce(&str) -> Result<FileIdentity, FileIdentityError>,
) -> Result<(), CustodyCheckError> {
    let volume = resolve(root).map_err(CustodyCheckError::Unavailable)?;
    let actual = read_root(&volume).map_err(CustodyCheckError::from)?;
    if actual != pinned {
        return Err(CustodyCheckError::Mismatch);
    }
    Ok(())
}

fn file_identity(file: &File, directory: bool) -> io::Result<(FileIdentity, u64)> {
    file_identity_typed(file, directory).map_err(FileIdentityError::into_io)
}

fn file_identity_typed(
    file: &File,
    directory: bool,
) -> Result<(FileIdentity, u64), FileIdentityError> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    let mut id = FILE_ID_INFO::default();
    // Each request writes the matching fixed-size output structure.
    unsafe {
        for (class, buffer, size) in [
            (FileAttributeTagInfo, ptr_of(&mut tag), size_of_val(&tag)),
            (
                FileStandardInfo,
                ptr_of(&mut standard),
                size_of_val(&standard),
            ),
            (FileIdInfo, ptr_of(&mut id), size_of_val(&id)),
        ] {
            succeeded(GetFileInformationByHandleEx(
                file.as_raw_handle(),
                class,
                buffer,
                size as u32,
            ))
            .map_err(FileIdentityError::Io)?;
        }
    }
    if tag.FileAttributes
        & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_SPARSE_FILE | FILE_ATTRIBUTE_COMPRESSED)
        != 0
        || standard.Directory != directory
        || standard.DeletePending
        || (!directory && standard.NumberOfLinks != 1)
    {
        return Err(FileIdentityError::Unsafe);
    }
    let allocated = u64::try_from(standard.AllocationSize)
        .map_err(|error| FileIdentityError::Io(io::Error::other(error)))?;
    Ok((
        FileIdentity {
            volume: id.VolumeSerialNumber,
            file: id.FileId.Identifier,
        },
        allocated,
    ))
}

fn ptr_of<T>(value: &mut T) -> *mut core::ffi::c_void {
    std::ptr::from_mut(value).cast()
}

/// Owns a proc-thread attribute list and every value it references. Values
/// are copied into boxed slices whose heap addresses never move. `Drop`
/// destroys the list in its body, and Rust drops the fields, including the
/// values, only afterwards. Every exit path, including creation errors,
/// therefore keeps each value live through `DeleteProcThreadAttributeList`.
struct Attributes {
    storage: Vec<usize>,
    values: Vec<Box<[usize]>>,
}

impl Attributes {
    fn new(count: u32) -> io::Result<Self> {
        let mut bytes = 0;
        // The first call obtains the required size.
        unsafe { InitializeProcThreadAttributeList(null_mut(), count, 0, &mut bytes) };
        if bytes == 0 || bytes > 65_536 {
            return Err(io::Error::other("invalid attribute list size"));
        }
        let mut storage = vec![0; bytes.div_ceil(size_of::<usize>())];
        // The heap allocation is aligned and lives until DeleteProcThreadAttributeList.
        succeeded(unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), count, 0, &mut bytes)
        })?;
        Ok(Self {
            storage,
            values: Vec::with_capacity(count as usize),
        })
    }

    fn insert_paused_policy(&mut self) -> io::Result<()> {
        // Enforce no remote/Low-label images and prefer System32 from creation.
        let image_policy = (1u64 << 52) | (1u64 << 56) | (1u64 << 60);
        self.insert_words(
            PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY as usize,
            &[image_policy as usize],
            size_of::<u64>(),
        )?;
        // PROCESS_CREATION_CHILD_PROCESS_RESTRICTED, a DWORD policy.
        self.insert_words(
            PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY as usize,
            &[1],
            size_of::<u32>(),
        )
    }

    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }

    fn insert(&mut self, attribute: usize, handles: &[HANDLE]) -> io::Result<()> {
        let words: Vec<_> = handles.iter().map(|handle| *handle as usize).collect();
        self.insert_words(attribute, &words, size_of_val(handles))
    }

    fn insert_words(&mut self, attribute: usize, words: &[usize], size: usize) -> io::Result<()> {
        // Retain the copy before the list can reference it, so even a failed
        // update leaves no dangling value.
        self.values.push(words.into());
        let value = self.values.last().expect("value retained above");
        let address = value.as_ptr();
        // The boxed value lives in this guard until after list destruction.
        succeeded(unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                attribute,
                address.cast(),
                size,
                null_mut(),
                null_mut(),
            )
        })
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        // Created by InitializeProcThreadAttributeList in this allocation.
        // `self.values` is dropped only after this body returns.
        unsafe { DeleteProcThreadAttributeList(self.pointer()) };
    }
}

struct Job(OwnedHandle);

impl Job {
    fn new(memory_limit: usize, cpu_rate: Option<u32>) -> io::Result<Self> {
        Self::with_process_limit(memory_limit, cpu_rate, None)
    }

    fn with_process_limit(
        memory_limit: usize,
        cpu_rate: Option<u32>,
        processes: Option<u32>,
    ) -> io::Result<Self> {
        if memory_limit == 0 || cpu_rate.is_some_and(|rate| !(1..=5000).contains(&rate)) {
            return Err(io::Error::other("invalid Job limits"));
        }
        // An unnamed Job with default non-inheritable handle attributes.
        let job = Self(unsafe { owned(CreateJobObjectW(null(), null())) }?);
        let extended = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_JOB_MEMORY
                    | if processes.is_some() {
                        JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                    } else {
                        0
                    },
                ActiveProcessLimit: processes.unwrap_or(0),
                ..Default::default()
            },
            JobMemoryLimit: memory_limit,
            ..Default::default()
        };
        job.set(JobObjectExtendedLimitInformation, &extended)?;
        if let Some(rate) = cpu_rate {
            let cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
                ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 { CpuRate: rate },
            };
            job.set(JobObjectCpuRateControlInformation, &cpu)?;
        }
        job.verify_limits(memory_limit, cpu_rate, processes)?;
        Ok(job)
    }

    fn verify_empty_noninheritable(&self) -> io::Result<()> {
        let mut flags = 0;
        succeeded(unsafe { GetHandleInformation(self.0.as_raw_handle(), &mut flags) })?;
        if flags & HANDLE_FLAG_INHERIT != 0 || self.active()? != 0 {
            return Err(io::Error::other("Job is inheritable or occupied"));
        }
        Ok(())
    }

    fn verify_limits(
        &self,
        memory_limit: usize,
        cpu_rate: Option<u32>,
        processes: Option<u32>,
    ) -> io::Result<()> {
        let expected_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_JOB_MEMORY
            | if processes.is_some() {
                JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            } else {
                0
            };
        if let Some(rate) = cpu_rate {
            let expected_cpu_flags =
                JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
            let mut actual = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
            self.query(JobObjectCpuRateControlInformation, &mut actual)?;
            // Hard-cap flags select the CpuRate union member.
            if actual.ControlFlags != expected_cpu_flags
                || unsafe { actual.Anonymous.CpuRate } != rate
            {
                return Err(io::Error::other("Job CPU cap unavailable"));
            }
        }
        let mut actual = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        self.query(JobObjectExtendedLimitInformation, &mut actual)?;
        if actual.BasicLimitInformation.LimitFlags != expected_flags
            || actual.JobMemoryLimit != memory_limit
            || processes
                .is_some_and(|limit| actual.BasicLimitInformation.ActiveProcessLimit != limit)
        {
            return Err(io::Error::other("Job containment flags unavailable"));
        }
        Ok(())
    }

    fn set<T>(&self, class: JOBOBJECTINFOCLASS, info: &T) -> io::Result<()> {
        // Callers supply the structure belonging to the information class.
        succeeded(unsafe {
            SetInformationJobObject(
                self.0.as_raw_handle(),
                class,
                std::ptr::from_ref(info).cast(),
                size_of::<T>() as u32,
            )
        })
    }

    fn query<T>(&self, class: JOBOBJECTINFOCLASS, info: &mut T) -> io::Result<()> {
        // Callers supply the structure belonging to the information class.
        succeeded(unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                class,
                ptr_of(info),
                size_of::<T>() as u32,
                null_mut(),
            )
        })
    }

    fn active(&self) -> io::Result<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        self.query(JobObjectBasicAccountingInformation, &mut accounting)?;
        Ok(accounting.ActiveProcesses)
    }

    fn terminate(&self) -> io::Result<()> {
        // This is our unnamed Job, never a process opened by PID.
        succeeded(unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) })
    }
}

struct Stdio {
    input: OwnedHandle,
    output: OwnedHandle,
    error: OwnedHandle,
}

impl Stdio {
    fn private(attributes: &SECURITY_ATTRIBUTES) -> io::Result<(Self, Self)> {
        let (input, parent_input) = anonymous_pipe(attributes, false)?;
        let (parent_output, output) = anonymous_pipe(attributes, true)?;
        let (parent_error, error) = anonymous_pipe(attributes, true)?;
        Ok((
            Self {
                input,
                output,
                error,
            },
            Self {
                input: parent_input,
                output: parent_output,
                error: parent_error,
            },
        ))
    }

    fn handles(&self) -> [HANDLE; 3] {
        [
            self.input.as_raw_handle(),
            self.output.as_raw_handle(),
            self.error.as_raw_handle(),
        ]
    }
}

fn anonymous_pipe(
    attributes: &SECURITY_ATTRIBUTES,
    parent_reads: bool,
) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read = null_mut();
    let mut write = null_mut();
    // The caller supplies an explicit descriptor for both pipe endpoints.
    succeeded(unsafe { CreatePipe(&mut read, &mut write, attributes, 4096) })?;
    // CreatePipe successfully transferred both handles.
    let (read, write) = unsafe { (owned(read)?, owned(write)?) };
    let child = if parent_reads { &write } else { &read };
    // The parent ends keep their original non-inheritable flags.
    succeeded(unsafe {
        SetHandleInformation(
            child.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    })?;
    Ok((read, write))
}

struct Child {
    job: Job,
    process: OwnedHandle,
    suspended: Option<OwnedHandle>,
    _desktop: Option<desktop::LaunchDesktop>,
}

impl Child {
    fn verify_live(&self) -> io::Result<u32> {
        let mut member = 0;
        // Both handles are held from creation and cannot alias a reused PID.
        unsafe {
            succeeded(IsProcessInJob(
                self.process.as_raw_handle(),
                self.job.0.as_raw_handle(),
                &mut member,
            ))?;
            let pid = GetProcessId(self.process.as_raw_handle());
            if member == 0
                || pid == 0
                || WaitForSingleObject(self.process.as_raw_handle(), 0) != WAIT_TIMEOUT
            {
                return Err(io::Error::other("owned child is not live in its Job"));
            }
            Ok(pid)
        }
    }

    fn resume(&mut self) -> io::Result<()> {
        self.verify_live()?;
        self.release_suspended()
    }

    /// Releases the retained primary thread without any further checks; the
    /// caller verifies the live Job immediately before.
    fn release_suspended(&mut self) -> io::Result<()> {
        let thread = self
            .suspended
            .take()
            .ok_or_else(|| io::Error::other("child already resumed"))?;
        // Only the retained primary thread may be released.
        if unsafe { ResumeThread(thread.as_raw_handle()) } != 1 {
            return Err(io::Error::other("unexpected child suspend count"));
        }
        Ok(())
    }

    fn exited(&self) -> io::Result<bool> {
        wait(&self.process, 0)
    }

    async fn reap(&self, deadline: tokio::time::Instant) -> io::Result<()> {
        loop {
            if self.exited()? && self.job.active()? == 0 {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            tokio::time::sleep_until(
                deadline.min(tokio::time::Instant::now() + Duration::from_millis(10)),
            )
            .await;
        }
    }
}

fn wait(handle: &impl AsRawHandle, millis: u32) -> io::Result<bool> {
    // The handle is retained throughout the wait.
    match unsafe { WaitForSingleObject(handle.as_raw_handle(), millis) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

/// The only argv sources. Production launches take a sealed `CommandPlan`
/// under the restricted token; tests may re-execute their own binary.
enum Launch<'a> {
    Restricted {
        token: &'a VmToken,
        plan: &'a CommandPlan<'a>,
    },
    Paused {
        token: &'a VmToken,
        image: &'a LocalPath,
        args: &'a [String],
    },
    #[cfg(test)]
    Fixture { image: &'a Path, args: &'a [String] },
    #[cfg(test)]
    ContainedFixture {
        image: &'a Path,
        args: &'a [String],
        legacy_console: bool,
        token: Option<&'a VmToken>,
        interactive: bool,
    },
}

/// Creation-time Job assignment. Every production launch names one Job.
enum Assignment<'a> {
    Job(&'a Job),
    /// Test-only control that isolates the Job-list attribute when an outer
    /// runner Job may refuse nesting. It never resumes the created process.
    #[cfg(test)]
    ControlWithoutJob,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpawnStage {
    Recipe,
    Attributes,
    Create,
    Verify,
}

#[derive(Debug)]
struct SpawnError {
    stage: SpawnStage,
    error: io::Error,
    partial: Option<PartialChild>,
}

struct PartialChild {
    _process: Option<OwnedHandle>,
    _thread: Option<OwnedHandle>,
    _desktop: Option<desktop::LaunchDesktop>,
}

impl std::fmt::Debug for PartialChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartialChild").finish_non_exhaustive()
    }
}

fn at(stage: SpawnStage) -> impl FnOnce(io::Error) -> SpawnError {
    move |error| SpawnError {
        stage,
        error,
        partial: None,
    }
}

fn spawn(
    launch: Launch<'_>,
    cwd: &Path,
    environment: &[u16],
    job: Job,
    stdio: Stdio,
) -> Result<Child, (SpawnError, Job)> {
    match create_suspended(&launch, cwd, environment, Assignment::Job(&job), &stdio) {
        Ok((process, thread, desktop)) => Ok(Child {
            job,
            process,
            suspended: Some(thread),
            _desktop: desktop,
        }),
        Err(error) => Err((error, job)),
    }
}

fn create_suspended(
    launch: &Launch<'_>,
    cwd: &Path,
    environment: &[u16],
    assignment: Assignment<'_>,
    stdio: &Stdio,
) -> Result<(OwnedHandle, OwnedHandle, Option<desktop::LaunchDesktop>), SpawnError> {
    let (image, args): (&Path, &[String]) = match launch {
        Launch::Restricted { plan, .. } => (Path::new(plan.image().as_str()), plan.args()),
        Launch::Paused { image, args, .. } => (Path::new(image.as_str()), args),
        #[cfg(test)]
        Launch::Fixture { image, args } | Launch::ContainedFixture { image, args, .. } => {
            (image, args)
        }
    };
    let recipe = |message| at(SpawnStage::Recipe)(io::Error::other(message));
    if !image.is_absolute() || !cwd.is_absolute() || !environment.ends_with(&[0, 0]) {
        return Err(recipe("invalid launch recipe"));
    }
    let image = wide(image);
    let cwd = wide(cwd);
    if image[..image.len() - 1].contains(&0) || cwd[..cwd.len() - 1].contains(&0) {
        return Err(recipe("invalid launch path"));
    }
    let mut command = Vec::new();
    quote_arg(&mut command, &image[..image.len() - 1]).map_err(at(SpawnStage::Recipe))?;
    for arg in args {
        command.push(b' ' as u16);
        quote_arg(&mut command, &arg.encode_utf16().collect::<Vec<_>>())
            .map_err(at(SpawnStage::Recipe))?;
    }
    command.push(0);
    if command.len() > 32_767 {
        return Err(recipe("command line too long"));
    }
    let handles = stdio.handles();
    let contained = matches!(launch, Launch::Paused { .. } | Launch::Restricted { .. });
    #[cfg(test)]
    let contained = contained || matches!(launch, Launch::ContainedFixture { .. });
    let mut attributes =
        Attributes::new(if contained { 4 } else { 2 }).map_err(at(SpawnStage::Attributes))?;
    if contained {
        attributes
            .insert_paused_policy()
            .map_err(at(SpawnStage::Attributes))?;
    }
    for (index, handle) in handles.iter().copied().enumerate() {
        let mut flags = 0;
        // Verify the exact distinct child ends and their inheritance flags.
        succeeded(unsafe { GetHandleInformation(handle, &mut flags) })
            .map_err(at(SpawnStage::Attributes))?;
        if handles[..index].contains(&handle) || flags & HANDLE_FLAG_INHERIT == 0 {
            return Err(at(SpawnStage::Attributes)(io::Error::other(
                "invalid inherited handle list",
            )));
        }
    }
    // The guard copies and owns each value until after list destruction.
    match assignment {
        Assignment::Job(job) => attributes
            .insert(
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                &[job.0.as_raw_handle()],
            )
            .map_err(at(SpawnStage::Attributes))?,
        #[cfg(test)]
        Assignment::ControlWithoutJob => {}
    }
    attributes
        .insert(PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize, &handles)
        .map_err(at(SpawnStage::Attributes))?;
    let desktop = match launch {
        Launch::Paused { token, .. } | Launch::Restricted { token, .. } => {
            Some(desktop::LaunchDesktop::new(token).map_err(at(SpawnStage::Attributes))?)
        }
        // Negative controls vary one launch property at a time: the legacy
        // console probe keeps the production private desktop.
        #[cfg(test)]
        Launch::ContainedFixture {
            token: Some(token),
            interactive: false,
            ..
        } => Some(desktop::LaunchDesktop::new(token).map_err(at(SpawnStage::Attributes))?),
        #[cfg(test)]
        Launch::Fixture { .. } | Launch::ContainedFixture { .. } => None,
    };
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    if let Some(desktop) = &desktop {
        startup.StartupInfo.lpDesktop = desktop.path.as_ptr().cast_mut();
    }
    [
        startup.StartupInfo.hStdInput,
        startup.StartupInfo.hStdOutput,
        startup.StartupInfo.hStdError,
    ] = handles;
    #[cfg(test)]
    let interactive_desktop = wide(r"WinSta0\Default");
    #[cfg(test)]
    if matches!(
        launch,
        Launch::ContainedFixture {
            interactive: true,
            ..
        }
    ) {
        startup.StartupInfo.lpDesktop = interactive_desktop.as_ptr().cast_mut();
    }
    startup.lpAttributeList = attributes.pointer();
    let mut process = PROCESS_INFORMATION::default();
    // CREATE_NO_WINDOW can fail console initialization when the child
    // process policy is enforced. Start VM processes detached from consoles.
    let console = if contained {
        DETACHED_PROCESS
    } else {
        CREATE_NO_WINDOW
    };
    #[cfg(test)]
    let console = if matches!(
        launch,
        Launch::ContainedFixture {
            legacy_console: true,
            ..
        }
    ) {
        CREATE_NO_WINDOW
    } else {
        console
    };
    let flags =
        EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | console;
    // All arrays, command buffer, and attribute storage remain live for this call.
    let command = command.as_mut_ptr();
    let information = std::ptr::from_mut(&mut process);
    let create_as_user = |token: &VmToken| unsafe {
        CreateProcessAsUserW(
            token.token.as_raw_handle(),
            image.as_ptr(),
            command,
            null(),
            null(),
            1,
            flags,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            information,
        )
    };
    succeeded(match launch {
        Launch::Restricted { token, .. } | Launch::Paused { token, .. } => create_as_user(token),
        #[cfg(test)]
        Launch::ContainedFixture {
            token: Some(token), ..
        } => create_as_user(token),
        #[cfg(test)]
        Launch::Fixture { .. } | Launch::ContainedFixture { .. } => unsafe {
            CreateProcessW(
                image.as_ptr(),
                command,
                null(),
                null(),
                1,
                flags,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                information,
            )
        },
    })
    .map_err(at(SpawnStage::Create))?;
    drop(attributes);
    // The kernel has created both handles and assigned the Job before any child instruction.
    let (process, thread) = unsafe { (owned(process.hProcess), owned(process.hThread)) };
    match (process, thread) {
        (Ok(process), Ok(thread)) => Ok((process, thread, desktop)),
        (process, thread) => Err(SpawnError {
            stage: SpawnStage::Verify,
            error: io::Error::other("process created without both handles"),
            partial: Some(PartialChild {
                _process: process.ok(),
                _thread: thread.ok(),
                _desktop: desktop,
            }),
        }),
    }
}

fn quote_arg(out: &mut Vec<u16>, arg: &[u16]) -> io::Result<()> {
    if arg.contains(&0) {
        return Err(io::Error::other("embedded NUL in argument"));
    }
    let (quote, slash) = (u16::from(b'"'), u16::from(b'\\'));
    out.push(quote);
    let mut slashes = 0;
    for &unit in arg {
        if unit == slash {
            slashes += 1;
        } else {
            out.extend(std::iter::repeat_n(
                slash,
                if unit == quote {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            out.push(unit);
            slashes = 0;
        }
    }
    out.extend(std::iter::repeat_n(slash, slashes * 2));
    out.push(quote);
    Ok(())
}

/// Begin this connection alongside stdio drain, before awaiting any QMP record.
async fn connect_serial(
    name: &SerialName,
    child: &Child,
    owner: PSID,
    acl: &[vm_acl::Entry],
    cancel: &Shutdown,
    deadline: tokio::time::Instant,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, NativeRefusal> {
    let reason = NativeRefusal::GuestEndpointUnverified;
    let path = wide(format!(r"\\.\pipe\{}", name.basename()));
    loop {
        if cancel.is_cancelled() || tokio::time::Instant::now() >= deadline {
            return Err(reason);
        }
        child.verify_live().map_err(|_| reason)?;
        // The client can exchange data and inspect the descriptor, but has no
        // FILE_APPEND_DATA/FILE_CREATE_PIPE_INSTANCE or impersonation rights.
        let raw = unsafe {
            CreateFileW(
                path.as_ptr(),
                FILE_READ_DATA
                    | FILE_WRITE_DATA
                    | FILE_READ_ATTRIBUTES
                    | READ_CONTROL
                    | SYNCHRONIZE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
                null_mut(),
            )
        };
        // The fresh handle, if valid, is owned here until verified and registered.
        match unsafe { owned(raw) } {
            Ok(handle) => {
                prove_serial_peer(&handle, child, owner, acl)?;
                // CreateFileW provided a unique overlapped handle; Tokio takes ownership.
                return unsafe {
                    tokio::net::windows::named_pipe::NamedPipeClient::from_raw_handle(
                        handle.into_raw_handle(),
                    )
                }
                .map_err(|_| reason);
            }
            Err(error) if matches!(error.raw_os_error(), Some(2 | 231)) => {}
            Err(_) => return Err(reason),
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err(reason),
            _ = tokio::time::sleep_until(deadline.min(tokio::time::Instant::now() + Duration::from_millis(10))) => {}
        }
    }
}

fn prove_serial_peer(
    handle: &impl AsRawHandle,
    child: &Child,
    owner: PSID,
    acl: &[vm_acl::Entry],
) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::GuestEndpointUnverified;
    let expected = child.verify_live().map_err(|_| reason)?;
    let mut pid = 0;
    // The descriptor and server identity are both queried on this exact handle.
    let verified = unsafe {
        GetNamedPipeServerProcessId(handle.as_raw_handle(), &mut pid) != 0
            && pid == expected
            && vm_acl::exact(handle, owner, acl)
    };
    if !verified || child.verify_live().map_err(|_| reason)? != expected {
        return Err(reason);
    }
    Ok(())
}

fn host_resources() -> Result<(u32, u64), NativeRefusal> {
    measured_host_resources().map_err(|_| NativeRefusal::ReservesUnverified)
}

fn measured_host_resources() -> Result<(u32, u64), crate::vm::qualification_record::ReserveCheck> {
    let mut memory = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // Processor count spans groups; effective Job scope still needs qualification.
    let cpus = unsafe { GetActiveProcessorCount(u16::MAX) };
    check_host_resources(cpus, || {
        (unsafe { GlobalMemoryStatusEx(&mut memory) } != 0).then_some(memory.ullAvailPhys)
    })
}

fn check_host_resources(
    cpus: u32,
    memory: impl FnOnce() -> Option<u64>,
) -> Result<(u32, u64), crate::vm::qualification_record::ReserveCheck> {
    use crate::vm::qualification_record::ReserveCheck;

    if cpus == 0 {
        return Err(ReserveCheck::ActiveCpusUnavailable);
    }
    let available_bytes = memory().ok_or(ReserveCheck::MemoryStatusUnavailable)?;
    // The source gate also refuses a successful zero before checking the
    // launch envelope. Preserve that order under its own source label.
    if available_bytes == 0 {
        return Err(ReserveCheck::AvailableRamSourceZero);
    }
    Ok((cpus, available_bytes))
}

fn disk_free(root: &LocalPath) -> Result<(u64, u64), NativeRefusal> {
    let pin = PathPin::open(root, true, false)?;
    let (mut available, mut total, mut free) = (0, 0, 0);
    // The root remains pinned while the name-based API gathers its two free-space observations.
    let result = unsafe {
        GetDiskFreeSpaceExW(
            wide(root.as_str()).as_ptr(),
            &mut available,
            &mut total,
            &mut free,
        )
    };
    drop(pin);
    if result == 0 || free > total || available > total {
        return Err(NativeRefusal::DiskCeilingUnverified);
    }
    Ok((available.min(free), total))
}

struct LocalSid(PSID);

// A LocalAlloc SID has no thread affinity. This owner exposes the retained
// address only to read-only SID operations and frees it on exclusive drop.
unsafe impl Send for LocalSid {}
unsafe impl Sync for LocalSid {}

impl LocalSid {
    fn parse(text: &str) -> io::Result<Self> {
        let mut sid = null_mut();
        // The returned SID is owned by the caller and uses LocalFree.
        succeeded(unsafe { ConvertStringSidToSidW(wide(text).as_ptr(), &mut sid) })?;
        Ok(Self(sid))
    }
}

impl Drop for LocalSid {
    fn drop(&mut self) {
        // ConvertStringSidToSidW allocated this SID.
        unsafe { LocalFree(self.0) };
    }
}

fn token_data(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
    let mut size = 0;
    // The first query supplies the needed allocation size.
    unsafe { GetTokenInformation(token.as_raw_handle(), class, null_mut(), 0, &mut size) };
    if size == 0 || size > 65_536 {
        return Err(io::Error::other("unavailable token information"));
    }
    let mut result = vec![0; (size as usize).div_ceil(size_of::<usize>())];
    // The aligned allocation remains alive while its interior SID views are used.
    succeeded(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            result.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    })?;
    Ok(result)
}

fn process_token(process: HANDLE, access: TOKEN_ACCESS_MASK) -> io::Result<OwnedHandle> {
    let mut token = null_mut();
    // Request only the access needed at the current trust boundary.
    succeeded(unsafe { OpenProcessToken(process, access, &mut token) })?;
    // OpenProcessToken transferred a new handle.
    unsafe { owned(token) }
}

struct Identity {
    logon: Vec<usize>,
    authentication: LUID,
    session: u32,
}

impl Identity {
    fn logon_sid(&self) -> PSID {
        // Constructed only after verifying TokenLogonSid returned one group.
        unsafe { (*self.logon.as_ptr().cast::<TOKEN_GROUPS>()).Groups[0].Sid }
    }

    fn same_logon(&self, other: &Self) -> bool {
        // Both SIDs remain backed by the token query allocations.
        let same_sid = unsafe { EqualSid(self.logon_sid(), other.logon_sid()) != 0 };
        same_sid
            && self.session == other.session
            && self.authentication.LowPart == other.authentication.LowPart
            && self.authentication.HighPart == other.authentication.HighPart
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TokenMode {
    Dedicated,
    CurrentUser,
}

impl TokenMode {
    fn accepts_elevation(self, kind: TOKEN_ELEVATION_TYPE) -> bool {
        kind == TokenElevationTypeDefault
            || (self == Self::CurrentUser && kind == TokenElevationTypeLimited)
    }

    fn accepts_groups(self, entries: &[SID_AND_ATTRIBUTES]) -> bool {
        use windows_sys::Win32::System::SystemServices::*;
        let allowed = (SE_GROUP_MANDATORY
            | SE_GROUP_ENABLED_BY_DEFAULT
            | SE_GROUP_ENABLED
            | SE_GROUP_OWNER
            | SE_GROUP_USE_FOR_DENY_ONLY
            | SE_GROUP_INTEGRITY
            | SE_GROUP_INTEGRITY_ENABLED
            | SE_GROUP_LOGON_ID) as u32;
        entries.iter().all(|entry| {
            // These group SIDs come from the bounded token information query.
            let (admin, hyperv, system) = unsafe {
                (
                    IsWellKnownSid(entry.Sid, WinBuiltinAdministratorsSid) != 0,
                    IsWellKnownSid(entry.Sid, WinBuiltinHyperVAdminsSid) != 0,
                    IsWellKnownSid(entry.Sid, WinLocalSystemSid) != 0,
                )
            };
            !system
                && (self == Self::Dedicated || entry.Attributes & !allowed == 0)
                && (!(admin || hyperv)
                    || (self == Self::CurrentUser
                        && entry.Attributes & SE_GROUP_USE_FOR_DENY_ONLY as u32 != 0
                        && entry.Attributes
                            & (SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT) as u32
                            == 0))
        })
    }
}

/// The documented TokenGroups layout, bounded by its backing buffer.
fn token_entries(groups: &[usize]) -> io::Result<&[SID_AND_ATTRIBUTES]> {
    let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
    if size_of_val(groups) < offset {
        return Err(io::Error::other("invalid token group buffer"));
    }
    // The buffer starts with a TOKEN_GROUPS header and the count is checked
    // against the remaining bytes before the slice is formed.
    unsafe {
        let token_groups = groups.as_ptr().cast::<TOKEN_GROUPS>();
        let count = (*token_groups).GroupCount as usize;
        if count > (size_of_val(groups) - offset) / size_of::<SID_AND_ATTRIBUTES>() {
            return Err(io::Error::other("invalid token group count"));
        }
        Ok(std::slice::from_raw_parts(
            std::ptr::addr_of!((*token_groups).Groups).cast::<SID_AND_ATTRIBUTES>(),
            count,
        ))
    }
}

/// An owned copy of every group SID and its exact attribute mask.
fn captured_groups(entries: &[SID_AND_ATTRIBUTES]) -> io::Result<Vec<(Vec<u8>, u32)>> {
    entries
        .iter()
        .map(|entry| {
            // Each SID is checked for validity before its length is trusted.
            unsafe {
                if IsValidSid(entry.Sid) == 0 {
                    return Err(io::Error::other("invalid token group SID"));
                }
                let len = GetLengthSid(entry.Sid) as usize;
                let bytes = std::slice::from_raw_parts(entry.Sid.cast::<u8>(), len).to_vec();
                Ok((bytes, entry.Attributes))
            }
        })
        .collect()
}

/// The restricted child must carry exactly the captured parent groups and
/// masks: same SIDs, same order, same attributes.
fn verify_groups_typed(
    expected: &[(Vec<u8>, u32)],
    entries: &[SID_AND_ATTRIBUTES],
) -> Result<(), TokenCheckError> {
    let actual = captured_groups(entries).map_err(TokenCheckError::Io)?;
    if actual != expected {
        return Err(TokenCheckError::Mismatch(
            "VM child token groups differ from the policy",
        ));
    }
    Ok(())
}

/// Typed outcome for execution-profile and token checks feeding the
/// selection epoch. Permanent policy/profile mismatches confirm drift;
/// Win32/LSA failures and an inactive console session stay indeterminate.
#[derive(Debug)]
enum TokenCheckError {
    /// Positive measured mismatch in token identity, profile, groups,
    /// privileges, restrictions, logon, impersonation or child default DACL.
    Mismatch(&'static str),
    /// The owner's console session is not currently active (e.g. session switch
    /// or disconnect). Fails closed without claiming permanent profile drift.
    ConsoleInactive,
    /// Win32 or LSA API/I/O failure; no drift is claimed.
    Io(io::Error),
}

impl TokenCheckError {
    fn into_invalidation(self) -> Invalidation {
        let reason = NativeRefusal::IdentityUnverified;
        match self {
            Self::Mismatch(_) => Invalidation::Confirmed(reason),
            Self::ConsoleInactive | Self::Io(_) => Invalidation::Indeterminate(reason),
        }
    }

    fn into_io(self) -> io::Error {
        match self {
            Self::Mismatch(message) => io::Error::other(message),
            Self::ConsoleInactive => io::Error::other("unqualified VM token"),
            Self::Io(error) => error,
        }
    }
}

fn require_unimpersonated() -> io::Result<()> {
    require_unimpersonated_typed().map_err(TokenCheckError::into_io)
}

fn require_unimpersonated_typed() -> Result<(), TokenCheckError> {
    let mut impersonation = null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut impersonation) } != 0 {
        drop(unsafe { owned(impersonation) }.map_err(TokenCheckError::Io)?);
        return Err(TokenCheckError::Mismatch(
            "impersonation is outside the VM token policy",
        ));
    }
    if unsafe { GetLastError() } != ERROR_NO_TOKEN {
        return Err(TokenCheckError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

fn token_identity(
    token: &OwnedHandle,
    account: &LocalSid,
    restricted: bool,
    mode: TokenMode,
) -> io::Result<Identity> {
    inspect_token_identity(token, account, restricted, mode, true)
}

/// The read-only console witness can query the anchored owner token while
/// another session is active. Every effect gate uses token_identity above.
fn inspect_token_identity(
    token: &OwnedHandle,
    account: &LocalSid,
    restricted: bool,
    mode: TokenMode,
    require_active_console: bool,
) -> io::Result<Identity> {
    inspect_token_identity_typed(token, account, restricted, mode, require_active_console)
        .map_err(TokenCheckError::into_io)
}

fn inspect_token_identity_typed(
    token: &OwnedHandle,
    account: &LocalSid,
    restricted: bool,
    mode: TokenMode,
    require_active_console: bool,
) -> Result<Identity, TokenCheckError> {
    let user = token_data(token, TokenUser).map_err(TokenCheckError::Io)?;
    let integrity = token_data(token, TokenIntegrityLevel).map_err(TokenCheckError::Io)?;
    let elevation = token_data(token, TokenElevation).map_err(TokenCheckError::Io)?;
    let elevation_kind = token_data(token, TokenElevationType).map_err(TokenCheckError::Io)?;
    let session = token_data(token, TokenSessionId).map_err(TokenCheckError::Io)?[0] as u32;
    let stats = token_data(token, TokenStatistics).map_err(TokenCheckError::Io)?;
    let groups = token_data(token, TokenGroups).map_err(TokenCheckError::Io)?;
    let logon = token_data(token, TokenLogonSid).map_err(TokenCheckError::Io)?;
    if restricted && mode == TokenMode::CurrentUser {
        let restrictions = token_data(token, TokenRestrictedSids).map_err(TokenCheckError::Io)?;
        // The query owns the pointed-to SID. Require only the enrolled user
        // in the write restriction list, as supplied to CreateRestrictedToken.
        let exact = unsafe {
            let groups = &*restrictions.as_ptr().cast::<TOKEN_GROUPS>();
            groups.GroupCount == 1 && EqualSid(groups.Groups[0].Sid, account.0) != 0
        };
        if !exact {
            return Err(TokenCheckError::Mismatch(
                "unexpected VM token restrictions",
            ));
        }
    }
    // These are the documented layouts of the respective token information
    // classes, and each kernel-returned SID stays within its backing buffer.
    unsafe {
        let actual_user = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let integrity = (*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())
            .Label
            .Sid;
        let stats = &*stats.as_ptr().cast::<TOKEN_STATISTICS>();
        let entries = token_entries(&groups).map_err(TokenCheckError::Io)?;
        if IsWellKnownSid(actual_user, WinLocalSystemSid) != 0
            || EqualSid(actual_user, account.0) == 0
            || IsWellKnownSid(integrity, WinMediumLabelSid) == 0
            || elevation[0] != 0
            || !mode.accepts_elevation(elevation_kind[0] as i32)
            || session == 0
            || stats.TokenType != TokenPrimary
            || (IsTokenRestricted(token.as_raw_handle()) != 0) != restricted
            || (*logon.as_ptr().cast::<TOKEN_GROUPS>()).GroupCount != 1
            || !mode.accepts_groups(entries)
        {
            return Err(TokenCheckError::Mismatch("unqualified VM token"));
        }
        let mut data = null_mut();
        if LsaGetLogonSessionData(&stats.AuthenticationId, &mut data) != 0 || data.is_null() {
            return Err(TokenCheckError::Io(io::Error::other(
                "unavailable logon session",
            )));
        }
        let local = !(*data).Sid.is_null()
            && EqualSid((*data).Sid, actual_user) != 0
            && (*data).Session == session
            && (*data).LogonType == 2; // SECURITY_LOGON_TYPE::Interactive
        LsaFreeReturnBuffer(data.cast());
        if !local {
            return Err(TokenCheckError::Mismatch("unqualified VM logon"));
        }
        verify_privileges_typed(token, restricted && mode == TokenMode::CurrentUser)?;
        if require_active_console
            && mode == TokenMode::CurrentUser
            && WTSGetActiveConsoleSessionId() != session
        {
            return Err(TokenCheckError::ConsoleInactive);
        }
        Ok(Identity {
            logon,
            authentication: stats.AuthenticationId,
            session,
        })
    }
}

fn verify_privileges_typed(
    token: &OwnedHandle,
    current_user_child: bool,
) -> Result<(), TokenCheckError> {
    let data = token_data(token, TokenPrivileges).map_err(TokenCheckError::Io)?;
    let privileges = data.as_ptr().cast::<TOKEN_PRIVILEGES>();
    // The kernel returned a variable-length array with this layout.
    let entries = unsafe {
        let count = (*privileges).PrivilegeCount as usize;
        let offset = std::mem::offset_of!(TOKEN_PRIVILEGES, Privileges);
        if count > (size_of_val(data.as_slice()) - offset) / size_of::<LUID_AND_ATTRIBUTES>() {
            return Err(TokenCheckError::Io(io::Error::other(
                "invalid token privilege count",
            )));
        }
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*privileges).Privileges).cast::<LUID_AND_ATTRIBUTES>(),
            count,
        )
    };
    for entry in entries {
        let mut name = [0u16; 64];
        let mut size = name.len() as u32;
        // Resolve only a local privilege ID into the fixed buffer.
        succeeded(unsafe {
            LookupPrivilegeNameW(null(), &entry.Luid, name.as_mut_ptr(), &mut size)
        })
        .map_err(TokenCheckError::Io)?;
        let name = String::from_utf16(&name[..size as usize])
            .map_err(|error| TokenCheckError::Io(io::Error::other(error)))?;
        if name != "SeChangeNotifyPrivilege"
            && (current_user_child
                || entry.Attributes & SE_PRIVILEGE_ENABLED != 0
                || !matches!(
                    name.as_str(),
                    "SeShutdownPrivilege"
                        | "SeUndockPrivilege"
                        | "SeIncreaseWorkingSetPrivilege"
                        | "SeTimeZonePrivilege"
                ))
        {
            return Err(TokenCheckError::Mismatch("unqualified VM token privilege"));
        }
    }
    Ok(())
}

const PIPE_CLIENT: u32 =
    FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;

struct VmToken {
    token: OwnedHandle,
    account: LocalSid,
    network: LocalSid,
    system: LocalSid,
    identity: Identity,
    mode: TokenMode,
    /// The parent's group SIDs and masks, captured as the child policy.
    groups: Vec<(Vec<u8>, u32)>,
}

impl VmToken {
    fn derive(owner: &str, account: &str) -> Result<Self, NativeRefusal> {
        Self::derive_checked(owner, account).map_err(|_| NativeRefusal::IdentityUnverified)
    }

    fn derive_checked(owner: &str, account: &str) -> io::Result<Self> {
        let owner = LocalSid::parse(owner)?;
        let account = LocalSid::parse(account)?;
        // Both SIDs are locally parsed, and the caller's process handle is a pseudo-handle.
        if unsafe { EqualSid(owner.0, account.0) } != 0 {
            return Err(io::Error::other("owner and supervisor must differ"));
        }
        Self::derive_for(account, TokenMode::Dedicated)
    }

    fn derive_current_user(account: LocalSid) -> io::Result<Self> {
        require_unimpersonated()?;
        Self::derive_for(account, TokenMode::CurrentUser)
    }

    #[cfg(test)]
    fn test_current_user() -> Self {
        let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
        let (_, user) = tests::current_user();
        let logon = token_data(&token, TokenLogonSid).unwrap();
        assert_eq!(
            unsafe { (*logon.as_ptr().cast::<TOKEN_GROUPS>()).GroupCount },
            1
        );
        let stats = token_data(&token, TokenStatistics).unwrap();
        let authentication =
            unsafe { (*stats.as_ptr().cast::<TOKEN_STATISTICS>()).AuthenticationId };
        let session = token_data(&token, TokenSessionId).unwrap()[0] as u32;
        Self {
            token,
            account: LocalSid::parse(&user).unwrap(),
            network: LocalSid::parse("S-1-5-2").unwrap(),
            system: LocalSid::parse("S-1-5-18").unwrap(),
            identity: Identity {
                logon,
                authentication,
                session,
            },
            mode: TokenMode::CurrentUser,
            groups: Vec::new(),
        }
    }

    fn derive_for(account: LocalSid, mode: TokenMode) -> io::Result<Self> {
        let supervisor = process_token(
            unsafe { GetCurrentProcess() },
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
        )?;
        let identity = token_identity(&supervisor, &account, false, mode)?;
        let groups = captured_groups(token_entries(&token_data(&supervisor, TokenGroups)?)?)?;
        let mut token = null_mut();
        let restricting = SID_AND_ATTRIBUTES {
            Sid: account.0,
            Attributes: 0,
        };
        // The account SID is retained. Write access also requires this explicit SID.
        succeeded(unsafe {
            CreateRestrictedToken(
                supervisor.as_raw_handle(),
                DISABLE_MAX_PRIVILEGE | WRITE_RESTRICTED,
                0,
                null(),
                0,
                null(),
                1,
                &restricting,
                &mut token,
            )
        })?;
        // CreateRestrictedToken transferred ownership.
        let token = unsafe { owned(token) }?;
        let candidate = Self {
            token,
            account,
            network: LocalSid::parse("S-1-5-2")?,
            system: LocalSid::parse("S-1-5-18")?,
            identity,
            mode,
            groups,
        };
        candidate.install_default_acl()?;
        candidate.verify(&candidate.token)?;
        Ok(candidate)
    }

    /// Private local profile key. Only a scope-bound dependency digest goes
    /// into published evidence.
    fn profile_key(&self) -> io::Result<[u8; 32]> {
        use sha2::{Digest, Sha256};
        if self.mode != TokenMode::CurrentUser {
            return Err(io::Error::other("current-user profile required"));
        }
        let mut hash = Sha256::new();
        hash.update(b"arx-vm-execution-profile-v1/current_user");
        for sid in [self.account.0, self.identity.logon_sid()] {
            // Both SIDs are retained by the validated token snapshot.
            let sid = unsafe {
                if IsValidSid(sid) == 0 {
                    return Err(io::Error::other("invalid profile SID"));
                }
                std::slice::from_raw_parts(sid.cast::<u8>(), GetLengthSid(sid) as usize)
            };
            hash.update([sid.len() as u8]);
            hash.update(sid);
        }
        let luid = self.identity.authentication;
        hash.update(luid.LowPart.to_le_bytes());
        hash.update(luid.HighPart.to_le_bytes());
        hash.update(self.identity.session.to_le_bytes());
        for (sid, flags) in &self.groups {
            hash.update([sid.len() as u8]);
            hash.update(sid);
            hash.update(flags.to_le_bytes());
        }
        Ok(hash.finalize().into())
    }

    fn verify_current_profile(&self) -> Result<(), TokenCheckError> {
        self.verify_current_profile_with(
            require_unimpersonated_typed,
            || {
                let parent = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY)
                    .map_err(TokenCheckError::Io)?;
                let current =
                    inspect_token_identity_typed(&parent, &self.account, false, self.mode, true)?;
                let groups_data = token_data(&parent, TokenGroups).map_err(TokenCheckError::Io)?;
                let entries = token_entries(&groups_data).map_err(TokenCheckError::Io)?;
                let groups = captured_groups(entries).map_err(TokenCheckError::Io)?;
                Ok((current, groups))
            },
            || self.verify_typed(&self.token),
        )
    }

    fn verify_current_profile_with(
        &self,
        check_unimpersonated: impl FnOnce() -> Result<(), TokenCheckError>,
        read_parent: impl FnOnce() -> Result<(Identity, Vec<(Vec<u8>, u32)>), TokenCheckError>,
        verify_child: impl FnOnce() -> Result<(), TokenCheckError>,
    ) -> Result<(), TokenCheckError> {
        check_unimpersonated()?;
        let (current, groups) = read_parent()?;
        if !self.identity.same_logon(&current) || groups != self.groups {
            return Err(TokenCheckError::Mismatch("VM parent profile changed"));
        }
        verify_child()
    }

    fn entries(&self) -> [vm_acl::Entry; 4] {
        [
            vm_acl::Entry {
                deny: true,
                sid: self.network.0,
                mask: FILE_ALL_ACCESS,
            },
            vm_acl::Entry {
                deny: false,
                sid: self.account.0,
                mask: FILE_ALL_ACCESS,
            },
            vm_acl::Entry {
                deny: false,
                sid: self.identity.logon_sid(),
                mask: PIPE_CLIENT,
            },
            vm_acl::Entry {
                deny: false,
                sid: self.system.0,
                mask: FILE_ALL_ACCESS,
            },
        ]
    }

    fn install_default_acl(&self) -> io::Result<()> {
        let mut acl_storage = [0usize; 128];
        let acl = acl_storage.as_mut_ptr().cast::<ACL>();
        // Each ACE references a SID owned by self; Windows copies it into the ACL.
        unsafe {
            succeeded(InitializeAcl(
                acl,
                size_of_val(&acl_storage) as u32,
                ACL_REVISION,
            ))?;
            for entry in self.entries() {
                let append = if entry.deny {
                    AddAccessDeniedAce
                } else {
                    AddAccessAllowedAce
                };
                succeeded(append(acl, ACL_REVISION, entry.mask, entry.sid))?;
            }
            let info = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
            succeeded(SetTokenInformation(
                self.token.as_raw_handle(),
                TokenDefaultDacl,
                std::ptr::from_ref(&info).cast(),
                size_of_val(&info) as u32,
            ))
        }
    }

    fn verify(&self, token: &OwnedHandle) -> io::Result<()> {
        self.verify_typed(token).map_err(TokenCheckError::into_io)
    }

    fn verify_typed(&self, token: &OwnedHandle) -> Result<(), TokenCheckError> {
        let observed = inspect_token_identity_typed(token, &self.account, true, self.mode, true)?;
        let groups_data = token_data(token, TokenGroups).map_err(TokenCheckError::Io)?;
        let entries = token_entries(&groups_data).map_err(TokenCheckError::Io)?;
        verify_groups_typed(&self.groups, entries)?;
        if !observed.same_logon(&self.identity) {
            return Err(TokenCheckError::Mismatch(
                "VM child token differs from the qualified policy",
            ));
        }
        let data = token_data(token, TokenDefaultDacl).map_err(TokenCheckError::Io)?;
        // Both identities and the ACL remain backed by the token query allocations.
        let acl_matches = unsafe {
            vm_acl::matches_typed(
                (*data.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl,
                &self.entries(),
            )
        }
        .map_err(TokenCheckError::Io)?;
        if !acl_matches {
            return Err(TokenCheckError::Mismatch(
                "VM child token differs from the qualified policy",
            ));
        }
        Ok(())
    }
}

struct WtsBuffer<T>(*mut T);

impl<T> Drop for WtsBuffer<T> {
    fn drop(&mut self) {
        // The WTS query or enumeration owns the compatible allocation.
        unsafe { WTSFreeMemory(self.0.cast()) };
    }
}

fn wts_session(id: u32) -> Option<WTSINFOEX_LEVEL1_W> {
    let mut buffer = WtsBuffer(null_mut());
    let mut size = 0;
    // Only the local host and typed session info are queried.
    let ok = unsafe {
        WTSQuerySessionInformationW(null_mut(), id, WTSSessionInfoEx, &mut buffer.0, &mut size)
    };
    if ok == 0 || buffer.0.is_null() || (size as usize) < size_of::<WTSINFOEXW>() {
        return None;
    }
    // The aligned buffer and reported level identify the active union member.
    let raw = unsafe { &*buffer.0.cast::<WTSINFOEXW>() };
    if raw.Level != 1 {
        return None;
    }
    // Level 1 selects precisely this union member.
    Some(unsafe { raw.Data.WTSInfoExLevel1 })
}

fn wts_owner_idle(owner: &LocalSid) -> Result<bool, NativeRefusal> {
    let reason = NativeRefusal::OwnerActivityUnverified;
    let mut computer = [0u16; 64];
    let mut len = computer.len() as u32;
    let mut buffer = WtsBuffer(null_mut());
    let mut count = 0;
    // Resolve the local SAM name and enumerate local WTS sessions only.
    unsafe {
        if GetComputerNameExW(ComputerNameNetBIOS, computer.as_mut_ptr(), &mut len) == 0
            || WTSEnumerateSessionsW(null_mut(), 0, 1, &mut buffer.0, &mut count) == 0
            || buffer.0.is_null()
            || !(1..=128).contains(&count)
        {
            return Err(reason);
        }
    }
    let computer = String::from_utf16(&computer[..len as usize]).map_err(|_| reason)?;
    // Enumeration gave count records in this allocation.
    let sessions = unsafe { std::slice::from_raw_parts(buffer.0, count as usize) };
    let mut found = false;
    let mut idle = true;
    for session in sessions
        .iter()
        .filter(|session| session.SessionId != 0 && session.State != WTSListen)
    {
        let sample = wts_session(session.SessionId).ok_or(reason)?;
        let local = wts_text(&sample.DomainName).ok_or(reason)?;
        let user = wts_text(&sample.UserName).ok_or(reason)?;
        if user.is_empty() || !local.eq_ignore_ascii_case(&computer) {
            continue;
        }
        let mut sid = [0usize; 16];
        let mut sid_size = size_of_val(&sid) as u32;
        let mut domain = [0u16; 64];
        let mut domain_size = domain.len() as u32;
        let mut kind = 0;
        // The account string is explicitly qualified by the verified local SAM.
        let matches = unsafe {
            if LookupAccountNameW(
                null(),
                wide(format!("{computer}\\{user}")).as_ptr(),
                sid.as_mut_ptr().cast(),
                &mut sid_size,
                domain.as_mut_ptr(),
                &mut domain_size,
                &mut kind,
            ) == 0
                || kind != SidTypeUser
                || !wts_text(&domain).is_some_and(|name| name.eq_ignore_ascii_case(&computer))
            {
                return Err(reason);
            }
            EqualSid(sid.as_mut_ptr().cast(), owner.0) != 0
        };
        if !matches {
            continue;
        }
        let current = wts_session(session.SessionId).ok_or(reason)?;
        if current.SessionId != session.SessionId
            || sample.SessionId != current.SessionId
            || sample.LogonTime <= 0
            || sample.LogonTime != current.LogonTime
            || sample.DomainName != current.DomainName
            || sample.UserName != current.UserName
            || current.SessionState != WTSActive
            || current.SessionFlags != WTS_SESSIONSTATE_UNLOCK as i32
            || current.LastInputTime < current.LogonTime
        {
            return Err(reason);
        }
        let mut time = FILETIME::default();
        // WTS and FILETIME are both in 100 ns units.
        unsafe { GetSystemTimeAsFileTime(&mut time) };
        let now = ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) as i64;
        if now
            .checked_sub(current.CurrentTime)
            .is_none_or(|age| !(0..=50_000_000).contains(&age))
        {
            return Err(reason);
        }
        let elapsed = current
            .CurrentTime
            .checked_sub(current.LastInputTime)
            .filter(|&time| time >= 0)
            .ok_or(reason)?;
        found = true;
        idle &= elapsed >= 6_000_000_000;
    }
    if found { Ok(idle) } else { Err(reason) }
}

fn wts_text(buffer: &[u16]) -> Option<String> {
    String::from_utf16(&buffer[..buffer.iter().position(|&unit| unit == 0)?]).ok()
}

struct Worker(std::thread::JoinHandle<io::Result<()>>);

impl Worker {
    fn start(
        name: &str,
        work: impl FnOnce() -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(work)
            .map(Self)
    }

    fn cancel_io(&self) {
        // The retained thread handle belongs to this one synchronous I/O worker.
        unsafe { CancelSynchronousIo(self.0.as_raw_handle()) };
    }
}

#[derive(Default)]
struct WorkerSet {
    cancel: Shutdown,
    workers: Vec<Worker>,
}

impl WorkerSet {
    fn stop(&self) {
        self.cancel.cancel();
        for worker in &self.workers {
            worker.cancel_io();
        }
    }

    async fn wait_stopped(&self, deadline: tokio::time::Instant) -> io::Result<()> {
        while self.workers.iter().any(|worker| !worker.0.is_finished()) {
            // A cancellation can race a worker entering its next system call.
            // Repeat until the threads finish or the launch must be quarantined.
            self.stop();
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            tokio::time::sleep_until(
                deadline.min(tokio::time::Instant::now() + Duration::from_millis(5)),
            )
            .await;
        }
        Ok(())
    }

    async fn join(mut self, deadline: tokio::time::Instant) -> io::Result<()> {
        match self.join_all(deadline).await? {
            WorkersEnded { failed: false } => Ok(()),
            WorkersEnded { failed: true } => Err(io::Error::other("stdio worker failed")),
        }
    }

    /// Joins every handle, even after one worker returned an error or
    /// panicked. A timeout keeps all handles for a later attempt. `Ok` proves
    /// that every worker ended; `failed` separately reports how they ended.
    async fn join_all(&mut self, deadline: tokio::time::Instant) -> io::Result<WorkersEnded> {
        self.wait_stopped(deadline).await?;
        let mut failed = false;
        for worker in self.workers.drain(..) {
            failed |= !matches!(worker.0.join(), Ok(Ok(())));
        }
        Ok(WorkersEnded { failed })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkersEnded {
    failed: bool,
}

impl Drop for WorkerSet {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Evidence for host dispatch of the startup `cont`. Once armed, only the
/// stdin worker changes it, and it decides each transition at the pipe write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Dispatch {
    /// No startup window; writes are not tracked.
    Idle,
    /// A fresh sample allows dispatch until this deadline.
    Armed(std::time::Instant),
    /// A write started before the deadline and has not returned yet.
    Writing(std::time::Instant),
    /// Every write in the window started and completed before the deadline.
    WrittenInTime(std::time::Instant),
    /// The worker reached the pipe after the deadline and wrote nothing.
    Expired,
    /// A write completed late or failed, or the worker stopped mid-window.
    Ambiguous,
}

/// Freshness gate for the startup `cont`. Boot arms it from a fresh sample,
/// the stdin worker records what happened at each write, and boot accepts the
/// QMP acknowledgement only when the settled state is `WrittenInTime`. The
/// acknowledgement itself never changes the state. This bounds when the host
/// dispatches `cont`; when the guest starts executing belongs to QEMU.
struct DispatchGate {
    state: tokio::sync::watch::Sender<Dispatch>,
    #[cfg(test)]
    test: GateHooks,
}

#[cfg(test)]
#[derive(Default)]
struct GateHooks {
    /// Injected monotonic time; `None` uses the real clock.
    clock: std::sync::Mutex<Option<std::time::Instant>>,
    /// Called with `true` before and `false` after each tracked write.
    checkpoint: std::sync::Mutex<Option<Checkpoint>>,
    /// Signals the worker's attempt to acquire the dispatch-state lock.
    before_write_lock: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

#[cfg(test)]
type Checkpoint = Box<dyn Fn(bool) + Send>;

impl Default for DispatchGate {
    fn default() -> Self {
        Self {
            state: tokio::sync::watch::Sender::new(Dispatch::Idle),
            #[cfg(test)]
            test: GateHooks::default(),
        }
    }
}

impl DispatchGate {
    fn now(&self) -> std::time::Instant {
        #[cfg(test)]
        if let Some(now) = *self.test.clock.lock().unwrap() {
            return now;
        }
        std::time::Instant::now()
    }

    #[cfg(test)]
    fn checkpoint(&self, before_write: bool) {
        if let Some(hook) = &*self.test.checkpoint.lock().unwrap() {
            hook(before_write);
        }
    }

    fn current(&self) -> Dispatch {
        *self.state.borrow()
    }

    /// Boot: arm from a sample captured after the prelaunch status receipt.
    /// A stale sample, or a gate that is not idle, refuses without arming.
    fn arm(&self, captured: std::time::Instant) -> Result<(), NativeRefusal> {
        self.arm_until(captured, None)
    }

    fn arm_until(
        &self,
        captured: std::time::Instant,
        cutoff: Option<std::time::Instant>,
    ) -> Result<(), NativeRefusal> {
        let deadline = captured + FRESHNESS;
        let deadline = cutoff.map_or(deadline, |cutoff| deadline.min(cutoff));
        let armed = self.state.send_if_modified(|state| {
            let now = self.now();
            if *state != Dispatch::Idle || now < captured || now >= deadline {
                return false;
            }
            *state = Dispatch::Armed(deadline);
            true
        });
        if armed {
            Ok(())
        } else {
            Err(NativeRefusal::ReservesUnverified)
        }
    }

    /// Worker: take write permission from the locked state. A timestamp
    /// observed while waiting for the lock cannot authorize a pipe write.
    fn begin_write(&self) -> bool {
        #[cfg(test)]
        if let Some(signal) = self.test.before_write_lock.lock().unwrap().take() {
            let _ = signal.send(());
        }
        let mut start = false;
        self.state.send_modify(|state| match *state {
            Dispatch::Idle => start = true,
            Dispatch::Armed(deadline) | Dispatch::WrittenInTime(deadline) => {
                start = self.now() < deadline;
                *state = if start {
                    Dispatch::Writing(deadline)
                } else {
                    Dispatch::Expired
                };
            }
            // A single worker never overlaps writes; treat it as unknown.
            Dispatch::Writing(_) => *state = Dispatch::Ambiguous,
            Dispatch::Expired | Dispatch::Ambiguous => {}
        });
        start
    }

    /// Worker: recheck at the pipe handoff, including after a short write
    /// or interruption. Keep the state lock out of the blocking pipe call.
    fn write_pipe(
        &self,
        pipe: &mut impl std::io::Write,
        mut bytes: &[u8],
        cancel: &Shutdown,
    ) -> io::Result<bool> {
        while !bytes.is_empty() {
            let current = self.state.borrow();
            if cancel.is_cancelled() {
                return Ok(false);
            }
            let fresh = match *current {
                Dispatch::Idle => true,
                Dispatch::Writing(deadline) => self.now() < deadline,
                _ => false,
            };
            drop(current);
            if !fresh {
                return Ok(false);
            }
            match pipe.write(bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => bytes = &bytes[written..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }

    /// Worker: record whether the write it started completed in time.
    fn end_write(&self, completed: bool) {
        self.state.send_modify(|state| {
            if let Dispatch::Writing(deadline) = *state {
                *state = if completed && self.now() < deadline {
                    Dispatch::WrittenInTime(deadline)
                } else {
                    Dispatch::Ambiguous
                };
            }
        });
    }

    /// Worker exit: an armed or in-flight window can no longer be confirmed.
    fn worker_stopped(&self) {
        self.state.send_modify(|state| {
            if matches!(*state, Dispatch::Armed(_) | Dispatch::Writing(_)) {
                *state = Dispatch::Ambiguous;
            }
        });
    }

    fn tripped(&self) -> bool {
        matches!(self.current(), Dispatch::Expired | Dispatch::Ambiguous)
    }

    /// Boot, after the QMP acknowledgement: wait until the worker has finished
    /// any write it started, then accept only `WrittenInTime` and disarm.
    /// Every other outcome leaves the gate tripped.
    async fn settle(&self, limit: Duration) -> Result<(), NativeRefusal> {
        self.settle_observed(limit)
            .await
            .map_err(|_| NativeRefusal::ReservesUnverified)
    }

    async fn settle_observed(&self, limit: Duration) -> Result<(), (Dispatch, bool)> {
        let mut watch = self.state.subscribe();
        let finished = matches!(
            tokio::time::timeout(
                limit,
                watch.wait_for(|state| !matches!(state, Dispatch::Writing(_)))
            )
            .await,
            Ok(Ok(_))
        );
        let mut refusal = None;
        self.state.send_modify(|state| match *state {
            Dispatch::WrittenInTime(_) if finished => *state = Dispatch::Idle,
            observed => {
                refusal = Some((observed, finished));
                if !matches!(observed, Dispatch::Expired | Dispatch::Ambiguous) {
                    *state = Dispatch::Ambiguous;
                }
            }
        });
        refusal.map_or(Ok(()), Err)
    }
}

/// Marks an armed window unconfirmable however the stdin worker exits.
struct WorkerStop(std::sync::Arc<DispatchGate>);

impl Drop for WorkerStop {
    fn drop(&mut self) {
        self.0.worker_stopped();
    }
}

/// Async end of the child's stdout. A native read failure in the output
/// worker crosses the bridge as an I/O error. The closed duplex half alone
/// would otherwise look like the peer's end of stream.
struct PipeOutput {
    stream: tokio::io::DuplexStream,
    failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl tokio::io::AsyncRead for PipeOutput {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::{sync::atomic::Ordering, task::Poll};
        let filled = buf.filled().len();
        let this = &mut *self;
        let polled = std::pin::Pin::new(&mut this.stream).poll_read(cx, buf);
        // Buffered bytes still drain first; only the end itself is classified.
        if matches!(polled, Poll::Ready(Ok(())))
            && buf.filled().len() == filled
            && buf.remaining() > 0
            && this.failed.load(Ordering::Acquire)
        {
            return Poll::Ready(Err(io::ErrorKind::Other.into()));
        }
        polled
    }
}

/// Stdout worker. It publishes a read failure before it closes the bridge,
/// so the reader classifies the end it observes as I/O, not EOF.
fn start_output(
    mut pipe: impl std::io::Read + Send + 'static,
    cancel: Shutdown,
    executor: tokio::runtime::Handle,
) -> io::Result<(PipeOutput, Worker)> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::AsyncWriteExt;

    let (stream, mut output) = tokio::io::duplex(8192);
    let failed = std::sync::Arc::new(AtomicBool::new(false));
    let outcome = failed.clone();
    let worker = Worker::start("arx-vm-output", move || {
        let mut bytes = [0; 4096];
        while !cancel.is_cancelled() {
            let count = match read_pipe(&mut pipe, &mut bytes, &cancel) {
                Ok(Some(count)) => count,
                Ok(None) => break,
                Err(error) => {
                    outcome.store(true, Ordering::Release);
                    drop(output);
                    return Err(error);
                }
            };
            let result = executor.block_on(async {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Ok(()),
                    result = output.write_all(&bytes[..count]) => result,
                }
            });
            if result.is_err() {
                break;
            }
        }
        Ok(())
    })?;
    Ok((PipeOutput { stream, failed }, worker))
}

/// Bounded asynchronous ends of the synchronous pipes. A full output buffer
/// applies backpressure, and stderr is drained without retaining peer bytes.
fn start_stdio(
    stdio: Stdio,
    gate: std::sync::Arc<DispatchGate>,
) -> io::Result<(PipeOutput, tokio::io::DuplexStream, WorkerSet)> {
    let mut workers = WorkerSet::default();
    let (read, write) = attach_stdio(stdio, gate, &mut workers)?;
    Ok((read, write, workers))
}

/// Every successfully started worker is recorded even if a later attach fails.
fn attach_stdio(
    stdio: Stdio,
    gate: std::sync::Arc<DispatchGate>,
    set: &mut WorkerSet,
) -> io::Result<BootIo> {
    use tokio::io::{AsyncReadExt, duplex};

    let (writer, mut input) = duplex(8192);
    let runtime = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;

    let (reader, output) = start_output(
        File::from(stdio.output),
        set.cancel.clone(),
        runtime.clone(),
    )?;
    set.workers.push(output);
    let cancel = set.cancel.clone();
    set.workers.push(Worker::start("arx-vm-input", move || {
        let _stopped = WorkerStop(gate.clone());
        let mut pipe = File::from(stdio.input);
        let mut bytes = [0; 4096];
        while !cancel.is_cancelled() {
            let count = runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Ok(0),
                    result = input.read(&mut bytes) => result,
                }
            })?;
            if count == 0 || cancel.is_cancelled() {
                break;
            }
            // Past the armed deadline nothing is written and stdin closes.
            if !gate.begin_write() {
                break;
            }
            #[cfg(test)]
            gate.checkpoint(true);
            let written = gate.write_pipe(&mut pipe, &bytes[..count], &cancel);
            #[cfg(test)]
            gate.checkpoint(false);
            // The worker, not the QMP reply, records whether it was in time.
            gate.end_write(matches!(written, Ok(true)));
            match written {
                Ok(true) => {}
                Ok(false) => break,
                Err(error)
                    if cancel.is_cancelled() || error.kind() == io::ErrorKind::BrokenPipe =>
                {
                    break;
                }
                Err(error) => return Err(error),
            }
            // A late completion is ambiguous delivery: stop and close stdin.
            if gate.tripped() {
                break;
            }
        }
        Ok(())
    })?);
    let cancel = set.cancel.clone();
    set.workers.push(Worker::start("arx-vm-error", move || {
        let mut pipe = File::from(stdio.error);
        let mut bytes = [0; 4096];
        while !cancel.is_cancelled() && read_pipe(&mut pipe, &mut bytes, &cancel)?.is_some() {}
        Ok(())
    })?);
    Ok((reader, writer))
}

fn read_pipe(
    pipe: &mut impl std::io::Read,
    bytes: &mut [u8],
    cancel: &Shutdown,
) -> io::Result<Option<usize>> {
    match pipe.read(bytes) {
        Ok(0) => Ok(None),
        Ok(count) => Ok(Some(count)),
        Err(error) if cancel.is_cancelled() || error.kind() == io::ErrorKind::BrokenPipe => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

impl VmToken {
    fn stdio(&self) -> io::Result<(Stdio, Stdio)> {
        let mut acl_storage = [0usize; 128];
        let acl = acl_storage.as_mut_ptr().cast::<ACL>();
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        // This descriptor is consumed synchronously by CreatePipe. Only the
        // exclusive supervisor and SYSTEM get access by name.
        unsafe {
            succeeded(InitializeAcl(
                acl,
                size_of_val(&acl_storage) as u32,
                ACL_REVISION,
            ))?;
            succeeded(AddAccessDeniedAce(
                acl,
                ACL_REVISION,
                FILE_ALL_ACCESS,
                self.network.0,
            ))?;
            succeeded(AddAccessAllowedAce(
                acl,
                ACL_REVISION,
                FILE_ALL_ACCESS,
                self.account.0,
            ))?;
            succeeded(AddAccessAllowedAce(
                acl,
                ACL_REVISION,
                FILE_ALL_ACCESS,
                self.system.0,
            ))?;
            succeeded(InitializeSecurityDescriptor(ptr_of(&mut descriptor), 1))?;
            succeeded(SetSecurityDescriptorDacl(
                ptr_of(&mut descriptor),
                1,
                acl,
                0,
            ))?;
        }
        let attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr_of(&mut descriptor),
            bInheritHandle: 0,
        };
        Stdio::private(&attrs)
    }
}

fn verify_image(
    child: &Child,
    expected: &LocalPath,
    identity: FileIdentity,
) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::DistributionUnverified;
    let mut name = [0u16; 512];
    let mut size = name.len() as u32;
    succeeded(unsafe {
        QueryFullProcessImageNameW(
            child.process.as_raw_handle(),
            0,
            name.as_mut_ptr(),
            &mut size,
        )
    })
    .map_err(|_| reason)?;
    let name = String::from_utf16(name.get(..size as usize).ok_or(reason)?).map_err(|_| reason)?;
    if !name.eq_ignore_ascii_case(expected.as_str())
        || PathPin::open(expected, false, false)?.identity != identity
    {
        return Err(reason);
    }
    Ok(())
}

fn verify_child_policy(child: &Child, token: &VmToken) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::JobUnverified;
    child.verify_live().map_err(|_| reason)?;
    if !matches!(child.job.active(), Ok(1)) {
        return Err(reason);
    }
    let actual = process_token(child.process.as_raw_handle(), TOKEN_QUERY)
        .map_err(|_| NativeRefusal::IdentityUnverified)?;
    token
        .verify(&actual)
        .map_err(|_| NativeRefusal::IdentityUnverified)?;
    for (policy, flags) in [
        (ProcessChildProcessPolicy, 1u32),
        (ProcessImageLoadPolicy, 0b111),
    ] {
        let mut actual = 0u32;
        succeeded(unsafe {
            GetProcessMitigationPolicy(
                child.process.as_raw_handle(),
                policy,
                ptr_of(&mut actual),
                size_of_val(&actual),
            )
        })
        .map_err(|_| reason)?;
        if actual & flags != flags {
            return Err(reason);
        }
    }
    Ok(())
}

/// Read the host's configured IPv4 DNS resolvers via `iphlpapi.dll`
/// `GetNetworkParams`, matching libslirp's Windows resolver source for `10.0.2.3`.
fn host_dns_servers() -> io::Result<Vec<std::net::IpAddr>> {
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };

    #[repr(C)]
    struct IpAddrString {
        next: *const IpAddrString,
        ip_address: [u8; 16],
        ip_mask: [u8; 16],
        context: u32,
    }

    #[repr(C)]
    struct FixedInfoHeader {
        host_name: [u8; 132],
        domain_name: [u8; 132],
        current_dns_server: *const IpAddrString,
        dns_server_list: IpAddrString,
    }

    const MAX_BUF: usize = 64 * 1024;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;

    let lib = unsafe {
        LoadLibraryExW(
            wide("iphlpapi.dll").as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if lib.is_null() {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let entry = unsafe { GetProcAddress(lib, c"GetNetworkParams".as_ptr().cast()) }
            .ok_or_else(|| io::Error::other("GetNetworkParams unavailable"))?;
        type GetNetworkParamsFn = unsafe extern "system" fn(*mut u8, *mut u32) -> u32;
        let query: GetNetworkParamsFn = unsafe { std::mem::transmute(entry) };
        let mut size = size_of::<FixedInfoHeader>() as u32;
        let mut buf = vec![0u8; size as usize];
        let mut status = unsafe { query(buf.as_mut_ptr(), &mut size) };
        if status == ERROR_BUFFER_OVERFLOW {
            let needed = size as usize;
            if needed < size_of::<FixedInfoHeader>() || needed > MAX_BUF {
                return Err(io::Error::other("GetNetworkParams buffer out of bounds"));
            }
            buf.resize(needed, 0);
            status = unsafe { query(buf.as_mut_ptr(), &mut size) };
        }
        if status != ERROR_SUCCESS || buf.len() < size_of::<FixedInfoHeader>() {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let base = buf.as_ptr() as usize;
        let end = base + buf.len();
        let mut out = Vec::new();
        let header = unsafe { &*buf.as_ptr().cast::<FixedInfoHeader>() };
        let mut current: *const IpAddrString = &header.dns_server_list;
        for _ in 0..32 {
            if current.is_null() {
                break;
            }
            let addr = current as usize;
            if addr < base
                || addr + size_of::<IpAddrString>() > end
                || !addr.is_multiple_of(align_of::<IpAddrString>())
            {
                return Err(io::Error::other("DNS resolver node out of bounds"));
            }
            let node = unsafe { &*current };
            let len = node
                .ip_address
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(node.ip_address.len());
            if let Ok(text) = std::str::from_utf8(&node.ip_address[..len])
                && let Ok(ip) = text.trim().parse::<std::net::Ipv4Addr>()
                && !ip.is_unspecified()
            {
                out.push(std::net::IpAddr::V4(ip));
            }
            current = node.next;
        }
        Ok(out)
    })();
    unsafe { FreeLibrary(lib) };
    result
}

/// The checked stock inventory and protected, empty startup namespace.
struct StockLaunch<'a> {
    bundle: &'a qualification::inventory::Bundle,
    token: &'a VmToken,
    cwd: PathPin,
}

impl StockLaunch<'_> {
    fn recheck(&self) -> Result<(), NativeRefusal> {
        use qualification::inventory;
        require_unimpersonated().map_err(|_| NativeRefusal::IdentityUnverified)?;
        inventory::validate_launch_namespace(
            self.bundle,
            &self.cwd,
            self.token.account.0,
            inventory::safe_dll_search(),
        )
    }

    /// Re-verify bundle, search namespace, workspace custody and every
    /// consumed input pin at the create edge and after prelaunch.
    fn recheck_all(
        &self,
        pins: &[PathPin],
        workspace: Option<&WorkspaceCustody<'_>>,
    ) -> Result<(), NativeRefusal> {
        use qualification::inventory::binding;
        let custody = NativeRefusal::PathCustodyUnverified;
        self.recheck()?;
        if let Some(workspace) = workspace {
            workspace.verify()?;
        }
        for pin in pins {
            binding(pin)?;
            let is_dir = pin.leaf.metadata().map_err(|_| custody)?.is_dir();
            let (current_id, _) = file_identity(&pin.leaf, is_dir).map_err(|_| custody)?;
            if current_id != pin.identity {
                return Err(custody);
            }
            if let Some(expected) = pin.verified_content
                && pin.leaf.metadata().map_err(|_| custody)?.len() != expected.size
            {
                return Err(custody);
            }
        }
        Ok(())
    }

    fn verify_inputs(
        bundle: &qualification::inventory::Bundle,
        cwd: &LocalPath,
        plan: &CommandPlan<'_>,
        pins: &[PathPin],
        workspace: &WorkspaceCustody<'_>,
    ) -> Result<Vec<usize>, NativeRefusal> {
        use ArtifactRole::*;
        use qualification::inventory::binding;
        use std::io::{Read, Seek, SeekFrom};

        let custody = NativeRefusal::PathCustodyUnverified;
        workspace.verify()?;
        let workspace_path = workspace.path();
        if cwd == workspace_path {
            return Err(custody);
        }
        if pins.len() != plan.inputs().len() || workspace_path.overlaps(&bundle.root.path) {
            return Err(custody);
        }
        let mut identities = Vec::with_capacity(pins.len());
        let mut writers = Vec::new();
        let mut images = Vec::new();
        let mut golden = None;
        for (role, artifact) in plan.inputs() {
            let role = *role;
            let path = artifact.path_for(role).map_err(|_| custody)?;
            let (i, pin) = pins
                .iter()
                .enumerate()
                .find(|(_, pin)| pin.path == *path)
                .ok_or(custody)?;
            if identities.contains(&pin.identity) || path.overlaps(cwd) {
                return Err(custody);
            }
            identities.push(pin.identity);
            binding(pin)?;
            match role {
                Qemu | QemuImg => {
                    let leaf = if role == Qemu {
                        "qemu-system-x86_64.exe"
                    } else {
                        "qemu-img.exe"
                    };
                    let expected = bundle.root.path.join(leaf).map_err(|_| custody)?;
                    if path != &expected
                        || path != plan.image()
                        || pin.identity != bundle.identity(leaf)?
                        || pin.mutable
                    {
                        return Err(NativeRefusal::DistributionUnverified);
                    }
                }
                Data => {
                    let expected = bundle.root.path.join("share").map_err(|_| custody)?;
                    if path != &expected || pin.mutable {
                        return Err(NativeRefusal::DistributionUnverified);
                    }
                }
                PublicEgressQemu | PublicEgressPolicy | StaticDisk | StaticVars | Scratch | Dll => {
                    return Err(NativeRefusal::NetworkFenceUnverified);
                }
                _ => {
                    let inspection = plan.reads_snapshot_disk(role);
                    let writable = role.writable() && !inspection;
                    if artifact.identity() != Some(pin.identity) || pin.mutable != writable {
                        return Err(custody);
                    }
                    if writable || inspection {
                        if !workspace_path.contains(path)
                            || pin.identity.volume != workspace.volume()
                        {
                            return Err(custody);
                        }
                        if writable {
                            writers.push(i);
                        }
                    } else if artifact.pin().is_none() || artifact.pin() != pin.verified_content {
                        return Err(custody);
                    }
                    match role {
                        Golden => golden = Some(pin),
                        GuestDisk | TestInstallDisk | SnapshotDisk | SnapshotStagingDisk => {
                            images.push(pin)
                        }
                        Staging if !pin.leaf.metadata().is_ok_and(|m| m.len() == 0) => {
                            return Err(custody);
                        }
                        _ => {}
                    }
                }
            }
        }
        let inspect = |pin: &PathPin, backing: Option<&LocalPath>| {
            let mut file = &pin.leaf;
            let mut header = vec![0; super::qemu_img::CLUSTER];
            file.seek(SeekFrom::Start(0))
                .and_then(|_| file.read_exact(&mut header))
                .map_err(|_| custody)?;
            super::qemu_img::inspect_header(&header, backing).map_err(|_| custody)
        };
        let golden_size = golden.map(|pin| inspect(pin, None)).transpose()?;
        for disk in images {
            let size = inspect(disk, golden.map(|pin| &pin.path))?;
            if golden_size.is_some_and(|expected| size != expected) {
                return Err(custody);
            }
        }
        Ok(writers)
    }
}

struct LaunchInputs<'a> {
    plan: CommandPlan<'a>,
    cwd: &'a LocalPath,
    workspace: WorkspaceCustody<'a>,
    bundle: &'a qualification::inventory::Bundle,
    memory: usize,
    cpu_rate: u32,
    installer: Option<InstallerStage>,
    finalization_slots: Option<super::PairSlots>,
    // The issuer must retain all executable, firmware, DLL/data, media,
    // volume and writable slot custody here, including their ancestors.
    pins: Vec<PathPin>,
}

/// Private checked execution core shared below separate daemon and qualifier
/// entry points. Neither entry can mint or convert into the other's authority.
struct CheckedLaunchCore<'a> {
    child: Child,
    workers: WorkerSet,
    shutdown_observed: Option<(bool, bool)>,
    workers_ended: Option<WorkersEnded>,
    gate: std::sync::Arc<DispatchGate>,
    pins: std::sync::Arc<[PathPin]>,
    setup_vars_handoff: Option<File>,
    writers: Vec<usize>,
    memory: usize,
    cpu_rate: u32,
    stock: StockLaunch<'a>,
    workspace: WorkspaceCustody<'a>,
    finalization: Option<(super::PairSlots, [usize; 2])>,
    kind: PlanKind,
    installer: Option<InstallerStage>,
}

/// Creation custody comes from the kernel create edge, independent of the
/// refusal reason. Keeping the Job also preserves kill-on-close.
enum LaunchCreation {
    NeverCreated,
    Created(Box<CreatedChild>),
    Unconfirmed {
        job: Job,
        _partial: Option<PartialChild>,
    },
}

struct CreatedChild {
    child: Child,
    /// Retains every unjoined worker handle until settlement drains them.
    workers: WorkerSet,
    worker_failed: bool,
}

impl CreatedChild {
    /// Confirms an exited child, an empty Job and that every stdio worker
    /// ended. Any failure leaves the remaining custody for a later attempt.
    async fn settle(&mut self, deadline: tokio::time::Instant) -> io::Result<()> {
        self.workers.stop();
        if !self.child.exited()? || self.child.job.active()? != 0 {
            self.child.job.terminate()?;
        }
        // Reaping confirms both the child exit and an empty Job.
        self.child.reap(deadline).await?;
        let ended = self.workers.join_all(deadline).await?;
        self.worker_failed |= ended.failed;
        Ok(())
    }
}

struct HoldRefusal {
    reason: NativeRefusal,
    creation: LaunchCreation,
}

impl From<NativeRefusal> for HoldRefusal {
    fn from(reason: NativeRefusal) -> Self {
        Self {
            reason,
            creation: LaunchCreation::NeverCreated,
        }
    }
}

impl HoldRefusal {
    fn from_spawn((error, job): (SpawnError, Job)) -> Self {
        let reason = NativeRefusal::JobUnverified;
        if error.stage == SpawnStage::Verify || !matches!(job.active(), Ok(0)) {
            let _ = job.terminate();
            Self {
                reason,
                creation: LaunchCreation::Unconfirmed {
                    job,
                    _partial: error.partial,
                },
            }
        } else {
            reason.into()
        }
    }

    fn created(reason: NativeRefusal, child: Child, workers: WorkerSet) -> Self {
        workers.stop();
        let _ = child.job.terminate();
        Self {
            reason,
            creation: LaunchCreation::Created(Box::new(CreatedChild {
                child,
                workers,
                worker_failed: false,
            })),
        }
    }
}

fn attach_checked_child(
    child: Child,
    verify: impl FnOnce(&Child) -> Result<(), NativeRefusal>,
    attach: impl FnOnce(&mut WorkerSet) -> Result<BootIo, NativeRefusal>,
) -> Result<(Child, WorkerSet, BootIo), HoldRefusal> {
    let mut workers = WorkerSet::default();
    match verify(&child).and_then(|()| attach(&mut workers)) {
        Ok(io) => Ok((child, workers, io)),
        Err(reason) => Err(HoldRefusal::created(reason, child, workers)),
    }
}

impl<'a> CheckedLaunchCore<'a> {
    fn hold_checked<T>(
        token: &'a VmToken,
        inputs: LaunchInputs<'a>,
        refresh: impl FnOnce(&CommandPlan<'_>) -> Result<(std::time::Instant, T), NativeRefusal>,
        before_spawn: impl FnOnce() -> Result<(), NativeRefusal>,
    ) -> Result<(Self, BootIo, T), HoldRefusal> {
        let kind = inputs.plan.kind();
        // ADR 0064: a public-egress plan also stays closed until its patched
        // build, policy custody and route evidence have native qualification.
        if matches!(
            kind,
            PlanKind::StaticWebBoot
                | PlanKind::PublicEgressBoot
                | PlanKind::StaticComposeInstaller
                | PlanKind::StaticComposeDetached
        ) {
            return Err(NativeRefusal::NetworkFenceUnverified.into());
        }
        match (kind, inputs.installer) {
            (PlanKind::InstallerBoot(phase), Some(stage))
                if phase == stage.phase() && tokio::time::Instant::now() >= stage.deadline() =>
            {
                return Err(NativeRefusal::InstallerDeadlineExpired.into());
            }
            (PlanKind::InstallerBoot(phase), Some(stage)) if phase == stage.phase() => {}
            (PlanKind::InstallerBoot(_), _) | (_, Some(_)) => {
                return Err(NativeRefusal::InstallerPhaseMismatch.into());
            }
            _ => {}
        }
        let stock = StockLaunch {
            bundle: inputs.bundle,
            token,
            cwd: PathPin::open(inputs.cwd, true, false)?,
        };
        let writers = StockLaunch::verify_inputs(
            stock.bundle,
            &stock.cwd.path,
            &inputs.plan,
            &inputs.pins,
            &inputs.workspace,
        )?;
        use super::runtime::InstallerPhase;
        let finalization = match (kind, inputs.finalization_slots) {
            (PlanKind::InstallerBoot(InstallerPhase::WindowsFinalize), Some(slots)) => {
                let index = |role| {
                    inputs
                        .plan
                        .installer_artifacts()
                        .iter()
                        .find(|input| input.role == role)
                        .and_then(|input| {
                            inputs.pins.iter().position(|pin| pin.path == *input.path)
                        })
                        .ok_or(NativeRefusal::PathCustodyUnverified)
                };
                Some((
                    slots,
                    [
                        index(ArtifactRole::TestInstallDisk)?,
                        index(ArtifactRole::Vars)?,
                    ],
                ))
            }
            (PlanKind::InstallerBoot(InstallerPhase::WindowsFinalize), None) | (_, Some(_)) => {
                return Err(NativeRefusal::InstallerPhaseMismatch.into());
            }
            (_, None) => None,
        };
        let mut setup_vars = None;
        for expected in inputs.plan.installer_artifacts() {
            let (index, setup_digest) = bind_installer_pin(expected, &inputs.pins)?;
            if let Some(digest) = setup_digest {
                setup_vars = Some((index, digest));
            }
        }
        if kind == PlanKind::InstallerBoot(InstallerPhase::UbuntuInstall) {
            // The foreground preparation pins the initial stock VARS bytes.
            // Retain writer exclusion until QEMU opens its writable copy.
            if let Some((_, vars)) = inputs
                .plan
                .inputs()
                .iter()
                .find(|(role, _)| *role == ArtifactRole::Vars)
                && let Some(digest) = vars.pin()
            {
                let path = vars
                    .path_for(ArtifactRole::Vars)
                    .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
                let index = inputs
                    .pins
                    .iter()
                    .position(|pin| &pin.path == path)
                    .ok_or(NativeRefusal::PathCustodyUnverified)?;
                setup_vars = Some((index, digest));
            }
        }
        let gate = std::sync::Arc::new(DispatchGate::default());
        let (stdio, parent) = token.stdio().map_err(|_| NativeRefusal::QmpUnverified)?;
        let job = Job::with_process_limit(inputs.memory, Some(inputs.cpu_rate), Some(1))
            .map_err(|_| NativeRefusal::JobUnverified)?;
        let environment = qualification::inventory::restricted_environment(inputs.cwd)?;
        let setup_vars_handoff = setup_vars
            .map(|(index, digest)| guard_setup_vars(&inputs.pins[index], digest))
            .transpose()?;
        if inputs
            .installer
            .is_some_and(|stage| tokio::time::Instant::now() >= stage.deadline())
        {
            return Err(NativeRefusal::InstallerDeadlineExpired.into());
        }
        // Recheck all consumed inputs, bundle and workspace custody right at
        // the actual process-creation edge.
        stock.recheck_all(&inputs.pins, Some(&inputs.workspace))?;
        let (captured, authority) = refresh(&inputs.plan)?;
        // The checked Job is unnamed, owned by this stack frame, omitted
        // from stdio inheritance and assigned by the kernel at creation.
        job.verify_empty_noninheritable()
            .map_err(|_| NativeRefusal::JobUnverified)?;
        before_spawn()?;
        fresh_observation(
            captured,
            std::time::Instant::now(),
            NativeRefusal::ReservesUnverified,
        )?;
        let child = spawn(
            Launch::Restricted {
                token,
                plan: &inputs.plan,
            },
            Path::new(inputs.cwd.as_str()),
            &environment,
            job,
            stdio,
        )
        .map_err(HoldRefusal::from_spawn)?;
        let (child, workers, (read, write)) = attach_checked_child(
            child,
            |child| {
                child
                    .verify_live()
                    .map_err(|_| NativeRefusal::JobUnverified)?;
                let program = if matches!(kind, PlanKind::Boot | PlanKind::InstallerBoot(_)) {
                    "qemu-system-x86_64.exe"
                } else {
                    "qemu-img.exe"
                };
                let expected_image = inputs.bundle.identity(program)?;
                verify_image(child, inputs.plan.image(), expected_image)?;
                verify_child_policy(child, token)?;
                child
                    .job
                    .verify_limits(inputs.memory, Some(inputs.cpu_rate), Some(1))
                    .map_err(|_| NativeRefusal::JobUnverified)
            },
            |workers| {
                attach_stdio(parent, gate.clone(), workers)
                    .map_err(|_| NativeRefusal::QmpUnverified)
            },
        )?;
        Ok((
            Self {
                child,
                workers,
                shutdown_observed: None,
                workers_ended: None,
                gate,
                pins: inputs.pins.into(),
                setup_vars_handoff,
                writers,
                memory: inputs.memory,
                cpu_rate: inputs.cpu_rate,
                stock,
                workspace: inputs.workspace,
                finalization,
                kind,
                installer: inputs.installer,
            },
            (read, write),
            authority,
        ))
    }
}

type PipeQmp = Qmp<PipeOutput, tokio::io::DuplexStream>;
type SerialGuest = GuestChannel<tokio::net::windows::named_pipe::NamedPipeClient>;
type BootIo = (PipeOutput, tokio::io::DuplexStream);
type HeldRelease = (StopDisposition, Option<HeldWriters>);

const LIVE_POLL: Duration = Duration::from_secs(1);

/// Keep polling the same operation and prefer refusal over a simultaneous
/// result. The caller owns teardown after this future releases its I/O borrows.
async fn monitor_running<T, E>(
    work: impl std::future::Future<Output = Result<T, E>>,
    mut observe: impl FnMut() -> Result<(), E>,
    cancel: &Shutdown,
    deadline: tokio::time::Instant,
    interrupted: impl Fn(ChannelError) -> E,
) -> Result<T, E> {
    use tokio::time::Instant;
    tokio::pin!(work);
    let check = || {
        if cancel.is_cancelled() {
            Err(interrupted(ChannelError::Cancelled))
        } else if Instant::now() >= deadline {
            Err(interrupted(ChannelError::Timeout))
        } else {
            Ok(())
        }
    };
    check()?;
    // An observer refusal, like a work result, still takes the terminal
    // sample and yields to cancellation or expiry before it is returned.
    let result = match observe() {
        Err(error) => Err(error),
        Ok(()) => loop {
            check()?;
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break Err(interrupted(ChannelError::Cancelled)),
                _ = tokio::time::sleep_until(deadline) => break Err(interrupted(ChannelError::Timeout)),
                _ = tokio::time::sleep(LIVE_POLL) => {
                    if let Err(error) = observe() {
                        break Err(error);
                    }
                }
                result = &mut work => break result,
            }
        },
    };
    let terminal = observe();
    check()?;
    result.and_then(|value| terminal.map(|()| value))
}

/// Runs the installed setup phase under the live monitor. The setup future
/// stamps its accepted `cont` itself, so the monitor's terminal sample and
/// any later scheduling count against the guest-ready budget it returns.
async fn monitor_setup<T>(
    setup: impl std::future::Future<Output = Result<T, NativeRefusal>>,
    observe: impl FnMut() -> Result<(), NativeRefusal>,
    cancel: &Shutdown,
    setup_by: tokio::time::Instant,
    boot_by: tokio::time::Instant,
) -> Result<(T, tokio::time::Instant), NativeRefusal> {
    let (value, accepted) = monitor_running(
        async {
            let value = setup.await?;
            Ok::<_, NativeRefusal>((value, tokio::time::Instant::now()))
        },
        observe,
        cancel,
        setup_by,
        |_| NativeRefusal::QmpUnverified,
    )
    .await?;
    Ok((value, guest_ready_cutoff(boot_by, accepted)))
}

/// Release keeps its own teardown budget so custody evidence is collected
/// after a refusal too. Output is accepted only if a host sample taken after
/// release settles succeeds and its capture time is still fresh, and then
/// cancellation and the original cutoff, checked after that sample, still
/// allow it; this is the same order as the common monitor tail. The held
/// release is returned in either case.
async fn accept_after_release<T, E, R, S>(
    result: Result<T, E>,
    release: impl std::future::Future<Output = R>,
    cancel: &Shutdown,
    deadline: tokio::time::Instant,
    recheck: impl FnOnce() -> S,
    stale: E,
    interrupted: impl Fn(ChannelError) -> E,
) -> (Result<T, E>, R)
where
    S: std::future::Future<Output = Result<std::time::Instant, E>>,
{
    let release = release.await;
    let result = match result {
        Ok(value) => {
            let sample = recheck().await.and_then(|captured| {
                fresh_observation(
                    captured,
                    std::time::Instant::now(),
                    NativeRefusal::ReservesUnverified,
                )
                .map_err(|_| stale)
            });
            if cancel.is_cancelled() {
                Err(interrupted(ChannelError::Cancelled))
            } else if tokio::time::Instant::now() >= deadline {
                Err(interrupted(ChannelError::Timeout))
            } else {
                sample.map(|()| value)
            }
        }
        Err(error) => Err(error),
    };
    (result, release)
}

trait InstallerChild {
    fn observe(&self, captured: std::time::Instant) -> Result<(), NativeRefusal>;
    async fn dispatch(
        &self,
        qmp: &mut PipeQmp,
        cancel: &Shutdown,
        stage: InstallerStage,
        refresh: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
    ) -> Result<(), InstallerFailureReason>;
    async fn exit(&self, by: tokio::time::Instant) -> Result<(), InstallerFailureReason>;
}

impl InstallerChild for CheckedLaunchCore<'_> {
    fn observe(&self, captured: std::time::Instant) -> Result<(), NativeRefusal> {
        self.check_running(|| Ok(captured), true)
    }

    async fn dispatch(
        &self,
        qmp: &mut PipeQmp,
        cancel: &Shutdown,
        stage: InstallerStage,
        mut refresh: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
    ) -> Result<(), InstallerFailureReason> {
        self.dispatch_cont(qmp, cancel, &mut refresh, Some(stage.deadline()))
            .await
    }

    async fn exit(&self, by: tokio::time::Instant) -> Result<(), InstallerFailureReason> {
        self.child
            .reap(by)
            .await
            .map_err(|_| InstallerFailureReason::ReleaseUnverified)?;
        (self.child.exit_code() == Ok(0))
            .then_some(())
            .ok_or(InstallerFailureReason::ProcessFailed)
    }
}

async fn installer_session(
    child: &impl InstallerChild,
    stage: InstallerStage,
    (read, write): BootIo,
    cancel: &Shutdown,
    refresh: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
) -> Result<(), InstallerFailureReason> {
    use InstallerFailureReason as Failure;
    use tokio::time::Instant;
    let refresh = std::cell::RefCell::new(refresh);
    monitor_running(
        async {
            let mut qmp = Qmp::installer_handshake(read, write, cancel, stage).await?;
            qmp.verify_whpx(cancel).await.map_err(|error| match error {
                ChannelError::Timeout | ChannelError::Cancelled => Failure::from(error),
                _ => Failure::Native(NativeRefusal::WhpxUnverified),
            })?;
            child
                .dispatch(&mut qmp, cancel, stage, || refresh.borrow_mut()())
                .await?;
            qmp.observe_installer_shutdown(cancel).await?;
            // Keep QMP stdin open until the owned child exits.
            let by = stage
                .deadline()
                .min(Instant::now() + Duration::from_secs(2));
            let exited = tokio::time::timeout_at(by, child.exit(by))
                .await
                .map_err(|_| Failure::Timeout)?;
            // Expiry wins: an exit observed at or after the grace cutoff is
            // late even when it was already ready when polled.
            if Instant::now() >= by {
                return Err(Failure::Timeout);
            }
            exited
        },
        || {
            refresh.borrow_mut()()
                .and_then(|captured| child.observe(captured))
                .map_err(Failure::Native)
        },
        cancel,
        stage.deadline(),
        Failure::from,
    )
    .await
}

enum BootOutcome {
    Installed(Box<(PipeQmp, SerialGuest)>),
    InstallerReleased(Option<offline_read::ReleasedPair>, HeldWriters),
}

enum BootFailure {
    Native(NativeRefusal),
    Installer(InstallerFailure, Option<HeldWriters>),
}

impl From<ChannelError> for InstallerFailureReason {
    fn from(error: ChannelError) -> Self {
        match error {
            ChannelError::Timeout => Self::Timeout,
            ChannelError::Cancelled => Self::Cancelled,
            error => Self::Protocol(error),
        }
    }
}

impl CheckedLaunchCore<'_> {
    fn check_running(
        &self,
        refresh: impl FnOnce() -> Result<std::time::Instant, NativeRefusal>,
        allow_exited: bool,
    ) -> Result<(), NativeRefusal> {
        let job_error = NativeRefusal::JobUnverified;
        let captured = refresh()?;
        self.stock.recheck_all(&self.pins, Some(&self.workspace))?;
        self.child
            .job
            .verify_limits(self.memory, Some(self.cpu_rate), Some(1))
            .map_err(|_| job_error)?;
        let live = self
            .child
            .verify_live()
            .map_err(|_| job_error)
            .and_then(|_| verify_child_policy(&self.child, self.stock.token));
        if let Err(reason) = live
            && !(allow_exited
                && self.child.exited().is_ok_and(|exited| exited)
                && matches!(self.child.job.active(), Ok(0)))
        {
            return Err(reason);
        }
        fresh_observation(
            captured,
            std::time::Instant::now(),
            NativeRefusal::ReservesUnverified,
        )
    }

    async fn helper(
        &mut self,
        io: BootIo,
        cancel: &Shutdown,
        deadline: tokio::time::Instant,
        refresh: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> (Result<Vec<u8>, NativeRefusal>, StopDisposition) {
        let (output, (disposition, _)) = self
            .helper_held(io, cancel, deadline, refresh, verify_release)
            .await;
        (output, disposition)
    }

    async fn helper_held(
        &mut self,
        (mut read, write): BootIo,
        cancel: &Shutdown,
        deadline: tokio::time::Instant,
        mut refresh: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> (Result<Vec<u8>, NativeRefusal>, HeldRelease) {
        use tokio::io::AsyncReadExt;
        let refused = NativeRefusal::JobUnverified;
        let work = async {
            if !matches!(
                self.kind,
                PlanKind::Inspect
                    | PlanKind::QualifierCreate
                    | PlanKind::InspectWritable
                    | PlanKind::Mutate
            ) {
                return Err(NativeRefusal::QualificationMissing);
            }
            self.stock.recheck_all(&self.pins, Some(&self.workspace))?;
            self.child
                .resume_checked_by(cancel, deadline, &mut refresh)?;
            monitor_running(
                async {
                    let mut output = Vec::new();
                    (&mut read)
                        .take(16 * 1024 + 1)
                        .read_to_end(&mut output)
                        .await
                        .map_err(|_| refused)?;
                    self.child.reap(deadline).await.map_err(|_| refused)?;
                    if output.len() > 16 * 1024 || self.child.exit_code()? != 0 {
                        return Err(refused);
                    }
                    Ok(output)
                },
                || self.check_running(&mut refresh, true),
                cancel,
                deadline,
                |_| refused,
            )
            .await
        };
        let result = work.await;
        drop(write);
        if result.is_err() && self.child.job.terminate().is_err() {
            return (result, (StopDisposition::ChargedQuarantine, None));
        }
        let successful = result.is_ok();
        let release_by = tokio::time::Instant::now() + Duration::from_secs(5);
        let release_by = if self.kind == PlanKind::InspectWritable {
            release_by.min(deadline)
        } else {
            release_by
        };
        accept_after_release(
            result,
            self.release_evidence_held(successful, successful, release_by, verify_release),
            cancel,
            deadline,
            || std::future::ready(refresh()),
            NativeRefusal::ReservesUnverified,
            |_| refused,
        )
        .await
    }

    async fn boot(
        &mut self,
        endpoint: Option<(&VmToken, &SerialName)>,
        io: BootIo,
        cancel: &Shutdown,
        revalidate: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<BootOutcome, BootFailure> {
        let cutoff = tokio::time::Instant::now()
            + Duration::from_secs(60)
            + super::guest::GUEST_READY_BUDGET;
        self.boot_until(
            endpoint.map(|(token, serial)| (token, serial, cutoff)),
            io,
            cancel,
            revalidate,
            verify_release,
        )
        .await
    }

    async fn boot_until(
        &mut self,
        endpoint: Option<(&VmToken, &SerialName, tokio::time::Instant)>,
        io: BootIo,
        cancel: &Shutdown,
        mut revalidate: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<BootOutcome, BootFailure> {
        if let Some(stage) = self.installer {
            return self
                .boot_installer(stage, io, cancel, &mut revalidate, verify_release)
                .await
                .map_err(|(failure, held)| BootFailure::Installer(failure, held));
        }
        let (token, serial, boot_by) = endpoint
            .filter(|_| self.kind == PlanKind::Boot)
            .ok_or(BootFailure::Native(NativeRefusal::QmpUnverified))?;
        let setup_by = setup_cutoff(boot_by, tokio::time::Instant::now());
        self.child
            .resume_checked_by(cancel, setup_by, &mut revalidate)
            .map_err(BootFailure::Native)?;
        let refresh = std::cell::RefCell::new(revalidate);
        let ((mut qmp, pipe), ready_by) = monitor_setup(
            async {
                // QEMU can wait for a serial client before sending QMP greeting.
                let pipe = connect_serial(
                    serial,
                    &self.child,
                    token.account.0,
                    &token.entries(),
                    cancel,
                    setup_by.min(tokio::time::Instant::now() + Duration::from_secs(10)),
                )
                .await?;
                let (read, write) = io;
                let mut qmp = Qmp::handshake(read, write, cancel)
                    .await
                    .map_err(|_| NativeRefusal::QmpUnverified)?;
                qmp.verify_whpx(cancel)
                    .await
                    .map_err(|_| NativeRefusal::WhpxUnverified)?;
                self.dispatch_cont(
                    &mut qmp,
                    cancel,
                    &mut || refresh.borrow_mut()(),
                    Some(setup_by),
                )
                .await
                .map_err(|error| match error {
                    InstallerFailureReason::Native(reason) => reason,
                    _ => NativeRefusal::QmpUnverified,
                })?;
                Ok((qmp, pipe))
            },
            || self.check_running(&mut *refresh.borrow_mut(), false),
            cancel,
            setup_by,
            boot_by,
        )
        .await
        .map_err(BootFailure::Native)?;
        // The guest-ready phase started at the accepted `cont`.
        let guest = monitor_running(
            async {
                GuestChannel::ready(pipe, &mut qmp, cancel)
                    .await
                    .map_err(|_| NativeRefusal::GuestEndpointUnverified)
            },
            || self.check_running(&mut *refresh.borrow_mut(), false),
            cancel,
            ready_by,
            |_| NativeRefusal::GuestEndpointUnverified,
        )
        .await
        .map_err(BootFailure::Native)?;
        Ok(BootOutcome::Installed(Box::new((qmp, guest))))
    }

    async fn dispatch_cont(
        &self,
        qmp: &mut PipeQmp,
        cancel: &Shutdown,
        revalidate: &mut impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
        cutoff: Option<tokio::time::Instant>,
    ) -> Result<(), InstallerFailureReason> {
        // Resample between the prelaunch status receipt and `cont`, rechecking
        // bundle, search namespace and all consumed input pins before arming.
        let mut refused = None;
        let gate = self.gate.clone();
        let released = qmp
            .release_startup_hold(cancel, || {
                let armed = self
                    .stock
                    .recheck_all(&self.pins, Some(&self.workspace))
                    .and_then(|()| revalidate())
                    .and_then(|captured| match cutoff {
                        Some(deadline) => gate.arm_until(captured, Some(deadline.into_std())),
                        None => gate.arm(captured),
                    });
                match armed {
                    Ok(()) => true,
                    Err(reason) => {
                        refused = Some(reason);
                        false
                    }
                }
            })
            .await;
        released.map_err(|error| {
            refused.map_or_else(|| error.into(), InstallerFailureReason::Native)
        })?;
        // The acknowledgement does not touch the gate. Accept it only once the
        // worker has finished its write and recorded it as in time.
        let grace = Duration::from_secs(2);
        if let Some(deadline) = cutoff {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(InstallerFailureReason::Cancelled),
                _ = tokio::time::sleep_until(deadline) => return Err(InstallerFailureReason::Timeout),
                result = self.gate.settle(grace) => result,
            }
        } else {
            self.gate.settle(grace).await
        }
        .map_err(InstallerFailureReason::Native)
    }

    async fn boot_installer(
        &mut self,
        stage: InstallerStage,
        io: BootIo,
        cancel: &Shutdown,
        revalidate: &mut impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<BootOutcome, (InstallerFailure, Option<HeldWriters>)> {
        use InstallerFailureReason as Failure;
        use tokio::time::Instant;
        let grace = Duration::from_secs(2);
        let deadline = stage.deadline();
        let result = async {
            if cancel.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Failure::Timeout);
            }
            self.child
                .resume_checked_with_handoff(cancel, Some(deadline), revalidate, || {
                    // QEMU opens writable pflash by name after resume. The
                    // original object and ancestry pin stays held across it.
                    drop(self.setup_vars_handoff.take());
                })
                .map_err(Failure::Native)?;
            installer_session(self, stage, io, cancel, &mut *revalidate).await
        }
        .await;

        let release_by = match result {
            Ok(()) => deadline.min(Instant::now() + grace),
            Err(reason) => {
                // Failure teardown has an independent bound after stage expiry.
                if self.child.job.terminate().is_err() {
                    return Err((
                        InstallerFailure {
                            phase: stage.phase(),
                            reason,
                            recovery: StopDisposition::ChargedQuarantine,
                        },
                        None,
                    ));
                }
                Instant::now() + grace
            }
        };
        let successful = result.is_ok();
        let mut result = result;
        let (observed, held) = {
            let release =
                self.release_evidence_held(successful, successful, release_by, verify_release);
            tokio::pin!(release);
            // Schedule the next sample after the previous one returns so a
            // slow observation cannot starve the independently bounded release.
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled(), if result.is_ok() => result = Err(Failure::Cancelled),
                    _ = tokio::time::sleep(LIVE_POLL), if result.is_ok() => {
                        result = revalidate().map(|_| ()).map_err(Failure::Native);
                    }
                    proof = &mut release => break proof,
                }
            }
        };
        if result.is_ok() {
            result = self
                .check_running(revalidate, true)
                .map_err(Failure::Native);
        }
        if let Err(failure) = stage.complete(result, observed, cancel.is_cancelled()) {
            return Err((failure, held));
        }
        let unverified = || {
            (
                InstallerFailure {
                    phase: stage.phase(),
                    reason: Failure::ReleaseUnverified,
                    recovery: StopDisposition::ChargedQuarantine,
                },
                None,
            )
        };
        let held = held.ok_or_else(unverified)?;
        let released = match self.finalization {
            Some((slots, indices)) => {
                let readers =
                    offline_read::ReleasedPair::hold(slots, self.pins.clone(), indices, observed)
                        .map_err(|_| unverified())?;
                Some(readers)
            }
            None => None,
        };
        Ok(BootOutcome::InstallerReleased(released, held))
    }

    /// Every caller, including a failed boot, must reconcile this owned launch.
    /// On inconclusive evidence the caller retains the pinned generation.
    async fn stop(
        &mut self,
        qmp: Option<PipeQmp>,
        cancel: &Shutdown,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> StopDisposition {
        self.stop_held(qmp, cancel, verify_release).await.0
    }

    async fn stop_held(
        &mut self,
        qmp: Option<PipeQmp>,
        cancel: &Shutdown,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> (StopDisposition, Option<HeldWriters>) {
        let (guest_shutdown, clean_process_exit) = match self.shutdown_observed {
            Some(_) => {
                let job_empty = matches!(self.child.job.active(), Ok(0));
                let (workers, job) = (&self.workers, &self.child.job);
                match retry_shutdown_observation(&mut self.shutdown_observed, job_empty, || {
                    workers.stop();
                    job.terminate()
                }) {
                    Some(observed) => observed,
                    None => return (StopDisposition::ChargedQuarantine, None),
                }
            }
            None => {
                let guest_shutdown = match qmp {
                    Some(qmp) => qmp.powerdown(cancel).await.is_ok(),
                    None => false,
                };
                let grace = tokio::time::Instant::now() + Duration::from_secs(2);
                let clean_process_exit = if guest_shutdown && self.child.reap(grace).await.is_ok() {
                    self.child.exit_code() == Ok(0)
                } else {
                    self.workers.stop();
                    if !matches!(self.child.job.active(), Ok(0))
                        && self.child.job.terminate().is_err()
                    {
                        return (StopDisposition::ChargedQuarantine, None);
                    }
                    false
                };
                self.shutdown_observed = Some((guest_shutdown, clean_process_exit));
                (guest_shutdown, clean_process_exit)
            }
        };
        self.release_evidence_held(
            guest_shutdown,
            clean_process_exit,
            tokio::time::Instant::now() + Duration::from_secs(2),
            verify_release,
        )
        .await
    }

    async fn release_evidence_held(
        &mut self,
        guest_shutdown: bool,
        clean_process_exit: bool,
        deadline: tokio::time::Instant,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> (StopDisposition, Option<HeldWriters>) {
        let mut observed = StopEvidence {
            guest_shutdown,
            clean_process_exit,
            ..StopEvidence::default()
        };
        observed.process_exited = self.child.reap(deadline).await.is_ok();
        observed.job_empty = matches!(self.child.job.active(), Ok(0));
        if self.workers_ended.is_none() {
            self.workers_ended = self.workers.join_all(deadline).await.ok();
        }
        if let Some(ended) = self.workers_ended {
            observed.workers_joined = true;
            observed.clean_process_exit &= !ended.failed;
        }
        let pins = self.pins.clone();
        let writers = self.writers.clone();
        finish_release_held(
            observed,
            deadline,
            move || {
                let guards = writers
                    .iter()
                    .map(|&index| pins[index].reopen_without_writer())
                    .collect::<io::Result<Vec<_>>>()?;
                // Keep every pin and the additional writer exclusions
                // through the read-only generation check.
                Ok((pins, guards))
            },
            verify_release,
        )
        .await
    }
}

type HeldWriters = (std::sync::Arc<[PathPin]>, Vec<File>);

/// The installed setup phase (resume through accepted `cont`) never outlives
/// 60 seconds or the start's own cutoff.
fn setup_cutoff(boot_by: tokio::time::Instant, now: tokio::time::Instant) -> tokio::time::Instant {
    boot_by.min(now + Duration::from_secs(60))
}

/// The guest-ready phase starts at the accepted `cont` and never outlives
/// `GUEST_READY_BUDGET` or the start's own cutoff.
fn guest_ready_cutoff(
    boot_by: tokio::time::Instant,
    accepted: tokio::time::Instant,
) -> tokio::time::Instant {
    boot_by.min(accepted + super::guest::GUEST_READY_BUDGET)
}

/// Returns the shutdown observation for a stop retry and whether the retry
/// must terminate a Job that is no longer empty.
fn demote_on_live_job(
    (guest_shutdown, clean_process_exit): (bool, bool),
    job_empty: bool,
) -> ((bool, bool), bool) {
    if job_empty {
        ((guest_shutdown, clean_process_exit), false)
    } else {
        ((guest_shutdown, false), true)
    }
}

/// A stop retry reuses the cached graceful observation only while the Job
/// stays empty. A survivor demotes it to a dirty exit, and the demotion is
/// cached before termination is attempted, so a failed terminate cannot
/// restore the clean result on a later retry. `None` keeps the charge.
fn retry_shutdown_observation(
    cache: &mut Option<(bool, bool)>,
    job_empty: bool,
    terminate: impl FnOnce() -> io::Result<()>,
) -> Option<(bool, bool)> {
    let (observed, survivor) = demote_on_live_job((*cache)?, job_empty);
    *cache = Some(observed);
    if survivor && terminate().is_err() {
        return None;
    }
    Some(observed)
}

/// The worker owns custody and can only report evidence. The async caller
/// alone accepts it and decides release. A detached worker may finish I/O,
/// but a closed receiver suppresses any new callback or accepted proof.
async fn finish_release<T: Send + 'static>(
    observed: StopEvidence,
    deadline: tokio::time::Instant,
    reopen: impl FnOnce() -> io::Result<T> + Send + 'static,
    verify_release: impl FnOnce() -> bool + Send + 'static,
) -> StopDisposition {
    finish_release_held(observed, deadline, reopen, verify_release)
        .await
        .0
}

async fn finish_release_held<T: Send + 'static>(
    mut observed: StopEvidence,
    deadline: tokio::time::Instant,
    reopen: impl FnOnce() -> io::Result<T> + Send + 'static,
    verify_release: impl FnOnce() -> bool + Send + 'static,
) -> (StopDisposition, Option<T>) {
    use tokio::time::{Instant, sleep_until};
    if !observed.ready_to_verify_release() || Instant::now() >= deadline {
        return (StopDisposition::ChargedQuarantine, None);
    }
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let Ok(worker) = Worker::start("arx-vm-release", move || {
        if tx.is_closed() {
            return Ok(());
        }
        let guard = reopen()?;
        let verified = !tx.is_closed() && verify_release();
        let _ = tx.send(verified.then_some(guard));
        Ok(())
    }) else {
        return (StopDisposition::ChargedQuarantine, None);
    };
    let verified = tokio::select! {
        biased;
        _ = sleep_until(deadline) => {
            rx.close();
            worker.cancel_io();
            None
        }
        evidence = &mut rx => evidence.ok().flatten().filter(|_| Instant::now() < deadline),
    };
    observed.verify_release(|| verified.is_some());
    (observed.disposition(), verified)
}

impl Child {
    fn exit_code(&self) -> Result<u32, NativeRefusal> {
        let mut code = 0;
        // The caller has already observed a signaled held process.
        succeeded(unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) })
            .map_err(|_| NativeRefusal::JobUnverified)?;
        Ok(code)
    }
}

impl Child {
    /// Checks cancellation, the live held child and a fresh resource sample,
    /// then releases the suspended thread. Nothing runs between the resource
    /// check and `ResumeThread` except the held-child liveness recheck.
    #[cfg(test)]
    fn resume_checked(
        &mut self,
        cancel: &Shutdown,
        revalidate: &mut dyn FnMut() -> Result<std::time::Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        self.resume_checked_with_handoff(cancel, None, revalidate, || {})
    }

    /// Resume only while the exclusive effect cutoff is still ahead. The
    /// cutoff and cancellation are checked after revalidation, immediately
    /// before the thread is released, so a slow sample cannot let the child
    /// start late.
    fn resume_checked_by(
        &mut self,
        cancel: &Shutdown,
        cutoff: tokio::time::Instant,
        revalidate: &mut dyn FnMut() -> Result<std::time::Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        self.resume_checked_with_handoff(cancel, Some(cutoff), revalidate, || {})
    }

    fn resume_checked_with_handoff(
        &mut self,
        cancel: &Shutdown,
        cutoff: Option<tokio::time::Instant>,
        revalidate: &mut dyn FnMut() -> Result<std::time::Instant, NativeRefusal>,
        handoff: impl FnOnce(),
    ) -> Result<(), NativeRefusal> {
        if cancel.is_cancelled() {
            return Err(NativeRefusal::JobUnverified);
        }
        self.verify_live()
            .map_err(|_| NativeRefusal::JobUnverified)?;
        let captured = revalidate()?;
        fresh_observation(
            captured,
            std::time::Instant::now(),
            NativeRefusal::ReservesUnverified,
        )?;
        let fenced = || {
            cutoff.is_some_and(|cutoff| {
                cancel.is_cancelled() || tokio::time::Instant::now() >= cutoff
            })
        };
        if fenced() {
            return Err(NativeRefusal::JobUnverified);
        }
        handoff();
        // Release edge: the handoff may block, so cancellation and the cutoff
        // are checked again after the final Job verification. Nothing that can
        // wait sits between this check and ResumeThread.
        self.verify_live()
            .map_err(|_| NativeRefusal::JobUnverified)?;
        if fenced() {
            return Err(NativeRefusal::JobUnverified);
        }
        self.release_suspended()
            .map_err(|_| NativeRefusal::JobUnverified)
    }
}

fn launch_environment(loader: &LocalPath, scratch: &LocalPath) -> io::Result<Vec<u16>> {
    let mut root = [0u16; 260];
    // Use the system directory API instead of an inherited environment value.
    let size = unsafe { GetSystemWindowsDirectoryW(root.as_mut_ptr(), root.len() as u32) } as usize;
    if size == 0 || size >= root.len() {
        return Err(io::Error::other("unavailable system root"));
    }
    let root = String::from_utf16(&root[..size]).map_err(io::Error::other)?;
    let mut environment = Vec::new();
    for (key, value) in [
        ("PATH", format!("{};{root}\\System32", loader.as_str())),
        ("SystemRoot", root.clone()),
        ("TEMP", scratch.as_str().to_owned()),
        ("TMP", scratch.as_str().to_owned()),
        ("WINDIR", root),
    ] {
        environment.extend(format!("{key}={value}").encode_utf16());
        environment.push(0);
    }
    environment.push(0);
    Ok(environment)
}

fn allocated_under(root: &LocalPath, expected: u64, limit: usize) -> Result<u64, NativeRefusal> {
    let reason = NativeRefusal::DiskCeilingUnverified;
    let mut todo = vec![root.clone()];
    let mut count = 0;
    let mut total = 0u64;
    while let Some(directory) = todo.pop() {
        let dir = PathPin::open(&directory, true, false).map_err(|_| reason)?;
        if dir.identity.volume != expected {
            return Err(reason);
        }
        total = total.checked_add(dir.allocated).ok_or(reason)?;
        for entry in std::fs::read_dir(directory.as_str()).map_err(|_| reason)? {
            count += 1;
            if count > limit.min(16_384) {
                return Err(reason);
            }
            let entry = entry.map_err(|_| reason)?;
            let name = entry.file_name();
            let path = directory
                .join(name.to_str().ok_or(reason)?)
                .map_err(|_| reason)?;
            if entry.file_type().map_err(|_| reason)?.is_dir() {
                todo.push(path);
            } else {
                let leaf = PathPin::open(&path, false, true).map_err(|_| reason)?;
                if leaf.identity.volume != expected {
                    return Err(reason);
                }
                total = total.checked_add(leaf.allocated).ok_or(reason)?;
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod monitor_tests;

#[cfg(test)]
fn open_test_epoch() -> (
    crate::vm::selection_epoch::SelectionEpoch,
    crate::vm::selection_epoch::Sample,
) {
    use crate::vm::{
        model::ClockReading,
        qualification_record::Instant2,
        selection_epoch::{Sample, SelectionEpoch},
    };
    let now = std::time::Instant::now();
    let sample = Sample {
        captured: now,
        now: Instant2 {
            utc: 1_800_000_000,
            mono_ms: 3_600_000,
        },
        boot_marker: [7; 32],
        reading: ClockReading {
            utc_ms: 1_800_000_000_000,
            monotonic: now,
        },
    };
    let epoch = SelectionEpoch::open([9; 16], sample, now).unwrap();
    (epoch, sample)
}

#[cfg(test)]
fn unused_drive_root() -> String {
    let mask = unsafe { GetLogicalDrives() };
    let letter = (0u32..26)
        .rev()
        .find(|bit| (mask & (1u32 << bit)) == 0)
        .map(|bit| (b'A' + bit as u8) as char)
        .expect("at least one unused Windows drive letter");
    format!(r"{letter}:\")
}

#[cfg(test)]
fn assert_admission_closed() {
    use crate::vm::runtime::{CandidateRuntime, NativeCapability, NativeMutation};
    let profile = crate::config::VmRuntimeProfile::parse(
        &serde_json::to_vec(&serde_json::json!({
            "enabled": true, "owner_sid": "S-1-5-21-1-2-3-1001",
            "supervisor_sid": "S-1-5-21-1-2-3-1002", "bundle": "E:/pinned bundle",
            "volume": "D:/", "state": "D:/test_vms/state",
            "disk_ceiling_bytes": 10_737_418_240_u64, "manifest": "qemu-11.1.3-windows-x64",
        }))
        .unwrap(),
    )
    .unwrap();
    let runtime = CandidateRuntime::new(Some(&profile));
    assert!(matches!(
        runtime.capability(),
        NativeCapability::Unavailable(_)
    ));
    assert!(runtime.dispatch(NativeMutation::Boot, |_| ()).is_err());
}

#[cfg(test)]
mod tests {
    use super::*;

    mod qmp_stdio_fixture;

    #[test]
    fn installed_boot_phases_clamp_to_their_own_budgets_and_the_start_cutoff() {
        let now = tokio::time::Instant::now();
        let tick = Duration::from_nanos(1);
        let setup = Duration::from_secs(60);
        let ready = crate::vm::guest::GUEST_READY_BUDGET;
        assert_eq!(ready, Duration::from_secs(180));
        // A distant start cutoff leaves each phase its own budget.
        let distant = now + Duration::from_secs(360);
        assert_eq!(setup_cutoff(distant, now), now + setup);
        assert_eq!(guest_ready_cutoff(distant, now), now + ready);
        // One tick either side of each nested budget.
        for (cutoff, budget) in [
            (setup_cutoff as fn(_, _) -> _, setup),
            (guest_ready_cutoff as fn(_, _) -> _, ready),
        ] {
            assert_eq!(cutoff(now + budget - tick, now), now + budget - tick);
            assert_eq!(cutoff(now + budget, now), now + budget);
            assert_eq!(cutoff(now + budget + tick, now), now + budget);
        }
        // A late accepted `cont` cannot extend readiness past the start cutoff.
        let accepted = distant - Duration::from_secs(10);
        assert_eq!(guest_ready_cutoff(distant, accepted), distant);
    }

    #[tokio::test]
    async fn guest_ready_cutoff_counts_the_terminal_sample_after_accepted_cont() {
        let accepted = std::cell::Cell::new(None);
        let samples = std::cell::Cell::new(0_u32);
        let now = tokio::time::Instant::now();
        let (setup_by, boot_by) = (
            now + Duration::from_secs(60),
            now + Duration::from_secs(360),
        );
        let lag = Duration::from_millis(200);
        let ((), ready_by) = monitor_setup(
            async {
                accepted.set(Some(tokio::time::Instant::now()));
                Ok::<_, NativeRefusal>(())
            },
            || {
                samples.set(samples.get() + 1);
                // The terminal sample after the accepted `cont` is slow.
                if accepted.get().is_some() {
                    std::thread::sleep(lag);
                }
                Ok(())
            },
            &Shutdown::default(),
            setup_by,
            boot_by,
        )
        .await
        .unwrap();
        let accepted = accepted.get().unwrap();
        let budget = crate::vm::guest::GUEST_READY_BUDGET;
        assert!(samples.get() >= 2, "the terminal sample did not run");
        assert!(ready_by >= accepted + budget);
        assert!(
            ready_by < accepted + budget + lag,
            "the guest-ready phase started after the terminal sample"
        );
    }

    #[test]
    fn stop_retry_demotes_a_graceful_exit_while_the_job_still_has_a_process() {
        for guest in [false, true] {
            for clean in [false, true] {
                assert_eq!(
                    demote_on_live_job((guest, clean), true),
                    ((guest, clean), false)
                );
                assert_eq!(
                    demote_on_live_job((guest, clean), false),
                    ((guest, false), true)
                );
            }
        }
    }

    #[tokio::test]
    async fn failed_terminate_keeps_the_demoted_observation_for_the_exact_release() {
        // A cached graceful exit meets a live Job whose termination fails.
        let mut cache = Some((true, true));
        let terminated = std::cell::Cell::new(0);
        let failed = retry_shutdown_observation(&mut cache, false, || {
            terminated.set(terminated.get() + 1);
            Err(io::Error::other("fixture terminate failure"))
        });
        assert_eq!(failed, None);
        assert_eq!(terminated.get(), 1);
        assert_eq!(cache, Some((true, false)), "the dirty downgrade was lost");
        // The next retry finds an empty Job and must not reuse the clean exit.
        let observed = retry_shutdown_observation(&mut cache, true, || {
            panic!("an empty Job needs no termination")
        })
        .unwrap();
        assert_eq!(observed, (true, false));
        // The exact proof: exited child, empty Job, joined workers and
        // reacquired writer exclusions. Its disposition stays dirty.
        let (disposition, held) = finish_release_held(
            StopEvidence {
                guest_shutdown: observed.0,
                clean_process_exit: observed.1,
                process_exited: true,
                job_empty: true,
                workers_joined: true,
                ..StopEvidence::default()
            },
            tokio::time::Instant::now() + Duration::from_secs(2),
            || Ok(()),
            || true,
        )
        .await;
        assert_eq!(disposition, StopDisposition::DirtyRelease);
        assert!(held.is_some());
    }

    #[test]
    fn paused_launch_attributes_and_single_process_job_are_applied() {
        let mut attributes = Attributes::new(4).unwrap();
        attributes.insert_paused_policy().unwrap();
        assert_eq!(&*attributes.values[0], &[0x1110_0000_0000_0000]);
        assert_eq!(&*attributes.values[1], &[1]);
        let job = Job::with_process_limit(2 << 30, None, Some(1)).unwrap();
        let mut actual = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        job.query(JobObjectExtendedLimitInformation, &mut actual)
            .unwrap();
        assert_eq!(actual.BasicLimitInformation.ActiveProcessLimit, 1);
        assert_eq!(
            actual.BasicLimitInformation.LimitFlags,
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_JOB_MEMORY
        );
    }

    use std::{
        io::{Read, Write},
        os::windows::fs::MetadataExt,
    };

    /// A synthetic native stdout: one partial QMP record, then the end.
    struct ScriptedPipe(std::collections::VecDeque<io::Result<&'static [u8]>>);

    impl Read for ScriptedPipe {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.pop_front() {
                Some(Ok(bytes)) => {
                    buf[..bytes.len()].copy_from_slice(bytes);
                    Ok(bytes.len())
                }
                Some(Err(error)) => Err(error),
                None => Ok(0),
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_stdout_read_failure_reaches_qmp_as_io_and_peer_close_as_eof() {
        use crate::vm::qmp::{ChannelError, Qmp};
        use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;

        for (end, expected) in [
            (
                Some(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32)),
                ChannelError::Io,
            ),
            // A closed peer end is the ordinary end of the child's output.
            (Some(io::ErrorKind::BrokenPipe.into()), ChannelError::Eof),
            (None, ChannelError::Eof),
        ] {
            let failed = end
                .as_ref()
                .is_some_and(|error| error.kind() != io::ErrorKind::BrokenPipe);
            let mut script = std::collections::VecDeque::from([Ok(&b"{\"QMP\":"[..])]);
            script.extend(end.map(Err));
            let cancel = Shutdown::default();
            let (output, worker) = start_output(
                ScriptedPipe(script),
                cancel.clone(),
                tokio::runtime::Handle::current(),
            )
            .unwrap();
            let refused = tokio::time::timeout(
                Duration::from_secs(2),
                Qmp::handshake(output, tokio::io::sink(), &Shutdown::default()),
            )
            .await
            .unwrap()
            .err();
            assert_eq!(refused, Some(expected));
            let joined = tokio::task::spawn_blocking(move || worker.0.join().unwrap())
                .await
                .unwrap();
            assert_eq!(joined.is_err(), failed);
            if failed {
                assert_eq!(
                    joined.unwrap_err().raw_os_error(),
                    Some(ERROR_ACCESS_DENIED as i32)
                );
            }
        }
    }
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    };

    const FIXTURE: &str = "vm::windows::tests::fixture_child";
    const SLEEPER: &str = "vm::windows::tests::fixture_sleeper";
    const SUPERVISOR: &str = "vm::windows::tests::fixture_supervisor";
    const NONCE: &str = "ARX_VM_SUPERVISOR_NONCE";
    const UNSUPPORTED: &str =
        "VM Job fixture unsupported: outer runner Job rejected creation-time assignment";

    /// A disposable NTFS directory under its final long spelling, so neither
    /// an 8.3 TEMP alias nor a junctioned TEMP ancestor decides a result.
    fn fixture_root() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(dir.path()).unwrap();
        let text = canonical.to_str().unwrap();
        let text = text.strip_prefix(r"\\?\").unwrap_or(text).to_owned();
        (dir, text)
    }

    fn fixture_stdio() -> (Stdio, Stdio) {
        let mut descriptor = null_mut();
        // Test-only descriptor gives the current creator/owner and SYSTEM access.
        succeeded(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide("D:P(D;;GA;;;NU)(A;;GA;;;OW)(A;;GA;;;SY)").as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        })
        .unwrap();
        let attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let result = Stdio::private(&attrs);
        // The pipe endpoints have been created and the descriptor is no longer borrowed.
        unsafe { LocalFree(descriptor) };
        result.unwrap()
    }

    fn fixture_environment(probe: usize) -> Vec<u16> {
        let mut env = launch_environment(
            &LocalPath::parse("D:/fixture").unwrap(),
            &LocalPath::parse("D:/fixture").unwrap(),
        )
        .unwrap();
        let mut marker = wide(format!("ARX_VM_JOB_FIXTURE={probe}"));
        marker.append(&mut env);
        marker
    }

    pub(super) fn fixture_launch(probe: usize) -> Option<(Child, Stdio)> {
        fixture_spawn(FIXTURE, probe)
    }

    fn fixture_spawn(fixture: &str, probe: usize) -> Option<(Child, Stdio)> {
        let (child_io, parent_io) = fixture_stdio();
        let job = Job::new(256 * 1024 * 1024, None).unwrap();
        let image = std::env::current_exe().unwrap();
        let args = ["--exact", fixture, "--ignored", "--nocapture"].map(str::to_owned);
        let launch = Launch::Fixture {
            image: &image,
            args: &args,
        };
        let cwd = image.parent().unwrap();
        let environment = fixture_environment(probe);
        match spawn(launch, cwd, &environment, job, child_io) {
            Ok(child) => {
                child.verify_live().unwrap();
                Some((child, parent_io))
            }
            Err((
                SpawnError {
                    stage: SpawnStage::Create,
                    error,
                    ..
                },
                _,
            )) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                classify_job_refusal(&image, &args, cwd, &environment);
                None
            }
            Err((SpawnError { stage, error, .. }, _)) => {
                panic!("fixture launch failed at {stage:?}: {error}")
            }
        }
    }

    /// Only an outer runner Job that refuses the creation-time Job-list
    /// attribute is an unsupported case. The control repeats the identical
    /// creation without that one attribute; any other denial is a failure.
    fn classify_job_refusal(image: &Path, args: &[String], cwd: &Path, environment: &[u16]) {
        let mut outer = 0;
        // Query only the current process's own Job membership.
        succeeded(unsafe { IsProcessInJob(GetCurrentProcess(), null_mut(), &mut outer) }).unwrap();
        assert_ne!(outer, 0, "access denied without an outer runner Job");
        let (child_io, _parent_io) = fixture_stdio();
        let control = Launch::Fixture { image, args };
        let (process, _thread, _desktop) = create_suspended(
            &control,
            cwd,
            environment,
            Assignment::ControlWithoutJob,
            &child_io,
        )
        .unwrap_or_else(|failure| {
            panic!("access denied is not specific to the Job list: {failure:?}")
        });
        // The suspended control never runs an instruction; discard it.
        succeeded(unsafe { TerminateProcess(process.as_raw_handle(), 1) }).unwrap();
        assert!(wait(&process, 2000).unwrap());
        skip_unsupported("vm-job-fixture");
    }

    /// A local skip stays visible on stdout and stderr. Hosted CI, which sets
    /// `CI=true`, must take the supported path, so a skip there fails.
    fn skip_unsupported(context: &str) {
        let line = format!("SKIP {context}: {UNSUPPORTED}");
        println!("{line}");
        eprintln!("{line}");
        assert!(
            std::env::var_os("CI").is_none_or(|value| value != "true"),
            "{line}; CI=true requires the supported Job path"
        );
    }

    #[test]
    #[ignore = "child-only fixture; spawned with a Job and explicit stdio"]
    fn fixture_child() {
        let probe: usize = std::env::var("ARX_VM_JOB_FIXTURE")
            .unwrap()
            .parse()
            .unwrap();
        if probe != 0 {
            // Report an inheritance failure via the actual parent-owned event.
            unsafe { SetEvent(probe as HANDLE) };
        }
        std::io::stdout().write_all(b"ARX_JOB_READY\n").unwrap();
        std::io::stdout().flush().unwrap();
        std::io::stderr().write_all(b"ARX_JOB_STDERR\n").unwrap();
        let mut marker = [0];
        std::io::stdin().read_exact(&mut marker).unwrap();
        assert_eq!(marker, [42]);
    }

    #[test]
    #[ignore = "child-only listener fixture; spawned with a Job and explicit stdio"]
    fn fixture_listener() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        std::io::stdout().write_all(b"ARX_JOB_READY\n").unwrap();
        // Emit two binary port bytes after the readiness marker.
        std::io::stdout()
            .write_all(&listener.local_addr().unwrap().port().to_le_bytes())
            .unwrap();
        std::io::stdout().flush().unwrap();
        let mut exit = [0];
        std::io::stdin().read_exact(&mut exit).unwrap();
        assert_eq!(exit, [42]);
        drop(listener);
    }

    #[tokio::test]
    async fn docker_trial_listener_belongs_to_the_held_fixture_child() {
        use qualification::docker_trial::witness::listener;
        use std::net::{Ipv4Addr, TcpListener};

        let foreign = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let Some((mut child, stdio)) = fixture_spawn("vm::windows::tests::fixture_listener", 0)
        else {
            return;
        };
        let pid = child.verify_live().unwrap();
        let (mut read, mut write, workers) = start_stdio(stdio, Default::default()).unwrap();
        child.resume().unwrap();
        ready(&mut read).await;
        let port = tokio::time::timeout(Duration::from_secs(2), read.read_u16_le())
            .await
            .unwrap()
            .unwrap();
        assert!(listener(&child, port).unwrap().check(port, pid).is_ok());
        assert!(listener(&child, foreign.local_addr().unwrap().port()).is_err());
        assert!(listener(&child, 0).is_err());
        write.write_all(&[42]).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        child.reap(deadline).await.unwrap();
        assert_eq!(child.exit_code(), Ok(0));
        assert!(listener(&child, port).is_err());
        workers.join(deadline).await.unwrap();
    }

    async fn ready(reader: &mut PipeOutput) {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut observed = Vec::new();
            while observed.len() < 4096 {
                observed.push(reader.read_u8().await.unwrap());
                if observed.ends_with(b"ARX_JOB_READY\n")
                    || observed.ends_with(b"ARX_JOB_READY\r\n")
                {
                    return;
                }
            }
            panic!("fixture readiness marker absent");
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn creation_time_job_holds_child_and_excludes_other_inheritable_handles() {
        let attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        // A separate inheritable event is deliberately absent from the handle list.
        let probe = unsafe { owned(CreateEventW(&attrs, 1, 0, null())) }.unwrap();
        let Some((mut child, stdio)) = fixture_launch(probe.as_raw_handle() as usize) else {
            return;
        };
        let pid = child.verify_live().unwrap();
        assert_eq!(child.job.active().unwrap(), 1);
        let (mut read, mut write, workers) = start_stdio(stdio, Default::default()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), read.read_u8())
                .await
                .is_err()
        );
        child.resume().unwrap();
        ready(&mut read).await;
        assert_eq!(child.verify_live().unwrap(), pid);
        assert!(!wait(&probe, 0).unwrap());
        write.write_all(&[42]).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        child.reap(deadline).await.unwrap();
        assert_eq!(child.exit_code(), Ok(0));
        workers.join(deadline).await.unwrap();
    }

    #[tokio::test]
    async fn losing_the_sole_job_handle_reaps_the_owned_process() {
        let Some((mut child, stdio)) = fixture_launch(0) else {
            return;
        };
        let process = child.process.try_clone().unwrap();
        let (mut read, _write, workers) = start_stdio(stdio, Default::default()).unwrap();
        child.resume().unwrap();
        ready(&mut read).await;
        assert!(!wait(&process, 0).unwrap());
        drop(child);
        assert!(wait(&process, 2000).unwrap());
        workers
            .join(tokio::time::Instant::now() + Duration::from_secs(2))
            .await
            .unwrap();
    }

    #[test]
    fn custody_rejects_hard_links_and_retains_the_path_binding() {
        let (_dir, root) = fixture_root();
        let target = format!(r"{root}\owned.img");
        let policy = LocalPath::parse(&target).unwrap();
        let pin = PathPin::create(&policy).unwrap();
        assert!(PathPin::create(&policy).is_err());
        assert!(std::fs::rename(&target, format!(r"{root}\replacement.img")).is_err());
        drop(pin);
        std::fs::hard_link(&target, format!(r"{root}\alias.img")).unwrap();
        assert!(matches!(
            PathPin::open(&policy, false, true),
            Err(NativeRefusal::PathCustodyUnverified)
        ));
    }

    #[test]
    fn custody_pins_the_drive_letter_to_its_volume_root() {
        let (_dir, root) = fixture_root();
        let drive = &root[..3];
        let pinned = file_identity(&open_directory(drive).unwrap(), true)
            .unwrap()
            .0;
        verify_volume_root(drive, pinned).unwrap();
        // A folder on the same volume is not that volume's root object.
        let folder = file_identity(&open_directory(&root).unwrap(), true)
            .unwrap()
            .0;
        assert_eq!(folder.volume, pinned.volume);
        assert!(verify_volume_root(drive, folder).is_err());
        // A folder spelling is not a volume mount point at all.
        assert!(verify_volume_root(&format!(r"{root}\"), pinned).is_err());
    }

    /// Unprivileged directory junction made with FSCTL_SET_REPARSE_POINT.
    fn fixture_junction(path: &str, target: &str) {
        std::fs::create_dir(path).unwrap();
        set_junction(path, target, GENERIC_WRITE).unwrap();
    }

    /// Tries to make an existing directory a mount-point junction through a
    /// handle with the given access.
    fn set_junction(path: &str, target: &str, access: u32) -> io::Result<()> {
        // A fresh directory handle that addresses the reparse point itself.
        let handle = unsafe {
            owned(CreateFileW(
                wide(path).as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            ))
        }?;
        let substitute = wide(format!(r"\??\{target}"));
        let print = wide(target);
        let mut data = Vec::new();
        data.extend_from_slice(&0xA000_0003_u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
        let names = u16::try_from((substitute.len() + print.len()) * 2).unwrap();
        data.extend_from_slice(&(8 + names).to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        for value in [
            0,
            (substitute.len() - 1) * 2,
            substitute.len() * 2,
            (print.len() - 1) * 2,
        ] {
            data.extend_from_slice(&u16::try_from(value).unwrap().to_le_bytes());
        }
        for unit in substitute.into_iter().chain(print) {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        let mut returned = 0;
        // The input buffer is a complete mount-point reparse record.
        succeeded(unsafe {
            windows_sys::Win32::System::IO::DeviceIoControl(
                handle.as_raw_handle(),
                0x0009_00A4, // FSCTL_SET_REPARSE_POINT
                data.as_ptr().cast(),
                u32::try_from(data.len()).unwrap(),
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            )
        })
    }

    /// Identity of an object by name, for comparing what a walk pinned.
    fn identity_of(path: &str, directory: bool) -> FileIdentity {
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .unwrap();
        file_identity(&file, directory).unwrap().0
    }

    #[test]
    fn relative_opens_follow_the_held_parent_object_not_its_name() {
        let (_dir, root) = fixture_root();
        let original = format!(r"{root}\original");
        let elsewhere = format!(r"{root}\elsewhere");
        for dir in [&original, &elsewhere] {
            std::fs::create_dir(dir).unwrap();
            std::fs::write(format!(r"{dir}\leaf.img"), dir.as_bytes()).unwrap();
        }
        let expected = identity_of(&format!(r"{original}\leaf.img"), false);
        assert_ne!(
            identity_of(&format!(r"{elsewhere}\leaf.img"), false),
            expected
        );
        // Unlike a pin, this parent handle permits its name to change.
        let parent = OpenOptions::new()
            .access_mode(FILE_GENERIC_READ)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&original)
            .unwrap();
        // Move the checked parent away and put a junction at its old name.
        std::fs::rename(&original, format!(r"{root}\moved")).unwrap();
        fixture_junction(&original, &elsewhere);
        let leaf = open_relative(&parent, "leaf.img", Node::File, true, false).unwrap();
        assert_eq!(file_identity(&leaf, false).unwrap().0, expected);
        // A multi-component name cannot leave the held parent.
        assert!(open_relative(&parent, r"x\leaf.img", Node::File, true, false).is_err());
        drop((leaf, parent));
        std::fs::remove_dir(&original).unwrap();
    }

    #[test]
    fn custody_walk_refuses_a_component_swapped_for_a_junction_mid_walk() {
        let (_dir, root) = fixture_root();
        let a = format!(r"{root}\a");
        let b = format!(r"{a}\b");
        let elsewhere = format!(r"{root}\elsewhere");
        for dir in [&a, &b, &elsewhere] {
            std::fs::create_dir(dir).unwrap();
        }
        std::fs::write(format!(r"{b}\leaf.img"), b"b").unwrap();
        std::fs::write(format!(r"{elsewhere}\leaf.img"), b"e").unwrap();
        let path = LocalPath::parse(&format!(r"{b}\leaf.img")).unwrap();
        // Positive control: an unraced walk pins the real leaf.
        assert!(PathPin::pin_observed(&path, false, true, false, &mut |_| {}).is_ok());
        let mut swapped = false;
        let result = PathPin::pin_observed(&path, false, true, false, &mut |pinned| {
            if pinned == a {
                // `a` is pinned and `b` is not yet opened.
                std::fs::remove_file(format!(r"{b}\leaf.img")).unwrap();
                std::fs::remove_dir(&b).unwrap();
                fixture_junction(&b, &elsewhere);
                swapped = true;
            }
        });
        assert!(swapped, "the race hook did not run");
        assert!(
            result.is_err(),
            "the walk followed a junction swapped in after its parent was pinned"
        );
        std::fs::remove_dir(&b).unwrap();
    }

    #[test]
    fn pinned_ancestors_cannot_be_renamed_or_made_junctions_during_the_walk() {
        let (_dir, root) = fixture_root();
        let a = format!(r"{root}\a");
        let b = format!(r"{a}\b");
        let elsewhere = format!(r"{root}\elsewhere");
        for dir in [&a, &b, &elsewhere] {
            std::fs::create_dir(dir).unwrap();
        }
        std::fs::write(format!(r"{b}\leaf.img"), b"b").unwrap();
        let expected = identity_of(&format!(r"{b}\leaf.img"), false);
        let path = LocalPath::parse(&format!(r"{b}\leaf.img")).unwrap();
        let mut attempts = Vec::new();
        let pin = PathPin::pin_observed(&path, false, true, false, &mut |pinned| {
            if pinned == b {
                // Both checked ancestors are held. Try to redirect each one.
                attempts.push(("rename a", std::fs::rename(&a, format!(r"{root}\a2")).err()));
                attempts.push(("rename b", std::fs::rename(&b, format!(r"{a}\b2")).err()));
                attempts.push((
                    "junction a (write)",
                    set_junction(&a, &elsewhere, GENERIC_WRITE).err(),
                ));
                attempts.push((
                    "junction a (attributes only)",
                    set_junction(&a, &elsewhere, FILE_WRITE_ATTRIBUTES).err(),
                ));
            }
        })
        .unwrap();
        assert_eq!(attempts.len(), 4, "the race hook did not run");
        for (attempt, error) in &attempts {
            assert!(
                error.is_some(),
                "{attempt} succeeded against a pinned ancestor"
            );
        }
        assert_eq!(pin.identity, expected);
        drop(pin);
        // Still plain directories: identity_of refuses a reparse point.
        identity_of(&a, true);
        identity_of(&b, true);
    }

    #[test]
    fn custody_refuses_junction_components_and_leaves() {
        let (_dir, root) = fixture_root();
        let real = format!(r"{root}\real");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(format!(r"{real}\leaf.img"), b"x").unwrap();
        let middle = format!(r"{root}\middle");
        let leaf = format!(r"{root}\leaf");
        fixture_junction(&middle, &real);
        fixture_junction(&leaf, &real);
        let path = |raw: &str| LocalPath::parse(raw).unwrap();
        let refused = |result: Result<PathPin, NativeRefusal>| {
            matches!(result, Err(NativeRefusal::PathCustodyUnverified))
        };
        // Positive controls: the same objects pin under their real spelling.
        assert!(PathPin::open(&path(&real), true, false).is_ok());
        assert!(PathPin::open(&path(&format!(r"{real}\leaf.img")), false, true).is_ok());
        assert!(PathPin::create(&path(&format!(r"{real}\control.img"))).is_ok());
        // A junction as an intermediate component.
        let through = path(&format!(r"{middle}\leaf.img"));
        assert!(refused(PathPin::open(&through, false, true)));
        assert!(refused(PathPin::open(&through, false, false)));
        assert!(refused(PathPin::create(&path(&format!(
            r"{middle}\new.img"
        )))));
        assert!(
            !Path::new(&format!(r"{real}\new.img")).exists(),
            "creation followed the junction"
        );
        // A junction as the leaf, as a directory, a file, or a creation target.
        assert!(refused(PathPin::open(&path(&leaf), true, false)));
        assert!(refused(PathPin::open(&path(&leaf), false, true)));
        assert!(refused(PathPin::create(&path(&leaf))));
        for junction in [&middle, &leaf] {
            let kind = std::fs::symlink_metadata(junction).unwrap();
            assert!(
                kind.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0,
                "junction fixture was replaced"
            );
            std::fs::remove_dir(junction).unwrap();
        }
        assert!(Path::new(&format!(r"{real}\leaf.img")).exists());
    }

    struct Descriptor(PSECURITY_DESCRIPTOR);

    impl Descriptor {
        fn parse(sddl: &str) -> Self {
            let mut raw = null_mut();
            // The SDDL string is NUL-terminated and the result uses LocalFree.
            succeeded(unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide(sddl).as_ptr(),
                    1,
                    &mut raw,
                    null_mut(),
                )
            })
            .unwrap();
            Self(raw)
        }

        fn dacl(&self) -> *const ACL {
            let (mut present, mut defaulted, mut dacl) = (0, 0, null_mut());
            // The ACL view remains backed by this descriptor allocation.
            succeeded(unsafe {
                GetSecurityDescriptorDacl(self.0, &mut present, &mut dacl, &mut defaulted)
            })
            .unwrap();
            dacl
        }
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            // Allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe { LocalFree(self.0) };
        }
    }

    /// The current TokenUser allocation and its SDDL string form.
    pub(super) fn current_user() -> (Vec<usize>, String) {
        // The pseudo-handle needs no close; the token handle is owned.
        let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
        let data = token_data(&token, TokenUser).unwrap();
        let mut text = null_mut();
        // The SID stays inside `data`; the string is LocalFree'd after copying.
        let string = unsafe {
            let sid = (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            succeeded(ConvertSidToStringSidW(sid, &mut text)).unwrap();
            let len = (0..).take_while(|&at| *text.add(at) != 0).count();
            let string = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
            LocalFree(text.cast());
            string
        };
        (data, string)
    }

    fn create_with(path: &str, sddl: &str) -> File {
        let descriptor = Descriptor::parse(sddl);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        // The descriptor outlives this synchronous exclusive creation.
        let handle = unsafe {
            owned(CreateFileW(
                wide(path).as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                0,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            ))
        }
        .unwrap();
        File::from(handle)
    }

    #[test]
    fn vm_acl_requires_the_exact_owner_and_ordered_explicit_dacl() {
        let (_dir, root) = fixture_root();
        let (user_data, user) = current_user();
        // `user_data` backs this SID for the rest of the test.
        let me = unsafe { (*user_data.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let network = LocalSid::parse("S-1-5-2").unwrap();
        let system = LocalSid::parse("S-1-5-18").unwrap();
        let entry = |deny, sid, mask| vm_acl::Entry { deny, sid, mask };
        let expected = [
            entry(true, network.0, FILE_ALL_ACCESS),
            entry(false, me, FILE_ALL_ACCESS),
            entry(false, system.0, FILE_GENERIC_READ),
        ];
        let exact_sddl = format!("O:{user}D:P(D;;FA;;;NU)(A;;FA;;;{user})(A;;0x120089;;;SY)");
        let file = create_with(&format!(r"{root}\exact.bin"), &exact_sddl);
        // Every SID pointer is backed by a value that outlives the check.
        let check = |file: &File, owner: PSID, entries: &[vm_acl::Entry]| unsafe {
            vm_acl::exact(file, owner, entries)
        };
        assert!(check(&file, me, &expected), "positive exact match");
        assert!(!check(&file, system.0, &expected), "wrong owner");
        let reordered = [
            entry(false, me, FILE_ALL_ACCESS),
            entry(true, network.0, FILE_ALL_ACCESS),
            entry(false, system.0, FILE_GENERIC_READ),
        ];
        assert!(!check(&file, me, &reordered), "reordered entries");
        let wider = [
            entry(true, network.0, FILE_ALL_ACCESS),
            entry(false, me, FILE_ALL_ACCESS),
            entry(false, system.0, FILE_ALL_ACCESS),
        ];
        assert!(!check(&file, me, &wider), "wrong mask");
        let allowed = [
            entry(false, network.0, FILE_ALL_ACCESS),
            entry(false, me, FILE_ALL_ACCESS),
            entry(false, system.0, FILE_GENERIC_READ),
        ];
        assert!(!check(&file, me, &allowed), "deny recorded as allow");
        assert!(!check(&file, me, &expected[..2]), "unexpected extra ACE");
        let extra = create_with(
            &format!(r"{root}\extra.bin"),
            &format!("{exact_sddl}(A;;0x120089;;;WD)"),
        );
        assert!(!check(&extra, me, &expected), "extra Everyone ACE");
        let open = create_with(
            &format!(r"{root}\null.bin"),
            &format!("O:{user}D:NO_ACCESS_CONTROL"),
        );
        assert!(!check(&open, me, &expected), "null DACL");
        assert!(!check(&open, me, &[]), "null DACL with no entries");
        // Inherited and inheritable flags are never an exact explicit match.
        let only_system = [entry(false, system.0, FILE_ALL_ACCESS)];
        for (sddl, accepted) in [
            ("D:(A;;FA;;;SY)", true),
            ("D:(A;ID;FA;;;SY)", false),
            ("D:(A;OI;FA;;;SY)", false),
            ("D:(A;CIIO;FA;;;SY)", false),
            ("D:(D;;FA;;;SY)", false),
        ] {
            let descriptor = Descriptor::parse(sddl);
            // The descriptor backs the ACL for this call.
            let matched = unsafe { vm_acl::matches(descriptor.dacl(), &only_system) };
            assert_eq!(matched, accepted, "{sddl}");
        }
        // A missing DACL never matches.
        assert!(!unsafe { vm_acl::matches(null(), &[]) });
    }

    #[test]
    #[ignore = "grandchild-only fixture for the supervisor-loss test"]
    fn fixture_sleeper() {
        if std::env::var_os("ARX_VM_JOB_FIXTURE").is_none() {
            return;
        }
        std::io::stdout().write_all(b"ARX_JOB_READY\n").unwrap();
        std::io::stdout().flush().unwrap();
        // Outlives every bounded observer wait unless its Job reaps it.
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    #[ignore = "disposable parent fixture for the supervisor-loss test"]
    fn fixture_supervisor() {
        let Ok(nonce) = std::env::var(NONCE) else {
            return;
        };
        assert!(nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let mut out = std::io::stdout();
        let Some((mut child, stdio)) = fixture_spawn(SLEEPER, 0) else {
            writeln!(out, "ARX_SUPERVISOR_UNSUPPORTED {nonce}").unwrap();
            out.flush().unwrap();
            return;
        };
        child.resume().unwrap();
        let mut pipe = File::from(stdio.output);
        let mut seen = Vec::new();
        let mut bytes = [0; 512];
        while !seen.windows(13).any(|window| window == b"ARX_JOB_READY") {
            let count = pipe.read(&mut bytes).unwrap();
            assert!(count > 0 && seen.len() < 4096, "sleeper readiness absent");
            seen.extend_from_slice(&bytes[..count]);
        }
        let pid = child.verify_live().unwrap();
        writeln!(out, "ARX_SUPERVISOR {pid} {nonce}").unwrap();
        out.flush().unwrap();
        // The observer terminates this process. Only this process holds the Job.
        std::thread::sleep(Duration::from_secs(30));
        child.job.terminate().unwrap();
    }

    struct KillOnDrop(std::process::Child);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn supervisor_loss_reaps_the_job_child_through_kill_on_close() {
        use std::{io::BufRead, process::Stdio as Pipe, sync::mpsc};

        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).unwrap();
        let nonce = hex::encode(nonce);
        // A plain disposable parent: this observer creates no Job for it.
        let mut parent = KillOnDrop(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", SUPERVISOR, "--ignored", "--nocapture"])
                .env(NONCE, &nonce)
                // The parent reports; this observer applies the CI policy.
                .env_remove("CI")
                .stdin(Pipe::null())
                .stdout(Pipe::piped())
                .stderr(Pipe::null())
                .spawn()
                .unwrap(),
        );
        let stdout = parent.0.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines().take(64) {
                let Ok(line) = line else { break };
                if let Some(at) = line.find("ARX_SUPERVISOR") {
                    let _ = send.send(line[at..].to_owned());
                    break;
                }
            }
        });
        let report = receive
            .recv_timeout(Duration::from_secs(10))
            .expect("supervisor fixture report");
        let fields: Vec<&str> = report.split(' ').collect();
        let pid: u32 = match fields.as_slice() {
            ["ARX_SUPERVISOR_UNSUPPORTED", seen] if *seen == nonce => {
                skip_unsupported("vm-supervisor-loss");
                return;
            }
            ["ARX_SUPERVISOR", pid, seen] if *seen == nonce => pid.parse().unwrap(),
            _ => panic!("unexpected supervisor report"),
        };
        // Only a wait right: no Job, process-control or thread authority.
        let grandchild = unsafe { owned(OpenProcess(SYNCHRONIZE, 0, pid)) }.unwrap();
        // The live parent holds the grandchild's handle, so the PID was not reused.
        assert!(
            parent.0.try_wait().unwrap().is_none(),
            "parent exited early"
        );
        assert!(!wait(&grandchild, 0).unwrap(), "grandchild exited early");
        parent.0.kill().unwrap();
        assert!(
            wait(&grandchild, 2000).unwrap(),
            "kill-on-close Job did not reap the grandchild after supervisor loss"
        );
        parent.0.wait().unwrap();
    }

    #[tokio::test]
    async fn wrong_pipe_server_is_refused_before_any_exchange_and_cancel_is_bounded() {
        let Some((child, _stdio)) = fixture_launch(0) else {
            return;
        };
        let (user_data, user) = current_user();
        // `user_data` backs this SID for the rest of the test.
        let me = unsafe { (*user_data.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let network = LocalSid::parse("S-1-5-2").unwrap();
        let system = LocalSid::parse("S-1-5-18").unwrap();
        let expected = [
            vm_acl::Entry {
                deny: true,
                sid: network.0,
                mask: FILE_ALL_ACCESS,
            },
            vm_acl::Entry {
                deny: false,
                sid: me,
                mask: FILE_ALL_ACCESS,
            },
            vm_acl::Entry {
                deny: false,
                sid: system.0,
                mask: FILE_ALL_ACCESS,
            },
        ];
        let descriptor = Descriptor::parse(&format!(
            "O:{user}D:P(D;;FA;;;NU)(A;;FA;;;{user})(A;;FA;;;SY)"
        ));
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let serial = SerialName::generate().unwrap();
        let name = wide(format!(r"\\.\pipe\{}", serial.basename()));
        // This test process, not the held child, serves the pipe.
        let server = unsafe {
            owned(CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                0,
                &attributes,
            ))
        }
        .unwrap();
        // The endpoint carries exactly the owner and DACL connect_serial is
        // told to expect, so the server PID is the only property that differs.
        assert!(
            unsafe { vm_acl::exact(&server, me, &expected) },
            "fixture pipe descriptor differs from the expected one"
        );
        assert_ne!(
            unsafe { GetCurrentProcessId() },
            child.verify_live().unwrap()
        );
        let cancel = Shutdown::default();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        let error = connect_serial(&serial, &child, me, &expected, &cancel, deadline)
            .await
            .unwrap_err();
        assert_eq!(error, NativeRefusal::GuestEndpointUnverified);
        cancel.cancel();
        let unknown = SerialName::generate().unwrap();
        assert!(
            connect_serial(&unknown, &child, me, &expected, &cancel, deadline)
                .await
                .is_err()
        );
        child.job.terminate().unwrap();
        child.reap(deadline).await.unwrap();
    }

    #[tokio::test]
    async fn resume_rechecks_resources_immediately_before_releasing_the_thread() {
        let Some((mut child, stdio)) = fixture_launch(0) else {
            return;
        };
        let (mut read, mut write, workers) = start_stdio(stdio, Default::default()).unwrap();
        let mut calls = 0;
        let fresh = Shutdown::default();
        let refused = child.resume_checked(&fresh, &mut || {
            calls += 1;
            Err(NativeRefusal::ReservesUnverified)
        });
        assert_eq!(refused, Err(NativeRefusal::ReservesUnverified));
        assert_eq!(calls, 1);
        assert!(child.suspended.is_some(), "thread released after a refusal");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), read.read_u8())
                .await
                .is_err(),
            "the refused child ran"
        );
        // Cancellation refuses before any resource sample is taken.
        let cancelled = Shutdown::default();
        cancelled.cancel();
        let refused = child.resume_checked(&cancelled, &mut || {
            calls += 1;
            Ok(std::time::Instant::now())
        });
        assert_eq!(refused, Err(NativeRefusal::JobUnverified));
        assert_eq!(calls, 1);
        assert!(child.suspended.is_some());
        child
            .resume_checked(&fresh, &mut || {
                calls += 1;
                Ok(std::time::Instant::now())
            })
            .unwrap();
        assert_eq!(calls, 2);
        ready(&mut read).await;
        write.write_all(&[42]).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        child.reap(deadline).await.unwrap();
        workers.join(deadline).await.unwrap();
    }

    #[tokio::test]
    async fn resume_refuses_once_the_effect_cutoff_or_cancellation_lands_during_revalidation() {
        let Some((mut child, stdio)) = fixture_launch(0) else {
            return;
        };
        let (mut read, mut write, workers) = start_stdio(stdio, Default::default()).unwrap();
        let fresh = Shutdown::default();
        // The cutoff is still ahead when resume starts and is reached while
        // the otherwise valid sample runs; the thread must stay suspended.
        let cutoff = tokio::time::Instant::now() + Duration::from_millis(20);
        let mut calls = 0;
        let refused = child.resume_checked_by(&fresh, cutoff, &mut || {
            calls += 1;
            std::thread::sleep(Duration::from_millis(40));
            Ok(std::time::Instant::now())
        });
        assert!(tokio::time::Instant::now() >= cutoff);
        assert_eq!(refused, Err(NativeRefusal::JobUnverified));
        assert_eq!(calls, 1);
        assert!(child.suspended.is_some(), "thread released at the cutoff");
        // Cancellation landing inside a valid sample also keeps it held.
        let cancel = Shutdown::default();
        let ahead = tokio::time::Instant::now() + Duration::from_secs(30);
        let refused = child.resume_checked_by(&cancel, ahead, &mut || {
            calls += 1;
            cancel.cancel();
            Ok(std::time::Instant::now())
        });
        assert_eq!(refused, Err(NativeRefusal::JobUnverified));
        assert_eq!(calls, 2);
        assert!(child.suspended.is_some(), "thread released after cancel");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), read.read_u8())
                .await
                .is_err(),
            "the held child ran"
        );
        // With the cutoff still ahead after the sample, the child runs.
        child
            .resume_checked_by(&fresh, ahead, &mut || {
                calls += 1;
                Ok(std::time::Instant::now())
            })
            .unwrap();
        assert_eq!(calls, 3);
        assert!(child.suspended.is_none());
        ready(&mut read).await;
        write.write_all(&[42]).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        child.reap(deadline).await.unwrap();
        workers.join(deadline).await.unwrap();
    }

    #[tokio::test]
    async fn resume_refuses_when_cancellation_or_the_cutoff_lands_during_the_handoff() {
        let Some((mut child, stdio)) = fixture_launch(0) else {
            return;
        };
        let (mut read, mut write, workers) = start_stdio(stdio, Default::default()).unwrap();
        let mut samples = 0;
        let mut handoffs = 0;
        // Cancellation arrives inside the handoff, after the post-sample guard
        // already passed.
        let cancel = Shutdown::default();
        let ahead = tokio::time::Instant::now() + Duration::from_secs(30);
        let refused = child.resume_checked_with_handoff(
            &cancel,
            Some(ahead),
            &mut || {
                samples += 1;
                Ok(std::time::Instant::now())
            },
            || {
                handoffs += 1;
                assert!(!cancel.is_cancelled());
                cancel.cancel();
            },
        );
        assert_eq!(refused, Err(NativeRefusal::JobUnverified));
        assert_eq!((samples, handoffs), (1, 1));
        assert!(child.suspended.is_some(), "thread released after cancel");
        // The cutoff is still ahead after the sample and is crossed while the
        // handoff blocks.
        let fresh = Shutdown::default();
        let cutoff = tokio::time::Instant::now() + Duration::from_millis(300);
        let refused = child.resume_checked_with_handoff(
            &fresh,
            Some(cutoff),
            &mut || {
                samples += 1;
                Ok(std::time::Instant::now())
            },
            || {
                handoffs += 1;
                let now = tokio::time::Instant::now();
                assert!(now < cutoff);
                std::thread::sleep((cutoff - now) + Duration::from_millis(5));
            },
        );
        assert!(tokio::time::Instant::now() >= cutoff);
        assert_eq!(refused, Err(NativeRefusal::JobUnverified));
        assert_eq!((samples, handoffs), (2, 2));
        assert!(child.suspended.is_some(), "thread released at the cutoff");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), read.read_u8())
                .await
                .is_err(),
            "the held child ran"
        );
        // With both still clear after the handoff, the child is released.
        child
            .resume_checked_with_handoff(
                &fresh,
                Some(tokio::time::Instant::now() + Duration::from_secs(30)),
                &mut || {
                    samples += 1;
                    Ok(std::time::Instant::now())
                },
                || handoffs += 1,
            )
            .unwrap();
        assert_eq!((samples, handoffs), (3, 3));
        assert!(child.suspended.is_none());
        ready(&mut read).await;
        write.write_all(&[42]).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        child.reap(deadline).await.unwrap();
        workers.join(deadline).await.unwrap();
    }

    #[test]
    fn attribute_values_outlive_the_list_on_a_creation_error() {
        let job = Job::new(64 * 1024 * 1024, None).unwrap();
        // The guard copies values, so a caller's temporary array may end first.
        let mut attributes = Attributes::new(2).unwrap();
        {
            let temporary = [job.0.as_raw_handle()];
            attributes
                .insert(PROC_THREAD_ATTRIBUTE_JOB_LIST as usize, &temporary)
                .unwrap();
            assert_ne!(
                attributes.values[0].as_ptr(),
                temporary.as_ptr().cast::<usize>()
            );
        }
        assert_eq!(&*attributes.values[0], &[job.0.as_raw_handle() as usize]);
        drop(attributes);
        // Failure injection: both attributes are set, then creation fails.
        let (child_io, _parent_io) = fixture_stdio();
        let image = std::env::current_exe().unwrap();
        let missing = image.with_file_name("arx-missing-fixture-image.exe");
        let args: [String; 0] = [];
        let launch = Launch::Fixture {
            image: &missing,
            args: &args,
        };
        for _ in 0..32 {
            let failure = create_suspended(
                &launch,
                image.parent().unwrap(),
                &fixture_environment(0),
                Assignment::Job(&job),
                &child_io,
            )
            .err()
            .expect("missing test image refused");
            assert_eq!(failure.stage, SpawnStage::Create);
            assert_eq!(
                failure.error.raw_os_error(),
                Some(ERROR_FILE_NOT_FOUND as i32)
            );
        }
        assert_eq!(job.active().unwrap(), 0);
    }

    const CONT: &[u8] = b"{\"execute\":\"cont\",\"id\":3}\n";
    /// Upper bound for waits in the dispatch tests. Ordering never depends on it.
    const BOUND: Duration = Duration::from_secs(10);

    /// Bytes waiting in the child's stdin pipe, without consuming them.
    fn queued(pipe: &OwnedHandle) -> u32 {
        let mut available = 0;
        // Peek only reports the count; a closed writer with no data is zero.
        let ok = unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        };
        if ok == 0 {
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(ERROR_BROKEN_PIPE as i32)
            );
            return 0;
        }
        available
    }

    struct Gated {
        child: Stdio,
        gate: std::sync::Arc<DispatchGate>,
        write: tokio::io::DuplexStream,
        workers: WorkerSet,
        _read: PipeOutput,
    }

    /// Real pipes and the production stdin worker, with an injected clock.
    fn gated(now: std::time::Instant) -> Gated {
        let (child, parent) = fixture_stdio();
        let gate = std::sync::Arc::new(DispatchGate::default());
        *gate.test.clock.lock().unwrap() = Some(now);
        let (_read, write, workers) = start_stdio(parent, gate.clone()).unwrap();
        Gated {
            child,
            gate,
            write,
            workers,
            _read,
        }
    }

    fn set_clock(gate: &DispatchGate, now: std::time::Instant) {
        *gate.test.clock.lock().unwrap() = Some(now);
    }

    /// Parks the worker after each tracked write until the test releases it.
    fn park_after_write(
        gate: &DispatchGate,
    ) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        let (reached, parked) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        *gate.test.checkpoint.lock().unwrap() = Some(Box::new(move |before_write| {
            if !before_write {
                reached.send(()).unwrap();
                released.recv_timeout(BOUND).unwrap();
            }
        }));
        (parked, release)
    }

    async fn state_becomes(gate: &DispatchGate, expected: Dispatch) {
        let mut watch = gate.state.subscribe();
        tokio::time::timeout(BOUND, watch.wait_for(|state| *state == expected))
            .await
            .expect("dispatch state did not settle")
            .unwrap();
    }

    async fn input_worker_exits(set: &WorkerSet) {
        tokio::time::timeout(BOUND, async {
            while !set.workers[1].0.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the input worker kept the child's stdin open");
    }

    /// Polls boot's settle future exactly once. `Pending` comes from the
    /// future itself after it registered as a waiter on the gate state.
    fn parks<F: std::future::Future>(future: std::pin::Pin<&mut F>) -> bool {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        future.poll(&mut context).is_pending()
    }

    #[test]
    fn a_stale_cont_sample_never_arms_the_gate() {
        let base = std::time::Instant::now();
        let gate = DispatchGate::default();
        set_clock(&gate, base + FRESHNESS + Duration::from_millis(1));
        assert_eq!(gate.arm(base), Err(NativeRefusal::ReservesUnverified));
        assert_eq!(gate.current(), Dispatch::Idle);
        set_clock(&gate, base);
        gate.arm(base).unwrap();
        // A second arm while a window is open is refused.
        assert_eq!(gate.arm(base), Err(NativeRefusal::ReservesUnverified));
    }

    #[test]
    fn installer_dispatch_cannot_write_cont_at_the_stage_deadline() {
        let base = std::time::Instant::now();
        let cutoff = base + Duration::from_millis(10);
        let gate = DispatchGate::default();
        set_clock(&gate, base);
        gate.arm_until(base, Some(cutoff)).unwrap();
        assert_eq!(gate.current(), Dispatch::Armed(cutoff));
        set_clock(&gate, cutoff);
        assert!(!gate.begin_write());
        assert_eq!(gate.current(), Dispatch::Expired);
    }

    #[test]
    fn a_worker_waiting_for_dispatch_rechecks_the_deadline_after_the_lock() {
        use std::sync::{Arc, mpsc};

        let now = std::time::Instant::now();
        let short_window = Duration::from_millis(10);
        for (captured, deadline, decision, allowed) in [
            (
                now - FRESHNESS + short_window,
                now + FRESHNESS,
                now + short_window - Duration::from_millis(1),
                true,
            ),
            (
                now - FRESHNESS + short_window,
                now + FRESHNESS,
                now + short_window,
                false,
            ),
            (
                now,
                now + Duration::from_millis(250),
                now + Duration::from_millis(250),
                false,
            ),
            (
                now,
                now + Duration::from_millis(20),
                now + Duration::from_millis(21),
                false,
            ),
        ] {
            let gate = Arc::new(DispatchGate::default());
            set_clock(&gate, now);
            gate.arm_until(captured, Some(deadline)).unwrap();
            let state = gate.state.subscribe();
            let held = state.borrow();
            let (started, waiting) = mpsc::channel();
            *gate.test.before_write_lock.lock().unwrap() = Some(started);
            let worker_gate = gate.clone();
            let worker = std::thread::spawn(move || worker_gate.begin_write());
            let attempted = waiting.recv_timeout(BOUND);
            set_clock(&gate, decision);
            drop(held);
            let permitted = worker.join().unwrap();
            attempted.unwrap();
            assert_eq!(permitted, allowed);
            assert_eq!(
                gate.current(),
                if allowed {
                    Dispatch::Writing(deadline.min(captured + FRESHNESS))
                } else {
                    Dispatch::Expired
                }
            );
        }
    }

    #[tokio::test]
    async fn a_worker_rechecks_at_the_pipe_after_taking_write_permission() {
        let now = std::time::Instant::now();
        let mut io = gated(now);
        let (reached, parked) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        *io.gate.test.checkpoint.lock().unwrap() = Some(Box::new(move |before_write| {
            if before_write {
                reached.send(()).unwrap();
                released.recv_timeout(BOUND).unwrap();
            }
        }));
        io.gate.arm(now).unwrap();
        io.write.write_all(CONT).await.unwrap();
        parked.recv_timeout(BOUND).unwrap();
        assert!(matches!(io.gate.current(), Dispatch::Writing(_)));
        set_clock(&io.gate, now + FRESHNESS);
        release.send(()).unwrap();
        state_becomes(&io.gate, Dispatch::Ambiguous).await;
        input_worker_exits(&io.workers).await;
        assert_eq!(queued(&io.child.input), 0);
        io.workers
            .join(tokio::time::Instant::now() + BOUND)
            .await
            .unwrap();
    }

    #[test]
    fn a_short_pipe_write_cannot_authorize_its_remaining_bytes_after_expiry() {
        struct ShortWrite<'a> {
            gate: &'a DispatchGate,
            deadline: std::time::Instant,
            written: Vec<u8>,
        }

        impl std::io::Write for ShortWrite<'_> {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.written.push(bytes[0]);
                set_clock(self.gate, self.deadline);
                Ok(1)
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let now = std::time::Instant::now();
        let deadline = now + FRESHNESS;
        let gate = DispatchGate::default();
        set_clock(&gate, now);
        gate.arm(now).unwrap();
        assert!(gate.begin_write());
        let mut pipe = ShortWrite {
            gate: &gate,
            deadline,
            written: Vec::new(),
        };
        assert!(
            !gate
                .write_pipe(&mut pipe, CONT, &Shutdown::default())
                .unwrap()
        );
        assert_eq!(pipe.written, CONT[..1]);
    }

    #[tokio::test]
    async fn an_expired_sample_writes_nothing_and_closes_stdin() {
        let base = std::time::Instant::now();
        let mut io = gated(base);
        io.gate.arm(base).unwrap();
        // The deadline passes before the worker reaches the pipe.
        set_clock(&io.gate, base + FRESHNESS);
        io.write.write_all(CONT).await.unwrap();
        state_becomes(&io.gate, Dispatch::Expired).await;
        input_worker_exits(&io.workers).await;
        assert_eq!(queued(&io.child.input), 0, "a `cont` reached the child");
        assert_eq!(
            io.gate.settle(BOUND).await,
            Err(NativeRefusal::ReservesUnverified)
        );
        io.workers
            .join(tokio::time::Instant::now() + BOUND)
            .await
            .unwrap();
    }

    /// The reviewer's interleaving: QEMU reads the bytes and acknowledges while
    /// the worker is still between its write and recording the result, and
    /// the deadline passes in that gap. Boot must wait, then refuse.
    #[tokio::test]
    async fn an_acknowledgement_before_a_late_write_is_recorded_is_refused() {
        let base = std::time::Instant::now();
        let mut io = gated(base);
        let (parked, release) = park_after_write(&io.gate);
        io.gate.arm(base).unwrap();
        io.write.write_all(CONT).await.unwrap();
        parked.recv_timeout(BOUND).unwrap();
        // The bytes are in the pipe; the worker has not recorded the write.
        assert_eq!(queued(&io.child.input) as usize, CONT.len());
        assert!(matches!(io.gate.current(), Dispatch::Writing(_)));
        // Boot observes the acknowledgement now.
        let gate = io.gate.clone();
        let mut boot = Box::pin(async move { gate.settle(BOUND).await });
        assert!(
            parks(boot.as_mut()),
            "boot decided before the worker finished its write"
        );
        set_clock(&io.gate, base + FRESHNESS);
        release.send(()).unwrap();
        assert_eq!(boot.await, Err(NativeRefusal::ReservesUnverified));
        assert_eq!(io.gate.current(), Dispatch::Ambiguous);
        input_worker_exits(&io.workers).await;
        io.workers
            .join(tokio::time::Instant::now() + BOUND)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_acknowledged_cont_written_in_time_is_accepted() {
        let base = std::time::Instant::now();
        let mut io = gated(base);
        let (parked, release) = park_after_write(&io.gate);
        io.gate.arm(base).unwrap();
        io.write.write_all(CONT).await.unwrap();
        parked.recv_timeout(BOUND).unwrap();
        let gate = io.gate.clone();
        let mut boot = Box::pin(async move { gate.settle(BOUND).await });
        assert!(
            parks(boot.as_mut()),
            "boot decided before the worker finished its write"
        );
        // Completed one tick before the deadline.
        set_clock(&io.gate, base + FRESHNESS - Duration::from_nanos(1));
        release.send(()).unwrap();
        assert_eq!(boot.await, Ok(()));
        assert_eq!(io.gate.current(), Dispatch::Idle);
        // After startup, later commands pass through untracked.
        *io.gate.test.checkpoint.lock().unwrap() = None;
        set_clock(&io.gate, base + FRESHNESS * 10);
        io.write.write_all(CONT).await.unwrap();
        tokio::time::timeout(BOUND, async {
            while (queued(&io.child.input) as usize) < 2 * CONT.len() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("an untracked command did not reach the child");
        assert!(!io.workers.workers[1].0.is_finished());
        io.workers
            .join(tokio::time::Instant::now() + BOUND)
            .await
            .unwrap();
    }

    #[test]
    fn an_existing_writable_leaf_pin_refuses_rename_and_delete() {
        let (_dir, root) = fixture_root();
        let disk = format!(r"{root}\disk.qcow2");
        let moved = format!(r"{root}\moved.qcow2");
        std::fs::write(&disk, b"existing").unwrap();
        let pin = PathPin::open(&LocalPath::parse(&disk).unwrap(), false, true).unwrap();
        assert!(std::fs::rename(&disk, &moved).is_err(), "rename succeeded");
        assert!(std::fs::remove_file(&disk).is_err(), "delete succeeded");
        // The link count is checked once at pin time; the held pin then
        // refuses a new alias (observed as a sharing violation).
        assert!(
            std::fs::hard_link(&disk, format!(r"{root}\alias.qcow2")).is_err(),
            "hard link added to a pinned leaf"
        );
        // The consumer can still open the pinned disk for read and write.
        let consumer = OpenOptions::new().read(true).write(true).open(&disk);
        assert!(consumer.is_ok(), "pin excludes its read/write consumer");
        drop((consumer, pin));
        // Control: without the pin the same rename succeeds.
        std::fs::rename(&disk, &moved).unwrap();
    }

    #[tokio::test]
    async fn installer_release_reopen_excludes_live_and_new_writers() {
        let (_dir, root) = fixture_root();
        let disk = format!(r"{root}\disk.qcow2");
        std::fs::write(&disk, b"candidate").unwrap();
        let pin = PathPin::open(&LocalPath::parse(&disk).unwrap(), false, true).unwrap();
        let writer = || OpenOptions::new().read(true).write(true).open(&disk);
        let live_writer = writer().unwrap();
        assert!(pin.reopen_without_writer().is_err());
        drop(live_writer);
        let guard = pin.reopen_without_writer().unwrap();
        assert!(writer().is_err());
        drop(guard);
        assert!(writer().is_ok());

        let observed = StopEvidence {
            guest_shutdown: true,
            clean_process_exit: true,
            process_exited: true,
            job_empty: true,
            workers_joined: true,
            ..StopEvidence::default()
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        assert_eq!(
            finish_release(
                observed,
                deadline,
                move || pin.reopen_without_writer(),
                move || OpenOptions::new().write(true).open(&disk).is_err(),
            )
            .await,
            StopDisposition::CleanRelease
        );
    }

    #[test]
    fn installer_setup_refuses_stale_vars_and_requires_writer_exclusion() {
        use sha2::Digest;
        let (_dir, root) = fixture_root();
        let path = LocalPath::parse(&format!(r"{root}\setup-vars.fd")).unwrap();
        let original = b"one-shot";
        let digest = ArtifactPin {
            size: original.len() as u64,
            sha256: sha2::Sha256::digest(original).into(),
        };
        std::fs::write(path.as_str(), original).unwrap();
        let mut pin = PathPin::open(&path, false, true).unwrap();
        pin.verify_content(digest, MAX_SETUP_VARS_BYTES).unwrap();
        let expected = InstallerArtifact {
            path: &path,
            identity: pin.identity,
            pin: Some(digest),
            role: ArtifactRole::WindowsSetupVars,
        };
        let pins = [pin];
        let writer = || OpenOptions::new().write(true).open(path.as_str());

        let mut active = writer().unwrap();
        assert!(matches!(
            guard_setup_vars(&pins[0], digest),
            Err(NativeRefusal::PathCustodyUnverified)
        ));
        std::io::Write::write_all(&mut active, b"modified").unwrap();
        drop(active);
        assert_eq!(pins[0].verified_content, Some(digest));
        let (index, expected_digest) = bind_installer_pin(&expected, &pins).unwrap();
        assert!(matches!(
            guard_setup_vars(&pins[index], expected_digest.unwrap()),
            Err(NativeRefusal::PathCustodyUnverified)
        ));

        std::fs::write(path.as_str(), original).unwrap();
        let guard = guard_setup_vars(&pins[0], digest).unwrap();
        assert!(writer().is_err());
        drop(guard); // The same bound object can be opened writable at handoff.
        assert!(writer().is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn installer_release_quarantines_stalled_reopen_and_callback_at_the_deadline() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        use tokio::time::{Instant, advance};

        struct Finished(mpsc::Sender<()>);
        impl Drop for Finished {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }

        for stall_reopen in [true, false] {
            let (entered_tx, entered_rx) = mpsc::channel();
            let (unblock_tx, unblock_rx) = mpsc::channel();
            let (finished_tx, finished_rx) = mpsc::channel();
            let guard = Finished(finished_tx);
            let wall_bound = Duration::from_secs(3);
            let latch = move || {
                entered_tx.send(()).unwrap();
                unblock_rx.recv_timeout(wall_bound).unwrap();
            };
            let (reopen_latch, callback_latch) = if stall_reopen {
                (Some(latch), None)
            } else {
                (None, Some(latch))
            };
            let callback_started = Arc::new(AtomicBool::new(false));
            let started = callback_started.clone();
            let duration = Duration::from_secs(2);
            let deadline = Instant::now() + duration;
            let mut release = Box::pin(finish_release(
                StopEvidence {
                    guest_shutdown: true,
                    clean_process_exit: true,
                    process_exited: true,
                    job_empty: true,
                    workers_joined: true,
                    ..StopEvidence::default()
                },
                deadline,
                move || {
                    if let Some(latch) = reopen_latch {
                        latch();
                    }
                    Ok(guard)
                },
                move || {
                    started.store(true, Ordering::Release);
                    if let Some(latch) = callback_latch {
                        latch();
                    }
                    true
                },
            ));
            assert!(parks(release.as_mut()));
            entered_rx.recv_timeout(wall_bound).unwrap();
            advance(duration).await;
            assert_eq!(release.await, StopDisposition::ChargedQuarantine);
            assert_eq!(Instant::now(), deadline);
            assert!(finished_rx.try_recv().is_err());
            unblock_tx.send(()).unwrap();
            finished_rx.recv_timeout(wall_bound).unwrap();
            assert_eq!(callback_started.load(Ordering::Acquire), !stall_reopen);
        }
    }

    /// The parent of an exclusive create can be empty, so the nonempty-directory
    /// rule does not protect it. Whatever the file system allows, creation
    /// must not land in the junction target.
    #[test]
    fn a_junction_on_an_empty_pinned_parent_does_not_redirect_creation() {
        let (_dir, root) = fixture_root();
        let a = format!(r"{root}\a");
        let elsewhere = format!(r"{root}\elsewhere");
        for dir in [&a, &elsewhere] {
            std::fs::create_dir(dir).unwrap();
        }
        let path = LocalPath::parse(&format!(r"{a}\new.img")).unwrap();
        let mut converted = None;
        let result = PathPin::pin_observed(&path, false, true, true, &mut |pinned| {
            if pinned == a {
                converted = Some(set_junction(&a, &elsewhere, FILE_WRITE_ATTRIBUTES));
            }
        });
        let converted = converted.expect("the race hook did not run");
        println!(
            "empty pinned parent: junction via attribute-only handle = {:?}; walk = {}",
            converted.as_ref().map_err(|error| error.raw_os_error()),
            if result.is_ok() { "pinned" } else { "refused" }
        );
        assert!(
            !Path::new(&format!(r"{elsewhere}\new.img")).exists(),
            "creation followed a junction set on a pinned parent"
        );
        // Either the conversion is refused or the relative walk refuses.
        assert!(converted.is_err() || result.is_err());
        if converted.is_ok() {
            drop(result);
            std::fs::remove_dir(&a).unwrap();
        }
    }

    fn group(sid: &LocalSid, attributes: i32) -> SID_AND_ATTRIBUTES {
        SID_AND_ATTRIBUTES {
            Sid: sid.0,
            Attributes: attributes as u32,
        }
    }

    #[test]
    fn dedicated_and_current_user_token_policies_stay_distinct() {
        use windows_sys::Win32::System::SystemServices::*;
        for (kind, dedicated, current) in [
            (TokenElevationTypeDefault, true, true),
            (TokenElevationTypeLimited, false, true),
            (TokenElevationTypeFull, false, false),
        ] {
            assert_eq!(TokenMode::Dedicated.accepts_elevation(kind), dedicated);
            assert_eq!(TokenMode::CurrentUser.accepts_elevation(kind), current);
        }
        let users = LocalSid::parse("S-1-5-32-545").unwrap();
        let admins = LocalSid::parse("S-1-5-32-544").unwrap();
        let hyperv = LocalSid::parse("S-1-5-32-578").unwrap();
        let system = LocalSid::parse("S-1-5-18").unwrap();
        let enabled = SE_GROUP_MANDATORY | SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_ENABLED;
        let deny = SE_GROUP_USE_FOR_DENY_ONLY;
        for (name, entries, dedicated, current) in [
            ("ordinary group", vec![group(&users, enabled)], true, true),
            (
                "deny-only Administrators",
                vec![group(&admins, deny)],
                false,
                true,
            ),
            (
                "deny-only Hyper-V Administrators",
                vec![group(&hyperv, deny)],
                false,
                true,
            ),
            (
                "enabled Administrators",
                vec![group(&admins, enabled)],
                false,
                false,
            ),
            (
                "Administrators neither enabled nor deny-only",
                vec![group(&admins, 0)],
                false,
                false,
            ),
            (
                "deny-only but enabled",
                vec![group(&admins, deny | SE_GROUP_ENABLED)],
                false,
                false,
            ),
            (
                "enabled Hyper-V Administrators",
                vec![group(&hyperv, enabled)],
                false,
                false,
            ),
            (
                "SYSTEM even deny-only",
                vec![group(&system, deny)],
                false,
                false,
            ),
            (
                "unknown attribute bits",
                vec![group(&users, enabled | SE_GROUP_RESOURCE)],
                true,
                false,
            ),
        ] {
            assert_eq!(
                TokenMode::Dedicated.accepts_groups(&entries),
                dedicated,
                "{name}"
            );
            assert_eq!(
                TokenMode::CurrentUser.accepts_groups(&entries),
                current,
                "{name}"
            );
        }
    }

    #[test]
    fn dedicated_mode_still_refuses_a_shared_owner_and_supervisor() {
        let same = "S-1-5-21-1-2-3-1001";
        let error = VmToken::derive_checked(same, same).err().unwrap();
        assert_eq!(error.to_string(), "owner and supervisor must differ");
        assert_eq!(
            VmToken::derive(same, same).err(),
            Some(NativeRefusal::IdentityUnverified)
        );
    }

    #[test]
    fn restricted_child_groups_must_match_the_captured_parent_masks() {
        use windows_sys::Win32::System::SystemServices::*;
        let users = LocalSid::parse("S-1-5-32-545").unwrap();
        let interactive = LocalSid::parse("S-1-5-4").unwrap();
        let other = LocalSid::parse("S-1-5-11").unwrap();
        let enabled = SE_GROUP_MANDATORY | SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_ENABLED;
        let parent = [group(&users, enabled), group(&interactive, enabled)];
        let policy = captured_groups(&parent).unwrap();
        assert!(verify_groups_typed(&policy, &parent).is_ok());
        for (name, child) in [
            (
                "mask changed",
                vec![
                    group(&users, SE_GROUP_USE_FOR_DENY_ONLY),
                    group(&interactive, enabled),
                ],
            ),
            (
                "order swapped",
                vec![group(&interactive, enabled), group(&users, enabled)],
            ),
            ("group missing", vec![group(&users, enabled)]),
            (
                "group added",
                vec![
                    group(&users, enabled),
                    group(&interactive, enabled),
                    group(&other, enabled),
                ],
            ),
            (
                "group replaced",
                vec![group(&users, enabled), group(&other, enabled)],
            ),
        ] {
            let err = verify_groups_typed(&policy, &child).err().unwrap();
            assert_eq!(
                err.into_invalidation(),
                Invalidation::Confirmed(NativeRefusal::IdentityUnverified),
                "{name}"
            );
        }
        // An invalid SID entry fails decoding as I/O (Indeterminate), not Mismatch.
        let invalid_sid = [SID_AND_ATTRIBUTES {
            Sid: null_mut(),
            Attributes: enabled as u32,
        }];
        assert_eq!(
            verify_groups_typed(&policy, &invalid_sid)
                .err()
                .unwrap()
                .into_invalidation(),
            Invalidation::Indeterminate(NativeRefusal::IdentityUnverified)
        );
        // Null DACL is a measured mismatch (Ok(false)), not a GetAce error.
        let expected_acl = [vm_acl::Entry {
            deny: false,
            sid: users.0,
            mask: FILE_ALL_ACCESS,
        }];
        assert!(!unsafe { vm_acl::matches_typed(null(), &expected_acl) }.unwrap());
        // A malformed ACL whose AceCount exceeds its AclSize makes Win32 GetAce
        // fail cleanly with an OS error, which must propagate as Err (I/O /
        // Indeterminate) rather than Ok(false) (Mismatch / Confirmed).
        let mut malformed = ACL {
            AclRevision: ACL_REVISION as u8,
            Sbz1: 0,
            AclSize: size_of::<ACL>() as u16,
            AceCount: 0,
            Sbz2: 0,
        };
        assert_ne!(
            unsafe { InitializeAcl(&mut malformed, size_of::<ACL>() as u32, ACL_REVISION,) },
            0
        );
        malformed.AceCount = 1;
        let ace_err = unsafe { vm_acl::matches_typed(&malformed, &expected_acl) }
            .expect_err("GetAce must fail when AceCount exceeds AclSize");
        assert_eq!(
            TokenCheckError::Io(ace_err).into_invalidation(),
            Invalidation::Indeterminate(NativeRefusal::IdentityUnverified)
        );
    }

    #[test]
    fn verify_current_profile_confirms_mismatch_keeps_io_indeterminate_and_latches_reverted_drift()
    {
        const ID_REFUSAL: NativeRefusal = NativeRefusal::IdentityUnverified;

        // Real Win32 checks on inspect_token_identity_typed:
        // 1) A real process token against an enrolled SID that does not match
        //    is a measured mismatch -> Confirmed.
        let parent = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
        let wrong_sid = LocalSid::parse("S-1-5-21-1-2-3-9999").unwrap();
        let mismatch =
            inspect_token_identity_typed(&parent, &wrong_sid, false, TokenMode::CurrentUser, false)
                .err()
                .unwrap();
        assert_eq!(
            mismatch.into_invalidation(),
            Invalidation::Confirmed(ID_REFUSAL)
        );

        // 2) Passing a non-token handle fails GetTokenInformation with an OS
        //    error -> Indeterminate, never Confirmed.
        let temp = tempfile::tempdir().unwrap();
        let dummy_path = temp.path().join("not-a-token.bin");
        std::fs::write(&dummy_path, b"x").unwrap();
        let not_a_token = OwnedHandle::from(File::open(&dummy_path).unwrap());
        let io_err = inspect_token_identity_typed(
            &not_a_token,
            &wrong_sid,
            false,
            TokenMode::CurrentUser,
            false,
        )
        .err()
        .unwrap();
        assert_eq!(
            io_err.into_invalidation(),
            Invalidation::Indeterminate(ID_REFUSAL)
        );

        // Construct a real VmToken snapshot from the current process token so
        // this test runs identically on interactive and CI Windows sessions.
        let stats_buf = token_data(&parent, TokenStatistics).unwrap();
        let auth = unsafe { (*stats_buf.as_ptr().cast::<TOKEN_STATISTICS>()).AuthenticationId };
        let session = token_data(&parent, TokenSessionId).unwrap()[0] as u32;
        let logon = token_data(&parent, TokenLogonSid).unwrap();
        let groups_buf = token_data(&parent, TokenGroups).unwrap();
        let groups = captured_groups(token_entries(&groups_buf).unwrap()).unwrap();
        let clone_identity = || Identity {
            logon: logon.clone(),
            authentication: auth,
            session,
        };
        let token = VmToken {
            token: parent,
            account: LocalSid::parse("S-1-5-21-1-2-3-1001").unwrap(),
            network: LocalSid::parse("S-1-5-2").unwrap(),
            system: LocalSid::parse("S-1-5-18").unwrap(),
            identity: clone_identity(),
            mode: TokenMode::CurrentUser,
            groups: groups.clone(),
        };

        // Matching parent and child pass.
        assert!(
            token
                .verify_current_profile_with(
                    || Ok(()),
                    || Ok((clone_identity(), groups.clone())),
                    || Ok(()),
                )
                .is_ok()
        );

        // Measured parent group drift -> Confirmed.
        let mut drifted_groups = groups.clone();
        drifted_groups.push((vec![1, 2, 3, 4], 0));
        assert_eq!(
            token
                .verify_current_profile_with(
                    || Ok(()),
                    || Ok((clone_identity(), drifted_groups.clone())),
                    || Ok(()),
                )
                .err()
                .unwrap()
                .into_invalidation(),
            Invalidation::Confirmed(ID_REFUSAL)
        );

        // Measured parent logon session/LUID drift -> Confirmed.
        let mut drifted_logon = clone_identity();
        drifted_logon.authentication.LowPart ^= 1;
        assert_eq!(
            token
                .verify_current_profile_with(
                    || Ok(()),
                    || Ok((drifted_logon, groups.clone())),
                    || Ok(()),
                )
                .err()
                .unwrap()
                .into_invalidation(),
            Invalidation::Confirmed(ID_REFUSAL)
        );

        // Measured thread impersonation and child mismatch -> Confirmed.
        assert_eq!(
            token
                .verify_current_profile_with(
                    || Err(TokenCheckError::Mismatch("impersonation")),
                    || Ok((clone_identity(), groups.clone())),
                    || Ok(()),
                )
                .err()
                .unwrap()
                .into_invalidation(),
            Invalidation::Confirmed(ID_REFUSAL)
        );
        assert_eq!(
            token
                .verify_current_profile_with(
                    || Ok(()),
                    || Ok((clone_identity(), groups.clone())),
                    || Err(TokenCheckError::Mismatch("child policy drift")),
                )
                .err()
                .unwrap()
                .into_invalidation(),
            Invalidation::Confirmed(ID_REFUSAL)
        );

        // API/I/O failure stays Indeterminate and does not revoke the epoch.
        let (io_epoch, sample) = open_test_epoch();
        let io_refresh = io_epoch.refresh(
            || {
                token
                    .verify_current_profile_with(
                        || Ok(()),
                        || Err(TokenCheckError::Io(io::Error::other("LSA busy"))),
                        || Ok(()),
                    )
                    .map_err(TokenCheckError::into_invalidation)?;
                Ok(sample)
            },
            || sample.captured,
        );
        assert_eq!(io_refresh, Err(Invalidation::Indeterminate(ID_REFUSAL)));
        assert!(!io_epoch.effects_revoked());
        assert!(
            io_epoch
                .refresh(
                    || {
                        token
                            .verify_current_profile_with(
                                || Ok(()),
                                || Ok((clone_identity(), groups.clone())),
                                || Ok(()),
                            )
                            .map_err(TokenCheckError::into_invalidation)?;
                        Ok(sample)
                    },
                    || sample.captured,
                )
                .is_ok()
        );

        // Inactive console session refuses fail-closed as Indeterminate without
        // revoking the epoch.
        let (console_epoch, sample) = open_test_epoch();
        let console_refresh = console_epoch.refresh(
            || {
                token
                    .verify_current_profile_with(
                        || Ok(()),
                        || Err(TokenCheckError::ConsoleInactive),
                        || Ok(()),
                    )
                    .map_err(TokenCheckError::into_invalidation)?;
                Ok(sample)
            },
            || sample.captured,
        );
        assert_eq!(
            console_refresh,
            Err(Invalidation::Indeterminate(ID_REFUSAL))
        );
        assert!(!console_epoch.effects_revoked());
        assert!(
            console_epoch
                .refresh(
                    || {
                        token
                            .verify_current_profile_with(
                                || Ok(()),
                                || Ok((clone_identity(), groups.clone())),
                                || Ok(()),
                            )
                            .map_err(TokenCheckError::into_invalidation)?;
                        Ok(sample)
                    },
                    || sample.captured,
                )
                .is_ok()
        );

        // Observed-then-reverted profile drift latches Confirmed and keeps the
        // epoch unusable even after the parent profile matches again.
        let (drift_epoch, sample) = open_test_epoch();
        let drifted = drift_epoch.refresh(
            || {
                token
                    .verify_current_profile_with(
                        || Ok(()),
                        || Ok((clone_identity(), drifted_groups.clone())),
                        || Ok(()),
                    )
                    .map_err(TokenCheckError::into_invalidation)?;
                Ok(sample)
            },
            || sample.captured,
        );
        assert_eq!(drifted, Err(Invalidation::Confirmed(ID_REFUSAL)));
        assert!(drift_epoch.effects_revoked());
        // The underlying check passes again after revert, yet the epoch stays revoked.
        assert!(
            token
                .verify_current_profile_with(
                    || Ok(()),
                    || Ok((clone_identity(), groups.clone())),
                    || Ok(()),
                )
                .is_ok()
        );
        assert_eq!(
            drift_epoch.refresh(
                || {
                    token
                        .verify_current_profile_with(
                            || Ok(()),
                            || Ok((clone_identity(), groups.clone())),
                            || Ok(()),
                        )
                        .map_err(TokenCheckError::into_invalidation)?;
                    Ok(sample)
                },
                || sample.captured,
            ),
            Err(Invalidation::Confirmed(NativeRefusal::QualificationMissing))
        );
        assert!(drift_epoch.effects_revoked());
        assert_admission_closed();
    }
}

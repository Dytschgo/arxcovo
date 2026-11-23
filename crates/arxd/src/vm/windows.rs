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
        FRESHNESS, InstallerFailure, InstallerFailureReason, InstallerStage, LaunchAuthority,
        NativeRefusal, OfflineLease, StopDisposition, StopEvidence, fresh_observation,
    },
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
    // Every branch requests data access, so the pin participates in sharing
    // checks: no branch shares delete, so rename and delete of a pinned
    // object are refused. A writable leaf shares write for its consumer.
    let access = if create {
        FILE_GENERIC_WRITE | FILE_GENERIC_READ
    } else {
        FILE_GENERIC_READ | READ_CONTROL
    };
    let share = FILE_SHARE_READ | if mutable { FILE_SHARE_WRITE } else { 0 };
    let options = FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | match node {
            Node::Directory => FILE_DIRECTORY_FILE,
            Node::File => FILE_NON_DIRECTORY_FILE,
        };
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
            if create { FILE_CREATE } else { FILE_OPEN },
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

/// A SUBST or other drive-letter alias for a folder is not a volume mount
/// point. The pinned drive-letter root must be the volume's own root object.
fn verify_volume_root(root: &str, pinned: FileIdentity) -> io::Result<()> {
    let mut name = [0u16; 64];
    // The fixed buffer exceeds the 49-unit `\\?\Volume{GUID}\` name.
    succeeded(unsafe {
        GetVolumeNameForVolumeMountPointW(wide(root).as_ptr(), name.as_mut_ptr(), name.len() as u32)
    })?;
    let end = name
        .iter()
        .position(|&unit| unit == 0)
        .ok_or_else(|| io::Error::other("unterminated volume name"))?;
    let volume = String::from_utf16(&name[..end]).map_err(io::Error::other)?;
    if !volume.starts_with(r"\\?\Volume{") || !volume.ends_with(r"}\") {
        return Err(io::Error::other("unexpected volume name"));
    }
    let actual = file_identity(&open_directory(&volume)?, true)?.0;
    if actual != pinned {
        return Err(io::Error::other("drive letter is not a volume root"));
    }
    Ok(())
}

fn file_identity(file: &File, directory: bool) -> io::Result<(FileIdentity, u64)> {
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
            ))?;
        }
    }
    if tag.FileAttributes
        & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_SPARSE_FILE | FILE_ATTRIBUTE_COMPRESSED)
        != 0
        || standard.Directory != directory
        || standard.DeletePending
        || (!directory && standard.NumberOfLinks != 1)
    {
        return Err(io::Error::other("unsafe file type or alias"));
    }
    let allocated = u64::try_from(standard.AllocationSize).map_err(io::Error::other)?;
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
    values: Vec<Box<[HANDLE]>>,
}

impl Attributes {
    fn new() -> io::Result<Self> {
        let mut bytes = 0;
        // The first call obtains the required size.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes) };
        if bytes == 0 || bytes > 65_536 {
            return Err(io::Error::other("invalid attribute list size"));
        }
        let mut storage = vec![0; bytes.div_ceil(size_of::<usize>())];
        // The heap allocation is aligned and lives until DeleteProcThreadAttributeList.
        succeeded(unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut bytes)
        })?;
        Ok(Self {
            storage,
            values: Vec::with_capacity(2),
        })
    }

    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }

    fn insert(&mut self, attribute: usize, handles: &[HANDLE]) -> io::Result<()> {
        // Retain the copy before the list can reference it, so even a failed
        // update leaves no dangling value.
        self.values.push(handles.into());
        let value = self.values.last().expect("value retained above");
        let (address, size) = (value.as_ptr(), size_of_val(&**value));
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
        if memory_limit == 0 || cpu_rate.is_some_and(|rate| !(1..=5000).contains(&rate)) {
            return Err(io::Error::other("invalid Job limits"));
        }
        // An unnamed Job with default non-inheritable handle attributes.
        let job = Self(unsafe { owned(CreateJobObjectW(null(), null())) }?);
        let extended = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY,
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
            let mut actual = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
            job.query(JobObjectCpuRateControlInformation, &mut actual)?;
            // Hard-cap flags select the CpuRate union member.
            if actual.ControlFlags != cpu.ControlFlags
                || unsafe { actual.Anonymous.CpuRate } != rate
            {
                return Err(io::Error::other("Job CPU cap unavailable"));
            }
        }
        let mut actual = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        job.query(JobObjectExtendedLimitInformation, &mut actual)?;
        if actual.BasicLimitInformation.LimitFlags != extended.BasicLimitInformation.LimitFlags
            || actual.JobMemoryLimit != memory_limit
        {
            return Err(io::Error::other("Job containment flags unavailable"));
        }
        Ok(job)
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
        token: &'a OwnedHandle,
        plan: &'a CommandPlan<'a>,
    },
    #[cfg(test)]
    Fixture { image: &'a Path, args: &'a [String] },
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
}

fn at(stage: SpawnStage) -> impl FnOnce(io::Error) -> SpawnError {
    move |error| SpawnError { stage, error }
}

fn spawn(
    launch: Launch<'_>,
    cwd: &Path,
    environment: &[u16],
    job: Job,
    stdio: Stdio,
) -> Result<Child, SpawnError> {
    let (process, thread) =
        create_suspended(&launch, cwd, environment, Assignment::Job(&job), &stdio)?;
    let child = Child {
        job,
        process,
        suspended: Some(thread),
    };
    child.verify_live().map_err(at(SpawnStage::Verify))?;
    Ok(child)
}

fn create_suspended(
    launch: &Launch<'_>,
    cwd: &Path,
    environment: &[u16],
    assignment: Assignment<'_>,
    stdio: &Stdio,
) -> Result<(OwnedHandle, OwnedHandle), SpawnError> {
    let (image, args): (&Path, &[String]) = match launch {
        Launch::Restricted { plan, .. } => (Path::new(plan.image().as_str()), plan.args()),
        #[cfg(test)]
        Launch::Fixture { image, args } => (image, args),
    };
    let recipe = |message| SpawnError {
        stage: SpawnStage::Recipe,
        error: io::Error::other(message),
    };
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
    let mut attributes = Attributes::new().map_err(at(SpawnStage::Attributes))?;
    for (index, handle) in handles.iter().copied().enumerate() {
        let mut flags = 0;
        // Verify the exact distinct child ends and their inheritance flags.
        succeeded(unsafe { GetHandleInformation(handle, &mut flags) })
            .map_err(at(SpawnStage::Attributes))?;
        if handles[..index].contains(&handle) || flags & HANDLE_FLAG_INHERIT == 0 {
            return Err(SpawnError {
                stage: SpawnStage::Attributes,
                error: io::Error::other("invalid inherited handle list"),
            });
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
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    [
        startup.StartupInfo.hStdInput,
        startup.StartupInfo.hStdOutput,
        startup.StartupInfo.hStdError,
    ] = handles;
    startup.lpAttributeList = attributes.pointer();
    let mut process = PROCESS_INFORMATION::default();
    let flags = EXTENDED_STARTUPINFO_PRESENT
        | CREATE_SUSPENDED
        | CREATE_UNICODE_ENVIRONMENT
        | CREATE_NO_WINDOW;
    // All arrays, command buffer, and attribute storage remain live for this call.
    succeeded(unsafe {
        match launch {
            Launch::Restricted { token, .. } => CreateProcessAsUserW(
                token.as_raw_handle(),
                image.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                flags,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut process,
            ),
            #[cfg(test)]
            Launch::Fixture { .. } => CreateProcessW(
                image.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                flags,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut process,
            ),
        }
    })
    .map_err(at(SpawnStage::Create))?;
    drop(attributes);
    // The kernel has created both handles and assigned the Job before any child instruction.
    let (process, thread) = unsafe { (owned(process.hProcess), owned(process.hThread)) };
    Ok((
        process.map_err(at(SpawnStage::Verify))?,
        thread.map_err(at(SpawnStage::Verify))?,
    ))
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
    let mut memory = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // Processor count spans groups; effective Job scope still needs qualification.
    let cpus = unsafe { GetActiveProcessorCount(u16::MAX) };
    if cpus == 0 || unsafe { GlobalMemoryStatusEx(&mut memory) } == 0 || memory.ullAvailPhys == 0 {
        return Err(NativeRefusal::ReservesUnverified);
    }
    Ok((cpus, memory.ullAvailPhys))
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
}

fn token_identity(
    token: &OwnedHandle,
    account: &LocalSid,
    restricted: bool,
) -> io::Result<Identity> {
    let user = token_data(token, TokenUser)?;
    let integrity = token_data(token, TokenIntegrityLevel)?;
    let elevation = token_data(token, TokenElevation)?;
    let elevation_kind = token_data(token, TokenElevationType)?;
    let session = token_data(token, TokenSessionId)?[0] as u32;
    let stats = token_data(token, TokenStatistics)?;
    let groups = token_data(token, TokenGroups)?;
    let logon = token_data(token, TokenLogonSid)?;
    // These are the documented layouts of the respective token information
    // classes, and each kernel-returned SID stays within its backing buffer.
    unsafe {
        let actual_user = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let integrity = (*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())
            .Label
            .Sid;
        let stats = &*stats.as_ptr().cast::<TOKEN_STATISTICS>();
        let token_groups = groups.as_ptr().cast::<TOKEN_GROUPS>();
        let count = (*token_groups).GroupCount as usize;
        let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
        if count > (size_of_val(groups.as_slice()) - offset) / size_of::<SID_AND_ATTRIBUTES>() {
            return Err(io::Error::other("invalid token group count"));
        }
        let entries = std::slice::from_raw_parts(
            std::ptr::addr_of!((*token_groups).Groups).cast::<SID_AND_ATTRIBUTES>(),
            count,
        );
        if EqualSid(actual_user, account.0) == 0
            || IsWellKnownSid(integrity, WinMediumLabelSid) == 0
            || elevation[0] != 0
            || elevation_kind[0] as i32 != TokenElevationTypeDefault
            || session == 0
            || stats.TokenType != TokenPrimary
            || (IsTokenRestricted(token.as_raw_handle()) != 0) != restricted
            || (*logon.as_ptr().cast::<TOKEN_GROUPS>()).GroupCount != 1
            || entries.iter().any(|entry| {
                IsWellKnownSid(entry.Sid, WinBuiltinAdministratorsSid) != 0
                    || IsWellKnownSid(entry.Sid, WinBuiltinHyperVAdminsSid) != 0
                    || IsWellKnownSid(entry.Sid, WinLocalSystemSid) != 0
            })
        {
            return Err(io::Error::other("unqualified VM token"));
        }
        let mut data = null_mut();
        if LsaGetLogonSessionData(&stats.AuthenticationId, &mut data) != 0 || data.is_null() {
            return Err(io::Error::other("unavailable logon session"));
        }
        let local = !(*data).Sid.is_null()
            && EqualSid((*data).Sid, actual_user) != 0
            && (*data).Session == session
            && (*data).LogonType == 2; // SECURITY_LOGON_TYPE::Interactive
        LsaFreeReturnBuffer(data.cast());
        if !local {
            return Err(io::Error::other("unqualified VM logon"));
        }
        verify_privileges(token)?;
        Ok(Identity {
            logon,
            authentication: stats.AuthenticationId,
            session,
        })
    }
}

fn verify_privileges(token: &OwnedHandle) -> io::Result<()> {
    let data = token_data(token, TokenPrivileges)?;
    let privileges = data.as_ptr().cast::<TOKEN_PRIVILEGES>();
    // The kernel returned a variable-length array with this layout.
    let entries = unsafe {
        let count = (*privileges).PrivilegeCount as usize;
        let offset = std::mem::offset_of!(TOKEN_PRIVILEGES, Privileges);
        if count > (size_of_val(data.as_slice()) - offset) / size_of::<LUID_AND_ATTRIBUTES>() {
            return Err(io::Error::other("invalid token privilege count"));
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
        })?;
        let name = String::from_utf16(&name[..size as usize]).map_err(io::Error::other)?;
        if name != "SeChangeNotifyPrivilege"
            && (entry.Attributes & SE_PRIVILEGE_ENABLED != 0
                || !matches!(
                    name.as_str(),
                    "SeShutdownPrivilege"
                        | "SeUndockPrivilege"
                        | "SeIncreaseWorkingSetPrivilege"
                        | "SeTimeZonePrivilege"
                ))
        {
            return Err(io::Error::other("unqualified VM token privilege"));
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
        let supervisor = process_token(
            unsafe { GetCurrentProcess() },
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
        )?;
        let identity = token_identity(&supervisor, &account, false)?;
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
        };
        candidate.install_default_acl()?;
        candidate.verify(&candidate.token)?;
        Ok(candidate)
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
        let observed = token_identity(token, &self.account, true)?;
        let data = token_data(token, TokenDefaultDacl)?;
        // Both identities and the ACL remain backed by the token query allocations.
        let matches = unsafe {
            EqualSid(observed.logon_sid(), self.identity.logon_sid()) != 0
                && observed.session == self.identity.session
                && observed.authentication.LowPart == self.identity.authentication.LowPart
                && observed.authentication.HighPart == self.identity.authentication.HighPart
                && vm_acl::matches(
                    (*data.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl,
                    &self.entries(),
                )
        };
        if !matches {
            return Err(io::Error::other(
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

    async fn join(mut self, deadline: tokio::time::Instant) -> io::Result<()> {
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
        for worker in self.workers.drain(..) {
            worker
                .0
                .join()
                .map_err(|_| io::Error::other("stdio worker failed"))??;
        }
        Ok(())
    }
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
        fresh_observation(captured, self.now(), NativeRefusal::ReservesUnverified)?;
        let deadline = captured + FRESHNESS;
        let deadline = cutoff.map_or(deadline, |cutoff| deadline.min(cutoff));
        let mut armed = false;
        self.state.send_modify(|state| {
            if *state == Dispatch::Idle {
                *state = Dispatch::Armed(deadline);
                armed = true;
            }
        });
        if armed {
            Ok(())
        } else {
            Err(NativeRefusal::ReservesUnverified)
        }
    }

    /// Worker: whether this write may start. Untracked when idle.
    fn begin_write(&self) -> bool {
        let now = self.now();
        let mut start = false;
        self.state.send_modify(|state| match *state {
            Dispatch::Idle => start = true,
            Dispatch::Armed(deadline) | Dispatch::WrittenInTime(deadline) => {
                start = now < deadline;
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

    /// Worker: record whether the write it started completed in time.
    fn end_write(&self, completed: bool) {
        let now = self.now();
        self.state.send_modify(|state| {
            if let Dispatch::Writing(deadline) = *state {
                *state = if completed && now < deadline {
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
        let mut watch = self.state.subscribe();
        let finished = matches!(
            tokio::time::timeout(
                limit,
                watch.wait_for(|state| !matches!(state, Dispatch::Writing(_)))
            )
            .await,
            Ok(Ok(_))
        );
        let mut accepted = false;
        self.state.send_modify(|state| match *state {
            Dispatch::WrittenInTime(_) if finished => {
                *state = Dispatch::Idle;
                accepted = true;
            }
            Dispatch::Expired | Dispatch::Ambiguous => {}
            _ => *state = Dispatch::Ambiguous,
        });
        if accepted {
            Ok(())
        } else {
            Err(NativeRefusal::ReservesUnverified)
        }
    }
}

/// Marks an armed window unconfirmable however the stdin worker exits.
struct WorkerStop(std::sync::Arc<DispatchGate>);

impl Drop for WorkerStop {
    fn drop(&mut self) {
        self.0.worker_stopped();
    }
}

/// Bounded asynchronous ends of the synchronous pipes. A full output buffer
/// applies backpressure, and stderr is drained without retaining peer bytes.
fn start_stdio(
    stdio: Stdio,
    gate: std::sync::Arc<DispatchGate>,
) -> io::Result<(tokio::io::DuplexStream, tokio::io::DuplexStream, WorkerSet)> {
    use std::io::Write;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    let (reader, mut output) = duplex(8192);
    let (writer, mut input) = duplex(8192);
    let runtime = tokio::runtime::Handle::current();
    let mut set = WorkerSet {
        cancel: Shutdown::default(),
        workers: Vec::new(),
    };

    let cancel = set.cancel.clone();
    let executor = runtime.clone();
    set.workers.push(Worker::start("arx-vm-output", move || {
        let mut pipe = File::from(stdio.output);
        let mut bytes = [0; 4096];
        while !cancel.is_cancelled() {
            let Some(count) = read_pipe(&mut pipe, &mut bytes, &cancel)? else {
                break;
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
    })?);
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
            let written = pipe.write_all(&bytes[..count]);
            #[cfg(test)]
            gate.checkpoint(false);
            // The worker, not the QMP reply, records whether it was in time.
            gate.end_write(written.is_ok());
            if let Err(error) = written {
                if cancel.is_cancelled() || error.kind() == io::ErrorKind::BrokenPipe {
                    break;
                }
                return Err(error);
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
    Ok((reader, writer, set))
}

fn read_pipe(pipe: &mut File, bytes: &mut [u8], cancel: &Shutdown) -> io::Result<Option<usize>> {
    use std::io::Read;
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

struct LaunchInputs<'a> {
    plan: CommandPlan<'a>,
    cwd: &'a LocalPath,
    scratch: &'a LocalPath,
    loader_root: &'a LocalPath,
    memory: usize,
    cpu_rate: u32,
    installer: Option<InstallerStage>,
    // The issuer must retain all executable, firmware, DLL/data, media,
    // volume and writable slot custody here, including their ancestors.
    pins: Vec<PathPin>,
}

struct NativeLaunch<'a> {
    child: Child,
    workers: Option<WorkerSet>,
    gate: std::sync::Arc<DispatchGate>,
    pins: std::sync::Arc<[PathPin]>,
    setup_vars_handoff: Option<File>,
    installer_writers: Vec<usize>,
    kind: PlanKind,
    installer: Option<InstallerStage>,
    _authority: &'a LaunchAuthority,
    // A mutating qemu-img launch stays borrowed against its offline lease.
    _offline: Option<&'a OfflineLease>,
}

type PipeQmp = Qmp<tokio::io::DuplexStream, tokio::io::DuplexStream>;
type SerialGuest = GuestChannel<tokio::net::windows::named_pipe::NamedPipeClient>;
type BootIo = (tokio::io::DuplexStream, tokio::io::DuplexStream);

enum BootOutcome {
    Installed(Box<(PipeQmp, SerialGuest)>),
    InstallerReleased,
}

enum BootFailure {
    Native(NativeRefusal),
    Installer(InstallerFailure),
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

impl<'a> NativeLaunch<'a> {
    /// Shared entry for sealed QEMU and qemu-img plans. There is no issuer
    /// for LaunchAuthority in this slice.
    fn hold(
        authority: &'a LaunchAuthority,
        token: &VmToken,
        inputs: LaunchInputs<'a>,
    ) -> Result<(Self, tokio::io::DuplexStream, tokio::io::DuplexStream), NativeRefusal> {
        let kind = inputs.plan.kind();
        // ADR 0064: a public-egress plan also stays closed until its patched
        // build, policy custody and route evidence have native qualification.
        if matches!(kind, PlanKind::StaticWebBoot | PlanKind::PublicEgressBoot) {
            return Err(NativeRefusal::NetworkFenceUnverified);
        }
        match (kind, inputs.installer) {
            (PlanKind::InstallerBoot(phase), Some(stage))
                if phase == stage.phase() && tokio::time::Instant::now() >= stage.deadline() =>
            {
                return Err(NativeRefusal::InstallerDeadlineExpired);
            }
            (PlanKind::InstallerBoot(phase), Some(stage)) if phase == stage.phase() => {}
            (PlanKind::InstallerBoot(_), _) | (_, Some(_)) => {
                return Err(NativeRefusal::InstallerPhaseMismatch);
            }
            _ => {}
        }
        let mut installer_writers = Vec::new();
        let mut setup_vars = None;
        for expected in inputs.plan.installer_artifacts() {
            let (index, setup_digest) = bind_installer_pin(expected, &inputs.pins)?;
            if let Some(digest) = setup_digest {
                setup_vars = Some((index, digest));
            }
            if expected.role.writable() {
                installer_writers.push(index);
            }
        }
        let offline = inputs.plan.offline_lease();
        let gate = std::sync::Arc::new(DispatchGate::default());
        let (stdio, parent) = token.stdio().map_err(|_| NativeRefusal::QmpUnverified)?;
        let job = Job::new(inputs.memory, Some(inputs.cpu_rate))
            .map_err(|_| NativeRefusal::JobUnverified)?;
        let environment = launch_environment(inputs.loader_root, inputs.scratch)
            .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
        let setup_vars_handoff = setup_vars
            .map(|(index, digest)| guard_setup_vars(&inputs.pins[index], digest))
            .transpose()?;
        if inputs
            .installer
            .is_some_and(|stage| tokio::time::Instant::now() >= stage.deadline())
        {
            return Err(NativeRefusal::InstallerDeadlineExpired);
        }
        let child = spawn(
            Launch::Restricted {
                token: &token.token,
                plan: &inputs.plan,
            },
            Path::new(inputs.cwd.as_str()),
            &environment,
            job,
            stdio,
        )
        .map_err(|_| NativeRefusal::JobUnverified)?;
        let actual = process_token(child.process.as_raw_handle(), TOKEN_QUERY)
            .map_err(|_| NativeRefusal::IdentityUnverified)?;
        token
            .verify(&actual)
            .map_err(|_| NativeRefusal::IdentityUnverified)?;
        let (read, write, workers) =
            start_stdio(parent, gate.clone()).map_err(|_| NativeRefusal::QmpUnverified)?;
        Ok((
            Self {
                child,
                workers: Some(workers),
                gate,
                pins: inputs.pins.into(),
                setup_vars_handoff,
                installer_writers,
                kind,
                installer: inputs.installer,
                _authority: authority,
                _offline: offline,
            },
            read,
            write,
        ))
    }

    async fn boot(
        &mut self,
        endpoint: Option<(&VmToken, &SerialName)>,
        io: BootIo,
        cancel: &Shutdown,
        // Returns the monotonic capture time of a fresh resource sample.
        mut revalidate: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<BootOutcome, BootFailure> {
        if let Some(stage) = self.installer {
            return self
                .boot_installer(stage, io, cancel, &mut revalidate, verify_release)
                .await
                .map_err(BootFailure::Installer);
        }
        let (token, serial) = endpoint
            .filter(|_| self.kind == PlanKind::Boot)
            .ok_or(BootFailure::Native(NativeRefusal::QmpUnverified))?;
        self.child
            .resume_checked(cancel, &mut revalidate)
            .map_err(BootFailure::Native)?;
        // Workers are already draining. Do not await the QMP greeting while
        // QEMU may be waiting for a client at CreateNamedPipe/ConnectNamedPipe.
        let pipe = connect_serial(
            serial,
            &self.child,
            token.account.0,
            &token.entries(),
            cancel,
            tokio::time::Instant::now() + Duration::from_secs(10),
        )
        .await
        .map_err(BootFailure::Native)?;
        let (read, write) = io;
        let mut qmp = Qmp::handshake(read, write, cancel)
            .await
            .map_err(|_| BootFailure::Native(NativeRefusal::QmpUnverified))?;
        self.dispatch_cont(&mut qmp, cancel, &mut revalidate, None)
            .await
            .map_err(|error| {
                BootFailure::Native(match error {
                    InstallerFailureReason::Native(reason) => reason,
                    _ => NativeRefusal::QmpUnverified,
                })
            })?;
        let guest = GuestChannel::ready(pipe, &mut qmp, cancel)
            .await
            .map_err(|_| BootFailure::Native(NativeRefusal::GuestEndpointUnverified))?;
        Ok(BootOutcome::Installed(Box::new((qmp, guest))))
    }

    async fn dispatch_cont(
        &self,
        qmp: &mut PipeQmp,
        cancel: &Shutdown,
        revalidate: &mut dyn FnMut() -> Result<std::time::Instant, NativeRefusal>,
        stage: Option<InstallerStage>,
    ) -> Result<(), InstallerFailureReason> {
        // Resample between the prelaunch status receipt and `cont`, and arm the
        // stdin worker with that sample's deadline.
        let mut refused = None;
        let gate = self.gate.clone();
        let released = qmp
            .release_startup_hold(cancel, || {
                let armed = revalidate().and_then(|captured| match stage {
                    Some(stage) => gate.arm_until(captured, Some(stage.deadline().into_std())),
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
        if let Some(stage) = stage {
            let deadline = stage.deadline();
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(InstallerFailureReason::Cancelled),
                _ = tokio::time::sleep_until(deadline) => return Err(InstallerFailureReason::Timeout),
                result = self.gate.settle(grace) => result,
            }
        } else {
            self.gate.settle(grace).await
        }.map_err(InstallerFailureReason::Native)
    }

    async fn boot_installer(
        &mut self,
        stage: InstallerStage,
        io: BootIo,
        cancel: &Shutdown,
        revalidate: &mut dyn FnMut() -> Result<std::time::Instant, NativeRefusal>,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> Result<BootOutcome, InstallerFailure> {
        use InstallerFailureReason as Failure;
        use tokio::time::{Instant, sleep_until};
        let grace = Duration::from_secs(2);
        let deadline = stage.deadline();
        let result = async {
            if cancel.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Failure::Timeout);
            }
            self.child.resume_checked_with_handoff(cancel, revalidate, || {
                // QEMU opens writable pflash by name after resume. The
                // original object and ancestry pin stays held across it.
                drop(self.setup_vars_handoff.take());
            }).map_err(Failure::Native)?;
            let (read, write) = io;
            let mut qmp = Qmp::installer_handshake(read, write, cancel, stage).await?;
            self.dispatch_cont(&mut qmp, cancel, revalidate, Some(stage)).await?;
            {
                let progress = qmp.observe_installer_shutdown(cancel);
                tokio::pin!(progress);
                loop {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return Err(Failure::Cancelled),
                        result = &mut progress => { result?; break; }
                        _ = tokio::time::sleep(FRESHNESS) => {
                            let captured = revalidate().map_err(Failure::Native)?;
                            fresh_observation(captured, std::time::Instant::now(), NativeRefusal::ReservesUnverified)
                                .map_err(Failure::Native)?;
                        }
                    }
                }
            }
            // Keep QMP's stdin open while QEMU exits voluntarily.
            let exit_by = deadline.min(Instant::now() + grace);
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(Failure::Cancelled),
                _ = sleep_until(exit_by) => Err(Failure::Timeout),
                result = self.child.reap(exit_by) => {
                    result.map_err(|_| Failure::ReleaseUnverified)?;
                    (self.child.exit_code() == Ok(0)).then_some(()).ok_or(Failure::ProcessFailed)
                }
            }
        }.await;

        let release_by = match result {
            Ok(()) => deadline.min(Instant::now() + grace),
            Err(reason) => {
                // Failure teardown has an independent bound after stage expiry.
                if self.child.job.terminate().is_err() {
                    return Err(InstallerFailure {
                        phase: stage.phase(),
                        reason,
                        recovery: StopDisposition::ChargedQuarantine,
                    });
                }
                Instant::now() + grace
            }
        };
        let observed = self
            .release_evidence(result.is_ok(), result.is_ok(), release_by, verify_release)
            .await;
        stage.complete(result, observed, cancel.is_cancelled())?;
        Ok(BootOutcome::InstallerReleased)
    }

    /// Every caller, including a failed boot, must reconcile this owned launch.
    /// On inconclusive evidence the caller retains the pinned generation.
    async fn stop(
        &mut self,
        qmp: Option<PipeQmp>,
        cancel: &Shutdown,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> StopDisposition {
        let mut observed = StopEvidence::default();
        if let Some(qmp) = qmp {
            observed.guest_shutdown = qmp.powerdown(cancel).await.is_ok();
        }
        let grace = tokio::time::Instant::now() + Duration::from_secs(2);
        if observed.guest_shutdown && self.child.reap(grace).await.is_ok() {
            observed.clean_process_exit = self.child.exit_code() == Ok(0);
        } else if self.child.job.terminate().is_err() {
            return StopDisposition::ChargedQuarantine;
        }
        self.release_evidence(
            observed.guest_shutdown,
            observed.clean_process_exit,
            tokio::time::Instant::now() + Duration::from_secs(2),
            verify_release,
        )
        .await
    }

    async fn release_evidence(
        &mut self,
        guest_shutdown: bool,
        clean_process_exit: bool,
        deadline: tokio::time::Instant,
        verify_release: impl FnOnce() -> bool + Send + 'static,
    ) -> StopDisposition {
        let mut observed = StopEvidence {
            guest_shutdown,
            clean_process_exit,
            ..StopEvidence::default()
        };
        observed.process_exited = self.child.reap(deadline).await.is_ok();
        observed.job_empty = matches!(self.child.job.active(), Ok(0));
        observed.workers_joined = if let Some(workers) = self.workers.take() {
            workers.join(deadline).await.is_ok()
        } else {
            false
        };
        let pins = self.pins.clone();
        let writers = self.installer_writers.clone();
        finish_release(
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

/// The worker owns custody and can only report evidence. The async caller
/// alone accepts it and decides release. A detached worker may finish I/O,
/// but a closed receiver suppresses any new callback or accepted proof.
async fn finish_release<T: 'static>(
    mut observed: StopEvidence,
    deadline: tokio::time::Instant,
    reopen: impl FnOnce() -> io::Result<T> + Send + 'static,
    verify_release: impl FnOnce() -> bool + Send + 'static,
) -> StopDisposition {
    use tokio::time::{Instant, sleep_until};
    if !observed.ready_to_verify_release() || Instant::now() >= deadline {
        return StopDisposition::ChargedQuarantine;
    }
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let Ok(worker) = Worker::start("arx-vm-release", move || {
        if tx.is_closed() {
            return Ok(());
        }
        let guard = reopen()?;
        let verified = !tx.is_closed() && verify_release();
        drop(guard);
        let _ = tx.send(verified);
        Ok(())
    }) else {
        return StopDisposition::ChargedQuarantine;
    };
    let verified = tokio::select! {
        biased;
        _ = sleep_until(deadline) => {
            rx.close();
            worker.cancel_io();
            false
        }
        evidence = &mut rx => matches!(evidence, Ok(true)) && Instant::now() < deadline,
    };
    observed.verify_release(|| verified);
    observed.disposition()
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
    fn resume_checked(
        &mut self,
        cancel: &Shutdown,
        revalidate: &mut dyn FnMut() -> Result<std::time::Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        self.resume_checked_with_handoff(cancel, revalidate, || {})
    }

    fn resume_checked_with_handoff(
        &mut self,
        cancel: &Shutdown,
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
        handoff();
        self.resume().map_err(|_| NativeRefusal::JobUnverified)
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
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::windows::fs::MetadataExt,
    };
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

    fn fixture_launch(probe: usize) -> Option<(Child, Stdio)> {
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
            Ok(child) => Some((child, parent_io)),
            Err(SpawnError {
                stage: SpawnStage::Create,
                error,
            }) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                classify_job_refusal(&image, &args, cwd, &environment);
                None
            }
            Err(SpawnError { stage, error }) => {
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
        let (process, _thread) = create_suspended(
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

    async fn ready(reader: &mut tokio::io::DuplexStream) {
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
    fn current_user() -> (Vec<usize>, String) {
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

    #[test]
    fn attribute_values_outlive_the_list_on_a_creation_error() {
        let job = Job::new(64 * 1024 * 1024, None).unwrap();
        // The guard copies values, so a caller's temporary array may end first.
        let mut attributes = Attributes::new().unwrap();
        {
            let temporary = [job.0.as_raw_handle()];
            attributes
                .insert(PROC_THREAD_ATTRIBUTE_JOB_LIST as usize, &temporary)
                .unwrap();
            assert_ne!(attributes.values[0].as_ptr(), temporary.as_ptr());
        }
        assert_eq!(&*attributes.values[0], &[job.0.as_raw_handle()]);
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
            .unwrap_err();
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
        _read: tokio::io::DuplexStream,
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
}

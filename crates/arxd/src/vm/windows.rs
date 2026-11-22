//! Candidate Windows mechanisms. The runtime has no production issuer.
#![allow(
    dead_code,
    reason = "private native adapter awaits owner qualification and slice 4 wiring"
)]

use super::{
    guest::GuestChannel,
    paths::{FileIdentity, LocalPath},
    qemu_args::{CommandPlan, PlanKind, SerialName},
    qmp::Qmp,
    runtime::{LaunchAuthority, NativeRefusal, OfflineLease, StopDisposition, StopEvidence},
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
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authentication::Identity::*, Authorization::ConvertStringSidToSidW, *},
    Storage::FileSystem::*,
    System::{
        IO::CancelSynchronousIo, JobObjects::*, Pipes::*, RemoteDesktop::*, SystemInformation::*,
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
}

impl PathPin {
    fn open(path: &LocalPath, directory: bool, mutable: bool) -> Result<Self, NativeRefusal> {
        Self::pin(path, directory, mutable, false).map_err(|_| NativeRefusal::PathCustodyUnverified)
    }

    fn create(path: &LocalPath) -> Result<Self, NativeRefusal> {
        Self::pin(path, false, true, true).map_err(|_| NativeRefusal::PathCustodyUnverified)
    }

    fn pin(path: &LocalPath, directory: bool, mutable: bool, create: bool) -> io::Result<Self> {
        let parts = &path.as_str()[3..];
        let root = &path.as_str()[..3];
        // A drive alias for a network, optical, or removable device is refused.
        if unsafe { GetDriveTypeW(wide(root).as_ptr()) } != 3 {
            return Err(io::Error::other("expected a fixed local volume"));
        }
        let mut ancestors = Vec::new();
        let mut volume = None;
        for end in std::iter::once(3)
            .chain(parts.match_indices('\\').map(|(at, _)| at + 3))
            .chain((!parts.is_empty()).then_some(path.as_str().len()))
        {
            let last = end == path.as_str().len();
            let expected_directory = !last || directory;
            let file = open_path(&path.as_str()[..end], last && mutable, last && create)?;
            let (identity, allocated) = file_identity(&file, expected_directory)?;
            if end == 3 {
                verify_ntfs(&file)?;
                verify_volume_root(root, identity)?;
                volume = Some(identity.volume);
            }
            if Some(identity.volume) != volume {
                return Err(io::Error::other("unexpected volume transition"));
            }
            if last {
                // Reopen by the pinned spelling and compare the actual object.
                let check = open_path(path.as_str(), mutable, false)?;
                if file_identity(&check, directory)?.0 != identity {
                    return Err(io::Error::other("path binding changed"));
                }
                return Ok(Self {
                    _ancestors: ancestors,
                    leaf: file,
                    identity,
                    allocated,
                });
            }
            ancestors.push(file);
        }
        Err(io::Error::other("path has no leaf"))
    }
}

fn open_path(path: &str, mutable: bool, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .access_mode(if create {
            FILE_GENERIC_WRITE | FILE_GENERIC_READ
        } else if mutable {
            FILE_READ_ATTRIBUTES | READ_CONTROL
        } else {
            FILE_GENERIC_READ | READ_CONTROL
        })
        .share_mode(FILE_SHARE_READ | if mutable { FILE_SHARE_WRITE } else { 0 })
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        // The explicit access mode above takes precedence; std requires the
        // write flag before it accepts exclusive creation.
        .write(create)
        .create_new(create);
    options.open(path)
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
    let actual = file_identity(&open_path(&volume, false, false)?, true)?.0;
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

struct Attributes {
    storage: Vec<usize>,
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
        Ok(Self { storage })
    }

    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }

    fn insert(&mut self, attribute: usize, handles: &[HANDLE]) -> io::Result<()> {
        // The caller retains the attribute value array until this list is destroyed.
        succeeded(unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                attribute,
                handles.as_ptr().cast(),
                size_of_val(handles),
                null_mut(),
                null_mut(),
            )
        })
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        // Created by InitializeProcThreadAttributeList in this allocation.
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
    // The Job array outlives the attribute list, which is destroyed below.
    let jobs = match assignment {
        Assignment::Job(job) => Some([job.0.as_raw_handle()]),
        #[cfg(test)]
        Assignment::ControlWithoutJob => None,
    };
    if let Some(jobs) = &jobs {
        attributes
            .insert(PROC_THREAD_ATTRIBUTE_JOB_LIST as usize, jobs)
            .map_err(at(SpawnStage::Attributes))?;
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

/// Bounded asynchronous ends of the synchronous pipes. A full output buffer
/// applies backpressure, and stderr is drained without retaining peer bytes.
fn start_stdio(
    stdio: Stdio,
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
            if let Err(error) = pipe.write_all(&bytes[..count]) {
                if cancel.is_cancelled() || error.kind() == io::ErrorKind::BrokenPipe {
                    break;
                }
                return Err(error);
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
    // The issuer must retain all executable, firmware, DLL/data, media,
    // volume and writable slot custody here, including their ancestors.
    pins: Vec<PathPin>,
}

struct NativeLaunch<'a> {
    child: Child,
    workers: Option<WorkerSet>,
    _pins: Vec<PathPin>,
    // A mutating qemu-img launch stays borrowed against its offline lease.
    _offline: Option<&'a OfflineLease>,
}

type PipeQmp = Qmp<tokio::io::DuplexStream, tokio::io::DuplexStream>;
type SerialGuest = GuestChannel<tokio::net::windows::named_pipe::NamedPipeClient>;

impl<'a> NativeLaunch<'a> {
    /// Shared entry for sealed QEMU and qemu-img plans. There is no issuer
    /// for LaunchAuthority in this slice.
    fn hold(
        _authority: &LaunchAuthority,
        token: &VmToken,
        inputs: LaunchInputs<'a>,
    ) -> Result<(Self, tokio::io::DuplexStream, tokio::io::DuplexStream), NativeRefusal> {
        // A mutating plan carries its lease by construction; refuse otherwise.
        let offline = match inputs.plan.kind() {
            PlanKind::Mutate => Some(
                inputs
                    .plan
                    .offline_lease()
                    .ok_or(NativeRefusal::QualificationMissing)?,
            ),
            PlanKind::Boot | PlanKind::Inspect => None,
        };
        let (stdio, parent) = token.stdio().map_err(|_| NativeRefusal::QmpUnverified)?;
        let job = Job::new(inputs.memory, Some(inputs.cpu_rate))
            .map_err(|_| NativeRefusal::JobUnverified)?;
        let environment = launch_environment(inputs.loader_root, inputs.scratch)
            .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
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
            start_stdio(parent).map_err(|_| NativeRefusal::QmpUnverified)?;
        Ok((
            Self {
                child,
                workers: Some(workers),
                _pins: inputs.pins,
                _offline: offline,
            },
            read,
            write,
        ))
    }

    async fn boot(
        &mut self,
        token: &VmToken,
        serial: &SerialName,
        read: tokio::io::DuplexStream,
        write: tokio::io::DuplexStream,
        cancel: &Shutdown,
        mut revalidate: impl FnMut() -> Result<(), NativeRefusal>,
    ) -> Result<(PipeQmp, SerialGuest), NativeRefusal> {
        self.child.resume_checked(cancel, &mut revalidate)?;
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
        .await?;
        let mut qmp = Qmp::handshake(read, write, cancel)
            .await
            .map_err(|_| NativeRefusal::QmpUnverified)?;
        // Resample between the prelaunch status receipt and `cont`, so guest
        // execution never starts on a sample older than the freshness window.
        let mut refused = None;
        let released = qmp
            .release_startup_hold(cancel, || match revalidate() {
                Ok(()) => true,
                Err(reason) => {
                    refused = Some(reason);
                    false
                }
            })
            .await;
        released.map_err(|_| refused.unwrap_or(NativeRefusal::QmpUnverified))?;
        let guest = GuestChannel::handshake(pipe, cancel)
            .await
            .map_err(|_| NativeRefusal::GuestEndpointUnverified)?;
        Ok((qmp, guest))
    }

    /// Every caller, including a failed boot, must reconcile this owned launch.
    /// On inconclusive evidence the caller retains the pinned generation.
    async fn stop(
        &mut self,
        qmp: Option<PipeQmp>,
        cancel: &Shutdown,
        verify_release: impl FnOnce() -> bool,
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
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        observed.process_exited = self.child.reap(deadline).await.is_ok();
        observed.job_empty = matches!(self.child.job.active(), Ok(0));
        observed.workers_joined = if let Some(workers) = self.workers.take() {
            workers.join(deadline).await.is_ok()
        } else {
            false
        };
        observed.artifacts_reopened = observed.process_exited
            && observed.job_empty
            && observed.workers_joined
            && verify_release();
        observed.disposition()
    }
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
        revalidate: &mut dyn FnMut() -> Result<(), NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        if cancel.is_cancelled() {
            return Err(NativeRefusal::JobUnverified);
        }
        self.verify_live()
            .map_err(|_| NativeRefusal::JobUnverified)?;
        revalidate()?;
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
        println!("SKIP vm-job-fixture: {UNSUPPORTED}");
        eprintln!("SKIP vm-job-fixture: {UNSUPPORTED}");
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
        let (mut read, mut write, workers) = start_stdio(stdio).unwrap();
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
        let (mut read, _write, workers) = start_stdio(stdio).unwrap();
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
        let pinned = file_identity(&open_path(drive, false, false).unwrap(), true)
            .unwrap()
            .0;
        verify_volume_root(drive, pinned).unwrap();
        // A folder on the same volume is not that volume's root object.
        let folder = file_identity(&open_path(&root, false, false).unwrap(), true)
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
        // A fresh directory handle that addresses the reparse point itself.
        let handle = unsafe {
            owned(CreateFileW(
                wide(path).as_ptr(),
                GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            ))
        }
        .unwrap();
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
        .unwrap();
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
                println!("SKIP vm-supervisor-loss: {UNSUPPORTED}");
                eprintln!("SKIP vm-supervisor-loss: {UNSUPPORTED}");
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
        let (mut read, mut write, workers) = start_stdio(stdio).unwrap();
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
            Ok(())
        });
        assert_eq!(refused, Err(NativeRefusal::JobUnverified));
        assert_eq!(calls, 1);
        assert!(child.suspended.is_some());
        child
            .resume_checked(&fresh, &mut || {
                calls += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(calls, 2);
        ready(&mut read).await;
        write.write_all(&[42]).await.unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        child.reap(deadline).await.unwrap();
        workers.join(deadline).await.unwrap();
    }
}

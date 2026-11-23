//! Windows path custody, process ownership, and readiness listener attribution.
use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{
        Console::*, JobObjects::*, SystemInformation::GetSystemWindowsDirectoryW, Threading::*,
    },
};
use windows_sys::core::BOOL;

use crate::vm::paths::LocalPath;

fn ok(result: BOOL) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // Successful APIs transfer handle ownership.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn connector_environment(credential: &str) -> io::Result<Vec<u16>> {
    let mut root = [0u16; MAX_PATH as usize];
    let length = unsafe { GetSystemWindowsDirectoryW(root.as_mut_ptr(), root.len() as u32) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize >= root.len() {
        return Err(refuse());
    }
    // Winsock needs SystemRoot to load its providers. Resolve it from Windows
    // instead of trusting a caller's environment or inheriting other variables.
    Ok("SystemRoot="
        .encode_utf16()
        .chain(root[..length as usize].iter().copied())
        .chain("\0TUNNEL_CRED_CONTENTS=".encode_utf16())
        .chain(credential.encode_utf16())
        .chain([0, 0])
        .collect())
}

fn refuse() -> io::Error {
    io::Error::other("connector custody refused")
}

pub(super) struct HeldPath {
    pub(super) path: PathBuf,
    pub(super) file: File,
    _parents: Vec<File>,
}

impl HeldPath {
    pub(super) fn open(path: &Path, directory: bool, private: bool) -> io::Result<Self> {
        let raw = path.to_str().ok_or_else(refuse)?;
        let local = LocalPath::parse(raw).map_err(|_| refuse())?;
        if local.is_drive_root() {
            return Err(refuse());
        }
        let path = PathBuf::from(local.as_str());
        let root = path.ancestors().last().ok_or_else(refuse)?;
        // DRIVE_FIXED. A drive-letter spelling alone also accepts mapped shares.
        if unsafe { GetDriveTypeW(wide(root.as_os_str()).as_ptr()) } != 3 {
            return Err(refuse());
        }
        let mut parents: Vec<_> = path.ancestors().skip(1).collect();
        parents.reverse();
        let parents = parents
            .into_iter()
            .map(|parent| open_object(parent, true))
            .collect::<io::Result<_>>()?;
        let file = open_object(&path, directory)?;
        if private {
            require_owner(&file)?;
            crate::security::private(&path).map_err(|_| refuse())?;
        }
        Ok(Self {
            path,
            file,
            _parents: parents,
        })
    }
}

fn open_object(path: &Path, directory: bool) -> io::Result<File> {
    let flags = FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS;
    // Deny rename/delete throughout the retained ancestor chain. Deny leaf writes too.
    let sharing = FILE_SHARE_READ | if directory { FILE_SHARE_WRITE } else { 0 };
    let file = OpenOptions::new()
        .read(true)
        .share_mode(sharing)
        .custom_flags(flags)
        .open(path)?;
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }
    {
        return Err(refuse());
    }
    if !directory {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        ok(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) })?;
        if info.nNumberOfLinks != 1 {
            return Err(refuse());
        }
    }
    Ok(file)
}

fn require_owner(file: &File) -> io::Result<()> {
    let mut token = null_mut();
    ok(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) })?;
    let token = owned(token)?;
    let mut size = 0;
    unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size) };
    if size == 0 || size > 4096 {
        return Err(refuse());
    }
    let mut user = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    ok(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            user.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    })?;
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(refuse());
    }
    // Both SIDs are backed by live OS-validated buffers.
    let matches = !owner.is_null()
        && unsafe { EqualSid(owner, (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid) != 0 };
    unsafe { LocalFree(descriptor) };
    if matches { Ok(()) } else { Err(refuse()) }
}

struct Attributes {
    storage: Vec<usize>,
}

impl Attributes {
    fn new() -> io::Result<Self> {
        let mut size = 0;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size) };
        if size == 0 || size > 64 * 1024 {
            return Err(refuse());
        }
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        ok(unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut size)
        })?;
        Ok(Self { storage })
    }

    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }

    fn add(&mut self, kind: u32, handle: &HANDLE) -> io::Result<()> {
        // The referenced handles are retained by the calling frame until spawn returns.
        ok(unsafe {
            UpdateProcThreadAttribute(
                self.ptr(),
                0,
                kind as usize,
                (handle as *const HANDLE).cast(),
                size_of::<HANDLE>(),
                null_mut(),
                null_mut(),
            )
        })
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.ptr()) };
    }
}

pub(super) struct Process {
    job: OwnedHandle,
    process: OwnedHandle,
    pid: u32,
}

impl Process {
    pub(super) fn spawn(
        executable: &Path,
        args: &[&str],
        cwd: &Path,
        credential: &str,
    ) -> io::Result<Self> {
        let job = owned(unsafe { CreateJobObjectW(null(), null()) })?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        ok(unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            )
        })?;

        // The kernel null device is a bounded sink for both output streams. No
        // subprocess text enters a Rust buffer or an application log.
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let device = owned(unsafe {
            CreateFileW(
                windows_sys::w!("NUL"),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &security,
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        })?;
        let job_handle = job.as_raw_handle();
        let io_handle = device.as_raw_handle();
        let mut attributes = Attributes::new()?;
        attributes.add(PROC_THREAD_ATTRIBUTE_JOB_LIST, &job_handle)?;
        attributes.add(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &io_handle)?;

        // These paths came through LocalPath. The remaining args are fixed flags
        // and a loopback address. There are no embedded quotes or trailing slashes.
        let image = executable.to_str().ok_or_else(refuse)?;
        let command = std::iter::once(image)
            .chain(args.iter().copied())
            .map(|part| format!("\"{part}\""))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = wide(OsStr::new(&command));
        let env = zeroize::Zeroizing::new(connector_environment(credential)?);
        let image = wide(executable.as_os_str());
        let cwd = wide(cwd.as_os_str());
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
        startup.StartupInfo.hStdInput = io_handle;
        startup.StartupInfo.hStdOutput = io_handle;
        startup.StartupInfo.hStdError = io_handle;
        startup.lpAttributeList = attributes.ptr();
        let mut info = PROCESS_INFORMATION::default();
        let flags = EXTENDED_STARTUPINFO_PRESENT
            | CREATE_SUSPENDED
            | CREATE_UNICODE_ENVIRONMENT
            | CREATE_NEW_PROCESS_GROUP;
        let status = unsafe {
            CreateProcessW(
                image.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                flags,
                env.as_ptr().cast(),
                cwd.as_ptr(),
                &startup.StartupInfo,
                &mut info,
            )
        };
        ok(status)?;
        let process = owned(info.hProcess)?;
        let thread = owned(info.hThread)?;
        // The initial thread can run only once the kernel assigned the Job.
        if unsafe { ResumeThread(thread.as_raw_handle()) } != 1 {
            return Err(refuse());
        }
        Ok(Self {
            job,
            process,
            pid: info.dwProcessId,
        })
    }

    pub(super) fn running(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) == WAIT_TIMEOUT }
    }

    pub(super) fn empty(&self) -> io::Result<bool> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        ok(unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of_val(&info) as u32,
                null_mut(),
            )
        })?;
        Ok(info.ActiveProcesses == 0
            && unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) == WAIT_OBJECT_0 })
    }

    pub(super) fn interrupt(&self) {
        // Works when the foreground supervisor and child share a console.
        // The caller still times out and kills the owned Job on any failure.
        if self.running() {
            unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, self.pid) };
        }
    }

    pub(super) fn kill(&self) -> io::Result<()> {
        ok(unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) })
    }

    pub(super) fn owns_peer(&self, server: u16, client: u16) -> bool {
        // TCP_TABLE_OWNER_PID_ALL includes both the loopback listener and accepted connections.
        let mut size = 0;
        unsafe { GetExtendedTcpTable(null_mut(), &mut size, 0, 2, 5, 0) };
        if !(4..=1024 * 1024).contains(&size) {
            return false;
        }
        let mut data = vec![0u32; (size as usize).div_ceil(4)];
        let status =
            unsafe { GetExtendedTcpTable(data.as_mut_ptr().cast(), &mut size, 0, 2, 5, 0) };
        let Some(rows) = data[1..].get(..(data[0] as usize).saturating_mul(6)) else {
            return false;
        };
        status == 0
            && self.running()
            && rows.chunks_exact(6).any(|row| {
                let [state, local_addr, local_port, remote_addr, remote_port, pid] = *row else {
                    unreachable!("six DWORD fields in MIB_TCPROW_OWNER_PID")
                };
                // ESTABLISHED or CLOSE_WAIT/FIN_WAIT while the client socket is still held.
                matches!(state, 5..=8)
                    && local_addr.to_ne_bytes() == [127, 0, 0, 1]
                    && remote_addr.to_ne_bytes() == [127, 0, 0, 1]
                    && u16::from_be(local_port as u16) == server
                    && u16::from_be(remote_port as u16) == client
                    && pid == self.pid
            })
    }
}

// Win32_NetworkManagement_IpHelper is outside the manifest fence.
#[link(name = "iphlpapi")]
unsafe extern "system" {
    fn GetExtendedTcpTable(
        table: *mut std::ffi::c_void,
        size: *mut u32,
        order: BOOL,
        family: u32,
        class: i32,
        reserved: u32,
    ) -> u32;
}

/// Load one DPAPI ciphertext through retained, non-aliasing custody.
pub(in crate::vm) fn read_secret(
    path: &Path,
    limit: usize,
) -> io::Result<zeroize::Zeroizing<Vec<u8>>> {
    use std::io::Read;
    let _root = HeldPath::open(path.parent().ok_or_else(refuse)?, true, true)?;
    let held = HeldPath::open(path, false, true)?;
    let mut encrypted = Vec::new();
    (&held.file).take(16_385).read_to_end(&mut encrypted)?;
    if encrypted.len() > 16_384 {
        return Err(refuse());
    }
    crate::security::unprotect_current_user(&mut encrypted, limit).map_err(|_| refuse())
}

/// Native DNS uses the Windows HTTPS trust store and never uses the origin CA.
pub(in crate::vm) fn public_roots() -> io::Result<rustls::RootCertStore> {
    use windows_sys::Win32::Security::Cryptography::*;
    let store = unsafe { CertOpenSystemStoreW(0, windows_sys::w!("ROOT")) };
    if store.is_null() {
        return Err(refuse());
    }
    let mut roots = rustls::RootCertStore::empty();
    let mut previous = null();
    let mut count = 0;
    loop {
        // Enumeration frees the previous context. The final store close
        // releases the store even when its bounded admission check fails.
        let cert = unsafe { CertEnumCertificatesInStore(store, previous) };
        if cert.is_null() {
            break;
        }
        count += 1;
        if count > 1024 {
            unsafe { CertFreeCertificateContext(cert) };
            roots.roots.clear();
            break;
        }
        let context = unsafe { &*cert };
        if !context.pbCertEncoded.is_null() && (1..=16_384).contains(&context.cbCertEncoded) {
            let der = unsafe {
                std::slice::from_raw_parts(context.pbCertEncoded, context.cbCertEncoded as usize)
            };
            let _ = roots.add(rustls::pki_types::CertificateDer::from(der));
        }
        previous = cert;
    }
    let closed = unsafe { CertCloseStore(store, 0) };
    if closed == 0 || roots.is_empty() {
        return Err(refuse());
    }
    Ok(roots)
}

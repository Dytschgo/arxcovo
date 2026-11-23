//! Windows guest service. Only the dispatcher owns the protocol endpoint.
//! Launch workers can create a suspended child but cannot run it or hold the port.

use std::cell::Cell;
use std::io::{self, Read, Write};
use std::mem::{offset_of, size_of, size_of_val};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use arx_guest::{MAX_SCRIPT, ScriptExecution, ScriptExecutor, ScriptOutcome, serve};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
use windows_sys::Win32::System::SystemServices::{
    MAXIMUM_ALLOWED, SECURITY_MANDATORY_LOW_RID, SECURITY_MANDATORY_MEDIUM_RID,
};
use windows_sys::Win32::System::Threading::*;
use windows_sys::core::GUID;

const SERVICE_NAME: &[u16] = &[97, 114, 120, 45, 103, 117, 101, 115, 116, 0];
const PORT_NAME: &[u8] = b"org.arxcovo.agent.0";
const PORT_GUID: GUID = GUID::from_u128(0x6fde7521_1b65_48ae_b628_80be62016026);
const PORT_INFO_IOCTL: u32 = 0x0022_2002;
const MAX_INTERFACES: u32 = 64;
const MAX_PATH_UNITS: usize = 1024;
const MAX_PORT_INFO: usize = 512;
const START_BUDGET: Duration = Duration::from_secs(30);
const IO_BUDGET: Duration = Duration::from_secs(5);
const IDLE_READ_BUDGET: Duration = Duration::from_secs(180);
const CLEANUP_BUDGET: Duration = Duration::from_secs(2);
const CLEANUP_UNKNOWN: &str = "script cleanup unverified";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn succeeded(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn handle(raw: HANDLE) -> io::Result<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: callers pass a freshly returned, CloseHandle-owned handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
}

fn event() -> io::Result<OwnedHandle> {
    // SAFETY: unnamed, manual-reset event, no inheritable attributes.
    handle(unsafe { CreateEventW(null(), 1, 0, null()) })
}

fn signal(event: &OwnedHandle) {
    // SAFETY: the event remains owned for the duration of this call.
    if unsafe { SetEvent(event.as_raw_handle()) } == 0 {
        std::process::abort();
    }
}

fn millis_left(deadline: Instant) -> u32 {
    let left = deadline.saturating_duration_since(Instant::now());
    left.as_nanos()
        .div_ceil(1_000_000)
        .min(u128::from(u32::MAX - 1)) as u32
}

#[derive(Debug, PartialEq, Eq)]
enum Wait {
    Ready,
    Cancelled,
    Deadline,
}

fn wait_for(ready: HANDLE, cancel: &OwnedHandle, deadline: Instant) -> io::Result<Wait> {
    let handles = [ready, cancel.as_raw_handle()];
    // SAFETY: the caller retains both live waitable handles.
    match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, millis_left(deadline)) } {
        WAIT_OBJECT_0 => Ok(Wait::Ready),
        1 => Ok(Wait::Cancelled),
        WAIT_TIMEOUT => Ok(Wait::Deadline),
        _ => Err(io::Error::last_os_error()),
    }
}

fn signaled(h: HANDLE, millis: u32) -> io::Result<bool> {
    // SAFETY: callers retain the owned handle while waiting.
    match unsafe { WaitForSingleObject(h, millis) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

fn join_ready<T>(worker: JoinHandle<T>) -> Result<T, &'static str> {
    // The caller has observed the Windows thread handle signal. Joining therefore
    // also proves the producer has dropped its inputs and native I/O handles.
    worker.join().map_err(|_| CLEANUP_UNKNOWN)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Starting,
    Running,
    Stopping,
    Stopped,
}

struct Status {
    phase: Phase,
    handle: SERVICE_STATUS_HANDLE,
}

// SCM may invoke the registered callback from another thread. The opaque
// status handle is SCM-owned and only accessed under Service::status.
unsafe impl Send for Status {}

impl Status {
    fn publish(&self, failed: bool) -> bool {
        let (state, controls, hint) = match self.phase {
            Phase::Starting => (SERVICE_START_PENDING, 0, START_BUDGET.as_millis() as u32),
            Phase::Running => (
                SERVICE_RUNNING,
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN,
                0,
            ),
            Phase::Stopping => (SERVICE_STOP_PENDING, 0, 4000),
            Phase::Stopped => (SERVICE_STOPPED, 0, 0),
        };
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: controls,
            dwWin32ExitCode: if failed {
                ERROR_SERVICE_SPECIFIC_ERROR
            } else {
                0
            },
            dwServiceSpecificExitCode: u32::from(failed),
            dwCheckPoint: u32::from(hint != 0),
            dwWaitHint: hint,
        };
        // SAFETY: this handle is the live registration and status is initialized.
        unsafe { SetServiceStatus(self.handle, &status) != 0 }
    }
}

fn on_control(phase: &mut Phase, control: u32) -> (u32, bool) {
    match control {
        SERVICE_CONTROL_INTERROGATE => (NO_ERROR, false),
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            if matches!(phase, Phase::Starting | Phase::Running) {
                *phase = Phase::Stopping;
                (NO_ERROR, true)
            } else {
                (NO_ERROR, false)
            }
        }
        _ => (ERROR_CALL_NOT_IMPLEMENTED, false),
    }
}

struct Service {
    stop: Arc<OwnedHandle>,
    status: Mutex<Option<Status>>,
    result: OnceLock<Result<(), &'static str>>,
}

static SERVICE: OnceLock<Service> = OnceLock::new();

unsafe extern "system" fn control(
    code: u32,
    _: u32,
    _: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> u32 {
    let service = SERVICE.get().expect("dispatcher initialized");
    let mut registration = service.status.lock().expect("service status lock");
    let Some(status) = registration.as_mut() else {
        return ERROR_SERVICE_CANNOT_ACCEPT_CTRL;
    };
    let (result, stopping) = on_control(&mut status.phase, code);
    if stopping {
        let _ = status.publish(false);
        signal(&service.stop);
    }
    result
}

unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let service = SERVICE.get().expect("dispatcher initialized");
    let result = run_service(service);
    let mut registration = service.status.lock().expect("service status lock");
    if let Some(status) = registration.take() {
        let _ = Status {
            phase: Phase::Stopped,
            ..status
        }
        .publish(result.is_err());
    }
    let _ = service.result.set(result);
}

pub(crate) fn serve_service() -> Result<(), &'static str> {
    let stop = event().map_err(|_| "service cancellation unavailable")?;
    SERVICE
        .set(Service {
            stop: Arc::new(stop),
            status: Mutex::new(None),
            result: OnceLock::new(),
        })
        .map_err(|_| "service dispatcher unavailable")?;
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: SERVICE_NAME.as_ptr().cast_mut(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    // SAFETY: table is terminated and lives until the dispatcher returns.
    succeeded(unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) })
        .map_err(|_| "service dispatcher unavailable")?;
    SERVICE
        .get()
        .and_then(|service| service.result.get())
        .copied()
        .unwrap_or(Err("service status unavailable"))
}

fn run_service(service: &Service) -> Result<(), &'static str> {
    let deadline = Instant::now() + START_BUDGET;
    {
        let mut registration = service.status.lock().expect("service status lock");
        // SAFETY: fixed terminated name and static callback/state.
        let handle =
            unsafe { RegisterServiceCtrlHandlerExW(SERVICE_NAME.as_ptr(), Some(control), null()) };
        if handle.is_null() {
            return Err("service control unavailable");
        }
        let status = registration.insert(Status {
            phase: Phase::Starting,
            handle,
        });
        if !status.publish(false) {
            return Err("service status unavailable");
        }
    }

    let stop = Arc::clone(&service.stop);
    let worker = thread::Builder::new()
        .spawn(move || {
            let profile = Profile::for_service().ok();
            let port = discover_until(&stop, deadline)?;
            let profile = profile.filter(|profile| profile.check_port(&port).is_ok());
            Ok((port, profile))
        })
        .map_err(|_| "service startup unavailable")?;
    let (port, profile) = match wait_for(worker.as_raw_handle(), &service.stop, deadline)
        .map_err(|_| "service startup wait failed")?
    {
        Wait::Ready => join_ready(worker)??,
        Wait::Cancelled => {
            if signaled(
                worker.as_raw_handle(),
                millis_left(Instant::now() + CLEANUP_BUDGET),
            )
            .map_err(|_| "service startup wait failed")?
            {
                let _ = join_ready(worker)?;
                return Ok(());
            }
            return Err("service startup cleanup unverified");
        }
        Wait::Deadline => return Err("service startup deadline"),
    };
    {
        let mut registration = service.status.lock().expect("service status lock");
        let status = registration.as_mut().expect("registered service");
        if status.phase == Phase::Stopping {
            return Ok(());
        }
        status.phase = Phase::Running;
        if !status.publish(false) {
            return Err("service status unavailable");
        }
    }
    let stop = Arc::clone(&service.stop);
    let worker = thread::Builder::new()
        .spawn(move || serve_port(port, profile, &stop))
        .map_err(|_| "service worker unavailable")?;
    let handles = [worker.as_raw_handle(), service.stop.as_raw_handle()];
    // SAFETY: retain the live worker and cancellation handles. The service may
    // remain connected indefinitely, but cancellation starts a bounded stop.
    match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) } {
        WAIT_OBJECT_0 => join_ready(worker)?,
        1 if signaled(worker.as_raw_handle(), 3500).unwrap_or(false) => join_ready(worker)?,
        _ => Err("service worker cleanup unverified"),
    }
}

#[repr(C)]
struct PortInfo {
    id: u32,
    out_vq_full: u8,
    host_connected: u8,
    guest_connected: u8,
    name: [u8; 1],
}

fn port_name(info: &[u8]) -> Option<&[u8]> {
    // vioserial completes with sizeof(VIRTIO_PORT_INFO) + NameString.MaximumLength.
    // Name starts at offsetof, not at the end of the fixed struct.
    let storage = info.get(offset_of!(PortInfo, name)..)?;
    let name_len = info.len().checked_sub(size_of::<PortInfo>())?;
    if name_len == 0 {
        return (storage == [0]).then_some(&[]);
    }
    let name = storage.get(..name_len)?;
    let end = name.iter().position(|byte| *byte == 0)?;
    // Ignore the byte accounted for by sizeof beyond the copied name storage.
    (end != 0 && name[end..].iter().all(|byte| *byte == 0)).then_some(&name[..end])
}

struct DeviceSet(HDEVINFO);

impl Drop for DeviceSet {
    fn drop(&mut self) {
        // SAFETY: owned SetupAPI set returned by SetupDiGetClassDevsW.
        unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}

enum Discovery {
    Absent,
    Found(OwnedHandle),
}

fn discover_once(stop: &OwnedHandle, deadline: Instant) -> Result<Discovery, &'static str> {
    // SAFETY: fixed present-interface query, no caller-provided path or class.
    let raw = unsafe {
        SetupDiGetClassDevsW(
            &PORT_GUID,
            null(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if raw == INVALID_HANDLE_VALUE as HDEVINFO {
        return Err("serial enumeration failed");
    }
    let devices = DeviceSet(raw);
    let mut found = None;
    for index in 0..=MAX_INTERFACES {
        if Instant::now() >= deadline || signaled(stop.as_raw_handle(), 0).unwrap_or(true) {
            return Err("serial discovery interrupted");
        }
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        // SAFETY: initialized, correctly sized interface output for this set.
        if unsafe {
            SetupDiEnumDeviceInterfaces(devices.0, null(), &PORT_GUID, index, &mut interface)
        } == 0
        {
            return if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
                Ok(found.map_or(Discovery::Absent, Discovery::Found))
            } else {
                Err("serial enumeration failed")
            };
        }
        if index == MAX_INTERFACES {
            return Err("serial interface limit");
        }

        // DWORD-aligned storage, sized to cap the returned UTF-16 path.
        let mut detail = [0u32; MAX_PATH_UNITS / 2 + 2];
        detail[0] = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        let mut used = 0;
        // SAFETY: initialized cbSize, aligned bounded storage; Windows fills the path.
        if unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                devices.0,
                &interface,
                detail.as_mut_ptr().cast(),
                size_of_val(&detail) as u32,
                &mut used,
                null_mut(),
            )
        } == 0
        {
            if unsafe { GetLastError() } == ERROR_NO_SUCH_DEVICE {
                return Ok(Discovery::Absent);
            }
            return Err("serial interface detail failed");
        }
        let path_offset = offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
        let path_len = (used as usize)
            .checked_sub(path_offset)
            .filter(|len| *len >= 4 && *len % 2 == 0 && used as usize <= size_of_val(&detail))
            .ok_or("serial interface detail malformed")?
            / 2;
        // SAFETY: offset is u16-aligned and the length was checked against the allocation.
        let path = unsafe {
            std::slice::from_raw_parts(
                detail.as_ptr().cast::<u8>().add(path_offset).cast::<u16>(),
                path_len,
            )
        };
        if path.last() != Some(&0) || path[..path.len() - 1].contains(&0) {
            return Err("serial interface detail malformed");
        }
        // SAFETY: validated NUL-terminated path from this SetupAPI interface set.
        let device = handle(unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | FILE_FLAG_NO_BUFFERING,
                null_mut(),
            )
        });
        let device = match device {
            Ok(device) => device,
            Err(error)
                if matches!(
                    error.raw_os_error().map(|code| code as u32),
                    Some(ERROR_FILE_NOT_FOUND | ERROR_NO_SUCH_DEVICE | ERROR_DEVICE_NOT_CONNECTED)
                ) =>
            {
                return Ok(Discovery::Absent);
            }
            Err(_) => return Err("serial interface inaccessible"),
        };
        let info = transfer(
            &device,
            stop,
            deadline.min(Instant::now() + IO_BUDGET),
            vec![0; MAX_PORT_INFO],
            |device, bytes, size, count, pending| {
                // SAFETY: transfer holds the output buffer and OVERLAPPED through completion.
                unsafe {
                    DeviceIoControl(
                        device,
                        PORT_INFO_IOCTL,
                        null(),
                        0,
                        bytes.cast(),
                        size,
                        count,
                        pending,
                    )
                }
            },
        )
        .map_err(|_| "serial port information unavailable")?;
        if port_name(&info).ok_or("serial port information malformed")? == PORT_NAME
            && found.replace(device).is_some()
        {
            return Err("serial port ambiguous");
        }
    }
    unreachable!("bounded interface enumeration")
}

fn discover_until(stop: &OwnedHandle, deadline: Instant) -> Result<OwnedHandle, &'static str> {
    loop {
        match discover_once(stop, deadline)? {
            Discovery::Found(port) => return Ok(port),
            Discovery::Absent => {}
        }
        if Instant::now() >= deadline {
            return Err("serial discovery deadline");
        }
        if signaled(
            stop.as_raw_handle(),
            millis_left(deadline.min(Instant::now() + Duration::from_millis(250))),
        )
        .map_err(|_| "serial discovery wait failed")?
        {
            return Err("serial discovery interrupted");
        }
    }
}

#[derive(Debug)]
enum TransferError {
    Os(io::Error),
    Deadline,
    Cancelled,
    Undrained,
}

struct PendingIo {
    event: OwnedHandle,
    overlapped: OVERLAPPED,
    bytes: Vec<u8>,
}

fn transfer(
    device: &OwnedHandle,
    cancel: &OwnedHandle,
    deadline: Instant,
    bytes: Vec<u8>,
    start: impl FnOnce(HANDLE, *mut u8, u32, *mut u32, *mut OVERLAPPED) -> i32,
) -> Result<Vec<u8>, TransferError> {
    if signaled(cancel.as_raw_handle(), 0).map_err(TransferError::Os)? {
        return Err(TransferError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(TransferError::Deadline);
    }
    let event = event().map_err(TransferError::Os)?;
    let mut io = Box::new(PendingIo {
        overlapped: OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..Default::default()
        },
        event,
        bytes,
    });
    let mut count = 0;
    let device = device.as_raw_handle();
    if start(
        device,
        io.bytes.as_mut_ptr(),
        io.bytes.len() as u32,
        &mut count,
        &mut io.overlapped,
    ) == 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(TransferError::Os(error));
        }
        let wait = wait_for(io.event.as_raw_handle(), cancel, deadline);
        if !matches!(wait, Ok(Wait::Ready)) {
            // SAFETY: only cancel this operation. A normal completion can race cancellation.
            unsafe { CancelIoEx(device, &io.overlapped) };
            if !signaled(io.event.as_raw_handle(), CLEANUP_BUDGET.as_millis() as u32)
                .unwrap_or(false)
            {
                // The kernel may still reference all three allocations. The service
                // exits on Undrained; retain them until the OS destroys the process.
                Box::leak(io);
                return Err(TransferError::Undrained);
            }
        }
        // SAFETY: the private event signaled; the kernel no longer owns the buffers.
        let result =
            succeeded(unsafe { GetOverlappedResult(device, &io.overlapped, &mut count, 0) });
        match wait {
            Ok(Wait::Ready) => result.map_err(TransferError::Os)?,
            Ok(Wait::Cancelled) => return Err(TransferError::Cancelled),
            Ok(Wait::Deadline) => return Err(TransferError::Deadline),
            Err(error) => return Err(TransferError::Os(error)),
        }
    }
    if count as usize > io.bytes.len() {
        return Err(TransferError::Os(io::ErrorKind::InvalidData.into()));
    }
    io.bytes.truncate(count as usize);
    Ok(io.bytes)
}

struct Port<'a> {
    handle: OwnedHandle,
    stop: &'a OwnedHandle,
    retire: &'a Cell<Option<&'static str>>,
    read_by: Cell<Option<Instant>>,
    write_by: Cell<Option<Instant>>,
}

impl Port<'_> {
    fn check(&self) -> io::Result<()> {
        if self.retire.get().is_some() || signaled(self.stop.as_raw_handle(), 0)? {
            Err(io::ErrorKind::BrokenPipe.into())
        } else {
            Ok(())
        }
    }

    fn result(&self, result: Result<Vec<u8>, TransferError>) -> io::Result<Vec<u8>> {
        result.map_err(|error| match error {
            TransferError::Os(error) => error,
            TransferError::Deadline => io::ErrorKind::TimedOut.into(),
            TransferError::Cancelled => io::ErrorKind::BrokenPipe.into(),
            TransferError::Undrained => {
                self.retire.set(Some("serial I/O cleanup unverified"));
                io::ErrorKind::BrokenPipe.into()
            }
        })
    }
}

impl Read for &Port<'_> {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        let frame_deadline = self.read_by.get();
        let bytes = self.result(transfer(
            &self.handle,
            self.stop,
            frame_deadline.unwrap_or_else(|| Instant::now() + IDLE_READ_BUDGET),
            vec![0; dst.len()],
            |device, bytes, size, count, pending| {
                // SAFETY: transfer holds the mutable read buffer until completion.
                unsafe { ReadFile(device, bytes, size, count, pending) }
            },
        ))?;
        if frame_deadline.is_none() && !bytes.is_empty() {
            self.read_by.set(Some(Instant::now() + IO_BUDGET));
        }
        dst[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

impl Write for &Port<'_> {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.check()?;
        let deadline = self
            .write_by
            .get()
            .unwrap_or_else(|| Instant::now() + IO_BUDGET);
        self.write_by.set(Some(deadline));
        self.result(transfer(
            &self.handle,
            self.stop,
            deadline,
            src.to_vec(),
            |device, bytes, size, count, pending| {
                // SAFETY: transfer holds the write buffer until completion.
                unsafe { WriteFile(device, bytes, size, count, pending) }
            },
        ))
        .map(|bytes| bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.check()?;
        self.read_by.set(None);
        self.write_by.set(None);
        Ok(())
    }
}

fn serve_port(
    port: OwnedHandle,
    profile: Option<Profile>,
    stop: &Arc<OwnedHandle>,
) -> Result<(), &'static str> {
    let retire = Cell::new(None);
    let port = Port {
        handle: port,
        stop,
        retire: &retire,
        read_by: Cell::new(None),
        write_by: Cell::new(None),
    };
    let mut executor = Executor {
        profile: profile.map(Arc::new),
        stop,
        retire: &retire,
    };
    let _ = serve(&port, &port, &mut executor);
    drop(port);
    if let Some(reason) = retire.get() {
        Err(reason)
    } else if signaled(stop.as_raw_handle(), 0).unwrap_or(false) {
        Ok(())
    } else {
        Err("serial session lost")
    }
}

const JOB_PROCESSES: u32 = 16;
const JOB_PROCESS_MEMORY: usize = 256 * 1024 * 1024;
const JOB_MEMORY: usize = 512 * 1024 * 1024;
const JOB_FLAGS: u32 = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
    | JOB_OBJECT_LIMIT_PROCESS_MEMORY
    | JOB_OBJECT_LIMIT_JOB_MEMORY;
const WRAPPER: &str = "$ErrorActionPreference='Stop'; try { \
    $inputStream=[Console]::OpenStandardInput(); \
    $bytes=New-Object byte[] 4097; $length=0; \
    while ($length -lt $bytes.Length) { \
        $read=$inputStream.Read($bytes,$length,$bytes.Length-$length); \
        if ($read -eq 0) { break }; $length+=$read \
    }; \
    if ($length -eq 0 -or $length -eq $bytes.Length) { exit 1 }; \
    $text=[Text.UTF8Encoding]::new($false,$true).GetString($bytes,0,$length); \
    if ($text.IndexOf([char]0) -ge 0) { exit 1 }; \
    & ([scriptblock]::Create($text)); \
    if ($?) { exit 0 }; exit 1 \
} catch { exit 1 }";
const _: () = assert!(MAX_SCRIPT == 4096);

fn refused() -> io::Error {
    io::ErrorKind::PermissionDenied.into()
}

fn must(value: bool) -> io::Result<()> {
    if value { Ok(()) } else { Err(refused()) }
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut units: Vec<_> = path.as_os_str().encode_wide().collect();
    must(path.is_absolute() && units.len() < MAX_PATH_UNITS && !units.contains(&0))?;
    units.push(0);
    Ok(units)
}

fn fixed_path(read: impl FnOnce(*mut u16, u32) -> u32) -> io::Result<PathBuf> {
    let mut buf = [0; MAX_PATH_UNITS];
    let count = read(buf.as_mut_ptr(), buf.len() as u32) as usize;
    must((1..buf.len()).contains(&count) && !buf[..count].contains(&0))?;
    let path = PathBuf::from(String::from_utf16(&buf[..count]).map_err(|_| refused())?);
    must(path.is_absolute())?;
    Ok(path)
}

fn system_paths() -> io::Result<(PathBuf, PathBuf)> {
    // SAFETY: fixed_path provides bounded, writable output buffers.
    let windows = fixed_path(|buf, count| unsafe { GetWindowsDirectoryW(buf, count) })?;
    let system = fixed_path(|buf, count| unsafe { GetSystemDirectoryW(buf, count) })?;
    must(
        windows
            .join("System32")
            .to_str()
            .zip(system.to_str())
            .is_some_and(|(expected, actual)| expected.eq_ignore_ascii_case(actual))
            && cfg!(target_arch = "x86_64"),
    )?;
    Ok((windows, system))
}

fn open_token(process: HANDLE, access: u32) -> io::Result<OwnedHandle> {
    let mut raw = null_mut();
    // SAFETY: initialized output for the token of a live process.
    succeeded(unsafe { OpenProcessToken(process, access, &mut raw) })?;
    handle(raw)
}

fn token_buffer(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<usize>> {
    let mut bytes = 0;
    // SAFETY: size query followed by an aligned bounded token information buffer.
    unsafe { GetTokenInformation(token.as_raw_handle(), class, null_mut(), 0, &mut bytes) };
    must(bytes != 0 && bytes <= 65536)?;
    let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    succeeded(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    })?;
    Ok(buffer)
}

fn token_dword(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> io::Result<u32> {
    let mut value = 0;
    let mut bytes = 0;
    // SAFETY: each caller selects a token information class with a DWORD payload.
    succeeded(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            (&mut value as *mut u32).cast(),
            size_of::<u32>() as u32,
            &mut bytes,
        )
    })?;
    Ok(value)
}

fn sid(kind: WELL_KNOWN_SID_TYPE) -> io::Result<[u32; 17]> {
    let mut buffer = [0u32; 17];
    let mut bytes = size_of_val(&buffer) as u32;
    // SAFETY: aligned SECURITY_MAX_SID_SIZE output, no domain needed.
    succeeded(unsafe {
        CreateWellKnownSid(kind, null_mut(), buffer.as_mut_ptr().cast(), &mut bytes)
    })?;
    Ok(buffer)
}

fn service_sid() -> io::Result<[u32; 17]> {
    let name = wide(r"NT SERVICE\arx-guest");
    let mut buffer = [0u32; 17];
    let mut bytes = size_of_val(&buffer) as u32;
    let mut domain = [0; 64];
    let mut domain_len = domain.len() as u32;
    let mut use_type = 0;
    // SAFETY: fixed local account name and bounded output buffers.
    succeeded(unsafe {
        LookupAccountNameW(
            null(),
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut use_type,
        )
    })?;
    Ok(buffer)
}

fn token_user_is(token: &OwnedHandle, kind: WELL_KNOWN_SID_TYPE) -> io::Result<bool> {
    let buffer = token_buffer(token, TokenUser)?;
    // SAFETY: TokenUser embeds a SID pointer into the live aligned buffer.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    Ok(unsafe { IsWellKnownSid(user.User.Sid, kind) } != 0)
}

fn token_groups(buffer: &[usize]) -> &[SID_AND_ATTRIBUTES] {
    // SAFETY: callers request TokenGroups or TokenRestrictedSids, both of which
    // are OS-initialized TOKEN_GROUPS buffers. Check the variable array extent.
    let count = unsafe { *buffer.as_ptr().cast::<u32>() } as usize;
    if count == 0 {
        return &[];
    }
    let offset = offset_of!(TOKEN_GROUPS, Groups);
    assert!(offset + count * size_of::<SID_AND_ATTRIBUTES>() <= size_of_val(buffer));
    unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>().add(offset).cast(), count) }
}

fn group_attributes(
    token: &OwnedHandle,
    class: TOKEN_INFORMATION_CLASS,
    sid: PSID,
) -> io::Result<Option<u32>> {
    let buffer = token_buffer(token, class)?;
    Ok(token_groups(&buffer).iter().find_map(|group| {
        // SAFETY: both SIDs remain live during the comparison.
        (unsafe { EqualSid(group.Sid, sid) } != 0).then_some(group.Attributes)
    }))
}

const GROUP_ENABLED: u32 = 0x0000_0004;
const GROUP_DENY_ONLY: u32 = 0x0000_0010;
const GROUP_INTEGRITY: u32 = 0x0000_0020;
const LABEL_NO_WRITE_UP: u32 = 1;
// CreateRestrictedToken preserves the source handle's access rights.
const TOKEN_SOURCE_ACCESS: u32 =
    TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ADJUST_DEFAULT | TOKEN_ASSIGN_PRIMARY;

fn medium_or_low(token: &OwnedHandle, expected: u32) -> io::Result<()> {
    let buffer = token_buffer(token, TokenIntegrityLevel)?;
    // SAFETY: TokenIntegrityLevel embeds one valid mandatory-label SID.
    let label = unsafe { &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    let count = unsafe { *GetSidSubAuthorityCount(label.Label.Sid) };
    must(count == 1 && unsafe { *GetSidSubAuthority(label.Label.Sid, 0) } == expected)
}

fn only_privileges(token: &OwnedHandle, names: &[&str]) -> io::Result<()> {
    let mut allowed = Vec::with_capacity(names.len());
    for name in names {
        let name = wide(name);
        let mut luid = LUID::default();
        // SAFETY: terminated local privilege name and writable LUID.
        succeeded(unsafe { LookupPrivilegeValueW(null(), name.as_ptr(), &mut luid) })?;
        allowed.push(luid);
    }
    let buffer = token_buffer(token, TokenPrivileges)?;
    // SAFETY: TokenPrivileges starts with a count and a flexible array.
    let count = unsafe { *buffer.as_ptr().cast::<u32>() } as usize;
    let offset = offset_of!(TOKEN_PRIVILEGES, Privileges);
    must(offset + count * size_of::<LUID_AND_ATTRIBUTES>() <= size_of_val(buffer.as_slice()))?;
    let entries = unsafe {
        std::slice::from_raw_parts(
            buffer
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<LUID_AND_ATTRIBUTES>(),
            count,
        )
    };
    must(entries.iter().all(|entry| {
        allowed
            .iter()
            .any(|luid| entry.Luid.HighPart == luid.HighPart && entry.Luid.LowPart == luid.LowPart)
    }))
}

fn no_thread_token(thread: HANDLE) -> io::Result<()> {
    let mut raw = null_mut();
    // SAFETY: initialized output for a live thread, including a pseudo handle.
    let result = unsafe { OpenThreadToken(thread, TOKEN_QUERY, 1, &mut raw) };
    if result != 0 {
        drop(handle(raw)?);
        return Err(refused());
    }
    must(unsafe { GetLastError() } == ERROR_NO_TOKEN)
}

fn no_linked_token(token: &OwnedHandle) -> io::Result<()> {
    let mut raw: HANDLE = null_mut();
    let mut bytes = 0;
    // SAFETY: TOKEN_LINKED_TOKEN consists of one handle.
    let result = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenLinkedToken,
            (&mut raw as *mut HANDLE).cast(),
            size_of::<HANDLE>() as u32,
            &mut bytes,
        )
    };
    if result != 0 {
        drop(handle(raw)?);
        return Err(refused());
    }
    must(matches!(
        unsafe { GetLastError() },
        ERROR_NO_SUCH_LOGON_SESSION | ERROR_NO_TOKEN
    ))
}

fn restrict_token(
    source: &OwnedHandle,
    mut disabled: Vec<SID_AND_ATTRIBUTES>,
) -> io::Result<OwnedHandle> {
    let mut admins = sid(WinBuiltinAdministratorsSid)?;
    disabled.push(SID_AND_ATTRIBUTES {
        Sid: admins.as_mut_ptr().cast(),
        Attributes: 0,
    });
    // WinRestrictedCodeSid is in the restricting set only, never granted to the
    // service's ordinary group set. Provisioning must grant it on the work area.
    let mut restricting = sid(WinRestrictedCodeSid)?;
    let restrictions = SID_AND_ATTRIBUTES {
        Sid: restricting.as_mut_ptr().cast(),
        Attributes: 0,
    };
    let mut raw = null_mut();
    // SAFETY: all SID buffers and arrays live until CreateRestrictedToken copies them.
    succeeded(unsafe {
        CreateRestrictedToken(
            source.as_raw_handle(),
            DISABLE_MAX_PRIVILEGE | WRITE_RESTRICTED,
            disabled.len() as u32,
            disabled.as_ptr(),
            0,
            null(),
            1,
            &restrictions,
            &mut raw,
        )
    })?;
    let token = handle(raw)?;
    let mut low = [0u32; 3];
    // S-1-16-4096, with one subauthority in a SID of known size.
    low[0] = u32::from_le_bytes([1, 1, 0, 0]);
    low[1] = u32::from_le_bytes([0, 0, 0, 16]);
    low[2] = SECURITY_MANDATORY_LOW_RID as u32;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: low.as_mut_ptr().cast(),
            Attributes: GROUP_INTEGRITY,
        },
    };
    // SAFETY: label and its SID both remain live for the call.
    succeeded(unsafe {
        SetTokenInformation(
            token.as_raw_handle(),
            TokenIntegrityLevel,
            (&label as *const TOKEN_MANDATORY_LABEL).cast(),
            (size_of_val(&label) + size_of_val(&low)) as u32,
        )
    })?;
    verify_restrictions(&token)?;
    Ok(token)
}

fn verify_restrictions(token: &OwnedHandle) -> io::Result<()> {
    let mut restricting = sid(WinRestrictedCodeSid)?;
    must(unsafe { IsTokenRestricted(token.as_raw_handle()) } != 0)?;
    must(group_attributes(token, TokenRestrictedSids, restricting.as_mut_ptr().cast())?.is_some())?;
    must(token_dword(token, TokenType)? == TokenPrimary as u32)?;
    medium_or_low(token, SECURITY_MANDATORY_LOW_RID as u32)?;
    only_privileges(token, &["SeChangeNotifyPrivilege"])
}

fn verify_service_child_token(token: &OwnedHandle, service_sid: &[u32; 17]) -> io::Result<()> {
    verify_restrictions(token)?;
    must(
        token_user_is(token, WinLocalServiceSid)?
            && token_dword(token, TokenSessionId)? == 0
            && token_dword(token, TokenElevation)? == 0,
    )?;
    let flags = group_attributes(token, TokenGroups, service_sid.as_ptr().cast_mut().cast())?;
    must(flags.is_some_and(|flags| flags & GROUP_DENY_ONLY != 0 && flags & GROUP_ENABLED == 0))?;
    no_linked_token(token)
}

struct Descriptor(PSECURITY_DESCRIPTOR);

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: security APIs returned a LocalFree-owned allocation.
        unsafe { LocalFree(self.0) };
    }
}

impl Descriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let sddl = wide(sddl);
        let mut value = null_mut();
        // SAFETY: fixed terminated SDDL and initialized output slot.
        succeeded(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut value,
                null_mut(),
            )
        })?;
        Ok(Self(value))
    }

    fn object(
        h: HANDLE,
        kind: SE_OBJECT_TYPE,
        flags: OBJECT_SECURITY_INFORMATION,
    ) -> io::Result<Self> {
        let mut value = null_mut();
        // SAFETY: handle supports READ_CONTROL and descriptor is allocated by Windows.
        let error = unsafe {
            GetSecurityInfo(
                h,
                kind,
                flags,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut value,
            )
        };
        if error == ERROR_SUCCESS {
            Ok(Self(value))
        } else {
            Err(io::Error::from_raw_os_error(error as i32))
        }
    }

    fn admin_owner(&self) -> io::Result<()> {
        let mut owner = null_mut();
        let mut defaulted = 0;
        // SAFETY: owner SID references the retained descriptor.
        succeeded(unsafe { GetSecurityDescriptorOwner(self.0, &mut owner, &mut defaulted) })?;
        must(
            !owner.is_null()
                && (unsafe { IsWellKnownSid(owner, WinBuiltinAdministratorsSid) } != 0
                    || unsafe { IsWellKnownSid(owner, WinLocalSystemSid) } != 0),
        )
    }

    fn low_label(&self) -> io::Result<()> {
        let mut acl = null_mut();
        let mut present = 0;
        let mut defaulted = 0;
        // SAFETY: retained descriptor contains the file's requested label information.
        succeeded(unsafe {
            GetSecurityDescriptorSacl(self.0, &mut present, &mut acl, &mut defaulted)
        })?;
        must(present != 0 && !acl.is_null() && unsafe { (*acl).AceCount } == 1)?;
        let mut ace = null_mut();
        succeeded(unsafe { GetAce(acl, 0, &mut ace) })?;
        // SAFETY: ACE has at least the common header. Check type and size before reading its SID.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        must(
            header.AceType == 0x11
                && usize::from(header.AceSize)
                    >= offset_of!(SYSTEM_MANDATORY_LABEL_ACE, SidStart) + 12,
        )?;
        let ace = unsafe { &*ace.cast::<SYSTEM_MANDATORY_LABEL_ACE>() };
        let sid = (&ace.SidStart as *const u32).cast_mut().cast();
        must(
            unsafe { IsValidSid(sid) } != 0
                && unsafe {
                    *GetSidSubAuthorityCount(sid) == 1
                        && *GetSidSubAuthority(sid, 0) == SECURITY_MANDATORY_LOW_RID as u32
                }
                && ace.Mask & LABEL_NO_WRITE_UP != 0,
        )
    }

    fn allowed(&self, token: &OwnedHandle, mapping: GENERIC_MAPPING) -> io::Result<u32> {
        let mut privileges = [0usize; 32];
        let mut bytes = size_of_val(&privileges) as u32;
        let mut granted = 0;
        let mut access = 0;
        // SAFETY: descriptor and impersonation token are retained and all outputs are aligned.
        succeeded(unsafe {
            AccessCheck(
                self.0,
                token.as_raw_handle(),
                MAXIMUM_ALLOWED,
                &mapping,
                privileges.as_mut_ptr().cast(),
                &mut bytes,
                &mut granted,
                &mut access,
            )
        })?;
        Ok(if access != 0 { granted } else { 0 })
    }
}

fn impersonation_token(token: &OwnedHandle) -> io::Result<OwnedHandle> {
    let mut raw = null_mut();
    // SAFETY: copy for authorization checks only, with a bounded impersonation level.
    succeeded(unsafe {
        DuplicateTokenEx(
            token.as_raw_handle(),
            TOKEN_QUERY | TOKEN_IMPERSONATE,
            null(),
            SecurityImpersonation,
            TokenImpersonation,
            &mut raw,
        )
    })?;
    handle(raw)
}

fn as_restricted<T>(token: &OwnedHandle, check: impl FnOnce() -> T) -> io::Result<T> {
    // SAFETY: this dedicated worker owns its impersonation scope.
    succeeded(unsafe { SetThreadToken(null(), token.as_raw_handle()) })?;
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            // Continuing under an unexpected identity is unsafe.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
    let _revert = Revert;
    Ok(check())
}

fn denied(raw: io::Result<OwnedHandle>) -> io::Result<()> {
    match raw {
        Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => Ok(()),
        _ => Err(refused()),
    }
}

fn protect_parent(token: &OwnedHandle, authority: &OwnedHandle) -> io::Result<()> {
    let security = Descriptor::from_sddl("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;RC;;;OW)")?;
    // SAFETY: pseudo process handle and retained token handle with WRITE_DAC.
    let parent = unsafe { GetCurrentProcess() };
    for target in [parent, authority.as_raw_handle()] {
        succeeded(unsafe {
            SetKernelObjectSecurity(target, DACL_SECURITY_INFORMATION, security.0)
        })?;
    }
    let pid = unsafe { GetCurrentProcessId() };
    as_restricted(token, || {
        // Probe one right at a time: denial of a union would hide an allowed right.
        for access in [
            PROCESS_CREATE_PROCESS,
            PROCESS_CREATE_THREAD,
            PROCESS_VM_WRITE,
            PROCESS_VM_OPERATION,
            PROCESS_DUP_HANDLE,
            WRITE_DAC,
            WRITE_OWNER,
        ] {
            // SAFETY: fixed self PID and no inheritable handle. The live parent owns this identity.
            denied(handle(unsafe { OpenProcess(access, 0, pid) }))?;
        }
        denied(open_token(parent, TOKEN_DUPLICATE))?;
        denied(open_token(parent, TOKEN_ADJUST_PRIVILEGES))
    })?
}

fn file_mapping() -> GENERIC_MAPPING {
    GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ,
        GenericWrite: FILE_GENERIC_WRITE,
        GenericExecute: FILE_GENERIC_EXECUTE,
        GenericAll: FILE_ALL_ACCESS,
    }
}

const FILE_MUTATION: u32 = FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | FILE_DELETE_CHILD
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER;

fn checked_file(path: &Path, directory: bool) -> io::Result<OwnedHandle> {
    let path = wide_path(path)?;
    // SAFETY: fixed absolute file name, open the reparse point itself and deny delete sharing.
    let file = handle(unsafe {
        CreateFileW(
            path.as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    })?;
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: initialized output of the exact documented class and size.
    succeeded(unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of_val(&info) as u32,
        )
    })?;
    must(
        info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
            && (info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) == directory,
    )?;
    Ok(file)
}

fn protected_file(
    path: &Path,
    token: &OwnedHandle,
    directory: bool,
    work: bool,
) -> io::Result<OwnedHandle> {
    let file = checked_file(path, directory)?;
    let security = Descriptor::object(
        file.as_raw_handle(),
        SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION
            | GROUP_SECURITY_INFORMATION
            | DACL_SECURITY_INFORMATION
            | LABEL_SECURITY_INFORMATION,
    )?;
    security.admin_owner()?;
    let rights = security.allowed(token, file_mapping())?;
    if work {
        security.low_label()?;
        must(
            rights & (FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY)
                == (FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY)
                && rights & (DELETE | WRITE_DAC | WRITE_OWNER) == 0,
        )?;
    } else {
        must(rights & FILE_MUTATION == 0)?;
    }
    Ok(file)
}

struct ScHandle(SC_HANDLE);

impl Drop for ScHandle {
    fn drop(&mut self) {
        // SAFETY: this SCM handle is owned by the wrapper.
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn scm(handle: SC_HANDLE) -> io::Result<ScHandle> {
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(ScHandle(handle))
    }
}

fn check_scm(token: &OwnedHandle, installed: &Path) -> io::Result<()> {
    // SAFETY: fixed local SCM database and service name.
    let manager = scm(unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT) })?;
    let service = scm(unsafe {
        OpenServiceW(
            manager.0,
            SERVICE_NAME.as_ptr(),
            SERVICE_QUERY_CONFIG | READ_CONTROL,
        )
    })?;
    let security = Descriptor::object(service.0, SE_SERVICE, OWNER_SECURITY_INFORMATION)?;
    security.admin_owner()?;

    let mut config = [0usize; 8192 / size_of::<usize>()];
    let mut needed = 0;
    // SAFETY: aligned fixed buffer and live service handle.
    succeeded(unsafe {
        QueryServiceConfigW(
            service.0,
            config.as_mut_ptr().cast(),
            size_of_val(&config) as u32,
            &mut needed,
        )
    })?;
    let config_data = unsafe { &*config.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
    let quoted = wide(&format!("\"{}\" serve-service", installed.display()));
    let expected_user = wide(r"NT AUTHORITY\LocalService");
    must(
        config_data.dwServiceType == SERVICE_WIN32_OWN_PROCESS
            && config_data.dwStartType == SERVICE_AUTO_START
            && config_field(&config, config_data.lpBinaryPathName) == Some(quoted.as_slice())
            && config_field(&config, config_data.lpServiceStartName)
                == Some(expected_user.as_slice()),
    )?;
    let mut sid_type = SERVICE_SID_INFO::default();
    // SAFETY: exact service SID info output size and initialized storage.
    succeeded(unsafe {
        QueryServiceConfig2W(
            service.0,
            SERVICE_CONFIG_SERVICE_SID_INFO,
            (&mut sid_type as *mut SERVICE_SID_INFO).cast(),
            size_of_val(&sid_type) as u32,
            &mut needed,
        )
    })?;
    must(sid_type.dwServiceSidType == SERVICE_SID_TYPE_UNRESTRICTED)?;

    as_restricted(token, || {
        if let Ok(_manager) =
            scm(unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CREATE_SERVICE) })
        {
            return Err(refused());
        }
        must(unsafe { GetLastError() } == ERROR_ACCESS_DENIED)?;
        let restricted = scm(unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT) })?;
        for access in [
            SERVICE_CHANGE_CONFIG,
            SERVICE_START,
            SERVICE_STOP,
            SERVICE_PAUSE_CONTINUE,
            SERVICE_USER_DEFINED_CONTROL,
            DELETE,
            WRITE_DAC,
            WRITE_OWNER,
        ] {
            if let Ok(_service) =
                scm(unsafe { OpenServiceW(restricted.0, SERVICE_NAME.as_ptr(), access) })
            {
                return Err(refused());
            }
            must(unsafe { GetLastError() } == ERROR_ACCESS_DENIED)?;
        }
        Ok(())
    })?
}

fn config_field(buffer: &[usize], ptr: *const u16) -> Option<&[u16]> {
    let remaining = (buffer.as_ptr() as usize + size_of_val(buffer)).checked_sub(ptr as usize)?;
    if (ptr as usize) < buffer.as_ptr() as usize || !ptr.is_aligned() {
        return None;
    }
    // SAFETY: bounds and alignment are checked against the initialized output buffer.
    let units = unsafe { std::slice::from_raw_parts(ptr, remaining / 2) };
    let end = units.iter().position(|&c| c == 0)?;
    units.get(..=end)
}

struct CommandLine {
    application: Vec<u16>,
    command: Vec<u16>,
    env: Vec<u16>,
    cwd: Vec<u16>,
}

impl CommandLine {
    fn powershell(windows: &Path, system: &Path, work: &Path) -> io::Result<Self> {
        let shell = system.join(r"WindowsPowerShell\v1.0\powershell.exe");
        let command = wide(&format!(
            "\"{}\" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command \"{WRAPPER}\"",
            shell.display()
        ));
        must(!command[..command.len() - 1].contains(&0))?;
        let mut env = Vec::new();
        for entry in [
            format!("PATH={}", system.display()),
            format!(
                r"PSModulePath={}\WindowsPowerShell\v1.0\Modules",
                system.display()
            ),
            format!("SystemRoot={}", windows.display()),
            format!("TEMP={}", work.display()),
            format!("TMP={}", work.display()),
            format!("windir={}", windows.display()),
        ] {
            env.extend(wide(&entry));
        }
        env.push(0);
        Ok(Self {
            application: wide_path(&shell)?,
            command,
            env,
            cwd: wide_path(work)?,
        })
    }
}

struct Profile {
    token: OwnedHandle,
    probe: OwnedHandle,
    service_sid: [u32; 17],
    command: CommandLine,
    _held_paths: Vec<OwnedHandle>,
}

impl Profile {
    fn for_service() -> io::Result<Self> {
        // SAFETY: pseudo handles describe the current process and current thread.
        no_thread_token(unsafe { GetCurrentThread() })?;
        let authority = open_token(
            unsafe { GetCurrentProcess() },
            TOKEN_SOURCE_ACCESS | WRITE_DAC,
        )?;
        let service_sid = service_sid()?;
        let mut admins = sid(WinBuiltinAdministratorsSid)?;
        must(
            token_user_is(&authority, WinLocalServiceSid)?
                && token_dword(&authority, TokenSessionId)? == 0
                && token_dword(&authority, TokenElevation)? == 0
                && unsafe { IsTokenRestricted(authority.as_raw_handle()) } == 0
                && group_attributes(&authority, TokenGroups, admins.as_mut_ptr().cast())?
                    .is_none_or(|flags| flags & GROUP_ENABLED == 0)
                && group_attributes(
                    &authority,
                    TokenGroups,
                    service_sid.as_ptr().cast_mut().cast(),
                )?
                .is_some_and(|flags| flags & GROUP_ENABLED != 0),
        )?;
        medium_or_low(&authority, SECURITY_MANDATORY_MEDIUM_RID as u32)?;
        no_linked_token(&authority)?;
        only_privileges(
            &authority,
            &["SeChangeNotifyPrivilege", "SeIncreaseQuotaPrivilege"],
        )?;

        let token = restrict_token(
            &authority,
            vec![SID_AND_ATTRIBUTES {
                Sid: service_sid.as_ptr().cast_mut().cast(),
                Attributes: 0,
            }],
        )?;
        verify_service_child_token(&token, &service_sid)?;
        let probe = impersonation_token(&token)?;
        protect_parent(&probe, &authority)?;

        let (windows, system) = system_paths()?;
        let installed = fixed_path(|out, cap| unsafe { GetModuleFileNameW(null_mut(), out, cap) })?;
        let dir = installed.parent().ok_or_else(refused)?;
        let work = dir.join("work");
        let shell = system.join(r"WindowsPowerShell\v1.0\powershell.exe");
        let command = CommandLine::powershell(&windows, &system, &work)?;

        // Walk from the volume root so an intermediate reparse point cannot
        // redirect the path. Retain every opened component without delete sharing.
        let mut held = Vec::new();
        for path in [&installed, &shell, &work] {
            let parents: Vec<_> = path.ancestors().skip(1).collect();
            for ancestor in parents.into_iter().rev() {
                held.push(checked_file(ancestor, true)?);
            }
        }
        held.push(protected_file(dir, &probe, true, false)?);
        held.push(protected_file(&installed, &probe, false, false)?);
        held.push(protected_file(&work, &probe, true, true)?);
        held.push(checked_file(&shell, false)?);
        check_scm(&probe, &installed)?;
        Ok(Self {
            token,
            probe,
            service_sid,
            command,
            _held_paths: held,
        })
    }

    fn check_port(&self, port: &OwnedHandle) -> io::Result<()> {
        let descriptor = Descriptor::object(
            port.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        )?;
        must(
            descriptor.allowed(&self.probe, file_mapping())? & (FILE_MUTATION | FILE_READ_DATA)
                == 0,
        )
    }
}

struct Job(OwnedHandle);

impl Job {
    fn new() -> io::Result<Self> {
        // SAFETY: unnamed, non-inheritable Job and a fully initialized limits structure.
        let job = Self(handle(unsafe { CreateJobObjectW(null(), null()) })?);
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_FLAGS,
                ActiveProcessLimit: JOB_PROCESSES,
                ..Default::default()
            },
            ProcessMemoryLimit: JOB_PROCESS_MEMORY,
            JobMemoryLimit: JOB_MEMORY,
            ..Default::default()
        };
        succeeded(unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            )
        })?;
        job.verify_limits()?;
        Ok(job)
    }

    fn verify_limits(&self) -> io::Result<()> {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: output size matches the documented information class.
        succeeded(unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
                null_mut(),
            )
        })?;
        must(
            limits.BasicLimitInformation.LimitFlags == JOB_FLAGS
                && limits.BasicLimitInformation.ActiveProcessLimit == JOB_PROCESSES
                && limits.ProcessMemoryLimit == JOB_PROCESS_MEMORY
                && limits.JobMemoryLimit == JOB_MEMORY,
        )
    }

    fn contains(&self, process: &OwnedHandle) -> io::Result<bool> {
        let mut member = 0;
        // SAFETY: both identities are held by live handles, with BOOL output storage.
        succeeded(unsafe {
            IsProcessInJob(process.as_raw_handle(), self.0.as_raw_handle(), &mut member)
        })?;
        Ok(member != 0)
    }

    fn active(&self) -> io::Result<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: output size matches the documented information class.
        succeeded(unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of_val(&accounting) as u32,
                null_mut(),
            )
        })?;
        Ok(accounting.ActiveProcesses)
    }
}

struct Attributes {
    _buffer: Vec<usize>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl Attributes {
    fn new() -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: initial size query with null output by API contract.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size) };
        must(size > 0 && size <= 65536)?;
        let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>())];
        let list = buffer.as_mut_ptr().cast();
        // SAFETY: aligned storage meeting the exact size query.
        succeeded(unsafe { InitializeProcThreadAttributeList(list, 2, 0, &mut size) })?;
        Ok(Self {
            _buffer: buffer,
            list,
        })
    }

    fn insert(&mut self, kind: u32, handles: &[HANDLE]) -> io::Result<()> {
        // SAFETY: caller keeps each handle array live and unmoved through process creation.
        succeeded(unsafe {
            UpdateProcThreadAttribute(
                self.list,
                0,
                kind as usize,
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
        // SAFETY: one successful initialization and storage remains live here.
        unsafe { DeleteProcThreadAttributeList(self.list) };
    }
}

struct Child {
    process: OwnedHandle,
    thread: OwnedHandle,
}

#[derive(Debug)]
enum SpawnError {
    JobAttribute(io::Error),
    Setup(io::Error),
    Creation(io::Error),
}

impl SpawnError {
    fn outcome(&self) -> ScriptOutcome {
        match self {
            Self::JobAttribute(error) => {
                let _ = error.raw_os_error();
                ScriptOutcome::Unavailable
            }
            Self::Creation(error)
                if matches!(
                    error.raw_os_error().map(|code| code as u32),
                    Some(
                        ERROR_ACCESS_DENIED
                            | ERROR_PRIVILEGE_NOT_HELD
                            | ERROR_INVALID_PARAMETER
                            | ERROR_NOT_SUPPORTED
                            | ERROR_BAD_TOKEN_TYPE
                            | ERROR_EXE_MACHINE_TYPE_MISMATCH
                    )
                ) =>
            {
                ScriptOutcome::Unavailable
            }
            Self::Setup(error) | Self::Creation(error) => {
                let _ = error.raw_os_error();
                ScriptOutcome::LaunchFailed
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Identity<'a> {
    Restricted(&'a OwnedHandle),
    #[cfg(test)]
    TestCurrent,
}

fn create_suspended(
    job: &Job,
    input: &OwnedHandle,
    output: &OwnedHandle,
    command: &CommandLine,
    identity: Identity<'_>,
) -> Result<Child, SpawnError> {
    let inherited = [input.as_raw_handle(), output.as_raw_handle()];
    let jobs = [job.0.as_raw_handle()];
    let mut attributes = Attributes::new().map_err(SpawnError::Setup)?;
    attributes
        .insert(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &inherited)
        .map_err(SpawnError::Setup)?;
    attributes
        .insert(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)
        .map_err(SpawnError::JobAttribute)?;
    let start = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: size_of::<STARTUPINFOEXW>() as u32,
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: inherited[0],
            hStdOutput: inherited[1],
            hStdError: inherited[1],
            ..Default::default()
        },
        lpAttributeList: attributes.list,
    };
    let mut command_line = command.command.clone();
    let flags = CREATE_SUSPENDED
        | CREATE_NO_WINDOW
        | CREATE_UNICODE_ENVIRONMENT
        | EXTENDED_STARTUPINFO_PRESENT;
    let mut out = PROCESS_INFORMATION::default();
    // SAFETY: explicit executable, mutable terminated command line, terminated fixed
    // environment and working directory. Both attribute arrays and all handles
    // survive the call. Inheritance is restricted by the HANDLE_LIST attribute.
    let ok = unsafe {
        match identity {
            Identity::Restricted(token) => CreateProcessAsUserW(
                token.as_raw_handle(),
                command.application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                flags,
                command.env.as_ptr().cast(),
                command.cwd.as_ptr(),
                &start.StartupInfo,
                &mut out,
            ),
            #[cfg(test)]
            Identity::TestCurrent => CreateProcessW(
                command.application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                flags,
                command.env.as_ptr().cast(),
                command.cwd.as_ptr(),
                &start.StartupInfo,
                &mut out,
            ),
        }
    };
    succeeded(ok).map_err(SpawnError::Creation)?;
    // SAFETY: a successful creation transfers two valid CloseHandle-owned handles.
    Ok(unsafe {
        Child {
            process: OwnedHandle::from_raw_handle(out.hProcess),
            thread: OwnedHandle::from_raw_handle(out.hThread),
        }
    })
}

fn inheritable() -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    }
}

fn null_output() -> io::Result<OwnedHandle> {
    // SAFETY: fixed DOS device. Inheritance is explicitly scoped at process creation.
    handle(unsafe {
        CreateFileW(
            wide(r"\\.\NUL").as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &inheritable(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    })
}

fn stdin_pipe(capacity: u32) -> io::Result<(OwnedHandle, OwnedHandle)> {
    static SERIAL: AtomicU32 = AtomicU32::new(0);
    let name = wide(&format!(
        r"\\.\pipe\arx-guest-stdin-{}-{}",
        unsafe { GetCurrentProcessId() },
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let descriptor = Descriptor::from_sddl("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)")?;
    let reader_security = SECURITY_ATTRIBUTES {
        lpSecurityDescriptor: descriptor.0,
        ..inheritable()
    };
    // SAFETY: first private local byte-mode instance. Only its read handle is inherited.
    let reader = handle(unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            capacity,
            0,
            &reader_security,
        )
    })?;
    let writer = handle(unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_WRITE_DATA | SYNCHRONIZE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            null_mut(),
        )
    })?;
    // SAFETY: client already connected; confirm the single expected connection.
    let connected = unsafe { ConnectNamedPipe(reader.as_raw_handle(), null_mut()) } != 0;
    if !connected && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
        return Err(io::Error::last_os_error());
    }
    Ok((reader, writer))
}

enum Prepared {
    NoChild(ScriptOutcome),
    Child {
        job: Job,
        child: Child,
        input: Result<(), TransferError>,
        qualified: bool,
    },
}

fn prepare(
    command: &CommandLine,
    identity: Identity<'_>,
    script: Vec<u8>,
    stop: &OwnedHandle,
    deadline: Instant,
    verify: impl FnOnce(&Child) -> io::Result<()>,
) -> Prepared {
    let interrupted = || {
        if signaled(stop.as_raw_handle(), 0).unwrap_or(true) {
            Some(ScriptOutcome::Terminated)
        } else if Instant::now() >= deadline {
            Some(ScriptOutcome::TimedOut)
        } else {
            None
        }
    };
    if let Some(outcome) = interrupted() {
        return Prepared::NoChild(outcome);
    }
    let Ok(job) = Job::new() else {
        return Prepared::NoChild(ScriptOutcome::Unavailable);
    };
    let Ok((reader, writer)) = stdin_pipe(MAX_SCRIPT as u32) else {
        return Prepared::NoChild(ScriptOutcome::LaunchFailed);
    };
    let Ok(output) = null_output() else {
        return Prepared::NoChild(ScriptOutcome::LaunchFailed);
    };
    if let Some(outcome) = interrupted() {
        return Prepared::NoChild(outcome);
    }
    let child = match create_suspended(&job, &reader, &output, command, identity) {
        Ok(child) => child,
        Err(error) => return Prepared::NoChild(interrupted().unwrap_or_else(|| error.outcome())),
    };
    drop((reader, output));
    let qualified = job.contains(&child.process).unwrap_or(false) && verify(&child).is_ok();
    let size = script.len();
    let input = if qualified {
        pipe_write(&writer, stop, deadline, script).and_then(|count| {
            if count == size {
                Ok(())
            } else {
                Err(TransferError::Os(io::ErrorKind::WriteZero.into()))
            }
        })
    } else {
        Ok(())
    };
    // Closing the writer before handoff gives the bounded wrapper EOF. This
    // producer is joined before the supervisor can resume the child.
    drop(writer);
    Prepared::Child {
        job,
        child,
        input,
        qualified,
    }
}

fn pipe_write(
    writer: &OwnedHandle,
    stop: &OwnedHandle,
    deadline: Instant,
    bytes: Vec<u8>,
) -> Result<usize, TransferError> {
    transfer(
        writer,
        stop,
        deadline,
        bytes,
        |device, bytes, size, count, pending| {
            // SAFETY: transfer holds the write buffer until completion.
            unsafe { WriteFile(device, bytes, size, count, pending) }
        },
    )
    .map(|bytes| bytes.len())
}

trait Cleanup {
    fn kill(&self) -> io::Result<()>;
    fn signaled(&self, wait_ms: u32) -> io::Result<bool>;
    fn active(&self) -> io::Result<u32>;
}

struct Contained<'a> {
    job: &'a Job,
    process: &'a OwnedHandle,
}

impl Cleanup for Contained<'_> {
    fn kill(&self) -> io::Result<()> {
        // SAFETY: retain the exact Job handle through termination and accounting.
        succeeded(unsafe { TerminateJobObject(self.job.0.as_raw_handle(), 1) })
    }

    fn signaled(&self, wait_ms: u32) -> io::Result<bool> {
        signaled(self.process.as_raw_handle(), wait_ms)
    }

    fn active(&self) -> io::Result<u32> {
        self.job.active()
    }
}

fn finish(
    containment: &impl Cleanup,
    deadline: Instant,
    outcome: Result<ScriptOutcome, &'static str>,
) -> Result<ScriptOutcome, &'static str> {
    containment.kill().map_err(|_| CLEANUP_UNKNOWN)?;
    if !containment
        .signaled(millis_left(deadline))
        .map_err(|_| CLEANUP_UNKNOWN)?
    {
        return Err(CLEANUP_UNKNOWN);
    }
    loop {
        if containment.active().map_err(|_| CLEANUP_UNKNOWN)? == 0 {
            return outcome;
        }
        if Instant::now() >= deadline {
            return Err(CLEANUP_UNKNOWN);
        }
        thread::sleep(
            Duration::from_millis(2).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

fn exit_code(process: &OwnedHandle) -> io::Result<u32> {
    let mut code = 0;
    // SAFETY: caller observed signal on this exact process handle before requesting the code.
    succeeded(unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) })?;
    Ok(code)
}

fn finish_prepared(
    prepared: Prepared,
    stop: &OwnedHandle,
    deadline: Instant,
    interrupted: Option<(ScriptOutcome, Instant)>,
) -> Result<ScriptOutcome, &'static str> {
    let Prepared::Child {
        job,
        child,
        input,
        qualified,
    } = prepared
    else {
        let Prepared::NoChild(outcome) = prepared else {
            unreachable!()
        };
        return Ok(interrupted.map_or(outcome, |(outcome, _)| outcome));
    };
    let outcome = if matches!(input, Err(TransferError::Undrained)) {
        Err(CLEANUP_UNKNOWN)
    } else if let Some((outcome, _)) = interrupted {
        Ok(outcome)
    } else if !qualified {
        Ok(ScriptOutcome::Unavailable)
    } else if let Err(error) = input {
        Ok(match error {
            TransferError::Cancelled => ScriptOutcome::Terminated,
            TransferError::Deadline => ScriptOutcome::TimedOut,
            _ => ScriptOutcome::LaunchFailed,
        })
    } else {
        execute_ready(&child, stop, deadline)
    };
    let cleanup_by = interrupted.map_or_else(|| Instant::now() + CLEANUP_BUDGET, |(_, when)| when);
    finish(
        &Contained {
            job: &job,
            process: &child.process,
        },
        cleanup_by,
        outcome,
    )
}

fn execute_ready(
    child: &Child,
    stop: &OwnedHandle,
    deadline: Instant,
) -> Result<ScriptOutcome, &'static str> {
    if signaled(stop.as_raw_handle(), 0).map_err(|_| CLEANUP_UNKNOWN)? {
        return Ok(ScriptOutcome::Terminated);
    }
    if Instant::now() >= deadline {
        return Ok(ScriptOutcome::TimedOut);
    }
    // SAFETY: caller verified the suspended child and Job before this handoff.
    match unsafe { ResumeThread(child.thread.as_raw_handle()) } {
        1 => {}
        u32::MAX => return Ok(ScriptOutcome::LaunchFailed),
        _ => return Err(CLEANUP_UNKNOWN),
    }
    match wait_for(child.process.as_raw_handle(), stop, deadline).map_err(|_| CLEANUP_UNKNOWN)? {
        Wait::Ready => Ok(ScriptOutcome::Exited {
            exit_code: exit_code(&child.process).map_err(|_| CLEANUP_UNKNOWN)?,
        }),
        Wait::Cancelled => Ok(ScriptOutcome::Terminated),
        Wait::Deadline => Ok(ScriptOutcome::TimedOut),
    }
}

fn supervise(
    stop: &OwnedHandle,
    deadline: Instant,
    cleanup_budget: Duration,
    launch: impl FnOnce() -> Prepared + Send + 'static,
) -> Result<ScriptOutcome, &'static str> {
    let Ok(worker) = thread::Builder::new().spawn(launch) else {
        return Ok(ScriptOutcome::LaunchFailed);
    };
    let wait = wait_for(worker.as_raw_handle(), stop, deadline).map_err(|_| CLEANUP_UNKNOWN)?;
    let interrupted = match wait {
        Wait::Ready => None,
        Wait::Cancelled => Some((ScriptOutcome::Terminated, Instant::now() + cleanup_budget)),
        Wait::Deadline => Some((ScriptOutcome::TimedOut, deadline + cleanup_budget)),
    };
    if let Some((_, cleanup_by)) = interrupted
        && !signaled(worker.as_raw_handle(), millis_left(cleanup_by))
            .map_err(|_| CLEANUP_UNKNOWN)?
    {
        // A blocked Win32 creation call has no cancellable handle yet. It can
        // create only a suspended, already contained child. Drop the channel
        // and fail the service; no worker can resume it or inherit the port.
        return Err(CLEANUP_UNKNOWN);
    }
    finish_prepared(join_ready(worker)?, stop, deadline, interrupted)
}

struct Executor<'a> {
    profile: Option<Arc<Profile>>,
    stop: &'a Arc<OwnedHandle>,
    retire: &'a Cell<Option<&'static str>>,
}

fn record_outcome(
    result: Result<ScriptOutcome, &'static str>,
    retire: &Cell<Option<&'static str>>,
) -> ScriptOutcome {
    match result {
        Ok(outcome) => outcome,
        Err(category) => {
            retire.set(Some(category));
            ScriptOutcome::Terminated
        }
    }
}

impl ScriptExecutor for Executor<'_> {
    fn script_execution(&self) -> ScriptExecution {
        if self.profile.is_some() {
            ScriptExecution::Available
        } else {
            ScriptExecution::Unavailable
        }
    }

    fn run_script(&mut self, script: &str, timeout_s: u32) -> ScriptOutcome {
        let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_s));
        let Some(profile) = self.profile.as_ref().map(Arc::clone) else {
            return ScriptOutcome::Unavailable;
        };
        let stop = Arc::clone(self.stop);
        let script = script.as_bytes().to_vec();
        let result = supervise(self.stop, deadline, CLEANUP_BUDGET, move || {
            prepare(
                &profile.command,
                Identity::Restricted(&profile.token),
                script,
                &stop,
                deadline,
                |child| {
                    no_thread_token(child.thread.as_raw_handle())?;
                    let effective = open_token(child.process.as_raw_handle(), TOKEN_QUERY)?;
                    verify_service_child_token(&effective, &profile.service_sid)
                },
            )
        });
        record_outcome(result, self.retire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_guest::{Request, Response, ScriptStatus, read_frame, write_frame};
    use std::fs::File;

    const GUARD: Duration = Duration::from_secs(10);

    #[test]
    fn port_info_obeys_driver_length_and_name_offset() {
        assert_eq!(offset_of!(PortInfo, name), 7);
        assert_eq!(size_of::<PortInfo>(), 8);
        let mut buf = vec![0; 7];
        assert_eq!(port_name(&buf), None);
        buf.push(0);
        assert_eq!(port_name(&buf), Some([].as_slice()));
        buf.remove(7);
        buf.extend(PORT_NAME);
        buf.extend([0, 0, 0xa5]);
        assert_eq!(port_name(&buf), Some(PORT_NAME));
        buf[7] = b'O';
        assert_ne!(port_name(&buf), Some(PORT_NAME));
        buf[7] = b'o';
        let nul = 7 + PORT_NAME.len();
        buf[nul + 1] = 1;
        assert_eq!(port_name(&buf), None);
        buf.truncate(nul + 1);
        assert_eq!(port_name(&buf), None, "the only NUL would be padding");
        assert_eq!(port_name(&[0; 9]), None);
    }

    #[test]
    fn stop_and_shutdown_are_idempotent_during_startup_and_running() {
        for initial in [Phase::Starting, Phase::Running] {
            for code in [SERVICE_CONTROL_STOP, SERVICE_CONTROL_SHUTDOWN] {
                let mut phase = initial;
                let stop = event().unwrap();
                for (command, expected) in [
                    (SERVICE_CONTROL_INTERROGATE, (NO_ERROR, false)),
                    (u32::MAX, (ERROR_CALL_NOT_IMPLEMENTED, false)),
                    (code, (NO_ERROR, true)),
                    (code, (NO_ERROR, false)),
                ] {
                    assert_eq!(on_control(&mut phase, command), expected);
                    if expected.1 {
                        signal(&stop);
                    }
                }
                assert_eq!(phase, Phase::Stopping);
                assert!(signaled(stop.as_raw_handle(), 0).unwrap());
            }
        }
        assert_eq!(
            on_control(&mut Phase::Stopped, SERVICE_CONTROL_STOP),
            (NO_ERROR, false)
        );
    }

    fn request_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(
            &mut bytes,
            &Request::Hello {
                version: 1,
                challenge: "0123456789abcdef0123456789abcdef".into(),
            },
        )
        .unwrap();
        write_frame(
            &mut bytes,
            &Request::RunScript {
                id: 1,
                script: "exit 7".into(),
                timeout_s: 1,
            },
        )
        .unwrap();
        bytes
    }

    #[test]
    fn missing_execution_prerequisite_is_unavailable_on_wire() {
        let stop = Arc::new(event().unwrap());
        let retire = Cell::new(None);
        let mut executor = Executor {
            profile: None,
            stop: &stop,
            retire: &retire,
        };
        let mut wire = Vec::new();
        serve(request_bytes().as_slice(), &mut wire, &mut executor).unwrap();
        let mut wire = wire.as_slice();
        assert!(matches!(
            read_frame::<Response>(&mut wire).unwrap(),
            Some(Response::HelloAck {
                script_execution: ScriptExecution::Unavailable,
                ..
            })
        ));
        assert!(matches!(
            read_frame::<Response>(&mut wire).unwrap(),
            Some(Response::ScriptResult {
                status: ScriptStatus::Unavailable,
                exit_code: None,
                ..
            })
        ));
        assert!(wire.is_empty());
    }

    struct FixtureCleanup {
        killed: Cell<bool>,
        empty_after_kill: bool,
    }

    impl Cleanup for FixtureCleanup {
        fn kill(&self) -> io::Result<()> {
            self.killed.set(true);
            Ok(())
        }

        fn signaled(&self, _: u32) -> io::Result<bool> {
            Ok(true)
        }

        fn active(&self) -> io::Result<u32> {
            Ok(u32::from(!(self.killed.get() && self.empty_after_kill)))
        }
    }

    #[test]
    fn cleanup_requires_job_kill_even_when_the_main_process_already_exited() {
        let cleanup = FixtureCleanup {
            killed: Cell::new(false),
            empty_after_kill: true,
        };
        let result = ScriptOutcome::Exited { exit_code: 259 };
        assert_eq!(finish(&cleanup, Instant::now(), Ok(result)), Ok(result));
    }

    #[test]
    fn cleanup_while_job_still_active_suppresses_the_wire_result() {
        struct WireExecutor<'a>(&'a Cell<Option<&'static str>>);
        impl ScriptExecutor for WireExecutor<'_> {
            fn script_execution(&self) -> ScriptExecution {
                ScriptExecution::Available
            }
            fn run_script(&mut self, _: &str, _: u32) -> ScriptOutcome {
                let cleanup = FixtureCleanup {
                    killed: Cell::new(false),
                    empty_after_kill: false,
                };
                record_outcome(
                    finish(&cleanup, Instant::now(), Ok(ScriptOutcome::TimedOut)),
                    self.0,
                )
            }
        }
        let (read, write) = stdin_pipe(4096).unwrap();
        let stop = event().unwrap();
        let retire = Cell::new(None);
        let port = Port {
            handle: write,
            stop: &stop,
            retire: &retire,
            read_by: Cell::new(None),
            write_by: Cell::new(None),
        };
        assert!(
            serve(
                request_bytes().as_slice(),
                &port,
                &mut WireExecutor(&retire)
            )
            .is_err()
        );
        assert_eq!(retire.get(), Some(CLEANUP_UNKNOWN));
        drop(port);
        let mut read = File::from(read);
        assert!(matches!(
            read_frame::<Response>(&mut read).unwrap(),
            Some(Response::HelloAck { .. })
        ));
        assert!(read_frame::<Response>(&mut read).unwrap().is_none());
    }

    #[test]
    fn stalled_spawn_has_a_deadline_and_cannot_deliver_a_result() {
        let stop = event().unwrap();
        let gate = Arc::new(event().unwrap());
        let entered = Arc::new(event().unwrap());
        let worker_gate = Arc::clone(&gate);
        let worker_entered = Arc::clone(&entered);
        // A zero-duration injected execution budget exercises the production
        // wait against a worker which cannot finish before the explicit release.
        let result = supervise(&stop, Instant::now(), Duration::ZERO, move || {
            signal(&worker_entered);
            assert!(signaled(worker_gate.as_raw_handle(), GUARD.as_millis() as u32).unwrap());
            Prepared::NoChild(ScriptOutcome::Unavailable)
        });
        assert_eq!(result, Err(CLEANUP_UNKNOWN));
        assert!(signaled(entered.as_raw_handle(), GUARD.as_millis() as u32).unwrap());
        signal(&gate);
    }

    #[test]
    fn overlapped_input_timeout_and_stop_cancel_and_drain() {
        for cancel in [false, true] {
            let (_reader, writer) = stdin_pipe(1).unwrap();
            let stop = event().unwrap();
            // The unconsumed pipe cannot admit a single write of this size.
            // Inject expiry/cancel only once WriteFile has returned IO_PENDING.
            let deadline = if cancel {
                Instant::now() + GUARD
            } else {
                Instant::now() + Duration::from_millis(100)
            };
            let result = transfer(
                &writer,
                &stop,
                deadline,
                vec![1; 256 * 1024],
                |h, buf, len, n, ov| {
                    // SAFETY: held, overdemanding write on a connected non-reading endpoint.
                    let pending = unsafe { WriteFile(h, buf, len, n, ov) };
                    assert_eq!(pending, 0);
                    assert_eq!(unsafe { GetLastError() }, ERROR_IO_PENDING);
                    if cancel {
                        signal(&stop);
                        unsafe { SetLastError(ERROR_IO_PENDING) };
                    }
                    pending
                },
            );
            if cancel {
                assert!(
                    matches!(result, Err(TransferError::Cancelled)),
                    "{result:?}"
                );
            } else {
                assert!(matches!(result, Err(TransferError::Deadline)), "{result:?}");
            }
        }
    }

    fn powershell(script: &[u8], expected: u32) {
        let (windows, system) = system_paths().unwrap();
        let command = CommandLine::powershell(&windows, &system, &system).unwrap();
        let stop = Arc::new(event().unwrap());
        let worker_stop = Arc::clone(&stop);
        let script = script.to_vec();
        let deadline = Instant::now() + GUARD;
        let outcome = supervise(&stop, deadline, CLEANUP_BUDGET, move || {
            prepare(
                &command,
                Identity::TestCurrent,
                script,
                &worker_stop,
                deadline,
                |_| Ok(()),
            )
        });
        assert_eq!(
            outcome,
            Ok(ScriptOutcome::Exited {
                exit_code: expected
            })
        );
    }

    #[test]
    fn wrapper_explicit_zero() {
        powershell(b"exit 0", 0);
    }
    #[test]
    fn wrapper_explicit_seven() {
        powershell(b"exit 7", 7);
    }
    #[test]
    fn wrapper_explicit_still_active_value() {
        powershell(b"exit 259", 259);
    }
    #[test]
    fn wrapper_preserves_high_bit() {
        powershell(b"exit -2147483648", 0x8000_0000);
    }
    #[test]
    fn wrapper_evaluates_utf8_multiline_with_implicit_success() {
        powershell("$x = 'é漢'\nif ($x[0] -ne [char]233 -or $x[1] -ne [char]28450) { throw 'fixture' }\n$x".as_bytes(), 0);
    }
    #[test]
    fn wrapper_reports_implicit_error() {
        powershell(b"throw 'fixture'", 1);
    }
    #[test]
    fn wrapper_rejects_invalid_utf8() {
        powershell(b"exit 0\n\xff", 1);
    }

    #[test]
    fn restricted_token_denies_write_to_a_peer_object() {
        enum TokenTest {
            Verified,
            Unsupported(u32),
        }
        let result = (|| -> io::Result<TokenTest> {
            let source = open_token(unsafe { GetCurrentProcess() }, TOKEN_SOURCE_ACCESS)?;
            let token = match restrict_token(&source, vec![]) {
                Ok(token) => token,
                Err(error)
                    if matches!(
                        error.raw_os_error().map(|code| code as u32),
                        Some(ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD)
                    ) =>
                {
                    return Ok(TokenTest::Unsupported(error.raw_os_error().unwrap() as u32));
                }
                Err(error) => return Err(error),
            };
            let probe = impersonation_token(&token)?;
            let name = wide(&format!(r"Local\arx-guest-token-test-{}", unsafe {
                GetCurrentProcessId()
            }));
            let descriptor = Descriptor::from_sddl("D:P(A;;GA;;;OW)S:(ML;;NW;;;LW)")?;
            let attrs = SECURITY_ATTRIBUTES {
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: 0,
                ..inheritable()
            };
            // SAFETY: bounded test-scoped name and valid DACL/MIC descriptors.
            let _peer = handle(unsafe { CreateEventW(&attrs, 1, 0, name.as_ptr()) })?;
            as_restricted(&probe, || -> io::Result<()> {
                // Positive read establishes that the normal SID can access the
                // peer. Only the second write check should deny modification.
                drop(handle(unsafe {
                    OpenEventW(READ_CONTROL, 0, name.as_ptr())
                })?);
                denied(handle(unsafe {
                    OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr())
                }))
            })??;
            Ok(TokenTest::Verified)
        })()
        .unwrap();
        if let TokenTest::Unsupported(code) = result {
            println!("typed skip: restricted token privilege unavailable ({code})");
        }
    }

    const FIXTURE: &str = "platform::windows::tests::job_fixture_process";
    const JOB_MODE: &str = "ARX_GUEST_JOB_TEST_MODE";
    const JOB_READY: &str = "ARX_GUEST_JOB_TEST_READY";
    const JOB_SENTINEL: &str = "ARX_GUEST_JOB_TEST_SENTINEL";

    #[test]
    #[allow(clippy::zombie_processes)] // The supervising test waits on the contained descendant.
    fn job_fixture_process() {
        let Ok(mode) = std::env::var(JOB_MODE) else {
            return;
        };
        if mode == "parent" {
            let out_of_list: usize = std::env::var(JOB_SENTINEL).unwrap().parse().unwrap();
            // SAFETY: exercise whether this inherited numeric value can signal
            // the sentinel. The owning test holds and checks the real event.
            unsafe { SetEvent(out_of_list as HANDLE) };
            let _descendant = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", FIXTURE, "--nocapture"])
                .env(JOB_MODE, "descendant")
                .spawn()
                .unwrap();
        } else {
            assert_eq!(mode, "descendant");
            let name = wide(&std::env::var(JOB_READY).unwrap());
            let ready =
                handle(unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) }).unwrap();
            signal(&ready);
            let parked = event().unwrap();
            assert!(!signaled(parked.as_raw_handle(), GUARD.as_millis() as u32).unwrap());
        }
    }

    fn fixture_launch(job: &Job) -> Option<(Child, OwnedHandle, OwnedHandle)> {
        let name = format!(
            r"Local\arx-guest-job-test-{}-{}",
            unsafe { GetCurrentProcessId() },
            unsafe { GetCurrentThreadId() }
        );
        let ready = handle(unsafe { CreateEventW(null(), 1, 0, wide(&name).as_ptr()) }).unwrap();
        let sentinel = handle(unsafe { CreateEventW(&inheritable(), 1, 0, null()) }).unwrap();
        let (windows, system) = system_paths().unwrap();
        let exe = std::env::current_exe().unwrap();
        let env_block = [
            format!("{JOB_MODE}=parent"),
            format!("{JOB_READY}={name}"),
            format!("{JOB_SENTINEL}={}", sentinel.as_raw_handle() as usize),
            format!("SystemRoot={}", windows.display()),
        ]
        .into_iter()
        .flat_map(|value| wide(&value))
        .chain([0])
        .collect();
        let command = CommandLine {
            application: wide_path(&exe).unwrap(),
            command: wide(&format!(
                "\"{}\" --exact {FIXTURE} --nocapture",
                exe.display()
            )),
            env: env_block,
            cwd: wide_path(&system).unwrap(),
        };
        let (input, _writer) = stdin_pipe(4096).unwrap();
        let outcome = create_suspended(
            job,
            &input,
            &null_output().unwrap(),
            &command,
            Identity::TestCurrent,
        );
        let child = match outcome {
            Ok(child) => child,
            Err(SpawnError::JobAttribute(error))
                if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
                    && !std::env::var("CI")
                        .is_ok_and(|value| value.eq_ignore_ascii_case("true")) =>
            {
                println!("typed skip: creation-time Job attribute denied ({ERROR_ACCESS_DENIED})");
                return None;
            }
            Err(error) => panic!("required Job fixture creation failed: {error:?}"),
        };
        assert!(job.contains(&child.process).unwrap());
        assert_eq!(job.active().unwrap(), 1);
        assert_eq!(unsafe { ResumeThread(child.thread.as_raw_handle()) }, 1);
        assert!(signaled(child.process.as_raw_handle(), GUARD.as_millis() as u32).unwrap());
        assert_eq!(exit_code(&child.process).unwrap(), 0);
        assert!(signaled(ready.as_raw_handle(), GUARD.as_millis() as u32).unwrap());
        assert!(!signaled(sentinel.as_raw_handle(), 0).unwrap());
        Some((child, ready, sentinel))
    }

    fn held_descendant(job: &Job, parent: &Child) -> OwnedHandle {
        let mut list = [0usize; 4];
        // SAFETY: aligned storage fits the header and both known PIDs. A signaled
        // parent can remain in the accounting list while the kernel finalizes it.
        succeeded(unsafe {
            QueryInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectBasicProcessIdList,
                list.as_mut_ptr().cast(),
                size_of_val(&list) as u32,
                null_mut(),
            )
        })
        .unwrap();
        let ptr = list.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        let count = unsafe { (*ptr).NumberOfProcessIdsInList } as usize;
        assert!((1..=2).contains(&count));
        let parent_id = unsafe { GetProcessId(parent.process.as_raw_handle()) } as usize;
        let pids = unsafe {
            std::slice::from_raw_parts(
                list.as_ptr()
                    .cast::<u8>()
                    .add(offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList))
                    .cast::<usize>(),
                count,
            )
        };
        let pid = *pids
            .iter()
            .find(|&&pid| pid != parent_id)
            .expect("descendant in Job") as u32;
        let process =
            handle(unsafe { OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })
                .unwrap();
        assert!(job.contains(&process).unwrap());
        assert!(!signaled(process.as_raw_handle(), 0).unwrap());
        process
    }

    #[test]
    fn native_job_cleanup_kills_descendant_and_observes_zero_active() {
        let job = Job::new().unwrap();
        job.verify_limits().unwrap();
        let Some((child, _ready, _sentinel)) = fixture_launch(&job) else {
            return;
        };
        let descendant = held_descendant(&job, &child);
        let result = finish(
            &Contained {
                job: &job,
                process: &child.process,
            },
            Instant::now() + CLEANUP_BUDGET,
            Ok(ScriptOutcome::Exited { exit_code: 0 }),
        );
        assert_eq!(result, Ok(ScriptOutcome::Exited { exit_code: 0 }));
        assert_eq!(job.active().unwrap(), 0);
        assert!(signaled(descendant.as_raw_handle(), GUARD.as_millis() as u32).unwrap());
    }

    #[test]
    fn native_job_kill_on_close_terminates_descendant_with_process_handle_held() {
        let job = Job::new().unwrap();
        let Some((child, _ready, _sentinel)) = fixture_launch(&job) else {
            return;
        };
        let descendant = held_descendant(&job, &child);
        drop(job);
        assert!(signaled(descendant.as_raw_handle(), GUARD.as_millis() as u32).unwrap());
    }

    #[test]
    fn absolute_deadline_keeps_a_late_ready_child_suspended() {
        let (windows, system) = system_paths().unwrap();
        let command = CommandLine::powershell(&windows, &system, &system).unwrap();
        let job = Job::new().unwrap();
        let stop = event().unwrap();
        let (reader, _writer) = stdin_pipe(4096).unwrap();
        let child = create_suspended(
            &job,
            &reader,
            &null_output().unwrap(),
            &command,
            Identity::TestCurrent,
        )
        .unwrap();
        assert_eq!(
            finish_prepared(
                Prepared::Child {
                    job,
                    child,
                    input: Ok(()),
                    qualified: true
                },
                &stop,
                Instant::now(),
                None
            ),
            Ok(ScriptOutcome::TimedOut)
        );
    }

    #[test]
    fn wrapper_environment_contains_only_fixed_paths() {
        let (windows, system) = system_paths().unwrap();
        let command = CommandLine::powershell(&windows, &system, &system).unwrap();
        let values = command.env.split(|&unit| unit == 0);
        let keys: Vec<_> = values
            .filter(|value| !value.is_empty())
            .map(|value| {
                String::from_utf16(value)
                    .unwrap()
                    .split_once('=')
                    .unwrap()
                    .0
                    .to_owned()
            })
            .collect();
        assert_eq!(
            keys,
            [
                "PATH",
                "PSModulePath",
                "SystemRoot",
                "TEMP",
                "TMP",
                "windir"
            ]
        );
        assert!(command.env.ends_with(&[0, 0]));
        let script = String::from_utf16(&command.command).unwrap();
        assert!(
            script.contains("-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command")
        );
    }
}

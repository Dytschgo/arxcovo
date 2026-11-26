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
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_DWORD, RRF_NOEXPAND, RRF_RT_ANY, RRF_RT_REG_DWORD,
    RegGetValueW,
};
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::SystemInformation::{GetSystemDirectoryW, GetWindowsDirectoryW};
use windows_sys::Win32::System::SystemServices::{
    ACCESS_ALLOWED_ACE_TYPE, MAXIMUM_ALLOWED, SECURITY_MANDATORY_LOW_RID,
    SECURITY_MANDATORY_SYSTEM_RID,
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
const CLEANUP_BUDGET: Duration = Duration::from_secs(2);
const SESSION_BACKOFF: Duration = Duration::from_millis(250);
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
    wait_millis(ready, cancel, millis_left(deadline))
}

/// `INFINITE` is only reachable through an explicit `None` deadline, because
/// `millis_left` saturates below it.
fn wait_until(ready: HANDLE, cancel: &OwnedHandle, deadline: Option<Instant>) -> io::Result<Wait> {
    wait_millis(ready, cancel, deadline.map_or(INFINITE, millis_left))
}

fn wait_millis(ready: HANDLE, cancel: &OwnedHandle, millis: u32) -> io::Result<Wait> {
    let handles = [ready, cancel.as_raw_handle()];
    // SAFETY: the caller retains both live waitable handles.
    match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, millis) } {
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
            // The self-test and its cleanup must finish inside the start budget.
            let self_test_by = deadline - 2 * CLEANUP_BUDGET;
            let profile = match Profile::for_service() {
                Ok(profile) => admit(profile, &read_policy(), |profile| {
                    run_profile(profile, &stop, SELF_TEST.as_bytes().to_vec(), self_test_by)
                })?,
                Err(_) => None,
            };
            let port = discover_until(&stop, deadline)?;
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
        .spawn(move || {
            serve_sessions(
                &stop,
                port,
                SESSION_BACKOFF,
                || discover_until(&stop, Instant::now() + START_BUDGET),
                |port| {
                    let session = profile
                        .as_ref()
                        .filter(|profile| profile.check_port(&port).is_ok())
                        .cloned();
                    serve_port(port, session, &stop)
                },
            )
        })
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
            Some(deadline.min(Instant::now() + IO_BUDGET)),
            vec![0; MAX_PORT_INFO],
            |device, bytes, size, pending| {
                // SAFETY: transfer holds the output buffer and OVERLAPPED through completion.
                unsafe {
                    DeviceIoControl(
                        device,
                        PORT_INFO_IOCTL,
                        null(),
                        0,
                        bytes.cast(),
                        size,
                        null_mut(),
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

/// The two allocations Windows may write while an operation is in flight. They
/// are held only as raw pointers from `Box::into_raw`. No `&` or `&mut` to them
/// exists between submission and confirmed completion.
struct InFlight {
    overlapped: *mut OVERLAPPED,
    buffer: *mut [u8],
}

impl InFlight {
    fn new(event: HANDLE, bytes: Vec<u8>) -> Self {
        Self {
            overlapped: Box::into_raw(Box::new(OVERLAPPED {
                hEvent: event,
                ..Default::default()
            })),
            buffer: Box::into_raw(bytes.into_boxed_slice()),
        }
    }

    /// Takes back both allocations.
    ///
    /// # Safety
    /// The operation never started, or its completion has been observed through
    /// the private event, so the kernel no longer references either allocation.
    unsafe fn reclaim(self) -> Vec<u8> {
        // SAFETY: both pointers came from Box::into_raw and are reclaimed exactly once.
        unsafe {
            drop(Box::from_raw(self.overlapped));
            Box::from_raw(self.buffer).into_vec()
        }
    }
}

/// Starts one overlapped operation and waits for it. `None` waits without a
/// deadline, only for completion or cancellation. The start callback must pass
/// a null byte-count pointer, as Microsoft recommends for overlapped calls; the
/// count always comes from `GetOverlappedResult`.
fn transfer(
    device: &OwnedHandle,
    cancel: &OwnedHandle,
    deadline: Option<Instant>,
    bytes: Vec<u8>,
    start: impl FnOnce(HANDLE, *mut u8, u32, *mut OVERLAPPED) -> i32,
) -> Result<Vec<u8>, TransferError> {
    if signaled(cancel.as_raw_handle(), 0).map_err(TransferError::Os)? {
        return Err(TransferError::Cancelled);
    }
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(TransferError::Deadline);
    }
    let event = event().map_err(TransferError::Os)?;
    let io = InFlight::new(event.as_raw_handle(), bytes);
    let capacity = io.buffer.len();
    let device = device.as_raw_handle();
    if start(
        device,
        io.buffer.cast::<u8>(),
        capacity as u32,
        io.overlapped,
    ) == 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            // SAFETY: the operation did not start, so nothing references the allocations.
            drop(unsafe { io.reclaim() });
            return Err(TransferError::Os(error));
        }
        let wait = wait_until(event.as_raw_handle(), cancel, deadline);
        if !matches!(wait, Ok(Wait::Ready)) {
            // SAFETY: the raw pointer used at submission names only this
            // operation. A normal completion can race the cancellation.
            unsafe { CancelIoEx(device, io.overlapped) };
            if !signaled(event.as_raw_handle(), CLEANUP_BUDGET.as_millis() as u32).unwrap_or(false)
            {
                // The kernel may still write both allocations and signal the
                // event. Relinquish all three without forming a reference:
                // `InFlight` has no Drop, so returning leaks its allocations. The
                // service exits on Undrained and the OS reclaims them.
                std::mem::forget(event);
                return Err(TransferError::Undrained);
            }
        }
        match wait {
            Ok(Wait::Ready) => {}
            other => {
                // SAFETY: the private event signaled after CancelIoEx: completed.
                drop(unsafe { io.reclaim() });
                return Err(match other {
                    Ok(Wait::Cancelled) => TransferError::Cancelled,
                    Ok(Wait::Deadline) => TransferError::Deadline,
                    Err(error) => TransferError::Os(error),
                    Ok(Wait::Ready) => unreachable!(),
                });
            }
        }
    }
    let mut count = 0;
    // SAFETY: the operation completed synchronously or its private event
    // signaled; the raw OVERLAPPED pointer is still the submitted one.
    let result = succeeded(unsafe { GetOverlappedResult(device, io.overlapped, &mut count, 0) });
    // SAFETY: completion was observed above, so the kernel is done with both.
    let mut bytes = unsafe { io.reclaim() };
    result.map_err(TransferError::Os)?;
    if count as usize > bytes.len() {
        return Err(TransferError::Os(io::ErrorKind::InvalidData.into()));
    }
    bytes.truncate(count as usize);
    Ok(bytes)
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
        // Between frames the host may stay silent indefinitely: it sends no
        // keepalives, like the Linux adapter's host. Only cancellation ends
        // that wait. A started frame keeps its bounded deadline.
        let frame_deadline = self.read_by.get();
        let bytes = self.result(transfer(
            &self.handle,
            self.stop,
            frame_deadline,
            vec![0; dst.len()],
            |device, bytes, size, pending| {
                // SAFETY: transfer holds the mutable read buffer until completion.
                unsafe { ReadFile(device, bytes, size, null_mut(), pending) }
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
            Some(deadline),
            src.to_vec(),
            |device, bytes, size, pending| {
                // SAFETY: transfer holds the write buffer until completion.
                unsafe { WriteFile(device, bytes, size, null_mut(), pending) }
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

/// Serves one port session. `Ok` means the session ended without retiring:
/// host EOF, local port invalidation or cancellation.
fn serve_port(
    port: OwnedHandle,
    profile: Option<Arc<Profile>>,
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
        profile,
        stop,
        retire: &retire,
        policy: read_policy,
    };
    let _ = serve(&port, &port, &mut executor);
    drop(port);
    retire.get().map_or(Ok(()), Err)
}

/// Serves sessions until cancellation. After a session ends without retiring,
/// the port handle is released and bounded discovery runs again. A retired
/// session or failed rediscovery fails the service.
fn serve_sessions(
    stop: &OwnedHandle,
    first: OwnedHandle,
    backoff: Duration,
    mut discover: impl FnMut() -> Result<OwnedHandle, &'static str>,
    mut session: impl FnMut(OwnedHandle) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    let mut port = first;
    loop {
        session(port)?;
        let pause = backoff.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        if signaled(stop.as_raw_handle(), pause).map_err(|_| "service cancellation unavailable")? {
            return Ok(());
        }
        port = match discover() {
            Ok(port) => port,
            Err(_) if signaled(stop.as_raw_handle(), 0).unwrap_or(false) => return Ok(()),
            Err(category) => return Err(category),
        };
    }
}

const JOB_PROCESSES: u32 = 16;
const JOB_PROCESS_MEMORY: usize = 256 * 1024 * 1024;
const JOB_MEMORY: usize = 512 * 1024 * 1024;
const JOB_FLAGS: u32 = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
    | JOB_OBJECT_LIMIT_PROCESS_MEMORY
    | JOB_OBJECT_LIMIT_JOB_MEMORY;
// The wrapper uses no cmdlets, so it never triggers module discovery. Windows
// PowerShell 5.1 appends Program Files and Documents to any inherited
// PSModulePath. Resetting it here keeps the caller's command discovery
// system-only ($PSHOME\Modules).
const WRAPPER: &str = "$ErrorActionPreference='Stop'; try { \
    $env:PSModulePath=[IO.Path]::Combine($PSHOME,'Modules'); \
    $inputStream=[Console]::OpenStandardInput(); \
    $bytes=[byte[]]::new(4097); $length=0; \
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

fn integrity_rid(token: &OwnedHandle) -> io::Result<u32> {
    let buffer = token_buffer(token, TokenIntegrityLevel)?;
    // SAFETY: TokenIntegrityLevel embeds one mandatory-label SID in the live buffer.
    let label = unsafe { &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    let sid = label.Label.Sid;
    must(unsafe { IsValidSid(sid) } != 0 && unsafe { *GetSidSubAuthorityCount(sid) } == 1)?;
    Ok(unsafe { *GetSidSubAuthority(sid, 0) })
}

/// SCM starts LocalService processes at System integrity. Medium or High would
/// mean the binary is not running as the provisioned built-in service.
fn authority_integrity_ok(rid: u32) -> bool {
    rid == SECURITY_MANDATORY_SYSTEM_RID as u32
}

fn privilege_entries(buffer: &[usize]) -> io::Result<&[LUID_AND_ATTRIBUTES]> {
    // SAFETY: TokenPrivileges starts with a count and a flexible array. Check its extent.
    let count = unsafe { *buffer.as_ptr().cast::<u32>() } as usize;
    let offset = offset_of!(TOKEN_PRIVILEGES, Privileges);
    must(offset + count * size_of::<LUID_AND_ATTRIBUTES>() <= size_of_val(buffer))?;
    Ok(unsafe {
        std::slice::from_raw_parts(
            buffer
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<LUID_AND_ATTRIBUTES>(),
            count,
        )
    })
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
    must(privilege_entries(&buffer)?.iter().all(|entry| {
        allowed
            .iter()
            .any(|luid| entry.Luid.HighPart == luid.HighPart && entry.Luid.LowPart == luid.LowPart)
    }))
}

fn sid_string(sid: PSID) -> io::Result<String> {
    let mut raw = null_mut();
    // SAFETY: valid SID from a live token or SID buffer; Windows allocates the string.
    succeeded(unsafe { ConvertSidToStringSidW(sid, &mut raw) })?;
    // SAFETY: Windows returned a NUL-terminated LocalAlloc string.
    let text = unsafe {
        let len = (0..256).position(|i| *raw.add(i) == 0);
        let text = len.map(|len| String::from_utf16(std::slice::from_raw_parts(raw, len)));
        LocalFree(raw.cast());
        text
    };
    match text {
        Some(Ok(text)) if text.starts_with("S-1-") => Ok(text),
        _ => Err(refused()),
    }
}

fn token_user_string(token: &OwnedHandle) -> io::Result<String> {
    let buffer = token_buffer(token, TokenUser)?;
    // SAFETY: TokenUser embeds a SID pointer into the live aligned buffer.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    sid_string(user.User.Sid)
}

fn token_owner_is_user(token: &OwnedHandle) -> io::Result<bool> {
    let owner = token_buffer(token, TokenOwner)?;
    let user = token_buffer(token, TokenUser)?;
    // SAFETY: both classes embed one SID pointer into their live aligned buffers.
    Ok(unsafe {
        EqualSid(
            (*owner.as_ptr().cast::<TOKEN_OWNER>()).Owner,
            (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid,
        ) != 0
    })
}

/// The script token's default DACL: SYSTEM and the token user only. Without it
/// a child can fail console start-up (0xC0000142) on objects it creates itself.
fn default_dacl_sddl(user: &str) -> String {
    format!("D:(A;;GA;;;SY)(A;;GA;;;{user})")
}

fn verify_default_dacl(token: &OwnedHandle) -> io::Result<()> {
    let buffer = token_buffer(token, TokenDefaultDacl)?;
    let user = token_user_string(token)?;
    // SAFETY: TokenDefaultDacl is one ACL pointer into the live aligned buffer.
    let acl = unsafe { (*buffer.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl };
    must(!acl.is_null() && unsafe { (*acl).AceCount } == 2)?;
    let mut seen = [false; 2];
    for index in 0..2 {
        let mut ace = null_mut();
        // SAFETY: index is below the checked ACE count of this retained ACL.
        succeeded(unsafe { GetAce(acl, index, &mut ace) })?;
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        must(
            u32::from(header.AceType) == ACCESS_ALLOWED_ACE_TYPE
                && header.AceFlags == 0
                && usize::from(header.AceSize) >= offset_of!(ACCESS_ALLOWED_ACE, SidStart) + 12,
        )?;
        let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = (&ace.SidStart as *const u32).cast_mut().cast();
        must(ace.Mask == GENERIC_ALL && unsafe { IsValidSid(sid) } != 0)?;
        let text = sid_string(sid)?;
        let slot = if text == "S-1-5-18" {
            0
        } else if text == user {
            1
        } else {
            return Err(refused());
        };
        must(!std::mem::replace(&mut seen[slot], true))?;
    }
    Ok(())
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

fn elevation_type_ok(kind: u32) -> bool {
    kind == TokenElevationTypeDefault as u32
}

fn no_linked_token(token: &OwnedHandle) -> io::Result<()> {
    // Primary check: only a default-type token has no UAC split partner.
    must(elevation_type_ok(token_dword(token, TokenElevationType)?))?;
    // Secondary check: a returned linked handle always refuses. Its failure
    // codes vary by token origin, so they are deliberately not interpreted.
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
    Ok(())
}

/// Groups that become deny-only in every script token. CreateRestrictedToken
/// ignores entries the source token does not contain.
const DENY_ONLY_GROUPS: [WELL_KNOWN_SID_TYPE; 4] = [
    WinBuiltinAdministratorsSid,
    WinLocalAccountAndAdministratorSid,
    WinBuiltinPowerUsersSid,
    WinBuiltinBackupOperatorsSid,
];

/// ADR 0059 script identity: a Low-integrity primary token derived from the
/// caller's token, with sensitive groups deny-only, every privilege deleted and
/// an explicit default DACL. There are no restricting SIDs: the designed
/// WRITE_RESTRICTED profile cannot start the PowerShell CLR (0x80070005).
fn restrict_token(source: &OwnedHandle, deny_only: &[PSID]) -> io::Result<OwnedHandle> {
    let mut groups = Vec::with_capacity(DENY_ONLY_GROUPS.len());
    for kind in DENY_ONLY_GROUPS {
        groups.push(sid(kind)?);
    }
    let disabled: Vec<_> = groups
        .iter_mut()
        .map(|group| -> PSID { group.as_mut_ptr().cast() })
        .chain(deny_only.iter().copied())
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: 0,
        })
        .collect();
    let privileges = token_buffer(source, TokenPrivileges)?;
    let delete = privilege_entries(&privileges)?;
    let mut raw = null_mut();
    // SAFETY: all SID buffers and arrays live until CreateRestrictedToken copies them.
    succeeded(unsafe {
        CreateRestrictedToken(
            source.as_raw_handle(),
            0,
            disabled.len() as u32,
            disabled.as_ptr(),
            delete.len() as u32,
            delete.as_ptr(),
            0,
            null(),
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
    // An elevated administrator token defaults object ownership to
    // BUILTIN\Administrators, which is deny-only here. The script would then
    // create objects owned by a SID that grants it nothing. Own as the user.
    let user = token_buffer(&token, TokenUser)?;
    let owner = TOKEN_OWNER {
        // SAFETY: TokenUser embeds a SID pointer into the live aligned buffer.
        Owner: unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid },
    };
    // SAFETY: the owner SID lives in `user`, which outlives this copying call.
    succeeded(unsafe {
        SetTokenInformation(
            token.as_raw_handle(),
            TokenOwner,
            (&owner as *const TOKEN_OWNER).cast(),
            size_of_val(&owner) as u32,
        )
    })?;
    let dacl = Descriptor::from_sddl(&default_dacl_sddl(&token_user_string(&token)?))?;
    let default = TOKEN_DEFAULT_DACL {
        DefaultDacl: dacl.dacl()?,
    };
    // SAFETY: the ACL is owned by `dacl`, which outlives this copying call.
    succeeded(unsafe {
        SetTokenInformation(
            token.as_raw_handle(),
            TokenDefaultDacl,
            (&default as *const TOKEN_DEFAULT_DACL).cast(),
            size_of_val(&default) as u32,
        )
    })?;
    verify_restrictions(&token)?;
    Ok(token)
}

fn verify_restrictions(token: &OwnedHandle) -> io::Result<()> {
    // ADR 0059: no restricting SIDs, so the write check is MIC, not a second pass.
    must(unsafe { IsTokenRestricted(token.as_raw_handle()) } == 0)?;
    must(token_dword(token, TokenType)? == TokenPrimary as u32)?;
    must(integrity_rid(token)? == SECURITY_MANDATORY_LOW_RID as u32)?;
    only_privileges(token, &[])?;
    must(token_owner_is_user(token)?)?;
    for kind in DENY_ONLY_GROUPS {
        let mut group = sid(kind)?;
        must(
            group_attributes(token, TokenGroups, group.as_mut_ptr().cast())?
                .is_none_or(|flags| flags & GROUP_DENY_ONLY != 0 && flags & GROUP_ENABLED == 0),
        )?;
    }
    verify_default_dacl(token)
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

    fn dacl(&self) -> io::Result<*mut ACL> {
        let mut present = 0;
        let mut acl = null_mut();
        let mut defaulted = 0;
        // SAFETY: the returned ACL points into the retained descriptor.
        succeeded(unsafe {
            GetSecurityDescriptorDacl(self.0, &mut present, &mut acl, &mut defaulted)
        })?;
        must(present != 0 && !acl.is_null())?;
        Ok(acl)
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

fn protected_file(path: &Path, token: &OwnedHandle, directory: bool) -> io::Result<OwnedHandle> {
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
    // FILE_MUTATION includes FILE_ADD_FILE and FILE_ADD_SUBDIRECTORY.
    must(security.allowed(token, file_mapping())? & FILE_MUTATION == 0)?;
    Ok(file)
}

/// One private, Low-labelled directory per request. Scripts can create
/// children but cannot delete, rename or re-ACL the directory itself, and the
/// work root above it is not writable by them. Size is not bounded (ADR 0059).
struct Workspace(PathBuf);

const MAX_WORKSPACE_ENTRIES: usize = 4096;
const MAX_WORKSPACE_DEPTH: usize = 32;
const WORKSPACE_UNKNOWN: &str = "script workspace cleanup failed";
// List, add file, add subdirectory, read EA, traverse, delete child, read
// attributes, READ_CONTROL and SYNCHRONIZE on the directory itself only.
const WORKSPACE_DIRECTORY_RIGHTS: u32 = 0x0012_00ef;

fn workspace_sddl(child_user: &str) -> String {
    format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;;{WORKSPACE_DIRECTORY_RIGHTS:#x};;;{child_user})\
         (A;OICIIO;FA;;;{child_user})(A;;RC;;;OW)S:(ML;OICI;NW;;;LW)"
    )
}

/// Why a request directory is not available.
#[derive(Debug, PartialEq, Eq)]
enum CreateError {
    /// Nothing is left on disk.
    Failed,
    /// The directory was created, failed verification and could not be removed.
    Leaked,
}

impl Workspace {
    fn create(launcher: &Launcher) -> Result<Self, CreateError> {
        static SERIAL: AtomicU32 = AtomicU32::new(0);
        let path = launcher.work.join(format!(
            "r{}-{}",
            unsafe { GetCurrentProcessId() },
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let descriptor = Descriptor::from_sddl(&workspace_sddl(&launcher.child_user))
            .map_err(|_| CreateError::Failed)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let name = wide_path(&path).map_err(|_| CreateError::Failed)?;
        // SAFETY: fixed absolute path below the verified root and a valid descriptor.
        succeeded(unsafe { CreateDirectoryW(name.as_ptr(), &attributes) })
            .map_err(|_| CreateError::Failed)?;
        let verified = checked_file(&path, true).and_then(|directory| {
            Descriptor::object(
                directory.as_raw_handle(),
                SE_FILE_OBJECT,
                LABEL_SECURITY_INFORMATION,
            )?
            .low_label()
        });
        match verified {
            Ok(()) => Ok(Self(path)),
            // Nothing has run inside it yet, so an empty-directory removal suffices.
            Err(_) if std::fs::remove_dir(&path).is_ok() => Err(CreateError::Failed),
            Err(_) => Err(CreateError::Leaked),
        }
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Bounded removal that never follows a reparse point. Only call this
    /// after Job cleanup is verified, so no script can race the walk.
    fn remove(self) -> io::Result<()> {
        let mut budget = MAX_WORKSPACE_ENTRIES;
        remove_tree(&self.0, 0, &mut budget)
    }
}

fn remove_tree(path: &Path, depth: usize, budget: &mut usize) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    must(depth <= MAX_WORKSPACE_DEPTH)?;
    for entry in std::fs::read_dir(path)? {
        *budget = budget.checked_sub(1).ok_or_else(refused)?;
        let entry = entry?.path();
        // symlink_metadata opens the reparse point itself, never its target.
        let attributes = std::fs::symlink_metadata(&entry)?.file_attributes();
        if attributes & FILE_ATTRIBUTE_READONLY != 0 {
            let name = wide_path(&entry)?;
            // SAFETY: fixed absolute path of an entry inside this workspace.
            succeeded(unsafe { SetFileAttributesW(name.as_ptr(), FILE_ATTRIBUTE_NORMAL) })?;
        }
        if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            std::fs::remove_file(&entry)?;
        } else if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            // Removes the junction or link itself, whatever its target holds.
            std::fs::remove_dir(&entry)?;
        } else {
            remove_tree(&entry, depth + 1, budget)?;
        }
    }
    std::fs::remove_dir(path)
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
        // Fixed values only, sorted case-insensitively. Every per-user location
        // (profile, application data and the module analysis cache) points into
        // the private Low-labelled request directory, so a fresh or unwritable
        // account profile is never touched and everything is removed afterwards.
        let mut env = Vec::new();
        for entry in [
            format!(r"APPDATA={}\AppData\Roaming", work.display()),
            format!(r"LOCALAPPDATA={}\AppData\Local", work.display()),
            format!("PATH={}", system.display()),
            format!(
                r"PSModuleAnalysisCachePath={}\ModuleAnalysisCache",
                work.display()
            ),
            format!(
                r"PSModulePath={}\WindowsPowerShell\v1.0\Modules",
                system.display()
            ),
            format!("SystemRoot={}", windows.display()),
            format!("TEMP={}", work.display()),
            format!("TMP={}", work.display()),
            format!("USERPROFILE={}", work.display()),
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

/// How a suspended child's effective token is verified before resume.
enum ChildCheck {
    Service([u32; 17]),
    #[cfg(test)]
    Restrictions,
}

/// Fixed launch inputs. Each request gets its own directory under `work`.
struct Launcher {
    windows: PathBuf,
    system: PathBuf,
    work: PathBuf,
    /// SID string of the script token's user, granted inside request directories.
    child_user: String,
    /// The only SID that may open the client end of a stdin pipe.
    pipe_client: [u32; 17],
}

struct Profile {
    token: OwnedHandle,
    probe: OwnedHandle,
    check: ChildCheck,
    launcher: Launcher,
    _held_paths: Vec<OwnedHandle>,
}

impl Profile {
    fn verify_child(&self, child: &Child) -> io::Result<()> {
        no_thread_token(child.thread.as_raw_handle())?;
        let effective = open_token(child.process.as_raw_handle(), TOKEN_QUERY)?;
        match &self.check {
            ChildCheck::Service(service_sid) => verify_service_child_token(&effective, service_sid),
            #[cfg(test)]
            ChildCheck::Restrictions => verify_restrictions(&effective),
        }
    }

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
        must(authority_integrity_ok(integrity_rid(&authority)?))?;
        no_linked_token(&authority)?;
        only_privileges(
            &authority,
            &["SeChangeNotifyPrivilege", "SeIncreaseQuotaPrivilege"],
        )?;

        let token = restrict_token(&authority, &[service_sid.as_ptr().cast_mut().cast()])?;
        verify_service_child_token(&token, &service_sid)?;
        let probe = impersonation_token(&token)?;
        protect_parent(&probe, &authority)?;

        let (windows, system) = system_paths()?;
        let installed = fixed_path(|out, cap| unsafe { GetModuleFileNameW(null_mut(), out, cap) })?;
        let dir = installed.parent().ok_or_else(refused)?;
        let work = dir.join("work");
        let shell = system.join(r"WindowsPowerShell\v1.0\powershell.exe");

        // Walk from the volume root so an intermediate reparse point cannot
        // redirect the path. Retain every opened component without delete sharing.
        let mut held = Vec::new();
        for path in [&installed, &shell, &work] {
            let parents: Vec<_> = path.ancestors().skip(1).collect();
            for ancestor in parents.into_iter().rev() {
                held.push(checked_file(ancestor, true)?);
            }
        }
        held.push(protected_file(dir, &probe, true)?);
        held.push(protected_file(&installed, &probe, false)?);
        // The work root is not writable by scripts. The service creates one
        // Low-labelled directory per request inside it (see Workspace).
        held.push(protected_file(&work, &probe, true)?);
        held.push(checked_file(&shell, false)?);
        check_scm(&probe, &installed)?;
        let child_user = token_user_string(&token)?;
        Ok(Self {
            token,
            probe,
            check: ChildCheck::Service(service_sid),
            launcher: Launcher {
                windows,
                system,
                work,
                child_user,
                pipe_client: service_sid,
            },
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
#[allow(dead_code)] // The raw errors are carried for Debug diagnostics only.
enum SpawnError {
    JobAttribute(io::Error),
    Setup(io::Error),
    Creation(io::Error),
}

impl SpawnError {
    /// Only a refused privilege or access check means the execution
    /// prerequisite is closed. Every other failure, including invalid
    /// parameters, is an adapter or launch fault and must not be hidden as
    /// `Unavailable`. No child survives any of these errors.
    fn outcome(&self) -> ScriptOutcome {
        match self {
            Self::JobAttribute(error) | Self::Creation(error)
                if matches!(
                    error.raw_os_error().map(|code| code as u32),
                    Some(ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD)
                ) =>
            {
                ScriptOutcome::Unavailable
            }
            Self::JobAttribute(_) | Self::Setup(_) | Self::Creation(_) => {
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

/// Private stdin pipe. Only `client` (the service SID in production, never the
/// LocalService owner shared by other services) may open the client end, and
/// the connected client must be this process.
fn stdin_pipe(capacity: u32, client: &[u32; 17]) -> io::Result<(OwnedHandle, OwnedHandle)> {
    static SERIAL: AtomicU32 = AtomicU32::new(0);
    let name = wide(&format!(
        r"\\.\pipe\arx-guest-stdin-{}-{}",
        unsafe { GetCurrentProcessId() },
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let client = sid_string(client.as_ptr().cast_mut().cast())?;
    let descriptor = Descriptor::from_sddl(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{client})"))?;
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
    let mut pid = 0;
    // SAFETY: connected server handle and initialized output.
    succeeded(unsafe { GetNamedPipeClientProcessId(reader.as_raw_handle(), &mut pid) })?;
    must(pid == unsafe { GetCurrentProcessId() })?;
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
    pipe_client: &[u32; 17],
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
    let Ok((reader, writer)) = stdin_pipe(MAX_SCRIPT as u32, pipe_client) else {
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
        Some(deadline),
        bytes,
        |device, bytes, size, pending| {
            // SAFETY: transfer holds the write buffer until completion.
            unsafe { WriteFile(device, bytes, size, null_mut(), pending) }
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
    /// Read before every request: a policy changed after admission (for
    /// example script block logging turned on) closes that request.
    policy: fn() -> [PolicyValue; POLICY_VALUES.len()],
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
        let Some(profile) = self.profile.as_ref() else {
            return ScriptOutcome::Unavailable;
        };
        // Each request starts a new interpreter, which reads the policy afresh.
        // Re-check before caller text reaches it; unreadable counts as conflict.
        if policy_conflict(&(self.policy)()) {
            return ScriptOutcome::Unavailable;
        }
        let result = run_profile(profile, self.stop, script.as_bytes().to_vec(), deadline);
        record_outcome(result, self.retire)
    }
}

/// Runs one script under the profile identity in a fresh request directory.
/// The directory is removed only after verified Job cleanup; if cleanup is
/// unverified it is left in place and the session retires.
fn run_profile(
    profile: &Arc<Profile>,
    stop: &Arc<OwnedHandle>,
    script: Vec<u8>,
    deadline: Instant,
) -> Result<ScriptOutcome, &'static str> {
    let Some((workspace, command)) = prepare_workspace(profile, stop, deadline, Workspace::create)?
    else {
        return Ok(ScriptOutcome::LaunchFailed);
    };
    let worker_profile = Arc::clone(profile);
    let worker_stop = Arc::clone(stop);
    let outcome = supervise(stop, deadline, CLEANUP_BUDGET, move || {
        let profile = worker_profile;
        prepare(
            &command,
            Identity::Restricted(&profile.token),
            &profile.launcher.pipe_client,
            script,
            &worker_stop,
            deadline,
            |child| profile.verify_child(child),
        )
    })?;
    // Removal gets its own cleanup allowance, never beyond the request's
    // overall cleanup bound. The host allows the budget plus 5 s.
    let removal_by = (Instant::now() + CLEANUP_BUDGET).min(deadline + 2 * CLEANUP_BUDGET);
    remove_workspace(stop, workspace, removal_by, Workspace::remove)?;
    Ok(outcome)
}

enum Bounded<T> {
    Done(T),
    Unstarted,
    Missed,
}

/// The latest instant a cancelled step is still awaited: `cancel_grace` after
/// cancellation, but never past the absolute `limit`.
fn cancel_deadline(cancelled_at: Instant, cancel_grace: Duration, limit: Instant) -> Instant {
    cancelled_at
        .checked_add(cancel_grace)
        .map_or(limit, |grace| grace.min(limit))
}

/// Runs one filesystem step on a worker that holds no port handle and no
/// child. Waits until `by`; after cancellation, at most `cancel_grace` more and
/// never past `limit`. A missed step keeps running detached, and the caller
/// retires the session, so nothing waits on it or uses its result.
fn bounded<T: Send + 'static>(
    stop: &OwnedHandle,
    by: Instant,
    cancel_grace: Duration,
    limit: Instant,
    step: impl FnOnce() -> T + Send + 'static,
) -> Bounded<T> {
    let Ok(worker) = thread::Builder::new().spawn(step) else {
        return Bounded::Unstarted;
    };
    let finished = match wait_for(worker.as_raw_handle(), stop, by.min(limit)) {
        Ok(Wait::Ready) => true,
        Ok(Wait::Cancelled) => signaled(
            worker.as_raw_handle(),
            millis_left(cancel_deadline(Instant::now(), cancel_grace, limit)),
        )
        .unwrap_or(false),
        Ok(Wait::Deadline) | Err(_) => false,
    };
    if !finished {
        return Bounded::Missed;
    }
    join_ready(worker).map_or(Bounded::Missed, Bounded::Done)
}

enum Setup {
    Ready(Workspace, CommandLine),
    Failed,
    /// Created, then could not be removed after a later setup step failed.
    Leaked,
}

/// Creates the request directory and command inside the request budget. A
/// stalled setup, or a directory that cannot be rolled back, retires the
/// session; no child can exist yet.
fn prepare_workspace(
    profile: &Arc<Profile>,
    stop: &OwnedHandle,
    by: Instant,
    create: impl FnOnce(&Launcher) -> Result<Workspace, CreateError> + Send + 'static,
) -> Result<Option<(Workspace, CommandLine)>, &'static str> {
    let worker_profile = Arc::clone(profile);
    let step = move || {
        let launcher = &worker_profile.launcher;
        let workspace = match create(launcher) {
            Ok(workspace) => workspace,
            Err(CreateError::Failed) => return Setup::Failed,
            Err(CreateError::Leaked) => return Setup::Leaked,
        };
        let Ok(command) =
            CommandLine::powershell(&launcher.windows, &launcher.system, workspace.path())
        else {
            return if workspace.remove().is_ok() {
                Setup::Failed
            } else {
                Setup::Leaked
            };
        };
        Setup::Ready(workspace, command)
    };
    match bounded(stop, by, CLEANUP_BUDGET, by + CLEANUP_BUDGET, step) {
        Bounded::Done(Setup::Ready(workspace, command)) => Ok(Some((workspace, command))),
        Bounded::Done(Setup::Failed) | Bounded::Unstarted => Ok(None),
        Bounded::Done(Setup::Leaked) | Bounded::Missed => Err(WORKSPACE_UNKNOWN),
    }
}

/// Removes the request directory by the absolute instant `by`, whatever the
/// cancellation timing, or retires the session.
fn remove_workspace(
    stop: &OwnedHandle,
    workspace: Workspace,
    by: Instant,
    remove: impl FnOnce(Workspace) -> io::Result<()> + Send + 'static,
) -> Result<(), &'static str> {
    match bounded(stop, by, CLEANUP_BUDGET, by, move || remove(workspace)) {
        Bounded::Done(Ok(())) => Ok(()),
        _ => Err(WORKSPACE_UNKNOWN),
    }
}

/// Fixed start-up probe. It proves the production identity can start the CLR
/// in full language mode with system-only module discovery, and can write its
/// request directory, which the cleanup must then remove. It uses no cmdlets,
/// so it needs no module discovery. Machine and user execution policies are
/// the registry values `read_policy` already requires to be absent.
const SELF_TEST: &str = "if ($ExecutionContext.SessionState.LanguageMode -ne 'FullLanguage') { exit 3 }\n\
    if ($env:PSModulePath -ne [IO.Path]::Combine($PSHOME, 'Modules')) { exit 4 }\n\
    [IO.File]::WriteAllText([IO.Path]::Combine($env:TEMP, 'self-test'), 'arx')\n\
    exit 23131";
const SELF_TEST_SENTINEL: u32 = 23131;

/// Advertises script execution only when no policy conflicts and the
/// self-test returned the exact sentinel. A start-up failure under the
/// production identity (0xC0000142, CLR 0xFFFF0000) therefore yields
/// `Unavailable`, never a misleading `Exited`. An unverified self-test
/// cleanup fails the service.
fn admit(
    profile: Profile,
    policy: &[PolicyValue; POLICY_VALUES.len()],
    self_test: impl FnOnce(&Arc<Profile>) -> Result<ScriptOutcome, &'static str>,
) -> Result<Option<Arc<Profile>>, &'static str> {
    if policy_conflict(policy) {
        return Ok(None);
    }
    let profile = Arc::new(profile);
    let passed = self_test(&profile)?
        == ScriptOutcome::Exited {
            exit_code: SELF_TEST_SENTINEL,
        };
    Ok(passed.then_some(profile))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicyValue {
    Absent,
    Dword(u32),
    Other,
    Unreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicyRule {
    /// Any configured value overrides `-ExecutionPolicy Bypass` or the language mode.
    Unset,
    /// Logging that could copy payloads must be absent or explicitly zero.
    OffOrUnset,
    /// Must be present and zero. Windows PowerShell logs "suspicious" script
    /// blocks, including caller text, whenever script block logging is not
    /// configured. Only an explicit `EnableScriptBlockLogging = 0` turns that off.
    ExplicitlyOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hive {
    /// Machine policy. It takes precedence for PowerShell's group policy lookup.
    Machine,
    /// The service account's own user policy, which scripts inherit (same user).
    User,
}

const POWERSHELL_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\PowerShell";
const SCRIPT_BLOCK_POLICY: &str =
    r"SOFTWARE\Policies\Microsoft\Windows\PowerShell\ScriptBlockLogging";
const MODULE_LOGGING_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\PowerShell\ModuleLogging";
const TRANSCRIPTION_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\PowerShell\Transcription";
const POLICY_VALUES: [(Hive, &str, &str, PolicyRule); 13] = [
    (
        Hive::Machine,
        POWERSHELL_POLICY,
        "ExecutionPolicy",
        PolicyRule::Unset,
    ),
    (
        Hive::Machine,
        POWERSHELL_POLICY,
        "EnableScripts",
        PolicyRule::Unset,
    ),
    (
        Hive::User,
        POWERSHELL_POLICY,
        "ExecutionPolicy",
        PolicyRule::Unset,
    ),
    (
        Hive::User,
        POWERSHELL_POLICY,
        "EnableScripts",
        PolicyRule::Unset,
    ),
    (
        Hive::Machine,
        SCRIPT_BLOCK_POLICY,
        "EnableScriptBlockLogging",
        PolicyRule::ExplicitlyOff,
    ),
    (
        Hive::User,
        SCRIPT_BLOCK_POLICY,
        "EnableScriptBlockLogging",
        PolicyRule::OffOrUnset,
    ),
    (
        Hive::Machine,
        SCRIPT_BLOCK_POLICY,
        "EnableScriptBlockInvocationLogging",
        PolicyRule::OffOrUnset,
    ),
    (
        Hive::User,
        SCRIPT_BLOCK_POLICY,
        "EnableScriptBlockInvocationLogging",
        PolicyRule::OffOrUnset,
    ),
    (
        Hive::Machine,
        MODULE_LOGGING_POLICY,
        "EnableModuleLogging",
        PolicyRule::OffOrUnset,
    ),
    (
        Hive::User,
        MODULE_LOGGING_POLICY,
        "EnableModuleLogging",
        PolicyRule::OffOrUnset,
    ),
    (
        Hive::Machine,
        TRANSCRIPTION_POLICY,
        "EnableTranscripting",
        PolicyRule::OffOrUnset,
    ),
    (
        Hive::User,
        TRANSCRIPTION_POLICY,
        "EnableTranscripting",
        PolicyRule::OffOrUnset,
    ),
    (
        Hive::Machine,
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        "__PSLockdownPolicy",
        PolicyRule::Unset,
    ),
];

fn rule_allows(rule: PolicyRule, value: PolicyValue) -> bool {
    matches!(
        (rule, value),
        (
            PolicyRule::Unset | PolicyRule::OffOrUnset,
            PolicyValue::Absent
        ) | (
            PolicyRule::OffOrUnset | PolicyRule::ExplicitlyOff,
            PolicyValue::Dword(0)
        )
    )
}

fn policy_conflict(snapshot: &[PolicyValue; POLICY_VALUES.len()]) -> bool {
    POLICY_VALUES
        .iter()
        .zip(snapshot)
        .any(|((_, _, _, rule), value)| !rule_allows(*rule, *value))
}

fn read_policy() -> [PolicyValue; POLICY_VALUES.len()] {
    POLICY_VALUES.map(|(hive, key, value, _)| read_policy_value(hive, key, value))
}

fn read_policy_value(hive: Hive, key: &str, value: &str) -> PolicyValue {
    let root = match hive {
        Hive::Machine => HKEY_LOCAL_MACHINE,
        Hive::User => HKEY_CURRENT_USER,
    };
    let (key, value) = (wide(key), wide(value));
    let mut kind = 0;
    let mut size = 0;
    // SAFETY: fixed terminated subkey and value names; size query only.
    let found = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_ANY | RRF_NOEXPAND,
            &mut kind,
            null_mut(),
            &mut size,
        )
    };
    match found {
        ERROR_SUCCESS if kind == REG_DWORD => {}
        ERROR_SUCCESS => return PolicyValue::Other,
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => return PolicyValue::Absent,
        _ => return PolicyValue::Unreadable,
    }
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: DWORD-typed read into initialized DWORD storage of the stated size.
    let read = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    if read == ERROR_SUCCESS {
        PolicyValue::Dword(data)
    } else {
        PolicyValue::Unreadable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arx_guest::{Request, Response, ScriptStatus, read_frame, write_frame};
    use std::fs::File;
    use windows_sys::Win32::System::SystemServices::ACCESS_DENIED_ACE_TYPE;

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
        // An otherwise valid production-shaped profile under a conflicting
        // machine policy is never advertised or used.
        let root = TestRoot::new("policy");
        let mut policy = clean_policy();
        policy[0] = PolicyValue::Other;
        let profile = admit(test_profile(&root.0), &policy, |_| {
            Ok(ScriptOutcome::Exited {
                exit_code: SELF_TEST_SENTINEL,
            })
        })
        .unwrap();
        assert!(profile.is_none());
        let mut executor = Executor {
            profile,
            stop: &stop,
            retire: &retire,
            policy: clean_policy,
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

    static POLICY_READS: AtomicU32 = AtomicU32::new(0);

    /// Admission passed, then script block logging was enabled.
    fn logging_enabled_after_admission() -> [PolicyValue; POLICY_VALUES.len()] {
        POLICY_READS.fetch_add(1, Ordering::SeqCst);
        let mut policy = clean_policy();
        let index = POLICY_VALUES
            .iter()
            .position(|(_, _, _, rule)| *rule == PolicyRule::ExplicitlyOff)
            .unwrap();
        policy[index] = PolicyValue::Dword(1);
        policy
    }

    #[test]
    fn policy_change_after_admission_closes_each_request() {
        let stop = Arc::new(event().unwrap());
        let retire = Cell::new(None);
        let root = TestRoot::new("recheck");
        // An admitted production-shaped profile: execution is advertised.
        let profile = admit(test_profile(&root.0), &clean_policy(), |_| {
            Ok(ScriptOutcome::Exited {
                exit_code: SELF_TEST_SENTINEL,
            })
        })
        .unwrap();
        assert!(profile.is_some());
        let mut executor = Executor {
            profile,
            stop: &stop,
            retire: &retire,
            policy: logging_enabled_after_admission,
        };
        let before = POLICY_READS.load(Ordering::SeqCst);
        let mut wire = Vec::new();
        serve(request_bytes().as_slice(), &mut wire, &mut executor).unwrap();
        assert_eq!(executor.run_script("exit 7", 1), ScriptOutcome::Unavailable);
        assert_eq!(
            POLICY_READS.load(Ordering::SeqCst) - before,
            2,
            "read once per request"
        );
        assert_eq!(retire.get(), None);
        assert_eq!(root.entries(), 0, "no request directory was created");
        let mut wire = wire.as_slice();
        assert!(matches!(
            read_frame::<Response>(&mut wire).unwrap(),
            Some(Response::HelloAck {
                script_execution: ScriptExecution::Available,
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
        let (read, write) = stdin_pipe(4096, &current_user_sid()).unwrap();
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
            let (_reader, writer) = stdin_pipe(1, &current_user_sid()).unwrap();
            let stop = event().unwrap();
            // The unconsumed pipe cannot admit a single write of this size.
            // Expiry or cancellation is injected only after WriteFile has
            // returned IO_PENDING, and the test asserts that it did. A preflight
            // Deadline without started I/O cannot pass.
            let deadline = if cancel {
                Instant::now() + GUARD
            } else {
                Instant::now() + Duration::from_millis(250)
            };
            let started = Cell::new(false);
            let result = transfer(
                &writer,
                &stop,
                Some(deadline),
                vec![1; 256 * 1024],
                |h, buf, len, ov| {
                    // SAFETY: held, overdemanding write on a connected non-reading endpoint.
                    let pending = unsafe { WriteFile(h, buf, len, null_mut(), ov) };
                    assert_eq!(pending, 0);
                    assert_eq!(unsafe { GetLastError() }, ERROR_IO_PENDING);
                    started.set(true);
                    if cancel {
                        signal(&stop);
                    } else {
                        // Let the deadline pass while the write is pending.
                        while Instant::now() < deadline {
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                    unsafe { SetLastError(ERROR_IO_PENDING) };
                    pending
                },
            );
            assert!(started.get(), "overlapped I/O started before expiry");
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

    fn current_user_sid() -> [u32; 17] {
        let token = open_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
        let buffer = token_buffer(&token, TokenUser).unwrap();
        // SAFETY: TokenUser embeds a SID pointer into the live aligned buffer.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut sid = [0u32; 17];
        // SAFETY: SECURITY_MAX_SID_SIZE output and a valid source SID.
        assert_ne!(
            unsafe {
                CopySid(
                    size_of_val(&sid) as u32,
                    sid.as_mut_ptr().cast(),
                    user.User.Sid,
                )
            },
            0
        );
        sid
    }

    /// A test-owned work root under the task TMP. Like the production root it
    /// carries no Low label, so scripts cannot write it.
    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(tag: &str) -> Self {
            static SERIAL: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "arx-guest-{tag}-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn entries(&self) -> usize {
            std::fs::read_dir(&self.0).unwrap().count()
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The production identity derived from the runner's own token: the same
    /// `restrict_token`, request directories, pipe DACL and child verification,
    /// minus the LocalService and SCM checks only an owner host can provide.
    fn test_profile(work: &Path) -> Profile {
        let source = open_token(unsafe { GetCurrentProcess() }, TOKEN_SOURCE_ACCESS).unwrap();
        let token = restrict_token(&source, &[]).unwrap();
        let probe = impersonation_token(&token).unwrap();
        let (windows, system) = system_paths().unwrap();
        let child_user = token_user_string(&token).unwrap();
        Profile {
            token,
            probe,
            check: ChildCheck::Restrictions,
            launcher: Launcher {
                windows,
                system,
                work: work.to_owned(),
                child_user,
                pipe_client: current_user_sid(),
            },
            _held_paths: Vec::new(),
        }
    }

    /// PowerShell start-up gets more room than native fixtures: a cold hosted
    /// image can be slow. Only a failure ever waits this long.
    const POWERSHELL_GUARD: Duration = Duration::from_secs(30);

    const HOSTED_POWERSHELL_CRASH: u32 = 0x8013_1623;

    fn is_hosted_win25(ci: Option<&str>, runner: Option<&str>, image: Option<&str>) -> bool {
        ci.is_some_and(|value| value.eq_ignore_ascii_case("true"))
            && runner == Some("github-hosted")
            && image.is_some_and(|value| value.starts_with("win25"))
    }

    fn is_known_hosted_crash(
        outcome: Result<ScriptOutcome, &'static str>,
        ci: Option<&str>,
        runner: Option<&str>,
        image: Option<&str>,
    ) -> bool {
        is_hosted_win25(ci, runner, image)
            && outcome
                == Ok(ScriptOutcome::Exited {
                    exit_code: HOSTED_POWERSHELL_CRASH,
                })
    }

    fn quarantine_hosted_crash(
        outcome: Result<ScriptOutcome, &'static str>,
        expected: ScriptOutcome,
        profile: &Arc<Profile>,
    ) -> bool {
        if outcome == Ok(expected) {
            return false;
        }
        let ci = std::env::var("CI").ok();
        let runner = std::env::var("RUNNER_ENVIRONMENT").ok();
        let image = std::env::var("ImageOS").ok();
        let os = std::env::var("RUNNER_OS").ok();
        eprintln!(
            "restricted PowerShell runner: CI={ci:?} RUNNER_ENVIRONMENT={runner:?} \
             ImageOS={image:?} RUNNER_OS={os:?}"
        );
        let known =
            is_known_hosted_crash(outcome, ci.as_deref(), runner.as_deref(), image.as_deref());
        if known {
            hosted_restricted_powershell_ladder(profile);
        }
        known
    }

    #[test]
    fn hosted_exception_requires_a_win25_image_and_the_exact_crash_exit() {
        let ci = Some("true");
        let hosted = Some("github-hosted");
        let affected = Some("win25");
        let crash = Ok(ScriptOutcome::Exited {
            exit_code: HOSTED_POWERSHELL_CRASH,
        });
        assert!(is_known_hosted_crash(crash, ci, hosted, affected));
        assert!(is_known_hosted_crash(
            crash,
            ci,
            hosted,
            Some("win25-vs2026")
        ));
        for (ci, runner, image) in [
            (None, hosted, affected),
            (ci, None, affected),
            (ci, Some("self-hosted"), affected),
            (ci, hosted, Some("win22")),
            (ci, hosted, Some("win22-vs2026")),
            (ci, hosted, None),
        ] {
            assert!(!is_known_hosted_crash(crash, ci, runner, image));
        }
        for outcome in [
            Ok(ScriptOutcome::Exited { exit_code: 7 }),
            Ok(ScriptOutcome::Exited { exit_code: 3 }),
            Ok(ScriptOutcome::Exited {
                exit_code: 0xc000_0142,
            }),
            Ok(ScriptOutcome::Unavailable),
            Ok(ScriptOutcome::LaunchFailed),
            Ok(ScriptOutcome::TimedOut),
            Err(CLEANUP_UNKNOWN),
            Err(WORKSPACE_UNKNOWN),
        ] {
            assert!(!is_known_hosted_crash(outcome, ci, hosted, affected));
        }
    }

    fn restricted_probe(
        profile: &Arc<Profile>,
        name: &str,
        direct: Option<(&Path, &str)>,
        script: &[u8],
    ) -> u32 {
        let stop = Arc::new(event().unwrap());
        let deadline = Instant::now() + POWERSHELL_GUARD;
        let (workspace, mut command) =
            prepare_workspace(profile, &stop, deadline, Workspace::create)
                .unwrap_or_else(|error| panic!("{name}: {error}"))
                .unwrap_or_else(|| panic!("{name}: request workspace setup failed"));
        // Change only the executable/arguments. Keep the wrapper's exact
        // environment and cwd for each fresh request directory.
        if let Some((application, args)) = direct {
            command.application = wide_path(application).unwrap();
            command.command = wide(&format!("\"{}\" {args}", application.display()));
        }
        let worker_profile = Arc::clone(profile);
        let worker_stop = Arc::clone(&stop);
        let script = script.to_vec();
        let outcome = supervise(&stop, deadline, CLEANUP_BUDGET, move || {
            prepare(
                &command,
                Identity::Restricted(&worker_profile.token),
                &worker_profile.launcher.pipe_client,
                script,
                &worker_stop,
                deadline,
                |child| worker_profile.verify_child(child),
            )
        })
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        let removal_by = (Instant::now() + CLEANUP_BUDGET).min(deadline + 2 * CLEANUP_BUDGET);
        remove_workspace(&stop, workspace, removal_by, Workspace::remove)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        // A child exit is evidence that setup, pre-resume token and Job
        // checks, and input handoff succeeded. Keep those failures fatal.
        let ScriptOutcome::Exited { exit_code } = outcome else {
            panic!("{name}: expected a verified child exit, got {outcome:?}");
        };
        exit_code
    }

    fn hosted_restricted_powershell_ladder(profile: &Arc<Profile>) {
        let launcher = &profile.launcher;
        let cmd = launcher.system.join("cmd.exe");
        let clr = launcher
            .windows
            .join(r"Microsoft.NET\Framework64\v4.0.30319\MSBuild.exe");
        let powershell = launcher
            .system
            .join(r"WindowsPowerShell\v1.0\powershell.exe");

        let native = restricted_probe(profile, "cmd", Some((&cmd, "/d /c exit 7")), b"");
        // The x64 Framework tool runs managed code for /version without
        // building a project or writing any binaries.
        let managed = clr
            .try_exists()
            .expect("check for .NET Framework CLR probe")
            .then(|| restricted_probe(profile, "CLR", Some((&clr, "/nologo /version")), b""));
        let bare = restricted_probe(
            profile,
            "bare PowerShell",
            Some((
                &powershell,
                r#"-NoProfile -NonInteractive -Command "exit 7""#,
            )),
            b"",
        );
        let wrapper = restricted_probe(profile, "wrapper", None, b"exit 7");
        let divergence = [
            ("cmd", Some(native), 7),
            ("CLR", managed, 0),
            ("bare PowerShell", Some(bare), 7),
            ("wrapper", Some(wrapper), 7),
        ]
        .into_iter()
        .find(|(_, actual, expected)| actual.is_some_and(|actual| actual != *expected))
        .map_or("none", |(name, _, _)| name);
        let clr_summary = managed.map_or_else(
            || "skipped (x64 .NET Framework MSBuild absent)".to_owned(),
            |code| format!("{code:#010x}"),
        );
        eprintln!(
            "KNOWN-ISSUE hosted-restricted-powershell: original={HOSTED_POWERSHELL_CRASH:#010x}, \
             cmd={native:#010x}, CLR={clr_summary}, \
             bare PowerShell={bare:#010x}, wrapper={wrapper:#010x}, first divergence={divergence}"
        );
        assert_eq!(native, 7, "native restricted control exit");
        if let Some(code) = managed {
            assert_eq!(code, 0, "managed restricted control exit");
        }
    }

    // Failure-only diagnostics. Nothing below runs on a passing test. Every read
    // is bounded and correlated with a child this test launched itself.

    fn wevtutil(system: &Path, query: &str, count: u32) -> io::Result<String> {
        use std::process::{Command, Stdio};
        const MAX_EVENT_OUTPUT: u64 = 64 * 1024;
        let mut child = Command::new(system.join("wevtutil.exe"))
            .args([
                "qe",
                "Application",
                &format!("/q:{query}"),
                &format!("/c:{count}"),
                "/rd:true",
                "/f:xml",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut output = Vec::new();
        let read = child
            .stdout
            .take()
            .expect("piped stdout")
            .take(MAX_EVENT_OUTPUT)
            .read_to_end(&mut output);
        let capped = output.len() as u64 == MAX_EVENT_OUTPUT;
        if read.is_err() || capped {
            let _ = child.kill();
        }
        let status = child.wait()?;
        read?;
        if !status.success() && !capped {
            return Err(io::Error::other("event query failed"));
        }
        Ok(String::from_utf8_lossy(&output).into_owned())
    }

    // Read just a named field in wevtutil's XML EventData. An incomplete
    // record at the capture limit cannot be matched by this parser.
    fn event_data<'a>(event: &'a str, name: &str) -> Option<&'a str> {
        let (_, data) = event.split_once("<EventData>")?;
        let (data, _) = data.split_once("</EventData>")?;
        for quote in ['\'', '"'] {
            let field = format!("<Data Name={quote}{name}{quote}>");
            if let Some((_, value)) = data.split_once(&field) {
                return value.split_once("</Data>").map(|(value, _)| value.trim());
            }
        }
        None
    }

    fn hex_digits(value: &str) -> Option<&str> {
        value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
    }

    fn crash_event_summary(events: &str, pid: u32) -> String {
        let Some((events, _)) = events.rsplit_once("</Event>") else {
            return String::new();
        };
        events
            .split("</Event>")
            .filter(|event| {
                event_data(event, "ProcessId").and_then(|value| match hex_digits(value) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => value.parse().ok(),
                }) == Some(pid)
            })
            .take(2)
            .map(|event| {
                let code = event_data(event, "ExceptionCode").and_then(|value| {
                    u32::from_str_radix(hex_digits(value).unwrap_or(value), 16).ok()
                });
                code.map_or_else(|| "unknown".to_owned(), |code| format!("{code:#010x}"))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn event_summary_matches_the_full_pid_and_only_prints_numeric_fields() {
        let events = concat!(
            "<Event><EventData><Data Name='ProcessId'>0x12a</Data>",
            "<Data Name='ExceptionCode'>0x11111111</Data></EventData></Event>",
            "<Event><System><Computer>private-host</Computer></System><EventData>",
            "<Data Name=\"ProcessId\">0X0012</Data><Data Name='AppPath'>private-path</Data>",
            "<Data Name='ExceptionCode'>c0000005</Data></EventData></Event>",
            "<Event><EventData><Data Name='ProcessId'>18</Data>",
            "<Data Name='ExceptionCode'>private-value</Data></EventData></Event>",
            "<Event><EventData><Data Name='ProcessId'>0x12</Data>",
        );
        assert_eq!(crash_event_summary(events, 0x12), "0xc0000005, unknown");
        assert_eq!(crash_event_summary(events, 0x12a), "0x11111111");
        assert!(crash_event_summary(events, 0x1).is_empty());
    }

    /// Summarize only numeric facts from events inside the rerun's time window.
    fn child_events(system: &Path, pid: u32, since: Instant) -> String {
        let window = since.elapsed().as_millis();
        let time = format!(
            "TimeCreated[timediff(@SystemTime) >= 0 and timediff(@SystemTime) <= {window}]"
        );
        let runtime = wevtutil(
            system,
            &format!(
                "*[System[Provider[@Name='.NET Runtime'] and Execution[@ProcessID='{pid}'] \
                 and {time}]]"
            ),
            2,
        )
        .map_or_else(
            |_| "unavailable".to_owned(),
            |xml| xml.matches("</Event>").count().to_string(),
        );
        let crashes = wevtutil(
            system,
            &format!("*[System[Provider[@Name='Application Error'] and {time}]]"),
            8,
        )
        .map_or_else(
            |_| "unavailable".to_owned(),
            |xml| {
                let summary = crash_event_summary(&xml, pid);
                if summary.is_empty() {
                    "none".to_owned()
                } else {
                    summary
                }
            },
        );
        format!(".NET Runtime count={runtime}; Application Error exception codes={crashes}")
    }

    fn with_env(command: &mut CommandLine, entry: &str) {
        let mut entries: Vec<String> = command
            .env
            .split(|&unit| unit == 0)
            .filter(|value| !value.is_empty())
            .map(|value| String::from_utf16(value).unwrap())
            .chain([entry.to_owned()])
            .collect();
        entries.sort_by_key(|entry| entry.split('=').next().unwrap_or("").to_ascii_uppercase());
        command.env = entries
            .iter()
            .flat_map(|entry| wide(entry))
            .chain([0])
            .collect();
    }

    /// Re-runs the script and reports whether the redirected streams had output.
    /// Child text can contain host identifiers, so only its size is read.
    fn capture_powershell(
        identity: Identity<'_>,
        pipe_client: &[u32; 17],
        work: &Path,
        script: &[u8],
        extra_env: Option<&str>,
    ) -> String {
        let started = Instant::now();
        let (windows, system) = system_paths().unwrap();
        let mut command = CommandLine::powershell(&windows, &system, work).unwrap();
        if let Some(entry) = extra_env {
            with_env(&mut command, entry);
        }
        let path = wide_path(&work.join("diagnostic-output.txt")).unwrap();
        // SAFETY: test-owned file in the request directory; inherited only through
        // the explicit handle list. Windows removes it after the last handle closes.
        let output = match handle(unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE,
                FILE_SHARE_READ | FILE_SHARE_DELETE,
                &inheritable(),
                CREATE_NEW,
                FILE_FLAG_DELETE_ON_CLOSE,
                null_mut(),
            )
        }) {
            Ok(output) => output,
            Err(error) => return format!("output file OS error {:?}", error.raw_os_error()),
        };
        let job = Job::new().unwrap();
        let (reader, writer) = stdin_pipe(MAX_SCRIPT as u32, pipe_client).unwrap();
        let child = match create_suspended(&job, &reader, &output, &command, identity) {
            Ok(child) => child,
            Err(error) => return format!("launch: {:?}", error.outcome()),
        };
        drop(reader);
        let stop = event().unwrap();
        let input = pipe_write(&writer, &stop, Instant::now() + GUARD, script.to_vec());
        drop(writer);
        // SAFETY: the suspended child is contained in the held Job.
        unsafe { ResumeThread(child.thread.as_raw_handle()) };
        let finished = signaled(
            child.process.as_raw_handle(),
            POWERSHELL_GUARD.as_millis() as u32,
        )
        .unwrap_or(false);
        let code = finished.then(|| exit_code(&child.process).ok()).flatten();
        // SAFETY: held Job handle; the child is waited on below.
        unsafe { TerminateJobObject(job.0.as_raw_handle(), 1) };
        let _ = signaled(child.process.as_raw_handle(), GUARD.as_millis() as u32);
        let pid = unsafe { GetProcessId(child.process.as_raw_handle()) };
        let output_bytes = File::from(output).metadata().ok().map(|file| file.len());
        format!(
            "input accepted {}; finished {finished}; exit {code:#x?}; pid {pid}; \
             output bytes {output_bytes:?}\nevents: {}",
            input.is_ok(),
            child_events(&system, pid, started)
        )
    }

    fn diagnostic_sid_name(sid: &str) -> &'static str {
        match sid {
            "S-1-5-18" => "LocalSystem",
            "S-1-5-32-544" => "Administrators",
            "S-1-5-32-545" => "Users",
            "S-1-15-2-1" => "All Application Packages",
            "S-1-15-2-2" => "All Restricted Application Packages",
            sid if sid.starts_with("S-1-15-2-") => "AppContainer package",
            _ => "<sid>",
        }
    }

    fn diagnostic_sid(sid: PSID) -> &'static str {
        if sid.is_null() {
            return "none";
        }
        sid_string(sid).map_or("<sid>", |sid| diagnostic_sid_name(&sid))
    }

    #[test]
    fn diagnostic_sids_name_well_known_groups_and_hide_individual_identifiers() {
        assert_eq!(diagnostic_sid_name("S-1-5-18"), "LocalSystem");
        assert_eq!(diagnostic_sid_name("S-1-5-32-544"), "Administrators");
        assert_eq!(diagnostic_sid_name("S-1-5-32-545"), "Users");
        assert_eq!(
            diagnostic_sid_name("S-1-15-2-2"),
            "All Restricted Application Packages"
        );
        assert_eq!(
            diagnostic_sid_name("S-1-15-2-111-222-333"),
            "AppContainer package"
        );
        assert_eq!(diagnostic_sid_name("S-1-5-21-111-222-333-1000"), "<sid>");
    }

    /// Owner, a short DACL summary and the access the restricted probe token
    /// would be granted, for one object, or the open error.
    fn object_access(
        name: &str,
        open: io::Result<impl AsRawHandle>,
        kind: SE_OBJECT_TYPE,
        probe: &OwnedHandle,
        mapping: GENERIC_MAPPING,
    ) -> String {
        let object = match open {
            Ok(object) => object,
            Err(error) => return format!("{name}: open OS error {:?}\n", error.raw_os_error()),
        };
        let descriptor = match Descriptor::object(
            object.as_raw_handle(),
            kind,
            OWNER_SECURITY_INFORMATION
                | GROUP_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | LABEL_SECURITY_INFORMATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => return format!("{name}: security OS error {:?}\n", error.raw_os_error()),
        };
        let mut owner = null_mut();
        let mut defaulted = 0;
        // SAFETY: the owner pointer refers into the retained descriptor.
        unsafe { GetSecurityDescriptorOwner(descriptor.0, &mut owner, &mut defaulted) };
        let mut aces = Vec::new();
        if let Ok(acl) = descriptor.dacl() {
            // SAFETY: the ACL is retained by `descriptor`; ACE index below AceCount.
            let count = unsafe { (*acl).AceCount };
            for index in 0..u32::from(count).min(8) {
                let mut ace = null_mut();
                if unsafe { GetAce(acl, index, &mut ace) } == 0 {
                    break;
                }
                let header = unsafe { &*ace.cast::<ACE_HEADER>() };
                let kind = u32::from(header.AceType);
                if kind == ACCESS_ALLOWED_ACE_TYPE || kind == ACCESS_DENIED_ACE_TYPE {
                    let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
                    let sid = (&ace.SidStart as *const u32).cast_mut().cast();
                    aces.push(format!(
                        "{}:{}:{:#x}",
                        if kind == ACCESS_ALLOWED_ACE_TYPE {
                            "A"
                        } else {
                            "D"
                        },
                        diagnostic_sid(sid),
                        ace.Mask
                    ));
                } else {
                    aces.push(format!("type{kind}"));
                }
            }
        }
        let granted = descriptor.allowed(probe, mapping).map_or_else(
            |error| format!("check OS error {:?}", error.raw_os_error()),
            |mask| format!("{mask:#x}"),
        );
        format!(
            "{name}: owner {} granted-to-restricted {granted} dacl [{}]\n",
            diagnostic_sid(owner),
            aces.join(" ")
        )
    }

    fn open_for_security(path: &Path) -> io::Result<OwnedHandle> {
        let name = wide_path(path)?;
        // SAFETY: fixed system path, metadata-only access.
        handle(unsafe {
            CreateFileW(
                name.as_ptr(),
                READ_CONTROL | FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        })
    }

    struct DiagnosticKey(windows_sys::Win32::System::Registry::HKEY);

    impl Drop for DiagnosticKey {
        fn drop(&mut self) {
            use windows_sys::Win32::System::Registry::RegCloseKey;
            // SAFETY: this is the owned handle from a successful RegOpenKeyExW.
            unsafe { RegCloseKey(self.0) };
        }
    }

    impl AsRawHandle for DiagnosticKey {
        fn as_raw_handle(&self) -> HANDLE {
            self.0
        }
    }

    fn open_key_for_security(key: &str) -> io::Result<DiagnosticKey> {
        use windows_sys::Win32::System::Registry::RegOpenKeyExW;
        let name = wide(key);
        let mut raw = null_mut();
        // SAFETY: fixed HKLM subkey, READ_CONTROL only.
        let error =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, name.as_ptr(), 0, READ_CONTROL, &mut raw) };
        if error != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(DiagnosticKey(raw))
    }

    /// Token facts and the access the restricted token has to the objects
    /// PowerShell start-up reads, including the faulting native image.
    fn restricted_access_report(profile: &Profile) -> String {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::System::Registry::{KEY_ALL_ACCESS, KEY_READ, KEY_WRITE};
        let mut report = String::new();
        let token = &profile.token;
        let source = open_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
        let owner = token_buffer(token, TokenOwner).unwrap();
        report.push_str(&format!(
            "restricted user {} owner {} il {:?} session {:?}; runner il {:?} elevation-type {:?} session {:?}; EnableLUA {:?}\n",
            token_user_string(token).map_or("<sid>", |sid| diagnostic_sid_name(&sid)),
            diagnostic_sid(unsafe { (*owner.as_ptr().cast::<TOKEN_OWNER>()).Owner }),
            integrity_rid(token).ok().map(|rid| format!("{rid:#x}")),
            token_dword(token, TokenSessionId).ok(),
            integrity_rid(&source).ok().map(|rid| format!("{rid:#x}")),
            token_dword(&source, TokenElevationType).ok(),
            token_dword(&source, TokenSessionId).ok(),
            read_policy_value(
                Hive::Machine,
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System",
                "EnableLUA"
            ),
        ));
        let windows = &profile.launcher.windows;
        let native = windows.join(r"assembly\NativeImages_v4.0.30319_64");
        let mut paths = vec![
            (
                "SMA native image directory",
                native.join("System.Management.Automation"),
            ),
            (
                "PowerShell system directory",
                profile.launcher.system.join(r"WindowsPowerShell\v1.0"),
            ),
            (
                "SMA GAC directory",
                windows.join(r"Microsoft.NET\assembly\GAC_MSIL\System.Management.Automation"),
            ),
        ];
        if let Ok(entries) = std::fs::read_dir(&native) {
            for entry in entries
                .take(128)
                .flatten()
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("System.Mana")
                })
                .filter(|entry| {
                    entry.metadata().is_ok_and(|metadata| {
                        metadata.is_dir()
                            && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
                    })
                })
                .take(3)
            {
                paths.push(("native assembly directory", entry.path()));
                if let Ok(images) = std::fs::read_dir(entry.path()) {
                    for image in images.take(2).flatten() {
                        paths.push(("native image entry", image.path()));
                        if image.metadata().is_ok_and(|metadata| {
                            metadata.is_dir()
                                && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
                        }) {
                            paths.push((
                                "SMA native image DLL",
                                image.path().join("System.Management.Automation.ni.dll"),
                            ));
                        }
                    }
                }
            }
        }
        for (label, path) in paths.iter().take(16) {
            report.push_str(&object_access(
                label,
                open_for_security(path),
                SE_FILE_OBJECT,
                &profile.probe,
                file_mapping(),
            ));
        }
        let registry = GENERIC_MAPPING {
            GenericRead: KEY_READ,
            GenericWrite: KEY_WRITE,
            GenericExecute: KEY_READ,
            GenericAll: KEY_ALL_ACCESS,
        };
        for key in [
            r"SOFTWARE\Microsoft\PowerShell\1\PowerShellEngine",
            r"SOFTWARE\Microsoft\PowerShell\1\ShellIds\Microsoft.PowerShell",
            r"SOFTWARE\Microsoft\.NETFramework",
        ] {
            report.push_str(&object_access(
                &format!(r"HKLM\{key}"),
                open_key_for_security(key),
                SE_REGISTRY_KEY,
                &profile.probe,
                registry,
            ));
        }
        report
    }

    /// Full failure evidence for the production identity: token and object
    /// access, the script re-run with captured output, and the same re-run
    /// with NGEN images disabled.
    fn restricted_failure_report(profile: &Arc<Profile>, script: &[u8]) -> String {
        let mut report = restricted_access_report(profile);
        for extra in [None, Some("COMPlus_ZapDisable=1")] {
            let capture = match Workspace::create(&profile.launcher) {
                Ok(workspace) => {
                    let text = capture_powershell(
                        Identity::Restricted(&profile.token),
                        &profile.launcher.pipe_client,
                        workspace.path(),
                        script,
                        extra,
                    );
                    let _ = workspace.remove();
                    text
                }
                Err(error) => format!("workspace: {error:?}"),
            };
            report.push_str(&format!("\n--- re-run with {extra:?}:\n{capture}\n"));
        }
        report
    }

    fn assert_powershell(
        outcome: Result<ScriptOutcome, &'static str>,
        expected: ScriptOutcome,
        diagnose: impl FnOnce() -> String,
    ) {
        if outcome != Ok(expected) {
            panic!(
                "PowerShell returned {outcome:?}, expected {expected:?}\ndiagnostics:\n{}",
                diagnose()
            );
        }
    }

    fn is_known_hosted_wrapper_timeout(
        outcome: Result<ScriptOutcome, &'static str>,
        ci: Option<&str>,
        runner: Option<&str>,
        image: Option<&str>,
    ) -> bool {
        is_hosted_win25(ci, runner, image) && outcome == Ok(ScriptOutcome::TimedOut)
    }

    #[test]
    fn hosted_wrapper_timeout_requires_the_exact_outcome() {
        let ci = Some("true");
        let runner = Some("github-hosted");
        let image = Some("win25-vs2026");
        assert!(is_known_hosted_wrapper_timeout(
            Ok(ScriptOutcome::TimedOut),
            ci,
            runner,
            image
        ));
        for outcome in [
            Ok(ScriptOutcome::Exited { exit_code: 0 }),
            Ok(ScriptOutcome::Exited { exit_code: 7 }),
            Ok(ScriptOutcome::Exited {
                exit_code: HOSTED_POWERSHELL_CRASH,
            }),
            Ok(ScriptOutcome::Unavailable),
            Ok(ScriptOutcome::LaunchFailed),
            Ok(ScriptOutcome::Terminated),
            Err(CLEANUP_UNKNOWN),
            Err(WORKSPACE_UNKNOWN),
        ] {
            assert!(!is_known_hosted_wrapper_timeout(outcome, ci, runner, image));
        }
    }

    #[test]
    fn hosted_wrapper_timeout_requires_github_ci_on_win25() {
        let ci = Some("true");
        let hosted = Some("github-hosted");
        let affected = Some("win25");
        for (ci, runner, image) in [
            (None, hosted, affected),
            (Some("false"), hosted, affected),
            (ci, None, affected),
            (ci, Some("self-hosted"), affected),
            (ci, hosted, Some("win22")),
            (ci, hosted, None),
        ] {
            assert!(!is_known_hosted_wrapper_timeout(
                Ok(ScriptOutcome::TimedOut),
                ci,
                runner,
                image
            ));
        }
    }

    fn assert_powershell_wrapper(script: &[u8], expected: u32) {
        let root = TestRoot::new("wrapper");
        let (windows, system) = system_paths().unwrap();
        let command = CommandLine::powershell(&windows, &system, &root.0).unwrap();
        let stop = Arc::new(event().unwrap());
        let worker_stop = Arc::clone(&stop);
        let worker_script = script.to_vec();
        let deadline = Instant::now() + POWERSHELL_GUARD;
        let outcome = supervise(&stop, deadline, CLEANUP_BUDGET, move || {
            prepare(
                &command,
                Identity::TestCurrent,
                &current_user_sid(),
                worker_script,
                &worker_stop,
                deadline,
                |_| Ok(()),
            )
        });
        if is_known_hosted_wrapper_timeout(
            outcome,
            std::env::var("CI").ok().as_deref(),
            std::env::var("RUNNER_ENVIRONMENT").ok().as_deref(),
            std::env::var("ImageOS").ok().as_deref(),
        ) {
            eprintln!("KNOWN-ISSUE hosted-windows-powershell-wrapper: Ok(TimedOut)");
            return;
        }
        assert_powershell(
            outcome,
            ScriptOutcome::Exited {
                exit_code: expected,
            },
            || {
                let rerun = TestRoot::new("wrapper-diagnostic");
                capture_powershell(
                    Identity::TestCurrent,
                    &current_user_sid(),
                    &rerun.0,
                    script,
                    None,
                )
            },
        );
    }

    // Native 259 and high-bit codes are mapped on the Rust side, so the cheap
    // self-re-exec fixture covers them (restricted_child_...). PowerShell runs
    // only for wrapper semantics and the production-identity gates.
    #[test]
    fn wrapper_evaluates_utf8_multiline_with_implicit_success() {
        assert_powershell_wrapper("$x = 'é漢'\nif ($x[0] -ne [char]233 -or $x[1] -ne [char]28450) { throw 'fixture' }\n$x".as_bytes(), 0);
    }
    #[test]
    fn wrapper_reports_implicit_error() {
        assert_powershell_wrapper(b"throw 'fixture'", 1);
    }

    #[test]
    fn production_identity_runs_powershell_and_removes_its_request_directory() {
        let root = TestRoot::new("ps");
        let profile = Arc::new(test_profile(&root.0));
        let stop = Arc::new(event().unwrap());
        // Cmdlets autoload from $PSHOME\Modules only: the wrapper pins the
        // module path that Windows PowerShell would otherwise extend.
        let script = "if ($env:PSModulePath -ne [IO.Path]::Combine($PSHOME, 'Modules')) { exit 3 }\n\
            if ($env:LOCALAPPDATA -ne [IO.Path]::Combine($env:TEMP, 'AppData\\Local')) { exit 4 }\n\
            $d = New-Item -ItemType Directory -Path (Join-Path $env:TEMP 'nested')\n\
            $f = Join-Path $d.FullName 'locked.txt'\n\
            [IO.File]::WriteAllText($f, 'arx')\n\
            (Get-Item -LiteralPath $f).IsReadOnly = $true\n\
            exit 7";
        let outcome = run_profile(
            &profile,
            &stop,
            script.as_bytes().to_vec(),
            Instant::now() + POWERSHELL_GUARD,
        );
        let expected = ScriptOutcome::Exited { exit_code: 7 };
        if quarantine_hosted_crash(outcome, expected, &profile) {
            assert_eq!(root.entries(), 0, "request and probe directories removed");
            return;
        }
        assert_powershell(outcome, expected, || {
            restricted_failure_report(&profile, script.as_bytes())
        });
        assert_eq!(root.entries(), 0, "request directory removed after cleanup");
    }

    /// The no-conflict snapshot: everything absent except the script block
    /// logging value that must be explicitly zero.
    fn clean_policy() -> [PolicyValue; POLICY_VALUES.len()] {
        POLICY_VALUES.map(|(_, _, _, rule)| {
            if rule == PolicyRule::ExplicitlyOff {
                PolicyValue::Dword(0)
            } else {
                PolicyValue::Absent
            }
        })
    }

    #[test]
    fn self_test_admits_the_production_identity() {
        let root = TestRoot::new("self");
        let stop = Arc::new(event().unwrap());
        let profile = Arc::new(test_profile(&root.0));
        let outcome = run_profile(
            &profile,
            &stop,
            SELF_TEST.as_bytes().to_vec(),
            Instant::now() + POWERSHELL_GUARD,
        );
        let sentinel = ScriptOutcome::Exited {
            exit_code: SELF_TEST_SENTINEL,
        };
        if quarantine_hosted_crash(outcome, sentinel, &profile) {
            assert_eq!(root.entries(), 0, "request and probe directories removed");
            return;
        }
        assert_powershell(outcome, sentinel, || {
            restricted_failure_report(&profile, SELF_TEST.as_bytes())
        });
        // The same outcome admits a production-shaped profile.
        let admitted = admit(test_profile(&root.0), &clean_policy(), |_| outcome);
        assert!(matches!(admitted, Ok(Some(_))));
        assert_eq!(
            root.entries(),
            0,
            "self-test directory removed after cleanup"
        );
    }

    #[test]
    fn admission_requires_the_exact_sentinel_and_no_policy_conflict() {
        let root = TestRoot::new("admit");
        let clean = clean_policy();
        for (result, expected) in [
            (
                Ok(ScriptOutcome::Exited {
                    exit_code: SELF_TEST_SENTINEL,
                }),
                Ok(true),
            ),
            // CLR start failure, CLR FailFast and console DLL-init failure.
            (
                Ok(ScriptOutcome::Exited {
                    exit_code: 0xFFFF_0000,
                }),
                Ok(false),
            ),
            (
                Ok(ScriptOutcome::Exited {
                    exit_code: 0x8013_1623,
                }),
                Ok(false),
            ),
            (
                Ok(ScriptOutcome::Exited {
                    exit_code: 0xC000_0142,
                }),
                Ok(false),
            ),
            (Ok(ScriptOutcome::Exited { exit_code: 0 }), Ok(false)),
            (Ok(ScriptOutcome::LaunchFailed), Ok(false)),
            (Ok(ScriptOutcome::Unavailable), Ok(false)),
            (Ok(ScriptOutcome::TimedOut), Ok(false)),
            (Err(CLEANUP_UNKNOWN), Err(CLEANUP_UNKNOWN)),
        ] {
            let admitted = admit(test_profile(&root.0), &clean, |_| result);
            assert_eq!(
                admitted.map(|profile| profile.is_some()),
                expected,
                "{result:?}"
            );
        }
        for conflict in [PolicyValue::Absent, PolicyValue::Dword(1)] {
            // Unconfigured script block logging still logs suspicious blocks.
            let mut snapshot = clean;
            let index = POLICY_VALUES
                .iter()
                .position(|(_, _, _, rule)| *rule == PolicyRule::ExplicitlyOff)
                .unwrap();
            snapshot[index] = conflict;
            let ran = Cell::new(false);
            let admitted = admit(test_profile(&root.0), &snapshot, |_| {
                ran.set(true);
                Ok(ScriptOutcome::Exited {
                    exit_code: SELF_TEST_SENTINEL,
                })
            });
            assert!(matches!(admitted, Ok(None)), "{conflict:?}");
            assert!(
                !ran.get(),
                "a policy conflict closes execution before the self-test"
            );
        }
    }

    #[test]
    fn policy_conflicts_close_execution() {
        use PolicyValue::*;
        let clean = clean_policy();
        assert!(!policy_conflict(&clean));
        // Both hives are read, and only the machine script block value is required.
        assert_eq!(
            POLICY_VALUES
                .iter()
                .filter(|(hive, _, _, _)| *hive == Hive::User)
                .count(),
            6
        );
        assert_eq!(
            POLICY_VALUES
                .iter()
                .filter(|(hive, _, value, rule)| *hive == Hive::Machine
                    && *value == "EnableScriptBlockLogging"
                    && *rule == PolicyRule::ExplicitlyOff)
                .count(),
            1
        );
        for (index, (_, _, value, rule)) in POLICY_VALUES.iter().enumerate() {
            for candidate in [Absent, Dword(0), Dword(1), Other, Unreadable] {
                let mut snapshot = clean;
                snapshot[index] = candidate;
                let allowed = match rule {
                    PolicyRule::Unset => candidate == Absent,
                    PolicyRule::OffOrUnset => matches!(candidate, Absent | Dword(0)),
                    PolicyRule::ExplicitlyOff => candidate == Dword(0),
                };
                assert_eq!(
                    policy_conflict(&snapshot),
                    !allowed,
                    "{value} {rule:?} {candidate:?}"
                );
            }
        }
        assert!(!read_policy().contains(&Unreadable));
    }

    #[test]
    fn bounded_steps_never_outlive_their_budget() {
        let stop = event().unwrap();
        let far = Instant::now() + GUARD;
        assert!(matches!(
            bounded(&stop, far, Duration::ZERO, far, || 5),
            Bounded::Done(5)
        ));
        let latch = Arc::new(event().unwrap());
        let held = Arc::clone(&latch);
        let started = Instant::now();
        let soon = Instant::now() + Duration::from_millis(50);
        let result = bounded(&stop, soon, Duration::ZERO, soon, move || {
            signaled(held.as_raw_handle(), GUARD.as_millis() as u32)
        });
        assert!(matches!(result, Bounded::Missed));
        assert!(
            started.elapsed() < GUARD / 2,
            "a missed step is not awaited"
        );
        signal(&latch);
        // Cancellation waits only for the grace period, not until `by`.
        let cancelled = event().unwrap();
        signal(&cancelled);
        let latch = Arc::new(event().unwrap());
        let held = Arc::clone(&latch);
        let started = Instant::now();
        let far = Instant::now() + GUARD;
        let result = bounded(&cancelled, far, Duration::from_millis(50), far, move || {
            signaled(held.as_raw_handle(), GUARD.as_millis() as u32)
        });
        assert!(matches!(result, Bounded::Missed));
        assert!(started.elapsed() < GUARD / 2);
        signal(&latch);
    }

    #[test]
    fn cancellation_grace_never_passes_the_absolute_limit() {
        let now = Instant::now();
        let limit = now + Duration::from_secs(3);
        assert_eq!(
            cancel_deadline(now, Duration::from_secs(1), limit),
            now + Duration::from_secs(1)
        );
        assert_eq!(cancel_deadline(now, Duration::from_secs(5), limit), limit);
        assert_eq!(cancel_deadline(limit, Duration::from_secs(1), limit), limit);
        assert_eq!(cancel_deadline(now, Duration::MAX, limit), limit);
    }

    #[test]
    fn late_cancellation_cannot_extend_workspace_removal() {
        let root = TestRoot::new("late");
        let profile = Arc::new(test_profile(&root.0));
        let stop = Arc::new(event().unwrap());
        let (workspace, _command) =
            prepare_workspace(&profile, &stop, Instant::now() + GUARD, Workspace::create)
                .unwrap()
                .unwrap();
        let latch = Arc::new(event().unwrap());
        let held = Arc::clone(&latch);
        let canceller_stop = Arc::clone(&stop);
        // Stop arrives late: after 300 ms of a 500 ms removal allowance. A grace
        // period restarted at that point (2 s) would end near 2.3 s.
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            signal(&canceller_stop);
        });
        let started = Instant::now();
        let removal = remove_workspace(
            &stop,
            workspace,
            started + Duration::from_millis(500),
            move |workspace| {
                signaled(held.as_raw_handle(), GUARD.as_millis() as u32).unwrap();
                workspace.remove()
            },
        );
        let elapsed = started.elapsed();
        canceller.join().unwrap();
        signal(&latch);
        assert_eq!(removal, Err(WORKSPACE_UNKNOWN));
        assert!(
            elapsed < Duration::from_millis(1200),
            "removal ended at {elapsed:?}, past its absolute deadline"
        );
    }

    #[test]
    fn stalled_workspace_setup_and_removal_retire_within_budget() {
        let root = TestRoot::new("stall");
        let profile = Arc::new(test_profile(&root.0));
        let stop = event().unwrap();
        let latch = Arc::new(event().unwrap());

        let held = Arc::clone(&latch);
        let started = Instant::now();
        let setup = prepare_workspace(
            &profile,
            &stop,
            Instant::now() + Duration::from_millis(50),
            move |_| {
                signaled(held.as_raw_handle(), GUARD.as_millis() as u32).unwrap();
                Err(CreateError::Failed)
            },
        );
        assert_eq!(setup.err(), Some(WORKSPACE_UNKNOWN));
        assert!(started.elapsed() < GUARD / 2);

        let failed = prepare_workspace(&profile, &stop, Instant::now() + GUARD, |_| {
            Err(CreateError::Failed)
        });
        assert!(matches!(failed, Ok(None)), "failed setup is LaunchFailed");
        // A created directory whose rollback could not be proven retires.
        let leaked = prepare_workspace(&profile, &stop, Instant::now() + GUARD, |_| {
            Err(CreateError::Leaked)
        });
        assert_eq!(leaked.err(), Some(WORKSPACE_UNKNOWN));

        let (workspace, _command) =
            prepare_workspace(&profile, &stop, Instant::now() + GUARD, Workspace::create)
                .unwrap()
                .unwrap();
        let held = Arc::clone(&latch);
        let started = Instant::now();
        let removal = remove_workspace(
            &stop,
            workspace,
            Instant::now() + Duration::from_millis(50),
            move |workspace| {
                signaled(held.as_raw_handle(), GUARD.as_millis() as u32).unwrap();
                workspace.remove()
            },
        );
        assert_eq!(removal, Err(WORKSPACE_UNKNOWN));
        assert!(started.elapsed() < GUARD / 2);
        signal(&latch);

        let (workspace, _command) =
            prepare_workspace(&profile, &stop, Instant::now() + GUARD, Workspace::create)
                .unwrap()
                .unwrap();
        assert_eq!(
            remove_workspace(&stop, workspace, Instant::now() + GUARD, Workspace::remove),
            Ok(())
        );
    }

    #[test]
    fn authority_must_be_a_system_integrity_default_token() {
        assert!(authority_integrity_ok(0x4000));
        for rid in [0x0000, 0x1000, 0x2000, 0x2100, 0x3000, 0x5000] {
            assert!(!authority_integrity_ok(rid), "{rid:#x}");
        }
        assert!(elevation_type_ok(1));
        assert!(!elevation_type_ok(2), "full half of a split token");
        assert!(!elevation_type_ok(3), "limited half of a split token");
    }

    #[test]
    fn only_refused_access_or_privilege_means_unavailable() {
        let error = |code: u32| io::Error::from_raw_os_error(code as i32);
        for code in [ERROR_ACCESS_DENIED, ERROR_PRIVILEGE_NOT_HELD] {
            assert_eq!(
                SpawnError::JobAttribute(error(code)).outcome(),
                ScriptOutcome::Unavailable
            );
            assert_eq!(
                SpawnError::Creation(error(code)).outcome(),
                ScriptOutcome::Unavailable
            );
            assert_eq!(
                SpawnError::Setup(error(code)).outcome(),
                ScriptOutcome::LaunchFailed
            );
        }
        for code in [
            ERROR_INVALID_PARAMETER,
            ERROR_NOT_SUPPORTED,
            ERROR_BAD_TOKEN_TYPE,
            ERROR_EXE_MACHINE_TYPE_MISMATCH,
            ERROR_FILE_NOT_FOUND,
        ] {
            for spawn in [
                SpawnError::JobAttribute(error(code)),
                SpawnError::Creation(error(code)),
                SpawnError::Setup(error(code)),
            ] {
                assert_eq!(spawn.outcome(), ScriptOutcome::LaunchFailed, "{code}");
            }
        }
    }

    #[derive(Debug)]
    #[allow(dead_code)] // Fields are read through Debug in the failure message.
    enum TokenStep {
        OpenSource(io::Error),
        Restrict(io::Error),
        Shape(io::Error),
        DuplicateProbe(io::Error),
        Descriptor(io::Error),
        AccessCheck(&'static str, io::Error),
        Expectation(&'static str, u32),
    }

    const EVENT_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
        GenericRead: 0x0002_0001,
        GenericWrite: 0x0002_0002,
        GenericExecute: 0x0012_0000,
        GenericAll: 0x001f_0003,
    };

    /// ADR 0059: write access is decided by MIC no-write-up and deny-only
    /// groups. Each pair of descriptors differs only by the relevant grant:
    /// the Low label, or the grantee. Owner SYSTEM avoids implicit owner rights.
    #[test]
    fn script_token_write_access_is_decided_by_label_and_deny_only_groups() {
        fn run() -> Result<(), TokenStep> {
            let source = open_token(unsafe { GetCurrentProcess() }, TOKEN_SOURCE_ACCESS)
                .map_err(TokenStep::OpenSource)?;
            let token = restrict_token(&source, &[]).map_err(TokenStep::Restrict)?;
            verify_restrictions(&token).map_err(TokenStep::Shape)?;
            let user = token_user_string(&token).map_err(TokenStep::Shape)?;
            let probe = impersonation_token(&token).map_err(TokenStep::DuplicateProbe)?;
            let rights = |name: &'static str, sddl: String| -> Result<u32, TokenStep> {
                let descriptor = Descriptor::from_sddl(&sddl).map_err(TokenStep::Descriptor)?;
                descriptor
                    .allowed(&probe, EVENT_MAPPING)
                    .map_err(|error| TokenStep::AccessCheck(name, error))
            };
            let expect = |what: &'static str, ok: bool, value: u32| {
                if ok {
                    Ok(())
                } else {
                    Err(TokenStep::Expectation(what, value))
                }
            };
            let grant = format!("O:SYG:SYD:P(A;;0x1f0003;;;{user})");
            let low = rights("low", format!("{grant}S:(ML;;NW;;;LW)"))?;
            let medium = rights("medium", grant)?;
            let admins = rights(
                "admins",
                "O:SYG:SYD:P(A;;0x1f0003;;;BA)S:(ML;;NW;;;LW)".to_owned(),
            )?;
            expect("Low object grants modify", low == 0x001f_0003, low)?;
            expect(
                "Medium object stays readable",
                medium & (READ_CONTROL | SYNCHRONIZE) == READ_CONTROL | SYNCHRONIZE,
                medium,
            )?;
            expect(
                "no write up to a Medium object",
                medium & (EVENT_MODIFY_STATE | DELETE | WRITE_DAC | WRITE_OWNER) == 0,
                medium,
            )?;
            expect(
                "deny-only Administrators grant nothing",
                admins == 0,
                admins,
            )
        }
        if let Err(step) = run() {
            panic!("script token step failed: {step:?}");
        }
    }

    #[test]
    fn script_token_has_no_privileges_and_a_fixed_default_dacl() {
        let source = open_token(unsafe { GetCurrentProcess() }, TOKEN_SOURCE_ACCESS).unwrap();
        let token = restrict_token(&source, &[]).unwrap();
        let privileges = token_buffer(&token, TokenPrivileges).unwrap();
        assert!(privilege_entries(&privileges).unwrap().is_empty());
        verify_default_dacl(&token).unwrap();
        // ADR 0059 group policy: broad groups keep their ordinary read grants.
        // Only administrator-type and service groups become deny-only.
        assert!(
            token_owner_is_user(&token).unwrap(),
            "objects are owned by the user"
        );
        for kind in [WinBuiltinUsersSid, WinAuthenticatedUserSid, WinWorldSid] {
            assert!(!DENY_ONLY_GROUPS.contains(&kind));
            let mut group = sid(kind).unwrap();
            let before = group_attributes(&source, TokenGroups, group.as_mut_ptr().cast()).unwrap();
            let after = group_attributes(&token, TokenGroups, group.as_mut_ptr().cast()).unwrap();
            if let Some(flags) = before.filter(|flags| flags & GROUP_ENABLED != 0) {
                assert_eq!(after, Some(flags), "group {kind} unchanged");
            }
        }
        assert_eq!(
            default_dacl_sddl("S-1-5-19"),
            "D:(A;;GA;;;SY)(A;;GA;;;S-1-5-19)"
        );
    }

    fn duplex_pipe() -> (OwnedHandle, OwnedHandle) {
        static SERIAL: AtomicU32 = AtomicU32::new(0);
        let name = wide(&format!(
            r"\\.\pipe\arx-guest-port-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        // SAFETY: test-scoped first instance with default security.
        let server = handle(unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                0,
                null(),
            )
        })
        .unwrap();
        let client = handle(unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        })
        .unwrap();
        (server, client)
    }

    #[test]
    fn idle_port_read_has_no_deadline_and_ends_on_stop() {
        let stop = Arc::new(event().unwrap());
        let (server, _client) = duplex_pipe();
        let reader_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let retire = Cell::new(None);
            let port = Port {
                handle: server,
                stop: &reader_stop,
                retire: &retire,
                read_by: Cell::new(None),
                write_by: Cell::new(None),
            };
            let mut reader = &port;
            let mut buffer = [0u8; 8];
            let result = reader.read(&mut buffer).map_err(|error| error.kind());
            (result, retire.get())
        });
        // No frame has started, so the read has no deadline and stays pending.
        assert!(!signaled(worker.as_raw_handle(), 300).unwrap());
        signal(&stop);
        assert!(signaled(worker.as_raw_handle(), GUARD.as_millis() as u32).unwrap());
        let (result, retire) = worker.join().unwrap();
        assert_eq!(result, Err(io::ErrorKind::BrokenPipe));
        assert_eq!(retire, None, "a cancelled read is drained, not retired");
    }

    #[test]
    fn ended_session_rediscovers_and_retire_fails_the_service() {
        let stop = event().unwrap();
        let discovered = Cell::new(0);
        let sessions = Cell::new(0);
        let result = serve_sessions(
            &stop,
            event().unwrap(),
            Duration::ZERO,
            || {
                discovered.set(discovered.get() + 1);
                event().map_err(|_| "fixture")
            },
            |_port| {
                sessions.set(sessions.get() + 1);
                if sessions.get() < 3 {
                    Ok(())
                } else {
                    Err(CLEANUP_UNKNOWN)
                }
            },
        );
        assert_eq!(result, Err(CLEANUP_UNKNOWN));
        assert_eq!((sessions.get(), discovered.get()), (3, 2));

        let live = event().unwrap();
        assert_eq!(
            serve_sessions(
                &live,
                event().unwrap(),
                Duration::ZERO,
                || Err("serial discovery deadline"),
                |_| Ok(()),
            ),
            Err("serial discovery deadline")
        );

        signal(&stop);
        assert_eq!(
            serve_sessions(
                &stop,
                event().unwrap(),
                Duration::ZERO,
                || panic!("no rediscovery after cancellation"),
                |_| Ok(()),
            ),
            Ok(())
        );
    }

    const FIXTURE: &str = "platform::windows::tests::job_fixture_process";
    const JOB_MODE: &str = "ARX_GUEST_JOB_TEST_MODE";
    const JOB_READY: &str = "ARX_GUEST_JOB_TEST_READY";
    const JOB_SENTINEL: &str = "ARX_GUEST_JOB_TEST_SENTINEL";
    const PROBE_ALLOWED: &str = "ARX_GUEST_PROBE_ALLOWED";
    const PROBE_DENIED: &str = "ARX_GUEST_PROBE_DENIED";
    const FIXTURE_EXIT: &str = "ARX_GUEST_FIXTURE_EXIT";
    /// 0x40 | allowed create succeeded | Medium create denied | seeded read succeeded.
    const PROBE_PASSED: u32 = 0x47;

    fn fixture_command(env: &[(&str, String)]) -> CommandLine {
        let (windows, system) = system_paths().unwrap();
        let exe = std::env::current_exe().unwrap();
        let env_block = env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .chain([format!("SystemRoot={}", windows.display())])
            .flat_map(|value| wide(&value))
            .chain([0])
            .collect();
        CommandLine {
            application: wide_path(&exe).unwrap(),
            command: wide(&format!(
                "\"{}\" --exact {FIXTURE} --nocapture",
                exe.display()
            )),
            env: env_block,
            cwd: wide_path(&system).unwrap(),
        }
    }

    fn run_restricted_fixture(
        profile: &Arc<Profile>,
        command: CommandLine,
    ) -> Result<ScriptOutcome, &'static str> {
        let stop = Arc::new(event().unwrap());
        let worker_stop = Arc::clone(&stop);
        let worker_profile = Arc::clone(profile);
        let deadline = Instant::now() + GUARD;
        supervise(&stop, deadline, CLEANUP_BUDGET, move || {
            let profile = worker_profile;
            prepare(
                &command,
                Identity::Restricted(&profile.token),
                &profile.launcher.pipe_client,
                b"fixture".to_vec(),
                &worker_stop,
                deadline,
                |child| profile.verify_child(child),
            )
        })
    }

    /// The production spawn path: CreateProcessAsUserW with the script token,
    /// the creation-time Job and child-token verification before resume. A
    /// missing default DACL fails here with 0xC0000142. No skip: an unsupported
    /// runner fails this test.
    #[test]
    fn restricted_child_starts_in_the_job_and_cannot_write_up() {
        let root = TestRoot::new("probe");
        let profile = Arc::new(test_profile(&root.0));
        let allowed = Workspace::create(&profile.launcher).unwrap();
        let denied = root.0.join("denied");
        std::fs::create_dir(&denied).unwrap();
        std::fs::write(denied.join("seeded"), b"seed").unwrap();
        let command = fixture_command(&[
            (JOB_MODE, "probe".to_owned()),
            (PROBE_ALLOWED, allowed.path().display().to_string()),
            (PROBE_DENIED, denied.display().to_string()),
        ]);
        assert_eq!(
            run_restricted_fixture(&profile, command),
            Ok(ScriptOutcome::Exited {
                exit_code: PROBE_PASSED
            })
        );
        assert!(allowed.path().join("created").exists());
        assert!(!denied.join("created").exists());
        allowed.remove().unwrap();
        for code in [259, 0x8000_0000] {
            let command = fixture_command(&[
                (JOB_MODE, "exit".to_owned()),
                (FIXTURE_EXIT, code.to_string()),
            ]);
            assert_eq!(
                run_restricted_fixture(&profile, command),
                Ok(ScriptOutcome::Exited { exit_code: code })
            );
        }
    }

    #[test]
    #[allow(clippy::zombie_processes)] // The supervising test waits on the contained descendant.
    fn job_fixture_process() {
        let Ok(mode) = std::env::var(JOB_MODE) else {
            return;
        };
        if mode == "probe" {
            let allowed = PathBuf::from(std::env::var(PROBE_ALLOWED).unwrap());
            let denied = PathBuf::from(std::env::var(PROBE_DENIED).unwrap());
            let mut bits = 0x40;
            if File::create(allowed.join("created")).is_ok() {
                bits |= 1;
            }
            if File::create(denied.join("created")).is_err() {
                bits |= 2;
            }
            if std::fs::read(denied.join("seeded")).is_ok_and(|bytes| bytes == b"seed") {
                bits |= 4;
            }
            std::process::exit(bits);
        }
        if mode == "exit" {
            let code: u32 = std::env::var(FIXTURE_EXIT).unwrap().parse().unwrap();
            std::process::exit(code as i32);
        }
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
        let command = fixture_command(&[
            (JOB_MODE, "parent".to_owned()),
            (JOB_READY, name),
            (
                JOB_SENTINEL,
                (sentinel.as_raw_handle() as usize).to_string(),
            ),
        ]);
        let (input, _writer) = stdin_pipe(4096, &current_user_sid()).unwrap();
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
        let (reader, _writer) = stdin_pipe(4096, &current_user_sid()).unwrap();
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
        let root = TestRoot::new("env");
        let (windows, system) = system_paths().unwrap();
        let command = CommandLine::powershell(&windows, &system, &root.0).unwrap();
        let values = command.env.split(|&unit| unit == 0);
        let entries: Vec<(String, String)> = values
            .filter(|value| !value.is_empty())
            .map(|value| {
                let entry = String::from_utf16(value).unwrap();
                let (key, value) = entry.split_once('=').unwrap();
                (key.to_owned(), value.to_owned())
            })
            .collect();
        let keys: Vec<_> = entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "APPDATA",
                "LOCALAPPDATA",
                "PATH",
                "PSModuleAnalysisCachePath",
                "PSModulePath",
                "SystemRoot",
                "TEMP",
                "TMP",
                "USERPROFILE",
                "windir"
            ]
        );
        let mut sorted = keys.clone();
        sorted.sort_by_key(|key| key.to_ascii_uppercase());
        assert_eq!(
            keys, sorted,
            "environment blocks are sorted case-insensitively"
        );
        let work = root.0.display().to_string();
        for (key, value) in &entries {
            if matches!(
                key.as_str(),
                "APPDATA"
                    | "LOCALAPPDATA"
                    | "PSModuleAnalysisCachePath"
                    | "TEMP"
                    | "TMP"
                    | "USERPROFILE"
            ) {
                assert!(
                    value.starts_with(&work),
                    "{key} stays in the request directory"
                );
            }
        }
        assert!(command.env.ends_with(&[0, 0]));
        let script = String::from_utf16(&command.command).unwrap();
        assert!(
            script.contains("-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command")
        );
        assert!(!WRAPPER.contains("New-Object") && WRAPPER.contains("$env:PSModulePath="));
    }
}

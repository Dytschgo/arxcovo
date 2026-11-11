//! SCM glue. All callbacks use process-lifetime context; no console signal path.
use super::{ExitCode, Reporter, Shutdown, State};
use std::{
    ptr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::*,
    Security::*,
    System::{Services::*, Threading::*},
};
const NAME: [u16; 5] = [97, 114, 120, 100, 0];
static CONTEXT: OnceLock<Context> = OnceLock::new();
static EXIT: AtomicU32 = AtomicU32::new(7);
struct Context {
    request: Shutdown,
    status: Mutex<Status>,
}
struct Status {
    handle: usize,
    state: State,
    checkpoint: u32,
    code: ExitCode,
}
impl Status {
    fn update(&mut self, next: State, code: ExitCode) {
        // Once STOP_PENDING is published a racing startup cannot publish RUNNING.
        if self.state == State::Stopped
            || (self.state == State::Stopping && matches!(next, State::Starting | State::Running))
        {
            return;
        }
        if self.state == next && matches!(next, State::Starting | State::Stopping) {
            self.checkpoint = self.checkpoint.saturating_add(1);
        } else {
            self.checkpoint = u32::from(matches!(next, State::Starting | State::Stopping));
        }
        self.state = next;
        self.code = code;
    }
    fn native(&self) -> SERVICE_STATUS {
        SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: match self.state {
                State::Starting => SERVICE_START_PENDING,
                State::Running => SERVICE_RUNNING,
                State::Stopping => SERVICE_STOP_PENDING,
                State::Stopped => SERVICE_STOPPED,
            },
            dwControlsAccepted: if self.state == State::Running {
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_PRESHUTDOWN
            } else {
                0
            },
            dwWin32ExitCode: if self.code == ExitCode::Clean {
                0
            } else {
                ERROR_SERVICE_SPECIFIC_ERROR
            },
            dwServiceSpecificExitCode: self.code as u32,
            dwCheckPoint: self.checkpoint,
            dwWaitHint: match self.state {
                State::Starting => 10_000,
                State::Stopping => 5_000,
                _ => 0,
            },
        }
    }
    fn publish(&self) -> Result<(), ExitCode> {
        if unsafe { SetServiceStatus(self.handle as SERVICE_STATUS_HANDLE, &self.native()) } == 0 {
            Err(ExitCode::NotServiceManager)
        } else {
            Ok(())
        }
    }
}
impl Reporter for &Context {
    fn report(&mut self, state: State, code: ExitCode) -> Result<(), ExitCode> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| ExitCode::NotServiceManager)?;
        status.update(state, code);
        status.publish()
    }
}
#[derive(Debug, PartialEq, Eq)]
enum Control {
    Stop,
    Interrogate,
    Unsupported,
}
fn control(code: u32) -> Control {
    match code {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_PRESHUTDOWN => Control::Stop,
        SERVICE_CONTROL_INTERROGATE => Control::Interrogate,
        _ => Control::Unsupported,
    }
}
unsafe extern "system" fn handler(
    code: u32,
    _: u32,
    _: *mut core::ffi::c_void,
    _: *mut core::ffi::c_void,
) -> u32 {
    let Some(context) = CONTEXT.get() else {
        return ERROR_CALL_NOT_IMPLEMENTED;
    };
    match control(code) {
        Control::Stop => {
            // Latch first, even if reporting fails. The host handles cancellation.
            context.request.cancel();
            let mut reporter = context;
            if reporter.report(State::Stopping, ExitCode::Clean).is_err() {
                return ERROR_INVALID_HANDLE;
            }
        }
        Control::Interrogate => {
            let Ok(status) = context.status.lock() else {
                return ERROR_INVALID_HANDLE;
            };
            if status.publish().is_err() {
                return ERROR_INVALID_HANDLE;
            }
        }
        Control::Unsupported => return ERROR_CALL_NOT_IMPLEMENTED,
    }
    NO_ERROR
}
unsafe extern "system" fn service_main(_: u32, _: *mut *mut u16) {
    let context = CONTEXT.get_or_init(|| Context {
        request: Shutdown::default(),
        status: Mutex::new(Status {
            handle: 0,
            state: State::Starting,
            checkpoint: 0,
            code: ExitCode::Clean,
        }),
    });
    let handle =
        unsafe { RegisterServiceCtrlHandlerExW(NAME.as_ptr(), Some(handler), ptr::null()) };
    if handle.is_null() {
        EXIT.store(10, Ordering::Release);
        return;
    }
    if let Ok(mut status) = context.status.lock() {
        status.handle = handle as usize;
    }
    let mut reporter = context;
    let result = (|| {
        reporter.report(State::Starting, ExitCode::Clean)?;
        // Before creating any service object, set only this process's default
        // owner. No privilege adjustment and no machine/account mutation.
        set_current_token_owner()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|_| ExitCode::Layout)?;
        let code = runtime.block_on(run_application(
            &mut super::application::ServiceApplication::native(),
        ));
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        if code == ExitCode::Clean {
            Ok(())
        } else {
            Err(code)
        }
    })();
    let mut code = result.err().unwrap_or(ExitCode::Clean);
    if reporter.report(State::Stopping, ExitCode::Clean).is_err() {
        code = ExitCode::NotServiceManager;
    }
    if reporter.report(State::Stopped, code).is_err() {
        code = ExitCode::NotServiceManager;
    }
    EXIT.store(code as u32, Ordering::Release);
}
pub fn run() -> ExitCode {
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: NAME.as_ptr().cast_mut(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        // ERROR_FAILED_SERVICE_CONTROLLER_CONNECT (ordinary console invocation)
        // and all dispatcher failures fail closed; never start foreground mode.
        return ExitCode::NotServiceManager;
    }
    match EXIT.load(Ordering::Acquire) {
        0 => ExitCode::Clean,
        2 => ExitCode::Usage,
        3 => ExitCode::Config,
        4 => ExitCode::Permissions,
        5 => ExitCode::Store,
        6 => ExitCode::Endpoint,
        10 => ExitCode::NotServiceManager,
        _ => ExitCode::Layout,
    }
}

/// Called on ServiceMain's runtime with the fixed application adapter.
/// Calling outside the dispatcher cannot start an application.
pub async fn run_application<A: super::Application>(app: &mut A) -> ExitCode {
    let Some(context) = CONTEXT.get() else {
        return ExitCode::NotServiceManager;
    };
    super::Host::run(app, context.request.clone(), &mut &*context).await
}

/// SetTokenOwner = TokenUser on the current process only. The allocation is
/// word-aligned because TOKEN_USER contains a SID pointer.
pub fn set_current_token_owner() -> Result<(), ExitCode> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
            &mut token,
        ) == 0
        {
            return Err(ExitCode::Permissions);
        }
        let result = (|| {
            let mut size = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
            if size == 0 {
                return Err(ExitCode::Permissions);
            }
            let mut data = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(token, TokenUser, data.as_mut_ptr().cast(), size, &mut size) == 0
            {
                return Err(ExitCode::Permissions);
            }
            let user = &*data.as_ptr().cast::<TOKEN_USER>();
            let owner = TOKEN_OWNER {
                Owner: user.User.Sid,
            };
            if SetTokenInformation(
                token,
                TokenOwner,
                (&owner as *const TOKEN_OWNER).cast(),
                size_of::<TOKEN_OWNER>() as u32,
            ) == 0
            {
                return Err(ExitCode::Permissions);
            }
            Ok(())
        })();
        CloseHandle(token);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scm_controls_checkpoints_and_stop_race() {
        assert_eq!(control(SERVICE_CONTROL_STOP), Control::Stop);
        assert_eq!(control(SERVICE_CONTROL_PRESHUTDOWN), Control::Stop);
        assert_eq!(control(SERVICE_CONTROL_INTERROGATE), Control::Interrogate);
        assert_eq!(control(SERVICE_CONTROL_PAUSE), Control::Unsupported);
        let mut status = Status {
            handle: 0,
            state: State::Starting,
            checkpoint: 0,
            code: ExitCode::Clean,
        };
        status.update(State::Starting, ExitCode::Clean);
        assert_eq!(status.native().dwWaitHint, 10_000);
        status.update(State::Starting, ExitCode::Clean);
        assert_eq!(status.native().dwCheckPoint, 2);
        status.update(State::Running, ExitCode::Clean);
        assert_eq!(
            status.native().dwControlsAccepted,
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_PRESHUTDOWN
        );
        assert_eq!(status.native().dwCheckPoint, 0);
        status.update(State::Stopping, ExitCode::Clean);
        status.update(State::Running, ExitCode::Clean);
        assert_eq!(status.native().dwCurrentState, SERVICE_STOP_PENDING);
        assert_eq!(status.native().dwWaitHint, 5_000);
        status.update(State::Stopping, ExitCode::Clean);
        assert_eq!(status.native().dwCheckPoint, 2);
        status.update(State::Stopped, ExitCode::Store);
        assert_eq!(
            status.native().dwWin32ExitCode,
            ERROR_SERVICE_SPECIFIC_ERROR
        );
        assert_eq!(status.native().dwServiceSpecificExitCode, 5);
        assert_eq!(status.native().dwCheckPoint, 0);
        status.update(State::Running, ExitCode::Clean);
        assert_eq!(status.native().dwCurrentState, SERVICE_STOPPED);
    }
}

#[cfg(test)]
mod owner_tests {
    use super::*;
    #[tokio::test]
    async fn current_process_owner_change_is_isolated_and_real() {
        for (filter, expected_ran) in [
            ("service::windows::owner_tests::owner_fixture", true),
            (
                "service::windows::owner_tests::missing_owner_fixture",
                false,
            ),
        ] {
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                tokio::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        filter,
                        "--ignored",
                        "--format",
                        "pretty",
                        "--color",
                        "never",
                    ])
                    .env("ARX_SERVICE_OWNER_FIXTURE", "1")
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(
                output.status.success(),
                "isolated TokenOwner fixture failed"
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            let ran = stdout.contains("test service::windows::owner_tests::owner_fixture ... ok")
                && stdout.contains("test result: ok. 1 passed;");
            assert_eq!(ran, expected_ran, "TokenOwner fixture execution evidence");
            if !expected_ran {
                assert!(stdout.contains("test result: ok. 0 passed;"));
            }
        }
    }
    #[test]
    #[ignore = "subprocess-only current-token fixture; never changes the parent token"]
    fn owner_fixture() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Security::Authorization::*;
        assert_eq!(
            std::env::var("ARX_SERVICE_OWNER_FIXTURE").as_deref(),
            Ok("1")
        );
        set_current_token_owner().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("created-after-token-owner");
        std::fs::write(&file, b"not configuration").unwrap();
        unsafe {
            let mut token = ptr::null_mut();
            assert_ne!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
                0
            );
            let mut size = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
            let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
            assert_ne!(
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    size,
                    &mut size
                ),
                0
            );
            let sid = (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            for path in [temp.path(), file.as_path()] {
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let mut descriptor = ptr::null_mut();
                let mut owner = ptr::null_mut();
                assert_eq!(
                    GetNamedSecurityInfoW(
                        wide.as_ptr(),
                        SE_FILE_OBJECT,
                        OWNER_SECURITY_INFORMATION,
                        &mut owner,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        &mut descriptor
                    ),
                    0
                );
                let equal = EqualSid(owner, sid);
                LocalFree(descriptor);
                assert_ne!(
                    equal, 0,
                    "new object owner must equal TokenUser, never an owner group"
                );
            }
            CloseHandle(token);
        }
    }
}

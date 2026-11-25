use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
    sync::mpsc,
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{Console::*, Threading::*},
};

struct Pseudoconsole(HPCON);

impl Drop for Pseudoconsole {
    fn drop(&mut self) {
        // SAFETY: creation succeeded, this is the sole owner, and output is drained separately.
        unsafe { ClosePseudoConsole(self.0) };
    }
}

struct ProcessAttributes {
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
    _storage: Vec<usize>,
}

impl Drop for ProcessAttributes {
    fn drop(&mut self) {
        // SAFETY: the initialized list lives in _storage until this method returns.
        unsafe { DeleteProcThreadAttributeList(self.pointer) };
    }
}

fn ok(result: i32) {
    assert_ne!(result, 0, "{}", io::Error::last_os_error());
}

impl Pseudoconsole {
    fn spawn(&self, application: &[u16], command: &mut [u16]) -> OwnedHandle {
        // SAFETY: all buffers outlive their calls, the list has aligned backing storage,
        // and a successful CreateProcessW returns two handles owned by this caller.
        unsafe {
            let mut bytes = 0;
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
            assert_ne!(bytes, 0);
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            let pointer = storage.as_mut_ptr().cast();
            ok(InitializeProcThreadAttributeList(pointer, 1, 0, &mut bytes));
            let attributes = ProcessAttributes {
                pointer,
                _storage: storage,
            };
            ok(UpdateProcThreadAttribute(
                attributes.pointer,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                self.0 as _,
                size_of::<HPCON>(),
                null_mut(),
                null(),
            ));
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            // Null standard handles make Windows supply the new console's
            // handles, even when this test's own handles are redirected.
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.lpAttributeList = attributes.pointer;
            let mut child = PROCESS_INFORMATION::default();
            ok(CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT,
                null(),
                null(),
                &startup.StartupInfo,
                &mut child,
            ));
            drop(OwnedHandle::from_raw_handle(child.hThread));
            OwnedHandle::from_raw_handle(child.hProcess)
        }
    }
}

fn provision_with_console_input(state: &Path, input_bytes: &[u8]) -> (u32, String) {
    let (input, mut keys) = io::pipe().unwrap();
    let (mut output, screen) = io::pipe().unwrap();
    let mut console = 0;
    // SAFETY: the pipe handles are live and console has writable storage.
    let result = unsafe {
        CreatePseudoConsole(
            COORD { X: 80, Y: 25 },
            input.as_raw_handle(),
            screen.as_raw_handle(),
            0,
            &mut console,
        )
    };
    assert_eq!(result, 0, "CreatePseudoConsole failed");
    drop((input, screen));
    let (ready, prompt) = mpsc::channel();
    let capture = thread::spawn(move || {
        let mut ready = Some(ready);
        let mut text = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let read = output.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            text.extend_from_slice(&chunk[..read]);
            let marker = b"Choose the dashboard password for user 'owner'";
            if ready.is_some() && text.windows(marker.len()).any(|part| part == marker) {
                let _ = ready.take().unwrap().send(());
            }
        }
        text
    });
    let console = Pseudoconsole(console);
    let wide = |text: &OsStr| text.encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let binary = env!("CARGO_BIN_EXE_arxd");
    let command = format!(
        "\"{binary}\" dashboard-password --state-dir \"{}\"",
        state.display()
    );
    let child = console.spawn(&wide(OsStr::new(binary)), &mut wide(OsStr::new(&command)));
    prompt
        .recv_timeout(Duration::from_secs(10))
        .expect("arxd did not show the console password prompt");
    keys.write_all(input_bytes).unwrap();
    // Keep input open. EOF would unblock an implementation still waiting for LF.
    // SAFETY: the child handle is owned by this test and stays live through the wait.
    let result = unsafe { WaitForSingleObject(child.as_raw_handle(), 10_000) };
    if result == WAIT_TIMEOUT {
        // SAFETY: only the isolated fixture child can be terminated here.
        unsafe { TerminateProcess(child.as_raw_handle(), 1) };
    }
    drop(console);
    let captured = capture.join().unwrap();
    assert_eq!(
        result, WAIT_OBJECT_0,
        "arxd did not exit after console input"
    );
    let mut status = 0;
    // SAFETY: the child handle is live and status has writable storage.
    ok(unsafe { GetExitCodeProcess(child.as_raw_handle(), &mut status) });
    (status, String::from_utf8(captured).unwrap())
}

#[test]
fn windows_console_enter_provisions_password_without_echo() {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    let state = super::private_dir();
    let fixture = "synthetic-console-sentinel";
    let (status, output) =
        provision_with_console_input(state.path(), format!("{fixture}\r{fixture}\r").as_bytes());
    assert_eq!(status, 0, "arxd refused synthetic console input");
    assert!(!output.contains(fixture));
    assert!(output.contains("Repeat password (input hidden):"));
    assert!(output.contains("Dashboard password saved for 'owner'. Sign in"));
    let hash = std::fs::read_to_string(state.path().join("dashboard-owner.phc")).unwrap();
    Argon2::default()
        .verify_password(fixture.as_bytes(), &PasswordHash::new(&hash).unwrap())
        .unwrap();
}

#[test]
fn windows_console_ctrl_c_cancels_password_entry() {
    use windows_sys::Win32::Foundation::STATUS_CONTROL_C_EXIT;

    let state = super::private_dir();
    let fixture = "synthetic-cancelled-console-sentinel";
    let (status, output) =
        provision_with_console_input(state.path(), format!("{fixture}\x03").as_bytes());
    assert_eq!(status, STATUS_CONTROL_C_EXIT as u32);
    assert!(!output.contains(fixture));
    assert!(!state.path().join("dashboard-owner.phc").exists());
}

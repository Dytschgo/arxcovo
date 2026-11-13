//! Inactive bridge's standalone Unix child protocol. No service state access.
#[path = "unix_shim_rights.rs"]
mod rights;
#[cfg(unix)]
use std::{
    ffi::{CString, OsString},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, process::ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn with_identity(uid: u32, step: impl FnOnce() -> i32) -> i32 {
    if uid == 0 { 125 } else { step() }
}

/// Only main's reserved Unix routes call this, before creating any threads.
#[cfg(unix)]
pub fn entry(args: &[OsString]) -> i32 {
    silent_entry(|| with_identity(unsafe { libc::geteuid() }, || body(args)))
}

#[cfg(unix)]
fn silent_entry(body: impl FnOnce() -> i32 + std::panic::UnwindSafe) -> i32 {
    std::panic::set_hook(Box::new(|_| {}));
    std::panic::catch_unwind(body).unwrap_or(125)
}

#[cfg(unix)]
fn body(args: &[OsString]) -> i32 {
    if args.len() == 1 && args[0] == "tool-probe" {
        return probe();
    }
    if args.len() < 4 || args[0] != "exec-child" {
        return 125;
    }
    let Some(text): Option<Vec<_>> = args.iter().map(|s| s.to_str().map(str::to_owned)).collect()
    else {
        return 125;
    };
    let (mode, target, cwd) = (&text[1], &text[2], &text[3]);
    let (tool, executable) = match mode.as_str() {
        "path" => (None, Some(target.clone())),
        "tool" => (Some(target.clone()), None),
        _ => return 125,
    };
    let request = crate::service::executor::Request {
        tool,
        executable,
        cwd: cwd.clone(),
        args: text[4..].to_vec(),
        timeout_s: 60,
        output_bytes: 4096,
    };
    if !request.validate() || change_cwd(cwd).is_err() {
        return 125;
    }
    let start = |path: &Path| {
        Command::new(path)
            .args(&request.args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
    };
    let child = if mode == "path" {
        start(Path::new(target)).map_err(start_code)
    } else {
        resolve(target, start)
    };
    match child {
        Ok(mut child) => match child.wait() {
            Ok(status) => status
                .code()
                .or_else(|| status.signal().map(|s| 128 + s))
                .unwrap_or(126),
            Err(_) => 126,
        },
        Err(code) => code,
    }
}

#[cfg(unix)]
fn start_code(error: std::io::Error) -> i32 {
    match error.raw_os_error() {
        Some(libc::ENOENT | libc::ENOTDIR) => 127,
        _ => 126,
    }
}

// Count entries before filtering: even skipped entries consume the 64-entry budget.
#[cfg(unix)]
fn resolve(
    name: &str,
    mut start: impl FnMut(&Path) -> std::io::Result<Child>,
) -> Result<Child, i32> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut denied = false;
    for entry in path.as_bytes().split(|b| *b == b':').take(64) {
        let Ok(text) = std::str::from_utf8(entry) else {
            continue;
        };
        if !clean_directory(text) {
            continue;
        }
        let candidate = PathBuf::from(text).join(name);
        match start(&candidate) {
            Ok(child) => return Ok(child),
            Err(error) => match error.raw_os_error() {
                Some(libc::ENOENT | libc::ENOTDIR) => (),
                Some(libc::EACCES) => denied = true,
                _ => return Err(126),
            },
        }
    }
    Err(if denied { 126 } else { 127 })
}

#[cfg(unix)]
fn clean_directory(s: &str) -> bool {
    s.starts_with('/')
        && !s.starts_with("//")
        && s.len() <= 4096
        && !s.chars().any(char::is_control)
        && !s.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && s.split('/')
            .all(|c| c != "." && c != ".." && !c.ends_with([' ', '.']))
}

#[cfg(unix)]
fn change_cwd(cwd: &str) -> std::io::Result<()> {
    if cwd.is_empty() {
        return Ok(());
    }
    let fd = open_cwd(cwd)?;
    if unsafe { libc::fchdir(fd.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn open_cwd(cwd: &str) -> std::io::Result<OwnedFd> {
    #[repr(C)]
    struct OpenHow {
        flags: u64,
        mode: u64,
        resolve: u64,
    }
    let name = CString::new(cwd)?;
    let how = OpenHow {
        flags: (libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64,
        mode: 0,
        resolve: 0x08 | 0x04,
    };
    let raw = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            libc::AT_FDCWD,
            name.as_ptr(),
            &how,
            std::mem::size_of::<OpenHow>(),
        )
    };
    if raw < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(raw as i32) })
    }
}

#[cfg(target_os = "macos")]
fn open_cwd(cwd: &str) -> std::io::Result<OwnedFd> {
    let raw = unsafe {
        libc::open(
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut fd = unsafe { OwnedFd::from_raw_fd(raw) };
    for component in cwd.split('/').filter(|s| !s.is_empty()) {
        let name = CString::new(component)?;
        let raw = unsafe {
            libc::openat(
                fd.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        fd = unsafe { OwnedFd::from_raw_fd(raw) };
    }
    Ok(fd)
}

#[cfg(unix)]
fn probe() -> i32 {
    if unsafe { libc::chdir(c"/".as_ptr()) } != 0 {
        return 125;
    }
    let mut output = Vec::new();
    for name in crate::development::TOOLS {
        let child = resolve(name, |path| {
            Command::new(path)
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        });
        let found = child.is_ok();
        let version = child.ok().and_then(probe_version);
        let line = serde_json::json!({"name":name,"found":found,"version":version});
        if serde_json::to_writer(&mut output, &line).is_err() {
            return 125;
        }
        output.push(b'\n');
    }
    if !rights::append(&mut output, rights::observe()) {
        return 125;
    }
    if std::io::stdout().lock().write_all(&output).is_err() {
        return 125;
    }
    0
}

// Guard reaps even on an unwind; no reader thread can outlive the deadline.
#[cfg(unix)]
struct ProbeChild(Child);
#[cfg(unix)]
impl Drop for ProbeChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(unix)]
fn probe_version(child: Child) -> Option<String> {
    let mut child = ProbeChild(child);
    let mut pipe = child.0.stdout.take()?;
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return None;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let mut eof = false;
    let mut exited = false;
    while Instant::now() < deadline {
        // One bounded read per tick, including when discarding beyond the cap.
        let mut buffer = [0u8; 4096];
        match pipe.read(&mut buffer) {
            Ok(0) => eof = true,
            Ok(n) => {
                let keep = n.min(4096usize.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..keep]);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return None,
        }
        if !exited {
            exited = child.0.try_wait().ok()?.is_some();
        }
        if eof && exited {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if !exited || !eof {
        return None;
    }
    version(&bytes)
}

#[cfg(unix)]
fn version(bytes: &[u8]) -> Option<String> {
    let first = bytes.split(|b| *b == b'\n').next()?;
    let text = std::str::from_utf8(first).ok()?.trim();
    if text.is_empty()
        || text.len() > 128
        || !text
            .bytes()
            .all(|b| (32..=126).contains(&b) && b != b'/' && b != b'\\')
    {
        return None;
    }
    Some(text.to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn s5_injected_identity_refuses_root() {
        assert_eq!(super::with_identity(0, || panic!("must not run")), 125);
        assert_eq!(super::with_identity(1000, || 17), 17);
    }
    #[cfg(unix)]
    include!("unix_shim_tests.rs");
}

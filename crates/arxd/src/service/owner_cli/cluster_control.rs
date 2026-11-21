//! Explicit owner client. No standard stream is used for transfer.
use crate::service::cluster_owner::{self, Failure, Paste, Reply, Request};
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::unix::AsyncFd,
    signal::unix::{Signal, SignalKind, signal},
    time::timeout,
};

const INPUT_WAIT: Duration = Duration::from_secs(60);
const DISPLAY_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
enum Action {
    Mint,
    Join,
}

fn parse(args: &[OsString]) -> Option<(Action, PathBuf, u32)> {
    let [service, control, verb, endpoint, path, daemon, uid] = args else {
        return None;
    };
    if service != "service"
        || control != "cluster-control"
        || endpoint != "--socket"
        || daemon != "--daemon-uid"
    {
        return None;
    }
    let action = match verb.to_str()? {
        "mint" => Action::Mint,
        "join" => Action::Join,
        _ => return None,
    };
    let path = Path::new(path);
    let uid = uid.to_str()?;
    if !cluster_owner::address(path)
        || uid.is_empty()
        || uid.len() > 10
        || !uid.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let uid = uid.parse().ok()?;
    (uid != 0 && uid != u32::MAX).then(|| (action, path.into(), uid))
}

pub(super) fn run(args: &[OsString]) -> i32 {
    let Some((action, path, daemon)) = parse(args) else {
        eprintln!(
            "arxd: usage: arxd service cluster-control mint|join --socket PATH --daemon-uid UID"
        );
        return 2;
    };
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failure::Refused)
        .and_then(|runtime| runtime.block_on(control(action, &path, daemon)));
    match result {
        Ok(()) => 0,
        Err(Failure::Refused) => {
            eprintln!("arxd: cluster-control-refused");
            6
        }
        Err(Failure::Unknown) => {
            eprintln!("arxd: cluster-control-unknown");
            8
        }
    }
}

async fn control(action: Action, path: &Path, daemon: u32) -> Result<(), Failure> {
    let mut signals = Signals::new().map_err(|_| Failure::Refused)?;
    let tty = Terminal::open().map_err(|_| Failure::Refused)?;
    let mut submitted = false;
    let disposition = tokio::select! {
        biased;
        _ = signals.interrupted() => {
            Err(if submitted { Failure::Unknown } else { Failure::Refused })
        }
        result = execute(&tty, action, path, daemon, &mut submitted) => result,
    };
    disposition
}

async fn execute(
    tty: &Terminal,
    action: Action,
    path: &Path,
    daemon: u32,
    submitted: &mut bool,
) -> Result<(), Failure> {
    let request = match action {
        Action::Mint => Request::Mint,
        Action::Join => Request::Join(
            timeout(INPUT_WAIT, tty.read_paste())
                .await
                .map_err(|_| Failure::Refused)?
                .map_err(|_| Failure::Refused)?,
        ),
    };
    let stream = cluster_owner::connect(path, daemon).await?;
    tty.foreground().map_err(|_| Failure::Refused)?;
    *submitted = true;
    match cluster_owner::request(stream, request).await? {
        Reply::Minted(paste) => timeout(DISPLAY_WAIT, tty.display(&paste))
            .await
            .map_err(|_| Failure::Unknown)?
            .map_err(|_| Failure::Unknown),
        Reply::Joined => Ok(()),
        Reply::Refused => Err(Failure::Refused),
        Reply::Unknown => Err(Failure::Unknown),
    }
}

struct Signals([Signal; 8]);

impl Signals {
    fn new() -> io::Result<Self> {
        Ok(Self([
            signal(SignalKind::interrupt())?,
            signal(SignalKind::terminate())?,
            signal(SignalKind::hangup())?,
            signal(SignalKind::quit())?,
            signal(SignalKind::from_raw(libc::SIGTSTP))?,
            signal(SignalKind::from_raw(libc::SIGTTIN))?,
            signal(SignalKind::from_raw(libc::SIGTTOU))?,
            signal(SignalKind::from_raw(libc::SIGPIPE))?,
        ]))
    }

    async fn interrupted(&mut self) {
        let [int, term, hup, quit, tstp, ttin, ttou, pipe] = &mut self.0;
        tokio::select! {
            _ = int.recv() => {}
            _ = term.recv() => {}
            _ = hup.recv() => {}
            _ = quit.recv() => {}
            _ = tstp.recv() => {}
            _ = ttin.recv() => {}
            _ = ttou.recv() => {}
            _ = pipe.recv() => {}
        }
    }
}

struct Terminal(AsyncFd<File>);

impl Terminal {
    fn open() -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY)
            .open("/dev/tty")?;
        let tty = Self(AsyncFd::new(file)?);
        tty.foreground()?;
        Ok(tty)
    }

    fn foreground(&self) -> io::Result<()> {
        let fd = self.0.as_raw_fd();
        let mut session = 0;
        // SAFETY: fd is owned and TIOCGSID writes one pid_t to valid storage.
        if unsafe {
            libc::ioctl(fd, libc::TIOCGSID, &mut session) == 0
                && session == libc::getsid(0)
                && libc::tcgetpgrp(fd) == libc::getpgrp()
        } {
            Ok(())
        } else {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }

    async fn read_paste(&self) -> io::Result<Paste> {
        let mut mode = NoEcho::enter(self)?;
        let mut paste = Paste::empty();
        let mut chunk = InputChunk([0; 1024]);
        loop {
            self.foreground()?;
            let mut ready = self.0.readable().await?;
            let Ok(read) = ready.try_io(|fd| fd.get_ref().read(&mut chunk.0)) else {
                continue;
            };
            let size = read?;
            if size == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let bytes = &chunk.0[..size];
            if let Some(end) = bytes.iter().position(|&byte| byte == b'\n') {
                if end + 1 != size
                    || !paste.extend(&bytes[..end])
                    || !cluster_owner::paste_shape(paste.bytes())
                {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                // A second already queued chunk belongs to the same input.
                let trailing = self.0.get_ref().read(&mut chunk.0[..1]);
                if !trailing.is_err_and(|e| e.kind() == io::ErrorKind::WouldBlock) {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                mode.restore()?;
                return Ok(paste);
            }
            if !paste.extend(bytes) || !bytes.iter().all(u8::is_ascii_graphic) {
                return Err(io::ErrorKind::InvalidData.into());
            }
        }
    }

    async fn display(&self, paste: &Paste) -> io::Result<()> {
        for mut bytes in [paste.bytes(), b"\n"] {
            while !bytes.is_empty() {
                self.foreground()?;
                let mut ready = self.0.writable().await?;
                if let Ok(n) = ready.try_io(|fd| fd.get_ref().write(bytes)) {
                    let n = n?;
                    if n == 0 {
                        return Err(io::ErrorKind::WriteZero.into());
                    }
                    bytes = &bytes[n..];
                }
            }
        }
        Ok(())
    }
}

struct InputChunk([u8; 1024]);

impl Drop for InputChunk {
    fn drop(&mut self) {
        cluster_owner::wipe(&mut self.0);
    }
}

struct NoEcho {
    fd: libc::c_int,
    original: libc::termios,
    active: bool,
}

impl NoEcho {
    fn enter(tty: &Terminal) -> io::Result<Self> {
        tty.foreground()?;
        let fd = tty.0.as_raw_fd();
        let mut original = std::mem::MaybeUninit::uninit();
        // SAFETY: tcgetattr initializes the supplied termios on success.
        if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful tcgetattr initialized original.
        let original: libc::termios = unsafe { original.assume_init() };
        let mut input = original;
        // SAFETY: input is valid initialized storage.
        unsafe { libc::cfmakeraw(&mut input) };
        input.c_iflag |= libc::ICRNL;
        input.c_cc[libc::VMIN] = 1;
        input.c_cc[libc::VTIME] = 0;
        // Create the guard first, including if the kernel partially changes settings.
        let guard = Self {
            fd,
            original,
            active: true,
        };
        set_mode(fd, &input, false)?;
        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        set_mode(self.fd, &self.original, true)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for NoEcho {
    fn drop(&mut self) {
        if self.active {
            let _ = self.restore();
        }
    }
}

fn set_mode(fd: libc::c_int, mode: &libc::termios, restoring: bool) -> io::Result<()> {
    // Block SIGTTOU on this thread so restoration works even after a job-control
    // foreground change. Neither ioctl waits for the output queue to drain.
    let mut mask = std::mem::MaybeUninit::uninit();
    let mut previous = std::mem::MaybeUninit::uninit();
    // SAFETY: initialized mask storage is passed to pthread_sigmask, which saves
    // the previous mask on success. No async suspension occurs in this section.
    let status = unsafe {
        libc::sigemptyset(mask.as_mut_ptr());
        libc::sigaddset(mask.as_mut_ptr(), libc::SIGTTOU);
        libc::pthread_sigmask(libc::SIG_BLOCK, mask.as_ptr(), previous.as_mut_ptr())
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    let change = || {
        // SAFETY: fd belongs to the still-live terminal and mode is initialized.
        tty_result(unsafe { libc::tcsetattr(fd, libc::TCSANOW, mode) })
    };
    let discard = || {
        // SAFETY: fd belongs to the still-live terminal. Only input is discarded.
        tty_result(unsafe { libc::tcflush(fd, libc::TCIFLUSH) })
    };
    let result = if restoring {
        // Attempt restoration even if discarding unread input reports an error.
        let discarded = discard();
        let restored = change();
        restored.and(discarded)
    } else {
        discard().and_then(|()| change())
    };
    // SAFETY: successful SIG_BLOCK wrote the previous mask.
    unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, previous.as_ptr(), std::ptr::null_mut()) };
    result
}

fn tty_result(status: libc::c_int) -> io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests;

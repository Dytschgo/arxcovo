//! Unix signal and systemd datagram seams; no daemon or privileged operations.
use super::{ExitCode, Reporter, Shutdown, State};
use std::{
    ffi::OsStr,
    io,
    os::unix::{ffi::OsStrExt, net::UnixDatagram},
    path::Path,
};
use tokio::signal::unix::{Signal, SignalKind, signal};

pub struct Signals {
    term: Signal,
    int: Signal,
    hup: Signal,
    pipe: Signal,
}
impl Signals {
    pub fn install() -> io::Result<Self> {
        Ok(Self {
            term: signal(SignalKind::terminate())?,
            int: signal(SignalKind::interrupt())?,
            hup: signal(SignalKind::hangup())?,
            pipe: signal(SignalKind::pipe())?,
        })
    }
    pub async fn forward(mut self, request: Shutdown) {
        loop {
            tokio::select! {
                _ = self.term.recv() => { request.cancel(); return; },
                _ = self.int.recv() => { request.cancel(); return; },
                _ = self.hup.recv() => {},
                _ = self.pipe.recv() => {},
                _ = request.cancelled() => return,
            }
        }
    }
}

pub struct Notify {
    socket: Option<std::ffi::OsString>,
}
impl Notify {
    pub fn from_environment() -> Self {
        Self {
            socket: std::env::var_os("NOTIFY_SOCKET"),
        }
    }
    pub fn send_to(socket: &OsStr, frame: &[u8]) -> io::Result<()> {
        let bytes = socket.as_bytes();
        if bytes.is_empty() || bytes.contains(&0) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let sender = UnixDatagram::unbound()?;
        sender.set_write_timeout(Some(std::time::Duration::from_millis(200)))?;
        let sent = if let Some(name) = bytes.strip_prefix(b"@") {
            #[cfg(target_os = "linux")]
            {
                use std::os::linux::net::SocketAddrExt;
                sender.send_to_addr(
                    frame,
                    &std::os::unix::net::SocketAddr::from_abstract_name(name)?,
                )?
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = name;
                return Err(io::ErrorKind::Unsupported.into());
            }
        } else {
            if !Path::new(socket).is_absolute() {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            sender.send_to(frame, Path::new(socket))?
        };
        if sent != frame.len() {
            return Err(io::ErrorKind::WriteZero.into());
        }
        Ok(())
    }
}
impl Reporter for Notify {
    fn report(&mut self, state: State, _: ExitCode) -> Result<(), ExitCode> {
        let frame: &[u8] = match state {
            State::Running => b"READY=1",
            State::Stopping => b"STOPPING=1",
            _ => return Ok(()),
        };
        if let Some(socket) = &self.socket {
            Self::send_to(socket, frame).map_err(|_| ExitCode::Layout)?;
        }
        Ok(())
    }
}
pub fn run() -> ExitCode {
    // Service-only process: ensure no temporary socket/journal is exposed.
    unsafe {
        libc::umask(0o077);
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::Layout,
    };
    let code = runtime.block_on(run_application(
        &mut super::application::ServiceApplication::native(),
    ));
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    code
}

/// Lifecycle seam used by the fixed-layout adapter. Signals are installed
/// before startup and remain active through bounded cleanup.
pub async fn run_application<A: super::Application>(app: &mut A) -> ExitCode {
    let signals = match Signals::install() {
        Ok(signals) => signals,
        Err(_) => return ExitCode::Layout,
    };
    let request = Shutdown::default();
    let forward = tokio::spawn(signals.forward(request.clone()));
    let code = super::Host::run(app, request.clone(), &mut Notify::from_environment()).await;
    request.cancel();
    let _ = forward.await;
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filesystem_notifications_are_actual_datagrams() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("notify");
        let receiver = UnixDatagram::bind(&path).unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut reporter = Notify {
            socket: Some(path.into_os_string()),
        };
        let mut bytes = [0; 64];
        for (state, expected) in [(State::Running, "READY=1"), (State::Stopping, "STOPPING=1")] {
            reporter.report(state, ExitCode::Clean).unwrap();
            let size = receiver.recv(&mut bytes).unwrap();
            assert_eq!(&bytes[..size], expected.as_bytes());
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn abstract_notifications_are_actual_datagrams() {
        use std::os::linux::net::SocketAddrExt;
        let name = format!("arxcovo-notify-test-{}", std::process::id());
        let address = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
        let receiver = UnixDatagram::bind_addr(&address).unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        Notify::send_to(OsStr::new(&format!("@{name}")), b"READY=1").unwrap();
        let mut bytes = [0; 64];
        let size = receiver.recv(&mut bytes).unwrap();
        assert_eq!(&bytes[..size], b"READY=1");
        Notify::send_to(OsStr::new(&format!("@{name}")), b"STOPPING=1").unwrap();
        let size = receiver.recv(&mut bytes).unwrap();
        assert_eq!(&bytes[..size], b"STOPPING=1");
    }
}

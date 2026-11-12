//! Private credential writer. S3 will wire this module to the owner router.
#![allow(dead_code)] // Remove when S3 consumes create; no production dispatch yet.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io;
#[cfg(unix)]
#[path = "client_config/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "client_config/windows.rs"]
mod platform;
#[cfg(test)]
#[path = "client_config/tests.rs"]
mod tests;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClientError {
    Argument,
    Elevated,
    Parent,
    Exists,
    Io,
    Partial,
}
#[cfg(unix)]
type ClientHandle<'a> = std::os::fd::BorrowedFd<'a>;
#[cfg(windows)]
type ClientHandle<'a> = std::os::windows::io::BorrowedHandle<'a>;
trait PostCreateOps {
    #[cfg(unix)]
    fn mode(&mut self, file: ClientHandle<'_>) -> io::Result<()>;
    fn write(&mut self, file: ClientHandle<'_>, json: &[u8]) -> io::Result<()>;
    fn flush(&mut self, file: ClientHandle<'_>) -> io::Result<()>;
    #[cfg(unix)]
    fn sync_parent(&mut self, parent: ClientHandle<'_>) -> io::Result<()>;
    fn verify(&mut self, file: ClientHandle<'_>, expected_len: u64) -> io::Result<()>;
}

pub(crate) fn create(principal: &str, out: &str) -> Result<String, ClientError> {
    let layout = crate::service::layout::Layout::system().map_err(|_| ClientError::Io)?;
    let address = layout.endpoint().to_str().ok_or(ClientError::Io)?;
    #[cfg(windows)]
    let server = "S-1-5-18";
    #[cfg(unix)]
    let server = "0";
    create_with(
        principal,
        out,
        address,
        server,
        platform::elevated,
        |raw| getrandom::fill(raw).map_err(|_| ClientError::Io),
        &mut platform::NativeFileOps::new().map_err(|_| ClientError::Io)?,
    )
}
fn lexical(out: &str, windows: bool) -> bool {
    if out.is_empty() || out.len() > 4096 || out.bytes().any(|b| b == 0 || b < 32) {
        return false;
    }
    let rest = if windows {
        let b = out.as_bytes();
        if b.len() < 4
            || !b[0].is_ascii_alphabetic()
            || b[1] != b':'
            || b[2] != b'\\'
            || out[2..].contains([':', '/', '<', '>', '"', '|', '?', '*'])
        {
            return false;
        }
        &out[3..]
    } else {
        if !out.starts_with('/') || out.starts_with("//") || out.contains('\\') {
            return false;
        }
        &out[1..]
    };
    rest.split(if windows { '\\' } else { '/' }).all(|p| {
        !p.is_empty()
            && p != "."
            && p != ".."
            && (!windows || crate::service::executor::windows_component_safe(p))
    })
}
fn capacity(address: &str, server: &str) -> Result<usize, ClientError> {
    const EMPTY: usize =
        b"{\"address\":\"\",\"expected_server_identity\":\"\",\"token\":\"\"}".len();
    address
        .len()
        .checked_add(server.len())
        .and_then(|n| n.checked_add(64))
        .and_then(|n| n.checked_mul(6))
        .and_then(|n| n.checked_add(EMPTY))
        .ok_or(ClientError::Io)
}
struct Secrets {
    raw: [u8; 32],
    token: [u8; 64],
    json: Vec<u8>,
}
impl Secrets {
    fn new(capacity: usize) -> Self {
        Self {
            raw: [0; 32],
            token: [0; 64],
            json: Vec::with_capacity(capacity),
        }
    }
}
impl Drop for Secrets {
    fn drop(&mut self) {
        for byte in self
            .raw
            .iter_mut()
            .chain(self.token.iter_mut())
            .chain(self.json.iter_mut())
        {
            // Best effort for initialized owned storage only; no promise for OS,
            // allocator/compiler copies or aborts. Vec never grows after RNG.
            unsafe {
                std::ptr::write_volatile(byte, 0);
            }
        }
        #[cfg(test)]
        tests::observe_wipe(
            self.raw
                .iter()
                .chain(self.token.iter())
                .chain(self.json.iter())
                .all(|b| *b == 0),
        );
    }
}
#[derive(Serialize)]
struct Wire<'a> {
    address: &'a str,
    expected_server_identity: &'a str,
    token: &'a str,
}
fn serialize_owned(owner: &mut Secrets, address: &str, server: &str) -> Result<(), ClientError> {
    #[cfg(test)]
    tests::observe_allocation(true, owner.json.as_ptr() as usize, owner.json.capacity());
    let wire = Wire {
        address,
        expected_server_identity: server,
        token: std::str::from_utf8(&owner.token).map_err(|_| ClientError::Io)?,
    };
    let result = serde_json::to_writer(&mut owner.json, &wire).map_err(|_| ClientError::Io);
    #[cfg(test)]
    tests::observe_allocation(false, owner.json.as_ptr() as usize, owner.json.capacity());
    result
}
fn partial(result: io::Result<()>) -> Result<(), ClientError> {
    result.map_err(|_| ClientError::Partial)
}
fn create_with(
    principal: &str,
    out: &str,
    address: &str,
    server: &str,
    elevated: impl FnOnce() -> Result<bool, ClientError>,
    random: impl FnOnce(&mut [u8; 32]) -> Result<(), ClientError>,
    ops: &mut impl PostCreateOps,
) -> Result<String, ClientError> {
    if !crate::service::config::valid_principal(principal) || !lexical(out, cfg!(windows)) {
        return Err(ClientError::Argument);
    }
    if elevated()? {
        return Err(ClientError::Elevated);
    }
    let mut owner = Secrets::new(capacity(address, server)?);
    random(&mut owner.raw)?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, byte) in owner.raw.iter().enumerate() {
        owner.token[i * 2] = HEX[(byte >> 4) as usize];
        owner.token[i * 2 + 1] = HEX[(byte & 15) as usize];
    }
    let digest = Sha256::digest(owner.token.as_slice());
    serialize_owned(&mut owner, address, server)?;
    let parent = platform::Parent::open(out)?;
    let file = parent.create()?; // Exclusive create: all subsequent errors are Partial.
    #[cfg(unix)]
    use std::os::fd::AsFd;
    #[cfg(windows)]
    use std::os::windows::io::AsHandle;
    #[cfg(unix)]
    partial(ops.mode(file.as_fd()))?;
    #[cfg(unix)]
    let handle = file.as_fd();
    #[cfg(windows)]
    let handle = file.as_handle();
    partial(ops.write(handle, &owner.json))?;
    partial(ops.flush(handle))?;
    #[cfg(unix)]
    partial(ops.sync_parent(parent.handle()))?;
    partial(ops.verify(handle, owner.json.len() as u64))?;
    Ok(hex::encode(digest))
}

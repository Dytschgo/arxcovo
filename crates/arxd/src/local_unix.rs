use super::{ClientConfig, Endpoint};
use anyhow::{Result, bail};
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tokio::net::{UnixListener, UnixStream};
pub fn validate_identity(value: &str) -> Result<()> {
    let uid: u32 = value.parse()?;
    if uid.to_string() != value {
        bail!("invalid UID");
    }
    Ok(())
}
pub fn owned(path: &Path) -> Result<()> {
    crate::security::private(path)
}
pub fn validate_address(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.len() > 100 || path.file_name().is_none() {
        bail!("invalid socket path");
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("parent required"))?;
    crate::config::checked_path(parent)?;
    if service_address(value)? {
        crate::service::native::endpoint_guard(&crate::service::layout::Layout::system()?)?;
    } else {
        crate::security::private(parent)?;
    }
    if !parent.is_dir() {
        bail!("parent required");
    }
    Ok(())
}
fn service_address(value: &str) -> Result<bool> {
    Ok(Path::new(value) == crate::service::layout::Layout::system()?.endpoint())
}
pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
    dev: u64,
    ino: u64,
    owner: u32,
    uid: u32,
}
impl Listener {
    pub fn bind(endpoint: &Endpoint) -> Result<Self> {
        Self::bind_inner(endpoint, false)
    }
    pub fn bind_service(endpoint: &Endpoint) -> Result<Self> {
        if !service_address(&endpoint.address)? {
            bail!("fixed endpoint required");
        }
        crate::service::native::require_service_identity()?;
        Self::bind_inner(endpoint, true)
    }
    fn bind_inner(endpoint: &Endpoint, service: bool) -> Result<Self> {
        // The protected parent makes the brief bind/chmod window inaccessible to others.
        // bind refuses every existing object; never unlink a stale endpoint automatically.
        // Darwin may bind through a dangling symlink, unlike Linux. Inspect the
        // link itself before asking the OS to bind, without following its target.
        match std::fs::symlink_metadata(&endpoint.address) {
            Ok(_) => bail!("endpoint already exists"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let listener = UnixListener::bind(&endpoint.address)?;
        let m = std::fs::symlink_metadata(&endpoint.address)?;
        let mut listener = Self {
            listener,
            path: endpoint.address.clone().into(),
            dev: m.dev(),
            ino: m.ino(),
            owner: m.uid(),
            uid: endpoint.client_identity.parse()?,
        };
        std::fs::set_permissions(&endpoint.address, std::fs::Permissions::from_mode(0o600))?;
        if service {
            let name = std::ffi::CString::new(endpoint.address.as_str())?;
            if unsafe { libc::chown(name.as_ptr(), listener.uid, u32::MAX) } != 0 {
                bail!("endpoint ownership unavailable");
            }
            listener.owner = listener.uid;
        }
        Ok(listener)
    }
}
impl super::Acceptor for Listener {
    type Stream = UnixStream;
    async fn accept(&mut self) -> Result<Option<UnixStream>> {
        let (stream, _) = match self.listener.accept().await {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        // Credential lookup can fail after an early disconnect (notably macOS
        // LOCAL_PEEREPID). Never admit that peer or tear down the listener.
        let identity_matches = stream
            .peer_cred()
            .map(|cred| cred.uid() == self.uid)
            .map_err(Into::into);
        Ok(super::verified_peer(stream, identity_matches))
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // Never remove a replacement, symlink or foreign object. Foreground-owner
        // path replacement is outside this dev server's same-owner trust boundary.
        if let Ok(m) = std::fs::symlink_metadata(&self.path)
            && m.file_type().is_socket()
            && m.dev() == self.dev
            && m.ino() == self.ino
            && m.uid() == self.owner
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
pub async fn connect(config: &ClientConfig) -> Result<UnixStream> {
    if service_address(&config.address)? && config.expected_server_identity != "0" {
        bail!("service identity required");
    }
    validate_address(&config.address)?;
    crate::config::checked_path(Path::new(&config.address))?;
    crate::security::private(Path::new(&config.address))?;
    let stream = UnixStream::connect(&config.address).await?;
    if stream.peer_cred()?.uid().to_string() != config.expected_server_identity {
        bail!("endpoint identity mismatch");
    }
    Ok(stream)
}

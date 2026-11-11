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
    crate::security::private(parent)?;
    if !parent.is_dir() {
        bail!("parent required");
    }
    Ok(())
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
        // The protected parent makes the brief bind/chmod window inaccessible to others.
        // bind refuses every existing object; never unlink a stale endpoint automatically.
        let listener = UnixListener::bind(&endpoint.address)?;
        let m = std::fs::symlink_metadata(&endpoint.address)?;
        let listener = Self {
            listener,
            path: endpoint.address.clone().into(),
            dev: m.dev(),
            ino: m.ino(),
            owner: m.uid(),
            uid: endpoint.client_identity.parse()?,
        };
        std::fs::set_permissions(&endpoint.address, std::fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    }
    pub async fn accept(&mut self) -> Result<UnixStream> {
        loop {
            let (stream, _) = self.listener.accept().await?;
            if stream.peer_cred()?.uid() == self.uid {
                return Ok(stream);
            }
        }
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
    validate_address(&config.address)?;
    crate::config::checked_path(Path::new(&config.address))?;
    crate::security::private(Path::new(&config.address))?;
    let stream = UnixStream::connect(&config.address).await?;
    if stream.peer_cred()?.uid().to_string() != config.expected_server_identity {
        bail!("endpoint identity mismatch");
    }
    Ok(stream)
}

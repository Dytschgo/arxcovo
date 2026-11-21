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
            if unsafe { libc::chown(name.as_ptr(), listener.uid, 0) } != 0 {
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

#[cfg(target_os = "linux")]
pub(super) mod delegated {
    use super::*;
    use std::{
        ffi::CString,
        fs::{File, Metadata},
        os::fd::{AsRawFd, FromRawFd},
        os::unix::ffi::OsStrExt,
        path::Component,
    };

    #[derive(Clone, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct Profile {
        pub(crate) owner_uid: u32,
        pub(crate) daemon_uid: u32,
        pub(crate) agent_group: u32,
    }

    #[derive(PartialEq, Eq)]
    struct Pin(u64, u64, u32, u32, u32);

    impl From<Metadata> for Pin {
        fn from(m: Metadata) -> Self {
            Self(m.dev(), m.ino(), m.uid(), m.gid(), m.mode())
        }
    }

    fn bad() -> anyhow::Error {
        anyhow::anyhow!("delegated MCP endpoint refused")
    }

    fn open(dir: &File, name: &std::ffi::OsStr) -> Result<File> {
        let name = CString::new(name.as_bytes()).map_err(|_| bad())?;
        // SAFETY: a valid directory and a terminated single path component.
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(bad());
        }
        // SAFETY: openat succeeded and this owns the fresh descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn without_acl(file: &File) -> bool {
        // Resolve the fixed procfs descriptor reference, never a caller path.
        let path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd())).unwrap();
        [c"system.posix_acl_access", c"system.posix_acl_default"]
            .into_iter()
            .all(|name| {
                let result = unsafe {
                    libc::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0)
                };
                result == -1
                    && matches!(
                        std::io::Error::last_os_error().raw_os_error(),
                        Some(libc::ENODATA | libc::EOPNOTSUPP)
                    )
            })
    }

    fn directories(path: &Path, profile: &Profile) -> Result<Vec<File>> {
        let bytes = path.as_os_str().as_bytes();
        if !(2..=100).contains(&bytes.len())
            || bytes[0] != b'/'
            || bytes[1..]
                .split(|b| *b == b'/')
                .any(|p| p.is_empty() || p == b"." || p == b"..")
            || path.file_name().is_none()
            || profile.owner_uid == profile.daemon_uid
            || profile.daemon_uid == 0
            || profile.agent_group == 0
            || profile.agent_group == u32::MAX
        {
            return Err(bad());
        }
        let mut names = path.components();
        if names.next() != Some(Component::RootDir) {
            return Err(bad());
        }
        let mut names: Vec<_> = names.collect();
        names.pop();
        if names.len() < 2 {
            return Err(bad());
        }
        let mut ancestry = vec![File::open("/")?];
        let root = ancestry[0].metadata()?;
        if root.uid() != 0 || root.mode() & 0o022 != 0 {
            return Err(bad());
        }
        let last = names.len() - 1;
        for (i, name) in names.into_iter().enumerate() {
            let Component::Normal(name) = name else {
                return Err(bad());
            };
            let file = open(ancestry.last().unwrap(), name)?;
            let m = file.metadata()?;
            if !m.is_dir()
                || !without_acl(&file)
                || (i != last
                    && (![0, profile.owner_uid].contains(&m.uid()) || m.mode() & 0o022 != 0))
                || (i == last
                    && (m.uid() != profile.daemon_uid
                        || m.gid() != profile.agent_group
                        || m.mode() & 0o7777 != 0o2710))
            {
                return Err(bad());
            }
            ancestry.push(file);
        }
        Ok(ancestry)
    }

    fn socket(dir: &File, path: &Path, profile: &Profile) -> Result<File> {
        let file = open(dir, path.file_name().ok_or_else(bad)?)?;
        let m = file.metadata()?;
        if !m.file_type().is_socket()
            || !without_acl(&file)
            || m.uid() != profile.daemon_uid
            || m.gid() != profile.agent_group
            || m.mode() & 0o7777 != 0o660
        {
            return Err(bad());
        }
        Ok(file)
    }

    pub(crate) fn group_member(uid: u32, group: u32) -> bool {
        let mut bytes = [0u8; 16384];
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        // SAFETY: NSS fills the bounded buffer and initialized result.
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                record.as_mut_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                &mut result,
            )
        };
        if status != 0 || result.is_null() {
            return false;
        }
        // SAFETY: success with a non-null result initializes record.
        let record = unsafe { record.assume_init() };
        let mut groups = [0; 64];
        let mut count = groups.len() as libc::c_int;
        // SAFETY: the account name remains live in bytes; groups has count entries.
        let found = unsafe {
            libc::getgrouplist(
                record.pw_name,
                record.pw_gid,
                groups.as_mut_ptr(),
                &mut count,
            )
        };
        found >= 0 && (1..=64).contains(&count) && groups[..count as usize].contains(&group)
    }

    struct Bound {
        handles: Vec<File>,
        pins: Vec<Pin>,
    }

    impl Bound {
        fn read(path: &Path, profile: &Profile) -> Result<Self> {
            let mut handles = directories(path, profile)?;
            handles.push(socket(handles.last().unwrap(), path, profile)?);
            let pins = handles
                .iter()
                .map(|file| file.metadata().map(Pin::from))
                .collect::<std::io::Result<Vec<_>>>()?;
            Ok(Self { handles, pins })
        }

        fn same(&self, path: &Path, profile: &Profile) -> bool {
            Self::read(path, profile).is_ok_and(|current| current.pins == self.pins)
                && self
                    .handles
                    .iter()
                    .zip(&self.pins)
                    .all(|(fd, pin)| fd.metadata().is_ok_and(|m| Pin::from(m) == *pin))
        }
    }

    pub(crate) struct AgentListener {
        listener: UnixListener,
        path: PathBuf,
        profile: Profile,
        allowed: Vec<u32>,
        bound: Bound,
    }

    impl AgentListener {
        pub(crate) fn bind(path: PathBuf, profile: Profile, allowed: Vec<u32>) -> Result<Self> {
            let ancestry = directories(&path, &profile)?;
            if allowed.is_empty()
                || allowed.len() > 16
                || !allowed.iter().all(|&uid| {
                    uid != 0
                        && uid != profile.daemon_uid
                        && uid != profile.owner_uid
                        && group_member(uid, profile.agent_group)
                })
            {
                return Err(bad());
            }
            let listener = UnixListener::bind(&path)?;
            let initial = open(ancestry.last().unwrap(), path.file_name().unwrap())?;
            let m = initial.metadata()?;
            if !m.file_type().is_socket()
                || m.uid() != profile.daemon_uid
                || m.gid() != profile.agent_group
            {
                return Err(bad());
            }
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))?;
            let bound = Bound::read(&path, &profile)?;
            if bound.pins.last() != Some(&Pin::from(initial.metadata()?))
                || ancestry
                    .iter()
                    .zip(&bound.pins)
                    .any(|(fd, pin)| !fd.metadata().is_ok_and(|m| Pin::from(m) == *pin))
            {
                return Err(bad());
            }
            Ok(Self {
                listener,
                path,
                profile,
                allowed,
                bound,
            })
        }
    }

    impl super::super::Acceptor for AgentListener {
        type Stream = UnixStream;

        async fn accept(&mut self) -> Result<Option<Self::Stream>> {
            let (stream, _) = self.listener.accept().await?;
            let valid = self.bound.same(&self.path, &self.profile)
                && stream.peer_cred().is_ok_and(|cred| {
                    let uid = cred.uid();
                    self.allowed.contains(&uid) && group_member(uid, self.profile.agent_group)
                });
            Ok(valid.then_some(stream))
        }
    }

    impl Drop for AgentListener {
        fn drop(&mut self) {
            if self.bound.same(&self.path, &self.profile) {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn endpoint_controls(root: &Path) {
        use crate::local::Acceptor;
        let profile = Profile {
            owner_uid: 0,
            daemon_uid: 1001,
            agent_group: 1003,
        };
        let path = root.join("mcp/socket");
        assert_eq!(unsafe { libc::geteuid() }, profile.daemon_uid);
        assert!(group_member(1003, 1003));
        assert!(!group_member(1001, 1003));
        assert!(!path.exists());
        let mut endpoint = AgentListener::bind(path.clone(), profile.clone(), vec![1003]).unwrap();
        let _wrong_uid = UnixStream::connect(&path).await.unwrap();
        assert!(endpoint.accept().await.unwrap().is_none());

        // The listening daemon can reach the socket by DAC as its owner; it
        // is still refused by the group check even with its UID on the list.
        endpoint.allowed = vec![1001];
        let _non_member = UnixStream::connect(&path).await.unwrap();
        assert!(endpoint.accept().await.unwrap().is_none());
        endpoint.allowed = vec![1003];
        std::fs::write(root.join("mcp/agent-ready"), b"1").unwrap();
        let user = tokio::time::timeout(std::time::Duration::from_secs(5), endpoint.accept())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(user.peer_cred().unwrap().uid(), 1003);

        std::fs::remove_file(&path).unwrap();
        let replacement = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).unwrap();
        assert!(Bound::read(&path, &profile).is_ok());
        assert!(!endpoint.bound.same(&path, &profile));
        drop(replacement);
        std::fs::remove_file(&path).unwrap();

        // Move a real socket entry between qualified directories. Holding
        // the leaf inode constant isolates the parent-directory pin.
        let moved = AgentListener::bind(path.clone(), profile.clone(), vec![1003]).unwrap();
        let other = root.join("other/socket");
        std::fs::rename(&path, &other).unwrap();
        let replacement = Bound::read(&other, &profile).unwrap();
        assert!(replacement.pins.last() == moved.bound.pins.last());
        assert!(!moved.bound.same(&other, &profile));
        std::fs::remove_file(&other).unwrap();
        assert!(AgentListener::bind(root.join("link/socket"), profile, vec![1003]).is_err());
    }

    pub(in crate::local) fn validate(config: &ClientConfig, profile: &Profile) -> Result<()> {
        if config.expected_server_identity != profile.daemon_uid.to_string() {
            return Err(bad());
        }
        let path = Path::new(&config.address);
        Bound::read(path, profile)?;
        Ok(())
    }

    pub(in crate::local) async fn connect(
        config: &ClientConfig,
        profile: &Profile,
    ) -> Result<UnixStream> {
        if config.expected_server_identity != profile.daemon_uid.to_string() {
            return Err(bad());
        }
        let path = Path::new(&config.address);
        let bound = Bound::read(path, profile)?;
        let stream = UnixStream::connect(path).await?;
        if stream.peer_cred()?.uid() != profile.daemon_uid || !bound.same(path, profile) {
            return Err(bad());
        }
        Ok(stream)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod adr15_tests {
    #[test]
    fn old_runtime_address_refuses() {
        assert!(super::validate_address("/private/var/run/arxcovo/arxd.sock").is_err());
    }
}

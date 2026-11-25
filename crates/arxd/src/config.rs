use anyhow::{Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::Read,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
};
use subtle::ConstantTimeEq;

/// Candidate-only VM profile. The installed and foreground schemas stay closed.
///
/// Every value of this type went through [`VmRuntimeProfile::parse`], with
/// its size limit and semantic validation; public-egress admission relies on
/// that provenance for its pins. The type therefore implements no
/// `Deserialize` (so `serde_json::from_*::<VmRuntimeProfile>` does not
/// compile) and its only field is private (so no struct literal compiles
/// outside this module). The foreground file's `runtime` section also takes
/// `parse`, through `validated_runtime`.
pub struct VmRuntimeProfile(Box<RawVmRuntimeProfile>);

/// The serde shape of a profile. Private: only `parse` may lift it into a
/// [`VmRuntimeProfile`], after validation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVmRuntimeProfile {
    enabled: bool,
    owner_sid: String,
    supervisor_sid: String,
    bundle: String,
    volume: String,
    state: String,
    disk_ceiling_bytes: u64,
    manifest: VmManifest,
    #[serde(default, deserialize_with = "present")]
    public_egress: Option<Box<PublicEgressProfile>>,
}

#[derive(Deserialize)]
enum VmManifest {
    #[serde(rename = "qemu-11.1.3-windows-x64")]
    Qemu1113WindowsX64,
}

/// ADR 0064: the separately pinned patched QEMU/libslirp manifest for a
/// public-egress static boot, plus the enrolled policy artifact's pin. It
/// never replaces the stock `manifest`, which TEST VMs keep. Admission still
/// refuses these pins until a reviewed receipt qualifies them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicEgressProfile {
    manifest: EgressManifest,
    /// The console executable, which statically links patched libslirp.
    qemu: PinSpec,
    receipt: PinSpec,
    policy: PinSpec,
}

#[derive(Deserialize)]
enum EgressManifest {
    #[serde(rename = "qemu-11.1.3-arx-public-egress-windows-x64")]
    Qemu1113PublicEgressWindowsX64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PinSpec {
    length: u64,
    sha256: String,
}

impl PinSpec {
    fn pin(&self, limit: u64) -> Option<crate::vm::paths::ArtifactPin> {
        let mut sha256 = [0; 32];
        (self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            && (1..=limit).contains(&self.length)
            && hex::decode_to_slice(&self.sha256, &mut sha256).is_ok())
        .then_some(crate::vm::paths::ArtifactPin {
            size: self.length,
            sha256,
        })
    }
}

impl PublicEgressProfile {
    fn binary(&self) -> Option<crate::vm::public_egress::EgressBinaryPin> {
        const EXECUTABLE_LIMIT: u64 = 512 * 1024 * 1024;
        const RECEIPT_LIMIT: u64 = 1024 * 1024;
        Some(crate::vm::public_egress::EgressBinaryPin {
            qemu: self.qemu.pin(EXECUTABLE_LIMIT)?,
            receipt: self.receipt.pin(RECEIPT_LIMIT)?,
        })
    }

    fn policy(&self) -> Option<crate::vm::paths::ArtifactPin> {
        self.policy.pin(crate::vm::public_egress::MAX_POLICY_BYTES)
    }
}

#[derive(Debug)]
pub struct InvalidVmProfile;

impl std::fmt::Display for InvalidVmProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid VM profile")
    }
}

impl std::error::Error for InvalidVmProfile {}

impl VmRuntimeProfile {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 16_384 {
            bail!("VM profile too large");
        }
        let raw: RawVmRuntimeProfile =
            serde_json::from_slice(bytes).map_err(|_| InvalidVmProfile)?;
        if raw
            .public_egress
            .as_ref()
            .is_some_and(|egress| egress.binary().is_none() || egress.policy().is_none())
        {
            bail!(InvalidVmProfile);
        }
        raw.validate_paths()?;
        Ok(Self(Box::new(raw)))
    }

    fn validate_paths(&self) -> Result<crate::vm::paths::LocalPath> {
        self.0.validate_paths()
    }

    pub fn enabled(&self) -> bool {
        self.0.enabled
    }

    pub fn manifest_id(&self) -> &'static str {
        match self.0.manifest {
            VmManifest::Qemu1113WindowsX64 => "qemu-11.1.3-windows-x64",
        }
    }

    /// The patched build's manifest identifier, when one is configured.
    pub fn public_egress_manifest_id(&self) -> Option<&'static str> {
        self.0
            .public_egress
            .as_ref()
            .map(|egress| match egress.manifest {
                EgressManifest::Qemu1113PublicEgressWindowsX64 => {
                    crate::vm::public_egress::EGRESS_MANIFEST
                }
            })
    }

    /// The configured patched-binary pin. Present is not qualified: admission
    /// still requires a reviewed receipt.
    pub fn public_egress_binary(&self) -> Option<crate::vm::public_egress::EgressBinaryPin> {
        self.0.public_egress.as_ref()?.binary()
    }

    /// The enrolled policy artifact's pin.
    pub fn public_egress_policy_pin(&self) -> Option<crate::vm::paths::ArtifactPin> {
        self.0.public_egress.as_ref()?.policy()
    }
}

impl RawVmRuntimeProfile {
    fn validate_paths(&self) -> Result<crate::vm::paths::LocalPath> {
        use crate::vm::paths::LocalPath;

        let invalid_path = |_| anyhow::anyhow!("invalid VM profile path");
        let bundle = LocalPath::parse(&self.bundle).map_err(invalid_path)?;
        let volume = LocalPath::parse(&self.volume).map_err(invalid_path)?;
        let state = LocalPath::parse(&self.state).map_err(invalid_path)?;
        if !local_account_sid(&self.owner_sid)
            || !local_account_sid(&self.supervisor_sid)
            || self.owner_sid == self.supervisor_sid
            || !volume.is_drive_root()
            || state.is_drive_root()
            || bundle.is_drive_root()
            || !volume.contains(&state)
            || volume.overlaps(&bundle)
            || !(1..=crate::vm::VmPolicy::default().disk_cap).contains(&self.disk_ceiling_bytes)
        {
            bail!(InvalidVmProfile);
        }
        Ok(state)
    }
}

fn local_account_sid(sid: &str) -> bool {
    let Some(parts) = sid.strip_prefix("S-1-5-21-") else {
        return false;
    };
    let mut parts = parts.split('-');
    (0..4).all(|_| {
        parts.next().is_some_and(|part| {
            !part.starts_with('0')
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u32>().is_ok_and(|n| n > 0)
        })
    }) && parts.next().is_none()
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(skip)]
    pub execution: Option<std::sync::Arc<crate::development::Execution>>,
    #[serde(rename = "execution")]
    pub execution_settings: Option<crate::development::WindowsSettings>,
    pub development_execution: Option<crate::development::Settings>,
    pub listen: Option<SocketAddr>,
    pub dashboard: Option<crate::dashboard::DashboardConfig>,
    pub state_dir: PathBuf,
    pub agents: Vec<Agent>,
    pub local: Option<crate::local::Endpoint>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub principal: String,
    pub token_sha256: String,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let value = serde_json::from_slice(&config_bytes(path)?)
            .map_err(|_| anyhow::anyhow!("invalid configuration"))?;
        Self::initialize(path, value)
    }

    fn initialize(path: &Path, mut value: Self) -> Result<Self> {
        value.validate()?;
        if let Some(execution) = &value.execution_settings {
            execution.disjoint(
                path.parent()
                    .ok_or_else(|| anyhow::anyhow!("invalid configuration"))?,
            )?;
        }
        if let Some(dev) = &value.development_execution {
            dev.disjoint(
                path.parent()
                    .ok_or_else(|| anyhow::anyhow!("invalid configuration"))?,
            )?;
        }
        value.execution = if let Some(settings) = value.execution_settings.clone() {
            Some(crate::development::Execution::from_windows(settings)?)
        } else {
            value
                .development_execution
                .clone()
                .map(crate::development::Execution::new)
                .transpose()?
        };
        Ok(value)
    }
    /// Foreground execution is Windows-only, never with HTTP, and its workspace
    /// is disjoint from private state. Config::load also checks the config dir.
    pub fn validate_development(&self) -> Result<()> {
        if let Some(execution) = &self.execution_settings {
            if self.development_execution.is_some()
                || !cfg!(windows)
                || self.listen.is_some()
                || self.local.is_none()
            {
                bail!("invalid execution configuration");
            }
            execution.validate()?;
            execution.disjoint(&self.state_dir)?;
        }
        if let Some(dev) = &self.development_execution {
            if !cfg!(windows) || self.listen.is_some() {
                bail!("invalid development configuration");
            }
            dev.validate()?;
            dev.disjoint(&self.state_dir)?;
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_development()?;
        if let Some(endpoint) = &self.local {
            endpoint.validate()?;
        }
        if self.listen.is_some_and(|a| !a.ip().is_loopback()) {
            bail!("listener must be loopback");
        }
        if let Some(dashboard) = &self.dashboard {
            dashboard.validate()?;
        }
        let dashboard_only =
            self.dashboard.is_some() && self.listen.is_none() && self.local.is_none();
        if self.agents.len() > 32 || (self.agents.is_empty() && !dashboard_only) {
            bail!("agent count out of bounds");
        }
        let mut names = HashSet::new();
        let mut hashes = HashSet::new();
        for agent in &self.agents {
            if agent.principal.is_empty()
                || agent.principal.len() > 64
                || !agent
                    .principal
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                || !names.insert(&agent.principal)
            {
                bail!("invalid principal");
            }
            if agent.token_sha256.len() != 64
                || hex::decode(&agent.token_sha256).is_err()
                || !hashes.insert(agent.token_sha256.to_ascii_lowercase())
            {
                bail!("invalid token hash");
            }
        }
        checked_path(&self.state_dir)?;
        if !self.state_dir.is_dir() {
            bail!("state directory required");
        }
        crate::security::private(&self.state_dir)?;
        Ok(())
    }
    pub fn authenticate(&self, token: &str) -> Option<String> {
        // Provision 32 random bytes as 64 lowercase hex characters. Never accept passwords.
        if token.len() != 64
            || !token
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return None;
        }
        let hash = Sha256::digest(token.as_bytes());
        let mut principal = None;
        for agent in &self.agents {
            let expected = hex::decode(&agent.token_sha256).ok()?;
            if bool::from(hash.as_slice().ct_eq(&expected)) {
                principal = Some(agent.principal.clone());
            }
        }
        principal
    }
}

fn config_bytes(path: &Path) -> Result<Vec<u8>> {
    checked_path(path)?;
    crate::security::private(path)?;
    anyhow::ensure!(path.is_file(), "configuration file required");
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(16385)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 16384, "configuration too large");
    Ok(bytes)
}

pub enum ForegroundConfig {
    Inventory(Config),
    TestVms(VmForeground),
}

pub struct VmForeground {
    pub(crate) config: Config,
    pub(crate) profile: TestVms,
    /// The validated state root, held from validation through composition
    /// and serving. `config.state_dir` is its canonical name.
    pub(crate) root: Box<RootPin>,
}

/// The independently enrolled Windows volume name and FILE_ID_INFO.
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedVmStore {
    pub volume_guid: String,
    pub volume_serial: String,
    pub directory_file_id: String,
}

#[derive(Debug)]
pub struct VmStoreIdentityRefusal;

impl std::fmt::Display for VmStoreIdentityRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VM store identity refused")
    }
}

impl std::error::Error for VmStoreIdentityRefusal {}

#[derive(Debug)]
pub struct VmStoreComposeConflict;

impl std::fmt::Display for VmStoreComposeConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("enrolled VM store requires a profile without Compose secret custody")
    }
}

impl std::error::Error for VmStoreComposeConflict {}

#[cfg(windows)]
struct EnrolledStoreIdentity {
    volume_guid: String,
    directory: crate::vm::paths::FileIdentity,
}

#[cfg(windows)]
impl EnrolledStoreIdentity {
    fn verified_path(&self, file: &std::fs::File) -> Result<PathBuf> {
        anyhow::ensure!(
            full_directory_identity(file)? == self.directory,
            VmStoreIdentityRefusal
        );
        let path = held_volume_path(file)?;
        anyhow::ensure!(
            path.to_str()
                .and_then(|name| name.get(..self.volume_guid.len()))
                .is_some_and(|volume| volume.eq_ignore_ascii_case(&self.volume_guid)),
            VmStoreIdentityRefusal
        );
        Ok(path)
    }
}

#[cfg(windows)]
impl ExpectedVmStore {
    fn identity(&self) -> Result<EnrolledStoreIdentity> {
        let guid = self
            .volume_guid
            .strip_prefix(r"\\?\Volume{")
            .and_then(|value| value.strip_suffix(r"}\"))
            .ok_or(VmStoreIdentityRefusal)?;
        let mut parts = guid.split('-');
        for length in [8, 4, 4, 4, 12] {
            let part = parts.next().ok_or(VmStoreIdentityRefusal)?;
            anyhow::ensure!(
                part.len() == length
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                VmStoreIdentityRefusal
            );
        }
        anyhow::ensure!(parts.next().is_none(), VmStoreIdentityRefusal);
        let mut file = [0; 16];
        let volume =
            u64::from_str_radix(&self.volume_serial, 16).map_err(|_| VmStoreIdentityRefusal)?;
        hex::decode_to_slice(&self.directory_file_id, &mut file)
            .map_err(|_| VmStoreIdentityRefusal)?;
        anyhow::ensure!(
            format!("{volume:016x}") == self.volume_serial
                && hex::encode(file) == self.directory_file_id,
            VmStoreIdentityRefusal
        );
        Ok(EnrolledStoreIdentity {
            volume_guid: self.volume_guid.clone(),
            directory: crate::vm::paths::FileIdentity { volume, file },
        })
    }
}

/// Prepare a private, bounded copy for the owner acceptance client. The
/// caller holds the original config and its parent while writing the copy.
#[cfg(windows)]
pub fn pinned_vm_config(path: &Path, expected: &ExpectedVmStore) -> Result<Vec<u8>> {
    let parent = path.parent().ok_or(VmStoreIdentityRefusal)?;
    checked_path(parent)?;
    crate::security::private(parent)?;
    crate::local::require_owner(parent)?;
    crate::local::require_owner(path)?;
    expected.identity()?;
    let bytes = config_bytes(path)?;
    let mut config = crate::local::check_json(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid VM configuration"))?;
    let profile = config
        .get_mut("test_vms")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("VM configuration required"))?;
    anyhow::ensure!(
        !profile.contains_key("expected_store"),
        "VM config already has a store pin"
    );
    profile.insert("expected_store".into(), serde_json::to_value(expected)?);
    let bytes = serde_json::to_vec(&config)?;
    anyhow::ensure!(bytes.len() <= 16384, "configuration too large");
    Ok(bytes)
}

/// The identity of the validated VM state root, bound to its canonical name.
///
/// Windows holds the root and every ancestor directory open without delete
/// sharing, so no component of the name can be renamed, deleted or replaced
/// while the pin lives; [`RootPin::verify`] then also compares volume and
/// file ID from the handle with a fresh open of the name. Operators must stop
/// the foreground before renaming or moving the root or any parent folder
/// (docs/dev-host-setup.md, "Foreground VM profile: state root custody").
///
/// POSIX holds a non-following descriptor, which keeps the object alive but
/// does not lock the name. Validation requires every ancestor to be
/// non-replaceable by any other principal (owned by the effective UID or
/// root, and not group/other writable unless sticky with the child owned by
/// the effective UID or root), so only the same effective UID, which can
/// already control this process, could rename a component. That is the
/// stated POSIX trust boundary for this diagnostic-only profile. On macOS
/// the walk also refuses any extended ACL on the root or an ancestor. `verify`
/// compares device and inode against a fresh non-following lookup and so
/// refuses a persistent replacement made before composition or bind.
pub(crate) struct RootPin {
    #[cfg(windows)]
    _ancestors: Vec<std::fs::File>,
    leaf: std::fs::File,
    identity: (u64, u64),
    #[cfg(windows)]
    enrolled: Option<(PathBuf, EnrolledStoreIdentity)>,
}

#[cfg(windows)]
thread_local! {
    // Only synchronous calls through a held enrolled store can use its
    // volume-GUID path in the shared local-path validator.
    static ENROLLED_STATE_PATH: std::cell::RefCell<Option<PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
}

pub(crate) struct VmStatePath {
    path: PathBuf,
    #[cfg(windows)]
    enrolled: bool,
}

impl VmStatePath {
    pub(crate) fn access<T>(&self, action: impl FnOnce(&Path) -> T) -> T {
        #[cfg(windows)]
        if self.enrolled {
            struct Restore(Option<PathBuf>);
            impl Drop for Restore {
                fn drop(&mut self) {
                    ENROLLED_STATE_PATH.with(|slot| slot.replace(self.0.take()));
                }
            }
            let _restore =
                Restore(ENROLLED_STATE_PATH.with(|slot| slot.replace(Some(self.path.clone()))));
            return action(&self.path);
        }
        action(&self.path)
    }
}

impl RootPin {
    pub(crate) fn acquire(root: &Path) -> Result<Self> {
        #[cfg(windows)]
        {
            let mut ancestors = Vec::new();
            let mut prefix = PathBuf::new();
            let mut components = root.components().peekable();
            while let Some(part) = components.next() {
                prefix.push(part);
                if matches!(part, Component::Prefix(_)) {
                    continue;
                }
                let handle = pin_directory(&prefix)?;
                if components.peek().is_none() {
                    let identity = directory_identity(&handle)?;
                    return Ok(Self {
                        _ancestors: ancestors,
                        leaf: handle,
                        identity,
                        enrolled: None,
                    });
                }
                directory_identity(&handle)?;
                ancestors.push(handle);
            }
            bail!("invalid VM state")
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            stable_ancestry(root)?;
            let leaf = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(root)?;
            let identity = directory_identity(&leaf)?;
            let pin = Self { leaf, identity };
            pin.verify(root)?;
            Ok(pin)
        }
    }

    #[cfg(windows)]
    fn acquire_enrolled(root: &Path, expected: &ExpectedVmStore) -> Result<Self> {
        let acquire = || {
            let expected = expected.identity()?;
            let original = Self::acquire(root)?;
            // The drive letter can be rebound independently of a directory
            // handle. Require the independently enrolled GUID on that handle
            // before holding its GUID path for state I/O.
            let path = expected.verified_path(&original.leaf)?;
            let mut anchored = Self::acquire(&path)?;
            anchored.enrolled = Some((path, expected));
            anchored.verify(root)?;
            Ok::<_, anyhow::Error>(anchored)
        };
        acquire().map_err(|_| VmStoreIdentityRefusal.into())
    }

    pub(crate) fn state_path(&self, configured: &Path) -> VmStatePath {
        #[cfg(windows)]
        if let Some((path, _)) = &self.enrolled {
            return VmStatePath {
                path: path.clone(),
                enrolled: true,
            };
        }
        VmStatePath {
            path: configured.to_owned(),
            #[cfg(windows)]
            enrolled: false,
        }
    }

    /// The canonical name still denotes the pinned directory.
    pub(crate) fn verify(&self, root: &Path) -> Result<()> {
        #[cfg(windows)]
        if let Some((path, expected)) = &self.enrolled {
            let matches = |file: &std::fs::File| {
                expected
                    .verified_path(file)
                    .is_ok_and(|current| current.as_os_str().eq_ignore_ascii_case(path.as_os_str()))
            };
            anyhow::ensure!(
                matches(&self.leaf)
                    && Self::acquire(root).is_ok_and(|current| matches(&current.leaf))
                    && Self::acquire(path).is_ok_and(|current| matches(&current.leaf)),
                VmStoreIdentityRefusal
            );
            return Ok(());
        }
        #[cfg(windows)]
        let current = directory_identity(&pin_directory(root)?)?;
        #[cfg(unix)]
        let current = {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::symlink_metadata(root)?;
            anyhow::ensure!(metadata.is_dir(), "VM state directory required");
            (metadata.dev(), metadata.ino())
        };
        anyhow::ensure!(
            current == self.identity && directory_identity(&self.leaf)? == self.identity,
            "VM state root replaced"
        );
        Ok(())
    }
}

/// Holds the enrolled volume path across synchronous acceptance read batches.
#[cfg(windows)]
pub struct EnrolledVmStore {
    configured: PathBuf,
    pin: RootPin,
}

#[cfg(windows)]
impl EnrolledVmStore {
    pub fn open(root: &Path, expected: &ExpectedVmStore) -> Result<Self> {
        let configured = vm_path(root).map_err(|_| VmStoreIdentityRefusal)?;
        let pin = RootPin::acquire_enrolled(&configured, expected)?;
        Ok(Self { configured, pin })
    }

    pub fn read<T>(&self, action: impl FnOnce(&Path) -> T) -> Result<T> {
        self.read_from(|| self.configured.as_path(), action)
    }

    fn read_from<'a, T>(
        &self,
        configured: impl Fn() -> &'a Path,
        action: impl FnOnce(&Path) -> T,
    ) -> Result<T> {
        self.pin.verify(configured())?;
        let output = self.pin.state_path(&self.configured).access(action);
        self.pin.verify(configured())?;
        Ok(output)
    }
}

/// A single synchronous acceptance read batch.
#[cfg(windows)]
pub fn with_enrolled_vm_store<T>(
    root: &Path,
    expected: &ExpectedVmStore,
    action: impl FnOnce(&Path) -> T,
) -> Result<T> {
    EnrolledVmStore::open(root, expected)?.read(action)
}

#[cfg(windows)]
fn full_directory_identity(file: &std::fs::File) -> Result<crate::vm::paths::FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    directory_identity(file)?;
    let mut id = FILE_ID_INFO::default();
    anyhow::ensure!(
        unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                std::ptr::from_mut(&mut id).cast(),
                size_of_val(&id) as u32,
            )
        } != 0,
        VmStoreIdentityRefusal
    );
    Ok(crate::vm::paths::FileIdentity {
        volume: id.VolumeSerialNumber,
        file: id.FileId.Identifier,
    })
}

#[cfg(windows)]
fn held_volume_path(file: &std::fs::File) -> Result<PathBuf> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_GUID};
    let mut output = [0; 512];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            output.as_mut_ptr(),
            output.len() as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    anyhow::ensure!(length > 0 && length < output.len(), VmStoreIdentityRefusal);
    let path = String::from_utf16(&output[..length])?;
    anyhow::ensure!(path.starts_with(r"\\?\Volume{"), VmStoreIdentityRefusal);
    Ok(PathBuf::from(path))
}

#[cfg(all(test, windows))]
mod enrolled_store_tests {
    use super::*;
    use std::{cell::Cell, fs};

    #[test]
    fn later_artifact_batch_discards_evidence_when_configured_mount_rebinds() {
        let scratch = std::env::temp_dir().canonicalize().unwrap();
        let enrolled = tempfile::tempdir_in(&scratch).unwrap();
        let replacement = tempfile::tempdir_in(&scratch).unwrap();
        let drive_path =
            |path: &Path| PathBuf::from(path.to_str().unwrap().trim_start_matches(r"\\?\"));
        let configured = drive_path(enrolled.path());
        let alternate = drive_path(replacement.path());
        let file = pin_directory(&configured).unwrap();
        let identity = full_directory_identity(&file).unwrap();
        let volume_path = held_volume_path(&file).unwrap();
        let name = volume_path.to_str().unwrap();
        let guid_end = name.find(r"}\").unwrap() + 2;
        let expected = ExpectedVmStore {
            volume_guid: name[..guid_end]
                .to_ascii_lowercase()
                .replacen("volume", "Volume", 1),
            volume_serial: format!("{:016x}", identity.volume),
            directory_file_id: hex::encode(identity.file),
        };
        let store = EnrolledVmStore::open(&configured, &expected).unwrap();
        fs::write(configured.join("artifact"), b"enrolled fixture").unwrap();
        assert!(!alternate.join("artifact").exists());
        let read_artifact = |root: &Path| {
            let path = root.join("artifact");
            checked_path(&path).unwrap();
            fs::read(&path).unwrap()
        };
        assert_eq!(store.read(read_artifact).unwrap(), b"enrolled fixture");

        let rebound = Cell::new(false);
        let alias = || {
            if rebound.get() {
                alternate.as_path()
            } else {
                configured.as_path()
            }
        };
        let later = store.read_from(alias, |root| {
            // Inject a newer drive resolution while the actual I/O still uses
            // the pinned volume. The proof must be discarded after the read.
            rebound.set(true);
            read_artifact(root)
        });
        assert!(later.unwrap_err().is::<VmStoreIdentityRefusal>());
    }
}

/// An ordinary local directory opened without following a reparse point and
/// without delete sharing.
#[cfg(windows)]
fn pin_directory(path: &Path) -> Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    Ok(std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?)
}

#[cfg(windows)]
fn directory_identity(file: &std::fs::File) -> Result<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // The handle is owned by `file` for the duration of both queries.
    anyhow::ensure!(
        unsafe {
            GetFileType(file.as_raw_handle()) == FILE_TYPE_DISK
                && GetFileInformationByHandle(file.as_raw_handle(), &mut info) != 0
        },
        "VM state identity unavailable"
    );
    anyhow::ensure!(
        info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
            == FILE_ATTRIBUTE_DIRECTORY,
        "ordinary VM state directory required"
    );
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

#[cfg(unix)]
fn directory_identity(file: &std::fs::File) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    anyhow::ensure!(metadata.is_dir(), "VM state directory required");
    Ok((metadata.dev(), metadata.ino()))
}

/// No principal other than the effective UID (or root) can rename or replace
/// any ancestor entry of `root`: each parent is owned by the effective UID or
/// root and is not group/other writable, unless it is sticky and the child is
/// owned by the effective UID or root. Mode bits include the Linux ACL mask.
/// macOS ACLs are independent of the mode and can grant another principal
/// add-file or delete-child rights, so on macOS the root and every ancestor
/// must also carry no extended ACL entry; an unreadable ACL is refused too.
#[cfg(unix)]
fn stable_ancestry(root: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    #[cfg(target_os = "macos")]
    for path in root.ancestors() {
        use std::os::unix::fs::OpenOptionsExt;
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path);
        anyhow::ensure!(
            directory.is_ok_and(|directory| {
                crate::service::native::macos_empty_acl(&directory).is_ok()
            }),
            "extended ACL on VM state path"
        );
    }
    let euid = unsafe { libc::geteuid() };
    let trusted = |uid: u32| uid == euid || uid == 0;
    let mut child = std::fs::symlink_metadata(root)?;
    for parent in root.ancestors().skip(1) {
        let metadata = std::fs::symlink_metadata(parent)?;
        anyhow::ensure!(
            metadata.is_dir()
                && trusted(metadata.uid())
                && (metadata.mode() & 0o022 == 0
                    || (metadata.mode() & 0o1000 != 0 && trusted(child.uid()))),
            "replaceable VM state ancestor"
        );
        child = metadata;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VmFile {
    state_dir: PathBuf,
    agents: Vec<Agent>,
    local: crate::local::Endpoint,
    test_vms: TestVms,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestVms {
    pub(crate) enabled: bool,
    #[serde(default, deserialize_with = "present")]
    expected_store: Option<ExpectedVmStore>,
    #[serde(default)]
    pub(crate) allow_agent_public_sites: bool,
    #[serde(default, deserialize_with = "present")]
    pub(crate) publication: Option<Box<crate::vm::runtime::publication::Profile>>,
    #[serde(default, deserialize_with = "validated_runtime")]
    pub(crate) runtime: Option<VmRuntimeProfile>,
    #[serde(default, deserialize_with = "present")]
    recipes: Option<Vec<RecipePins>>,
    #[serde(default)]
    pub(crate) compose_releases: Vec<crate::vm::compose::ReleaseInput>,
    #[serde(default, deserialize_with = "present")]
    pub(crate) compose_secret_root: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipePins {
    iso: RecipeAsset,
    kernel: RecipeAsset,
    initrd: RecipeAsset,
    guest: RecipeAsset,
    firmware_code: RecipeAsset,
    firmware_vars: RecipeAsset,
    qemu_receipt: RecipeAsset,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeAsset {
    path: PathBuf,
    length: u64,
    sha256: String,
}

/// The foreground file's runtime section goes through
/// [`VmRuntimeProfile::parse`], with its size limit and semantic validation,
/// like every other profile. JSON `null` is refused.
fn validated_runtime<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<VmRuntimeProfile>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let bytes = serde_json::to_vec(&value).map_err(serde::de::Error::custom)?;
    VmRuntimeProfile::parse(&bytes)
        .map(Some)
        .map_err(|_| serde::de::Error::custom("invalid VM profile"))
}

fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

impl ForegroundConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = config_bytes(path)?;
        let value = crate::local::check_json(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid configuration"))?;
        if value.get("test_vms").is_none() {
            let config = serde_json::from_value(value)
                .map_err(|_| anyhow::anyhow!("invalid configuration"))?;
            return Config::initialize(path, config).map(Self::Inventory);
        }
        crate::local::require_owner(path)?;
        let file: VmFile = serde_json::from_value(value)
            .map_err(|_| anyhow::anyhow!("invalid VM configuration"))?;
        let (state_dir, root) = file.test_vms.validate(&file.state_dir, &file.local)?;
        let config = Config {
            state_dir,
            agents: file.agents,
            local: Some(file.local),
            ..Config::default()
        };
        anyhow::ensure!(
            config
                .agents
                .iter()
                .all(|agent| !agent.principal.eq_ignore_ascii_case("arxd")),
            "reserved VM principal"
        );
        config.validate()?;
        root.verify(&config.state_dir)?;
        Ok(Self::TestVms(VmForeground {
            config,
            profile: file.test_vms,
            root: Box::new(root),
        }))
    }
}

impl TestVms {
    /// Returns the canonical exclusive root that composition must open and
    /// its pin; the raw configured string is never reopened after
    /// validation. The pin is taken before the path-based owner, privacy and
    /// overlap checks, so on Windows they inspect the pinned objects.
    fn validate(&self, root: &Path, local: &crate::local::Endpoint) -> Result<(PathBuf, RootPin)> {
        use crate::vm::paths::LocalPath;

        anyhow::ensure!(
            self.expected_store.is_none() || self.compose_secret_root.is_none(),
            VmStoreComposeConflict
        );
        let layout = crate::service::layout::Layout::system()?;
        let reserved = layout.endpoint().to_string_lossy();
        anyhow::ensure!(
            if cfg!(windows) {
                !reserved.eq_ignore_ascii_case(&local.address)
            } else {
                layout.endpoint() != Path::new(&local.address)
            },
            "reserved local endpoint"
        );
        let (root, pin) = if let Some(expected) = &self.expected_store {
            #[cfg(windows)]
            {
                let root = vm_path(root).map_err(|_| VmStoreIdentityRefusal)?;
                let pin = RootPin::acquire_enrolled(&root, expected)?;
                (root, pin)
            }
            #[cfg(not(windows))]
            {
                let _ = expected;
                bail!(VmStoreIdentityRefusal);
            }
        } else {
            let root = vm_path(root)?;
            let pin = RootPin::acquire(&root)?;
            (root, pin)
        };
        anyhow::ensure!(root.is_dir(), "VM state directory required");
        crate::security::private(&root)?;
        crate::local::require_owner(&root)?;
        anyhow::ensure!(
            layout.workspace_disjoint(
                root.to_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid VM state"))?
            ),
            "protected VM state overlap"
        );
        let executable = std::env::current_exe()?.canonicalize()?;
        let executable_dir = executable
            .parent()
            .ok_or_else(|| anyhow::anyhow!("executable root missing"))?;
        anyhow::ensure!(
            !overlaps(&root, executable_dir),
            "executable VM state overlap"
        );
        #[cfg(unix)]
        anyhow::ensure!(
            !overlaps(&root, Path::new(&local.address)),
            "socket inside VM state"
        );
        let actual = crate::local::vm_supervisor_identity()?;
        // Windows serves a distinct local account through the pipe DACL.
        // POSIX is diagnostic-only: its socket and parent are private to the
        // supervisor UID, so that UID is the only client that can connect.
        #[cfg(windows)]
        {
            anyhow::ensure!(
                local.client_identity != actual,
                "VM client and supervisor must differ"
            );
            anyhow::ensure!(
                local_account_sid(&local.client_identity),
                "invalid VM client identity"
            );
        }
        #[cfg(unix)]
        anyhow::ensure!(
            local.client_identity == actual,
            "VM client must be the supervisor"
        );

        if let Some(runtime) = &self.runtime {
            anyhow::ensure!(
                cfg!(all(windows, target_arch = "x86_64")),
                "native VM platform required"
            );
            let state = runtime.validate_paths()?;
            let configured = LocalPath::parse(root.to_str().unwrap_or_default())
                .map_err(|_| anyhow::anyhow!("invalid VM state"))?;
            anyhow::ensure!(
                runtime.0.enabled == self.enabled
                    && runtime.0.supervisor_sid == actual
                    && local.client_identity != runtime.0.owner_sid
                    && state.as_str().eq_ignore_ascii_case(configured.as_str()),
                "invalid VM identity or state pairing"
            );
        }
        if let Some(publication) = &self.publication {
            let state = LocalPath::parse(root.to_str().unwrap_or_default())
                .map_err(|_| anyhow::anyhow!("invalid VM state"))?;
            publication.validate(&state)?;
        }
        if let Some(recipes) = &self.recipes {
            anyhow::ensure!(recipes.len() <= 1, "unsupported VM recipe count");
            for recipe in recipes {
                recipe.digest(&root)?;
            }
        }
        pin.verify(&root)?;
        Ok((root, pin))
    }

    pub(crate) fn recipe(&self) -> Result<Option<([u8; 16], [u8; 32])>> {
        self.recipes
            .as_ref()
            .and_then(|recipes| recipes.first())
            .map(|recipe| {
                let digest = recipe.pins()?;
                Ok((digest[..16].try_into()?, digest))
            })
            .transpose()
    }

    pub(crate) fn policy(&self) -> crate::vm::VmPolicy {
        let mut policy = crate::vm::VmPolicy::default();
        if let Some(runtime) = &self.runtime {
            policy.disk_cap = runtime.0.disk_ceiling_bytes;
        }
        policy
    }
}

impl RecipePins {
    fn assets(&self) -> [&RecipeAsset; 7] {
        [
            &self.iso,
            &self.kernel,
            &self.initrd,
            &self.guest,
            &self.firmware_code,
            &self.firmware_vars,
            &self.qemu_receipt,
        ]
    }

    fn pins(&self) -> Result<[u8; 32]> {
        let mut hash = Sha256::new();
        hash.update(b"arxcovo.recipe\0");
        hash.update(crate::vm::UbuntuRecipe::ID.as_bytes());
        for asset in self.assets() {
            let mut digest = [0; 32];
            hex::decode_to_slice(&asset.sha256, &mut digest)?;
            anyhow::ensure!(asset.length > 0, "invalid VM asset length");
            hash.update(asset.length.to_be_bytes());
            hash.update(digest);
        }
        Ok(hash.finalize().into())
    }

    fn digest(&self, root: &Path) -> Result<()> {
        self.pins()?;
        for asset in self.assets() {
            let path = vm_path(&asset.path)?;
            anyhow::ensure!(
                path.is_file() && !overlaps(root, &path),
                "invalid VM asset path"
            );
        }
        Ok(())
    }
}

fn vm_path(path: &Path) -> Result<PathBuf> {
    let name = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid VM path"))?;
    #[cfg(windows)]
    crate::vm::paths::LocalPath::parse(name).map_err(|_| anyhow::anyhow!("invalid VM path"))?;
    #[cfg(unix)]
    anyhow::ensure!(
        name.starts_with('/')
            && name[1..]
                .split('/')
                .all(|part| !matches!(part, "" | "." | "..")),
        "invalid VM path"
    );
    checked_path(path)?;
    let canonical = path.canonicalize()?;
    #[cfg(windows)]
    let canonical = PathBuf::from(
        canonical
            .to_str()
            .and_then(|path| path.strip_prefix(r"\\?\"))
            .ok_or_else(|| anyhow::anyhow!("invalid canonical VM path"))?,
    );
    anyhow::ensure!(
        if cfg!(windows) {
            name.replace('/', "\\")
                .eq_ignore_ascii_case(&canonical.to_string_lossy())
        } else {
            path.as_os_str() == canonical.as_os_str()
        },
        "aliased VM path"
    );
    Ok(canonical)
}

fn overlaps(first: &Path, second: &Path) -> bool {
    let normalize = |path: &Path| {
        let value = path.to_string_lossy().replace('\\', "/");
        let value = value.strip_prefix("//?/").unwrap_or(&value);
        if cfg!(any(windows, target_os = "macos")) {
            value.to_lowercase()
        } else {
            value.to_owned()
        }
    };
    let first = normalize(first);
    let second = normalize(second);
    Path::new(&first).starts_with(&second) || Path::new(&second).starts_with(&first)
}

/// Verify custody for an owner-run client before it reads private inputs or writes evidence.
#[cfg(windows)]
pub fn checked_owner_private_path(path: &Path) -> Result<()> {
    checked_path(path)?;
    crate::local::require_owner(path)?;
    crate::security::private(path)
}

pub fn checked_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || !path.components().any(|c| matches!(c, Component::RootDir))
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        bail!("absolute normalized path required");
    }
    let mut prefix = PathBuf::new();
    #[cfg(windows)]
    let enrolled = ENROLLED_STATE_PATH.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|root| path.starts_with(root) || root.starts_with(path))
    });
    for part in path.components() {
        #[cfg(windows)]
        {
            use std::path::Prefix;
            match part {
                Component::Prefix(p)
                    if !(matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
                        || enrolled && matches!(p.kind(), Prefix::Verbatim(_))) =>
                {
                    bail!("local disk paths required")
                }
                Component::Normal(value) if value.to_string_lossy().contains(':') => {
                    bail!("alternate streams forbidden")
                }
                _ => {}
            }
        }
        prefix.push(part);
        // A disk prefix alone is drive-relative (C:) or not a filesystem
        // object (\\?\C:). Inspect it only after RootDir forms the absolute
        // root; every root and descendant still receives the checks below.
        #[cfg(windows)]
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        let m = std::fs::symlink_metadata(&prefix)?;
        if m.file_type().is_symlink() {
            bail!("links forbidden");
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                bail!("reparse points forbidden");
            }
        }
    }
    Ok(())
}
/// The foreground file's runtime section reaches `VmRuntimeProfile` only
/// through `VmRuntimeProfile::parse`.
#[cfg(test)]
mod runtime_provenance_tests {
    use super::TestVms;
    use serde_json::json;

    fn runtime() -> serde_json::Value {
        json!({
            "enabled": true, "owner_sid": "S-1-5-21-1-2-3-1001",
            "supervisor_sid": "S-1-5-21-1-2-3-1002", "bundle": "E:/pinned bundle",
            "volume": "D:/", "state": "D:/test_vms/state",
            "disk_ceiling_bytes": 10_737_418_240_u64, "manifest": "qemu-11.1.3-windows-x64",
            "public_egress": {
                "manifest": "qemu-11.1.3-arx-public-egress-windows-x64",
                "qemu": {"length": 30_000_000, "sha256": "a1".repeat(32)},
                "receipt": {"length": 4096, "sha256": "a2".repeat(32)},
                "policy": {"length": 512, "sha256": "a3".repeat(32)},
            },
        })
    }

    #[test]
    fn foreground_runtime_section_is_parsed_and_validated() {
        let vms: TestVms =
            serde_json::from_value(json!({"enabled": true, "runtime": runtime()})).unwrap();
        let profile = vms.runtime.unwrap();
        assert!(profile.public_egress_binary().is_some());
        assert!(profile.public_egress_policy_pin().is_some());
        for (field, value) in [
            ("state", json!("test_vms/state")),
            ("bundle", json!("D:/")),
            ("owner_sid", json!("S-1-5-21-1-2-3-1002")),
        ] {
            let mut bad = runtime();
            bad[field] = value;
            let refused =
                serde_json::from_value::<TestVms>(json!({"enabled": true, "runtime": bad}));
            assert!(refused.is_err(), "{field}");
        }
        let mut bad = runtime();
        bad["public_egress"]["qemu"]["length"] = json!(0);
        assert!(
            serde_json::from_value::<TestVms>(json!({"enabled": true, "runtime": bad})).is_err()
        );
        assert!(
            serde_json::from_value::<TestVms>(json!({"enabled": true, "runtime": null})).is_err()
        );
    }
}

#[cfg(all(test, windows))]
mod tests {
    /// Mirrors PR16 8c3856a: a disk prefix is skipped only once a physical
    /// RootDir follows, so bare verbatim drives are refused, never accepted
    /// without filesystem inspection.
    #[test]
    fn rooted_prefix_skip_requires_physical_root() {
        for unsafe_path in [r"C:", r"C:relative", r"\\?\C:", r"\\?\Z:"] {
            assert!(
                super::checked_path(std::path::Path::new(unsafe_path)).is_err(),
                "{unsafe_path}"
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let verbatim = dir.path().canonicalize().unwrap();
        assert!(super::checked_path(&verbatim).is_ok(), "positive control");
        assert!(super::checked_path(dir.path()).is_ok(), "positive control");
    }
}

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
    /// ADR 0029's execution account for native work. Profiles written before
    /// the TEST opt-in omit it and keep their dedicated-account meaning.
    #[serde(default)]
    execution_profile: VmExecutionProfile,
    /// The explicit native TEST opt-in (ADR 0054). It requires an enabled
    /// `current_user` profile and pins the exact sealed dossier and scope.
    #[serde(default, deserialize_with = "present")]
    test_qualification: Option<Box<RawTestQualification>>,
    #[serde(default, deserialize_with = "present")]
    public_egress: Option<Box<PublicEgressProfile>>,
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum VmExecutionProfile {
    #[default]
    DedicatedAccount,
    CurrentUser,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTestQualification {
    enrollment: String,
    dossier: String,
    sha256: String,
    scope: String,
}

/// The configured TEST selection, only from a validated profile. It keeps
/// the profile's current-user SID so startup can require the enrollment
/// owner and the live token to be that account. Configuration alone grants
/// no effect until the foreground issuer verifies that selection.
#[derive(Clone)]
#[cfg_attr(
    not(all(windows, target_arch = "x86_64")),
    allow(dead_code, reason = "the native TEST issuer is Windows x64 only")
)]
pub(crate) struct TestQualificationSelection {
    enrollment: crate::vm::paths::LocalPath,
    dossier: crate::vm::paths::LocalPath,
    selection: crate::vm::qualification_record::Selection,
    owner_sid: String,
}

#[cfg_attr(
    not(all(windows, target_arch = "x86_64")),
    allow(dead_code, reason = "the native TEST issuer is Windows x64 only")
)]
impl TestQualificationSelection {
    pub(crate) fn enrollment(&self) -> &crate::vm::paths::LocalPath {
        &self.enrollment
    }

    pub(crate) fn dossier(&self) -> &crate::vm::paths::LocalPath {
        &self.dossier
    }

    pub(crate) fn selection(&self) -> &crate::vm::qualification_record::Selection {
        &self.selection
    }

    /// The profile's one current-user SID (owner and supervisor agree).
    pub(crate) fn owner_sid(&self) -> &str {
        &self.owner_sid
    }
}

impl RawTestQualification {
    fn checked(&self, owner_sid: &str) -> Option<TestQualificationSelection> {
        use crate::vm::paths::LocalPath;
        let digest = |value: &str| {
            let mut bytes = [0; 32];
            (value.len() == 64
                && value
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                && hex::decode_to_slice(value, &mut bytes).is_ok()
                && bytes != [0; 32])
                .then_some(bytes)
        };
        let enrollment = LocalPath::parse(&self.enrollment).ok()?;
        let dossier = LocalPath::parse(&self.dossier).ok()?;
        if enrollment.overlaps(&dossier) {
            return None;
        }
        Some(TestQualificationSelection {
            enrollment,
            dossier,
            selection: crate::vm::qualification_record::Selection {
                seal_sha256: digest(&self.sha256)?,
                scope: digest(&self.scope)?,
            },
            owner_sid: owner_sid.to_owned(),
        })
    }
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
        if let Some(selection) = &raw.test_qualification
            && (!raw.enabled
                || raw.execution_profile != VmExecutionProfile::CurrentUser
                || selection.checked(&raw.owner_sid).is_none())
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

    /// The validated TEST opt-in, if configured. `parse` refused any other
    /// shape, so a present value is always exact.
    #[cfg_attr(
        not(any(test, all(windows, target_arch = "x86_64"))),
        allow(dead_code, reason = "the native TEST issuer is Windows x64 only")
    )]
    pub(crate) fn test_qualification(&self) -> Option<TestQualificationSelection> {
        self.0
            .test_qualification
            .as_ref()?
            .checked(&self.0.owner_sid)
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
        // The native reader canonicalizes a current-user SID and checks it
        // against the live, non-elevated primary token.
        let identities = match self.execution_profile {
            VmExecutionProfile::DedicatedAccount => {
                local_account_sid(&self.owner_sid)
                    && local_account_sid(&self.supervisor_sid)
                    && self.owner_sid != self.supervisor_sid
            }
            VmExecutionProfile::CurrentUser => {
                self.owner_sid == self.supervisor_sid && sid_spelling(&self.owner_sid)
            }
        };
        if !identities
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

/// Canonical user-SID spelling: revision 1, a 48-bit authority and one to
/// fifteen 32-bit subauthorities, without signs or leading zeros.
fn sid_spelling(sid: &str) -> bool {
    let Some(parts) = sid.strip_prefix("S-1-") else {
        return false;
    };
    let mut values = parts.split('-');
    let canonical = |value: &str, max: u64| {
        value
            .parse::<u64>()
            .is_ok_and(|n| n <= max && n.to_string() == value)
    };
    if !values
        .next()
        .is_some_and(|authority| canonical(authority, (1 << 48) - 1))
    {
        return false;
    }
    let count = values.clone().count();
    (1..=15).contains(&count) && values.all(|value| canonical(value, u32::MAX.into()))
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
    pub(crate) custody: std::sync::Arc<VmCustody>,
}

/// The independently enrolled Windows volume name and FILE_ID_INFO.
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedVmStore {
    pub volume_guid: String,
    pub volume_serial: String,
    pub directory_file_id: String,
}

/// Independent enrollment for the Compose secret directory.
#[derive(Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExpectedComposeSecretRoot(pub ExpectedVmStore);

#[derive(Debug)]
pub struct VmStoreIdentityRefusal;

impl std::fmt::Display for VmStoreIdentityRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VM store identity refused")
    }
}

impl std::error::Error for VmStoreIdentityRefusal {}

#[cfg(all(windows, target_arch = "x86_64"))]
#[derive(Debug)]
struct StaticComposeCustodyRefusal;

#[cfg(all(windows, target_arch = "x86_64"))]
impl std::fmt::Display for StaticComposeCustodyRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("static Compose custody refused")
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl std::error::Error for StaticComposeCustodyRefusal {}

#[derive(Debug)]
pub struct ComposeCustodyRefusal;

impl std::fmt::Display for ComposeCustodyRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Compose secret custody refused")
    }
}

impl std::error::Error for ComposeCustodyRefusal {}

struct EnrolledStoreIdentity {
    #[cfg(windows)]
    volume_guid: String,
    #[cfg(windows)]
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

impl ExpectedVmStore {
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn verify_directory(&self, directory: &std::fs::File) -> Result<()> {
        self.identity()?.verified_path(directory)?;
        Ok(())
    }

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
            #[cfg(windows)]
            volume_guid: self.volume_guid.clone(),
            #[cfg(windows)]
            directory: crate::vm::paths::FileIdentity { volume, file },
        })
    }
}

/// Prepare a private, bounded copy for the owner acceptance client. The
/// caller holds the original config and its parent while writing the copy.
#[cfg(windows)]
pub fn pinned_vm_config(path: &Path, expected: &ExpectedVmStore) -> Result<Vec<u8>> {
    pinned_config(path, expected, None)
}

#[cfg(windows)]
fn pinned_config(
    path: &Path,
    expected: &ExpectedVmStore,
    secret: Option<&ExpectedComposeSecretRoot>,
) -> Result<Vec<u8>> {
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
    if let Some(secret) = secret {
        secret.0.identity().map_err(|_| ComposeCustodyRefusal)?;
        anyhow::ensure!(
            !profile.contains_key("expected_compose_secret_root"),
            "VM config already has a Compose secret pin"
        );
        profile.insert(
            "expected_compose_secret_root".into(),
            serde_json::to_value(secret)?,
        );
    }
    let bytes = serde_json::to_vec(&config)?;
    anyhow::ensure!(bytes.len() <= 16384, "configuration too large");
    Ok(bytes)
}

/// The identity of the validated VM state root, bound to its canonical name.
///
/// Windows holds the root and every ancestor directory open without write
/// or delete sharing, so no component of the name can be renamed, deleted or replaced
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
                #[cfg(target_arch = "x86_64")]
                let handle = if let Some(parent) = ancestors.last() {
                    let name = part.as_os_str().to_str().ok_or(VmStoreIdentityRefusal)?;
                    crate::vm::windows::hold_child_directory(parent, name)?
                } else {
                    pin_directory(&prefix)?
                };
                #[cfg(not(target_arch = "x86_64"))]
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

#[cfg(all(windows, target_arch = "x86_64"))]
impl RootPin {
    fn verify_mounted_name(&self, configured: &Path) -> Result<()> {
        use crate::vm::paths::LocalPath;
        let raw = LocalPath::parse(configured.to_str().ok_or(ComposeCustodyRefusal)?)
            .map_err(|_| ComposeCustodyRefusal)?;
        let drive = &raw.as_str()[..3];
        let current = Self::acquire(Path::new(raw.as_str()))?;
        let drive_root = current._ancestors.first().unwrap_or(&current.leaf);
        crate::vm::windows::verify_volume_root(drive, full_directory_identity(drive_root)?)?;
        let volume = held_volume_path(drive_root)?;
        let name = volume.join(&raw.as_str()[3..]);
        let held = held_volume_path(&self.leaf)?;
        let identity = full_directory_identity(&self.leaf)?;
        // Confirm that ordinary Win32 lookup of the configured spelling
        // agrees with the relative walk, including case-sensitive directories.
        let by_name = pin_directory(configured)?;
        anyhow::ensure!(
            full_directory_identity(&current.leaf)? == identity
                && full_directory_identity(&by_name)? == identity
                && windows_path_contains(&name, &held)
                && windows_path_contains(&held, &name)
                && held_volume_path(&current.leaf)? == held
                && held_volume_path(&by_name)? == held,
            ComposeCustodyRefusal
        );
        Ok(())
    }

    fn verify_static_root(&self, configured: &Path) -> Result<()> {
        self.verify(configured)?;
        self.verify_mounted_name(configured)?;
        crate::vm::qualification::require_held_private_protected(&self.leaf)?;
        Ok(())
    }

    pub(crate) fn require_disjoint(&self, other: &Self) -> Result<()> {
        let first = full_directory_identity(&self.leaf)?;
        let second = full_directory_identity(&other.leaf)?;
        anyhow::ensure!(first != second, ComposeCustodyRefusal);
        for (leaf, root) in [(first, other), (second, self)] {
            for ancestor in &root._ancestors {
                anyhow::ensure!(
                    full_directory_identity(ancestor)? != leaf,
                    ComposeCustodyRefusal
                );
            }
        }
        let first = held_volume_path(&self.leaf)?;
        let second = held_volume_path(&other.leaf)?;
        anyhow::ensure!(
            !windows_path_contains(&first, &second) && !windows_path_contains(&second, &first),
            ComposeCustodyRefusal
        );
        Ok(())
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn windows_path_contains(parent: &Path, child: &Path) -> bool {
    // The raw configured spellings have already passed LocalPath. These
    // components come from held handles or a verified volume-root name.
    let mut child = child.components();
    parent.components().all(|parent| {
        child.next().is_some_and(|child| {
            // CompareStringOrdinal takes explicit UTF-16 lengths.
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
            let parent: Vec<_> = parent.as_os_str().encode_wide().collect();
            let child: Vec<_> = child.as_os_str().encode_wide().collect();
            unsafe {
                CompareStringOrdinal(
                    parent.as_ptr(),
                    parent.len() as i32,
                    child.as_ptr(),
                    child.len() as i32,
                    1,
                ) == CSTR_EQUAL
            }
        })
    })
}

#[cfg(all(windows, target_arch = "x86_64"))]
struct ComposeRoot {
    configured: PathBuf,
    pin: RootPin,
}

/// Held for the lifetime of the store and by outstanding secret workers.
pub(crate) struct VmCustody {
    configured_state: PathBuf,
    state: RootPin,
    lost: crate::service::Shutdown,
    #[cfg(all(windows, target_arch = "x86_64"))]
    secret: Option<ComposeRoot>,
    #[cfg(all(windows, target_arch = "x86_64"))]
    static_inputs: Option<StaticComposeCustody>,
}

impl VmCustody {
    #[cfg(all(test, windows, target_arch = "x86_64"))]
    pub(crate) fn test_only(root: &Path) -> Result<Self> {
        Ok(Self {
            configured_state: root.to_owned(),
            state: RootPin::acquire(root)?,
            lost: Default::default(),
            secret: None,
            static_inputs: None,
        })
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn configured_state(&self) -> &Path {
        &self.configured_state
    }

    pub(crate) fn state_path(&self, canonical: &Path) -> VmStatePath {
        self.state.state_path(canonical)
    }

    pub(crate) fn verify(&self) -> Result<()> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            if let Some(static_inputs) = &self.static_inputs {
                let verify = || {
                    anyhow::ensure!(!self.lost.is_cancelled(), StaticComposeCustodyRefusal);
                    self.state.verify_static_root(&self.configured_state)?;
                    if let Some(secret) = &self.secret {
                        secret.pin.verify_static_root(&secret.configured)?;
                    }
                    static_inputs.verify()
                };
                return verify().map_err(|_| {
                    self.lost.cancel();
                    StaticComposeCustodyRefusal.into()
                });
            }
            if let Some(secret) = &self.secret {
                return self.verify_pair(&self.configured_state, &secret.configured);
            }
        }
        self.state.verify(&self.configured_state)
    }

    pub(crate) async fn lost(&self) {
        self.lost.cancelled().await;
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    fn verify_pair(&self, state_name: &Path, secret_name: &Path) -> Result<()> {
        let verify = || {
            use crate::vm::windows::qualification::require_held_private;
            anyhow::ensure!(!self.lost.is_cancelled(), ComposeCustodyRefusal);
            let secret = self.secret.as_ref().ok_or(ComposeCustodyRefusal)?;
            self.state.verify(state_name)?;
            secret.pin.verify(secret_name)?;
            self.state.verify_mounted_name(state_name)?;
            secret.pin.verify_mounted_name(secret_name)?;
            require_held_private(&self.state.leaf)?;
            require_held_private(&secret.pin.leaf)?;
            Ok::<_, anyhow::Error>(())
        };
        verify().map_err(|_| {
            self.lost.cancel();
            ComposeCustodyRefusal.into()
        })
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn select_static_dossier(
        &self,
        m4: Option<&crate::vm::qualification::ForegroundQualification>,
    ) -> Result<crate::vm::qualification::StaticDossier, crate::vm::runtime::NativeRefusal> {
        use crate::vm::runtime::NativeRefusal;
        self.verify()
            .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
        let inputs = self
            .static_inputs
            .as_ref()
            .ok_or(NativeRefusal::QualificationMissing)?;
        inputs
            .qualification
            .select(m4.ok_or(NativeRefusal::QualificationMissing)?)
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn has_compose_root(&self) -> bool {
        self.secret.is_some()
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn open_static_backup(
        &self,
    ) -> Result<Option<crate::vm::compose_backup::ManagedBackupDir>> {
        let Some(root) = self
            .static_inputs
            .as_ref()
            .and_then(|inputs| inputs.backup.as_ref())
        else {
            return Ok(None);
        };
        self.verify()?;
        let directory = crate::vm::compose_backup::ManagedBackupDir::new(
            &root.configured,
            &self.configured_state,
            self.secret
                .as_ref()
                .map(|secret| secret.configured.as_path()),
        )
        .map_err(|_| StaticComposeCustodyRefusal)?;
        self.verify()?;
        Ok(Some(directory))
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn static_backup_capacity(&self) -> Result<crate::vm::Capacity> {
        self.verify()?;
        let root = self
            .static_inputs
            .as_ref()
            .and_then(|inputs| inputs.backup.as_ref())
            .ok_or(StaticComposeCustodyRefusal)?;
        let capacity = crate::vm::qualification::backup_capacity(&self.state.leaf, &root.pin.leaf)
            .map_err(|_| anyhow::anyhow!("Compose backup capacity unavailable"))?;
        self.verify()?;
        Ok(capacity)
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn open_ciphertext(&self, reference: crate::vm::Id) -> Result<std::fs::File> {
        let secret = self.secret.as_ref().ok_or(ComposeCustodyRefusal)?;
        let volume = full_directory_identity(&secret.pin.leaf)?.volume;
        let name = format!("{}.dpapi", hex::encode(reference));
        Ok(crate::vm::windows::read_private_child(
            &secret.pin.leaf,
            &name,
            volume,
        )?)
    }
}

/// Private acceptance launch bytes, together with the roots checked by the daemon.
#[cfg(all(windows, target_arch = "x86_64"))]
pub struct PinnedStaticVmConfig {
    bytes: Vec<u8>,
    custody: std::sync::Arc<VmCustody>,
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl PinnedStaticVmConfig {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn verify(&self) -> Result<()> {
        self.custody.verify()
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub fn pinned_static_vm_config(
    path: &Path,
    store: &ExpectedVmStore,
    secret: &ExpectedComposeSecretRoot,
) -> Result<PinnedStaticVmConfig> {
    let bytes = pinned_config(path, store, Some(secret))?;
    let file: VmFile =
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid VM configuration"))?;
    let (_, custody) = file.test_vms.validate(&file.state_dir, &file.local)?;
    Ok(PinnedStaticVmConfig { bytes, custody })
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

#[cfg(all(test, windows, target_arch = "x86_64"))]
#[path = "config_compose_tests.rs"]
mod compose_custody_tests;
#[cfg(all(test, windows, target_arch = "x86_64"))]
pub(crate) use compose_custody_tests::fixture_directory_pin;

/// An ordinary local directory opened without following a reparse point and
/// without write or delete sharing.
#[cfg(windows)]
fn pin_directory(path: &Path) -> Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    };
    Ok(std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
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
    compose_secret_root: Option<PathBuf>,
    #[serde(default, deserialize_with = "present")]
    expected_compose_secret_root: Option<Box<ExpectedComposeSecretRoot>>,
    #[serde(default, deserialize_with = "present")]
    static_compose: Option<Box<StaticComposeProfile>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticComposeProfile {
    selection: StaticComposeSelection,
    #[serde(default, deserialize_with = "present")]
    backup: Option<StaticComposeBackup>,
    #[serde(default, deserialize_with = "present")]
    private_routes: Option<crate::vm::compose::PrivateRoutes>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticComposeSelection {
    enrollment: String,
    dossier: String,
    expected_dossier_root: ExpectedVmStore,
    seal_sha256: String,
    scope: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticComposeBackup {
    root: String,
    expected_root: ExpectedVmStore,
    targets: Vec<crate::vm::compose_backup::TargetEnrollment>,
}

impl StaticComposeProfile {
    fn validate(&self) -> Result<()> {
        let path = |raw: &str| {
            let path = crate::vm::paths::LocalPath::parse(raw)
                .map_err(|_| anyhow::anyhow!("invalid static Compose path"))?;
            anyhow::ensure!(!path.is_drive_root(), "invalid static Compose path");
            Ok(path)
        };
        let enrollment = path(&self.selection.enrollment)?;
        let dossier = path(&self.selection.dossier)?;
        anyhow::ensure!(
            !enrollment.overlaps(&dossier),
            "static Compose path overlap"
        );
        // Parse the full pins even on diagnostic platforms. The handles
        // are acquired only on Windows x64.
        self.selection.pins()?;
        self.selection.expected_dossier_root.identity()?;
        if let Some(backup) = &self.backup {
            let root = path(&backup.root)?;
            anyhow::ensure!(
                root.as_str().as_bytes()[0].eq_ignore_ascii_case(&b'D')
                    && !backup.targets.is_empty(),
                "invalid static Compose backup profile"
            );
            backup.expected_root.identity()?;
            crate::vm::compose_backup::validate_targets(&backup.targets)?;
        }
        Ok(())
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
struct StaticComposeCustody {
    backup: Option<ComposeRoot>,
    dossier: ComposeRoot,
    qualification: crate::vm::qualification::PreparedStaticDossier,
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl StaticComposeCustody {
    fn acquire(
        profile: &StaticComposeProfile,
        state_path: &Path,
        state: &RootPin,
        secret: Option<&ComposeRoot>,
    ) -> Result<Self> {
        use crate::vm::{paths::LocalPath, qualification::PreparedStaticDossier};
        let acquire = || {
            let local = |raw: &str| LocalPath::parse(raw).map_err(|_| StaticComposeCustodyRefusal);
            let hold = |raw: &str, expected: &ExpectedVmStore| {
                let configured = PathBuf::from(local(raw)?.as_str());
                let path = vm_path(&configured)?;
                let pin = RootPin::acquire_enrolled(&path, expected)?;
                pin.verify_static_root(&configured)?;
                Ok::<_, anyhow::Error>(ComposeRoot { configured, pin })
            };
            state.verify_static_root(state_path)?;
            if let Some(root) = secret {
                root.pin.verify_static_root(&root.configured)?;
            }
            let backup = profile
                .backup
                .as_ref()
                .map(|backup| hold(&backup.root, &backup.expected_root))
                .transpose()?;
            let selected = &profile.selection;
            let dossier = hold(&selected.dossier, &selected.expected_dossier_root)?;
            let mut roots = vec![(state_path, state)];
            roots.extend(
                secret
                    .into_iter()
                    .chain(&backup)
                    .chain(std::iter::once(&dossier))
                    .map(|root| (root.configured.as_path(), &root.pin)),
            );
            let enrollment = local(&selected.enrollment)?;
            for (index, (name, root)) in roots.iter().enumerate() {
                anyhow::ensure!(
                    !local(name.to_str().ok_or(StaticComposeCustodyRefusal)?)?
                        .overlaps(&enrollment),
                    StaticComposeCustodyRefusal
                );
                for (_, other) in &roots[..index] {
                    root.require_disjoint(other)?;
                }
            }
            // Forbid an alternate drive spelling of an enrollment underneath
            // any held root, including aliases that bypass lexical checks.
            let enrollment_path = vm_path(Path::new(enrollment.as_str()))?;
            let enrollment_parent = enrollment_path
                .parent()
                .ok_or(StaticComposeCustodyRefusal)?;
            let parent = RootPin::acquire(enrollment_parent)?;
            parent.verify_mounted_name(enrollment_parent)?;
            let parent_path = held_volume_path(&parent.leaf)?;
            for (_, root) in &roots {
                anyhow::ensure!(
                    !windows_path_contains(&held_volume_path(&root.leaf)?, &parent_path),
                    StaticComposeCustodyRefusal
                );
            }
            let qualification = PreparedStaticDossier::hold(
                &local(&selected.dossier)?,
                &selected.expected_dossier_root,
                &enrollment,
                selected.pins()?,
            )
            .map_err(|_| StaticComposeCustodyRefusal)?;
            Ok(Self {
                backup,
                dossier,
                qualification,
            })
        };
        acquire().map_err(|_: anyhow::Error| StaticComposeCustodyRefusal.into())
    }

    fn verify(&self) -> Result<()> {
        for root in self.backup.iter().chain(std::iter::once(&self.dossier)) {
            root.pin.verify_static_root(&root.configured)?;
        }
        self.qualification
            .verify()
            .map_err(|_| StaticComposeCustodyRefusal.into())
    }
}

impl StaticComposeSelection {
    fn pins(&self) -> Result<crate::vm::qualification_record::DockerSelection> {
        let hash = |text: &str| {
            let mut bytes = [0; 32];
            anyhow::ensure!(
                text.len() == 64
                    && text
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && hex::decode_to_slice(text, &mut bytes).is_ok()
                    && bytes != [0; 32],
                "invalid static Compose selection"
            );
            Ok(bytes)
        };
        Ok(crate::vm::qualification_record::DockerSelection {
            seal_sha256: hash(&self.seal_sha256)?,
            scope: hash(&self.scope)?,
        })
    }
}

#[cfg(test)]
mod static_compose_profile_tests {
    use super::*;
    use serde_json::json;

    use arx_guest::compose::routing::{MAX_ROUTES, RoutingError, validate_routes};

    const ROUTE_ZONE: &str = "example.invalid";

    fn route(index: usize, slot: usize) -> serde_json::Value {
        json!({
            "hostname": format!("site-{index}.{ROUTE_ZONE}"),
            "project_id": format!("project-{}", slot / 4),
            "service": format!("web-{}", slot % 4),
            "project_slot": slot / 4,
            "service_index": slot % 4,
            "route_slot": slot
        })
    }

    fn combined_route_profile(
        private: Vec<serde_json::Value>,
        public: Vec<serde_json::Value>,
    ) -> TestVms {
        let hostnames: Vec<_> = public.iter().map(|route| &route["hostname"]).collect();
        let profile: TestVms = serde_json::from_value(json!({
            "enabled": true,
            "static_compose": {
                "selection": {
                    "enrollment": "D:/qualification/enrollment.json",
                    "dossier": "D:/qualification/dossier",
                    "expected_dossier_root": {
                        "volume_guid": r"\\?\Volume{a0b1c2d3-aaaa-bbbb-cccc-a0b1c2d3e4f5}",
                        "volume_serial": "ab".repeat(8),
                        "directory_file_id": "cd".repeat(16)
                    },
                    "seal_sha256": "aa".repeat(32),
                    "scope": "bb".repeat(32)
                },
                "private_routes": {
                    "vm_id": "aa".repeat(16),
                    "zone": ROUTE_ZONE,
                    "routes": private
                }
            },
            "publication": {
                "hostnames": hostnames,
                "zone_name": ROUTE_ZONE,
                "routes": public,
                "static_name": "site",
                "host_id": "bb".repeat(16),
                "tunnel_id": "11111111-2222-3333-4444-555555555555",
                "account_id": "cc".repeat(16),
                "zone_id": "dd".repeat(16),
                "private_root": "D:/private/publication",
                "dns_ciphertext": "D:/private/publication/dns.dpapi",
                "tunnel_ciphertext": "D:/private/publication/tunnel.dpapi",
                "connector": "D:/tools/cloudflared.exe",
                "connector_length": 1,
                "connector_sha256": "ee".repeat(32)
            }
        }))
        .unwrap();

        // Each table passes its own validation before the union is checked.
        let private = profile
            .static_compose
            .as_ref()
            .unwrap()
            .private_routes
            .as_ref()
            .unwrap();
        validate_routes(Some(ROUTE_ZONE), private.targets()).unwrap();
        profile
            .publication
            .as_ref()
            .unwrap()
            .validate_binding()
            .unwrap();
        profile
    }

    fn assert_combined_route_refusal(profile: TestVms, expected: RoutingError) {
        let endpoint = crate::local::Endpoint {
            address: "unused".into(),
            client_identity: "unused".into(),
        };
        assert_eq!(
            profile
                .validate(Path::new("unused"), &endpoint)
                .err()
                .unwrap()
                .to_string(),
            format!("invalid combined Compose route bindings: {expected:?}")
        );
    }

    #[test]
    fn config_accepts_disjoint_static_route_bindings() {
        combined_route_profile(vec![route(1, 1)], vec![route(0, 0)])
            .validate_route_bindings()
            .unwrap();
    }

    #[test]
    fn config_accepts_identical_static_route_bindings() {
        combined_route_profile(vec![route(0, 0)], vec![route(0, 0)])
            .validate_route_bindings()
            .unwrap();
    }

    #[test]
    fn config_refuses_conflicting_static_route_bindings() {
        assert_combined_route_refusal(
            combined_route_profile(vec![route(0, 0)], vec![route(0, 1)]),
            RoutingError::UnsortedOrDuplicateHostname,
        );
    }

    #[test]
    fn config_refuses_combined_static_route_slot_collision() {
        let mut public = route(1, 1);
        public["route_slot"] = json!(0);
        assert_combined_route_refusal(
            combined_route_profile(vec![route(0, 0)], vec![public]),
            RoutingError::DuplicateRouteSlot,
        );
    }

    #[test]
    fn config_refuses_combined_static_route_table_overflow() {
        let private = (0..MAX_ROUTES).map(|slot| route(slot, slot)).collect();
        assert_combined_route_refusal(
            combined_route_profile(private, vec![route(MAX_ROUTES, 0)]),
            RoutingError::TooManyRoutes,
        );
    }

    #[test]
    fn static_compose_requires_supported_platform_and_independent_pins() {
        let pin = json!({
            "volume_guid": r"\\?\Volume{a0b1c2d3-aaaa-bbbb-cccc-a0b1c2d3e4f5}\",
            "volume_serial": "ab".repeat(8),
            "directory_file_id": "cd".repeat(16)
        });
        let mut raw = json!({
            "enabled": true,
            "expected_store": pin,
            "static_compose": {
                "selection": {
                    "enrollment": "D:/qualification/enrollment.json",
                    "dossier": "D:/qualification/dossier",
                    "expected_dossier_root": pin,
                    "seal_sha256": "aa".repeat(32),
                    "scope": "bb".repeat(32)
                },
                "backup": {
                    "root": "D:/private/backup",
                    "expected_root": pin,
                    "targets": [{
                        "vm_id": "aa".repeat(16), "project_id":"app", "volume":"data",
                        "database":"store.sqlite3", "max_bytes":8192
                    }]
                }
            }
        });
        let profile: TestVms = serde_json::from_value(raw.clone()).unwrap();
        assert!(!profile.allow_agent_public_sites);
        assert!(profile.static_compose.as_ref().unwrap().validate().is_ok());
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("unused");
        let endpoint = crate::local::Endpoint {
            address: "unused".into(),
            client_identity: "unused".into(),
        };
        let error = profile.validate(&state, &endpoint).err().unwrap();
        if cfg!(all(windows, target_arch = "x86_64")) {
            assert!(error.is::<VmStoreIdentityRefusal>());
        } else {
            assert_eq!(error.to_string(), "static Compose requires Windows x64");
        }
        let mut with_secrets = raw.clone();
        with_secrets["compose_secret_root"] = json!("D:/private/secrets");
        let profile: TestVms = serde_json::from_value(with_secrets).unwrap();
        assert!(
            profile
                .validate(&state, &endpoint)
                .err()
                .unwrap()
                .is::<ComposeCustodyRefusal>()
        );
        assert!(!state.exists());
        raw["static_compose"]["backup"]["root"] = json!("C:/backup");
        let profile: TestVms = serde_json::from_value(raw.clone()).unwrap();
        assert!(profile.static_compose.unwrap().validate().is_err());
        raw["static_compose"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<TestVms>(raw).is_err());
    }
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
        let (state_dir, custody) = file.test_vms.validate(&file.state_dir, &file.local)?;
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
        custody.verify()?;
        Ok(Self::TestVms(VmForeground {
            config,
            profile: file.test_vms,
            custody,
        }))
    }
}

impl TestVms {
    fn validate_route_bindings(&self) -> Result<()> {
        if let Some(routes) = self
            .static_compose
            .as_ref()
            .and_then(|profile| profile.private_routes.as_ref())
        {
            routes.validate()?;
            // Private enrollment uses a VM ID and publication uses a host and
            // static name. Without a common key in the config, validate the two
            // tables together before opening the store.
            if let Some(publication) = &self.publication {
                publication.validate_route_union(routes.targets())?;
            }
        }
        Ok(())
    }

    /// Hold the state directory and any Compose secret directory before
    /// composition can open SQLite or write release enrollments.
    fn validate(
        &self,
        root: &Path,
        local: &crate::local::Endpoint,
    ) -> Result<(PathBuf, std::sync::Arc<VmCustody>)> {
        use crate::vm::paths::LocalPath;

        self.validate_route_bindings()?;
        anyhow::ensure!(
            self.compose_secret_root.is_none() || cfg!(all(windows, target_arch = "x86_64")),
            ComposeCustodyRefusal
        );
        anyhow::ensure!(
            self.expected_compose_secret_root.is_some()
                == (self.compose_secret_root.is_some() && self.expected_store.is_some()),
            ComposeCustodyRefusal
        );
        if let Some(profile) = &self.static_compose {
            anyhow::ensure!(
                cfg!(all(windows, target_arch = "x86_64")),
                "static Compose requires Windows x64"
            );
            anyhow::ensure!(
                self.expected_store.is_some(),
                "static Compose requires an enrolled VM store"
            );
            anyhow::ensure!(self.enabled, "static Compose requires enabled VMs");
            profile.validate()?;
        }

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
        let configured_state = root.to_owned();
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
        #[cfg(all(windows, target_arch = "x86_64"))]
        let secret = self
            .compose_secret_root
            .as_deref()
            .map(|path| {
                let configured = vm_path(path)?;
                let secret = match &self.expected_compose_secret_root {
                    Some(expected) => RootPin::acquire_enrolled(&configured, &expected.0)?,
                    None => RootPin::acquire(&configured)?,
                };
                pin.require_disjoint(&secret)?;
                Ok::<_, anyhow::Error>(ComposeRoot {
                    configured: path.to_owned(),
                    pin: secret,
                })
            })
            .transpose()
            .map_err(|_| ComposeCustodyRefusal)?;
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
        #[cfg(all(windows, target_arch = "x86_64"))]
        let static_inputs = self
            .static_compose
            .as_deref()
            .map(|profile| {
                StaticComposeCustody::acquire(profile, &configured_state, &pin, secret.as_ref())
            })
            .transpose()?;
        let custody = VmCustody {
            configured_state,
            state: pin,
            lost: Default::default(),
            #[cfg(all(windows, target_arch = "x86_64"))]
            secret,
            #[cfg(all(windows, target_arch = "x86_64"))]
            static_inputs,
        };
        custody.verify()?;
        Ok((root, std::sync::Arc::new(custody)))
    }

    pub(crate) fn has_static_compose(&self) -> bool {
        self.static_compose.is_some()
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn take_static_private_routes(
        &mut self,
    ) -> Option<crate::vm::compose::PrivateRoutes> {
        self.static_compose.as_mut()?.private_routes.take()
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn take_static_backup_targets(
        &mut self,
    ) -> Vec<crate::vm::compose_backup::TargetEnrollment> {
        self.static_compose
            .as_mut()
            .and_then(|profile| profile.backup.as_mut())
            .map(|backup| std::mem::take(&mut backup.targets))
            .unwrap_or_default()
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
///
/// This is a point-in-time check of the path. Callers that rely on it must hold the
/// relevant directory or file handles open (without delete/write sharing) for as long as
/// they depend on the result; otherwise the path can be swapped after the check.
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

    #[cfg(not(all(windows, target_arch = "x86_64")))]
    #[test]
    fn compose_roots_require_windows_x64() {
        let endpoint = crate::local::Endpoint {
            address: "unused".into(),
            client_identity: "unused".into(),
        };
        let plain = json!({"enabled": false, "compose_secret_root": "D:/secrets"});
        let pin = json!({
            "volume_guid": r"\\?\Volume{12345678-1234-1234-1234-1234567890ab}\",
            "volume_serial": "0".repeat(16),
            "directory_file_id": "0".repeat(32),
        });
        let mut pinned = plain.clone();
        pinned["expected_store"] = pin.clone();
        pinned["expected_compose_secret_root"] = pin;
        for profile in [plain, pinned] {
            let profile: TestVms = serde_json::from_value(profile).unwrap();
            let error = profile
                .validate(std::path::Path::new("unused"), &endpoint)
                .err()
                .unwrap();
            assert!(error.is::<super::ComposeCustodyRefusal>());
        }
    }

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

    fn parse(value: &serde_json::Value) -> anyhow::Result<super::VmRuntimeProfile> {
        super::VmRuntimeProfile::parse(&serde_json::to_vec(value).unwrap())
    }

    fn selection() -> serde_json::Value {
        json!({
            "enrollment": "D:/private/enrollment.json",
            "dossier": "D:/private/dossier",
            "sha256": "a1".repeat(32), "scope": "b2".repeat(32),
        })
    }

    fn current_user(mut profile: serde_json::Value) -> serde_json::Value {
        profile["execution_profile"] = json!("current_user");
        profile["owner_sid"] = json!("S-1-12-1-1-2-3-4");
        profile["supervisor_sid"] = profile["owner_sid"].clone();
        profile
    }

    /// I25: profiles written before the TEST opt-in keep the dedicated
    /// account meaning, and `current_user` stays a separate explicit choice.
    #[test]
    fn old_dedicated_profiles_keep_their_meaning() {
        let old = parse(&runtime()).unwrap();
        assert!(old.test_qualification().is_none());
        let mut explicit = runtime();
        explicit["execution_profile"] = json!("dedicated_account");
        assert!(parse(&explicit).unwrap().test_qualification().is_none());
        // A dedicated account still needs two distinct local-account SIDs.
        for (owner, supervisor) in [
            ("S-1-5-21-1-2-3-1001", "S-1-5-21-1-2-3-1001"),
            ("S-1-12-1-1-2-3-4", "S-1-5-21-1-2-3-1002"),
        ] {
            let mut bad = runtime();
            bad["owner_sid"] = json!(owner);
            bad["supervisor_sid"] = json!(supervisor);
            assert!(parse(&bad).is_err(), "{owner}");
        }
        // Current-user mode pins one canonical SID for both roles.
        let current = current_user(runtime());
        assert!(parse(&current).unwrap().test_qualification().is_none());
        for (field, value) in [
            ("supervisor_sid", json!("S-1-5-21-1-2-3-1002")),
            ("owner_sid", json!("S-1-12-01-1-2-3-4")),
            ("execution_profile", json!("CurrentUser")),
        ] {
            let mut bad = current.clone();
            bad[field] = value.clone();
            if field == "owner_sid" {
                bad["supervisor_sid"] = value;
            }
            assert!(parse(&bad).is_err(), "{field}");
        }
    }

    /// I01/T02: the TEST opt-in is explicit, needs an enabled current-user
    /// profile, and pins the exact seal digest, scope and private paths.
    #[test]
    fn test_selection_is_opt_in_and_strictly_pinned() {
        let mut profile = current_user(runtime());
        profile["test_qualification"] = selection();
        let checked = parse(&profile).unwrap().test_qualification().unwrap();
        assert_eq!(checked.selection().seal_sha256, [0xa1; 32]);
        assert_eq!(checked.selection().scope, [0xb2; 32]);
        assert_eq!(checked.owner_sid(), "S-1-12-1-1-2-3-4");
        let local = |path| crate::vm::paths::LocalPath::parse(path).unwrap();
        assert_eq!(checked.enrollment(), &local("D:/private/enrollment.json"));
        assert_eq!(checked.dossier(), &local("D:/private/dossier"));

        let mut dedicated = runtime();
        dedicated["test_qualification"] = selection();
        let mut disabled = profile.clone();
        disabled["enabled"] = json!(false);
        let mut null = profile.clone();
        null["test_qualification"] = json!(null);
        for refused in [dedicated, disabled, null] {
            assert!(parse(&refused).is_err());
        }
        for (field, bad) in [
            ("sha256", json!("00".repeat(32))),
            ("sha256", json!("A1".repeat(32))),
            ("sha256", json!("a1".repeat(31))),
            ("scope", json!("F0".repeat(32))),
            ("enrollment", json!("D:/private/dossier/enrollment.json")),
            ("dossier", json!("D:/private/../qualification")),
            ("dossier", json!("private/dossier")),
            ("unknown", json!(true)),
        ] {
            let mut refused = profile.clone();
            refused["test_qualification"][field] = bad;
            assert!(parse(&refused).is_err(), "{field}");
        }
        let mut missing = profile.clone();
        missing["test_qualification"]
            .as_object_mut()
            .unwrap()
            .remove("scope");
        assert!(parse(&missing).is_err());
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

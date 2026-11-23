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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmRuntimeProfile {
    enabled: bool,
    owner_sid: String,
    supervisor_sid: String,
    bundle: String,
    volume: String,
    state: String,
    disk_ceiling_bytes: u64,
    manifest: VmManifest,
}

#[derive(Deserialize)]
enum VmManifest {
    #[serde(rename = "qemu-11.1.0-windows-x64")]
    Qemu1110WindowsX64,
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
        let profile: Self = serde_json::from_slice(bytes).map_err(|_| InvalidVmProfile)?;
        profile.validate_paths()?;
        Ok(profile)
    }

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

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn manifest_id(&self) -> &'static str {
        match self.manifest {
            VmManifest::Qemu1110WindowsX64 => "qemu-11.1.0-windows-x64",
        }
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
        if !(1..=32).contains(&self.agents.len()) {
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
    runtime: Option<VmRuntimeProfile>,
    #[serde(default, deserialize_with = "present")]
    recipes: Option<Vec<RecipePins>>,
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
        file.test_vms.validate(&file.state_dir, &file.local)?;
        let config = Config {
            state_dir: file.state_dir,
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
        Ok(Self::TestVms(VmForeground {
            config,
            profile: file.test_vms,
        }))
    }
}

impl TestVms {
    fn validate(&self, root: &Path, local: &crate::local::Endpoint) -> Result<()> {
        use crate::vm::paths::LocalPath;

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
        let root = vm_path(root)?;
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
        anyhow::ensure!(
            local.client_identity != actual,
            "VM client and supervisor must differ"
        );
        #[cfg(windows)]
        anyhow::ensure!(
            local_account_sid(&local.client_identity),
            "invalid VM client identity"
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
                runtime.enabled == self.enabled
                    && runtime.supervisor_sid == actual
                    && local.client_identity != runtime.owner_sid
                    && state.as_str().eq_ignore_ascii_case(configured.as_str()),
                "invalid VM identity or state pairing"
            );
        }
        if let Some(recipes) = &self.recipes {
            anyhow::ensure!(recipes.len() <= 1, "unsupported VM recipe count");
            for recipe in recipes {
                recipe.digest(&root)?;
            }
        }
        Ok(())
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
            policy.disk_cap = runtime.disk_ceiling_bytes;
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
    for part in path.components() {
        #[cfg(windows)]
        {
            use std::path::Prefix;
            match part {
                Component::Prefix(p)
                    if !matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) =>
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

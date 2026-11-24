//! The strict project type. The owner's Compose file is translated outside the
//! guest into this closed JSON schema. Unknown keys fail to decode, so build,
//! include/extends, networks, ports, devices, privileged, capabilities, host
//! namespaces, bind mounts and volume drivers have no representation at all.
//! Values are then checked against the rules below.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::codec::is_lower_hex;

pub const MANIFEST_SCHEMA: u32 = 1;
pub const MAX_PROJECTS: usize = 8;
/// Managed volumes per VM, across all projects.
pub const MAX_VOLUMES: usize = 8;
pub const MAX_SERVICES: usize = 4;
pub const MAX_SECRET_SLOTS: usize = 4;
pub const MAX_ENV: usize = 32;
pub const MAX_ENV_VALUE: usize = 256;
pub const MAX_ENV_BYTES: usize = 4096;
pub const MAX_MOUNTS: usize = 8;
pub const PLATFORM: &str = "linux/amd64";
const MAX_ID: usize = 24;
const MAX_NAME: usize = 16;
const MAX_SLOT: usize = 32;
const MAX_PATH: usize = 128;
const MAX_IMAGE: usize = 255;
const RESERVED_TARGETS: [&str; 4] = ["/proc", "/sys", "/dev", "/run/secrets"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    pub schema: u32,
    pub project_id: String,
    pub services: Vec<ServiceSpec>,
    pub volumes: Vec<VolumeSpec>,
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSpec {
    pub name: String,
    /// `repository@sha256:<64 hex>`, no tag.
    pub image: String,
    pub platform: String,
    pub uid: u32,
    pub gid: u32,
    /// Declared container HTTP port. The broker publishes it only on a fixed
    /// guest loopback port; the caller never chooses a host or guest port.
    pub http_port: Option<u16>,
    pub health_path: Option<String>,
    pub read_only_rootfs: bool,
    pub environment: Vec<EnvVar>,
    pub mounts: Vec<Mount>,
    pub secrets: Vec<String>,
    pub limits: Limits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    /// A managed volume slot declared by this project.
    pub volume: String,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeSpec {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub cpu_millis: u32,
    pub memory_mib: u32,
    pub pids: u32,
}

/// The rule a rejected manifest broke. No input text is echoed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestRule {
    /// Not the closed schema: unknown or missing keys, wrong types or not JSON.
    Malformed,
    Schema,
    Interpolation,
    ProjectId,
    ServiceCount,
    ServiceName,
    Image,
    Platform,
    Identity,
    Port,
    HealthPath,
    Environment,
    VolumeCount,
    VolumeName,
    Mount,
    Secret,
    Limits,
}

/// A manifest that passed every rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project(ProjectManifest);

impl Project {
    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestRule> {
        let manifest: ProjectManifest =
            serde_json::from_slice(bytes).map_err(|_| ManifestRule::Malformed)?;
        Self::validate(manifest)
    }

    pub fn validate(manifest: ProjectManifest) -> Result<Self, ManifestRule> {
        check(&manifest)?;
        Ok(Self(manifest))
    }

    pub fn manifest(&self) -> &ProjectManifest {
        &self.0
    }

    pub fn id(&self) -> &str {
        &self.0.project_id
    }

    /// The exact owner of a secret slot's source file. Validation guarantees
    /// every service that mounts the slot runs as this UID/GID.
    pub fn slot_owner(&self, slot: &str) -> Option<(u32, u32)> {
        self.0
            .services
            .iter()
            .find(|service| service.secrets.iter().any(|used| used == slot))
            .map(|service| (service.uid, service.gid))
    }

    pub fn services_using(&self, slot: &str) -> Vec<String> {
        self.0
            .services
            .iter()
            .filter(|service| service.secrets.iter().any(|used| used == slot))
            .map(|service| service.name.clone())
            .collect()
    }

    pub fn images(&self) -> Vec<&str> {
        let mut images: Vec<&str> = self.0.services.iter().map(|s| s.image.as_str()).collect();
        images.sort_unstable();
        images.dedup();
        images
    }
}

fn require(valid: bool, rule: ManifestRule) -> Result<(), ManifestRule> {
    if valid { Ok(()) } else { Err(rule) }
}

fn check(manifest: &ProjectManifest) -> Result<(), ManifestRule> {
    use ManifestRule as R;
    require(manifest.schema == MANIFEST_SCHEMA, R::Schema)?;
    require(
        !strings(manifest).any(|text| text.contains('$')),
        R::Interpolation,
    )?;
    require(is_project_id(&manifest.project_id), R::ProjectId)?;
    require(
        (1..=MAX_SERVICES).contains(&manifest.services.len()),
        R::ServiceCount,
    )?;

    require(manifest.volumes.len() <= MAX_VOLUMES, R::VolumeCount)?;
    let mut volumes = BTreeSet::new();
    for volume in &manifest.volumes {
        require(is_name(&volume.name), R::VolumeName)?;
        require(volumes.insert(volume.name.as_str()), R::VolumeName)?;
        require(is_identity(volume.uid, volume.gid), R::Identity)?;
    }

    require(manifest.secrets.len() <= MAX_SECRET_SLOTS, R::Secret)?;
    let mut slots = BTreeSet::new();
    for slot in &manifest.secrets {
        require(is_slot_name(slot) && slots.insert(slot.as_str()), R::Secret)?;
    }

    let mut names = BTreeSet::new();
    for service in &manifest.services {
        require(
            is_name(&service.name) && names.insert(service.name.as_str()),
            R::ServiceName,
        )?;
        require(is_image_ref(&service.image), R::Image)?;
        require(service.platform == PLATFORM, R::Platform)?;
        require(is_identity(service.uid, service.gid), R::Identity)?;
        require(service.http_port != Some(0), R::Port)?;
        match (service.http_port, &service.health_path) {
            (Some(_), Some(path)) => require(is_health_path(path), R::HealthPath)?,
            (None, None) => {}
            _ => return Err(R::HealthPath),
        }
        check_environment(&service.environment)?;

        require(service.mounts.len() <= MAX_MOUNTS, R::Mount)?;
        let mut targets = BTreeSet::new();
        for mount in &service.mounts {
            let volume = manifest
                .volumes
                .iter()
                .find(|volume| volume.name == mount.volume)
                .ok_or(R::Mount)?;
            require(
                is_mount_target(&mount.target) && targets.insert(mount.target.as_str()),
                R::Mount,
            )?;
            // A writable managed volume belongs to exactly this identity.
            require(
                mount.read_only || (volume.uid, volume.gid) == (service.uid, service.gid),
                R::Mount,
            )?;
        }

        let mut used = BTreeSet::new();
        for slot in &service.secrets {
            require(
                slots.contains(slot.as_str()) && used.insert(slot),
                R::Secret,
            )?;
        }
        let limits = service.limits;
        require(
            (10..=4000).contains(&limits.cpu_millis)
                && (16..=4096).contains(&limits.memory_mib)
                && (16..=4096).contains(&limits.pids),
            R::Limits,
        )?;
    }

    for slot in &manifest.secrets {
        let mut owners = manifest
            .services
            .iter()
            .filter(|service| service.secrets.contains(slot))
            .map(|service| (service.uid, service.gid));
        let first = owners.next().ok_or(R::Secret)?;
        require(owners.all(|owner| owner == first), R::Secret)?;
    }
    Ok(())
}

fn check_environment(environment: &[EnvVar]) -> Result<(), ManifestRule> {
    use ManifestRule as R;
    require(environment.len() <= MAX_ENV, R::Environment)?;
    let mut names = BTreeSet::new();
    let mut total = 0;
    for var in environment {
        require(is_env_name(&var.name), R::Environment)?;
        require(names.insert(var.name.as_str()), R::Environment)?;
        require(
            var.value.len() <= MAX_ENV_VALUE
                && var
                    .value
                    .bytes()
                    .all(|byte| matches!(byte, 0x20..=0x7e) && !matches!(byte, b'"' | b'\\')),
            R::Environment,
        )?;
        total += var.name.len() + var.value.len();
    }
    require(total <= MAX_ENV_BYTES, R::Environment)
}

fn strings(manifest: &ProjectManifest) -> impl Iterator<Item = &str> {
    let services = manifest.services.iter().flat_map(|service| {
        [
            service.name.as_str(),
            service.image.as_str(),
            service.platform.as_str(),
        ]
        .into_iter()
        .chain(service.health_path.as_deref())
        .chain(
            service
                .environment
                .iter()
                .flat_map(|var| [var.name.as_str(), var.value.as_str()]),
        )
        .chain(
            service
                .mounts
                .iter()
                .flat_map(|mount| [mount.volume.as_str(), mount.target.as_str()]),
        )
        .chain(service.secrets.iter().map(String::as_str))
    });
    std::iter::once(manifest.project_id.as_str())
        .chain(manifest.volumes.iter().map(|volume| volume.name.as_str()))
        .chain(manifest.secrets.iter().map(String::as_str))
        .chain(services)
}

fn is_identity(uid: u32, gid: u32) -> bool {
    // Nonzero, and never the script runner's nobody identity.
    (1..=65533).contains(&uid) && (1..=65533).contains(&gid)
}

fn is_label(text: &str, max: usize) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// Project IDs: lowercase letter, then lowercase letters, digits or `-`, at
/// most 24 bytes, not ending in `-`.
pub fn is_project_id(text: &str) -> bool {
    is_label(text, MAX_ID)
}

fn is_name(text: &str) -> bool {
    is_label(text, MAX_NAME)
}

/// Secret slots: lowercase letter, then lowercase letters, digits or `_`.
pub fn is_slot_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_SLOT
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn is_env_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_uppercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn is_health_path(text: &str) -> bool {
    text.len() <= MAX_PATH
        && text.starts_with('/')
        && text.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'/' | b'-')
        })
        && !text.split('/').any(|segment| segment == "..")
}

fn is_mount_target(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('/') else {
        return false;
    };
    text.len() <= MAX_PATH
        && !rest.is_empty()
        && rest.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        && !RESERVED_TARGETS.iter().any(|reserved| {
            text == *reserved
                || text
                    .strip_prefix(reserved)
                    .is_some_and(|tail| tail.starts_with('/'))
        })
}

fn is_path_component(text: &str) -> bool {
    let bytes = text.as_bytes();
    let alnum = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    !bytes.is_empty()
        && alnum(&bytes[0])
        && alnum(&bytes[bytes.len() - 1])
        && bytes
            .iter()
            .all(|byte| alnum(byte) || matches!(byte, b'.' | b'_' | b'-'))
        && !bytes
            .windows(2)
            .any(|pair| !alnum(&pair[0]) && !alnum(&pair[1]))
}

fn is_registry_host(text: &str) -> bool {
    let (host, port) = match text.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (text, None),
    };
    let host_ok = !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    let port_ok = port.is_none_or(|port| {
        (1..=5).contains(&port.len())
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port != 0)
    });
    host_ok && port_ok
}

/// `[registry[:port]/]path@sha256:<64 lowercase hex>`, lowercase, no tag.
pub fn is_image_ref(text: &str) -> bool {
    let Some((name, digest)) = text.split_once('@') else {
        return false;
    };
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    if !is_lower_hex(hex, 64) || name.is_empty() || text.len() > MAX_IMAGE {
        return false;
    }
    let mut components: Vec<&str> = name.split('/').collect();
    let first = components[0];
    if components.len() > 1 && (first.contains('.') || first.contains(':') || first == "localhost")
    {
        if !is_registry_host(first) {
            return false;
        }
        components.remove(0);
    }
    !components.is_empty()
        && components
            .iter()
            .all(|component| is_path_component(component))
}

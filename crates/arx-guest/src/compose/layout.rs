//! Fixed guest paths and names. Every path is derived here from validated IDs;
//! no request, reply or manifest supplies a path.

/// Root-owned 0700 state root on the persistent disk.
pub const STATE_ROOT: &str = "/var/lib/arx-compose";
/// Version directory under the state root. Renders live under it.
pub const LAYOUT_VERSION: &str = "v1";
/// Dedicated root-owned tmpfs for runtime secret files.
pub const SECRET_ROOT: &str = "/run/arx-compose/secrets";
/// Empty working directory for every engine invocation.
pub const ENGINE_CWD: &str = "/var/lib/arx-compose/empty";
/// Docker client configuration directory owned by the broker.
pub const DOCKER_CONFIG: &str = "/var/lib/arx-compose/docker-config";
/// Docker's local volume data root on the qualified recipe.
pub const DOCKER_VOLUME_ROOT: &str = "/var/lib/docker/volumes";
/// First fixed guest loopback port for routed services.
pub const LOOPBACK_BASE: u16 = 20000;

/// State file path relative to [`STATE_ROOT`].
pub fn state_file() -> String {
    format!("{LAYOUT_VERSION}/state.json")
}

/// Revision directory relative to [`STATE_ROOT`].
pub fn revision_dir_relative(project: &str, revision: u64) -> String {
    format!("{LAYOUT_VERSION}/projects/{project}/r{revision}")
}

pub fn revision_dir(project: &str, revision: u64) -> String {
    format!("{STATE_ROOT}/{}", revision_dir_relative(project, revision))
}

pub fn compose_file(project: &str, revision: u64) -> String {
    format!("{}/compose.yaml", revision_dir(project, revision))
}

pub fn secret_file(project: &str, slot: &str) -> String {
    format!("{SECRET_ROOT}/{project}/{slot}")
}

/// Compose project name. Project IDs contain no `_`, so names cannot collide.
pub fn compose_project(project: &str) -> String {
    format!("arx-{project}")
}

pub fn network_name(project: &str) -> String {
    format!("arx_{project}_net")
}

/// Docker volume name. `_` separates the two IDs, and neither ID may contain
/// it, so distinct (project, volume) pairs never share a name.
pub fn volume_name(project: &str, volume: &str) -> String {
    format!("arx-v_{project}_{volume}")
}

pub fn volume_mountpoint(name: &str) -> String {
    format!("{DOCKER_VOLUME_ROOT}/{name}/_data")
}

/// Ownership label value written on a broker-created volume.
pub fn volume_token(project: &str, volume: &str, uid: u32, gid: u32) -> String {
    format!("{project}/{volume}/{uid}:{gid}")
}

/// Guest loopback port for a project slot (0..8) and service index (0..4).
pub fn loopback_port(slot: u8, service_index: usize) -> u16 {
    LOOPBACK_BASE + u16::from(slot) * 4 + service_index as u16
}

//! Fixed Docker invocations. Every argv is built here from validated IDs and a
//! fixed layout. Runners must start [`DOCKER`] with an empty environment plus
//! exactly [`ENGINE_ENV`], in [`ENGINE_CWD`], with stdin and stderr on the null
//! device. Captured stdout is bounded and parsed into closed types here; raw
//! output never reaches a reply or the log.

use std::time::Duration;

use super::layout::{
    DOCKER_CONFIG, ENGINE_CWD as CWD, compose_file, compose_project, network_name, revision_dir,
};
use super::render::{PROJECT_LABEL, REVISION_LABEL, VOLUME_LABEL};
use super::wire::{HealthState, ServiceState, ServiceView};

/// The only program the Compose executor starts.
pub const DOCKER: &str = "/usr/bin/docker";
/// The complete child environment. The ambient environment is cleared, so no
/// DOCKER_HOST, DOCKER_CONTEXT, COMPOSE_* or proxy variable is inherited.
pub const ENGINE_ENV: [(&str, &str); 3] = [
    ("HOME", "/var/lib/arx-compose/home"),
    ("LANG", "C"),
    ("PATH", "/usr/sbin:/usr/bin:/sbin:/bin"),
];
pub const ENGINE_CWD: &str = CWD;
/// Bytes of stdout kept from a capturing invocation.
pub const MAX_CAPTURE: usize = 16 * 1024;

pub const PULL_TIMEOUT: Duration = Duration::from_secs(600);
pub const APPLY_TIMEOUT: Duration = Duration::from_secs(300);
pub const STOP_TIMEOUT: Duration = Duration::from_secs(120);
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
pub const HEALTH_ATTEMPTS: u32 = 30;
pub const HEALTH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub argv: Vec<String>,
    pub timeout: Duration,
    pub capture: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    /// The process exited and captured output (if any) fit completely within
    /// [`MAX_CAPTURE`].
    Exited { code: i32, stdout: Vec<u8> },
    /// The process exited, but captured stdout exceeded [`MAX_CAPTURE`] or its
    /// reader failed. Incomplete output cannot prove completeness or absence.
    OutputIncomplete { code: i32 },
    /// Killed by a signal other than the runner's own deadline kill.
    Signaled,
    /// The deadline expired and the runner killed the process.
    TimedOut,
    /// The process never started.
    LaunchFailed,
}

/// Starts fixed Docker invocations. The real runner is Linux-only and
/// **unverified on real hardware**; tests use a fake.
pub trait CommandRunner: Send {
    fn run(&mut self, invocation: &Invocation) -> RunResult;
    /// GET `path` from guest `127.0.0.1:port`; true only for a 2xx status.
    fn probe_http(&mut self, port: u16, path: &str) -> bool;
    fn pause(&mut self, duration: Duration);
}

pub trait Clock: Send + Sync {
    fn now_unix(&self) -> u64;
}

fn global(args: &[&str]) -> Vec<String> {
    ["--config", DOCKER_CONFIG]
        .iter()
        .chain(args)
        .map(|arg| (*arg).to_owned())
        .collect()
}

fn compose_with_file(project: &str, revision: u64, file: &str, args: &[&str]) -> Vec<String> {
    let dir = revision_dir(project, revision);
    let name = compose_project(project);
    let mut argv = global(&[
        "compose",
        "--ansi",
        "never",
        "--project-name",
        &name,
        "--project-directory",
        &dir,
        "--file",
        file,
        // An explicit empty env file disables working-directory dotenv.
        "--env-file",
        "/dev/null",
    ]);
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    argv
}

fn compose(project: &str, revision: u64, args: &[&str]) -> Vec<String> {
    let file = compose_file(project, revision);
    compose_with_file(project, revision, &file, args)
}

pub fn image_inspect(image: &str) -> Invocation {
    Invocation {
        argv: global(&[
            "image",
            "inspect",
            "--format",
            "{{.Os}}/{{.Architecture}}",
            image,
        ]),
        timeout: QUERY_TIMEOUT,
        capture: true,
    }
}

pub fn image_pull(image: &str) -> Invocation {
    Invocation {
        argv: global(&[
            "image",
            "pull",
            "--platform",
            "linux/amd64",
            "--quiet",
            image,
        ]),
        timeout: PULL_TIMEOUT,
        capture: false,
    }
}

pub fn volume_list() -> Invocation {
    Invocation {
        argv: global(&["volume", "ls", "--format", "json"]),
        timeout: QUERY_TIMEOUT,
        capture: true,
    }
}

pub fn volume_label(name: &str) -> Invocation {
    let format = format!("{{{{index .Labels \"{VOLUME_LABEL}\"}}}}");
    Invocation {
        argv: global(&["volume", "inspect", "--format", &format, name]),
        timeout: QUERY_TIMEOUT,
        capture: true,
    }
}

pub fn volume_mountpoint(name: &str) -> Invocation {
    Invocation {
        argv: global(&["volume", "inspect", "--format", "{{.Mountpoint}}", name]),
        timeout: QUERY_TIMEOUT,
        capture: true,
    }
}

pub fn volume_create(name: &str, token: &str) -> Invocation {
    let label = format!("{VOLUME_LABEL}={token}");
    Invocation {
        argv: global(&[
            "volume", "create", "--driver", "local", "--label", &label, name,
        ]),
        timeout: QUERY_TIMEOUT,
        capture: false,
    }
}

pub fn compose_up(project: &str, revision: u64) -> Invocation {
    Invocation {
        argv: compose(
            project,
            revision,
            &["up", "--detach", "--no-build", "--pull", "never"],
        ),
        timeout: APPLY_TIMEOUT,
        capture: false,
    }
}

pub fn compose_recreate_file(
    project: &str,
    revision: u64,
    file: &str,
    services: &[String],
) -> Invocation {
    let mut args = vec![
        "up",
        "--detach",
        "--no-build",
        "--pull",
        "never",
        "--force-recreate",
        "--no-deps",
    ];
    args.extend(services.iter().map(String::as_str));
    Invocation {
        argv: compose_with_file(project, revision, file, &args),
        timeout: APPLY_TIMEOUT,
        capture: false,
    }
}

pub fn compose_recreate(project: &str, revision: u64, services: &[String]) -> Invocation {
    let file = compose_file(project, revision);
    compose_recreate_file(project, revision, &file, services)
}

pub fn compose_stop(project: &str, revision: u64) -> Invocation {
    Invocation {
        argv: compose(project, revision, &["stop"]),
        timeout: STOP_TIMEOUT,
        capture: false,
    }
}

/// Fixed down: no `--volumes`, `--rmi` or `--remove-orphans`.
pub fn compose_down(project: &str, revision: u64) -> Invocation {
    Invocation {
        argv: compose(project, revision, &["down"]),
        timeout: STOP_TIMEOUT,
        capture: false,
    }
}

/// Stops one broker-attributed container by its verified hex ID.
pub fn container_stop(id: &str) -> Invocation {
    Invocation {
        argv: global(&["container", "stop", id]),
        timeout: STOP_TIMEOUT,
        capture: false,
    }
}

/// Removes one stopped broker-attributed container by its verified hex ID.
/// Never passes `--volumes`, `-v` or `--force`.
pub fn container_rm(id: &str) -> Invocation {
    Invocation {
        argv: global(&["container", "rm", id]),
        timeout: STOP_TIMEOUT,
        capture: false,
    }
}

/// Fixed-field inspection of a container's exact ID and mount table.
pub fn container_inspect_mounts(id: &str) -> Invocation {
    Invocation {
        argv: global(&[
            "container",
            "inspect",
            "--format",
            "{{.Id}}\n{{json .Mounts}}",
            id,
        ]),
        timeout: QUERY_TIMEOUT,
        capture: true,
    }
}

/// Complete bounded inventory of Docker networks with full IDs and labels.
pub fn network_list() -> Invocation {
    Invocation {
        argv: global(&["network", "ls", "--no-trunc", "--format", "json"]),
        timeout: QUERY_TIMEOUT,
        capture: true,
    }
}

/// Removes one verified broker-attributed network by its hex ID.
pub fn network_rm(id: &str) -> Invocation {
    Invocation {
        argv: global(&["network", "rm", id]),
        timeout: STOP_TIMEOUT,
        capture: false,
    }
}

pub fn compose_ps(project: &str, revision: u64) -> Invocation {
    Invocation {
        argv: compose(project, revision, &["ps", "--all", "--format", "json"]),
        timeout: QUERY_TIMEOUT,
        capture: true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedContainer {
    pub(crate) view: ServiceView,
    pub(crate) id: Option<String>,
}

pub(crate) fn is_container_id(text: &str) -> bool {
    (12..=64).contains(&text.len())
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn parse_json_objects(stdout: &[u8]) -> Option<Vec<serde_json::Value>> {
    let text = std::str::from_utf8(stdout).ok()?;
    let trimmed = text.trim();
    let objects: Vec<serde_json::Value> = if trimmed.is_empty() {
        Vec::new()
    } else if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).ok()?
    } else {
        trimmed
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()
            .ok()?
    };
    if objects.len() > 64 {
        return None;
    }
    Some(objects)
}

fn find_label<'a>(labels: &'a str, key: &str) -> Option<&'a str> {
    labels
        .split(',')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

/// Parses `compose ps --format json` (one object per line, or one array)
/// into every broker-attributed container for `project`, including services
/// retired by a later release. Containers without `PROJECT_LABEL` or labelled
/// for another project are ignored. Returns `None` for output that is not
/// that shape.
pub(crate) fn parse_ps_attributed(stdout: &[u8], project: &str) -> Option<Vec<ObservedContainer>> {
    let objects = parse_json_objects(stdout)?;
    let mut containers = Vec::new();
    for object in &objects {
        let field = |key: &str| object.get(key).and_then(serde_json::Value::as_str);
        let name = field("Service")?;
        let labels = field("Labels").unwrap_or("");
        if find_label(labels, PROJECT_LABEL) != Some(project) {
            continue;
        }
        let id = field("ID")
            .filter(|id| is_container_id(id))
            .map(str::to_owned);
        containers.push(ObservedContainer {
            view: ServiceView {
                name: name.to_owned(),
                state: match field("State").unwrap_or("") {
                    "created" => ServiceState::Created,
                    "running" => ServiceState::Running,
                    "restarting" => ServiceState::Restarting,
                    "paused" => ServiceState::Paused,
                    "exited" => ServiceState::Exited,
                    "dead" => ServiceState::Dead,
                    "removing" => ServiceState::Removing,
                    _ => ServiceState::Unknown,
                },
                health: match field("Health").unwrap_or("") {
                    "" => HealthState::None,
                    "starting" => HealthState::Starting,
                    "healthy" => HealthState::Healthy,
                    _ => HealthState::Unhealthy,
                },
                revision: find_label(labels, REVISION_LABEL).and_then(|value| value.parse().ok()),
            },
            id,
        });
    }
    containers.sort_by(|a, b| a.view.name.cmp(&b.view.name).then(a.id.cmp(&b.id)));
    Some(containers)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerMount {
    pub(crate) mount_type: String,
    pub(crate) source: String,
    pub(crate) destination: String,
    pub(crate) rw: bool,
}

/// Parses `container inspect --format "{{.Id}}\n{{json .Mounts}}"`.
/// Returns `None` if the ID does not match `expected_id` or the mount table is
/// malformed or over the bound.
pub(crate) fn parse_container_mounts(
    stdout: &[u8],
    expected_id: &str,
) -> Option<Vec<ContainerMount>> {
    #[derive(serde::Deserialize)]
    struct RawMount {
        #[serde(rename = "Type")]
        mount_type: String,
        #[serde(rename = "Source")]
        source: String,
        #[serde(rename = "Destination")]
        destination: String,
        #[serde(rename = "RW")]
        rw: bool,
    }

    let text = std::str::from_utf8(stdout).ok()?;
    let trimmed = text.trim_end_matches(['\r', '\n']);
    let (id_line, mounts_line) = trimmed.split_once('\n')?;
    let id_line = id_line.trim();
    if !is_container_id(id_line) || !(id_line == expected_id || id_line.starts_with(expected_id)) {
        return None;
    }
    let mounts_line = mounts_line.trim();
    if mounts_line == "null" {
        return Some(Vec::new());
    }
    let raw: Vec<RawMount> = serde_json::from_str(mounts_line).ok()?;
    if raw.len() > 64 {
        return None;
    }
    Some(
        raw.into_iter()
            .map(|m| ContainerMount {
                mount_type: m.mount_type,
                source: m.source,
                destination: m.destination,
                rw: m.rw,
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NetworkObservation {
    Absent,
    OwnedPresent(Vec<String>),
    ForeignConflict,
    Unavailable,
}

/// Classifies the generated project network from a complete `network ls` JSON
/// inventory. Distinguishes absence, broker-owned presence, foreign exact-name
/// collision, and malformed/over-bound output.
pub(crate) fn parse_network_inventory(stdout: &[u8], project: &str) -> NetworkObservation {
    let Some(objects) = parse_json_objects(stdout) else {
        return NetworkObservation::Unavailable;
    };
    let expected_name = network_name(project);
    let mut owned = Vec::new();
    for object in &objects {
        let field = |key: &str| object.get(key).and_then(serde_json::Value::as_str);
        let Some(name) = field("Name") else {
            return NetworkObservation::Unavailable;
        };
        let labels = field("Labels").unwrap_or("");
        let owner = find_label(labels, PROJECT_LABEL);
        let name_matches = name == expected_name;
        let owner_matches = owner == Some(project);
        if name_matches {
            if !owner_matches {
                return NetworkObservation::ForeignConflict;
            }
            let Some(id) = field("ID").filter(|id| is_container_id(id)) else {
                return NetworkObservation::ForeignConflict;
            };
            owned.push(id.to_owned());
        } else if owner_matches {
            let Some(id) = field("ID").filter(|id| is_container_id(id)) else {
                return NetworkObservation::ForeignConflict;
            };
            owned.push(id.to_owned());
        }
    }
    owned.sort();
    owned.dedup();
    if owned.is_empty() {
        NetworkObservation::Absent
    } else {
        NetworkObservation::OwnedPresent(owned)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumeObservation {
    Absent,
    OwnedPresent,
    Collision,
    Unavailable,
}

/// Classifies one named volume from a complete `volume ls --format json`
/// inventory. Never confuses missing/malformed output with absence.
pub(crate) fn parse_volume_inventory(
    stdout: &[u8],
    name: &str,
    expected_token: &str,
) -> VolumeObservation {
    let Some(objects) = parse_json_objects(stdout) else {
        return VolumeObservation::Unavailable;
    };
    let mut matched: Option<&serde_json::Value> = None;
    for object in &objects {
        let Some(vol_name) = object.get("Name").and_then(serde_json::Value::as_str) else {
            return VolumeObservation::Unavailable;
        };
        if vol_name == name {
            if matched.is_some() {
                return VolumeObservation::Unavailable;
            }
            matched = Some(object);
        }
    }
    let Some(object) = matched else {
        return VolumeObservation::Absent;
    };
    let labels = object
        .get("Labels")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if find_label(labels, VOLUME_LABEL) == Some(expected_token) {
        VolumeObservation::OwnedPresent
    } else {
        VolumeObservation::Collision
    }
}

pub(crate) fn to_service_views(containers: &[ObservedContainer]) -> Vec<ServiceView> {
    let mut views: Vec<ServiceView> = Vec::new();
    for container in containers {
        if let Some(existing) = views
            .iter_mut()
            .find(|view| view.name == container.view.name)
        {
            // Two containers for one service: report the conflict as unknown.
            existing.state = ServiceState::Unknown;
            continue;
        }
        views.push(container.view.clone());
    }
    views.sort_by(|a, b| a.name.cmp(&b.name));
    views
}

/// Parses `compose ps --format json` into views for every broker-attributed
/// container of `project`, including services retired by a later release.
/// Containers labelled for another project or without `PROJECT_LABEL` are
/// ignored.
pub fn parse_ps(stdout: &[u8], project: &str) -> Option<Vec<ServiceView>> {
    parse_ps_attributed(stdout, project).map(|containers| to_service_views(&containers))
}

//! Compose broker unit tests with a fake engine and an in-memory store. No
//! Docker, VM, registry or real secret is involved.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::codec::decode_base64;
use super::*;

const CHALLENGE: &str = "0123456789abcdef0123456789abcdef";
const GENERATION: u64 = 7;
const PROJECT: &str = "calm-life";
const SLOT: &str = "admin_password";

fn op(n: u32) -> String {
    format!("{n:032x}")
}

fn image() -> String {
    format!("registry.example.test/calm/web@sha256:{}", "a".repeat(64))
}

fn manifest() -> Value {
    json!({
        "schema": 1,
        "project_id": PROJECT,
        "services": [{
            "name": "web",
            "image": image(),
            "platform": "linux/amd64",
            "uid": 1000,
            "gid": 1000,
            "http_port": 8080,
            "health_path": "/healthz",
            "read_only_rootfs": true,
            "environment": [
                {"name": "PORT", "value": "8080"},
                {"name": "DATA_DIR", "value": "/data"},
                {"name": "ADMIN_PASSWORD_FILE", "value": "/run/secrets/admin_password"}
            ],
            "mounts": [{"volume": "data", "target": "/data", "read_only": false}],
            "secrets": [SLOT],
            "limits": {"cpu_millis": 500, "memory_mib": 512, "pids": 128}
        }],
        "volumes": [{"name": "data", "uid": 1000, "gid": 1000}],
        "secrets": [SLOT]
    })
}

fn multi_service_secret_manifest() -> Value {
    json!({
        "schema": 1,
        "project_id": PROJECT,
        "services": [
            {
                "name": "web",
                "image": image(),
                "platform": "linux/amd64",
                "uid": 1000,
                "gid": 1000,
                "http_port": 8080,
                "health_path": "/healthz",
                "read_only_rootfs": true,
                "environment": [],
                "mounts": [],
                "secrets": [SLOT],
                "limits": {"cpu_millis": 250, "memory_mib": 256, "pids": 64}
            },
            {
                "name": "api",
                "image": image(),
                "platform": "linux/amd64",
                "uid": 1000,
                "gid": 1000,
                "http_port": null,
                "health_path": null,
                "read_only_rootfs": true,
                "environment": [],
                "mounts": [],
                "secrets": [SLOT],
                "limits": {"cpu_millis": 250, "memory_mib": 256, "pids": 64}
            },
            {
                "name": "worker",
                "image": image(),
                "platform": "linux/amd64",
                "uid": 1000,
                "gid": 1000,
                "http_port": null,
                "health_path": null,
                "read_only_rootfs": true,
                "environment": [],
                "mounts": [],
                "secrets": [],
                "limits": {"cpu_millis": 100, "memory_mib": 128, "pids": 32}
            }
        ],
        "volumes": [],
        "secrets": [SLOT]
    })
}

fn plain(id: &str, volumes: &[&str]) -> Value {
    json!({
        "schema": 1,
        "project_id": id,
        "services": [{
            "name": "app",
            "image": format!("registry.example.test/app@sha256:{}", "b".repeat(64)),
            "platform": "linux/amd64",
            "uid": 2000,
            "gid": 2000,
            "http_port": null,
            "health_path": null,
            "read_only_rootfs": false,
            "environment": [],
            "mounts": volumes.iter().map(|volume| json!({
                "volume": volume, "target": format!("/{volume}"), "read_only": false
            })).collect::<Vec<_>>(),
            "secrets": [],
            "limits": {"cpu_millis": 100, "memory_mib": 64, "pids": 32}
        }],
        "volumes": volumes.iter().map(|volume| json!({
            "name": volume, "uid": 2000, "gid": 2000
        })).collect::<Vec<_>>(),
        "secrets": []
    })
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

#[derive(Clone, Debug)]
struct FakeMount {
    mount_type: String,
    source: String,
    destination: String,
    rw: bool,
}

#[derive(Clone, Debug)]
struct FakeNetwork {
    id: String,
    name: String,
    project_label: Option<String>,
}

struct Fault {
    needle: &'static str,
    result: RunResult,
    apply: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoreFault {
    BeforeRename,
    AfterRename,
}

#[derive(Clone)]
struct Container {
    id: String,
    service: String,
    state: &'static str,
    revision: u64,
    mounts: Vec<FakeMount>,
}

/// Extracts read-only secret bind mounts for each service from rendered Compose YAML.
fn parse_rendered_secret_mounts(yaml: &str) -> BTreeMap<String, Vec<FakeMount>> {
    let mut slot_files: BTreeMap<String, String> = BTreeMap::new();
    let mut service_slots: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut section = "";
    let mut current_service: Option<String> = None;
    let mut in_service_secrets = false;
    let mut pending_source: Option<String> = None;
    let mut current_top_secret: Option<String> = None;

    for raw_line in yaml.lines() {
        let line = raw_line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            section = line.trim_end_matches(':');
            current_service = None;
            in_service_secrets = false;
            pending_source = None;
            current_top_secret = None;
            continue;
        }
        let indent = line.bytes().take_while(|b| *b == b' ').count();
        let trimmed = line.trim();
        match section {
            "services" => {
                if indent == 2 && trimmed.ends_with(':') {
                    let name = trimmed.trim_end_matches(':').trim_matches('"').to_owned();
                    current_service = Some(name);
                    in_service_secrets = false;
                    pending_source = None;
                } else if indent == 4 && trimmed.ends_with(':') {
                    in_service_secrets = trimmed == "secrets:";
                    pending_source = None;
                } else if in_service_secrets && indent >= 6 {
                    if let Some(rest) = trimmed.strip_prefix("- source:") {
                        pending_source = Some(rest.trim().trim_matches('"').to_owned());
                    } else if let Some(rest) = trimmed.strip_prefix("target:")
                        && let (Some(svc), Some(src)) =
                            (current_service.as_ref(), pending_source.take())
                    {
                        let target = rest.trim().trim_matches('"').to_owned();
                        service_slots
                            .entry(svc.clone())
                            .or_default()
                            .push((src, target));
                    }
                }
            }
            "secrets" => {
                if indent == 2 && trimmed.ends_with(':') {
                    let slot = trimmed.trim_end_matches(':').trim_matches('"').to_owned();
                    current_top_secret = Some(slot);
                } else if indent == 4
                    && let Some(rest) = trimmed.strip_prefix("file:")
                    && let Some(slot) = current_top_secret.as_ref()
                {
                    slot_files.insert(slot.clone(), rest.trim().trim_matches('"').to_owned());
                }
            }
            _ => {}
        }
    }

    let mut result: BTreeMap<String, Vec<FakeMount>> = BTreeMap::new();
    for (svc, slots) in service_slots {
        let mut mounts = Vec::new();
        for (slot, target) in slots {
            if let Some(source) = slot_files.get(&slot) {
                mounts.push(FakeMount {
                    mount_type: "bind".to_owned(),
                    source: source.clone(),
                    destination: target,
                    rw: false,
                });
            }
        }
        result.insert(svc, mounts);
    }
    result
}

struct Docker {
    disk: Arc<Mutex<Disk>>,
    images: BTreeSet<String>,
    arch: Option<&'static str>,
    pull_fails: bool,
    volumes: BTreeMap<String, String>,
    containers: BTreeMap<String, Vec<Container>>,
    networks: BTreeMap<String, FakeNetwork>,
    keep_network_on_down: bool,
    services: BTreeMap<String, Vec<String>>,
    revision_services: BTreeMap<(String, u64), Vec<String>>,
    next_container_id: u64,
    next_network_id: u64,
    /// Optional per-service mount overrides applied on the next container creation
    /// (for testing stale mounts or RW mounts even after container ID replacement).
    override_mounts_once: BTreeMap<String, Vec<FakeMount>>,
    /// When set, `up --force-recreate` only replaces services listed here.
    recreate_subset_once: Option<BTreeSet<String>>,
    output_canary: Option<String>,
    unhealthy: BTreeSet<u64>,
    faults: Vec<Fault>,
    block: Option<(&'static str, mpsc::Receiver<()>)>,
    calls: Vec<Invocation>,
    probes: Vec<(u16, String)>,
}

fn exited(code: i32, stdout: impl Into<Vec<u8>>) -> RunResult {
    RunResult::Exited {
        code,
        stdout: stdout.into(),
    }
}

impl Docker {
    fn new(disk: Arc<Mutex<Disk>>) -> Self {
        Self {
            disk,
            images: BTreeSet::new(),
            arch: None,
            pull_fails: false,
            volumes: BTreeMap::new(),
            containers: BTreeMap::new(),
            networks: BTreeMap::new(),
            keep_network_on_down: false,
            services: BTreeMap::new(),
            revision_services: BTreeMap::new(),
            next_container_id: 0,
            next_network_id: 0,
            override_mounts_once: BTreeMap::new(),
            recreate_subset_once: None,
            output_canary: None,
            unhealthy: BTreeSet::new(),
            faults: Vec::new(),
            block: None,
            calls: Vec::new(),
            probes: Vec::new(),
        }
    }

    fn fresh_id(&mut self) -> String {
        self.next_container_id += 1;
        format!("{:012x}", self.next_container_id)
    }

    fn fresh_network_id(&mut self) -> String {
        self.next_network_id += 1;
        format!("a{:063x}", self.next_network_id)
    }

    fn ensure_project_network(&mut self, project: &str) {
        let name = network_name(project);
        if self.networks.values().any(|n| n.name == name) {
            return;
        }
        let id = self.fresh_network_id();
        self.networks.insert(
            id.clone(),
            FakeNetwork {
                id,
                name,
                project_label: Some(project.to_owned()),
            },
        );
    }

    fn declared_services(&self, project: &str, revision: u64) -> Vec<String> {
        self.revision_services
            .get(&(project.to_owned(), revision))
            .or_else(|| self.services.get(project))
            .cloned()
            .unwrap_or_default()
    }

    fn mounts_for_file(
        &mut self,
        project: &str,
        revision: u64,
        file: &str,
    ) -> BTreeMap<String, Vec<FakeMount>> {
        let yaml = {
            let disk = self.disk.lock().unwrap();
            let leaf = file.rsplit('/').next().unwrap_or("");
            if leaf == "compose.yaml" {
                disk.revisions.get(&(project.to_owned(), revision)).cloned()
            } else if let Some(rest) = leaf
                .strip_prefix("rot-")
                .and_then(|s| s.strip_suffix(".yaml"))
            {
                disk.rotations
                    .get(&(project.to_owned(), revision, rest.to_owned()))
                    .cloned()
            } else {
                None
            }
        };
        let mut parsed = yaml
            .as_deref()
            .map(parse_rendered_secret_mounts)
            .unwrap_or_default();
        for (svc, mounts) in std::mem::take(&mut self.override_mounts_once) {
            parsed.insert(svc, mounts);
        }
        parsed
    }

    fn noise(&self) -> String {
        self.output_canary.clone().unwrap_or_default()
    }

    fn exec(&mut self, argv: &[String]) -> RunResult {
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        assert_eq!(&args[..2], ["--config", DOCKER_CONFIG]);
        match &args[2..] {
            [
                "image",
                "inspect",
                "--format",
                "{{.Os}}/{{.Architecture}}",
                image,
            ] => {
                if self.images.contains(*image) {
                    exited(0, format!("{}\n", self.arch.unwrap_or("linux/amd64")))
                } else {
                    exited(1, self.noise())
                }
            }
            [
                "image",
                "pull",
                "--platform",
                "linux/amd64",
                "--quiet",
                image,
            ] => {
                if self.pull_fails {
                    exited(1, self.noise())
                } else {
                    self.images.insert((*image).to_owned());
                    exited(0, self.noise())
                }
            }
            ["volume", "ls", "--format", "json"] => {
                let extra = self.output_canary.as_deref();
                let lines: String = self
                    .volumes
                    .iter()
                    .map(|(name, token)| {
                        let labels = if token.is_empty() {
                            String::new()
                        } else {
                            format!("{VOLUME_LABEL}={token}")
                        };
                        let mut obj = json!({
                            "Name": name,
                            "Labels": labels,
                        });
                        if let Some(canary) = extra {
                            obj["Driver"] = json!(canary);
                        }
                        format!("{obj}\n")
                    })
                    .collect();
                exited(0, lines)
            }
            ["volume", "inspect", "--format", "{{.Mountpoint}}", name] => {
                if self.volumes.contains_key(*name) {
                    exited(0, format!("/var/lib/docker/volumes/{name}/_data\n"))
                } else {
                    exited(1, self.noise())
                }
            }
            ["volume", "inspect", "--format", format, name] => {
                assert_eq!(*format, "{{index .Labels \"org.arxcovo.volume\"}}");
                match self.volumes.get(*name) {
                    Some(label) => exited(0, format!("{label}\n")),
                    None => exited(1, self.noise()),
                }
            }
            [
                "volume",
                "create",
                "--driver",
                "local",
                "--label",
                label,
                name,
            ] => {
                let token = label.strip_prefix("org.arxcovo.volume=").unwrap();
                self.volumes
                    .entry((*name).to_owned())
                    .or_insert_with(|| token.to_owned());
                exited(0, self.noise())
            }
            ["network", "ls", "--no-trunc", "--format", "json"] => {
                let extra = self.output_canary.as_deref();
                let lines: String = self
                    .networks
                    .values()
                    .map(|net| {
                        let labels = match &net.project_label {
                            Some(p) => format!("{PROJECT_LABEL}={p}"),
                            None => String::new(),
                        };
                        let mut obj = json!({
                            "ID": net.id,
                            "Name": net.name,
                            "Labels": labels,
                        });
                        if let Some(canary) = extra {
                            obj["Scope"] = json!(canary);
                        }
                        format!("{obj}\n")
                    })
                    .collect();
                exited(0, lines)
            }
            ["network", "rm", id] => {
                if self.networks.remove(*id).is_some() {
                    exited(0, self.noise())
                } else {
                    exited(1, self.noise())
                }
            }
            [
                "container",
                "inspect",
                "--format",
                "{{.Id}}\n{{json .Mounts}}",
                id,
            ] => {
                let extra = self.output_canary.as_deref();
                let found = self
                    .containers
                    .values()
                    .flatten()
                    .find(|c| c.id == *id)
                    .cloned();
                match found {
                    Some(container) => {
                        let mounts: Vec<Value> = container
                            .mounts
                            .iter()
                            .map(|m| {
                                let mut obj = json!({
                                    "Type": m.mount_type,
                                    "Source": m.source,
                                    "Destination": m.destination,
                                    "RW": m.rw,
                                });
                                if let Some(canary) = extra {
                                    obj["Propagation"] = json!(canary);
                                }
                                obj
                            })
                            .collect();
                        let mounts_json = serde_json::to_string(&mounts).unwrap();
                        exited(0, format!("{}\n{mounts_json}\n", container.id))
                    }
                    None => exited(1, self.noise()),
                }
            }
            ["container", "stop", id] => {
                let mut found = false;
                for list in self.containers.values_mut() {
                    for container in list {
                        if container.id == *id {
                            container.state = "exited";
                            found = true;
                        }
                    }
                }
                if found {
                    exited(0, self.noise())
                } else {
                    exited(1, self.noise())
                }
            }
            ["container", "rm", id] => {
                let mut removed = false;
                for list in self.containers.values_mut() {
                    if let Some(pos) = list.iter().position(|c| c.id == *id) {
                        assert_ne!(
                            list[pos].state, "running",
                            "container rm without --force requires stopped container"
                        );
                        list.remove(pos);
                        removed = true;
                    }
                }
                if removed {
                    exited(0, self.noise())
                } else {
                    exited(1, self.noise())
                }
            }
            [
                "compose",
                "--ansi",
                "never",
                "--project-name",
                name,
                "--project-directory",
                dir,
                "--file",
                file,
                "--env-file",
                "/dev/null",
                rest @ ..,
            ] => {
                let project = name.strip_prefix("arx-").unwrap().to_owned();
                let is_compose_yaml = *file == format!("{dir}/compose.yaml");
                let is_rot_yaml =
                    file.starts_with(&format!("{dir}/rot-")) && file.ends_with(".yaml");
                assert!(
                    is_compose_yaml || is_rot_yaml,
                    "unexpected compose file {file}"
                );
                let revision: u64 = dir
                    .rsplit('/')
                    .next()
                    .and_then(|leaf| leaf.strip_prefix('r'))
                    .unwrap()
                    .parse()
                    .unwrap();
                match rest {
                    [
                        "up",
                        "--detach",
                        "--no-build",
                        "--pull",
                        "never",
                        "--force-recreate",
                        "--no-deps",
                        targets @ ..,
                    ] => {
                        let subset = self.recreate_subset_once.take();
                        let mounts_by_svc = self.mounts_for_file(&project, revision, file);
                        let new_ids: Vec<(String, String)> = targets
                            .iter()
                            .filter(|s| subset.as_ref().is_none_or(|sub| sub.contains(**s)))
                            .map(|service| ((*service).to_owned(), self.fresh_id()))
                            .collect();
                        self.ensure_project_network(&project);
                        let list = self.containers.entry(project).or_default();
                        for (service, id) in new_ids {
                            let mounts = mounts_by_svc.get(&service).cloned().unwrap_or_default();
                            list.retain(|container| container.service != service);
                            list.push(Container {
                                id,
                                service,
                                state: "running",
                                revision,
                                mounts,
                            });
                        }
                        exited(0, self.noise())
                    }
                    ["up", "--detach", "--no-build", "--pull", "never"] => {
                        assert!(is_compose_yaml, "normal up must use compose.yaml");
                        let services = self.declared_services(&project, revision);
                        let mounts_by_svc = self.mounts_for_file(&project, revision, file);
                        let new_ids: Vec<(String, String)> = services
                            .into_iter()
                            .map(|service| (service, self.fresh_id()))
                            .collect();
                        self.ensure_project_network(&project);
                        // Real Docker Compose `up` without `--remove-orphans`
                        // replaces declared services and leaves retired ones.
                        let list = self.containers.entry(project).or_default();
                        for (service, id) in new_ids {
                            let mounts = mounts_by_svc.get(&service).cloned().unwrap_or_default();
                            list.retain(|container| container.service != service);
                            list.push(Container {
                                id,
                                service,
                                state: "running",
                                revision,
                                mounts,
                            });
                        }
                        exited(0, self.noise())
                    }
                    ["stop"] => {
                        let services = self.declared_services(&project, revision);
                        for container in self.containers.entry(project).or_default() {
                            if services.contains(&container.service) {
                                container.state = "exited";
                            }
                        }
                        exited(0, self.noise())
                    }
                    ["down"] => {
                        let services = self.declared_services(&project, revision);
                        if let Some(list) = self.containers.get_mut(&project) {
                            list.retain(|container| !services.contains(&container.service));
                        }
                        let still_has_containers = self
                            .containers
                            .get(&project)
                            .is_some_and(|list| !list.is_empty());
                        if !still_has_containers && !self.keep_network_on_down {
                            let net_name = network_name(&project);
                            self.networks.retain(|_, net| {
                                !(net.name == net_name
                                    && net.project_label.as_deref() == Some(project.as_str()))
                            });
                        }
                        exited(0, self.noise())
                    }
                    ["ps", "--all", "--format", "json"] => {
                        let extra = self.output_canary.as_deref();
                        let lines: String = self
                            .containers
                            .get(&project)
                            .into_iter()
                            .flatten()
                            .map(|container| {
                                let mut obj = json!({
                                    "ID": container.id,
                                    "Service": container.service,
                                    "State": container.state,
                                    "Health": "",
                                    "Labels": format!(
                                        "org.arxcovo.project={project},org.arxcovo.revision={}",
                                        container.revision
                                    ),
                                });
                                if let Some(canary) = extra {
                                    obj["Status"] = json!(canary);
                                }
                                format!("{obj}\n")
                            })
                            .collect();
                        exited(0, lines)
                    }
                    other => panic!("unexpected compose arguments {other:?}"),
                }
            }
            other => panic!("unexpected docker argv {other:?}"),
        }
    }
}

struct FakeRunner(Arc<Mutex<Docker>>);

impl CommandRunner for FakeRunner {
    fn run(&mut self, invocation: &Invocation) -> RunResult {
        let joined = invocation.argv.join(" ");
        let blocker = {
            let mut docker = self.0.lock().unwrap();
            docker.calls.push(invocation.clone());
            let hit = docker
                .block
                .as_ref()
                .is_some_and(|(needle, _)| joined.contains(*needle));
            if hit {
                docker.block.take().map(|(_, receiver)| receiver)
            } else {
                None
            }
        };
        if let Some(receiver) = blocker {
            receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        let mut docker = self.0.lock().unwrap();
        if let Some(index) = docker
            .faults
            .iter()
            .position(|fault| joined.contains(fault.needle))
        {
            let fault = docker.faults.remove(index);
            if fault.apply {
                docker.exec(&invocation.argv);
            }
            return fault.result;
        }
        docker.exec(&invocation.argv)
    }

    fn probe_http(&mut self, port: u16, path: &str) -> bool {
        let mut docker = self.0.lock().unwrap();
        docker.probes.push((port, path.to_owned()));
        let running = docker
            .containers
            .values()
            .flatten()
            .filter(|container| container.state == "running")
            .map(|container| container.revision)
            .max();
        running.is_some_and(|revision| !docker.unhealthy.contains(&revision))
    }

    fn pause(&mut self, _: Duration) {}
}

/// Secret bytes and their immutable inode witness.
type StoredSecret = (Vec<u8>, SecretFileWitness);

#[derive(Default)]
struct Disk {
    state: Option<Vec<u8>>,
    // Only snapshots that completed the directory sync survive a simulated crash.
    save_snapshots: Vec<Vec<u8>>,
    load_overrides: VecDeque<Option<Vec<u8>>>,
    revisions: BTreeMap<(String, u64), String>,
    rotations: BTreeMap<(String, u64, String), String>,
    secrets: BTreeMap<(String, String), StoredSecret>,
    next_ino: u64,
    prepared: BTreeMap<String, (u32, u32)>,
    fail_save_state: bool,
    save_faults: VecDeque<Option<StoreFault>>,
    fail_write_secret: bool,
    write_secret_faults: VecDeque<StoreFault>,
    fail_write_rotation: bool,
    fail_prepare_volume: bool,
    secret_writes: usize,
}

impl Disk {
    fn next_witness(&mut self, owner: (u32, u32)) -> SecretFileWitness {
        self.next_ino += 1;
        SecretFileWitness {
            dev: 77,
            ino: self.next_ino,
            uid: owner.0,
            gid: owner.1,
        }
    }
}

struct MemoryStore(Arc<Mutex<Disk>>);

impl ComposeStore for MemoryStore {
    fn load_state(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        let mut disk = self.0.lock().unwrap();
        Ok(disk
            .load_overrides
            .pop_front()
            .unwrap_or_else(|| disk.state.clone()))
    }

    fn save_state(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        let mut disk = self.0.lock().unwrap();
        if let Some(maybe_fault) = disk.save_faults.pop_front()
            && let Some(fault) = maybe_fault
        {
            if fault == StoreFault::AfterRename {
                disk.state = Some(bytes.to_vec());
            }
            return Err(StoreError);
        }
        if disk.fail_save_state {
            return Err(StoreError);
        }
        disk.state = Some(bytes.to_vec());
        disk.save_snapshots.push(bytes.to_vec());
        Ok(())
    }

    fn write_revision(
        &mut self,
        project: &str,
        revision: u64,
        yaml: &str,
    ) -> Result<(), StoreError> {
        self.0
            .lock()
            .unwrap()
            .revisions
            .insert((project.to_owned(), revision), yaml.to_owned());
        Ok(())
    }

    fn write_rotation(
        &mut self,
        project: &str,
        revision: u64,
        operation_id: &str,
        yaml: &str,
    ) -> Result<(), StoreError> {
        let mut disk = self.0.lock().unwrap();
        if disk.fail_write_rotation {
            return Err(StoreError);
        }
        disk.rotations.insert(
            (project.to_owned(), revision, operation_id.to_owned()),
            yaml.to_owned(),
        );
        Ok(())
    }

    fn write_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
        bytes: &[u8],
    ) -> Result<SecretFileWitness, StoreError> {
        let mut disk = self.0.lock().unwrap();
        if disk
            .secrets
            .contains_key(&(project.to_owned(), instance.to_owned()))
        {
            return Err(StoreError);
        }
        if let Some(fault) = disk.write_secret_faults.pop_front() {
            if fault == StoreFault::AfterRename {
                disk.secret_writes += 1;
                let witness = disk.next_witness(owner);
                disk.secrets.insert(
                    (project.to_owned(), instance.to_owned()),
                    (bytes.to_vec(), witness),
                );
            }
            return Err(StoreError);
        }
        if disk.fail_write_secret {
            return Err(StoreError);
        }
        disk.secret_writes += 1;
        let witness = disk.next_witness(owner);
        disk.secrets.insert(
            (project.to_owned(), instance.to_owned()),
            (bytes.to_vec(), witness),
        );
        Ok(witness)
    }

    fn stat_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
    ) -> Option<SecretFileWitness> {
        self.0
            .lock()
            .unwrap()
            .secrets
            .get(&(project.to_owned(), instance.to_owned()))
            .filter(|(_, w)| (w.uid, w.gid) == owner)
            .map(|(_, w)| *w)
    }

    fn compare_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
        bytes: &[u8],
    ) -> SecretMatch {
        let disk = self.0.lock().unwrap();
        match disk.secrets.get(&(project.to_owned(), instance.to_owned())) {
            None => SecretMatch::Missing,
            Some((stored, w)) if (w.uid, w.gid) == owner && stored.as_slice() == bytes => {
                SecretMatch::Match(*w)
            }
            Some(_) => SecretMatch::Mismatch,
        }
    }

    fn prepare_volume(&mut self, name: &str, owner: (u32, u32)) -> Result<(), StoreError> {
        let mut disk = self.0.lock().unwrap();
        if disk.fail_prepare_volume {
            return Err(StoreError);
        }
        disk.prepared.insert(name.to_owned(), owner);
        Ok(())
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now_unix(&self) -> u64 {
        1_800_000_000
    }
}

struct Harness {
    svc: ComposeService,
    docker: Arc<Mutex<Docker>>,
    disk: Arc<Mutex<Disk>>,
}

impl Harness {
    fn new() -> Self {
        Self::on(Arc::new(Mutex::new(Disk::default())))
    }

    fn on(disk: Arc<Mutex<Disk>>) -> Self {
        let docker = Arc::new(Mutex::new(Docker::new(Arc::clone(&disk))));
        docker
            .lock()
            .unwrap()
            .services
            .insert(PROJECT.to_owned(), vec!["web".to_owned()]);
        Self::on_shared(disk, docker)
    }

    fn on_shared(disk: Arc<Mutex<Disk>>, docker: Arc<Mutex<Docker>>) -> Self {
        let svc = ComposeService::new(
            Box::new(FakeRunner(Arc::clone(&docker))),
            Box::new(MemoryStore(Arc::clone(&disk))),
            Box::new(FixedClock),
        )
        .unwrap();
        Self { svc, docker, disk }
    }

    fn hello(&self) -> ComposeResponse {
        self.hello_gen(GENERATION)
    }

    fn hello_gen(&self, generation: u64) -> ComposeResponse {
        self.svc.handle(ComposeRequest::ComposeHello {
            version: COMPOSE_PROTOCOL_VERSION,
            challenge: CHALLENGE.to_owned(),
            generation,
            capabilities: COMPOSE_CAPABILITIES.map(str::to_owned).to_vec(),
        })
    }

    fn stage(&self, manifest: &[u8]) -> String {
        let digest = sha256_hex(manifest);
        let chunks = manifest.len().div_ceil(STAGE_CHUNK) as u32;
        let begun = self.svc.handle(ComposeRequest::ComposeStageBegin {
            id: 1,
            digest: digest.clone(),
            total_bytes: manifest.len() as u32,
            chunks,
        });
        let ComposeResponse::ComposeStageProgress { next_index, .. } = begun else {
            panic!("stage begin refused: {begun:?}");
        };
        if next_index == 0 {
            for (index, chunk) in manifest.chunks(STAGE_CHUNK).enumerate() {
                let progress = self.svc.handle(ComposeRequest::ComposeStageChunk {
                    id: 2,
                    digest: digest.clone(),
                    index: index as u32,
                    data: encode_base64(chunk),
                });
                assert!(
                    matches!(progress, ComposeResponse::ComposeStageProgress { .. }),
                    "{progress:?}"
                );
            }
        }
        let committed = self.svc.handle(ComposeRequest::ComposeStageCommit {
            id: 3,
            digest: digest.clone(),
        });
        assert!(
            matches!(committed, ComposeResponse::ComposeReleaseStaged { .. }),
            "{committed:?}"
        );
        digest
    }

    fn put_secret(
        &self,
        operation: u32,
        digest: &str,
        epoch: u64,
        value: &[u8],
    ) -> ComposeResponse {
        self.put_secret_as(operation, GENERATION, PROJECT, digest, SLOT, epoch, value)
    }

    #[allow(clippy::too_many_arguments)]
    fn put_secret_as(
        &self,
        operation: u32,
        generation: u64,
        project_id: &str,
        digest: &str,
        slot: &str,
        epoch: u64,
        value: &[u8],
    ) -> ComposeResponse {
        self.svc.handle(ComposeRequest::ComposeSecretPut {
            id: 4,
            operation_id: op(operation),
            generation,
            project_id: project_id.to_owned(),
            digest: digest.to_owned(),
            slot: slot.to_owned(),
            epoch,
            data: SecretText::new(encode_base64(value)),
        })
    }

    fn deploy(
        &self,
        operation: u32,
        project: &str,
        revision: u64,
        digest: &str,
    ) -> ComposeResponse {
        self.svc.handle(ComposeRequest::ComposeDeploy {
            id: 5,
            operation_id: op(operation),
            project_id: project.to_owned(),
            revision,
            digest: digest.to_owned(),
        })
    }

    fn lifecycle(&self, operation: u32, project: &str, kind: OperationKind) -> ComposeResponse {
        let (id, operation_id, project_id) = (6, op(operation), project.to_owned());
        self.svc.handle(match kind {
            OperationKind::Start => ComposeRequest::ComposeStart {
                id,
                operation_id,
                project_id,
            },
            OperationKind::Stop => ComposeRequest::ComposeStop {
                id,
                operation_id,
                project_id,
            },
            OperationKind::Down => ComposeRequest::ComposeDown {
                id,
                operation_id,
                project_id,
            },
            other => panic!("not a lifecycle request: {other:?}"),
        })
    }

    fn reconcile(&self, operation: u32) -> ComposeResponse {
        self.svc.handle(ComposeRequest::ComposeReconcile {
            id: 11,
            operation_id: op(operation),
        })
    }

    fn settle(&self) {
        assert!(self.svc.wait_idle(Duration::from_secs(5)), "operation hung");
    }

    fn operation(&self, operation: u32) -> OperationView {
        match self.svc.handle(ComposeRequest::ComposeOperation {
            id: 9,
            operation_id: op(operation),
        }) {
            ComposeResponse::ComposeOperationState { operation, .. } => operation,
            other => panic!("unexpected reply {other:?}"),
        }
    }

    fn status(&self, project: &str) -> ProjectView {
        match self.svc.handle(ComposeRequest::ComposeStatus {
            id: 10,
            project_id: project.to_owned(),
        }) {
            ComposeResponse::ComposeProjectStatus { project, .. } => project,
            other => panic!("unexpected reply {other:?}"),
        }
    }

    /// Hello, stage, secret and a successful revision-1 deploy as op 1.
    fn deployed(&self) -> String {
        self.hello();
        let digest = self.stage(&bytes(&manifest()));
        assert!(matches!(
            self.put_secret(90, &digest, 1, b"first value"),
            ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
        ));
        self.deploy(1, PROJECT, 1, &digest);
        self.settle();
        assert_eq!(self.operation(1).state, OperationState::Succeeded);
        digest
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.docker
            .lock()
            .unwrap()
            .calls
            .iter()
            .map(|call| call.argv.clone())
            .collect()
    }

    fn count(&self, needle: &str) -> usize {
        self.calls()
            .iter()
            .filter(|argv| argv.join(" ").contains(needle))
            .count()
    }
}

fn rejected(response: &ComposeResponse) -> Option<RejectCode> {
    match response {
        ComposeResponse::ComposeRejected { code, .. } => Some(*code),
        _ => None,
    }
}

fn compose_argv_file(revision: u64, file: &str, tail: &[&str]) -> Vec<String> {
    let dir = format!("/var/lib/arx-compose/v1/projects/calm-life/r{revision}");
    let mut argv: Vec<String> = [
        "--config",
        "/var/lib/arx-compose/docker-config",
        "compose",
        "--ansi",
        "never",
        "--project-name",
        "arx-calm-life",
        "--project-directory",
        dir.as_str(),
        "--file",
        file,
        "--env-file",
        "/dev/null",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    argv.extend(tail.iter().map(|arg| (*arg).to_owned()));
    argv
}

fn compose_argv(revision: u64, tail: &[&str]) -> Vec<String> {
    let file = format!("/var/lib/arx-compose/v1/projects/calm-life/r{revision}/compose.yaml");
    compose_argv_file(revision, &file, tail)
}

fn golden_yaml_for(instance: &str) -> String {
    format!(
        r#"# Generated by arx-guest from a validated project. Layout v1.
name: "arx-calm-life"
services:
  "web":
    image: "registry.example.test/calm/web@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    platform: "linux/amd64"
    user: "1000:1000"
    read_only: true
    privileged: false
    restart: "no"
    ipc: "private"
    security_opt:
      - "no-new-privileges:true"
    cap_drop:
      - "ALL"
    cpus: "0.500"
    mem_limit: "512m"
    pids_limit: 128
    environment:
      "ADMIN_PASSWORD_FILE": "/run/secrets/admin_password"
      "DATA_DIR": "/data"
      "PORT": "8080"
    ports:
      - "127.0.0.1:20000:8080/tcp"
    networks:
      - "arx-project"
    volumes:
      - type: "volume"
        source: "arx-v_calm-life_data"
        target: "/data"
        read_only: false
        volume:
          nocopy: true
    secrets:
      - source: "admin_password"
        target: "/run/secrets/admin_password"
    labels:
      "org.arxcovo.project": "calm-life"
      "org.arxcovo.revision": "1"
    logging:
      driver: "none"
networks:
  "arx-project":
    name: "arx_calm-life_net"
    driver: "bridge"
    labels:
      "org.arxcovo.project": "calm-life"
volumes:
  "arx-v_calm-life_data":
    external: true
    name: "arx-v_calm-life_data"
secrets:
  "admin_password":
    file: "/run/arx-compose/secrets/calm-life/{instance}"
"#
    )
}

#[test]
fn render_is_canonical_hardened_and_disables_restart() {
    let raw = bytes(&manifest());
    let digest = sha256_hex(&raw);
    let instance = secret_instance(SLOT, &digest, 1, GENERATION);
    let mut instances = BTreeMap::new();
    instances.insert(SLOT.to_owned(), instance.clone());
    let project = Project::parse(&raw).unwrap();
    let yaml = render(&project, 1, 0, &instances);
    assert_eq!(yaml, golden_yaml_for(&instance));
    assert_eq!(
        render(&project, 1, 0, &instances),
        yaml,
        "render is deterministic"
    );
    // Another slot moves only the loopback port.
    assert!(render(&project, 1, 3, &instances).contains("\"127.0.0.1:20012:8080/tcp\""));
    for forbidden in [
        "$",
        "build",
        "privileged: true",
        "host",
        "driver_opts",
        "external: false",
    ] {
        assert!(
            !yaml.contains(forbidden),
            "rendered YAML contains {forbidden}"
        );
    }
}

#[test]
fn manifest_rejects_every_forbidden_input() {
    assert!(Project::parse(&bytes(&manifest())).is_ok());
    type Edit = fn(&mut Value);
    let cases: Vec<(&str, Edit, ManifestRule)> = vec![
        (
            "include",
            |m| m["include"] = json!(["other.yaml"]),
            ManifestRule::Malformed,
        ),
        (
            "top networks",
            |m| m["networks"] = json!({"lan": {"external": true}}),
            ManifestRule::Malformed,
        ),
        (
            "build",
            |m| m["services"][0]["build"] = json!("."),
            ManifestRule::Malformed,
        ),
        (
            "extends",
            |m| m["services"][0]["extends"] = json!({"service": "base"}),
            ManifestRule::Malformed,
        ),
        (
            "privileged",
            |m| m["services"][0]["privileged"] = json!(true),
            ManifestRule::Malformed,
        ),
        (
            "cap_add",
            |m| m["services"][0]["cap_add"] = json!(["NET_ADMIN"]),
            ManifestRule::Malformed,
        ),
        (
            "devices",
            |m| m["services"][0]["devices"] = json!(["/dev/kvm"]),
            ManifestRule::Malformed,
        ),
        (
            "network_mode",
            |m| m["services"][0]["network_mode"] = json!("host"),
            ManifestRule::Malformed,
        ),
        (
            "pid",
            |m| m["services"][0]["pid"] = json!("host"),
            ManifestRule::Malformed,
        ),
        (
            "ipc",
            |m| m["services"][0]["ipc"] = json!("host"),
            ManifestRule::Malformed,
        ),
        (
            "ports",
            |m| m["services"][0]["ports"] = json!(["0.0.0.0:80:8080"]),
            ManifestRule::Malformed,
        ),
        (
            "service networks",
            |m| m["services"][0]["networks"] = json!(["lan"]),
            ManifestRule::Malformed,
        ),
        (
            "driver_opts",
            |m| m["volumes"][0]["driver_opts"] = json!({"type": "nfs"}),
            ManifestRule::Malformed,
        ),
        (
            "external volume",
            |m| m["volumes"][0]["external"] = json!(true),
            ManifestRule::Malformed,
        ),
        (
            "bind mount",
            |m| m["services"][0]["mounts"][0]["type"] = json!("bind"),
            ManifestRule::Malformed,
        ),
        (
            "host source",
            |m| m["services"][0]["mounts"][0]["source"] = json!("/etc"),
            ManifestRule::Malformed,
        ),
        (
            "missing key",
            |m| {
                m["services"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("read_only_rootfs");
            },
            ManifestRule::Malformed,
        ),
        (
            "caller volume",
            |m| m["services"][0]["mounts"][0]["volume"] = json!("arx-v_other_data"),
            ManifestRule::Mount,
        ),
        (
            "reserved target",
            |m| m["services"][0]["mounts"][0]["target"] = json!("/proc/self"),
            ManifestRule::Mount,
        ),
        (
            "relative target",
            |m| m["services"][0]["mounts"][0]["target"] = json!("../etc"),
            ManifestRule::Mount,
        ),
        (
            "interpolation",
            |m| m["services"][0]["environment"][0]["value"] = json!("${HOME}"),
            ManifestRule::Interpolation,
        ),
        (
            "image interpolation",
            |m| m["services"][0]["image"] = json!("${IMAGE}"),
            ManifestRule::Interpolation,
        ),
        (
            "tagged image",
            |m| m["services"][0]["image"] = json!("registry.example.test/calm/web:latest"),
            ManifestRule::Image,
        ),
        (
            "short digest",
            |m| m["services"][0]["image"] = json!("calm/web@sha256:abc"),
            ManifestRule::Image,
        ),
        (
            "platform",
            |m| m["services"][0]["platform"] = json!("linux/arm64"),
            ManifestRule::Platform,
        ),
        (
            "root uid",
            |m| m["services"][0]["uid"] = json!(0),
            ManifestRule::Identity,
        ),
        (
            "root gid",
            |m| m["services"][0]["gid"] = json!(0),
            ManifestRule::Identity,
        ),
        (
            "zero port",
            |m| m["services"][0]["http_port"] = json!(0),
            ManifestRule::Port,
        ),
        (
            "port without health",
            |m| m["services"][0]["health_path"] = Value::Null,
            ManifestRule::HealthPath,
        ),
        ("schema", |m| m["schema"] = json!(2), ManifestRule::Schema),
        (
            "env name",
            |m| m["services"][0]["environment"][0]["name"] = json!("port"),
            ManifestRule::Environment,
        ),
        (
            "undeclared slot",
            |m| m["services"][0]["secrets"] = json!(["other"]),
            ManifestRule::Secret,
        ),
        (
            "limits",
            |m| m["services"][0]["limits"]["cpu_millis"] = json!(0),
            ManifestRule::Limits,
        ),
        (
            "project id",
            |m| m["project_id"] = json!("Calm_Life"),
            ManifestRule::ProjectId,
        ),
        (
            "services",
            |m| {
                let service = m["services"][0].clone();
                m["services"] = json!(
                    (1..=5)
                        .map(|n| {
                            let mut copy = service.clone();
                            copy["name"] = json!(format!("s{n}"));
                            copy
                        })
                        .collect::<Vec<_>>()
                );
            },
            ManifestRule::ServiceCount,
        ),
        (
            "volumes",
            |m| {
                m["volumes"] = json!(
                    (1..=9)
                        .map(|n| json!({"name": format!("v{n}"), "uid": 1000, "gid": 1000}))
                        .collect::<Vec<_>>()
                );
            },
            ManifestRule::VolumeCount,
        ),
    ];
    for (name, edit, rule) in cases {
        let mut value = manifest();
        edit(&mut value);
        assert_eq!(Project::parse(&bytes(&value)), Err(rule), "{name}");
    }
    for text in [
        &b"services: {web: {image: x}}"[..],
        b"[]",
        b"",
        b"{\"schema\": 1}",
    ] {
        assert_eq!(Project::parse(text), Err(ManifestRule::Malformed));
    }
}

#[test]
fn staging_enforces_order_size_digest_and_session_reset() {
    let h = Harness::new();
    h.hello();
    let mut manifest_bytes = bytes(&manifest());
    manifest_bytes.resize(2 * STAGE_CHUNK + 1000, b' ');
    let digest = sha256_hex(&manifest_bytes);
    let chunk = |index: usize| {
        let end = ((index + 1) * STAGE_CHUNK).min(manifest_bytes.len());
        encode_base64(&manifest_bytes[index * STAGE_CHUNK..end])
    };
    let begin = |digest: &str, total: usize, chunks: u32| {
        h.svc.handle(ComposeRequest::ComposeStageBegin {
            id: 1,
            digest: digest.to_owned(),
            total_bytes: total as u32,
            chunks,
        })
    };
    let send = |digest: &str, index: u32, data: String| {
        h.svc.handle(ComposeRequest::ComposeStageChunk {
            id: 2,
            digest: digest.to_owned(),
            index,
            data,
        })
    };
    let commit = |digest: &str| {
        h.svc.handle(ComposeRequest::ComposeStageCommit {
            id: 3,
            digest: digest.to_owned(),
        })
    };
    let total = manifest_bytes.len();

    assert_eq!(rejected(&begin(&digest, 0, 0)), Some(RejectCode::StageSize));
    assert_eq!(
        rejected(&begin(&digest, STAGE_TOTAL + 1, 9)),
        Some(RejectCode::StageSize)
    );
    assert_eq!(
        rejected(&begin(&digest, total, 2)),
        Some(RejectCode::StageSize)
    );

    assert!(rejected(&begin(&digest, total, 3)).is_none());
    assert_eq!(
        rejected(&send(&digest, 1, chunk(1))),
        Some(RejectCode::StageOrder)
    );
    assert_eq!(
        rejected(&send(&digest, 0, chunk(0))),
        Some(RejectCode::NoStaging)
    );

    assert!(rejected(&begin(&digest, total, 3)).is_none());
    assert_eq!(
        rejected(&send(&digest, 0, encode_base64(&manifest_bytes[..100]))),
        Some(RejectCode::StageSize)
    );

    assert!(rejected(&begin(&digest, total, 3)).is_none());
    assert!(rejected(&send(&digest, 0, chunk(0))).is_none());
    assert_eq!(
        rejected(&commit(&digest)),
        Some(RejectCode::StageIncomplete)
    );
    assert!(rejected(&send(&digest, 1, chunk(1))).is_none());

    assert!(matches!(
        h.hello(),
        ComposeResponse::ComposeHelloAck {
            cleared_staging: 1,
            ..
        }
    ));
    assert_eq!(
        rejected(&send(&digest, 2, chunk(2))),
        Some(RejectCode::NoStaging)
    );

    let wrong = sha256_hex(b"something else");
    assert!(rejected(&begin(&wrong, total, 3)).is_none());
    for index in 0..3 {
        assert!(rejected(&send(&wrong, index, chunk(index as usize))).is_none());
    }
    assert_eq!(rejected(&commit(&wrong)), Some(RejectCode::StageDigest));

    assert!(rejected(&begin(&digest, total, 3)).is_none());
    for index in 0..3 {
        let progress = send(&digest, index, chunk(index as usize));
        let ComposeResponse::ComposeStageProgress {
            received_bytes,
            next_index,
            ..
        } = progress
        else {
            panic!("{progress:?}");
        };
        assert_eq!(next_index, index + 1);
        assert_eq!(
            received_bytes as usize,
            ((index as usize + 1) * STAGE_CHUNK).min(total)
        );
    }
    assert_eq!(
        commit(&digest),
        ComposeResponse::ComposeReleaseStaged {
            id: 3,
            digest: digest.clone(),
            project_id: PROJECT.to_owned(),
        }
    );
    assert!(matches!(
        begin(&digest, total, 3),
        ComposeResponse::ComposeStageProgress { next_index: 3, .. }
    ));

    let mut hostile = manifest();
    hostile["services"][0]["privileged"] = json!(true);
    let hostile = bytes(&hostile);
    let hostile_digest = sha256_hex(&hostile);
    assert!(rejected(&begin(&hostile_digest, hostile.len(), 1)).is_none());
    assert!(rejected(&send(&hostile_digest, 0, encode_base64(&hostile))).is_none());
    assert_eq!(
        commit(&hostile_digest),
        ComposeResponse::ComposeRejected {
            id: 3,
            code: RejectCode::ManifestRejected,
            rule: Some(ManifestRule::Malformed),
        }
    );
}

#[test]
fn deploy_pulls_by_digest_creates_volumes_then_up_and_health_before_promotion() {
    let h = Harness::new();
    h.hello();
    let digest = h.stage(&bytes(&manifest()));
    assert_eq!(
        rejected(&h.deploy(1, PROJECT, 1, &digest)),
        Some(RejectCode::SecretMissing),
        "missing secret delivery blocks deploy"
    );
    assert!(matches!(
        h.put_secret(90, &digest, 1, b"first value"),
        ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
    ));
    let reply = h.deploy(1, PROJECT, 1, &digest);
    let ComposeResponse::ComposeOperationState { id: 5, operation } = reply else {
        panic!("{reply:?}");
    };
    assert_eq!(operation.state, OperationState::Running);
    assert_eq!(operation.stage, OperationStage::Queued);
    h.settle();
    let done = h.operation(1);
    assert_eq!(
        (done.state, done.stage, done.failure),
        (OperationState::Succeeded, OperationStage::Settled, None)
    );

    let global = |tail: &[&str]| -> Vec<String> {
        ["--config", "/var/lib/arx-compose/docker-config"]
            .iter()
            .chain(tail)
            .map(|arg| (*arg).to_owned())
            .collect()
    };
    let volume = "arx-v_calm-life_data";
    let img = image();
    let cid = h.docker.lock().unwrap().containers[PROJECT][0].id.clone();
    assert_eq!(
        h.calls(),
        vec![
            global(&[
                "image",
                "inspect",
                "--format",
                "{{.Os}}/{{.Architecture}}",
                img.as_str()
            ]),
            global(&[
                "image",
                "pull",
                "--platform",
                "linux/amd64",
                "--quiet",
                img.as_str()
            ]),
            global(&[
                "image",
                "inspect",
                "--format",
                "{{.Os}}/{{.Architecture}}",
                img.as_str()
            ]),
            global(&["volume", "ls", "--format", "json"]),
            global(&[
                "volume",
                "create",
                "--driver",
                "local",
                "--label",
                "org.arxcovo.volume=calm-life/data/1000:1000",
                volume,
            ]),
            global(&["volume", "ls", "--format", "json"]),
            global(&["volume", "inspect", "--format", "{{.Mountpoint}}", volume]),
            compose_argv(1, &["up", "--detach", "--no-build", "--pull", "never"]),
            compose_argv(1, &["ps", "--all", "--format", "json"]),
            global(&[
                "container",
                "inspect",
                "--format",
                "{{.Id}}\n{{json .Mounts}}",
                cid.as_str(),
            ]),
        ]
    );
    assert_eq!(
        h.docker.lock().unwrap().probes,
        vec![(20000, "/healthz".to_owned())]
    );
    let expected_instance = secret_instance(SLOT, &digest, 1, GENERATION);
    {
        let disk = h.disk.lock().unwrap();
        assert_eq!(
            disk.revisions[&(PROJECT.to_owned(), 1)],
            golden_yaml_for(&expected_instance)
        );
        assert_eq!(disk.prepared[volume], (1000, 1000));
    }
    let status = h.status(PROJECT);
    assert_eq!(status.desired, Desired::Started);
    assert_eq!(status.committed_revision, Some(1));
    assert_eq!(status.observation, Observation::Live);
    assert_eq!(
        status.services,
        vec![ServiceView {
            name: "web".to_owned(),
            state: ServiceState::Running,
            health: HealthState::None,
            revision: Some(1),
        }]
    );
    let restarted = Harness::on(Arc::clone(&h.disk));
    let status = restarted.status(PROJECT);
    assert_eq!(
        (status.desired, status.committed_revision),
        (Desired::Started, Some(1))
    );
}

#[test]
fn engine_argv_is_fixed_and_environment_is_closed() {
    assert_eq!(
        ENGINE_ENV.map(|(name, _)| name),
        ["HOME", "LANG", "PATH"],
        "the engine environment is exactly these names"
    );
    assert_eq!(ENGINE_CWD, "/var/lib/arx-compose/empty");
    assert_eq!(DOCKER, "/usr/bin/docker");
    assert_eq!(
        compose_down(PROJECT, 3).argv,
        compose_argv(3, &["down"]),
        "down carries no volume, image or orphan flags"
    );
    assert_eq!(compose_stop(PROJECT, 3).argv, compose_argv(3, &["stop"]));
    let images = [image()];
    let rot_file = rotation_compose_file(PROJECT, 3, &op(91));
    for invocation in [
        image_inspect(&images[0]),
        image_pull(&images[0]),
        volume_list(),
        volume_label("arx-v_calm-life_data"),
        volume_mountpoint("arx-v_calm-life_data"),
        volume_create("arx-v_calm-life_data", "calm-life/data/1000:1000"),
        compose_up(PROJECT, 3),
        compose_recreate(PROJECT, 3, &["web".to_owned()]),
        compose_recreate_file(PROJECT, 3, &rot_file, &["web".to_owned()]),
        compose_stop(PROJECT, 3),
        compose_down(PROJECT, 3),
        container_stop("000000000001"),
        container_rm("000000000001"),
        container_inspect_mounts("000000000001"),
        network_list(),
        network_rm("a00000000001"),
        compose_ps(PROJECT, 3),
    ] {
        assert_eq!(&invocation.argv[..2], ["--config", DOCKER_CONFIG]);
        for flag in [
            "--volumes",
            "-v",
            "--rmi",
            "--remove-orphans",
            "--build",
            "--host",
            "-H",
            "--context",
            "--privileged",
            "--force",
            "-f",
        ] {
            assert!(!invocation.argv.iter().any(|arg| arg == flag), "{flag}");
        }
        if invocation.argv[2] == "compose" {
            assert!(
                invocation
                    .argv
                    .windows(2)
                    .any(|pair| pair == ["--env-file", "/dev/null"]),
                "dotenv is disabled"
            );
        }
    }
}

#[test]
fn operation_ids_never_repeat_an_effect() {
    let h = Harness::new();
    let digest = h.deployed();
    let before = h.calls().len();
    assert_eq!(
        h.deploy(1, PROJECT, 1, &digest),
        ComposeResponse::ComposeOperationState {
            id: 5,
            operation: h.operation(1),
        }
    );
    assert_eq!(
        h.calls().len(),
        before,
        "a known operation ID re-ran an effect"
    );
    assert_eq!(
        rejected(&h.deploy(1, PROJECT, 2, &digest)),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(
        rejected(&h.lifecycle(1, PROJECT, OperationKind::Stop)),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(
        rejected(&h.deploy(2, PROJECT, 1, &digest)),
        Some(RejectCode::RevisionNotNewer)
    );
    assert_eq!(
        rejected(&h.deploy(2, PROJECT, 2, &"0".repeat(64))),
        Some(RejectCode::UnknownRelease)
    );
    assert_eq!(
        rejected(&h.deploy(2, "other", 2, &digest)),
        Some(RejectCode::ProjectMismatch)
    );
}

#[test]
fn ambiguous_apply_is_reconciled_from_observed_state_never_rerun() {
    for applied in [true, false] {
        let h = Harness::new();
        h.hello();
        let digest = h.stage(&bytes(&manifest()));
        h.put_secret(90, &digest, 1, b"first value");
        h.docker.lock().unwrap().faults.push(Fault {
            needle: " up --detach",
            result: RunResult::TimedOut,
            apply: applied,
        });
        h.deploy(1, PROJECT, 1, &digest);
        h.settle();
        assert_eq!(h.operation(1).state, OperationState::Ambiguous);
        assert_eq!(
            rejected(&h.deploy(2, PROJECT, 2, &digest)),
            Some(RejectCode::ReconcileRequired)
        );
        assert_eq!(
            rejected(&h.lifecycle(3, PROJECT, OperationKind::Down)),
            Some(RejectCode::ReconcileRequired)
        );
        assert_eq!(h.count(" up --detach"), 1);

        let before = h.calls().len();
        let reply = h.reconcile(1);
        let ComposeResponse::ComposeOperationState { operation, .. } = reply else {
            panic!("{reply:?}");
        };
        let after = h.calls();
        assert_eq!(
            after[before],
            compose_argv(1, &["ps", "--all", "--format", "json"])
        );
        assert!(
            after[before..].iter().all(|argv| {
                let joined = argv.join(" ");
                joined.contains("ps --all") || joined.contains("container inspect")
            }),
            "reconcile must issue only read-only queries: {:?}",
            &after[before..]
        );
        assert_eq!(h.count(" up --detach"), 1, "reconcile re-ran apply");
        let status = h.status(PROJECT);
        if applied {
            assert_eq!(operation.state, OperationState::Succeeded);
            assert_eq!(status.committed_revision, Some(1));
            assert_eq!(status.desired, Desired::Started);
        } else {
            assert_eq!(
                (operation.state, operation.failure),
                (OperationState::Failed, Some(FailureCode::NotApplied))
            );
            assert_eq!(status.committed_revision, None);
            assert!(!status.recovery_required);
        }
        assert_eq!(rejected(&h.reconcile(1)), Some(RejectCode::NotAmbiguous));
    }
}

#[test]
fn running_journal_is_persisted_before_effects_and_becomes_ambiguous_after_restart() {
    let h = Harness::new();
    h.hello();
    let digest = h.stage(&bytes(&manifest()));
    h.put_secret(90, &digest, 1, b"first value");
    let (release, receiver) = mpsc::channel();
    h.docker.lock().unwrap().block = Some((" up --detach", receiver));
    h.deploy(1, PROJECT, 1, &digest);
    let mut waited = 0;
    while h.operation(1).stage != OperationStage::Applying {
        assert!(waited < 500, "worker never reached apply");
        std::thread::sleep(Duration::from_millis(10));
        waited += 1;
    }
    let status = h.status(PROJECT);
    assert_eq!(status.running_operation, Some(op(1)));
    assert_eq!(status.observation, Observation::Recorded);
    assert_eq!(
        rejected(&h.lifecycle(2, PROJECT, OperationKind::Stop)),
        Some(RejectCode::Busy)
    );
    let snapshot = h.disk.lock().unwrap().state.clone();

    let restarted = Harness::on(Arc::new(Mutex::new(Disk {
        state: snapshot,
        ..Disk::default()
    })));
    assert!(matches!(
        restarted.hello(),
        ComposeResponse::ComposeHelloAck {
            ambiguous_operations: 1,
            ..
        }
    ));
    assert_eq!(restarted.operation(1).state, OperationState::Ambiguous);

    release.send(()).unwrap();
    h.settle();
    assert_eq!(h.operation(1).state, OperationState::Succeeded);
}

#[test]
fn failed_health_restores_a_compatible_previous_revision() {
    let h = Harness::new();
    h.deployed();
    let mut next = manifest();
    next["services"][0]["environment"][0]["value"] = json!("8081");
    let digest = h.stage(&bytes(&next));
    assert!(matches!(
        h.put_secret(91, &digest, 1, b"second release secret"),
        ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
    ));
    h.docker.lock().unwrap().unhealthy.insert(2);
    h.deploy(2, PROJECT, 2, &digest);
    h.settle();
    let failed = h.operation(2);
    assert_eq!(
        (failed.state, failed.failure, failed.restored_previous),
        (
            OperationState::Failed,
            Some(FailureCode::HealthFailed),
            true
        )
    );
    let calls = h.calls();
    let up2 = compose_argv(2, &["up", "--detach", "--no-build", "--pull", "never"]);
    let up1 = compose_argv(1, &["up", "--detach", "--no-build", "--pull", "never"]);
    let second = calls.iter().position(|argv| *argv == up2).unwrap();
    assert!(
        calls[second..].contains(&up1),
        "previous revision not restored"
    );
    let status = h.status(PROJECT);
    assert_eq!(status.committed_revision, Some(1));
    assert!(!status.recovery_required);
    assert_eq!(status.services[0].revision, Some(1));

    // Without a previous revision the failed one is stopped instead.
    let fresh = Harness::new();
    fresh.docker.lock().unwrap().unhealthy.insert(1);
    fresh.hello();
    let digest = fresh.stage(&bytes(&manifest()));
    fresh.put_secret(90, &digest, 1, b"first value");
    fresh.deploy(1, PROJECT, 1, &digest);
    fresh.settle();
    let failed = fresh.operation(1);
    assert_eq!(
        (failed.failure, failed.restored_previous),
        (Some(FailureCode::HealthFailed), false)
    );
    assert_eq!(fresh.count("/dev/null stop"), 1);
    assert_eq!(fresh.status(PROJECT).committed_revision, None);
}

#[test]
fn engine_failures_settle_without_apply() {
    type Setup = fn(&mut Docker);
    let cases: [(Setup, FailureCode); 3] = [
        (|docker| docker.pull_fails = true, FailureCode::PullFailed),
        (
            |docker| {
                docker.images.insert(image());
                docker.arch = Some("linux/arm64");
            },
            FailureCode::ImageIdentity,
        ),
        (
            |docker| {
                docker
                    .volumes
                    .insert("arx-v_calm-life_data".to_owned(), String::new());
            },
            FailureCode::VolumeCollision,
        ),
    ];
    for (setup, failure) in cases {
        let h = Harness::new();
        setup(&mut h.docker.lock().unwrap());
        h.hello();
        let digest = h.stage(&bytes(&manifest()));
        h.put_secret(90, &digest, 1, b"first value");
        h.deploy(1, PROJECT, 1, &digest);
        h.settle();
        assert_eq!(h.operation(1).failure, Some(failure));
        assert_eq!(h.count(" up --detach"), 0);
        assert_eq!(h.count("volume create"), 0);
    }
}

#[test]
fn per_vm_project_and_volume_limits() {
    let h = Harness::new();
    h.hello();
    for n in 0..MAX_PROJECTS as u32 {
        let id = format!("p{n}");
        h.docker
            .lock()
            .unwrap()
            .services
            .insert(id.clone(), vec!["app".to_owned()]);
        let digest = h.stage(&bytes(&plain(&id, &["data"])));
        h.deploy(100 + n, &id, 1, &digest);
        h.settle();
        assert_eq!(
            h.operation(100 + n).state,
            OperationState::Succeeded,
            "{id}"
        );
    }
    let digest = h.stage(&bytes(&plain("p8", &["data"])));
    assert_eq!(
        rejected(&h.deploy(200, "p8", 1, &digest)),
        Some(RejectCode::ProjectLimit)
    );
    let digest = h.stage(&bytes(&plain("p0", &["data", "extra"])));
    assert_eq!(
        rejected(&h.deploy(201, "p0", 2, &digest)),
        Some(RejectCode::VolumeLimit)
    );
}

#[test]
fn logs_are_typed_bounded_and_paged() {
    let h = Harness::new();
    h.deployed();
    for cycle in 0..6 {
        h.lifecycle(10 + 2 * cycle, PROJECT, OperationKind::Stop);
        h.settle();
        h.lifecycle(11 + 2 * cycle, PROJECT, OperationKind::Start);
        h.settle();
        assert_eq!(h.operation(11 + 2 * cycle).state, OperationState::Succeeded);
    }
    let page = |cursor: u64| match h.svc.handle(ComposeRequest::ComposeLogs {
        id: 12,
        project_id: PROJECT.to_owned(),
        cursor,
        limit: MAX_LOG_ENTRIES,
    }) {
        reply @ ComposeResponse::ComposeLogPage { .. } => {
            assert!(serde_json::to_vec(&reply).unwrap().len() <= MAX_LOG_PAGE);
            let ComposeResponse::ComposeLogPage {
                entries,
                next_cursor,
                ..
            } = reply
            else {
                unreachable!()
            };
            (entries, next_cursor)
        }
        other => panic!("{other:?}"),
    };
    let (first, next) = page(0);
    assert_eq!(first.len(), MAX_LOG_ENTRIES as usize);
    assert_eq!(first[0].event, LogEvent::SecretDelivered);
    let (second, after) = page(next);
    assert!(!second.is_empty());
    assert_eq!(second[0].seq, next);
    assert!(second.windows(2).all(|pair| pair[0].seq < pair[1].seq));
    let (third, last) = page(after);
    assert!(third.is_empty());
    assert_eq!(last, after);
    let (other, cursor) = match h.svc.handle(ComposeRequest::ComposeLogs {
        id: 13,
        project_id: "other".to_owned(),
        cursor: 0,
        limit: 1,
    }) {
        ComposeResponse::ComposeLogPage {
            entries,
            next_cursor,
            ..
        } => (entries, next_cursor),
        reply => panic!("{reply:?}"),
    };
    assert!(other.is_empty());
    assert_eq!(cursor, 0);
}

#[test]
fn replies_carry_no_paths_or_urls() {
    let h = Harness::new();
    let mut replies = vec![h.hello()];
    let digest = h.stage(&bytes(&manifest()));
    replies.push(h.put_secret(90, &digest, 1, b"first value"));
    replies.push(h.deploy(1, PROJECT, 1, &digest));
    h.settle();
    replies.push(h.svc.handle(ComposeRequest::ComposeOperation {
        id: 9,
        operation_id: op(1),
    }));
    replies.push(h.svc.handle(ComposeRequest::ComposeStatus {
        id: 10,
        project_id: PROJECT.to_owned(),
    }));
    replies.push(h.svc.handle(ComposeRequest::ComposeLogs {
        id: 11,
        project_id: PROJECT.to_owned(),
        cursor: 0,
        limit: MAX_LOG_ENTRIES,
    }));
    replies.push(h.lifecycle(2, PROJECT, OperationKind::Down));
    h.settle();
    for reply in replies {
        let text = serde_json::to_string(&reply).unwrap();
        assert!(!text.contains('/') && !text.contains('\\'), "{text}");
    }
}

#[test]
fn ps_parsing_keeps_all_attributed_services_of_the_project() {
    let line = |service: &str, project: Option<&str>, state: &str| {
        let labels = match project {
            Some(project) => format!("org.arxcovo.project={project},org.arxcovo.revision=4"),
            None => String::new(),
        };
        json!({
            "ID": "000000000001",
            "Service": service,
            "State": state,
            "Health": "healthy",
            "Labels": labels,
        })
        .to_string()
    };
    let ndjson = [
        line("web", Some(PROJECT), "running"),
        line("web", Some("other"), "running"),
        line("retired", Some(PROJECT), "running"),
        line("foreign", None, "running"),
    ]
    .join("\n");
    let views = parse_ps(ndjson.as_bytes(), PROJECT).unwrap();
    assert_eq!(
        views,
        vec![
            ServiceView {
                name: "retired".to_owned(),
                state: ServiceState::Running,
                health: HealthState::Healthy,
                revision: Some(4),
            },
            ServiceView {
                name: "web".to_owned(),
                state: ServiceState::Running,
                health: HealthState::Healthy,
                revision: Some(4),
            },
        ]
    );
    let array = format!(
        "[{},{}]",
        line("web", Some(PROJECT), "exited"),
        line("web", Some(PROJECT), "running")
    );
    let views = parse_ps(array.as_bytes(), PROJECT).unwrap();
    assert_eq!(
        views[0].state,
        ServiceState::Unknown,
        "duplicate containers"
    );
    assert_eq!(parse_ps(b"", PROJECT), Some(Vec::new()));
    assert_eq!(parse_ps(b"not json", PROJECT), None);
}

#[test]
fn base64_is_strict_and_canonical() {
    for len in [1, 2, 3, 4, 5, 16, 255, STAGE_CHUNK] {
        let data: Vec<u8> = (0..len).map(|n| (n * 37 % 251) as u8).collect();
        let text = encode_base64(&data);
        assert_eq!(text.len(), base64_len(len));
        assert_eq!(decode_base64(&text, STAGE_CHUNK), Some(data));
    }
    assert_eq!(decode_base64("QQ==", 4), Some(b"A".to_vec()));
    for bad in [
        "", "QQ=", "QR==", "Q===", "====", "QQ==QQ==", "QQ\n=", "Q Q=", "QUJD-A==",
    ] {
        assert_eq!(decode_base64(bad, 16), None, "{bad:?}");
    }
    assert_eq!(
        decode_base64(&encode_base64(&[0; 5]), 4),
        None,
        "over the bound"
    );
}

#[test]
fn cached_staging_enforces_exact_byte_count() {
    let h = Harness::new();
    h.hello();
    let input = bytes(&manifest());
    let digest = h.stage(&input);
    for wrong in [input.len() as u32 + 1, input.len() as u32 - 1] {
        let reply = h.svc.handle(ComposeRequest::ComposeStageBegin {
            id: 8,
            digest: digest.clone(),
            total_bytes: wrong,
            chunks: 1,
        });
        assert_eq!(rejected(&reply), Some(RejectCode::StageSize));
    }
    let exact = h.svc.handle(ComposeRequest::ComposeStageBegin {
        id: 8,
        digest,
        total_bytes: input.len() as u32,
        chunks: 1,
    });
    assert!(matches!(
        exact,
        ComposeResponse::ComposeStageProgress {
            received_bytes,
            next_index: 1,
            ..
        } if received_bytes == input.len() as u32
    ));
}

// =========================================================================
// Contract Row A: Durable delivery receipt
// =========================================================================

#[test]
fn contract_a_durable_delivery_receipt() {
    // 1. Cut intent/effect-started save BeforeRename: no tmpfs write, no acceptance.
    let h = Harness::new();
    h.hello();
    let digest = h.stage(&bytes(&manifest()));
    h.disk
        .lock()
        .unwrap()
        .save_faults
        .push_back(Some(StoreFault::BeforeRename));
    assert_eq!(
        rejected(&h.put_secret(10, &digest, 1, b"alpha")),
        Some(RejectCode::StoreFailure)
    );
    assert_eq!(h.disk.lock().unwrap().secret_writes, 0);
    assert!(h.disk.lock().unwrap().secrets.is_empty());

    // 2. Cut intent/effect-started save AfterRename: disk has EffectStarted (no file);
    // reload marks it Ambiguous; exact-ID retry & reconcile settle Failed(NotApplied)
    // with zero writes, never returning ComposeSecretAccepted.
    let h = Harness::new();
    h.hello();
    let digest = h.stage(&bytes(&manifest()));
    h.disk
        .lock()
        .unwrap()
        .save_faults
        .push_back(Some(StoreFault::AfterRename));
    assert_eq!(
        rejected(&h.put_secret(11, &digest, 1, b"alpha")),
        Some(RejectCode::StoreFailure)
    );
    assert_eq!(h.disk.lock().unwrap().secret_writes, 0);
    let reloaded = Harness::on(Arc::clone(&h.disk));
    assert!(matches!(
        reloaded.hello(),
        ComposeResponse::ComposeHelloAck {
            ambiguous_operations: 1,
            ..
        }
    ));
    assert_eq!(reloaded.operation(11).state, OperationState::Ambiguous);
    // Exact-ID retry sees missing file -> settles Failed(NotApplied), 0 writes.
    let retry = reloaded.put_secret(11, &digest, 1, b"alpha");
    assert!(
        matches!(
            retry,
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Failed,
                    failure: Some(FailureCode::NotApplied),
                    ..
                },
                ..
            }
        ),
        "missing file must never return ComposeSecretAccepted: {retry:?}"
    );
    assert_eq!(reloaded.disk.lock().unwrap().secret_writes, 0);

    // 3. Cut tmpfs publish BeforeRename vs AfterRename, then reload with missing,
    // matching, and wrong file on tmpfs.
    let instance = secret_instance(SLOT, &digest, 1, GENERATION);
    for fault in [StoreFault::BeforeRename, StoreFault::AfterRename] {
        let h = Harness::new();
        h.hello();
        let d = h.stage(&bytes(&manifest()));
        h.disk.lock().unwrap().write_secret_faults.push_back(fault);
        let reply = h.put_secret(12, &d, 1, b"alpha");
        assert!(
            matches!(
                reply,
                ComposeResponse::ComposeOperationState {
                    operation: OperationView {
                        state: OperationState::Ambiguous,
                        ..
                    },
                    ..
                }
            ),
            "write_secret fault {fault:?} must return Ambiguous, not Accepted: {reply:?}"
        );
        let state_snap = h.disk.lock().unwrap().state.clone();

        // Reload A: missing file -> reconcile settles Failed(NotApplied) with 0 writes.
        let disk_missing = Arc::new(Mutex::new(Disk {
            state: state_snap.clone(),
            ..Disk::default()
        }));
        let r_missing = Harness::on(Arc::clone(&disk_missing));
        r_missing.hello();
        let rec = r_missing.reconcile(12);
        assert!(matches!(
            rec,
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Failed,
                    failure: Some(FailureCode::NotApplied),
                    ..
                },
                ..
            }
        ));
        assert_eq!(disk_missing.lock().unwrap().secret_writes, 0);

        // Reload B: wrong file at bound path -> reconcile keeps Ambiguous (no data),
        // exact-ID retry sees Mismatch -> settles Failed(NotApplied) with 0 writes.
        let mut wrong_disk = Disk {
            state: state_snap.clone(),
            ..Disk::default()
        };
        let w = wrong_disk.next_witness((1000, 1000));
        wrong_disk.secrets.insert(
            (PROJECT.to_owned(), instance.clone()),
            (b"WRONG".to_vec(), w),
        );
        let disk_wrong = Arc::new(Mutex::new(wrong_disk));
        let r_wrong = Harness::on(Arc::clone(&disk_wrong));
        r_wrong.hello();
        let rec_wrong = r_wrong.reconcile(12);
        assert!(matches!(
            rec_wrong,
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Ambiguous,
                    ..
                },
                ..
            }
        ));
        let retry_wrong = r_wrong.put_secret(12, &d, 1, b"alpha");
        assert!(matches!(
            retry_wrong,
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Failed,
                    failure: Some(FailureCode::NotApplied),
                    ..
                },
                ..
            }
        ));
        assert_eq!(disk_wrong.lock().unwrap().secret_writes, 0);

        // Reload C: matching file at bound path -> exact-ID retry settles once with
        // zero additional writes, and subsequent replay adds zero writes.
        let mut match_disk = Disk {
            state: state_snap,
            ..Disk::default()
        };
        let w = match_disk.next_witness((1000, 1000));
        match_disk.secrets.insert(
            (PROJECT.to_owned(), instance.clone()),
            (b"alpha".to_vec(), w),
        );
        let disk_match = Arc::new(Mutex::new(match_disk));
        let r_match = Harness::on(Arc::clone(&disk_match));
        r_match.hello();
        let accepted = r_match.put_secret(12, &d, 1, b"alpha");
        assert!(matches!(
            accepted,
            ComposeResponse::ComposeSecretAccepted {
                epoch: 1,
                recreate: None,
                ..
            }
        ));
        assert_eq!(disk_match.lock().unwrap().secret_writes, 0);
        assert_eq!(r_match.operation(12).state, OperationState::Succeeded);
        // Replay adds zero writes; tampering conflicts.
        assert!(matches!(
            r_match.put_secret(12, &d, 1, b"alpha"),
            ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
        ));
        assert_eq!(
            rejected(&r_match.put_secret(12, &d, 1, b"tampered")),
            Some(RejectCode::OperationConflict)
        );
        assert_eq!(disk_match.lock().unwrap().secret_writes, 0);
    }

    // 4. Cut final settlement save BeforeRename and AfterRename + lost-reply replay.
    for fault in [StoreFault::BeforeRename, StoreFault::AfterRename] {
        let h = Harness::new();
        h.hello();
        let d = h.stage(&bytes(&manifest()));
        {
            let mut disk = h.disk.lock().unwrap();
            // Save #1 (EffectStarted) succeeds; Save #2 (final Settled) fails.
            disk.save_faults.push_back(None);
            disk.save_faults.push_back(Some(fault));
        }
        let reply = h.put_secret(15, &d, 1, b"alpha");
        assert!(
            matches!(
                reply,
                ComposeResponse::ComposeOperationState {
                    operation: OperationView {
                        state: OperationState::Ambiguous,
                        ..
                    },
                    ..
                }
            ),
            "final save failure ({fault:?}) must never return ComposeSecretAccepted: {reply:?}"
        );
        assert_eq!(h.disk.lock().unwrap().secret_writes, 1);
        // Dependent deploy must stay blocked while final save outcome is uncertain.
        assert_eq!(
            rejected(&h.deploy(16, PROJECT, 1, &d)),
            Some(RejectCode::ReconcileRequired)
        );
        // Reload from exact persisted state + tmpfs:
        let reloaded = Harness::on(Arc::clone(&h.disk));
        reloaded.hello();
        if fault == StoreFault::AfterRename {
            assert_eq!(reloaded.operation(15).state, OperationState::Succeeded);
        } else {
            assert_eq!(reloaded.operation(15).state, OperationState::Ambiguous);
        }
        // Exact-ID replay returns Accepted with zero extra tmpfs writes in both cases.
        assert!(matches!(
            reloaded.put_secret(15, &d, 1, b"alpha"),
            ComposeResponse::ComposeSecretAccepted {
                epoch: 1,
                recreate: None,
                ..
            }
        ));
        assert_eq!(reloaded.disk.lock().unwrap().secret_writes, 1);
    }

    // 5. Tmpfs reset (reboot): historical success cannot satisfy Start;
    // current-generation redelivery restores readiness and Start mounts the new path.
    let h = Harness::new();
    let digest = h.deployed();
    h.lifecycle(20, PROJECT, OperationKind::Stop);
    h.settle();
    assert_eq!(h.operation(20).state, OperationState::Succeeded);
    // Simulate reboot: tmpfs wiped, durable disk state intact.
    h.disk.lock().unwrap().secrets.clear();
    let rebooted = Harness::on_shared(Arc::clone(&h.disk), Arc::clone(&h.docker));
    rebooted.hello_gen(GENERATION + 1);
    assert_eq!(
        rejected(&rebooted.lifecycle(21, PROJECT, OperationKind::Start)),
        Some(RejectCode::SecretMissing),
        "historical delivery must not satisfy Start after tmpfs reset"
    );
    // Current-generation redelivery of epoch 1 with fresh op ID restores readiness.
    let redelivered = rebooted.put_secret_as(
        22,
        GENERATION + 1,
        PROJECT,
        &digest,
        SLOT,
        1,
        b"first value",
    );
    assert!(matches!(
        redelivered,
        ComposeResponse::ComposeSecretAccepted {
            epoch: 1,
            recreate: None,
            ..
        }
    ));
    rebooted.lifecycle(23, PROJECT, OperationKind::Start);
    rebooted.settle();
    assert_eq!(rebooted.operation(23).state, OperationState::Succeeded);
    let expected_new_src = secret_file(PROJECT, &secret_instance(SLOT, &digest, 1, GENERATION + 1));
    let mounted_src = rebooted.docker.lock().unwrap().containers[PROJECT][0].mounts[0]
        .source
        .clone();
    assert_eq!(mounted_src, expected_new_src);
}

// =========================================================================
// Contract Row B: Epoch immutability & busy-work pinning
// =========================================================================

#[test]
fn contract_b_epoch_immutability() {
    let h = Harness::new();
    let digest = h.deployed();
    let instance_e1 = secret_instance(SLOT, &digest, 1, GENERATION);
    let witness_before =
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), instance_e1.clone())].1;
    let writes_before = h.disk.lock().unwrap().secret_writes;
    assert_eq!(writes_before, 1);

    // 1. Fresh ID with identical data at same epoch -> accepted, 0 writes, 0 recreates, same inode.
    assert!(matches!(
        h.put_secret(101, &digest, 1, b"first value"),
        ComposeResponse::ComposeSecretAccepted {
            epoch: 1,
            recreate: None,
            ..
        }
    ));
    assert_eq!(h.disk.lock().unwrap().secret_writes, writes_before);
    assert_eq!(h.count("--force-recreate"), 0);
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), instance_e1.clone())].1,
        witness_before
    );

    // 2. Fresh ID with DIFFERENT data at same epoch -> SecretConflict, 0 writes, 0 recreates, same inode.
    assert_eq!(
        rejected(&h.put_secret(102, &digest, 1, b"mutated value")),
        Some(RejectCode::SecretConflict)
    );
    assert_eq!(h.disk.lock().unwrap().secret_writes, writes_before);
    assert_eq!(h.count("--force-recreate"), 0);
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), instance_e1.clone())],
        (b"first value".to_vec(), witness_before)
    );

    // 3. Lower epoch after rotation -> StaleEpoch, 0 writes, 0 recreates.
    assert!(matches!(
        h.put_secret(103, &digest, 2, b"second value"),
        ComposeResponse::ComposeSecretAccepted {
            epoch: 2,
            recreate: Some(_),
            ..
        }
    ));
    h.settle();
    assert_eq!(h.operation(103).state, OperationState::Succeeded);
    let writes_after_e2 = h.disk.lock().unwrap().secret_writes;
    let recreates_after_e2 = h.count("--force-recreate");
    assert_eq!(
        rejected(&h.put_secret(104, &digest, 1, b"first value")),
        Some(RejectCode::StaleEpoch)
    );
    assert_eq!(
        rejected(&h.put_secret_as(105, GENERATION + 1, PROJECT, &digest, SLOT, 3, b"v")),
        Some(RejectCode::GenerationMismatch)
    );
    assert_eq!(
        rejected(&h.put_secret_as(106, GENERATION, PROJECT, &digest, "other", 3, b"v")),
        Some(RejectCode::UnknownSlot)
    );
    assert_eq!(
        rejected(&h.put_secret(107, &digest, 3, &[7; MAX_SECRET + 1])),
        Some(RejectCode::SecretSize)
    );
    assert_eq!(h.disk.lock().unwrap().secret_writes, writes_after_e2);
    assert_eq!(h.count("--force-recreate"), recreates_after_e2);

    // 4. While a Deploy is paused, new mutating SecretPut is rejected Busy,
    // identical settled replay has 0 effects, conflicting replay fails OperationConflict,
    // and the in-flight Deploy finishes with its pinned file intact.
    let mut rev2 = manifest();
    rev2["services"][0]["environment"][0]["value"] = json!("8082");
    let digest2 = h.stage(&bytes(&rev2));
    assert!(matches!(
        h.put_secret(110, &digest2, 1, b"rev2 pinned"),
        ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
    ));
    let instance_rev2 = secret_instance(SLOT, &digest2, 1, GENERATION);
    let witness_rev2 =
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), instance_rev2.clone())].1;
    let writes_before_deploy = h.disk.lock().unwrap().secret_writes;

    let (release, rx) = mpsc::channel();
    h.docker.lock().unwrap().block = Some((" up --detach", rx));
    h.deploy(111, PROJECT, 2, &digest2);
    let mut waited = 0;
    while h.operation(111).stage != OperationStage::Applying {
        assert!(waited < 500, "deploy worker did not reach apply");
        std::thread::sleep(Duration::from_millis(10));
        waited += 1;
    }

    // New mutating SecretPut while Deploy runs -> Busy!
    assert_eq!(
        rejected(&h.put_secret(112, &digest2, 2, b"sneaky overwrite")),
        Some(RejectCode::Busy)
    );
    // Identical settled replay of op 110 -> accepted with 0 writes.
    assert!(matches!(
        h.put_secret(110, &digest2, 1, b"rev2 pinned"),
        ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
    ));
    // Conflicting replay of op 110 -> OperationConflict with 0 writes.
    assert_eq!(
        rejected(&h.put_secret(110, &digest2, 1, b"tampered")),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(h.disk.lock().unwrap().secret_writes, writes_before_deploy);
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), instance_rev2.clone())],
        (b"rev2 pinned".to_vec(), witness_rev2)
    );

    release.send(()).unwrap();
    h.settle();
    assert_eq!(h.operation(111).state, OperationState::Succeeded);
    assert_eq!(
        h.docker.lock().unwrap().containers[PROJECT][0].mounts[0].source,
        secret_file(PROJECT, &instance_rev2)
    );

    // 5. While a Recreate is paused, new mutating SecretPut is rejected Busy and
    // conflicting replay fails OperationConflict while in-flight recreate keeps its pinned file.
    let (release_rot, rx_rot) = mpsc::channel();
    h.docker.lock().unwrap().block = Some(("--force-recreate", rx_rot));
    assert!(matches!(
        h.put_secret(120, &digest2, 2, b"rev2 rotated"),
        ComposeResponse::ComposeSecretAccepted {
            epoch: 2,
            recreate: Some(_),
            ..
        }
    ));
    let instance_rev2_e2 = secret_instance(SLOT, &digest2, 2, GENERATION);
    let mut waited = 0;
    while h.count("--force-recreate") <= recreates_after_e2 {
        assert!(
            waited < 500,
            "recreate worker did not reach --force-recreate"
        );
        std::thread::sleep(Duration::from_millis(10));
        waited += 1;
    }
    assert_eq!(
        rejected(&h.put_secret(121, &digest2, 3, b"concurrent third")),
        Some(RejectCode::Busy)
    );
    assert_eq!(
        rejected(&h.put_secret(120, &digest2, 2, b"wrong bytes")),
        Some(RejectCode::OperationConflict)
    );
    release_rot.send(()).unwrap();
    h.settle();
    assert_eq!(h.operation(120).state, OperationState::Succeeded);
    assert_eq!(
        h.docker.lock().unwrap().containers[PROJECT][0].mounts[0].source,
        secret_file(PROJECT, &instance_rev2_e2)
    );
}

// =========================================================================
// Contract Row C: Rotation evidence (fresh baseline + exact mount witness)
// =========================================================================

#[test]
fn contract_c_rotation_evidence() {
    let setup_multi = || {
        let h = Harness::new();
        h.docker.lock().unwrap().services.insert(
            PROJECT.to_owned(),
            vec!["web".to_owned(), "api".to_owned(), "worker".to_owned()],
        );
        h.hello();
        let digest = h.stage(&bytes(&multi_service_secret_manifest()));
        assert!(matches!(
            h.put_secret(1, &digest, 1, b"epoch-1"),
            ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
        ));
        h.deploy(2, PROJECT, 1, &digest);
        h.settle();
        assert_eq!(h.operation(2).state, OperationState::Succeeded);
        (h, digest)
    };

    // 1. Failed Start changes container IDs (leaving cached observed_ids stale),
    // then fail the fresh baseline `ps` -> MUST fail with 0 secret writes & 0 recreates.
    {
        let (h, digest) = setup_multi();
        h.docker.lock().unwrap().unhealthy.insert(1);
        h.lifecycle(3, PROJECT, OperationKind::Start);
        h.settle();
        assert_eq!(h.operation(3).state, OperationState::Failed);
        h.docker.lock().unwrap().unhealthy.remove(&1);

        let writes_before = h.disk.lock().unwrap().secret_writes;
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "ps --all",
            result: exited(1, ""),
            apply: false,
        });
        assert!(matches!(
            h.put_secret(4, &digest, 2, b"epoch-2"),
            ComposeResponse::ComposeSecretAccepted {
                recreate: Some(_),
                ..
            }
        ));
        h.settle();
        assert_eq!(h.operation(4).state, OperationState::Failed);
        assert_eq!(
            h.count("--force-recreate"),
            0,
            "must not recreate without fresh baseline"
        );
        assert_eq!(h.disk.lock().unwrap().secret_writes, writes_before);
    }

    // 2. Fail saving the fresh baseline -> 0 writes, 0 recreates, Failed.
    {
        let (h, digest) = setup_multi();
        let writes_before = h.disk.lock().unwrap().secret_writes;
        // Intent save succeeds, then baseline save fails (use fail_save_state via pause/fault).
        // Let's block at `ps --all`, then enable `fail_save_state` before releasing so Intent succeeds
        // and baseline save fails deterministically.
        let (rel, rx) = mpsc::channel();
        h.docker.lock().unwrap().block = Some(("ps --all", rx));
        h.put_secret(4, &digest, 2, b"epoch-2");
        let mut waited = 0;
        while h.operation(4).stage != OperationStage::Recreating {
            assert!(waited < 500);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        h.disk
            .lock()
            .unwrap()
            .save_faults
            .push_back(Some(StoreFault::BeforeRename));
        rel.send(()).unwrap();
        h.settle();
        assert_eq!(h.operation(4).state, OperationState::Failed);
        assert_eq!(h.operation(4).failure, Some(FailureCode::StoreFailed));
        assert_eq!(h.count("--force-recreate"), 0);
        assert_eq!(h.disk.lock().unwrap().secret_writes, writes_before);
    }

    // 3. Cut file publication and rotation YAML publication -> 0 recreates.
    {
        let (h, digest) = setup_multi();
        h.disk.lock().unwrap().fail_write_secret = true;
        h.put_secret(4, &digest, 2, b"epoch-2");
        h.settle();
        assert_eq!(h.operation(4).state, OperationState::Failed);
        assert_eq!(h.count("--force-recreate"), 0);
    }
    {
        let (h, digest) = setup_multi();
        h.disk.lock().unwrap().fail_write_rotation = true;
        h.put_secret(5, &digest, 2, b"epoch-2");
        h.settle();
        assert_eq!(h.operation(5).state, OperationState::Failed);
        assert_eq!(h.count("--force-recreate"), 0);
    }

    // 4. Cut --force-recreate with: (a) no effect, (b) subset of users replaced,
    // (c) all IDs replaced with a stale-mount witness (old e1 path or RW: true),
    // (d) all affected users correctly replaced while unaffected `worker` keeps its exact ID.
    // (a) No effect (exit 0 without replacing containers) -> Failed + recovery_required.
    {
        let (h, digest) = setup_multi();
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "--force-recreate",
            result: exited(0, ""),
            apply: false,
        });
        h.put_secret(10, &digest, 2, b"epoch-2");
        h.settle();
        assert_eq!(h.operation(10).state, OperationState::Failed);
        assert!(h.status(PROJECT).recovery_required);
    }

    // (b) Subset of users replaced (only `web`, `api` untouched) -> Failed + recovery_required.
    {
        let (h, digest) = setup_multi();
        h.docker.lock().unwrap().recreate_subset_once = Some(BTreeSet::from(["web".to_owned()]));
        h.put_secret(11, &digest, 2, b"epoch-2");
        h.settle();
        assert_eq!(h.operation(11).state, OperationState::Failed);
        assert!(h.status(PROJECT).recovery_required);
    }

    // (c) All affected IDs replaced with fresh IDs, BUT stale-mount witness (old e1 source,
    // and independently RW: true) -> BOTH normal execution AND read-only reconcile MUST fail!
    let stale_e1 = secret_file(
        PROJECT,
        &secret_instance(
            SLOT,
            &sha256_hex(&bytes(&multi_service_secret_manifest())),
            1,
            GENERATION,
        ),
    );
    for rw_case in [false, true] {
        let (h, digest) = setup_multi();
        let new_e2 = secret_file(PROJECT, &secret_instance(SLOT, &digest, 2, GENERATION));
        let bad_mount = FakeMount {
            mount_type: "bind".to_owned(),
            source: if rw_case { new_e2 } else { stale_e1.clone() },
            destination: format!("/run/secrets/{SLOT}"),
            rw: rw_case,
        };
        {
            let mut docker = h.docker.lock().unwrap();
            docker
                .override_mounts_once
                .insert("web".to_owned(), vec![bad_mount.clone()]);
            docker
                .override_mounts_once
                .insert("api".to_owned(), vec![bad_mount]);
            // Timeout after applying so we verify read-only reconcile also rejects stale mounts.
            docker.faults.push(Fault {
                needle: "--force-recreate",
                result: RunResult::TimedOut,
                apply: true,
            });
        }
        h.put_secret(12, &digest, 2, b"epoch-2");
        h.settle();
        assert_eq!(h.operation(12).state, OperationState::Ambiguous);
        let rec = h.reconcile(12);
        assert!(
            matches!(
                rec,
                ComposeResponse::ComposeOperationState {
                    operation: OperationView {
                        state: OperationState::Failed,
                        failure: Some(FailureCode::NotApplied),
                        ..
                    },
                    ..
                }
            ),
            "reconcile must reject stale/rw mount (rw={rw_case}): {rec:?}"
        );
        assert!(h.status(PROJECT).recovery_required);
    }

    // Also normal exit-0 recreate with stale mount fails prove_health_recreate.
    {
        let (h, digest) = setup_multi();
        let bad_mount = FakeMount {
            mount_type: "bind".to_owned(),
            source: stale_e1,
            destination: format!("/run/secrets/{SLOT}"),
            rw: false,
        };
        h.docker
            .lock()
            .unwrap()
            .override_mounts_once
            .insert("api".to_owned(), vec![bad_mount]);
        h.put_secret(13, &digest, 2, b"epoch-2");
        h.settle();
        assert_eq!(h.operation(13).state, OperationState::Failed);
        assert_eq!(h.operation(13).failure, Some(FailureCode::HealthFailed));
        assert!(h.status(PROJECT).recovery_required);
    }

    // (d) Timeout after full valid effect: affected `web` & `api` replaced with exact e2 mount,
    // unaffected `worker` keeps its exact original container ID -> read-only reconcile succeeds
    // exactly once and replay returns Succeeded with 0 extra mutations.
    {
        let (h, digest) = setup_multi();
        let worker_id_before = h.docker.lock().unwrap().containers[PROJECT]
            .iter()
            .find(|c| c.service == "worker")
            .unwrap()
            .id
            .clone();
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "--force-recreate",
            result: RunResult::TimedOut,
            apply: true,
        });
        h.put_secret(14, &digest, 2, b"epoch-2");
        h.settle();
        assert_eq!(h.operation(14).state, OperationState::Ambiguous);
        let before_rec = h.calls().len();
        let rec = h.reconcile(14);
        assert!(matches!(
            rec,
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Succeeded,
                    ..
                },
                ..
            }
        ));
        let rec_calls = &h.calls()[before_rec..];
        assert!(
            rec_calls.iter().all(|argv| {
                let j = argv.join(" ");
                j.contains("ps --all") || j.contains("container inspect")
            }),
            "reconcile issued non-query call: {rec_calls:?}"
        );
        assert_eq!(rejected(&h.reconcile(14)), Some(RejectCode::NotAmbiguous));
        let worker_id_after = h.docker.lock().unwrap().containers[PROJECT]
            .iter()
            .find(|c| c.service == "worker")
            .unwrap()
            .id
            .clone();
        assert_eq!(
            worker_id_after, worker_id_before,
            "unaffected service must not be recreated"
        );
        let replay = h.put_secret(14, &digest, 2, b"epoch-2");
        assert!(matches!(
            replay,
            ComposeResponse::ComposeSecretAccepted {
                epoch: 2,
                recreate: Some(OperationView {
                    state: OperationState::Succeeded,
                    ..
                }),
                ..
            }
        ));
        assert_eq!(h.count("--force-recreate"), 1);
    }
}

// =========================================================================
// Contract Row D: Release isolation (explicit digest, reverse stage order, rollback)
// =========================================================================

#[test]
fn contract_d_release_isolation() {
    let h = Harness::new();
    h.hello();

    // Release A: uid/gid 1000:1000, slot `admin_password`.
    let rev_a = manifest();
    let digest_a = h.stage(&bytes(&rev_a));

    // Release B: uid/gid 2000:2000, SAME slot `admin_password`, staged AFTER A.
    let mut rev_b = manifest();
    rev_b["services"][0]["uid"] = json!(2000);
    rev_b["services"][0]["gid"] = json!(2000);
    rev_b["services"][0]["mounts"] = json!([]);
    rev_b["volumes"] = json!([]);
    let digest_b = h.stage(&bytes(&rev_b));

    // Release C: SAME owner 1000:1000 as A, different env -> different digest, staged LAST.
    let mut rev_c = manifest();
    rev_c["services"][0]["environment"][0]["value"] = json!("8099");
    let digest_c = h.stage(&bytes(&rev_c));

    // Deliver to A (earliest staged, NOT most recent!), B, and C in reverse/mixed order
    // with distinct payloads.
    assert!(matches!(
        h.put_secret(201, &digest_a, 1, b"secret-for-A"),
        ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
    ));
    assert!(matches!(
        h.put_secret(202, &digest_b, 1, b"secret-for-B"),
        ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
    ));
    assert!(matches!(
        h.put_secret(203, &digest_c, 1, b"secret-for-C"),
        ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
    ));

    // Verify each release got its own isolated immutable file with its own derived owner & payload.
    let inst_a = secret_instance(SLOT, &digest_a, 1, GENERATION);
    let inst_b = secret_instance(SLOT, &digest_b, 1, GENERATION);
    let inst_c = secret_instance(SLOT, &digest_c, 1, GENERATION);
    {
        let disk = h.disk.lock().unwrap();
        let (bytes_a, wit_a) = &disk.secrets[&(PROJECT.to_owned(), inst_a.clone())];
        let (bytes_b, wit_b) = &disk.secrets[&(PROJECT.to_owned(), inst_b.clone())];
        let (bytes_c, wit_c) = &disk.secrets[&(PROJECT.to_owned(), inst_c.clone())];
        assert_eq!(
            (bytes_a.as_slice(), wit_a.uid, wit_a.gid),
            (&b"secret-for-A"[..], 1000, 1000)
        );
        assert_eq!(
            (bytes_b.as_slice(), wit_b.uid, wit_b.gid),
            (&b"secret-for-B"[..], 2000, 2000)
        );
        assert_eq!(
            (bytes_c.as_slice(), wit_c.uid, wit_c.gid),
            (&b"secret-for-C"[..], 1000, 1000)
        );
    }

    // Replaying op 201 against a DIFFERENT digest must fail OperationConflict.
    assert_eq!(
        rejected(&h.put_secret(201, &digest_c, 1, b"secret-for-A")),
        Some(RejectCode::OperationConflict)
    );

    // Evict A from the staging cache by staging MAX_STAGED unrelated releases, then restage A.
    for i in 0..MAX_STAGED {
        let mut filler = manifest();
        filler["services"][0]["environment"][0]["value"] = json!(format!("900{i}"));
        h.stage(&bytes(&filler));
    }
    assert_eq!(
        rejected(&h.deploy(204, PROJECT, 1, &digest_a)),
        Some(RejectCode::UnknownRelease)
    );
    assert_eq!(h.stage(&bytes(&rev_a)), digest_a);

    // Deploy explicitly supplied A -> succeeds and mounts A's exact immutable file!
    h.deploy(205, PROJECT, 1, &digest_a);
    h.settle();
    assert_eq!(h.operation(205).state, OperationState::Succeeded);
    assert_eq!(
        h.docker.lock().unwrap().containers[PROJECT][0].mounts[0].source,
        secret_file(PROJECT, &inst_a)
    );

    // Now restage C (compatible layout with A, different secret) and fail apply/health ->
    // rollback restores A with A's OWN untouched binding and tmpfs file!
    assert_eq!(h.stage(&bytes(&rev_c)), digest_c);
    h.docker.lock().unwrap().unhealthy.insert(2);
    h.deploy(206, PROJECT, 2, &digest_c);
    h.settle();
    let failed = h.operation(206);
    assert_eq!(
        (failed.state, failed.failure, failed.restored_previous),
        (
            OperationState::Failed,
            Some(FailureCode::HealthFailed),
            true
        )
    );
    assert_eq!(
        h.docker.lock().unwrap().containers[PROJECT][0].mounts[0].source,
        secret_file(PROJECT, &inst_a),
        "rollback must mount prior release A's exact binding"
    );
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), inst_a)].0,
        b"secret-for-A",
        "prior release tmpfs file must not be overwritten or reclaimed"
    );
}

// =========================================================================
// Contract Row E: Down is a joint observation (containers + generated network)
// =========================================================================

#[test]
fn contract_e_down_joint_observation() {
    // 1. Both mixed states on Down timeout + read-only reconcile:
    // (a) containers gone, owned network STILL present -> reconcile fails NotApplied (0 mutations).
    {
        let h = Harness::new();
        h.deployed();
        h.docker.lock().unwrap().keep_network_on_down = true;
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "/dev/null down",
            result: RunResult::TimedOut,
            apply: true,
        });
        h.lifecycle(2, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(2).state, OperationState::Ambiguous);
        assert!(h.docker.lock().unwrap().containers[PROJECT].is_empty());
        assert!(!h.docker.lock().unwrap().networks.is_empty());

        let before_rec = h.calls().len();
        let rec = h.reconcile(2);
        assert!(
            matches!(
                rec,
                ComposeResponse::ComposeOperationState {
                    operation: OperationView {
                        state: OperationState::Failed,
                        failure: Some(FailureCode::NotApplied),
                        ..
                    },
                    ..
                }
            ),
            "reconcile must fail when owned network still exists: {rec:?}"
        );
        let rec_calls = &h.calls()[before_rec..];
        assert!(rec_calls.iter().all(|argv| {
            let j = argv.join(" ");
            j.contains("ps --all") || j.contains("network ls")
        }));
        assert!(h.status(PROJECT).recovery_required);
        assert_eq!(h.count("network rm"), 0);

        // Explicit new Down removes the lingering owned network via `network rm <id>` and succeeds.
        h.lifecycle(3, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(3).state, OperationState::Succeeded);
        assert_eq!(h.count("network rm"), 1);
        assert!(h.docker.lock().unwrap().networks.is_empty());
        assert_eq!(h.status(PROJECT).desired, Desired::Down);
        assert_eq!(h.status(PROJECT).committed_revision, Some(1));
        assert!(
            !h.docker.lock().unwrap().volumes.is_empty(),
            "Down keeps volumes"
        );
    }

    // (b) network gone, retired STOPPED container STILL present -> reconcile fails NotApplied.
    {
        let h = Harness::new();
        h.deployed();
        h.docker.lock().unwrap().networks.clear();
        h.docker
            .lock()
            .unwrap()
            .containers
            .entry(PROJECT.to_owned())
            .or_default()
            .push(Container {
                id: "00000000beef".to_owned(),
                service: "retired".to_owned(),
                state: "exited",
                revision: 1,
                mounts: Vec::new(),
            });
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "/dev/null down",
            result: RunResult::TimedOut,
            apply: true,
        });
        h.lifecycle(4, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(4).state, OperationState::Ambiguous);
        let rec = h.reconcile(4);
        assert!(matches!(
            rec,
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Failed,
                    failure: Some(FailureCode::NotApplied),
                    ..
                },
                ..
            }
        ));
        assert!(h.status(PROJECT).recovery_required);
    }

    // 2. Cut targeted container cleanup (`container rm`) and owned `network rm` individually.
    {
        let h = Harness::new();
        h.deployed();
        h.docker
            .lock()
            .unwrap()
            .containers
            .entry(PROJECT.to_owned())
            .or_default()
            .push(Container {
                id: "00000000cafe".to_owned(),
                service: "retired".to_owned(),
                state: "running",
                revision: 1,
                mounts: Vec::new(),
            });
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "container rm",
            result: exited(1, ""),
            apply: false,
        });
        h.lifecycle(5, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(5).state, OperationState::Failed);
        assert_eq!(h.operation(5).failure, Some(FailureCode::DownFailed));
        assert!(h.status(PROJECT).recovery_required);

        // Now let container cleanup finish, keep owned network on down, and timeout `network rm`
        // AFTER it applies -> reconcile sees both absent and succeeds!
        h.docker.lock().unwrap().keep_network_on_down = true;
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "network rm",
            result: RunResult::TimedOut,
            apply: true,
        });
        h.lifecycle(6, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(6).state, OperationState::Ambiguous);
        let rec = h.reconcile(6);
        assert!(matches!(
            rec,
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Succeeded,
                    ..
                },
                ..
            }
        ));
        assert_eq!(h.status(PROJECT).desired, Desired::Down);
        // Replay after lost reply returns Succeeded with 0 extra mutations.
        let calls_before = h.calls().len();
        assert!(matches!(
            h.lifecycle(6, PROJECT, OperationKind::Down),
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Succeeded,
                    ..
                },
                ..
            }
        ));
        assert_eq!(h.calls().len(), calls_before);
    }

    // 3. Exact-name FOREIGN network (unlabelled or other project) -> NEVER calls `network rm`,
    // NEVER yields false success on Down or Reconcile.
    {
        let h = Harness::new();
        h.deployed();
        {
            let mut docker = h.docker.lock().unwrap();
            docker.networks.clear();
            docker.networks.insert(
                "f".repeat(64),
                FakeNetwork {
                    id: "f".repeat(64),
                    name: network_name(PROJECT),
                    project_label: None,
                },
            );
        }
        h.lifecycle(7, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(7).state, OperationState::Failed);
        assert_eq!(h.operation(7).failure, Some(FailureCode::DownFailed));
        assert_eq!(
            h.count("network rm"),
            0,
            "foreign network must never be removed"
        );
        assert!(
            h.docker
                .lock()
                .unwrap()
                .networks
                .contains_key(&"f".repeat(64))
        );
    }

    // 4. Malformed, over-bound (>64), or truncated (`OutputIncomplete`) `network ls` ->
    // never yields false success on Down.
    for bad_result in [
        exited(0, "not-json\n"),
        exited(
            0,
            (0..65)
                .map(|i| format!(r#"{{"ID":"{i:012x}","Name":"n{i}","Labels":""}}"#))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        RunResult::OutputIncomplete { code: 0 },
    ] {
        let h = Harness::new();
        h.deployed();
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "network ls",
            result: bad_result,
            apply: false,
        });
        h.lifecycle(8, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(8).state, OperationState::Failed);
        assert_eq!(h.operation(8).failure, Some(FailureCode::DownFailed));
    }

    // 5. Rollback to Desired::Down uses the exact same joint Down proof:
    // if owned network remains and `network rm` fails, rollback sets recovery_required = true.
    {
        let h = Harness::new();
        let digest = h.deployed();
        h.lifecycle(10, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(10).state, OperationState::Succeeded);
        h.docker.lock().unwrap().unhealthy.insert(2);
        h.docker.lock().unwrap().keep_network_on_down = true;
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "network rm",
            result: exited(1, ""),
            apply: false,
        });
        h.deploy(11, PROJECT, 2, &digest);
        h.settle();
        assert_eq!(h.operation(11).state, OperationState::Failed);
        assert!(!h.operation(11).restored_previous);
        assert!(h.status(PROJECT).recovery_required);
    }
}

// =========================================================================
// Contract Row F: Volume reservation is conservative
// =========================================================================

#[test]
fn contract_f_volume_reservation_conservative() {
    // 1. Cut charge save BeforeRename -> zero `volume create` calls, Failed(StoreFailed).
    {
        let h = Harness::new();
        h.docker
            .lock()
            .unwrap()
            .services
            .insert(PROJECT.to_owned(), vec!["app".to_owned()]);
        h.hello();
        let digest = h.stage(&bytes(&plain(PROJECT, &["data1"])));
        let (rel, rx) = mpsc::channel();
        h.docker.lock().unwrap().block = Some(("volume ls", rx));
        h.deploy(1, PROJECT, 1, &digest);
        let mut waited = 0;
        while h.operation(1).stage != OperationStage::Volumes {
            assert!(waited < 500);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        h.disk
            .lock()
            .unwrap()
            .save_faults
            .push_back(Some(StoreFault::BeforeRename));
        rel.send(()).unwrap();
        h.settle();
        assert_eq!(h.operation(1).state, OperationState::Failed);
        assert_eq!(h.operation(1).failure, Some(FailureCode::StoreFailed));
        assert_eq!(
            h.count("volume create"),
            0,
            "must not create volume if charge save failed"
        );
    }

    // 2. Apply `volume create` then fail/timeout `volume create`, OR succeed create then
    // fail/truncate post-create `volume ls` or mountpoint or `prepare_volume`, reloading a
    // fresh broker between EVERY distinct request -> after 8 distinct volumes, the 9th is
    // rejected VolumeLimit and cannot create (>8 impossible)!
    {
        let disk = Arc::new(Mutex::new(Disk::default()));
        let docker = Arc::new(Mutex::new(Docker::new(Arc::clone(&disk))));
        docker
            .lock()
            .unwrap()
            .services
            .insert(PROJECT.to_owned(), vec!["app".to_owned()]);

        for rev in 1..=9u64 {
            let h = Harness::on_shared(Arc::clone(&disk), Arc::clone(&docker));
            h.hello();
            let slot = format!("v{rev}");
            let digest = h.stage(&bytes(&plain(PROJECT, &[&slot])));
            if rev <= MAX_VOLUMES as u64 {
                match rev % 4 {
                    1 => docker.lock().unwrap().faults.push(Fault {
                        needle: "volume create",
                        result: RunResult::TimedOut,
                        apply: true,
                    }),
                    2 => docker.lock().unwrap().faults.push(Fault {
                        needle: "volume create",
                        result: exited(1, ""),
                        apply: true,
                    }),
                    3 => docker.lock().unwrap().faults.push(Fault {
                        needle: "{{.Mountpoint}}",
                        result: exited(1, ""),
                        apply: false,
                    }),
                    _ => {
                        // Fail prepare_volume after successful create & verify.
                        disk.lock().unwrap().fail_prepare_volume = true;
                    }
                }
            }
            let reply = h.deploy(100 + rev as u32, PROJECT, rev, &digest);
            if rev <= MAX_VOLUMES as u64 {
                assert!(matches!(
                    reply,
                    ComposeResponse::ComposeOperationState { .. }
                ));
                h.settle();
                assert_eq!(
                    h.operation(100 + rev as u32).failure,
                    Some(FailureCode::VolumeFailed),
                    "rev {rev}"
                );
                disk.lock().unwrap().fail_prepare_volume = false;
            } else {
                assert_eq!(
                    rejected(&reply),
                    Some(RejectCode::VolumeLimit),
                    "9th volume must be rejected at admission"
                );
            }
        }
        assert_eq!(docker.lock().unwrap().volumes.len(), MAX_VOLUMES);
        assert!(docker.lock().unwrap().faults.is_empty());

        // Same-name matching retry reuses its existing charge and succeeds without exceeding cap.
        let h = Harness::on_shared(Arc::clone(&disk), Arc::clone(&docker));
        h.hello();
        let digest = h.stage(&bytes(&plain(PROJECT, &["v1"])));
        h.deploy(200, PROJECT, 10, &digest);
        h.settle();
        assert_eq!(h.operation(200).state, OperationState::Succeeded);
        assert_eq!(docker.lock().unwrap().volumes.len(), MAX_VOLUMES);
    }

    // 3. Failed/truncated initial `volume ls` never authorizes create, reuse, prepare, or uncharging.
    {
        let h = Harness::new();
        h.docker
            .lock()
            .unwrap()
            .services
            .insert(PROJECT.to_owned(), vec!["app".to_owned()]);
        h.hello();
        let digest = h.stage(&bytes(&plain(PROJECT, &["v1"])));
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "volume ls",
            result: RunResult::OutputIncomplete { code: 0 },
            apply: false,
        });
        h.deploy(1, PROJECT, 1, &digest);
        h.settle();
        assert_eq!(h.operation(1).failure, Some(FailureCode::VolumeFailed));
        assert_eq!(h.count("volume create"), 0);
        assert!(h.disk.lock().unwrap().prepared.is_empty());
    }

    // 4. Verified LaunchFailed + fresh complete Absent permits durable release of the new charge;
    // cutting that release save keeps the charge durably.
    {
        let h = Harness::new();
        h.docker
            .lock()
            .unwrap()
            .services
            .insert(PROJECT.to_owned(), vec!["app".to_owned()]);
        h.hello();
        let digest = h.stage(&bytes(&plain(PROJECT, &["v1"])));
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "volume create",
            result: RunResult::LaunchFailed,
            apply: false,
        });
        h.deploy(1, PROJECT, 1, &digest);
        h.settle();
        assert_eq!(
            h.operation(1).failure,
            Some(FailureCode::EngineLaunchFailed)
        );
        // Charge was durably released, so 8 fresh volumes can still be created!
        let state_str = String::from_utf8(h.disk.lock().unwrap().state.clone().unwrap()).unwrap();
        assert!(
            !state_str.contains("arx-v_calm-life_v1"),
            "LaunchFailed + Absent must release new charge"
        );
    }

    // 5. Collision cannot prepare another volume.
    {
        let h = Harness::new();
        h.docker
            .lock()
            .unwrap()
            .services
            .insert(PROJECT.to_owned(), vec!["app".to_owned()]);
        h.docker.lock().unwrap().volumes.insert(
            "arx-v_calm-life_v1".to_owned(),
            "other/v1/999:999".to_owned(),
        );
        h.hello();
        let digest = h.stage(&bytes(&plain(PROJECT, &["v1"])));
        h.deploy(1, PROJECT, 1, &digest);
        h.settle();
        assert_eq!(h.operation(1).failure, Some(FailureCode::VolumeCollision));
        assert!(h.disk.lock().unwrap().prepared.is_empty());
    }
}

// =========================================================================
// Contract Row G: Durable settlement, exact attribution, & OutputIncomplete
// =========================================================================

#[test]
fn contract_g_durable_settlement_and_exact_attribution() {
    // 1. Cut terminal state save after observed success for Deploy:
    // live status shows unpromoted revision + op is Ambiguous + dependent work rejected
    // ReconcileRequired; reload keeps Ambiguous; different target with same ID conflicts;
    // once storage recovers, read-only reconcile promotes and settles cleanly!
    {
        let h = Harness::new();
        h.hello();
        let digest = h.stage(&bytes(&manifest()));
        h.put_secret(90, &digest, 1, b"first value");
        let (rel, rx) = mpsc::channel();
        h.docker.lock().unwrap().block = Some(("container inspect", rx));
        h.deploy(1, PROJECT, 1, &digest);
        let mut waited = 0;
        while h.operation(1).stage != OperationStage::HealthCheck {
            assert!(waited < 500);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        // Fail the terminal settlement save BeforeRename!
        h.disk
            .lock()
            .unwrap()
            .save_faults
            .push_back(Some(StoreFault::BeforeRename));
        rel.send(()).unwrap();
        h.settle();

        assert_eq!(
            h.operation(1).state,
            OperationState::Ambiguous,
            "failed terminal save must leave Deploy Ambiguous, never Succeeded"
        );
        assert_eq!(
            h.status(PROJECT).committed_revision,
            None,
            "unpersisted promotion must not be exposed in memory"
        );
        // Subsequent admitted work cannot consume the unpersisted promotion.
        assert_eq!(
            rejected(&h.lifecycle(2, PROJECT, OperationKind::Start)),
            Some(RejectCode::ReconcileRequired)
        );
        assert_eq!(
            rejected(&h.deploy(1, PROJECT, 2, &digest)),
            Some(RejectCode::OperationConflict)
        );

        // Reload over exact disk snapshot keeps Ambiguous; reconcile settles Succeeded.
        let reloaded = Harness::on_shared(Arc::clone(&h.disk), Arc::clone(&h.docker));
        reloaded.hello();
        assert_eq!(reloaded.operation(1).state, OperationState::Ambiguous);
        assert_eq!(reloaded.status(PROJECT).committed_revision, None);
        let rec = reloaded.reconcile(1);
        assert!(matches!(
            rec,
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Succeeded,
                    ..
                },
                ..
            }
        ));
        assert_eq!(reloaded.status(PROJECT).committed_revision, Some(1));
    }

    // 2. Cut terminal save for Stop, Start, Down, and Recreate -> all become Ambiguous
    // without exposing unpersisted desired/binding transitions!
    {
        let h = Harness::new();
        let digest = h.deployed();

        // Stop terminal save cut:
        let (rel, rx) = mpsc::channel();
        h.docker.lock().unwrap().block = Some(("ps --all", rx));
        h.lifecycle(10, PROJECT, OperationKind::Stop);
        let mut waited = 0;
        while h.operation(10).stage != OperationStage::Stopping {
            assert!(waited < 500);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        h.disk
            .lock()
            .unwrap()
            .save_faults
            .push_back(Some(StoreFault::BeforeRename));
        rel.send(()).unwrap();
        h.settle();
        assert_eq!(h.operation(10).state, OperationState::Ambiguous);
        assert_eq!(h.status(PROJECT).desired, Desired::Started);
        assert!(matches!(
            h.reconcile(10),
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Succeeded,
                    ..
                },
                ..
            }
        ));
        assert_eq!(h.status(PROJECT).desired, Desired::Stopped);

        // Start terminal save cut:
        let (rel, rx) = mpsc::channel();
        h.docker.lock().unwrap().block = Some(("container inspect", rx));
        h.lifecycle(11, PROJECT, OperationKind::Start);
        let mut waited = 0;
        while h.operation(11).stage != OperationStage::HealthCheck {
            assert!(waited < 500);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        h.disk
            .lock()
            .unwrap()
            .save_faults
            .push_back(Some(StoreFault::BeforeRename));
        rel.send(()).unwrap();
        h.settle();
        assert_eq!(h.operation(11).state, OperationState::Ambiguous);
        assert_eq!(h.status(PROJECT).desired, Desired::Stopped);
        assert!(matches!(
            h.reconcile(11),
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Succeeded,
                    ..
                },
                ..
            }
        ));
        assert_eq!(h.status(PROJECT).desired, Desired::Started);

        // Recreate terminal save cut:
        let (rel, rx) = mpsc::channel();
        h.docker.lock().unwrap().block = Some(("container inspect", rx));
        h.put_secret(12, &digest, 2, b"rotated");
        let mut waited = 0;
        while h.operation(12).stage != OperationStage::HealthCheck {
            assert!(waited < 500);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        h.disk
            .lock()
            .unwrap()
            .save_faults
            .push_back(Some(StoreFault::BeforeRename));
        rel.send(()).unwrap();
        h.settle();
        assert_eq!(h.operation(12).state, OperationState::Ambiguous);
        assert!(matches!(
            h.reconcile(12),
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Succeeded,
                    ..
                },
                ..
            }
        ));

        // Down terminal save cut:
        let (rel, rx) = mpsc::channel();
        h.docker.lock().unwrap().block = Some(("network ls", rx));
        h.lifecycle(13, PROJECT, OperationKind::Down);
        let mut waited = 0;
        while h.operation(13).stage != OperationStage::Removing {
            assert!(waited < 500);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        h.disk
            .lock()
            .unwrap()
            .save_faults
            .push_back(Some(StoreFault::BeforeRename));
        rel.send(()).unwrap();
        h.settle();
        assert_eq!(h.operation(13).state, OperationState::Ambiguous);
        assert_eq!(h.status(PROJECT).desired, Desired::Started);
        assert!(matches!(
            h.reconcile(13),
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Succeeded,
                    ..
                },
                ..
            }
        ));
        assert_eq!(h.status(PROJECT).desired, Desired::Down);
    }

    // 3. Rollback from Stopped and Down preserves lifecycle states, and cutting terminal
    // save on rollback from Started keeps op Ambiguous without exposing unpersisted state.
    for target_desired in [Desired::Stopped, Desired::Down] {
        let h = Harness::new();
        let digest = h.deployed();
        let kind = if target_desired == Desired::Stopped {
            OperationKind::Stop
        } else {
            OperationKind::Down
        };
        h.lifecycle(10, PROJECT, kind);
        h.settle();
        assert_eq!(h.operation(10).state, OperationState::Succeeded);
        h.docker.lock().unwrap().unhealthy.insert(2);
        let before = h.calls().len();
        h.deploy(11, PROJECT, 2, &digest);
        h.settle();
        assert_eq!(h.operation(11).state, OperationState::Failed);
        assert_eq!(h.operation(11).failure, Some(FailureCode::HealthFailed));
        let up1 = compose_argv(1, &["up", "--detach", "--no-build", "--pull", "never"]);
        assert!(!h.calls()[before..].contains(&up1));
        let status = h.status(PROJECT);
        assert_eq!(status.desired, target_desired);
        assert_eq!(status.committed_revision, Some(1));
        assert!(!status.recovery_required);
    }
    {
        let h = Harness::new();
        let digest = h.deployed();
        h.docker.lock().unwrap().unhealthy.insert(2);
        // Deploy rev 2 fails health -> restores rev 1 -> block on rev 1's `container inspect`
        // and cut the terminal rollback settlement save!
        let (rel, rx) = mpsc::channel();
        h.docker.lock().unwrap().block = Some(("r1/compose.yaml --env-file /dev/null up", rx));
        h.deploy(12, PROJECT, 2, &digest);
        let mut waited = 0;
        while h.operation(12).stage != OperationStage::Restoring {
            assert!(waited < 500);
            std::thread::sleep(Duration::from_millis(10));
            waited += 1;
        }
        h.disk
            .lock()
            .unwrap()
            .save_faults
            .push_back(Some(StoreFault::BeforeRename));
        rel.send(()).unwrap();
        h.settle();
        assert_eq!(h.operation(12).state, OperationState::Ambiguous);
        assert_eq!(
            rejected(&h.lifecycle(13, PROJECT, OperationKind::Stop)),
            Some(RejectCode::ReconcileRequired)
        );
        // Reconcile resolves op 12 cleanly (rev 2 is not running -> Failed(NotApplied))
        // while preserving committed revision 1!
        assert!(matches!(
            h.reconcile(12),
            ComposeResponse::ComposeOperationState {
                operation: OperationView {
                    state: OperationState::Failed,
                    failure: Some(FailureCode::NotApplied),
                    ..
                },
                ..
            }
        ));
        assert_eq!(h.status(PROJECT).committed_revision, Some(1));
    }

    // 4. Same-ID replay after journal trim + restart + tombstones.
    {
        let h = Harness::new();
        let digest1 = h.deployed();
        for i in 0..MAX_JOURNAL as u32 {
            let kind = if i % 2 == 0 {
                OperationKind::Stop
            } else {
                OperationKind::Start
            };
            h.lifecycle(100 + i, PROJECT, kind);
            h.settle();
            assert_eq!(h.operation(100 + i).state, OperationState::Succeeded);
        }
        let before = h.calls().len();
        assert_eq!(
            rejected(&h.deploy(1, PROJECT, 1, &digest1)),
            Some(RejectCode::OperationConflict)
        );
        assert_eq!(h.calls().len(), before);
        let restarted = Harness::on(Arc::clone(&h.disk));
        restarted.hello();
        assert_eq!(
            rejected(&restarted.lifecycle(1, PROJECT, OperationKind::Stop)),
            Some(RejectCode::OperationConflict)
        );
    }

    // 5. Missing exact requested reconcile manifest -> rejected UnknownRelease (no fallback!).
    {
        let h = Harness::new();
        let digest = h.deployed();
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "/dev/null stop",
            result: RunResult::TimedOut,
            apply: false,
        });
        h.lifecycle(20, PROJECT, OperationKind::Stop);
        h.settle();
        assert_eq!(h.operation(20).state, OperationState::Ambiguous);
        // Tamper persisted state so the operation requests revision 99 (no matching manifest).
        let mut val: Value =
            serde_json::from_slice(h.disk.lock().unwrap().state.as_ref().unwrap()).unwrap();
        val["operations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|o| o["operation_id"] == op(20))
            .unwrap()["revision"] = json!(99);
        let tampered_disk = Arc::new(Mutex::new(Disk {
            state: Some(serde_json::to_vec(&val).unwrap()),
            ..Disk::default()
        }));
        let reloaded = Harness::on_shared(tampered_disk, Arc::clone(&h.docker));
        reloaded.hello();
        assert_eq!(
            rejected(&reloaded.reconcile(20)),
            Some(RejectCode::UnknownRelease),
            "reconcile must never fall back to a different revision manifest"
        );
        let _ = digest;
    }

    // 6. OutputIncomplete on `ps --all` is NEVER treated as an empty complete inventory.
    {
        let h = Harness::new();
        h.deployed();
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "ps --all",
            result: RunResult::OutputIncomplete { code: 0 },
            apply: false,
        });
        h.lifecycle(30, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(
            h.operation(30).state,
            OperationState::Failed,
            "OutputIncomplete on ps must never be treated as empty/complete Down proof"
        );
        assert_eq!(h.operation(30).failure, Some(FailureCode::DownFailed));
    }
}

#[test]
fn deploy_retires_removed_attributed_services() {
    let h = Harness::new();
    h.hello();
    let mut rev1 = manifest();
    rev1["services"].as_array_mut().unwrap().push(json!({
        "name": "worker",
        "image": image(),
        "platform": "linux/amd64",
        "uid": 1000,
        "gid": 1000,
        "http_port": null,
        "health_path": null,
        "read_only_rootfs": true,
        "environment": [],
        "mounts": [],
        "secrets": [],
        "limits": {"cpu_millis": 100, "memory_mib": 64, "pids": 32}
    }));
    {
        let mut docker = h.docker.lock().unwrap();
        docker.revision_services.insert(
            (PROJECT.to_owned(), 1),
            vec!["web".to_owned(), "worker".to_owned()],
        );
        docker
            .revision_services
            .insert((PROJECT.to_owned(), 2), vec!["web".to_owned()]);
    }
    let digest1 = h.stage(&bytes(&rev1));
    h.put_secret(90, &digest1, 1, b"first value");
    h.deploy(1, PROJECT, 1, &digest1);
    h.settle();
    assert_eq!(h.operation(1).state, OperationState::Succeeded);
    assert_eq!(h.docker.lock().unwrap().containers[PROJECT].len(), 2);

    let digest2 = h.stage(&bytes(&manifest()));
    h.put_secret(91, &digest2, 1, b"second value");
    h.deploy(2, PROJECT, 2, &digest2);
    h.settle();
    assert_eq!(h.operation(2).state, OperationState::Succeeded);
    {
        let docker = h.docker.lock().unwrap();
        let names: Vec<&str> = docker.containers[PROJECT]
            .iter()
            .map(|c| c.service.as_str())
            .collect();
        assert_eq!(names, vec!["web"], "retired service remained after deploy");
    }
    assert!(h.count("container stop") >= 1);
    assert!(h.count("container rm") >= 1);
    assert_eq!(h.count("--remove-orphans"), 0);
}

// =========================================================================
// Contract Row H: Existing fences & canary on all new query paths
// =========================================================================

#[test]
fn contract_h_existing_fences_and_new_query_canary() {
    let canary = format!("arx-canary-{:016x}", 0x5eed_c0de_feed_f00d_u64);
    let encoded = encode_base64(canary.as_bytes());
    let h = Harness::new();
    // Emit raw and base64 canary from EVERY subprocess, including the new
    // `volume ls`, `container inspect`, and `network ls` queries on success AND failure.
    {
        let mut docker = h.docker.lock().unwrap();
        docker.output_canary = Some(format!("proc-out {canary} {encoded}\n"));
        // Keep a benign unrelated network present so `network ls` always emits a
        // canary-bearing JSON object even after the project network is removed.
        docker.networks.insert(
            "b".repeat(64),
            FakeNetwork {
                id: "b".repeat(64),
                name: "bridge".to_owned(),
                project_label: None,
            },
        );
    }
    let mut replies = vec![h.hello()];
    let digest = h.stage(&bytes(&manifest()));
    replies.push(h.put_secret(90, &digest, 1, canary.as_bytes()));
    // Failing `volume ls` carrying the canary before succeeding Deploy.
    h.docker.lock().unwrap().faults.push(Fault {
        needle: "volume ls",
        result: exited(1, format!("vol-err {canary} {encoded}\n")),
        apply: false,
    });
    replies.push(h.deploy(88, PROJECT, 1, &digest));
    h.settle();
    replies.push(h.svc.handle(ComposeRequest::ComposeOperation {
        id: 7,
        operation_id: op(88),
    }));
    replies.push(h.deploy(1, PROJECT, 2, &digest));
    h.settle();
    replies.push(h.put_secret(91, &digest, 2, canary.as_bytes()));
    h.settle();
    // Failing `container inspect` carrying the canary.
    h.docker.lock().unwrap().faults.push(Fault {
        needle: "container inspect",
        result: exited(1, format!("inspect-err {canary} {encoded}\n")),
        apply: false,
    });
    replies.push(h.lifecycle(89, PROJECT, OperationKind::Start));
    h.settle();
    // Failing `network ls` carrying the canary.
    h.docker.lock().unwrap().faults.push(Fault {
        needle: "network ls",
        result: exited(1, format!("net-err {canary} {encoded}\n")),
        apply: false,
    });
    replies.push(h.lifecycle(94, PROJECT, OperationKind::Down));
    h.settle();
    replies.push(h.svc.handle(ComposeRequest::ComposeOperation {
        id: 8,
        operation_id: op(94),
    }));
    // Succeeding `Down` exercises `network ls` on success path with canary-bearing JSON.
    replies.push(h.lifecycle(95, PROJECT, OperationKind::Down));
    h.settle();
    replies.push(h.put_secret(92, &digest, 1, canary.as_bytes()));
    replies.push(h.put_secret_as(
        93,
        GENERATION + 1,
        PROJECT,
        &digest,
        SLOT,
        5,
        canary.as_bytes(),
    ));
    replies.push(h.put_secret_as(
        96,
        GENERATION,
        PROJECT,
        &digest,
        "other",
        5,
        canary.as_bytes(),
    ));
    replies.push(h.svc.handle(ComposeRequest::ComposeOperation {
        id: 9,
        operation_id: op(91),
    }));
    replies.push(h.svc.handle(ComposeRequest::ComposeStatus {
        id: 10,
        project_id: PROJECT.to_owned(),
    }));
    replies.push(h.svc.handle(ComposeRequest::ComposeLogs {
        id: 11,
        project_id: PROJECT.to_owned(),
        cursor: 0,
        limit: MAX_LOG_ENTRIES,
    }));

    // Verify all three new query commands actually ran with canary active.
    assert!(h.count("volume ls") >= 2);
    assert!(h.count("container inspect") >= 2);
    assert!(h.count("network ls") >= 2);

    let mut haystacks: Vec<String> = replies
        .iter()
        .map(|reply| serde_json::to_string(reply).unwrap() + &format!("{reply:?}"))
        .collect();
    haystacks.extend(h.calls().iter().map(|argv| argv.join(" ")));
    haystacks.extend(
        ENGINE_ENV
            .iter()
            .map(|(name, value)| format!("{name}={value}")),
    );
    let inst2 = secret_instance(SLOT, &digest, 2, GENERATION);
    {
        let disk = h.disk.lock().unwrap();
        haystacks.extend(disk.revisions.values().cloned());
        haystacks.extend(disk.rotations.values().cloned());
        let state_text = String::from_utf8(disk.state.clone().unwrap()).unwrap();
        // Pillar 2: persistent state must not contain any plaintext-derived verifier hash.
        assert!(
            !state_text.contains(&sha256_hex(canary.as_bytes())),
            "state contains plaintext hash"
        );
        haystacks.push(state_text);
        assert_eq!(
            disk.secrets[&(PROJECT.to_owned(), inst2)].0,
            canary.as_bytes(),
            "the tmpfs file holds the value"
        );
    }
    for haystack in &haystacks {
        assert!(!haystack.contains(&canary), "canary leaked: {haystack}");
        assert!(
            !haystack.contains(&encoded),
            "encoded canary leaked: {haystack}"
        );
    }
}

#[test]
fn contract_b_failed_rotation_cannot_reassign_its_epoch_instance() {
    let h = Harness::new();
    let digest = h.deployed();
    let instance = secret_instance(SLOT, &digest, 2, GENERATION);
    let key = (PROJECT.to_owned(), instance);
    h.disk.lock().unwrap().fail_write_rotation = true;
    h.put_secret(91, &digest, 2, b"fixture A");
    h.settle();
    assert_eq!(h.operation(91).failure, Some(FailureCode::StoreFailed));
    assert_eq!(h.count("--force-recreate"), 0);
    let first = h.disk.lock().unwrap().secrets[&key].1;
    h.disk.lock().unwrap().fail_write_rotation = false;
    let calls_before = h.count("--force-recreate");
    let h = Harness::on_shared(Arc::clone(&h.disk), Arc::clone(&h.docker));
    h.hello();
    assert_eq!(
        rejected(&h.put_secret(92, &digest, 2, b"fixture B")),
        Some(RejectCode::SecretConflict)
    );
    h.settle();
    let second = h.disk.lock().unwrap().secrets[&key].1;
    assert_eq!(
        second.ino, first.ino,
        "a new ID replaced the already published epoch-2 inode"
    );
    assert_eq!(h.count("--force-recreate"), calls_before);

    let writes_before = h.disk.lock().unwrap().secret_writes;
    h.put_secret(93, &digest, 2, b"fixture A");
    h.settle();
    assert_eq!(h.operation(93).state, OperationState::Succeeded);
    assert_eq!(h.count("--force-recreate"), calls_before + 1);
    let disk = h.disk.lock().unwrap();
    assert_eq!(disk.secret_writes, writes_before);
    assert_eq!(disk.secrets[&key].1, first);
}

#[test]
fn contract_d_rollback_must_mount_reconciled_epoch() {
    let h = Harness::new();
    let digest = h.deployed();
    let epoch2 = secret_file(PROJECT, &secret_instance(SLOT, &digest, 2, GENERATION));
    h.docker.lock().unwrap().faults.push(Fault {
        needle: "--force-recreate",
        result: RunResult::TimedOut,
        apply: true,
    });
    h.put_secret(91, &digest, 2, b"rotated fixture");
    h.settle();
    assert_eq!(h.operation(91).state, OperationState::Ambiguous);
    h.reconcile(91);
    assert_eq!(h.operation(91).state, OperationState::Succeeded);
    assert_eq!(
        h.docker.lock().unwrap().containers[PROJECT][0].mounts[0].source,
        epoch2
    );

    let mut other = manifest();
    other["services"][0]["environment"][0]["value"] = json!("8099");
    let other_digest = h.stage(&bytes(&other));
    h.put_secret(92, &other_digest, 1, b"fixture for next revision");
    h.docker.lock().unwrap().unhealthy.insert(2);
    h.deploy(2, PROJECT, 2, &other_digest);
    h.settle();
    assert_eq!(h.operation(2).failure, Some(FailureCode::HealthFailed));
    assert_eq!(h.docker.lock().unwrap().containers[PROJECT][0].revision, 1);
    assert_eq!(
        h.docker.lock().unwrap().containers[PROJECT][0].mounts[0].source,
        epoch2,
        "rollback relaunched the prior revision on a superseded secret epoch"
    );
    assert!(h.operation(2).restored_previous);
}

#[test]
fn contract_g_uncertain_receipt_cannot_rebind_operation_id() {
    for fault in [StoreFault::AfterRename, StoreFault::BeforeRename] {
        let h = Harness::new();
        h.hello();
        let digest_a = h.stage(&bytes(&manifest()));
        let mut other = manifest();
        other["services"][0]["environment"][0]["value"] = json!("8099");
        let digest_b = h.stage(&bytes(&other));
        assert!(matches!(
            h.put_secret(90, &digest_a, 1, b"fixture A"),
            ComposeResponse::ComposeSecretAccepted { .. }
        ));

        h.disk.lock().unwrap().save_faults.push_back(Some(fault));
        assert_eq!(
            rejected(&h.put_secret(91, &digest_a, 1, b"fixture A")),
            Some(RejectCode::StoreFailure)
        );
        let persisted = Harness::on(Arc::new(Mutex::new(Disk {
            state: h.disk.lock().unwrap().state.clone(),
            ..Disk::default()
        })));
        persisted.hello();
        if fault == StoreFault::AfterRename {
            assert_eq!(persisted.operation(91).state, OperationState::Succeeded);
        }
        let writes_before = h.disk.lock().unwrap().secret_writes;
        let reply = h.put_secret(91, &digest_b, 1, b"fixture B");
        let disk = h.disk.lock().unwrap();
        let state: Value = serde_json::from_slice(disk.state.as_ref().unwrap()).unwrap();
        let record = state["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["operation_id"] == op(91));
        let reaccepted = matches!(reply, ComposeResponse::ComposeSecretAccepted { digest, .. }
        if digest == digest_b);
        let rebound = record.is_some_and(|op| op["digest"] == digest_b);
        let new_writes = disk.secret_writes - writes_before;
        assert_eq!(
            (reaccepted, rebound, new_writes),
            (false, false, 0),
            "live broker re-accepted the same ID for B, rebound its durable digest, and wrote a file"
        );
    }
}

#[test]
fn contract_g_reconcile_must_checkpoint_visible_rename_before_success() {
    let h = Harness::new();
    h.hello();
    let digest_a = h.stage(&bytes(&manifest()));
    let mut other = manifest();
    other["services"][0]["environment"][0]["value"] = json!("8099");
    let other_input = bytes(&other);
    let digest_b = h.stage(&other_input);
    assert!(matches!(
        h.put_secret(90, &digest_a, 1, b"synthetic A"),
        ComposeResponse::ComposeSecretAccepted { .. }
    ));

    let stable = h.disk.lock().unwrap().save_snapshots.last().cloned();
    h.disk
        .lock()
        .unwrap()
        .save_faults
        .push_back(Some(StoreFault::AfterRename));
    assert_eq!(
        rejected(&h.put_secret(91, &digest_a, 1, b"synthetic A")),
        Some(RejectCode::StoreFailure)
    );
    assert_ne!(
        h.disk.lock().unwrap().state,
        stable,
        "renamed state must be visible"
    );
    {
        let mut disk = h.disk.lock().unwrap();
        // Model a rename visible to readers with the directory sync still failing.
        disk.fail_save_state = true;
    }
    let probe = || rejected(&h.put_secret(92, &"0".repeat(64), 1, b"probe"));
    let before = probe();
    assert_eq!(before, Some(RejectCode::StoreFailure));
    let saves_before = h.disk.lock().unwrap().save_snapshots.len();
    let premature_success = matches!(
        h.reconcile(91),
        ComposeResponse::ComposeOperationState {
            operation: OperationView {
                state: OperationState::Succeeded,
                ..
            },
            ..
        }
    );
    let new_saves = h.disk.lock().unwrap().save_snapshots.len() - saves_before;

    let after = probe();
    // A crash before directory sync may leave the previously durable name.
    let crashed = Harness::on(Arc::new(Mutex::new(Disk {
        state: stable.clone(),
        ..Disk::default()
    })));
    crashed.hello_gen(GENERATION + 1);
    assert_eq!(crashed.stage(&other_input), digest_b);
    assert_eq!(
        rejected(&crashed.put_secret_as(
            90,
            GENERATION + 1,
            PROJECT,
            &digest_b,
            SLOT,
            1,
            b"synthetic B"
        )),
        Some(RejectCode::OperationConflict)
    );
    let accepted_for_b_after_crash = matches!(
        crashed.put_secret_as(91, GENERATION + 1, PROJECT, &digest_b, SLOT, 1, b"synthetic B"),
        ComposeResponse::ComposeSecretAccepted { digest, .. } if digest == digest_b
    );
    assert!(
        !(premature_success && accepted_for_b_after_crash),
        "gate probe before={before:?} after={after:?}; reconcile reported success after {new_saves} successful saves, then a crash losing the unsynced rename let the same ID settle for another digest"
    );

    assert!(!premature_success);
    assert_eq!(after, Some(RejectCode::StoreFailure));
    assert_eq!(new_saves, 0);

    // A process-only restart sees the unsynced rename too. It must stay frozen.
    let restarted = Harness::on(Arc::clone(&h.disk));
    restarted.hello();
    assert_eq!(
        rejected(&restarted.reconcile(91)),
        Some(RejectCode::StoreFailure)
    );
    assert_eq!(
        rejected(&restarted.put_secret(91, &digest_b, 1, b"synthetic B")),
        Some(RejectCode::StoreFailure)
    );

    // Allow the recovery save. A stale, valid read-back still cannot release the gate.
    {
        let mut disk = h.disk.lock().unwrap();
        disk.fail_save_state = false;
        let visible = disk.state.clone();
        disk.load_overrides.extend([visible, stable]);
    }
    assert_eq!(rejected(&h.reconcile(91)), Some(RejectCode::StoreFailure));
    assert_eq!(probe(), Some(RejectCode::StoreFailure));
    assert!(h.disk.lock().unwrap().load_overrides.is_empty());

    // This time the re-read matches. A crash after the success reply must keep ID 91.
    assert!(matches!(
        h.reconcile(91),
        ComposeResponse::ComposeOperationState {
            operation: OperationView {
                state: OperationState::Succeeded,
                ..
            },
            ..
        }
    ));
    assert_eq!(probe(), Some(RejectCode::UnknownRelease));
    let durable = h.disk.lock().unwrap().save_snapshots.last().cloned();
    let crashed = Harness::on(Arc::new(Mutex::new(Disk {
        state: durable,
        ..Disk::default()
    })));
    crashed.hello_gen(GENERATION + 1);
    assert_eq!(crashed.stage(&other_input), digest_b);
    assert_eq!(crashed.operation(91).state, OperationState::Succeeded);
    assert_eq!(
        rejected(&crashed.put_secret_as(
            91,
            GENERATION + 1,
            PROJECT,
            &digest_b,
            SLOT,
            1,
            b"synthetic B"
        )),
        Some(RejectCode::OperationConflict)
    );
}

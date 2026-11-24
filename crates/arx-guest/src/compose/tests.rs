//! Compose broker unit tests with a fake engine and an in-memory store. No
//! Docker, VM, registry or real secret is involved.

use std::collections::{BTreeMap, BTreeSet};
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

struct Fault {
    needle: &'static str,
    result: RunResult,
    apply: bool,
}

#[derive(Clone)]
struct Container {
    id: String,
    service: String,
    state: &'static str,
    revision: u64,
}

#[derive(Default)]
struct Docker {
    images: BTreeSet<String>,
    arch: Option<&'static str>,
    pull_fails: bool,
    volumes: BTreeMap<String, String>,
    containers: BTreeMap<String, Vec<Container>>,
    services: BTreeMap<String, Vec<String>>,
    revision_services: BTreeMap<(String, u64), Vec<String>>,
    next_container_id: u64,
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
    fn fresh_id(&mut self) -> String {
        self.next_container_id += 1;
        format!("{:012x}", self.next_container_id)
    }

    fn declared_services(&self, project: &str, revision: u64) -> Vec<String> {
        self.revision_services
            .get(&(project.to_owned(), revision))
            .or_else(|| self.services.get(project))
            .cloned()
            .unwrap_or_default()
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
                assert_eq!(*file, format!("{dir}/compose.yaml"));
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
                        let new_ids: Vec<(String, String)> = targets
                            .iter()
                            .map(|service| ((*service).to_owned(), self.fresh_id()))
                            .collect();
                        let list = self.containers.entry(project).or_default();
                        for (service, id) in new_ids {
                            list.retain(|container| container.service != service);
                            list.push(Container {
                                id,
                                service,
                                state: "running",
                                revision,
                            });
                        }
                        exited(0, self.noise())
                    }
                    ["up", "--detach", "--no-build", "--pull", "never"] => {
                        let services = self.declared_services(&project, revision);
                        let new_ids: Vec<(String, String)> = services
                            .into_iter()
                            .map(|service| (service, self.fresh_id()))
                            .collect();
                        // Real Docker Compose `up` without `--remove-orphans`
                        // replaces declared services and leaves retired ones.
                        let list = self.containers.entry(project).or_default();
                        for (service, id) in new_ids {
                            list.retain(|container| container.service != service);
                            list.push(Container {
                                id,
                                service,
                                state: "running",
                                revision,
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

/// Secret bytes and their file owner.
type StoredSecret = (Vec<u8>, (u32, u32));

#[derive(Default)]
struct Disk {
    state: Option<Vec<u8>>,
    revisions: BTreeMap<(String, u64), String>,
    secrets: BTreeMap<(String, String), StoredSecret>,
    prepared: BTreeMap<String, (u32, u32)>,
    fail_save_state: bool,
    fail_write_secret: bool,
    secret_writes: usize,
}

struct MemoryStore(Arc<Mutex<Disk>>);

impl ComposeStore for MemoryStore {
    fn load_state(&mut self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.lock().unwrap().state.clone())
    }

    fn save_state(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        let mut disk = self.0.lock().unwrap();
        if disk.fail_save_state {
            return Err(StoreError);
        }
        disk.state = Some(bytes.to_vec());
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

    fn write_secret(
        &mut self,
        project: &str,
        slot: &str,
        owner: (u32, u32),
        bytes: &[u8],
    ) -> Result<(), StoreError> {
        let mut disk = self.0.lock().unwrap();
        if disk.fail_write_secret {
            return Err(StoreError);
        }
        disk.secret_writes += 1;
        disk.secrets.insert(
            (project.to_owned(), slot.to_owned()),
            (bytes.to_vec(), owner),
        );
        Ok(())
    }

    fn has_secret(&mut self, project: &str, slot: &str, owner: (u32, u32)) -> bool {
        self.0
            .lock()
            .unwrap()
            .secrets
            .get(&(project.to_owned(), slot.to_owned()))
            .is_some_and(|(_, stored)| *stored == owner)
    }

    fn prepare_volume(&mut self, name: &str, owner: (u32, u32)) -> Result<(), StoreError> {
        self.0
            .lock()
            .unwrap()
            .prepared
            .insert(name.to_owned(), owner);
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
        let docker = Arc::new(Mutex::new(Docker::default()));
        docker
            .lock()
            .unwrap()
            .services
            .insert(PROJECT.to_owned(), vec!["web".to_owned()]);
        let svc = ComposeService::new(
            Box::new(FakeRunner(Arc::clone(&docker))),
            Box::new(MemoryStore(Arc::clone(&disk))),
            Box::new(FixedClock),
        )
        .unwrap();
        Self { svc, docker, disk }
    }

    fn hello(&self) -> ComposeResponse {
        self.svc.handle(ComposeRequest::ComposeHello {
            version: COMPOSE_PROTOCOL_VERSION,
            challenge: CHALLENGE.to_owned(),
            generation: GENERATION,
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

    fn put_secret(&self, operation: u32, epoch: u64, value: &[u8]) -> ComposeResponse {
        self.put_secret_as(operation, GENERATION, SLOT, epoch, value)
    }

    fn put_secret_as(
        &self,
        operation: u32,
        generation: u64,
        slot: &str,
        epoch: u64,
        value: &[u8],
    ) -> ComposeResponse {
        self.svc.handle(ComposeRequest::ComposeSecretPut {
            id: 4,
            operation_id: op(operation),
            generation,
            project_id: PROJECT.to_owned(),
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
            self.put_secret(90, 1, b"first value"),
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

fn compose_argv(revision: u64, tail: &[&str]) -> Vec<String> {
    let dir = format!("/var/lib/arx-compose/v1/projects/calm-life/r{revision}");
    let file = format!("{dir}/compose.yaml");
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
        file.as_str(),
        "--env-file",
        "/dev/null",
    ]
    .iter()
    .map(|arg| (*arg).to_owned())
    .collect();
    argv.extend(tail.iter().map(|arg| (*arg).to_owned()));
    argv
}

const GOLDEN_YAML: &str = r#"# Generated by arx-guest from a validated project. Layout v1.
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
volumes:
  "arx-v_calm-life_data":
    external: true
    name: "arx-v_calm-life_data"
secrets:
  "admin_password":
    file: "/run/arx-compose/secrets/calm-life/admin_password"
"#;

#[test]
fn render_is_canonical_hardened_and_disables_restart() {
    let project = Project::parse(&bytes(&manifest())).unwrap();
    let yaml = render(&project, 1, 0);
    assert_eq!(yaml, GOLDEN_YAML);
    assert_eq!(render(&project, 1, 0), yaml, "render is deterministic");
    // Another slot moves only the loopback port.
    assert!(render(&project, 1, 3).contains("\"127.0.0.1:20012:8080/tcp\""));
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
    // Pad the manifest with JSON whitespace so it needs three chunks.
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

    // Out of order clears the staging.
    assert!(rejected(&begin(&digest, total, 3)).is_none());
    assert_eq!(
        rejected(&send(&digest, 1, chunk(1))),
        Some(RejectCode::StageOrder)
    );
    assert_eq!(
        rejected(&send(&digest, 0, chunk(0))),
        Some(RejectCode::NoStaging)
    );

    // A short non-final chunk is refused.
    assert!(rejected(&begin(&digest, total, 3)).is_none());
    assert_eq!(
        rejected(&send(&digest, 0, encode_base64(&manifest_bytes[..100]))),
        Some(RejectCode::StageSize)
    );

    // Commit before the last chunk keeps the staging.
    assert!(rejected(&begin(&digest, total, 3)).is_none());
    assert!(rejected(&send(&digest, 0, chunk(0))).is_none());
    assert_eq!(
        rejected(&commit(&digest)),
        Some(RejectCode::StageIncomplete)
    );
    assert!(rejected(&send(&digest, 1, chunk(1))).is_none());

    // A new session clears incomplete staging under the fixed quota.
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

    // The declared digest must match the bytes.
    let wrong = sha256_hex(b"something else");
    assert!(rejected(&begin(&wrong, total, 3)).is_none());
    for index in 0..3 {
        assert!(rejected(&send(&wrong, index, chunk(index as usize))).is_none());
    }
    assert_eq!(rejected(&commit(&wrong)), Some(RejectCode::StageDigest));

    // The exact sequence commits, and a repeat begin is already complete.
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

    // A rejected manifest names its rule and echoes nothing.
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
    h.put_secret(90, 1, b"first value");
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
    let label = "{{index .Labels \"org.arxcovo.volume\"}}";
    let img = image();
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
            global(&["volume", "inspect", "--format", label, volume]),
            global(&[
                "volume",
                "create",
                "--driver",
                "local",
                "--label",
                "org.arxcovo.volume=calm-life/data/1000:1000",
                volume,
            ]),
            global(&["volume", "inspect", "--format", label, volume]),
            global(&["volume", "inspect", "--format", "{{.Mountpoint}}", volume]),
            compose_argv(1, &["up", "--detach", "--no-build", "--pull", "never"]),
            compose_argv(1, &["ps", "--all", "--format", "json"]),
        ]
    );
    assert_eq!(
        h.docker.lock().unwrap().probes,
        vec![(20000, "/healthz".to_owned())]
    );
    {
        let disk = h.disk.lock().unwrap();
        assert_eq!(disk.revisions[&(PROJECT.to_owned(), 1)], GOLDEN_YAML);
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
    // Restart persistence: a fresh broker on the same disk keeps the binding.
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
    for invocation in [
        image_inspect(&images[0]),
        image_pull(&images[0]),
        volume_label("arx-v_calm-life_data"),
        volume_mountpoint("arx-v_calm-life_data"),
        volume_create("arx-v_calm-life_data", "calm-life/data/1000:1000"),
        compose_up(PROJECT, 3),
        compose_recreate(PROJECT, 3, &["web".to_owned()]),
        compose_stop(PROJECT, 3),
        compose_down(PROJECT, 3),
        container_stop("000000000001"),
        container_rm("000000000001"),
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
        h.put_secret(90, 1, b"first value");
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
        let reply = h.svc.handle(ComposeRequest::ComposeReconcile {
            id: 11,
            operation_id: op(1),
        });
        let ComposeResponse::ComposeOperationState { operation, .. } = reply else {
            panic!("{reply:?}");
        };
        let after = h.calls();
        assert_eq!(after.len(), before + 1, "reconcile issued only a query");
        assert_eq!(
            after[before],
            compose_argv(1, &["ps", "--all", "--format", "json"])
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
        assert_eq!(
            rejected(&h.svc.handle(ComposeRequest::ComposeReconcile {
                id: 12,
                operation_id: op(1),
            })),
            Some(RejectCode::NotAmbiguous)
        );
    }
}

#[test]
fn ambiguous_down_is_reconciled_never_rerun() {
    for applied in [true, false] {
        let h = Harness::new();
        h.deployed();
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "/dev/null down",
            result: RunResult::Signaled,
            apply: applied,
        });
        h.lifecycle(2, PROJECT, OperationKind::Down);
        h.settle();
        assert_eq!(h.operation(2).state, OperationState::Ambiguous);
        let reply = h.svc.handle(ComposeRequest::ComposeReconcile {
            id: 11,
            operation_id: op(2),
        });
        let ComposeResponse::ComposeOperationState { operation, .. } = reply else {
            panic!("{reply:?}");
        };
        assert_eq!(h.count("/dev/null down"), 1, "reconcile re-ran down");
        let status = h.status(PROJECT);
        if applied {
            assert_eq!(operation.state, OperationState::Succeeded);
            assert_eq!(status.desired, Desired::Down);
        } else {
            assert_eq!(operation.failure, Some(FailureCode::NotApplied));
            assert_eq!(status.desired, Desired::Started);
            assert!(status.recovery_required, "containers remain");
        }
    }
}

#[test]
fn running_journal_is_persisted_before_effects_and_becomes_ambiguous_after_restart() {
    let h = Harness::new();
    h.hello();
    let digest = h.stage(&bytes(&manifest()));
    h.put_secret(90, 1, b"first value");
    let (release, receiver) = mpsc::channel();
    h.docker.lock().unwrap().block = Some((" up --detach", receiver));
    h.deploy(1, PROJECT, 1, &digest);
    // Wait until the worker is inside the apply step.
    let mut waited = 0;
    while h.operation(1).stage != OperationStage::Applying {
        assert!(waited < 500, "worker never reached apply");
        std::thread::sleep(Duration::from_millis(10));
        waited += 1;
    }
    // The bounded status action answers while the long operation runs.
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
    fresh.put_secret(90, 1, b"first value");
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
        h.put_secret(90, 1, b"first value");
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
fn secret_rotation_replaces_the_file_and_recreates_only_users() {
    let h = Harness::new();
    h.deployed();
    let reply = h.put_secret(91, 2, b"second value");
    let ComposeResponse::ComposeSecretAccepted {
        epoch: 2,
        recreate: Some(recreate),
        ..
    } = reply
    else {
        panic!("{reply:?}");
    };
    assert_eq!(
        (recreate.kind, recreate.state),
        (OperationKind::Recreate, OperationState::Running)
    );
    h.settle();
    assert_eq!(h.operation(91).state, OperationState::Succeeded);
    assert!(h.calls().contains(&compose_argv(
        1,
        &[
            "up",
            "--detach",
            "--no-build",
            "--pull",
            "never",
            "--force-recreate",
            "--no-deps",
            "web"
        ]
    )));
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), SLOT.to_owned())],
        (b"second value".to_vec(), (1000, 1000))
    );
    assert_eq!(
        rejected(&h.put_secret(92, 1, b"old value")),
        Some(RejectCode::StaleEpoch)
    );
    assert_eq!(
        rejected(&h.put_secret_as(93, GENERATION + 1, SLOT, 3, b"value")),
        Some(RejectCode::GenerationMismatch)
    );
    assert_eq!(
        rejected(&h.put_secret_as(94, GENERATION, "other", 3, b"value")),
        Some(RejectCode::UnknownSlot)
    );
    assert_eq!(
        rejected(&h.put_secret(95, 3, &[7; MAX_SECRET + 1])),
        Some(RejectCode::SecretSize)
    );
    // Re-delivery of the current epoch, as after a guest reboot, recreates nothing.
    let recreates = h.count("--force-recreate");
    assert!(matches!(
        h.put_secret(96, 2, b"second value"),
        ComposeResponse::ComposeSecretAccepted { recreate: None, .. }
    ));
    h.settle();
    assert_eq!(h.count("--force-recreate"), recreates);
}

#[test]
fn secret_canary_never_reaches_yaml_argv_env_logs_status_errors_or_state() {
    let canary = format!("arx-canary-{:016x}", 0x5eed_c0de_feed_f00d_u64);
    let encoded = encode_base64(canary.as_bytes());
    let h = Harness::new();
    // Make every fake subprocess emit both raw and base64 canary on stdout
    // (including error exits and extra ps JSON fields).
    h.docker.lock().unwrap().output_canary = Some(format!("proc-out {canary} {encoded}\n"));
    let mut replies = vec![h.hello()];
    let digest = h.stage(&bytes(&manifest()));
    replies.push(h.put_secret(90, 1, canary.as_bytes()));
    replies.push(h.deploy(1, PROJECT, 1, &digest));
    h.settle();
    replies.push(h.put_secret(91, 2, canary.as_bytes()));
    h.settle();
    // Also exercise a failing subprocess that emits the canary on stdout.
    h.docker.lock().unwrap().faults.push(Fault {
        needle: "/dev/null stop",
        result: exited(1, format!("stderr/stdout {canary} {encoded}\n")),
        apply: false,
    });
    replies.push(h.lifecycle(95, PROJECT, OperationKind::Stop));
    h.settle();
    replies.push(h.svc.handle(ComposeRequest::ComposeOperation {
        id: 8,
        operation_id: op(95),
    }));
    replies.push(h.put_secret(92, 1, canary.as_bytes()));
    replies.push(h.put_secret_as(93, GENERATION + 1, SLOT, 5, canary.as_bytes()));
    replies.push(h.put_secret_as(94, GENERATION, "other", 5, canary.as_bytes()));
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
    {
        let disk = h.disk.lock().unwrap();
        haystacks.extend(disk.revisions.values().cloned());
        haystacks.push(String::from_utf8(disk.state.clone().unwrap()).unwrap());
        assert_eq!(
            disk.secrets[&(PROJECT.to_owned(), SLOT.to_owned())].0,
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
    replies.push(h.put_secret(90, 1, b"first value"));
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
fn secret_delivery_journals_before_file_replace_and_rejects_conflicting_replay() {
    let h = Harness::new();
    h.deployed();
    assert_eq!(h.disk.lock().unwrap().secret_writes, 1);

    let reply = h.put_secret(91, 2, b"second value");
    assert!(matches!(
        reply,
        ComposeResponse::ComposeSecretAccepted {
            epoch: 2,
            recreate: Some(_),
            ..
        }
    ));
    h.settle();
    assert_eq!(h.operation(91).state, OperationState::Succeeded);
    assert_eq!(h.disk.lock().unwrap().secret_writes, 2);
    let recreates = h.count("--force-recreate");
    assert_eq!(recreates, 1);

    // Replayed operation ID with different epoch, slot, or content is a conflict
    // and never touches the tmpfs file.
    assert_eq!(
        rejected(&h.put_secret(91, 3, b"third value")),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(
        rejected(&h.put_secret_as(91, GENERATION, "other", 2, b"second value")),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(
        rejected(&h.put_secret(91, 2, b"tampered value")),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(h.disk.lock().unwrap().secret_writes, 2);
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), SLOT.to_owned())].0,
        b"second value"
    );

    // Exact replay returns the journaled view without rewriting the file or recreating.
    assert!(matches!(
        h.put_secret(91, 2, b"second value"),
        ComposeResponse::ComposeSecretAccepted {
            epoch: 2,
            recreate: Some(OperationView {
                state: OperationState::Succeeded,
                ..
            }),
            ..
        }
    ));
    assert_eq!(h.disk.lock().unwrap().secret_writes, 2);
    assert_eq!(h.count("--force-recreate"), recreates);

    // Non-rotation replay with different content also conflicts without touching the file.
    assert_eq!(
        rejected(&h.put_secret(90, 1, b"tampered")),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(h.disk.lock().unwrap().secret_writes, 2);

    // Journal persistence failure leaves the old tmpfs file untouched for both
    // rotation and non-rotation deliveries.
    h.disk.lock().unwrap().fail_save_state = true;
    assert_eq!(
        rejected(&h.put_secret(92, 3, b"third value")),
        Some(RejectCode::StoreFailure)
    );
    assert_eq!(
        rejected(&h.put_secret(93, 2, b"second value")),
        Some(RejectCode::StoreFailure)
    );
    assert_eq!(h.disk.lock().unwrap().secret_writes, 2);
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), SLOT.to_owned())].0,
        b"second value"
    );
    assert_eq!(h.count("--force-recreate"), recreates);

    // Once storage recovers, a fresh rotation succeeds.
    h.disk.lock().unwrap().fail_save_state = false;
    assert!(matches!(
        h.put_secret(94, 3, b"third value"),
        ComposeResponse::ComposeSecretAccepted {
            epoch: 3,
            recreate: Some(_),
            ..
        }
    ));
    h.settle();
    assert_eq!(h.operation(94).state, OperationState::Succeeded);
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), SLOT.to_owned())].0,
        b"third value"
    );
}

#[test]
fn rotation_succeeds_only_when_container_replacement_is_observed() {
    // Timed-out recreate with zero replacements reconciles to Failed + recovery_required.
    let h = Harness::new();
    h.deployed();
    h.docker.lock().unwrap().faults.push(Fault {
        needle: "--force-recreate",
        result: RunResult::TimedOut,
        apply: false,
    });
    h.put_secret(91, 2, b"second value");
    h.settle();
    assert_eq!(h.operation(91).state, OperationState::Ambiguous);
    h.svc.handle(ComposeRequest::ComposeReconcile {
        id: 17,
        operation_id: op(91),
    });
    assert_eq!(h.operation(91).state, OperationState::Failed);
    assert_eq!(h.operation(91).failure, Some(FailureCode::NotApplied));
    assert!(h.status(PROJECT).recovery_required);

    // Even an exit-0 recreate that leaves the old container ID in place fails.
    let h_noop = Harness::new();
    h_noop.deployed();
    h_noop.docker.lock().unwrap().faults.push(Fault {
        needle: "--force-recreate",
        result: exited(0, ""),
        apply: false,
    });
    h_noop.put_secret(91, 2, b"second value");
    h_noop.settle();
    assert_eq!(h_noop.operation(91).state, OperationState::Failed);
    assert_eq!(
        h_noop.operation(91).failure,
        Some(FailureCode::HealthFailed)
    );
    assert!(h_noop.status(PROJECT).recovery_required);

    // Timed-out recreate after actual container replacement reconciles to Succeeded.
    let h_applied = Harness::new();
    h_applied.deployed();
    h_applied.docker.lock().unwrap().faults.push(Fault {
        needle: "--force-recreate",
        result: RunResult::TimedOut,
        apply: true,
    });
    h_applied.put_secret(91, 2, b"second value");
    h_applied.settle();
    assert_eq!(h_applied.operation(91).state, OperationState::Ambiguous);
    h_applied.svc.handle(ComposeRequest::ComposeReconcile {
        id: 18,
        operation_id: op(91),
    });
    assert_eq!(h_applied.operation(91).state, OperationState::Succeeded);
    assert!(!h_applied.status(PROJECT).recovery_required);
}

#[test]
fn staged_release_can_receive_new_secret_slot_and_changed_owner() {
    let h = Harness::new();
    h.deployed();

    // Revision 2 declares a brand-new secret slot.
    let mut rev2 = manifest();
    rev2["secrets"] = json!(["next_slot"]);
    rev2["services"][0]["secrets"] = json!(["next_slot"]);
    let digest2 = h.stage(&bytes(&rev2));
    assert_eq!(
        rejected(&h.deploy(2, PROJECT, 2, &digest2)),
        Some(RejectCode::SecretMissing)
    );
    assert!(matches!(
        h.put_secret_as(91, GENERATION, "next_slot", 1, b"next value"),
        ComposeResponse::ComposeSecretAccepted {
            epoch: 1,
            recreate: None,
            ..
        }
    ));
    assert_eq!(h.count("--force-recreate"), 0);
    h.deploy(2, PROJECT, 2, &digest2);
    h.settle();
    assert_eq!(h.operation(2).state, OperationState::Succeeded);

    // Revision 3 changes the owner of `next_slot` from 1000:1000 to 2000:2000.
    let mut rev3 = rev2;
    rev3["services"][0]["uid"] = json!(2000);
    rev3["services"][0]["gid"] = json!(2000);
    rev3["services"][0]["mounts"] = json!([]);
    rev3["volumes"] = json!([]);
    let digest3 = h.stage(&bytes(&rev3));
    assert_eq!(
        rejected(&h.deploy(3, PROJECT, 3, &digest3)),
        Some(RejectCode::SecretOwnerMismatch)
    );
    assert!(matches!(
        h.put_secret_as(92, GENERATION, "next_slot", 2, b"new owner value"),
        ComposeResponse::ComposeSecretAccepted {
            epoch: 2,
            recreate: None,
            ..
        }
    ));
    assert_eq!(h.count("--force-recreate"), 0);
    assert_eq!(
        h.disk.lock().unwrap().secrets[&(PROJECT.to_owned(), "next_slot".to_owned())].1,
        (2000, 2000)
    );
    h.deploy(3, PROJECT, 3, &digest3);
    h.settle();
    assert_eq!(h.operation(3).state, OperationState::Succeeded);
}

#[test]
fn deploy_and_down_cover_retired_attributed_services() {
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
    h.put_secret(90, 1, b"first value");
    h.deploy(1, PROJECT, 1, &digest1);
    h.settle();
    assert_eq!(h.operation(1).state, OperationState::Succeeded);
    assert_eq!(h.docker.lock().unwrap().containers[PROJECT].len(), 2);

    // Revision 2 retires `worker`: deploy stops and removes the retired container.
    let digest2 = h.stage(&bytes(&manifest()));
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

    // Inject a lingering attributed retired container and test Down + Reconcile:
    // `apply: true` removes the declared `web` container while leaving only the
    // retired attributed container running, so reconcile must still fail.
    h.docker
        .lock()
        .unwrap()
        .containers
        .entry(PROJECT.to_owned())
        .or_default()
        .push(Container {
            id: "00000000abcd".to_owned(),
            service: "retired".to_owned(),
            state: "running",
            revision: 1,
        });
    h.docker.lock().unwrap().faults.push(Fault {
        needle: "/dev/null down",
        result: RunResult::TimedOut,
        apply: true,
    });
    h.lifecycle(3, PROJECT, OperationKind::Down);
    h.settle();
    assert_eq!(h.operation(3).state, OperationState::Ambiguous);
    h.svc.handle(ComposeRequest::ComposeReconcile {
        id: 17,
        operation_id: op(3),
    });
    assert_eq!(h.operation(3).state, OperationState::Failed);
    assert_eq!(h.operation(3).failure, Some(FailureCode::NotApplied));
    assert!(h.status(PROJECT).recovery_required);

    // Normal Down removes both declared and retired attributed containers without
    // `--remove-orphans` or volume flags.
    h.lifecycle(4, PROJECT, OperationKind::Down);
    h.settle();
    assert_eq!(h.operation(4).state, OperationState::Succeeded);
    assert!(h.docker.lock().unwrap().containers[PROJECT].is_empty());
    assert_eq!(h.count("--remove-orphans"), 0);

    // If removing a lingering attributed container fails, Down never succeeds.
    h.docker
        .lock()
        .unwrap()
        .containers
        .entry(PROJECT.to_owned())
        .or_default()
        .push(Container {
            id: "00000000abce".to_owned(),
            service: "retired".to_owned(),
            state: "running",
            revision: 1,
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
}

#[test]
fn volume_cap_holds_under_repeated_failed_preparations() {
    let h = Harness::new();
    h.docker
        .lock()
        .unwrap()
        .services
        .insert(PROJECT.to_owned(), vec!["app".to_owned()]);
    h.hello();
    for revision in 1..=9u64 {
        let slot = format!("data{revision}");
        let digest = h.stage(&bytes(&plain(PROJECT, &[&slot])));
        h.docker.lock().unwrap().faults.push(Fault {
            needle: "{{.Mountpoint}}",
            result: exited(1, ""),
            apply: false,
        });
        let reply = h.deploy(100 + revision as u32, PROJECT, revision, &digest);
        if revision <= MAX_VOLUMES as u64 {
            assert!(
                matches!(reply, ComposeResponse::ComposeOperationState { .. }),
                "rev {revision}: {reply:?}"
            );
            h.settle();
            assert_eq!(
                h.operation(100 + revision as u32).failure,
                Some(FailureCode::VolumeFailed)
            );
        } else {
            assert_eq!(rejected(&reply), Some(RejectCode::VolumeLimit));
        }
    }
    assert_eq!(h.docker.lock().unwrap().volumes.len(), MAX_VOLUMES);
    // Revision 9 was rejected at admission, so its injected mountpoint fault
    // was never reached; clear it before testing retry of an already-charged volume.
    {
        let mut docker = h.docker.lock().unwrap();
        assert_eq!(docker.faults.len(), 1);
        docker.faults.clear();
    }

    // Retrying an already-charged volume succeeds without double-charging.
    let digest = h.stage(&bytes(&plain(PROJECT, &["data1"])));
    h.deploy(200, PROJECT, 10, &digest);
    h.settle();
    assert_eq!(h.operation(200).state, OperationState::Succeeded);
    assert_eq!(h.docker.lock().unwrap().volumes.len(), MAX_VOLUMES);
}

#[test]
fn failed_deploy_preserves_stopped_and_down_lifecycle_states() {
    // Deliberately stopped release stays stopped after an unhealthy revision 2.
    let h = Harness::new();
    let digest = h.deployed();
    h.lifecycle(10, PROJECT, OperationKind::Stop);
    h.settle();
    assert_eq!(h.operation(10).state, OperationState::Succeeded);
    h.docker.lock().unwrap().unhealthy.insert(2);
    let before = h.calls().len();
    h.deploy(2, PROJECT, 2, &digest);
    h.settle();
    assert_eq!(h.operation(2).state, OperationState::Failed);
    assert_eq!(h.operation(2).failure, Some(FailureCode::HealthFailed));
    let up1 = compose_argv(1, &["up", "--detach", "--no-build", "--pull", "never"]);
    assert!(
        !h.calls()[before..].contains(&up1),
        "failed deploy restarted the stopped revision"
    );
    assert!(
        h.docker.lock().unwrap().containers[PROJECT]
            .iter()
            .all(|c| c.state != "running"),
        "a container remained running after rollback to Stopped"
    );
    let status = h.status(PROJECT);
    assert_eq!(status.desired, Desired::Stopped);
    assert_eq!(status.committed_revision, Some(1));
    assert!(!status.recovery_required);

    // Deliberately down release stays down after an unhealthy revision 2.
    let h_down = Harness::new();
    let digest = h_down.deployed();
    h_down.lifecycle(10, PROJECT, OperationKind::Down);
    h_down.settle();
    assert_eq!(h_down.operation(10).state, OperationState::Succeeded);
    h_down.docker.lock().unwrap().unhealthy.insert(2);
    let before = h_down.calls().len();
    h_down.deploy(2, PROJECT, 2, &digest);
    h_down.settle();
    assert_eq!(h_down.operation(2).state, OperationState::Failed);
    assert!(
        !h_down.calls()[before..].contains(&up1),
        "failed deploy restarted a down revision"
    );
    assert!(
        h_down
            .docker
            .lock()
            .unwrap()
            .containers
            .get(PROJECT)
            .is_none_or(Vec::is_empty),
        "containers remained after rollback to Down"
    );
    let status = h_down.status(PROJECT);
    assert_eq!(status.desired, Desired::Down);
    assert_eq!(status.committed_revision, Some(1));
    assert!(!status.recovery_required);
}

#[test]
fn deploy_replay_binds_digest_and_tombstones_survive_trimming() {
    let h = Harness::new();
    let digest1 = h.deployed();

    let mut changed = manifest();
    changed["services"][0]["environment"][0]["value"] = json!("8081");
    let digest2 = h.stage(&bytes(&changed));
    assert_ne!(digest1, digest2);
    assert_eq!(
        rejected(&h.deploy(1, PROJECT, 1, &digest2)),
        Some(RejectCode::OperationConflict)
    );

    // Fill past MAX_JOURNAL so op 1 is evicted into tombstones.
    for i in 0..MAX_JOURNAL as u32 {
        let kind = if i % 2 == 0 {
            OperationKind::Stop
        } else {
            OperationKind::Start
        };
        let reply = h.lifecycle(100 + i, PROJECT, kind);
        assert!(
            matches!(reply, ComposeResponse::ComposeOperationState { .. }),
            "op {i}: {reply:?}"
        );
        h.settle();
        assert_eq!(h.operation(100 + i).state, OperationState::Succeeded);
    }

    let before = h.calls().len();
    assert_eq!(
        rejected(&h.deploy(1, PROJECT, 1, &digest1)),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(
        rejected(&h.lifecycle(1, PROJECT, OperationKind::Stop)),
        Some(RejectCode::OperationConflict)
    );
    assert_eq!(h.calls().len(), before, "evicted operation ID re-executed");

    // Tombstones persist across restart.
    let restarted = Harness::on(Arc::clone(&h.disk));
    restarted.hello();
    assert_eq!(
        rejected(&restarted.lifecycle(1, PROJECT, OperationKind::Stop)),
        Some(RejectCode::OperationConflict)
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

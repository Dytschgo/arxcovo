//! The guest Compose broker. Requests arrive serially from one session; long
//! physical operations run on one worker thread so the session can answer the
//! bounded status action meanwhile. At most one operation runs per VM.
//!
//! Idempotence: every physical request carries a host operation ID. A known
//! ID returns its journaled state and never repeats an effect; a known ID with
//! different parameters is a conflict. The journal entry is persisted as
//! Running before the first effect. A Running entry found at load, a deadline
//! kill or an unexplained signal makes the operation Ambiguous, and an
//! Ambiguous operation blocks its project until reconcile has queried the
//! observed state. Reconcile never re-runs the effect.

use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::codec::{SecretText, decode_secret, sha256_hex, wipe};
use super::engine::{
    self, Clock, CommandRunner, HEALTH_ATTEMPTS, HEALTH_INTERVAL, RunResult, parse_ps,
};
use super::layout::{self, loopback_port, volume_name, volume_token};
use super::manifest::{MAX_PROJECTS, MAX_VOLUMES, Project, ProjectManifest, VolumeSpec};
use super::render::render;
use super::wire::{
    COMPOSE_CAPABILITIES, ComposeRequest, ComposeResponse, Desired, FailureCode, HealthState,
    LogEntry, LogEvent, MAX_LOG_PAGE, MAX_SECRET, Observation, OperationKind, OperationStage,
    OperationState, OperationView, ProjectView, RejectCode, STAGE_CHUNK, STAGE_TOTAL, ServiceState,
    ServiceView,
};

const STATE_VERSION: u32 = 1;
/// Committed staged releases held for deploy. Older ones are dropped first.
pub const MAX_STAGED: usize = 8;
pub const MAX_JOURNAL: usize = 64;
pub const MAX_LOG: usize = 128;
pub const MAX_STATE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreError;

/// Root-owned guest storage. The Linux store is fd-relative and no-follow and
/// is **unverified on real hardware**; tests use an in-memory fake.
pub trait ComposeStore: Send {
    fn load_state(&mut self) -> Result<Option<Vec<u8>>, StoreError>;
    fn save_state(&mut self, bytes: &[u8]) -> Result<(), StoreError>;
    fn write_revision(
        &mut self,
        project: &str,
        revision: u64,
        yaml: &str,
    ) -> Result<(), StoreError>;
    /// Atomically replaces the slot's tmpfs file, owned by `owner`, mode 0400.
    fn write_secret(
        &mut self,
        project: &str,
        slot: &str,
        owner: (u32, u32),
        bytes: &[u8],
    ) -> Result<(), StoreError>;
    fn has_secret(&mut self, project: &str, slot: &str, owner: (u32, u32)) -> bool;
    /// Sets the owner of a broker-created volume's empty data root.
    fn prepare_volume(&mut self, name: &str, owner: (u32, u32)) -> Result<(), StoreError>;
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    revision: u64,
    digest: String,
    manifest: ProjectManifest,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectRecord {
    slot: u8,
    desired: Desired,
    committed: Option<Revision>,
    previous: Option<Revision>,
    pending: Option<Revision>,
    last_rendered: Option<u64>,
    recovery_required: bool,
    observed: Vec<ServiceView>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VolumeRecord {
    project: String,
    volume: String,
    uid: u32,
    gid: u32,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretRecord {
    epoch: u64,
    uid: u32,
    gid: u32,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationRecord {
    operation_id: String,
    kind: OperationKind,
    project_id: String,
    revision: u64,
    generation: u64,
    state: OperationState,
    stage: OperationStage,
    failure: Option<FailureCode>,
    restored_previous: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    projects: BTreeMap<String, ProjectRecord>,
    volumes: BTreeMap<String, VolumeRecord>,
    operations: Vec<OperationRecord>,
    secrets: BTreeMap<String, SecretRecord>,
    log: VecDeque<LogEntry>,
    next_log_seq: u64,
}

impl State {
    fn new() -> Self {
        Self {
            version: STATE_VERSION,
            projects: BTreeMap::new(),
            volumes: BTreeMap::new(),
            operations: Vec::new(),
            secrets: BTreeMap::new(),
            log: VecDeque::new(),
            next_log_seq: 1,
        }
    }

    fn load(bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.len() > MAX_STATE_BYTES {
            return Err(StoreError);
        }
        let mut state: Self = serde_json::from_slice(bytes).map_err(|_| StoreError)?;
        if state.version != STATE_VERSION
            || state.projects.len() > MAX_PROJECTS
            || state.volumes.len() > MAX_VOLUMES
            || state.operations.len() > MAX_JOURNAL
            || state.log.len() > MAX_LOG
        {
            return Err(StoreError);
        }
        for record in state.projects.values() {
            for revision in [&record.committed, &record.previous, &record.pending]
                .into_iter()
                .flatten()
            {
                Project::validate(revision.manifest.clone()).map_err(|_| StoreError)?;
            }
        }
        // A Running entry outlived its worker: the effect is now ambiguous.
        for operation in &mut state.operations {
            if operation.state == OperationState::Running {
                operation.state = OperationState::Ambiguous;
            }
        }
        Ok(state)
    }
}

struct Staging {
    digest: String,
    total: usize,
    chunks: u32,
    next: u32,
    bytes: Vec<u8>,
}

impl Drop for Staging {
    fn drop(&mut self) {
        wipe(&mut self.bytes);
    }
}

struct Staged {
    digest: String,
    project: Project,
}

struct Inner {
    state: State,
    staging: Option<Staging>,
    staged: VecDeque<Staged>,
    running: Option<String>,
    generation: Option<u64>,
}

struct Shared {
    inner: Mutex<Inner>,
    idle: Condvar,
    store: Mutex<Box<dyn ComposeStore>>,
    runner: Mutex<Box<dyn CommandRunner>>,
    clock: Box<dyn Clock>,
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn reject(id: u32, code: RejectCode) -> ComposeResponse {
    ComposeResponse::ComposeRejected {
        id,
        code,
        rule: None,
    }
}

fn view(operation: &OperationRecord) -> OperationView {
    OperationView {
        operation_id: operation.operation_id.clone(),
        kind: operation.kind,
        project_id: operation.project_id.clone(),
        revision: operation.revision,
        state: operation.state,
        stage: operation.stage,
        failure: operation.failure,
        restored_previous: operation.restored_previous,
    }
}

fn operation_state(id: u32, operation: &OperationRecord) -> ComposeResponse {
    ComposeResponse::ComposeOperationState {
        id,
        operation: view(operation),
    }
}

fn secret_key(project: &str, slot: &str) -> String {
    format!("{project}/{slot}")
}

/// The physical work of one operation, fixed at admission.
enum Plan {
    Deploy {
        project: Project,
        slot: u8,
        revision: u64,
        prior: Option<(Project, u64)>,
    },
    Start {
        project: Project,
        slot: u8,
        revision: u64,
    },
    Stop {
        project: String,
        revision: u64,
    },
    Down {
        project: String,
        revision: u64,
    },
    Recreate {
        project: Project,
        slot: u8,
        revision: u64,
        services: Vec<String>,
    },
}

/// How a finished operation changes the journal and its project.
struct Settlement {
    state: OperationState,
    failure: Option<FailureCode>,
    restored: bool,
    desired: Option<Desired>,
    promote: bool,
    clear_pending: bool,
    recovery_required: Option<bool>,
    observed: Option<Vec<ServiceView>>,
    event: LogEvent,
}

impl Settlement {
    fn succeeded(event: LogEvent, desired: Desired) -> Self {
        Self {
            state: OperationState::Succeeded,
            failure: None,
            restored: false,
            desired: Some(desired),
            promote: false,
            clear_pending: false,
            recovery_required: Some(false),
            observed: None,
            event,
        }
    }

    fn failed(code: FailureCode) -> Self {
        Self {
            state: OperationState::Failed,
            failure: Some(code),
            restored: false,
            desired: None,
            promote: false,
            clear_pending: false,
            recovery_required: None,
            observed: None,
            event: LogEvent::OperationFailed,
        }
    }

    fn ambiguous() -> Self {
        Self {
            state: OperationState::Ambiguous,
            failure: None,
            restored: false,
            desired: None,
            promote: false,
            clear_pending: false,
            recovery_required: None,
            observed: None,
            event: LogEvent::OperationAmbiguous,
        }
    }

    fn observed(mut self, views: Vec<ServiceView>) -> Self {
        self.observed = Some(views);
        self
    }
}

enum Step {
    Failed(FailureCode),
    Ambiguous,
}

impl From<Step> for Settlement {
    fn from(step: Step) -> Self {
        match step {
            Step::Failed(code) => Settlement::failed(code),
            Step::Ambiguous => Settlement::ambiguous(),
        }
    }
}

/// The guest Compose broker. Cloning is not offered; one service serves the
/// guest process across sessions.
pub struct ComposeService {
    shared: Arc<Shared>,
}

impl ComposeService {
    pub fn new(
        runner: Box<dyn CommandRunner>,
        mut store: Box<dyn ComposeStore>,
        clock: Box<dyn Clock>,
    ) -> Result<Self, StoreError> {
        let state = match store.load_state()? {
            Some(bytes) => State::load(&bytes)?,
            None => State::new(),
        };
        Ok(Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    state,
                    staging: None,
                    staged: VecDeque::new(),
                    running: None,
                    generation: None,
                }),
                idle: Condvar::new(),
                store: Mutex::new(store),
                runner: Mutex::new(runner),
                clock,
            }),
        })
    }

    /// Waits until no operation is running. Returns false at the timeout.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut inner = lock(&self.shared.inner);
        while inner.running.is_some() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            inner = self
                .shared
                .idle
                .wait_timeout(inner, left)
                .map(|(guard, _)| guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner().0);
        }
        true
    }

    pub fn handle(&self, request: ComposeRequest) -> ComposeResponse {
        match request {
            ComposeRequest::ComposeHello {
                version,
                challenge,
                generation,
                ..
            } => self.hello(version, challenge, generation),
            ComposeRequest::ComposeStageBegin {
                id,
                digest,
                total_bytes,
                chunks,
            } => self.stage_begin(id, digest, total_bytes, chunks),
            ComposeRequest::ComposeStageChunk {
                id,
                digest,
                index,
                data,
            } => self.stage_chunk(id, &digest, index, &data),
            ComposeRequest::ComposeStageCommit { id, digest } => self.stage_commit(id, &digest),
            ComposeRequest::ComposeDeploy {
                id,
                operation_id,
                project_id,
                revision,
                digest,
            } => self.deploy(id, operation_id, project_id, revision, &digest),
            ComposeRequest::ComposeStart {
                id,
                operation_id,
                project_id,
            } => self.start(id, operation_id, project_id),
            ComposeRequest::ComposeStop {
                id,
                operation_id,
                project_id,
            } => self.stop_or_down(id, operation_id, project_id, OperationKind::Stop),
            ComposeRequest::ComposeDown {
                id,
                operation_id,
                project_id,
            } => self.stop_or_down(id, operation_id, project_id, OperationKind::Down),
            ComposeRequest::ComposeOperation { id, operation_id } => {
                let inner = lock(&self.shared.inner);
                match find(&inner, &operation_id) {
                    Some(operation) => operation_state(id, operation),
                    None => reject(id, RejectCode::UnknownOperation),
                }
            }
            ComposeRequest::ComposeReconcile { id, operation_id } => {
                self.reconcile(id, &operation_id)
            }
            ComposeRequest::ComposeStatus { id, project_id } => self.status(id, &project_id),
            ComposeRequest::ComposeLogs {
                id,
                project_id,
                cursor,
                limit,
            } => self.logs(id, &project_id, cursor, limit as usize),
            ComposeRequest::ComposeSecretPut {
                id,
                operation_id,
                generation,
                project_id,
                slot,
                epoch,
                data,
            } => self.secret_put(id, operation_id, generation, project_id, slot, epoch, data),
        }
    }

    fn persist(&self, inner: &Inner) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec(&inner.state).map_err(|_| StoreError)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(StoreError);
        }
        lock(&self.shared.store).save_state(&bytes)
    }

    fn log(
        &self,
        inner: &mut Inner,
        project: &str,
        event: LogEvent,
        service: Option<&str>,
        failure: Option<FailureCode>,
    ) {
        push_log(&self.shared, inner, project, event, service, failure);
    }

    fn hello(&self, version: u32, challenge: String, generation: u64) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        // New-session reconciliation: incomplete staging never spans sessions.
        let cleared = u32::from(inner.staging.take().is_some());
        inner.generation = Some(generation);
        let ambiguous = inner
            .state
            .operations
            .iter()
            .filter(|operation| operation.state == OperationState::Ambiguous)
            .count() as u32;
        ComposeResponse::ComposeHelloAck {
            version,
            challenge,
            generation,
            capabilities: COMPOSE_CAPABILITIES.map(str::to_owned).to_vec(),
            cleared_staging: cleared,
            ambiguous_operations: ambiguous,
        }
    }

    fn stage_begin(&self, id: u32, digest: String, total: u32, chunks: u32) -> ComposeResponse {
        let total = total as usize;
        if !(1..=STAGE_TOTAL).contains(&total) || chunks as usize != total.div_ceil(STAGE_CHUNK) {
            return reject(id, RejectCode::StageSize);
        }
        let mut inner = lock(&self.shared.inner);
        // The quota holds one incomplete staging; a new one replaces it.
        inner.staging = None;
        if inner.staged.iter().any(|staged| staged.digest == digest) {
            return ComposeResponse::ComposeStageProgress {
                id,
                digest,
                received_bytes: total as u32,
                next_index: chunks,
            };
        }
        inner.staging = Some(Staging {
            digest: digest.clone(),
            total,
            chunks,
            next: 0,
            bytes: Vec::with_capacity(total),
        });
        ComposeResponse::ComposeStageProgress {
            id,
            digest,
            received_bytes: 0,
            next_index: 0,
        }
    }

    fn stage_chunk(&self, id: u32, digest: &str, index: u32, data: &str) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        let Some(staging) = inner.staging.as_mut().filter(|s| s.digest == digest) else {
            return reject(id, RejectCode::NoStaging);
        };
        if index != staging.next || index >= staging.chunks {
            inner.staging = None;
            return reject(id, RejectCode::StageOrder);
        }
        let offset = index as usize * STAGE_CHUNK;
        let expected = (staging.total - offset).min(STAGE_CHUNK);
        let Some(mut decoded) = super::codec::decode_base64(data, STAGE_CHUNK) else {
            inner.staging = None;
            return reject(id, RejectCode::StageSize);
        };
        if decoded.len() != expected {
            wipe(&mut decoded);
            inner.staging = None;
            return reject(id, RejectCode::StageSize);
        }
        staging.bytes.extend_from_slice(&decoded);
        wipe(&mut decoded);
        staging.next += 1;
        ComposeResponse::ComposeStageProgress {
            id,
            digest: digest.to_owned(),
            received_bytes: staging.bytes.len() as u32,
            next_index: staging.next,
        }
    }

    fn stage_commit(&self, id: u32, digest: &str) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        if let Some(staged) = inner.staged.iter().find(|staged| staged.digest == digest) {
            return ComposeResponse::ComposeReleaseStaged {
                id,
                digest: digest.to_owned(),
                project_id: staged.project.id().to_owned(),
            };
        }
        let Some(staging) = inner.staging.take_if(|s| s.digest == digest) else {
            return reject(id, RejectCode::NoStaging);
        };
        if staging.next != staging.chunks || staging.bytes.len() != staging.total {
            inner.staging = Some(staging);
            return reject(id, RejectCode::StageIncomplete);
        }
        if sha256_hex(&staging.bytes) != digest {
            return reject(id, RejectCode::StageDigest);
        }
        let project = match Project::parse(&staging.bytes) {
            Ok(project) => project,
            Err(rule) => {
                return ComposeResponse::ComposeRejected {
                    id,
                    code: RejectCode::ManifestRejected,
                    rule: Some(rule),
                };
            }
        };
        drop(staging);
        let project_id = project.id().to_owned();
        if inner.staged.len() >= MAX_STAGED {
            inner.staged.pop_front();
        }
        inner.staged.push_back(Staged {
            digest: digest.to_owned(),
            project,
        });
        ComposeResponse::ComposeReleaseStaged {
            id,
            digest: digest.to_owned(),
            project_id,
        }
    }

    fn deploy(
        &self,
        id: u32,
        operation_id: String,
        project_id: String,
        revision: u64,
        digest: &str,
    ) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        if let Some(response) = existing(
            &inner,
            id,
            &operation_id,
            OperationKind::Deploy,
            &project_id,
            Some(revision),
        ) {
            return response;
        }
        if let Some(code) = gate(&inner, &project_id) {
            return reject(id, code);
        }
        let Some(staged) = inner.staged.iter().find(|staged| staged.digest == digest) else {
            return reject(id, RejectCode::UnknownRelease);
        };
        let project = staged.project.clone();
        if project.id() != project_id {
            return reject(id, RejectCode::ProjectMismatch);
        }
        let (slot, prior) = match inner.state.projects.get(&project_id) {
            Some(record) => {
                let newest = [&record.committed, &record.pending]
                    .into_iter()
                    .flatten()
                    .map(|revision| revision.revision)
                    .chain(record.last_rendered)
                    .max();
                if newest.is_some_and(|newest| revision <= newest) {
                    return reject(id, RejectCode::RevisionNotNewer);
                }
                let prior = record.committed.as_ref().map(|committed| {
                    (
                        Project::validate(committed.manifest.clone())
                            .expect("state manifests are validated at load"),
                        committed.revision,
                    )
                });
                (record.slot, prior)
            }
            None => {
                if inner.state.projects.len() >= MAX_PROJECTS {
                    return reject(id, RejectCode::ProjectLimit);
                }
                let used: Vec<u8> = inner.state.projects.values().map(|r| r.slot).collect();
                let slot = (0..MAX_PROJECTS as u8)
                    .find(|slot| !used.contains(slot))
                    .expect("fewer than eight projects leave a slot");
                (slot, None)
            }
        };
        if let Some(code) = check_volumes(&inner.state, &project) {
            return reject(id, code);
        }
        if let Some(code) = self.check_secrets(&inner.state, &project) {
            return reject(id, code);
        }
        let target = Revision {
            revision,
            digest: digest.to_owned(),
            manifest: project.manifest().clone(),
        };
        let created = !inner.state.projects.contains_key(&project_id);
        let record = inner
            .state
            .projects
            .entry(project_id.clone())
            .or_insert_with(|| ProjectRecord {
                slot,
                desired: Desired::None,
                committed: None,
                previous: None,
                pending: None,
                last_rendered: None,
                recovery_required: false,
                observed: Vec::new(),
            });
        let old_pending = record.pending.replace(target);
        let plan = Plan::Deploy {
            project,
            slot,
            revision,
            prior,
        };
        match self.launch(
            &mut inner,
            operation_id,
            OperationKind::Deploy,
            &project_id,
            revision,
            plan,
        ) {
            Ok(response) => with_id(response, id),
            Err(code) => {
                if created {
                    inner.state.projects.remove(&project_id);
                } else if let Some(record) = inner.state.projects.get_mut(&project_id) {
                    record.pending = old_pending;
                }
                reject(id, code)
            }
        }
    }

    fn start(&self, id: u32, operation_id: String, project_id: String) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        if let Some(response) = existing(
            &inner,
            id,
            &operation_id,
            OperationKind::Start,
            &project_id,
            None,
        ) {
            return response;
        }
        if let Some(code) = gate(&inner, &project_id) {
            return reject(id, code);
        }
        let Some(record) = inner.state.projects.get(&project_id) else {
            return reject(id, RejectCode::UnknownProject);
        };
        let Some(committed) = &record.committed else {
            return reject(id, RejectCode::NotCommitted);
        };
        let project =
            Project::validate(committed.manifest.clone()).expect("state manifests are validated");
        let (slot, revision) = (record.slot, committed.revision);
        if let Some(code) = self.check_secrets(&inner.state, &project) {
            return reject(id, code);
        }
        let plan = Plan::Start {
            project,
            slot,
            revision,
        };
        match self.launch(
            &mut inner,
            operation_id,
            OperationKind::Start,
            &project_id,
            revision,
            plan,
        ) {
            Ok(response) => with_id(response, id),
            Err(code) => reject(id, code),
        }
    }

    fn stop_or_down(
        &self,
        id: u32,
        operation_id: String,
        project_id: String,
        kind: OperationKind,
    ) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        if let Some(response) = existing(&inner, id, &operation_id, kind, &project_id, None) {
            return response;
        }
        if let Some(code) = gate(&inner, &project_id) {
            return reject(id, code);
        }
        let Some(record) = inner.state.projects.get(&project_id) else {
            return reject(id, RejectCode::UnknownProject);
        };
        let Some(revision) = record
            .committed
            .as_ref()
            .map(|committed| committed.revision)
            .or(record.last_rendered)
        else {
            return reject(id, RejectCode::NotCommitted);
        };
        let plan = if kind == OperationKind::Stop {
            Plan::Stop {
                project: project_id.clone(),
                revision,
            }
        } else {
            Plan::Down {
                project: project_id.clone(),
                revision,
            }
        };
        match self.launch(&mut inner, operation_id, kind, &project_id, revision, plan) {
            Ok(response) => with_id(response, id),
            Err(code) => reject(id, code),
        }
    }

    /// Journals a Running entry, persists it, then starts the worker. The
    /// returned reply is built before the worker runs, so it is Running.
    fn launch(
        &self,
        inner: &mut MutexGuard<'_, Inner>,
        operation_id: String,
        kind: OperationKind,
        project_id: &str,
        revision: u64,
        plan: Plan,
    ) -> Result<ComposeResponse, RejectCode> {
        trim_journal(&mut inner.state.operations);
        if inner.state.operations.len() >= MAX_JOURNAL {
            return Err(RejectCode::JournalFull);
        }
        let record = OperationRecord {
            operation_id: operation_id.clone(),
            kind,
            project_id: project_id.to_owned(),
            revision,
            generation: inner.generation.unwrap_or(0),
            state: OperationState::Running,
            stage: OperationStage::Queued,
            failure: None,
            restored_previous: false,
        };
        let response = operation_state(1, &record);
        inner.state.operations.push(record);
        if self.persist(inner).is_err() {
            inner.state.operations.pop();
            return Err(RejectCode::StoreFailure);
        }
        self.log(inner, project_id, LogEvent::OperationStarted, None, None);
        inner.running = Some(operation_id.clone());
        let shared = Arc::clone(&self.shared);
        let worker_id = operation_id.clone();
        let spawned = thread::Builder::new()
            .name("arx-compose-op".to_owned())
            .spawn(move || run_worker(shared, worker_id, plan));
        if spawned.is_err() {
            // The worker never started, so no effect happened.
            inner.running = None;
            apply_settlement(
                &self.shared,
                inner,
                &operation_id,
                Settlement::failed(FailureCode::WorkerUnavailable),
            );
            let _ = self.persist(inner);
            let operation = find(inner, &operation_id).expect("journaled above");
            return Ok(operation_state(1, operation));
        }
        Ok(response)
    }

    fn check_secrets(&self, state: &State, project: &Project) -> Option<RejectCode> {
        let mut store = lock(&self.shared.store);
        for slot in &project.manifest().secrets {
            let owner = project.slot_owner(slot)?;
            let Some(record) = state.secrets.get(&secret_key(project.id(), slot)) else {
                return Some(RejectCode::SecretMissing);
            };
            if (record.uid, record.gid) != owner {
                return Some(RejectCode::SecretOwnerMismatch);
            }
            if !store.has_secret(project.id(), slot, owner) {
                return Some(RejectCode::SecretMissing);
            }
        }
        None
    }

    fn reconcile(&self, id: u32, operation_id: &str) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        let Some(operation) = find(&inner, operation_id) else {
            return reject(id, RejectCode::UnknownOperation);
        };
        if operation.state != OperationState::Ambiguous {
            return reject(id, RejectCode::NotAmbiguous);
        }
        if inner.running.is_some() {
            return reject(id, RejectCode::Busy);
        }
        let (kind, project_id, revision) = (
            operation.kind,
            operation.project_id.clone(),
            operation.revision,
        );
        let Some(record) = inner.state.projects.get(&project_id) else {
            return reject(id, RejectCode::UnknownProject);
        };
        let manifest = [&record.pending, &record.committed, &record.previous]
            .into_iter()
            .flatten()
            .find(|candidate| candidate.revision == revision)
            .or(record.committed.as_ref())
            .map(|revision| revision.manifest.clone());
        let Some(manifest) = manifest else {
            return reject(id, RejectCode::NotCommitted);
        };
        let project = Project::validate(manifest).expect("state manifests are validated");
        let slot = record.slot;
        // Hold the VM exclusively while querying, without holding the state lock.
        inner.running = Some(operation_id.to_owned());
        drop(inner);

        let observed = {
            let mut runner = lock(&self.shared.runner);
            let mut steps = Steps {
                shared: &self.shared,
                operation_id,
                runner: &mut **runner,
                last_observed: Vec::new(),
            };
            steps.observe(&project, revision, slot, kind)
        };

        let mut inner = lock(&self.shared.inner);
        inner.running = None;
        let response = match observed {
            None => reject(id, RejectCode::EngineUnavailable),
            Some((applied, views)) => {
                let any_container = !views.is_empty();
                let settlement = if applied {
                    let mut settlement = Settlement::succeeded(
                        LogEvent::Reconciled,
                        match kind {
                            OperationKind::Stop => Desired::Stopped,
                            OperationKind::Down => Desired::Down,
                            _ => Desired::Started,
                        },
                    );
                    settlement.promote = kind == OperationKind::Deploy;
                    settlement
                } else {
                    let mut settlement = Settlement::failed(FailureCode::NotApplied);
                    settlement.event = LogEvent::Reconciled;
                    settlement.clear_pending = kind == OperationKind::Deploy;
                    settlement.recovery_required = Some(any_container);
                    settlement
                };
                apply_settlement(
                    &self.shared,
                    &mut inner,
                    operation_id,
                    settlement.observed(views),
                );
                let _ = self.persist(&inner);
                operation_state(id, find(&inner, operation_id).expect("journaled"))
            }
        };
        self.shared.idle.notify_all();
        response
    }

    fn status(&self, id: u32, project_id: &str) -> ComposeResponse {
        let inner = lock(&self.shared.inner);
        let Some(record) = inner.state.projects.get(project_id) else {
            return reject(id, RejectCode::UnknownProject);
        };
        let revision = record
            .committed
            .as_ref()
            .map(|committed| committed.revision)
            .or(record.last_rendered);
        let names: Vec<String> = [&record.committed, &record.pending]
            .into_iter()
            .flatten()
            .next()
            .map(|revision| {
                revision
                    .manifest
                    .services
                    .iter()
                    .map(|service| service.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let idle = inner.running.is_none();
        drop(inner);

        // Query live only when idle and the engine is free; otherwise report
        // the last recorded observation. The query is one bounded `ps`.
        let live = match (idle, revision) {
            (true, Some(revision)) => match self.shared.runner.try_lock() {
                Ok(mut runner) => {
                    let names: Vec<&str> = names.iter().map(String::as_str).collect();
                    match runner.run(&engine::compose_ps(project_id, revision)) {
                        RunResult::Exited { code: 0, stdout } => {
                            parse_ps(&stdout, project_id, &names)
                        }
                        _ => None,
                    }
                }
                Err(_) => None,
            },
            _ => None,
        };

        let mut inner = lock(&self.shared.inner);
        let running = inner.running.clone();
        let Some(record) = inner.state.projects.get_mut(project_id) else {
            return reject(id, RejectCode::UnknownProject);
        };
        let observation = match live {
            Some(views) => {
                record.observed = views;
                Observation::Live
            }
            None => Observation::Recorded,
        };
        ComposeResponse::ComposeProjectStatus {
            id,
            project: ProjectView {
                project_id: project_id.to_owned(),
                desired: record.desired,
                committed_revision: record.committed.as_ref().map(|c| c.revision),
                previous_revision: record.previous.as_ref().map(|p| p.revision),
                recovery_required: record.recovery_required,
                running_operation: running,
                observation,
                services: record.observed.clone(),
            },
        }
    }

    fn logs(&self, id: u32, project_id: &str, cursor: u64, limit: usize) -> ComposeResponse {
        let inner = lock(&self.shared.inner);
        let mut entries = Vec::new();
        let mut next_cursor = cursor;
        for entry in inner
            .state
            .log
            .iter()
            .filter(|entry| entry.project_id == project_id && entry.seq >= cursor)
        {
            if entries.len() >= limit {
                break;
            }
            entries.push(entry.clone());
            let page = ComposeResponse::ComposeLogPage {
                id,
                entries: entries.clone(),
                next_cursor: entry.seq + 1,
            };
            if serde_json::to_vec(&page).map_or(usize::MAX, |bytes| bytes.len()) > MAX_LOG_PAGE {
                entries.pop();
                break;
            }
            next_cursor = entry.seq + 1;
        }
        ComposeResponse::ComposeLogPage {
            id,
            entries,
            next_cursor,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn secret_put(
        &self,
        id: u32,
        operation_id: String,
        generation: u64,
        project_id: String,
        slot: String,
        epoch: u64,
        data: SecretText,
    ) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        if inner.generation != Some(generation) {
            return reject(id, RejectCode::GenerationMismatch);
        }
        let record = inner.state.projects.get(&project_id);
        let committed = record
            .and_then(|record| record.committed.as_ref())
            .map(|committed| {
                Project::validate(committed.manifest.clone()).expect("state manifests validated")
            });
        let defining = committed.or_else(|| {
            inner
                .staged
                .iter()
                .rev()
                .find(|staged| staged.project.id() == project_id)
                .map(|staged| staged.project.clone())
        });
        let Some(project) = defining else {
            return reject(id, RejectCode::UnknownProject);
        };
        let Some(owner) = project.slot_owner(&slot) else {
            return reject(id, RejectCode::UnknownSlot);
        };
        let key = secret_key(&project_id, &slot);
        let prior = inner.state.secrets.get(&key).copied();
        if prior.is_some_and(|prior| epoch < prior.epoch) {
            return reject(id, RejectCode::StaleEpoch);
        }
        let known = find(&inner, &operation_id).cloned();
        if known.as_ref().is_some_and(|known| {
            known.kind != OperationKind::Recreate || known.project_id != project_id
        }) {
            return reject(id, RejectCode::OperationConflict);
        }
        let started = record
            .is_some_and(|record| record.desired == Desired::Started && record.committed.is_some());
        let rotation = known.is_none() && started && prior.is_some_and(|prior| epoch > prior.epoch);
        if rotation && let Some(code) = gate(&inner, &project_id) {
            return reject(id, code);
        }
        let Some(bytes) = decode_secret(&data, MAX_SECRET) else {
            return reject(id, RejectCode::SecretSize);
        };
        drop(data);
        let written =
            lock(&self.shared.store).write_secret(&project_id, &slot, owner, bytes.as_slice());
        drop(bytes);
        if written.is_err() {
            return reject(id, RejectCode::SecretWrite);
        }
        inner.state.secrets.insert(
            key,
            SecretRecord {
                epoch,
                uid: owner.0,
                gid: owner.1,
            },
        );
        self.log(
            &mut inner,
            &project_id,
            LogEvent::SecretDelivered,
            None,
            None,
        );
        let recreate = if rotation {
            let record = inner
                .state
                .projects
                .get(&project_id)
                .expect("started project");
            let committed = record
                .committed
                .as_ref()
                .expect("started project is committed");
            let plan = Plan::Recreate {
                services: project.services_using(&slot),
                project,
                slot: record.slot,
                revision: committed.revision,
            };
            let revision = committed.revision;
            match self.launch(
                &mut inner,
                operation_id,
                OperationKind::Recreate,
                &project_id,
                revision,
                plan,
            ) {
                Ok(ComposeResponse::ComposeOperationState { operation, .. }) => Some(operation),
                _ => {
                    // The file is replaced; the container keeps the old inode
                    // until a later start or rotation recreates it.
                    let _ = self.persist(&inner);
                    None
                }
            }
        } else {
            let _ = self.persist(&inner);
            known.as_ref().map(view)
        };
        ComposeResponse::ComposeSecretAccepted {
            id,
            project_id,
            slot,
            epoch,
            recreate,
        }
    }
}

fn with_id(response: ComposeResponse, id: u32) -> ComposeResponse {
    match response {
        ComposeResponse::ComposeOperationState { operation, .. } => {
            ComposeResponse::ComposeOperationState { id, operation }
        }
        other => other,
    }
}

fn find<'a>(inner: &'a Inner, operation_id: &str) -> Option<&'a OperationRecord> {
    inner
        .state
        .operations
        .iter()
        .find(|operation| operation.operation_id == operation_id)
}

fn existing(
    inner: &Inner,
    id: u32,
    operation_id: &str,
    kind: OperationKind,
    project_id: &str,
    revision: Option<u64>,
) -> Option<ComposeResponse> {
    let operation = find(inner, operation_id)?;
    if operation.kind == kind
        && operation.project_id == project_id
        && revision.is_none_or(|revision| revision == operation.revision)
    {
        Some(operation_state(id, operation))
    } else {
        Some(reject(id, RejectCode::OperationConflict))
    }
}

fn gate(inner: &Inner, project_id: &str) -> Option<RejectCode> {
    if inner.running.is_some() {
        return Some(RejectCode::Busy);
    }
    if inner.state.operations.iter().any(|operation| {
        operation.project_id == project_id && operation.state == OperationState::Ambiguous
    }) {
        return Some(RejectCode::ReconcileRequired);
    }
    None
}

fn trim_journal(operations: &mut Vec<OperationRecord>) {
    while operations.len() >= MAX_JOURNAL {
        let Some(oldest) = operations.iter().position(|operation| {
            matches!(
                operation.state,
                OperationState::Succeeded | OperationState::Failed
            )
        }) else {
            return;
        };
        operations.remove(oldest);
    }
}

fn check_volumes(state: &State, project: &Project) -> Option<RejectCode> {
    let mut total = state.volumes.len();
    for volume in &project.manifest().volumes {
        let name = volume_name(project.id(), &volume.name);
        match state.volumes.get(&name) {
            Some(record) if record.project != project.id() => {
                return Some(RejectCode::VolumeCollision);
            }
            Some(_) => {}
            None => total += 1,
        }
    }
    (total > MAX_VOLUMES).then_some(RejectCode::VolumeLimit)
}

fn push_log(
    shared: &Shared,
    inner: &mut Inner,
    project: &str,
    event: LogEvent,
    service: Option<&str>,
    failure: Option<FailureCode>,
) {
    let seq = inner.state.next_log_seq;
    inner.state.next_log_seq += 1;
    inner.state.log.push_back(LogEntry {
        seq,
        unix_s: shared.clock.now_unix(),
        project_id: project.to_owned(),
        event,
        service: service.map(str::to_owned),
        failure,
    });
    while inner.state.log.len() > MAX_LOG {
        inner.state.log.pop_front();
    }
}

fn apply_settlement(
    shared: &Shared,
    inner: &mut Inner,
    operation_id: &str,
    settlement: Settlement,
) {
    let Some(operation) = inner
        .state
        .operations
        .iter_mut()
        .find(|operation| operation.operation_id == operation_id)
    else {
        return;
    };
    operation.state = settlement.state;
    operation.failure = settlement.failure;
    operation.restored_previous = settlement.restored;
    if settlement.state != OperationState::Ambiguous {
        operation.stage = OperationStage::Settled;
    }
    let project_id = operation.project_id.clone();
    if let Some(record) = inner.state.projects.get_mut(&project_id) {
        if settlement.promote
            && let Some(pending) = record.pending.take()
        {
            record.previous = record.committed.replace(pending);
        }
        if settlement.clear_pending || settlement.state == OperationState::Failed {
            record.pending = None;
        }
        if let Some(desired) = settlement.desired {
            record.desired = desired;
        }
        if let Some(required) = settlement.recovery_required {
            record.recovery_required = required;
        }
        if let Some(observed) = settlement.observed {
            record.observed = observed;
        }
    }
    push_log(
        shared,
        inner,
        &project_id,
        settlement.event,
        None,
        settlement.failure,
    );
}

fn run_worker(shared: Arc<Shared>, operation_id: String, plan: Plan) {
    let settlement = catch_unwind(AssertUnwindSafe(|| {
        let mut runner = lock(&shared.runner);
        let mut steps = Steps {
            shared: &shared,
            operation_id: &operation_id,
            runner: &mut **runner,
            last_observed: Vec::new(),
        };
        steps.execute(plan)
    }))
    // An unexpected panic leaves the effect unknown.
    .unwrap_or_else(|_| Settlement::ambiguous());
    let mut inner = lock(&shared.inner);
    apply_settlement(&shared, &mut inner, &operation_id, settlement);
    inner.running = None;
    if let Ok(bytes) = serde_json::to_vec(&inner.state) {
        let _ = lock(&shared.store).save_state(&bytes);
    }
    drop(inner);
    shared.idle.notify_all();
}

struct Steps<'a> {
    shared: &'a Shared,
    operation_id: &'a str,
    runner: &'a mut dyn CommandRunner,
    last_observed: Vec<ServiceView>,
}

impl Steps<'_> {
    fn stage(&self, stage: OperationStage) {
        let mut inner = lock(&self.shared.inner);
        if let Some(operation) = inner
            .state
            .operations
            .iter_mut()
            .find(|operation| operation.operation_id == self.operation_id)
        {
            operation.stage = stage;
        }
    }

    fn log(&self, project: &str, event: LogEvent, service: Option<&str>) {
        let mut inner = lock(&self.shared.inner);
        push_log(self.shared, &mut inner, project, event, service, None);
    }

    fn execute(&mut self, plan: Plan) -> Settlement {
        match plan {
            Plan::Deploy {
                project,
                slot,
                revision,
                prior,
            } => self.deploy(&project, slot, revision, prior),
            Plan::Start {
                project,
                slot,
                revision,
            } => self.start(&project, slot, revision),
            Plan::Stop { project, revision } => {
                self.stage(OperationStage::Stopping);
                match self.apply(
                    &engine::compose_stop(&project, revision),
                    FailureCode::StopFailed,
                ) {
                    Ok(()) => Settlement::succeeded(LogEvent::Stopped, Desired::Stopped),
                    Err(step) => step.into(),
                }
            }
            Plan::Down { project, revision } => {
                self.stage(OperationStage::Removing);
                match self.apply(
                    &engine::compose_down(&project, revision),
                    FailureCode::DownFailed,
                ) {
                    Ok(()) => {
                        Settlement::succeeded(LogEvent::Removed, Desired::Down).observed(Vec::new())
                    }
                    Err(step) => step.into(),
                }
            }
            Plan::Recreate {
                project,
                slot,
                revision,
                services,
            } => {
                self.stage(OperationStage::Recreating);
                let invocation = engine::compose_recreate(project.id(), revision, &services);
                if let Err(step) = self.apply(&invocation, FailureCode::UpFailed) {
                    return step.into();
                }
                self.stage(OperationStage::HealthCheck);
                match self.prove_health(&project, revision, slot) {
                    Ok(views) => {
                        Settlement::succeeded(LogEvent::Recreated, Desired::Started).observed(views)
                    }
                    Err(step) => {
                        Settlement::from(step).observed(std::mem::take(&mut self.last_observed))
                    }
                }
            }
        }
    }

    fn deploy(
        &mut self,
        project: &Project,
        slot: u8,
        revision: u64,
        prior: Option<(Project, u64)>,
    ) -> Settlement {
        let id = project.id();
        self.stage(OperationStage::Rendering);
        let yaml = render(project, revision, slot);
        if lock(&self.shared.store)
            .write_revision(id, revision, &yaml)
            .is_err()
        {
            return Settlement::failed(FailureCode::StoreFailed);
        }
        {
            let mut inner = lock(&self.shared.inner);
            if let Some(record) = inner.state.projects.get_mut(id) {
                record.last_rendered = Some(revision);
            }
        }
        if let Err(step) = self.prepare(project) {
            return step.into();
        }
        self.stage(OperationStage::Applying);
        match self.apply(&engine::compose_up(id, revision), FailureCode::UpFailed) {
            Ok(()) => self.log(id, LogEvent::Applied, None),
            Err(Step::Ambiguous) => return Settlement::ambiguous(),
            Err(Step::Failed(code)) => return self.restore(code, project, slot, revision, prior),
        }
        self.stage(OperationStage::HealthCheck);
        match self.prove_health(project, revision, slot) {
            Ok(views) => {
                self.log(id, LogEvent::HealthPassed, None);
                let mut settlement =
                    Settlement::succeeded(LogEvent::Promoted, Desired::Started).observed(views);
                settlement.promote = true;
                settlement
            }
            Err(Step::Ambiguous) => Settlement::ambiguous(),
            Err(Step::Failed(code)) => {
                self.log(id, LogEvent::HealthFailed, None);
                self.restore(code, project, slot, revision, prior)
            }
        }
    }

    /// After a failed apply or health proof: bring back the previous committed
    /// revision when its volume and secret layout is unchanged, otherwise stop
    /// the failed revision and report recovery required. Data is kept.
    fn restore(
        &mut self,
        code: FailureCode,
        project: &Project,
        slot: u8,
        revision: u64,
        prior: Option<(Project, u64)>,
    ) -> Settlement {
        let id = project.id();
        let failed_views = std::mem::take(&mut self.last_observed);
        let compatible = prior.filter(|(previous, _)| {
            previous.manifest().volumes == project.manifest().volumes
                && previous.manifest().secrets == project.manifest().secrets
                && previous
                    .manifest()
                    .secrets
                    .iter()
                    .all(|slot| previous.slot_owner(slot) == project.slot_owner(slot))
        });
        let mut settlement = Settlement::failed(code);
        settlement.clear_pending = true;
        if let Some((previous, previous_revision)) = compatible {
            self.stage(OperationStage::Restoring);
            let restored = self
                .apply(
                    &engine::compose_up(id, previous_revision),
                    FailureCode::UpFailed,
                )
                .is_ok()
                && match self.prove_health(&previous, previous_revision, slot) {
                    Ok(views) => {
                        settlement.observed = Some(views);
                        true
                    }
                    Err(_) => false,
                };
            if restored {
                self.log(id, LogEvent::Restored, None);
            }
            settlement.restored = restored;
            settlement.recovery_required = Some(!restored);
            if !restored {
                settlement.observed = Some(std::mem::take(&mut self.last_observed));
            }
            return settlement;
        }
        self.stage(OperationStage::Stopping);
        let stopped = matches!(
            self.runner.run(&engine::compose_stop(id, revision)),
            RunResult::Exited { code: 0, .. }
        );
        settlement.recovery_required = Some(!stopped);
        settlement.observed = Some(failed_views);
        settlement
    }

    fn start(&mut self, project: &Project, slot: u8, revision: u64) -> Settlement {
        if let Err(step) = self.prepare(project) {
            return step.into();
        }
        self.stage(OperationStage::Applying);
        if let Err(step) = self.apply(
            &engine::compose_up(project.id(), revision),
            FailureCode::UpFailed,
        ) {
            return step.into();
        }
        self.stage(OperationStage::HealthCheck);
        match self.prove_health(project, revision, slot) {
            Ok(views) => {
                Settlement::succeeded(LogEvent::HealthPassed, Desired::Started).observed(views)
            }
            Err(step) => Settlement::from(step).observed(std::mem::take(&mut self.last_observed)),
        }
    }

    fn prepare(&mut self, project: &Project) -> Result<(), Step> {
        self.stage(OperationStage::Pulling);
        for image in project.images() {
            self.ensure_image(project.id(), image)?;
        }
        self.stage(OperationStage::Volumes);
        for volume in &project.manifest().volumes {
            self.ensure_volume(project.id(), volume)?;
        }
        Ok(())
    }

    fn ensure_image(&mut self, project: &str, image: &str) -> Result<(), Step> {
        const PLATFORM_LINE: &[u8] = b"linux/amd64\n";
        match self.runner.run(&engine::image_inspect(image)) {
            RunResult::Exited { code: 0, stdout } if stdout == PLATFORM_LINE => {
                self.log(project, LogEvent::ImageReady, None);
                return Ok(());
            }
            RunResult::Exited { code: 0, .. } => {
                return Err(Step::Failed(FailureCode::ImageIdentity));
            }
            RunResult::Exited { .. } => {}
            RunResult::TimedOut | RunResult::Signaled => {
                return Err(Step::Failed(FailureCode::EngineTimedOut));
            }
            RunResult::LaunchFailed => return Err(Step::Failed(FailureCode::EngineLaunchFailed)),
        }
        match self.runner.run(&engine::image_pull(image)) {
            RunResult::Exited { code: 0, .. } => {}
            RunResult::TimedOut => return Err(Step::Failed(FailureCode::PullTimedOut)),
            RunResult::Exited { .. } | RunResult::Signaled => {
                return Err(Step::Failed(FailureCode::PullFailed));
            }
            RunResult::LaunchFailed => return Err(Step::Failed(FailureCode::EngineLaunchFailed)),
        }
        match self.runner.run(&engine::image_inspect(image)) {
            RunResult::Exited { code: 0, stdout } if stdout == PLATFORM_LINE => {
                self.log(project, LogEvent::ImagePulled, None);
                Ok(())
            }
            _ => Err(Step::Failed(FailureCode::ImageIdentity)),
        }
    }

    fn volume_label(&mut self, name: &str) -> Result<Option<String>, Step> {
        match self.runner.run(&engine::volume_label(name)) {
            RunResult::Exited { code: 0, stdout } => Ok(Some(
                String::from_utf8(stdout)
                    .ok()
                    .and_then(|text| text.strip_suffix('\n').map(str::to_owned))
                    .unwrap_or_default(),
            )),
            RunResult::Exited { .. } => Ok(None),
            RunResult::LaunchFailed => Err(Step::Failed(FailureCode::EngineLaunchFailed)),
            RunResult::TimedOut | RunResult::Signaled => {
                Err(Step::Failed(FailureCode::EngineTimedOut))
            }
        }
    }

    /// Volumes are created by the broker, labelled with an ownership token and
    /// verified after creation, because `volume create` also succeeds for an
    /// existing foreign volume. Compose sees them only as external.
    fn ensure_volume(&mut self, project: &str, volume: &VolumeSpec) -> Result<(), Step> {
        let name = volume_name(project, &volume.name);
        let token = volume_token(project, &volume.name, volume.uid, volume.gid);
        match self.volume_label(&name)? {
            Some(label) if label == token => {}
            Some(_) => return Err(Step::Failed(FailureCode::VolumeCollision)),
            None => {
                match self.runner.run(&engine::volume_create(&name, &token)) {
                    RunResult::Exited { code: 0, .. } => {}
                    RunResult::LaunchFailed => {
                        return Err(Step::Failed(FailureCode::EngineLaunchFailed));
                    }
                    _ => return Err(Step::Failed(FailureCode::VolumeFailed)),
                }
                if self.volume_label(&name)?.as_deref() != Some(token.as_str()) {
                    return Err(Step::Failed(FailureCode::VolumeCollision));
                }
            }
        }
        let expected = format!("{}\n", layout::volume_mountpoint(&name));
        match self.runner.run(&engine::volume_mountpoint(&name)) {
            RunResult::Exited { code: 0, stdout } if stdout == expected.as_bytes() => {}
            _ => return Err(Step::Failed(FailureCode::VolumeFailed)),
        }
        if lock(&self.shared.store)
            .prepare_volume(&name, (volume.uid, volume.gid))
            .is_err()
        {
            return Err(Step::Failed(FailureCode::VolumeFailed));
        }
        let mut inner = lock(&self.shared.inner);
        inner.state.volumes.insert(
            name,
            VolumeRecord {
                project: project.to_owned(),
                volume: volume.name.clone(),
                uid: volume.uid,
                gid: volume.gid,
            },
        );
        push_log(
            self.shared,
            &mut inner,
            project,
            LogEvent::VolumeReady,
            None,
            None,
        );
        Ok(())
    }

    /// A deadline kill or foreign signal leaves the effect unknown.
    fn apply(&mut self, invocation: &engine::Invocation, failure: FailureCode) -> Result<(), Step> {
        match self.runner.run(invocation) {
            RunResult::Exited { code: 0, .. } => Ok(()),
            RunResult::Exited { .. } => Err(Step::Failed(failure)),
            RunResult::LaunchFailed => Err(Step::Failed(FailureCode::EngineLaunchFailed)),
            RunResult::TimedOut | RunResult::Signaled => Err(Step::Ambiguous),
        }
    }

    fn ps(&mut self, project: &Project, revision: u64) -> Option<Vec<ServiceView>> {
        let names: Vec<&str> = project
            .manifest()
            .services
            .iter()
            .map(|service| service.name.as_str())
            .collect();
        match self.runner.run(&engine::compose_ps(project.id(), revision)) {
            RunResult::Exited { code: 0, stdout } => parse_ps(&stdout, project.id(), &names),
            _ => None,
        }
    }

    fn ready(&mut self, project: &Project, revision: u64, slot: u8, views: &[ServiceView]) -> bool {
        let running = project.manifest().services.iter().all(|service| {
            views.iter().any(|view| {
                view.name == service.name
                    && view.state == ServiceState::Running
                    && view.revision == Some(revision)
                    && matches!(view.health, HealthState::None | HealthState::Healthy)
            })
        });
        running
            && project
                .manifest()
                .services
                .iter()
                .enumerate()
                .all(
                    |(index, service)| match (service.http_port, &service.health_path) {
                        (Some(_), Some(path)) => {
                            self.runner.probe_http(loopback_port(slot, index), path)
                        }
                        _ => true,
                    },
                )
    }

    /// The health proof before promotion: every declared service runs the
    /// revision, and every HTTP service answers 2xx on its loopback port.
    fn prove_health(
        &mut self,
        project: &Project,
        revision: u64,
        slot: u8,
    ) -> Result<Vec<ServiceView>, Step> {
        for attempt in 0..HEALTH_ATTEMPTS {
            if attempt > 0 {
                self.runner.pause(HEALTH_INTERVAL);
            }
            let Some(views) = self.ps(project, revision) else {
                continue;
            };
            if self.ready(project, revision, slot, &views) {
                return Ok(views);
            }
            self.last_observed = views;
        }
        Err(Step::Failed(FailureCode::HealthFailed))
    }

    /// Reconcile's read-only query. Returns whether the ambiguous effect is
    /// observed as applied, with the views, or `None` when the engine cannot
    /// answer.
    fn observe(
        &mut self,
        project: &Project,
        revision: u64,
        slot: u8,
        kind: OperationKind,
    ) -> Option<(bool, Vec<ServiceView>)> {
        let views = self.ps(project, revision)?;
        let applied = match kind {
            OperationKind::Deploy | OperationKind::Start | OperationKind::Recreate => {
                self.ready(project, revision, slot, &views)
            }
            OperationKind::Stop => views.iter().all(|view| {
                !matches!(view.state, ServiceState::Running | ServiceState::Restarting)
            }),
            OperationKind::Down => views.is_empty(),
        };
        Some((applied, views))
    }
}

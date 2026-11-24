//! The guest Compose broker. Requests arrive serially from one session; long
//! physical operations run on one worker thread so the session can answer the
//! bounded status action meanwhile. At most one operation runs per VM.
//!
//! Unified durable transition path: every mutating request (lifecycle and
//! secret delivery) uses one bounded operation journal (`state.operations`)
//! and one spent-ID tombstone set (`state.tombstones`). Operations advance
//! through durable checkpoints (`Intent` -> `EffectStarted` -> `Settled`) and
//! commit terminal receipts together with affected project/budget state before
//! exposing either. Reconcile is strictly read-only and shares the same
//! observation predicates as normal execution.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::codec::{SecretBytes, SecretText, decode_secret, sha256_hex, wipe};
use super::engine::{
    self, Clock, CommandRunner, HEALTH_ATTEMPTS, HEALTH_INTERVAL, NetworkObservation,
    ObservedContainer, RunResult, VolumeObservation, parse_container_mounts,
    parse_network_inventory, parse_ps_attributed, parse_volume_inventory, to_service_views,
};
use super::layout::{self, loopback_port, secret_file, secret_instance, volume_name, volume_token};
use super::manifest::{MAX_PROJECTS, MAX_VOLUMES, Project, ProjectManifest, VolumeSpec};
use super::render::render;
use super::wire::{
    COMPOSE_CAPABILITIES, ComposeRequest, ComposeResponse, Desired, FailureCode, HealthState,
    LogEntry, LogEvent, MAX_LOG_PAGE, MAX_SECRET, Observation, OperationKind, OperationStage,
    OperationState, OperationView, ProjectView, RejectCode, STAGE_CHUNK, STAGE_TOTAL, ServiceState,
    ServiceView,
};

/// State schema version. Bumped to v2 for unified operation phases, pinned
/// immutable secret bindings, and removal of plaintext-derived secret hashes.
const STATE_VERSION: u32 = 2;
/// Committed staged releases held for deploy. Older ones are dropped first.
pub const MAX_STAGED: usize = 8;
pub const MAX_JOURNAL: usize = 64;
/// Evicted settled operation IDs retained so trimmed IDs never re-execute.
pub const MAX_TOMBSTONES: usize = 4096;
/// Bound over retained secret bindings across staged, pending, active and prior releases.
pub const MAX_SECRET_BINDINGS: usize = 128;
pub const MAX_LOG: usize = 128;
pub const MAX_STATE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreError;

/// Non-secret OS identity witness for an immutable secret file on tmpfs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretFileWitness {
    pub dev: u64,
    pub ino: u64,
    pub uid: u32,
    pub gid: u32,
}

/// Result of a private, bounded byte comparison against an immutable tmpfs
/// secret file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretMatch {
    /// Regular file (0400, exact owner, nlink == 1) with matching bytes.
    Match(SecretFileWitness),
    /// File exists at the bound path with different bytes, owner, mode or type.
    Mismatch,
    /// No file exists at the bound path.
    Missing,
    /// Storage error while reading or verifying the path.
    Error,
}

/// Root-owned guest storage. The Linux store is fd-relative and no-follow and
/// is **unverified on real hardware**; tests use an in-memory fake.
pub trait ComposeStore: Send {
    fn install_routes(&mut self, _table: &super::routing::RoutingTable) -> Result<(), StoreError> {
        Err(StoreError)
    }
    fn load_state(&mut self) -> Result<Option<Vec<u8>>, StoreError>;
    fn save_state(&mut self, bytes: &[u8]) -> Result<(), StoreError>;
    fn write_revision(
        &mut self,
        project: &str,
        revision: u64,
        yaml: &str,
    ) -> Result<(), StoreError>;
    /// Writes a rotation-scoped compose file inside the revision directory so
    /// the prior committed `compose.yaml` remains intact until settlement.
    fn write_rotation(
        &mut self,
        project: &str,
        revision: u64,
        operation_id: &str,
        yaml: &str,
    ) -> Result<(), StoreError>;
    /// Atomically publishes an immutable secret file on tmpfs owned by `owner`,
    /// mode 0400, returning its non-secret inode witness.
    fn write_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
        bytes: &[u8],
    ) -> Result<SecretFileWitness, StoreError>;
    /// Verifies an immutable secret file exists with exact owner, mode 0400
    /// and nlink == 1, returning its non-secret inode witness.
    fn stat_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
    ) -> Option<SecretFileWitness>;
    /// Private bounded comparison of `bytes` against an immutable tmpfs secret
    /// file using wiping buffers. Never logs or returns secret bytes.
    fn compare_secret(
        &mut self,
        project: &str,
        instance: &str,
        owner: (u32, u32),
        bytes: &[u8],
    ) -> SecretMatch;
    /// Sets the owner of a broker-created volume's empty data root.
    fn prepare_volume(&mut self, name: &str, owner: (u32, u32)) -> Result<(), StoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretBinding {
    project_id: String,
    release_digest: String,
    slot: String,
    epoch: u64,
    generation: u64,
    uid: u32,
    gid: u32,
    instance: String,
    witness: SecretFileWitness,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    revision: u64,
    digest: String,
    manifest: ProjectManifest,
    #[serde(default)]
    secret_bindings: BTreeMap<String, SecretBinding>,
}

#[derive(Clone, Serialize, Deserialize)]
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
    #[serde(default)]
    observed_ids: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VolumeRecord {
    project: String,
    volume: String,
    uid: u32,
    gid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperationPhase {
    /// Intent and pinned target durably recorded; no physical effect started.
    Intent,
    /// Prerequisites durably recorded; physical effect may have started.
    EffectStarted,
    /// Post-effect observation recorded; ready for terminal settlement.
    Observed,
    /// Terminal receipt and affected project/budget state committed together.
    Settled,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationRecord {
    operation_id: String,
    kind: OperationKind,
    project_id: String,
    revision: u64,
    generation: u64,
    phase: OperationPhase,
    state: OperationState,
    stage: OperationStage,
    failure: Option<FailureCode>,
    restored_previous: bool,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    slot: Option<String>,
    #[serde(default)]
    epoch: Option<u64>,
    #[serde(default)]
    owner: Option<(u32, u32)>,
    #[serde(default)]
    secret_instance: Option<String>,
    #[serde(default)]
    file_witness: Option<SecretFileWitness>,
    #[serde(default)]
    services: Vec<String>,
    #[serde(default)]
    baseline_complete: bool,
    #[serde(default)]
    prior_container_ids: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    projects: BTreeMap<String, ProjectRecord>,
    volumes: BTreeMap<String, VolumeRecord>,
    operations: Vec<OperationRecord>,
    #[serde(default)]
    tombstones: VecDeque<String>,
    #[serde(default)]
    secret_bindings: BTreeMap<String, SecretBinding>,
    // Highest epoch reserved before a possible tmpfs publication, independent
    // of receipt retention. A later successful delivery supersedes it.
    #[serde(default)]
    pending_epochs: BTreeMap<String, (u64, u64)>,
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
            tombstones: VecDeque::new(),
            secret_bindings: BTreeMap::new(),
            pending_epochs: BTreeMap::new(),
            log: VecDeque::new(),
            next_log_seq: 1,
        }
    }

    /// A successful save includes the directory sync. Verify the complete
    /// checkpoint, including tombstones, before making recovered state visible.
    fn confirm_checkpoint(&self, store: &mut dyn ComposeStore) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec(self).map_err(|_| StoreError)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(StoreError);
        }
        store.save_state(&bytes)?;
        if store.load_state()?.as_deref() != Some(bytes.as_slice()) {
            return Err(StoreError);
        }
        Ok(())
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
            || state.tombstones.len() > MAX_TOMBSTONES
            || state.secret_bindings.len() > MAX_SECRET_BINDINGS
            || state.pending_epochs.len() > MAX_SECRET_BINDINGS
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
        // Any non-settled or Running operation outlived its worker/request:
        // mark Ambiguous so it resolves strictly from observed evidence.
        for operation in &mut state.operations {
            if operation.phase != OperationPhase::Settled
                || operation.state == OperationState::Running
            {
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
    total_bytes: usize,
    project: Project,
}

struct Inner {
    state: State,
    staging: Option<Staging>,
    staged: VecDeque<Staged>,
    running: Option<String>,
    generation: Option<u64>,
    /// Set when a terminal or budget persistence call fails with an uncertain
    /// on-disk outcome; freezes dependent mutations until a durable checkpoint
    /// succeeds.
    store_uncertain: bool,
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

fn binding_key(project: &str, digest: &str, slot: &str) -> String {
    format!("{project}/{digest}/{slot}")
}

fn secret_instance_map(bindings: &BTreeMap<String, SecretBinding>) -> BTreeMap<String, String> {
    bindings
        .iter()
        .map(|(slot, binding)| (slot.clone(), binding.instance.clone()))
        .collect()
}

fn collect_container_ids(containers: &[ObservedContainer]) -> BTreeMap<String, Vec<String>> {
    let mut ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for container in containers {
        if let Some(id) = &container.id {
            let entry = ids.entry(container.view.name.clone()).or_default();
            if !entry.contains(id) {
                entry.push(id.clone());
            }
        }
    }
    ids
}

/// A retry may reuse an already published file only after private byte
/// comparison at admission and verification of the same inode at use.
enum SecretPublication {
    Create(SecretBytes),
    Reuse(SecretFileWitness),
}

impl SecretPublication {
    fn publish(
        self,
        store: &mut dyn ComposeStore,
        project: &str,
        instance: &str,
        owner: (u32, u32),
    ) -> Result<SecretFileWitness, StoreError> {
        match self {
            Self::Create(bytes) => store.write_secret(project, instance, owner, bytes.as_slice()),
            Self::Reuse(witness) => store
                .stat_secret(project, instance, owner)
                .filter(|current| *current == witness)
                .ok_or(StoreError),
        }
    }
}

/// The physical work of one worker operation, fixed at admission.
enum Plan {
    Deploy {
        project: Project,
        slot: u8,
        revision: u64,
        bindings: BTreeMap<String, SecretBinding>,
        prior: Option<(Project, u64, Desired, BTreeMap<String, SecretBinding>)>,
    },
    Start {
        project: Project,
        slot: u8,
        revision: u64,
        bindings: BTreeMap<String, SecretBinding>,
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
        secret_slot: String,
        digest: String,
        epoch: u64,
        generation: u64,
        owner: (u32, u32),
        instance: String,
        bytes: SecretPublication,
        base_bindings: BTreeMap<String, SecretBinding>,
        services: Vec<String>,
    },
}

/// How a finished operation changes the journal, secret bindings, and project
/// atomically in one durable commit.
struct Settlement {
    state: OperationState,
    failure: Option<FailureCode>,
    restored: bool,
    desired: Option<Desired>,
    promote: bool,
    clear_pending: bool,
    recovery_required: Option<bool>,
    observed: Option<Vec<ServiceView>>,
    observed_ids: Option<BTreeMap<String, Vec<String>>>,
    file_witness: Option<SecretFileWitness>,
    upsert_secret: Option<SecretBinding>,
    committed_bindings: Option<BTreeMap<String, SecretBinding>>,
    event: LogEvent,
}

impl Settlement {
    fn succeeded(event: LogEvent, desired: Option<Desired>) -> Self {
        Self {
            state: OperationState::Succeeded,
            failure: None,
            restored: false,
            desired,
            promote: false,
            clear_pending: false,
            recovery_required: Some(false),
            observed: None,
            observed_ids: None,
            file_witness: None,
            upsert_secret: None,
            committed_bindings: None,
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
            observed_ids: None,
            file_witness: None,
            upsert_secret: None,
            committed_bindings: None,
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
            observed_ids: None,
            file_witness: None,
            upsert_secret: None,
            committed_bindings: None,
            event: LogEvent::OperationAmbiguous,
        }
    }

    fn observed_containers(mut self, containers: &[ObservedContainer]) -> Self {
        self.observed = Some(to_service_views(containers));
        self.observed_ids = Some(collect_container_ids(containers));
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

enum DownEvidence {
    CompleteAbsent,
    Partial {
        containers: Vec<ObservedContainer>,
        owned_networks: Vec<String>,
    },
    ForeignConflict {
        containers: Vec<ObservedContainer>,
    },
    Unavailable,
}

enum LiveEvidence {
    Satisfied(Vec<ObservedContainer>),
    NotSatisfied(Vec<ObservedContainer>),
    Unavailable,
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
        // A process restart can still see a rename whose directory sync failed.
        // Keep Compose available for recovery, but expose no receipts from that
        // snapshot until its checkpoint has been confirmed.
        let (state, store_uncertain) = match store.load_state()? {
            Some(bytes) => {
                let candidate = State::load(&bytes)?;
                match candidate.confirm_checkpoint(store.as_mut()) {
                    Ok(()) => (candidate, false),
                    Err(StoreError) => (State::new(), true),
                }
            }
            None => (State::new(), false),
        };
        Ok(Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    state,
                    staging: None,
                    staged: VecDeque::new(),
                    running: None,
                    generation: None,
                    store_uncertain,
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
            ComposeRequest::ComposeInstallRoutes {
                id,
                generation,
                readiness_host,
                zone,
                routes,
            } => {
                let inner = lock(&self.shared.inner);
                if inner.generation != Some(generation) {
                    return reject(id, RejectCode::GenerationMismatch);
                }
                if inner.running.is_some() || inner.store_uncertain {
                    return reject(id, RejectCode::Busy);
                }
                let Ok(table) = super::routing::RoutingTable::new(
                    readiness_host,
                    Some(zone),
                    super::routing::SLIRP_FORWARD_PEER,
                    routes,
                ) else {
                    return reject(id, RejectCode::ProjectMismatch);
                };
                for route in table.routes() {
                    let Some(project) = inner.state.projects.get(&route.project_id) else {
                        return reject(id, RejectCode::UnknownProject);
                    };
                    let service = project.committed.as_ref().and_then(|revision| {
                        revision
                            .manifest
                            .services
                            .get(usize::from(route.service_index))
                    });
                    if project.slot != route.project_slot
                        || project.recovery_required
                        || project.pending.is_some()
                        || project.desired != Desired::Started
                        || !service
                            .is_some_and(|s| s.name == route.service && s.http_port.is_some())
                    {
                        return reject(id, RejectCode::ProjectMismatch);
                    }
                }
                if lock(&self.shared.store).install_routes(&table).is_err() {
                    return reject(id, RejectCode::StoreFailure);
                }
                ComposeResponse::ComposeRoutesInstalled { id, generation }
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
                digest,
                slot,
                epoch,
                data,
            } => self.secret_put(
                id,
                operation_id,
                generation,
                project_id,
                digest,
                slot,
                epoch,
                data,
            ),
        }
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
        if let Some(pos) = inner
            .staged
            .iter()
            .position(|staged| staged.digest == digest)
        {
            if inner.staged[pos].total_bytes != total {
                return reject(id, RejectCode::StageSize);
            }
            if let Some(entry) = inner.staged.remove(pos) {
                inner.staged.push_back(entry);
            }
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
        if let Some(pos) = inner
            .staged
            .iter()
            .position(|staged| staged.digest == digest)
        {
            let project_id = inner.staged[pos].project.id().to_owned();
            if let Some(entry) = inner.staged.remove(pos) {
                inner.staged.push_back(entry);
            }
            return ComposeResponse::ComposeReleaseStaged {
                id,
                digest: digest.to_owned(),
                project_id,
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
        let total_bytes = staging.total;
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
            total_bytes,
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
            Some(digest),
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
                        record.desired,
                        committed.secret_bindings.clone(),
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
        let bindings = match self.collect_ready_secrets(&inner.state, &project, digest) {
            Ok(bindings) => bindings,
            Err(code) => return reject(id, code),
        };
        let target = Revision {
            revision,
            digest: digest.to_owned(),
            manifest: project.manifest().clone(),
            secret_bindings: bindings.clone(),
        };
        let plan = Plan::Deploy {
            project,
            slot,
            revision,
            bindings,
            prior,
        };
        let op_record = OperationRecord {
            operation_id: operation_id.clone(),
            kind: OperationKind::Deploy,
            project_id: project_id.clone(),
            revision,
            generation: inner.generation.unwrap_or(0),
            phase: OperationPhase::Intent,
            state: OperationState::Running,
            stage: OperationStage::Queued,
            failure: None,
            restored_previous: false,
            digest: Some(digest.to_owned()),
            slot: None,
            epoch: None,
            owner: None,
            secret_instance: None,
            file_witness: None,
            services: Vec::new(),
            baseline_complete: false,
            prior_container_ids: BTreeMap::new(),
        };
        let queued = operation_state(id, &op_record);
        let persisted = commit_state(&self.shared, &mut inner, |state| {
            trim_journal(state)?;
            if state.operations.len() >= MAX_JOURNAL {
                return Err(RejectCode::JournalFull);
            }
            let record =
                state
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
                        observed_ids: BTreeMap::new(),
                    });
            record.pending = Some(target);
            state.operations.push(op_record);
            push_log(
                &self.shared,
                state,
                &project_id,
                LogEvent::OperationStarted,
                None,
                None,
            );
            Ok(())
        });
        match persisted {
            Ok(()) => self.spawn_worker(&mut inner, id, operation_id, plan, queued),
            Err(code) => reject(id, code),
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
        let (slot, revision, digest) = (record.slot, committed.revision, committed.digest.clone());
        let bindings = match self.collect_ready_secrets(&inner.state, &project, &digest) {
            Ok(bindings) => bindings,
            Err(code) => return reject(id, code),
        };
        let plan = Plan::Start {
            project,
            slot,
            revision,
            bindings,
        };
        self.launch_lifecycle(
            &mut inner,
            id,
            operation_id,
            OperationKind::Start,
            &project_id,
            revision,
            Some(digest),
            plan,
        )
    }

    fn stop_or_down(
        &self,
        id: u32,
        operation_id: String,
        project_id: String,
        kind: OperationKind,
    ) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        if let Some(response) = existing(&inner, id, &operation_id, kind, &project_id, None, None) {
            return response;
        }
        if let Some(code) = gate(&inner, &project_id) {
            return reject(id, code);
        }
        let Some(record) = inner.state.projects.get(&project_id) else {
            return reject(id, RejectCode::UnknownProject);
        };
        let (revision, digest) = match &record.committed {
            Some(committed) => (committed.revision, Some(committed.digest.clone())),
            None => match record.last_rendered {
                Some(rev) => (
                    rev,
                    record
                        .pending
                        .as_ref()
                        .filter(|p| p.revision == rev)
                        .map(|p| p.digest.clone()),
                ),
                None => return reject(id, RejectCode::NotCommitted),
            },
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
        self.launch_lifecycle(
            &mut inner,
            id,
            operation_id,
            kind,
            &project_id,
            revision,
            digest,
            plan,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn launch_lifecycle(
        &self,
        inner: &mut MutexGuard<'_, Inner>,
        id: u32,
        operation_id: String,
        kind: OperationKind,
        project_id: &str,
        revision: u64,
        digest: Option<String>,
        plan: Plan,
    ) -> ComposeResponse {
        let op_record = OperationRecord {
            operation_id: operation_id.clone(),
            kind,
            project_id: project_id.to_owned(),
            revision,
            generation: inner.generation.unwrap_or(0),
            phase: OperationPhase::Intent,
            state: OperationState::Running,
            stage: OperationStage::Queued,
            failure: None,
            restored_previous: false,
            digest,
            slot: None,
            epoch: None,
            owner: None,
            secret_instance: None,
            file_witness: None,
            services: Vec::new(),
            baseline_complete: false,
            prior_container_ids: BTreeMap::new(),
        };
        let queued = operation_state(id, &op_record);
        let persisted = commit_state(&self.shared, inner, |state| {
            trim_journal(state)?;
            if state.operations.len() >= MAX_JOURNAL {
                return Err(RejectCode::JournalFull);
            }
            state.operations.push(op_record);
            push_log(
                &self.shared,
                state,
                project_id,
                LogEvent::OperationStarted,
                None,
                None,
            );
            Ok(())
        });
        match persisted {
            Ok(()) => self.spawn_worker(inner, id, operation_id, plan, queued),
            Err(code) => reject(id, code),
        }
    }

    fn spawn_worker(
        &self,
        inner: &mut MutexGuard<'_, Inner>,
        id: u32,
        operation_id: String,
        plan: Plan,
        queued: ComposeResponse,
    ) -> ComposeResponse {
        inner.running = Some(operation_id.clone());
        let shared = Arc::clone(&self.shared);
        let worker_id = operation_id.clone();
        let spawned = thread::Builder::new()
            .name("arx-compose-op".to_owned())
            .spawn(move || run_worker(shared, worker_id, plan));
        if spawned.is_err() {
            inner.running = None;
            let _ = commit_settlement(
                &self.shared,
                inner,
                &operation_id,
                Settlement::failed(FailureCode::WorkerUnavailable),
            );
            let operation = find(inner, &operation_id).expect("journaled above");
            return operation_state(id, operation);
        }
        queued
    }

    /// Resolves and verifies every secret slot declared by `project` against
    /// `digest`'s pinned bindings and live tmpfs file witnesses.
    fn collect_ready_secrets(
        &self,
        state: &State,
        project: &Project,
        digest: &str,
    ) -> Result<BTreeMap<String, SecretBinding>, RejectCode> {
        let mut store = lock(&self.shared.store);
        let mut resolved = BTreeMap::new();
        for slot in &project.manifest().secrets {
            let Some(owner) = project.slot_owner(slot) else {
                return Err(RejectCode::UnknownSlot);
            };
            let key = binding_key(project.id(), digest, slot);
            let Some(binding) = state.secret_bindings.get(&key) else {
                return Err(RejectCode::SecretMissing);
            };
            if (binding.uid, binding.gid) != owner {
                return Err(RejectCode::SecretOwnerMismatch);
            }
            if store.stat_secret(project.id(), &binding.instance, owner) != Some(binding.witness) {
                return Err(RejectCode::SecretMissing);
            }
            resolved.insert(slot.clone(), binding.clone());
        }
        Ok(resolved)
    }

    fn reconcile(&self, id: u32, operation_id: &str) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        let recovering = inner.store_uncertain;
        if recovering {
            if inner.running.is_some() {
                return reject(id, RejectCode::Busy);
            }
            if let Err(code) = reload_uncertain(&self.shared, &mut inner) {
                return reject(id, code);
            }
        }
        let Some(operation) = find(&inner, operation_id).cloned() else {
            return reject(id, RejectCode::UnknownOperation);
        };
        if operation.state != OperationState::Ambiguous {
            if recovering {
                return operation_state(id, &operation);
            }
            return reject(id, RejectCode::NotAmbiguous);
        }
        if inner.running.is_some() {
            return reject(id, RejectCode::Busy);
        }

        // Standalone SecretPut reconciliation (read-only).
        if operation.kind == OperationKind::SecretPut {
            return self.reconcile_secret_put(&mut inner, id, &operation);
        }

        let project_id = operation.project_id.clone();
        let revision = operation.revision;
        let Some(record) = inner.state.projects.get(&project_id) else {
            return reject(id, RejectCode::UnknownProject);
        };
        let slot = record.slot;

        // Exact requested manifest only — never fall back to a different
        // revision or digest.
        let matched_revision = [&record.pending, &record.committed, &record.previous]
            .into_iter()
            .flatten()
            .find(|candidate| {
                candidate.revision == revision
                    && operation
                        .digest
                        .as_deref()
                        .is_none_or(|d| candidate.digest == d)
            })
            .cloned();

        // Down and Stop still require an exact revision anchor if a digest was
        // recorded; if the exact requested manifest is missing, refuse.
        let Some(target_rev) = matched_revision else {
            return reject(id, RejectCode::UnknownRelease);
        };
        let project =
            Project::validate(target_rev.manifest.clone()).expect("state manifests are validated");

        // If the operation crashed at `Phase::Intent` (before EffectStarted),
        // no live container mutation was launched; settle Failed(NotApplied)
        // cleanly without running Docker.
        if operation.phase == OperationPhase::Intent
            || (operation.kind == OperationKind::Recreate
                && (!operation.baseline_complete || operation.file_witness.is_none()))
        {
            let mut settlement = Settlement::failed(FailureCode::NotApplied);
            settlement.event = LogEvent::Reconciled;
            settlement.clear_pending = operation.kind == OperationKind::Deploy;
            let _ = commit_settlement(&self.shared, &mut inner, operation_id, settlement);
            return operation_state(id, find(&inner, operation_id).expect("journaled"));
        }

        // Build the exact expected secret bindings for mount verification.
        let expected_bindings = match operation.kind {
            OperationKind::Recreate => {
                let mut map = target_rev.secret_bindings.clone();
                if let (
                    Some(secret_slot),
                    Some(digest),
                    Some(epoch),
                    Some(owner),
                    Some(instance),
                    Some(witness),
                ) = (
                    operation.slot.clone(),
                    operation.digest.clone(),
                    operation.epoch,
                    operation.owner,
                    operation.secret_instance.clone(),
                    operation.file_witness,
                ) {
                    map.insert(
                        secret_slot.clone(),
                        SecretBinding {
                            project_id: project_id.clone(),
                            release_digest: digest,
                            slot: secret_slot,
                            epoch,
                            generation: operation.generation,
                            uid: owner.0,
                            gid: owner.1,
                            instance,
                            witness,
                        },
                    );
                }
                map
            }
            OperationKind::Start => {
                // Start may have targeted refreshed bindings (e.g. new generation).
                let mut map = target_rev.secret_bindings.clone();
                for s in &project.manifest().secrets {
                    let key = binding_key(&project_id, &target_rev.digest, s);
                    if let Some(binding) = inner.state.secret_bindings.get(&key) {
                        map.insert(s.clone(), binding.clone());
                    }
                }
                map
            }
            _ => target_rev.secret_bindings.clone(),
        };

        // Hold the VM exclusively while running read-only observation.
        inner.running = Some(operation_id.to_owned());
        drop(inner);

        enum ReconcileOutcome {
            Unavailable,
            Settled(Box<Settlement>),
        }

        let outcome = {
            let mut runner = lock(&self.shared.runner);
            let mut steps = Steps {
                shared: &self.shared,
                operation_id,
                runner: &mut **runner,
                last_observed: Vec::new(),
            };
            match operation.kind {
                OperationKind::Deploy | OperationKind::Start => {
                    match steps.verify_live_services(&project, revision, slot, &expected_bindings) {
                        LiveEvidence::Unavailable => ReconcileOutcome::Unavailable,
                        LiveEvidence::Satisfied(containers) => {
                            let mut s =
                                Settlement::succeeded(LogEvent::Reconciled, Some(Desired::Started))
                                    .observed_containers(&containers);
                            s.promote = operation.kind == OperationKind::Deploy;
                            if operation.kind == OperationKind::Start {
                                s.committed_bindings = Some(expected_bindings.clone());
                            }
                            ReconcileOutcome::Settled(Box::new(s))
                        }
                        LiveEvidence::NotSatisfied(containers) => {
                            let mut s = Settlement::failed(FailureCode::NotApplied)
                                .observed_containers(&containers);
                            s.event = LogEvent::Reconciled;
                            s.clear_pending = operation.kind == OperationKind::Deploy;
                            s.recovery_required = Some(!containers.is_empty());
                            ReconcileOutcome::Settled(Box::new(s))
                        }
                    }
                }
                OperationKind::Recreate => match steps.verify_live_recreate(
                    &project,
                    revision,
                    slot,
                    &expected_bindings,
                    &operation.services,
                    &operation.prior_container_ids,
                ) {
                    LiveEvidence::Unavailable => ReconcileOutcome::Unavailable,
                    LiveEvidence::Satisfied(containers) => {
                        let mut s =
                            Settlement::succeeded(LogEvent::Reconciled, Some(Desired::Started))
                                .observed_containers(&containers);
                        s.committed_bindings = Some(expected_bindings.clone());
                        if let Some(secret_slot) = &operation.slot
                            && let Some(binding) = expected_bindings.get(secret_slot)
                        {
                            s.upsert_secret = Some(binding.clone());
                        }
                        ReconcileOutcome::Settled(Box::new(s))
                    }
                    LiveEvidence::NotSatisfied(containers) => {
                        let mut s = Settlement::failed(FailureCode::NotApplied)
                            .observed_containers(&containers);
                        s.event = LogEvent::Reconciled;
                        s.recovery_required = Some(true);
                        ReconcileOutcome::Settled(Box::new(s))
                    }
                },
                OperationKind::Stop => match steps.ps_attributed_strict(&project_id, revision) {
                    None => ReconcileOutcome::Unavailable,
                    Some(containers) => {
                        let all_stopped = containers.iter().all(|c| !is_active_state(c.view.state));
                        if all_stopped {
                            ReconcileOutcome::Settled(Box::new(
                                Settlement::succeeded(LogEvent::Reconciled, Some(Desired::Stopped))
                                    .observed_containers(&containers),
                            ))
                        } else {
                            let mut s = Settlement::failed(FailureCode::NotApplied)
                                .observed_containers(&containers);
                            s.event = LogEvent::Reconciled;
                            s.recovery_required = Some(true);
                            ReconcileOutcome::Settled(Box::new(s))
                        }
                    }
                },
                OperationKind::Down => match steps.observe_down(&project_id, revision) {
                    DownEvidence::Unavailable => ReconcileOutcome::Unavailable,
                    DownEvidence::CompleteAbsent => ReconcileOutcome::Settled(Box::new(
                        Settlement::succeeded(LogEvent::Reconciled, Some(Desired::Down))
                            .observed_containers(&[]),
                    )),
                    DownEvidence::Partial { containers, .. }
                    | DownEvidence::ForeignConflict { containers } => {
                        let mut s = Settlement::failed(FailureCode::NotApplied)
                            .observed_containers(&containers);
                        s.event = LogEvent::Reconciled;
                        s.recovery_required = Some(true);
                        ReconcileOutcome::Settled(Box::new(s))
                    }
                },
                OperationKind::SecretPut => unreachable!("handled above"),
            }
        };

        let mut inner = lock(&self.shared.inner);
        inner.running = None;
        let response = match outcome {
            ReconcileOutcome::Unavailable => reject(id, RejectCode::EngineUnavailable),
            ReconcileOutcome::Settled(settlement) => {
                let _ = commit_settlement(&self.shared, &mut inner, operation_id, *settlement);
                operation_state(id, find(&inner, operation_id).expect("journaled"))
            }
        };
        self.shared.idle.notify_all();
        response
    }

    fn reconcile_secret_put(
        &self,
        inner: &mut MutexGuard<'_, Inner>,
        id: u32,
        operation: &OperationRecord,
    ) -> ComposeResponse {
        let (Some(instance), Some(owner)) = (operation.secret_instance.as_deref(), operation.owner)
        else {
            let mut s = Settlement::failed(FailureCode::NotApplied);
            s.event = LogEvent::Reconciled;
            let _ = commit_settlement(&self.shared, inner, &operation.operation_id, s);
            return operation_state(id, find(inner, &operation.operation_id).expect("journaled"));
        };
        let stat = lock(&self.shared.store).stat_secret(&operation.project_id, instance, owner);
        match stat {
            None => {
                // Positively absent on tmpfs: settle Failed(NotApplied).
                let mut s = Settlement::failed(FailureCode::NotApplied);
                s.event = LogEvent::Reconciled;
                let _ = commit_settlement(&self.shared, inner, &operation.operation_id, s);
                operation_state(id, find(inner, &operation.operation_id).expect("journaled"))
            }
            Some(witness)
                if operation.phase == OperationPhase::Observed
                    && operation.file_witness == Some(witness) =>
            {
                // Exact inode was already observed after write before final save failed.
                let Some(binding) = build_binding_from_op(operation, witness) else {
                    return operation_state(
                        id,
                        find(inner, &operation.operation_id).expect("journaled"),
                    );
                };
                let mut s = Settlement::succeeded(LogEvent::Reconciled, None);
                s.recovery_required = None;
                s.file_witness = Some(witness);
                s.upsert_secret = Some(binding.clone());
                s.committed_bindings = updated_committed_bindings_if_matching(inner, &binding);
                let _ = commit_settlement(&self.shared, inner, &operation.operation_id, s);
                operation_state(id, find(inner, &operation.operation_id).expect("journaled"))
            }
            Some(_) => {
                // File exists on tmpfs, but without private byte verification
                // (supplied only on exact-ID retry) we cannot prove content match:
                // keep Ambiguous so unknown stays typed.
                operation_state(id, find(inner, &operation.operation_id).expect("journaled"))
            }
        }
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
        let idle = inner.running.is_none();
        drop(inner);

        // Query live only when idle and the engine is free; otherwise report
        // the last recorded observation.
        let live = match (idle, revision) {
            (true, Some(revision)) => match self.shared.runner.try_lock() {
                Ok(mut runner) => match runner.run(&engine::compose_ps(project_id, revision)) {
                    RunResult::Exited { code: 0, stdout } => {
                        parse_ps_attributed(&stdout, project_id)
                    }
                    _ => None,
                },
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
            Some(containers) => {
                record.observed = to_service_views(&containers);
                record.observed_ids = collect_container_ids(&containers);
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
        digest: String,
        slot: String,
        epoch: u64,
        data: SecretText,
    ) -> ComposeResponse {
        let mut inner = lock(&self.shared.inner);
        if inner.generation != Some(generation) {
            return reject(id, RejectCode::GenerationMismatch);
        }
        if is_tombstoned(&inner, &operation_id) {
            return reject(id, RejectCode::OperationConflict);
        }

        // Exact-ID replay or conflict in the unified operation journal.
        if let Some(known) = find(&inner, &operation_id).cloned() {
            let metadata_matches = matches!(
                known.kind,
                OperationKind::SecretPut | OperationKind::Recreate
            ) && known.project_id == project_id
                && known.generation == generation
                && known.digest.as_deref() == Some(digest.as_str())
                && known.slot.as_deref() == Some(slot.as_str())
                && known.epoch == Some(epoch);
            if !metadata_matches {
                return reject(id, RejectCode::OperationConflict);
            }
            let Some(bytes) = decode_secret(&data, MAX_SECRET) else {
                return reject(id, RejectCode::OperationConflict);
            };
            drop(data);
            return self.replay_secret_operation(&mut inner, id, &known, bytes);
        }

        // New mutating secret intake is serialized behind any active or
        // unresolved project/store work so in-flight inputs stay pinned.
        if let Some(code) = gate(&inner, &project_id) {
            return reject(id, code);
        }

        // Resolve ONLY the explicitly supplied release digest (staged or
        // persisted for this project); never guess from staging recency.
        let target_project = match resolve_release_manifest(&inner, &project_id, &digest) {
            Ok(project) => project,
            Err(code) => return reject(id, code),
        };
        let Some(owner) = target_project.slot_owner(&slot) else {
            return reject(id, RejectCode::UnknownSlot);
        };
        let Some(bytes) = decode_secret(&data, MAX_SECRET) else {
            return reject(id, RejectCode::SecretSize);
        };
        drop(data);

        let record = inner.state.projects.get(&project_id);
        let committed_rev = record.and_then(|r| r.committed.as_ref());
        let is_live_committed = record.is_some_and(|r| {
            r.desired == Desired::Started
                && r.committed.as_ref().is_some_and(|c| c.digest == digest)
        });
        let bkey = binding_key(&project_id, &digest, &slot);
        let existing_binding = inner.state.secret_bindings.get(&bkey).cloned().or_else(|| {
            committed_rev
                .filter(|c| c.digest == digest)
                .and_then(|c| c.secret_bindings.get(&slot).cloned())
        });

        let instance = secret_instance(&slot, &digest, epoch, generation);
        let mut reuse = None;
        if let Some(&(reserved_generation, reserved_epoch)) = inner.state.pending_epochs.get(&bkey)
        {
            if epoch < reserved_epoch {
                return reject(id, RejectCode::StaleEpoch);
            }
            if generation == reserved_generation && epoch == reserved_epoch {
                match lock(&self.shared.store).compare_secret(
                    &project_id,
                    &instance,
                    owner,
                    bytes.as_slice(),
                ) {
                    SecretMatch::Match(witness) => reuse = Some(witness),
                    SecretMatch::Error => return reject(id, RejectCode::StoreFailure),
                    _ => return reject(id, RejectCode::SecretConflict),
                }
            }
        }

        // Epoch immutability: decreasing epochs fail; equal epochs reuse only
        // for identical bytes and valid live file witness, or allow new-generation
        // redelivery after tmpfs loss.
        if let Some(existing) = &existing_binding {
            if epoch < existing.epoch {
                return reject(id, RejectCode::StaleEpoch);
            }
            if epoch == existing.epoch {
                let cmp = lock(&self.shared.store).compare_secret(
                    &project_id,
                    &existing.instance,
                    owner,
                    bytes.as_slice(),
                );
                match cmp {
                    SecretMatch::Match(witness) if witness == existing.witness => {
                        drop(bytes);
                        let op_record = OperationRecord {
                            operation_id: operation_id.clone(),
                            kind: OperationKind::SecretPut,
                            project_id: project_id.clone(),
                            revision: committed_rev
                                .filter(|c| c.digest == digest)
                                .map_or(0, |c| c.revision),
                            generation,
                            phase: OperationPhase::Settled,
                            state: OperationState::Succeeded,
                            stage: OperationStage::Settled,
                            failure: None,
                            restored_previous: false,
                            digest: Some(digest.clone()),
                            slot: Some(slot.clone()),
                            epoch: Some(epoch),
                            owner: Some(owner),
                            secret_instance: Some(existing.instance.clone()),
                            file_witness: Some(witness),
                            services: Vec::new(),
                            baseline_complete: false,
                            prior_container_ids: BTreeMap::new(),
                        };
                        let saved = commit_settlement(
                            &self.shared,
                            &mut inner,
                            &operation_id,
                            SettlementCommit::Receipt(op_record),
                        );
                        return match saved {
                            Ok(()) => ComposeResponse::ComposeSecretAccepted {
                                id,
                                project_id,
                                digest,
                                slot,
                                epoch,
                                recreate: None,
                            },
                            Err(code) => reject(id, code),
                        };
                    }
                    SecretMatch::Match(_) | SecretMatch::Mismatch => {
                        return reject(id, RejectCode::SecretConflict);
                    }
                    SecretMatch::Missing => {
                        // A missing file has no evidence for same-generation reuse.
                        if existing.generation == generation {
                            return reject(id, RejectCode::SecretConflict);
                        }
                        // After a generation change, fall through to publish a fresh
                        // immutable instance for this generation.
                    }
                    SecretMatch::Error => return reject(id, RejectCode::StoreFailure),
                }
            }
        }

        let bytes = match reuse {
            Some(witness) => SecretPublication::Reuse(witness),
            None => SecretPublication::Create(bytes),
        };
        let rotation =
            is_live_committed && existing_binding.as_ref().is_some_and(|b| epoch > b.epoch);

        if rotation {
            let record = inner
                .state
                .projects
                .get(&project_id)
                .expect("live committed project");
            let committed = record.committed.as_ref().expect("live committed revision");
            let project_slot = record.slot;
            let revision = committed.revision;
            let base_bindings = committed.secret_bindings.clone();
            let affected_services = target_project.services_using(&slot);
            let op_record = OperationRecord {
                operation_id: operation_id.clone(),
                kind: OperationKind::Recreate,
                project_id: project_id.clone(),
                revision,
                generation,
                phase: OperationPhase::Intent,
                state: OperationState::Running,
                stage: OperationStage::Queued,
                failure: None,
                restored_previous: false,
                digest: Some(digest.clone()),
                slot: Some(slot.clone()),
                epoch: Some(epoch),
                owner: Some(owner),
                secret_instance: Some(instance.clone()),
                file_witness: None,
                services: affected_services.clone(),
                baseline_complete: false,
                prior_container_ids: BTreeMap::new(),
            };
            let queued_view = view(&op_record);
            let staged_digests: BTreeSet<String> =
                inner.staged.iter().map(|s| s.digest.clone()).collect();
            let persisted = commit_state(&self.shared, &mut inner, |state| {
                trim_journal(state)?;
                if state.operations.len() >= MAX_JOURNAL {
                    return Err(RejectCode::JournalFull);
                }
                reserve_secret_epoch(state, &staged_digests, &bkey, generation, epoch)?;
                state.operations.push(op_record);
                push_log(
                    &self.shared,
                    state,
                    &project_id,
                    LogEvent::OperationStarted,
                    None,
                    None,
                );
                Ok(())
            });
            if let Err(code) = persisted {
                return reject(id, code);
            }
            let plan = Plan::Recreate {
                project: target_project,
                slot: project_slot,
                revision,
                secret_slot: slot.clone(),
                digest: digest.clone(),
                epoch,
                generation,
                owner,
                instance,
                bytes,
                base_bindings,
                services: affected_services,
            };
            inner.running = Some(operation_id.clone());
            let shared = Arc::clone(&self.shared);
            let worker_id = operation_id.clone();
            let spawned = thread::Builder::new()
                .name("arx-compose-op".to_owned())
                .spawn(move || run_worker(shared, worker_id, plan));
            let recreate = if spawned.is_err() {
                inner.running = None;
                let _ = commit_settlement(
                    &self.shared,
                    &mut inner,
                    &operation_id,
                    Settlement::failed(FailureCode::WorkerUnavailable),
                );
                Some(view(find(&inner, &operation_id).expect("journaled")))
            } else {
                Some(queued_view)
            };
            return ComposeResponse::ComposeSecretAccepted {
                id,
                project_id,
                digest,
                slot,
                epoch,
                recreate,
            };
        }

        // Standalone delivery (`OperationKind::SecretPut`):
        // 1. Durably persist `EffectStarted` before touching tmpfs.
        let target_revision = committed_rev
            .filter(|c| c.digest == digest)
            .map_or(0, |c| c.revision);
        let op_record = OperationRecord {
            operation_id: operation_id.clone(),
            kind: OperationKind::SecretPut,
            project_id: project_id.clone(),
            revision: target_revision,
            generation,
            phase: OperationPhase::EffectStarted,
            state: OperationState::Running,
            stage: OperationStage::Applying,
            failure: None,
            restored_previous: false,
            digest: Some(digest.clone()),
            slot: Some(slot.clone()),
            epoch: Some(epoch),
            owner: Some(owner),
            secret_instance: Some(instance.clone()),
            file_witness: None,
            services: Vec::new(),
            baseline_complete: false,
            prior_container_ids: BTreeMap::new(),
        };
        let staged_digests: BTreeSet<String> =
            inner.staged.iter().map(|s| s.digest.clone()).collect();
        let persisted = commit_state(&self.shared, &mut inner, |state| {
            trim_journal(state)?;
            if state.operations.len() >= MAX_JOURNAL {
                return Err(RejectCode::JournalFull);
            }
            reserve_secret_epoch(state, &staged_digests, &bkey, generation, epoch)?;
            state.operations.push(op_record);
            Ok(())
        });
        if let Err(code) = persisted {
            return reject(id, code);
        }

        // 2. Publish immutable tmpfs file. An error may follow rename, so an
        // uncertain publish leaves the operation Ambiguous (never Accepted).
        let written = bytes.publish(
            lock(&self.shared.store).as_mut(),
            &project_id,
            &instance,
            owner,
        );
        let witness = match written {
            Ok(witness) => witness,
            Err(StoreError) => {
                let _ = commit_settlement(
                    &self.shared,
                    &mut inner,
                    &operation_id,
                    Settlement::ambiguous(),
                );
                return operation_state(id, find(&inner, &operation_id).expect("journaled"));
            }
        };

        // 3. Record `Observed` witness and atomically commit `Settled(Succeeded)`
        // + binding update in one durable transaction. Only durable settlement
        // may return `ComposeSecretAccepted`.
        let binding = SecretBinding {
            project_id: project_id.clone(),
            release_digest: digest.clone(),
            slot: slot.clone(),
            epoch,
            generation,
            uid: owner.0,
            gid: owner.1,
            instance,
            witness,
        };
        let mut settlement = Settlement::succeeded(LogEvent::SecretDelivered, None);
        settlement.recovery_required = None;
        settlement.file_witness = Some(witness);
        settlement.upsert_secret = Some(binding.clone());
        settlement.committed_bindings = updated_committed_bindings_if_matching(&inner, &binding);
        if commit_settlement(&self.shared, &mut inner, &operation_id, settlement).is_err() {
            return operation_state(id, find(&inner, &operation_id).expect("journaled"));
        }
        ComposeResponse::ComposeSecretAccepted {
            id,
            project_id,
            digest,
            slot,
            epoch,
            recreate: None,
        }
    }

    fn replay_secret_operation(
        &self,
        inner: &mut MutexGuard<'_, Inner>,
        id: u32,
        known: &OperationRecord,
        bytes: SecretBytes,
    ) -> ComposeResponse {
        let project_id = known.project_id.clone();
        let digest = known.digest.clone().unwrap_or_default();
        let slot = known.slot.clone().unwrap_or_default();
        let epoch = known.epoch.unwrap_or(0);
        let (Some(instance), Some(owner)) = (known.secret_instance.as_deref(), known.owner) else {
            return reject(id, RejectCode::OperationConflict);
        };
        match (known.kind, known.phase, known.state) {
            (OperationKind::Recreate, OperationPhase::Settled, OperationState::Succeeded) => {
                let cmp = lock(&self.shared.store).compare_secret(
                    &project_id,
                    instance,
                    owner,
                    bytes.as_slice(),
                );
                drop(bytes);
                match cmp {
                    SecretMatch::Match(w) if known.file_witness == Some(w) => {
                        ComposeResponse::ComposeSecretAccepted {
                            id,
                            project_id,
                            digest,
                            slot,
                            epoch,
                            recreate: Some(view(known)),
                        }
                    }
                    SecretMatch::Match(_) | SecretMatch::Mismatch => {
                        reject(id, RejectCode::OperationConflict)
                    }
                    SecretMatch::Missing => reject(id, RejectCode::SecretMissing),
                    SecretMatch::Error => reject(id, RejectCode::StoreFailure),
                }
            }
            (OperationKind::Recreate, _, _) => {
                if known.file_witness.is_some()
                    && lock(&self.shared.store).compare_secret(
                        &project_id,
                        instance,
                        owner,
                        bytes.as_slice(),
                    ) == SecretMatch::Mismatch
                {
                    return reject(id, RejectCode::OperationConflict);
                }
                drop(bytes);
                operation_state(id, known)
            }
            (OperationKind::SecretPut, OperationPhase::Settled, OperationState::Succeeded) => {
                let cmp = lock(&self.shared.store).compare_secret(
                    &project_id,
                    instance,
                    owner,
                    bytes.as_slice(),
                );
                drop(bytes);
                match cmp {
                    SecretMatch::Match(w) if known.file_witness == Some(w) => {
                        ComposeResponse::ComposeSecretAccepted {
                            id,
                            project_id,
                            digest,
                            slot,
                            epoch,
                            recreate: None,
                        }
                    }
                    SecretMatch::Match(_) | SecretMatch::Mismatch => {
                        reject(id, RejectCode::OperationConflict)
                    }
                    SecretMatch::Missing => reject(id, RejectCode::SecretMissing),
                    SecretMatch::Error => reject(id, RejectCode::StoreFailure),
                }
            }
            (OperationKind::SecretPut, OperationPhase::Settled, OperationState::Failed) => {
                drop(bytes);
                operation_state(id, known)
            }
            (OperationKind::SecretPut, _, OperationState::Ambiguous) => {
                if inner.running.is_some() {
                    return reject(id, RejectCode::Busy);
                }
                // Read-only resolution from actual tmpfs evidence; never writes.
                let cmp = lock(&self.shared.store).compare_secret(
                    &project_id,
                    instance,
                    owner,
                    bytes.as_slice(),
                );
                drop(bytes);
                match cmp {
                    SecretMatch::Match(witness) => {
                        let Some(binding) = build_binding_from_op(known, witness) else {
                            return operation_state(id, known);
                        };
                        let mut s = Settlement::succeeded(LogEvent::Reconciled, None);
                        s.recovery_required = None;
                        s.file_witness = Some(witness);
                        s.upsert_secret = Some(binding.clone());
                        s.committed_bindings =
                            updated_committed_bindings_if_matching(inner, &binding);
                        if commit_settlement(&self.shared, inner, &known.operation_id, s).is_err() {
                            return operation_state(
                                id,
                                find(inner, &known.operation_id).expect("journaled"),
                            );
                        }
                        ComposeResponse::ComposeSecretAccepted {
                            id,
                            project_id,
                            digest,
                            slot,
                            epoch,
                            recreate: None,
                        }
                    }
                    SecretMatch::Missing => {
                        let mut s = Settlement::failed(FailureCode::NotApplied);
                        s.event = LogEvent::Reconciled;
                        let _ = commit_settlement(&self.shared, inner, &known.operation_id, s);
                        operation_state(id, find(inner, &known.operation_id).expect("journaled"))
                    }
                    SecretMatch::Mismatch => {
                        let mut s = Settlement::failed(FailureCode::NotApplied);
                        s.event = LogEvent::Reconciled;
                        if inner.state.projects.contains_key(&project_id) {
                            s.recovery_required = Some(true);
                        }
                        let _ = commit_settlement(&self.shared, inner, &known.operation_id, s);
                        operation_state(id, find(inner, &known.operation_id).expect("journaled"))
                    }
                    SecretMatch::Error => operation_state(id, known),
                }
            }
            _ => {
                drop(bytes);
                operation_state(id, known)
            }
        }
    }
}

fn build_binding_from_op(
    operation: &OperationRecord,
    witness: SecretFileWitness,
) -> Option<SecretBinding> {
    Some(SecretBinding {
        project_id: operation.project_id.clone(),
        release_digest: operation.digest.clone()?,
        slot: operation.slot.clone()?,
        epoch: operation.epoch?,
        generation: operation.generation,
        uid: operation.owner?.0,
        gid: operation.owner?.1,
        instance: operation.secret_instance.clone()?,
        witness,
    })
}

fn updated_committed_bindings_if_matching(
    inner: &Inner,
    binding: &SecretBinding,
) -> Option<BTreeMap<String, SecretBinding>> {
    let record = inner.state.projects.get(&binding.project_id)?;
    let committed = record.committed.as_ref()?;
    if committed.digest != binding.release_digest {
        return None;
    }
    let mut updated = committed.secret_bindings.clone();
    updated.insert(binding.slot.clone(), binding.clone());
    Some(updated)
}

fn resolve_release_manifest(
    inner: &Inner,
    project_id: &str,
    digest: &str,
) -> Result<Project, RejectCode> {
    if let Some(staged) = inner.staged.iter().find(|s| s.digest == digest) {
        if staged.project.id() != project_id {
            return Err(RejectCode::ProjectMismatch);
        }
        return Ok(staged.project.clone());
    }
    if let Some(record) = inner.state.projects.get(project_id) {
        for rev in [&record.committed, &record.pending, &record.previous]
            .into_iter()
            .flatten()
        {
            if rev.digest == digest {
                return Ok(
                    Project::validate(rev.manifest.clone()).expect("state manifests are validated")
                );
            }
        }
        return Err(RejectCode::UnknownRelease);
    }
    // Check if the digest belongs to another project in persisted state.
    for (other_id, record) in &inner.state.projects {
        if other_id != project_id
            && [&record.committed, &record.pending, &record.previous]
                .into_iter()
                .flatten()
                .any(|rev| rev.digest == digest)
        {
            return Err(RejectCode::ProjectMismatch);
        }
    }
    if inner.staged.iter().any(|s| s.project.id() == project_id) {
        Err(RejectCode::UnknownRelease)
    } else {
        Err(RejectCode::UnknownProject)
    }
}

fn reserve_secret_epoch(
    state: &mut State,
    staged_digests: &BTreeSet<String>,
    key: &str,
    generation: u64,
    epoch: u64,
) -> Result<(), RejectCode> {
    ensure_secret_binding_capacity(state, staged_digests, key)?;
    if !state.pending_epochs.contains_key(key) && state.pending_epochs.len() >= MAX_SECRET_BINDINGS
    {
        return Err(RejectCode::StoreFailure);
    }
    state
        .pending_epochs
        .insert(key.to_owned(), (generation, epoch));
    Ok(())
}

fn ensure_secret_binding_capacity(
    state: &mut State,
    staged_digests: &BTreeSet<String>,
    incoming_key: &str,
) -> Result<(), RejectCode> {
    if state.secret_bindings.contains_key(incoming_key)
        || state.secret_bindings.len() < MAX_SECRET_BINDINGS
    {
        return Ok(());
    }
    // Preserve every binding referenced by committed, previous, pending, or
    // currently cached staged manifests before reclaiming expired entries.
    let mut pinned = BTreeSet::new();
    for (project_id, record) in &state.projects {
        for rev in [&record.committed, &record.previous, &record.pending]
            .into_iter()
            .flatten()
        {
            for slot in &rev.manifest.secrets {
                pinned.insert(binding_key(project_id, &rev.digest, slot));
            }
            for binding in rev.secret_bindings.values() {
                pinned.insert(binding_key(
                    &binding.project_id,
                    &binding.release_digest,
                    &binding.slot,
                ));
            }
        }
    }
    let mut removable: Vec<String> = state
        .secret_bindings
        .iter()
        .filter(|(k, v)| !pinned.contains(*k) && !staged_digests.contains(&v.release_digest))
        .map(|(k, _)| k.clone())
        .collect();
    removable.sort();
    for key in removable {
        state.secret_bindings.remove(&key);
        if state.secret_bindings.len() < MAX_SECRET_BINDINGS {
            return Ok(());
        }
    }
    Err(RejectCode::StoreFailure)
}

fn find<'a>(inner: &'a Inner, operation_id: &str) -> Option<&'a OperationRecord> {
    inner
        .state
        .operations
        .iter()
        .find(|operation| operation.operation_id == operation_id)
}

fn is_tombstoned(inner: &Inner, operation_id: &str) -> bool {
    inner.state.tombstones.iter().any(|id| id == operation_id)
}

fn existing(
    inner: &Inner,
    id: u32,
    operation_id: &str,
    kind: OperationKind,
    project_id: &str,
    revision: Option<u64>,
    digest: Option<&str>,
) -> Option<ComposeResponse> {
    if let Some(operation) = find(inner, operation_id) {
        if operation.kind == kind
            && operation.project_id == project_id
            && revision.is_none_or(|revision| revision == operation.revision)
            && digest.is_none_or(|digest| operation.digest.as_deref() == Some(digest))
        {
            return Some(operation_state(id, operation));
        }
        return Some(reject(id, RejectCode::OperationConflict));
    }
    if is_tombstoned(inner, operation_id) {
        return Some(reject(id, RejectCode::OperationConflict));
    }
    None
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
    if inner.store_uncertain {
        if inner
            .state
            .operations
            .iter()
            .any(|operation| operation.state == OperationState::Ambiguous)
        {
            return Some(RejectCode::ReconcileRequired);
        }
        return Some(RejectCode::StoreFailure);
    }
    None
}

fn trim_journal(state: &mut State) -> Result<(), RejectCode> {
    if state.operations.len() < MAX_JOURNAL {
        return Ok(());
    }
    let Some(oldest) = state.operations.iter().position(|operation| {
        operation.phase == OperationPhase::Settled
            && matches!(
                operation.state,
                OperationState::Succeeded | OperationState::Failed
            )
    }) else {
        return Err(RejectCode::JournalFull);
    };
    if state.tombstones.len() >= MAX_TOMBSTONES {
        return Err(RejectCode::JournalFull);
    }
    let removed = state.operations.remove(oldest);
    state.tombstones.push_back(removed.operation_id);
    Ok(())
}

fn check_volumes(state: &State, project: &Project) -> Option<RejectCode> {
    let mut total = state.volumes.len();
    for volume in &project.manifest().volumes {
        let name = volume_name(project.id(), &volume.name);
        match state.volumes.get(&name) {
            Some(record)
                if record.project != project.id()
                    || (record.uid, record.gid) != (volume.uid, volume.gid) =>
            {
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
    state: &mut State,
    project: &str,
    event: LogEvent,
    service: Option<&str>,
    failure: Option<FailureCode>,
) {
    let seq = state.next_log_seq;
    state.next_log_seq += 1;
    state.log.push_back(LogEntry {
        seq,
        unix_s: shared.clock.now_unix(),
        project_id: project.to_owned(),
        event,
        service: service.map(str::to_owned),
        failure,
    });
    while state.log.len() > MAX_LOG {
        state.log.pop_front();
    }
}

/// Transactional state transition: applies `mutate` to a candidate clone,
/// persists the serialized bytes to `store.save_state`, and swaps into memory
/// ONLY after persistence succeeds.
fn commit_state(
    shared: &Shared,
    inner: &mut Inner,
    mutate: impl FnOnce(&mut State) -> Result<(), RejectCode>,
) -> Result<(), RejectCode> {
    if inner.store_uncertain {
        return Err(RejectCode::StoreFailure);
    }
    let mut candidate = inner.state.clone();
    mutate(&mut candidate)?;
    let bytes = serde_json::to_vec(&candidate).map_err(|_| RejectCode::StoreFailure)?;
    if bytes.len() > MAX_STATE_BYTES {
        return Err(RejectCode::StoreFailure);
    }
    if lock(&shared.store).save_state(&bytes).is_err() {
        inner.store_uncertain = true;
        return Err(RejectCode::StoreFailure);
    }
    inner.state = candidate;
    Ok(())
}

fn apply_settlement_to_state(
    shared: &Shared,
    state: &mut State,
    operation_id: &str,
    settlement: Settlement,
) {
    let Some(operation) = state
        .operations
        .iter_mut()
        .find(|operation| operation.operation_id == operation_id)
    else {
        return;
    };
    operation.state = settlement.state;
    operation.failure = settlement.failure;
    operation.restored_previous = settlement.restored;
    if let Some(witness) = settlement.file_witness {
        operation.file_witness = Some(witness);
    }
    if settlement.state == OperationState::Ambiguous {
        if operation.phase == OperationPhase::Intent {
            // Preserve Intent when no live effect started.
        }
    } else {
        operation.phase = OperationPhase::Settled;
        operation.stage = OperationStage::Settled;
    }
    let project_id = operation.project_id.clone();
    if let Some(binding) = settlement.upsert_secret {
        let key = binding_key(&binding.project_id, &binding.release_digest, &binding.slot);
        state.pending_epochs.remove(&key);
        state.secret_bindings.insert(key, binding);
    }
    if let Some(record) = state.projects.get_mut(&project_id) {
        if settlement.promote
            && let Some(pending) = record.pending.take()
        {
            record.previous = record.committed.replace(pending);
        }
        if settlement.clear_pending || settlement.state == OperationState::Failed {
            record.pending = None;
        }
        if let Some(bindings) = settlement.committed_bindings
            && let Some(committed) = record.committed.as_mut()
        {
            committed.secret_bindings = bindings;
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
        if let Some(observed_ids) = settlement.observed_ids {
            record.observed_ids = observed_ids;
        }
    }
    push_log(
        shared,
        state,
        &project_id,
        settlement.event,
        None,
        settlement.failure,
    );
}

enum SettlementCommit {
    Existing(Settlement),
    Receipt(OperationRecord),
}

impl From<Settlement> for SettlementCommit {
    fn from(settlement: Settlement) -> Self {
        Self::Existing(settlement)
    }
}

/// Recover the actual side of a failed rename before computing another transition.
fn reload_uncertain(shared: &Shared, inner: &mut Inner) -> Result<(), RejectCode> {
    if inner.store_uncertain {
        let mut store = lock(&shared.store);
        let candidate = match store.load_state().map_err(|_| RejectCode::StoreFailure)? {
            Some(bytes) => State::load(&bytes).map_err(|_| RejectCode::StoreFailure)?,
            // A first admission can fail before creating state.json. Preserve
            // any known receipts and tombstones if the file is missing.
            None => inner.state.clone(),
        };
        candidate
            .confirm_checkpoint(store.as_mut())
            .map_err(|_| RejectCode::StoreFailure)?;
        inner.state = candidate;
        inner.store_uncertain = false;
    }
    Ok(())
}

/// THE single durable settlement path shared by workers, synchronous intake,
/// and read-only reconciliation. Commits the terminal receipt and all affected
/// project/binding state together before exposing either. If persistence fails,
/// memory is NOT advanced to the terminal receipt; the operation becomes
/// Ambiguous and dependent effects freeze until a durable checkpoint succeeds.
fn commit_settlement(
    shared: &Shared,
    inner: &mut Inner,
    operation_id: &str,
    settlement: impl Into<SettlementCommit>,
) -> Result<(), RejectCode> {
    if inner.store_uncertain {
        return Err(RejectCode::StoreFailure);
    }
    let mut candidate = inner.state.clone();
    match settlement.into() {
        SettlementCommit::Existing(s) => {
            apply_settlement_to_state(shared, &mut candidate, operation_id, s)
        }
        SettlementCommit::Receipt(receipt) => {
            trim_journal(&mut candidate)?;
            candidate.operations.push(receipt);
        }
    }
    let bytes = serde_json::to_vec(&candidate).map_err(|_| RejectCode::StoreFailure)?;
    if bytes.len() > MAX_STATE_BYTES || lock(&shared.store).save_state(&bytes).is_err() {
        mark_uncertain(inner, operation_id);
        return Err(RejectCode::StoreFailure);
    }
    inner.state = candidate;
    Ok(())
}

fn mark_uncertain(inner: &mut Inner, operation_id: &str) {
    inner.store_uncertain = true;
    if let Some(operation) = inner
        .state
        .operations
        .iter_mut()
        .find(|operation| operation.operation_id == operation_id)
    {
        operation.state = OperationState::Ambiguous;
    }
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
    // A worker may be finishing after a failed pre-effect checkpoint. Resolve
    // that save before committing its failure against the actual durable state.
    if reload_uncertain(&shared, &mut inner).is_ok() {
        let _ = commit_settlement(&shared, &mut inner, &operation_id, settlement);
    } else {
        mark_uncertain(&mut inner, &operation_id);
    }
    inner.running = None;
    drop(inner);
    shared.idle.notify_all();
}

struct Steps<'a> {
    shared: &'a Shared,
    operation_id: &'a str,
    runner: &'a mut dyn CommandRunner,
    last_observed: Vec<ObservedContainer>,
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

    fn checkpoint_effect_started(&self) -> Result<(), Step> {
        let mut inner = lock(&self.shared.inner);
        commit_state(self.shared, &mut inner, |state| {
            if let Some(operation) = state
                .operations
                .iter_mut()
                .find(|operation| operation.operation_id == self.operation_id)
            {
                operation.phase = OperationPhase::EffectStarted;
            }
            Ok(())
        })
        .map_err(|_| Step::Failed(FailureCode::StoreFailed))
    }

    fn log(&self, project: &str, event: LogEvent, service: Option<&str>) {
        let mut inner = lock(&self.shared.inner);
        push_log(self.shared, &mut inner.state, project, event, service, None);
    }

    fn execute(&mut self, plan: Plan) -> Settlement {
        match plan {
            Plan::Deploy {
                project,
                slot,
                revision,
                bindings,
                prior,
            } => self.deploy(&project, slot, revision, &bindings, prior),
            Plan::Start {
                project,
                slot,
                revision,
                bindings,
            } => self.start(&project, slot, revision, &bindings),
            Plan::Stop { project, revision } => self.stop(&project, revision),
            Plan::Down { project, revision } => self.down(&project, revision),
            Plan::Recreate {
                project,
                slot,
                revision,
                secret_slot,
                digest,
                epoch,
                generation,
                owner,
                instance,
                bytes,
                base_bindings,
                services,
            } => self.recreate(
                &project,
                slot,
                revision,
                &secret_slot,
                &digest,
                epoch,
                generation,
                owner,
                &instance,
                bytes,
                base_bindings,
                &services,
            ),
        }
    }

    fn stop(&mut self, project: &str, revision: u64) -> Settlement {
        if let Err(step) = self.checkpoint_effect_started() {
            return step.into();
        }
        self.stage(OperationStage::Stopping);
        if let Err(step) = self.apply(
            &engine::compose_stop(project, revision),
            FailureCode::StopFailed,
        ) {
            return step.into();
        }
        match self.stop_remaining_attributed(project, revision) {
            Ok(containers) => Settlement::succeeded(LogEvent::Stopped, Some(Desired::Stopped))
                .observed_containers(&containers),
            Err(step) => {
                let mut settlement = Settlement::from(step);
                if settlement.state == OperationState::Failed {
                    settlement.recovery_required = Some(true);
                }
                let observed = std::mem::take(&mut self.last_observed);
                settlement.observed_containers(&observed)
            }
        }
    }

    fn down(&mut self, project: &str, revision: u64) -> Settlement {
        if let Err(step) = self.checkpoint_effect_started() {
            return step.into();
        }
        self.stage(OperationStage::Removing);
        match self.execute_down_with_proof(project, revision) {
            Ok(()) => Settlement::succeeded(LogEvent::Removed, Some(Desired::Down))
                .observed_containers(&[]),
            Err(step) => {
                let mut settlement = Settlement::from(step);
                if settlement.state == OperationState::Failed {
                    settlement.recovery_required = Some(true);
                }
                let observed = std::mem::take(&mut self.last_observed);
                settlement.observed_containers(&observed)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn recreate(
        &mut self,
        project: &Project,
        slot: u8,
        revision: u64,
        secret_slot: &str,
        digest: &str,
        epoch: u64,
        generation: u64,
        owner: (u32, u32),
        instance: &str,
        bytes: SecretPublication,
        base_bindings: BTreeMap<String, SecretBinding>,
        services: &[String],
    ) -> Settlement {
        self.stage(OperationStage::Recreating);
        // 1. Capture a FRESH, complete pre-effect container inventory. Never
        // fall back to cached container IDs when the fresh baseline fails.
        let Some(before) = self.ps_attributed_strict(project.id(), revision) else {
            drop(bytes);
            return Settlement::failed(FailureCode::EngineTimedOut);
        };
        let mut fresh_prior: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for service in services {
            let ids: Vec<String> = before
                .iter()
                .filter(|c| c.view.name == *service)
                .filter_map(|c| c.id.clone())
                .collect();
            if ids.is_empty() {
                drop(bytes);
                return Settlement::failed(FailureCode::UpFailed);
            }
            fresh_prior.insert(service.clone(), ids);
        }
        // Durably persist the fresh pre-effect baseline before publishing the
        // new secret file or running `--force-recreate`.
        {
            let mut inner = lock(&self.shared.inner);
            if commit_state(self.shared, &mut inner, |state| {
                if let Some(op) = state
                    .operations
                    .iter_mut()
                    .find(|op| op.operation_id == self.operation_id)
                {
                    op.prior_container_ids = fresh_prior.clone();
                    op.baseline_complete = true;
                }
                Ok(())
            })
            .is_err()
            {
                drop(bytes);
                return Settlement::failed(FailureCode::StoreFailed);
            }
        }

        // 2. Publish the new immutable tmpfs secret file and render the
        // rotation-scoped compose file (keeping `compose.yaml` untouched until
        // durable settlement).
        let written = bytes.publish(
            lock(&self.shared.store).as_mut(),
            project.id(),
            instance,
            owner,
        );
        let Ok(witness) = written else {
            return Settlement::failed(FailureCode::StoreFailed);
        };
        let new_binding = SecretBinding {
            project_id: project.id().to_owned(),
            release_digest: digest.to_owned(),
            slot: secret_slot.to_owned(),
            epoch,
            generation,
            uid: owner.0,
            gid: owner.1,
            instance: instance.to_owned(),
            witness,
        };
        let mut new_bindings = base_bindings;
        new_bindings.insert(secret_slot.to_owned(), new_binding.clone());
        let Ok(rot_yaml) = self.render_pinned(project, revision, slot, &new_bindings) else {
            return Settlement::failed(FailureCode::StoreFailed);
        };
        if lock(&self.shared.store)
            .write_rotation(project.id(), revision, self.operation_id, &rot_yaml)
            .is_err()
        {
            return Settlement::failed(FailureCode::StoreFailed);
        }

        // 3. Persist `EffectStarted` + `file_witness` BEFORE `--force-recreate`.
        {
            let mut inner = lock(&self.shared.inner);
            if commit_state(self.shared, &mut inner, |state| {
                if let Some(op) = state
                    .operations
                    .iter_mut()
                    .find(|op| op.operation_id == self.operation_id)
                {
                    op.phase = OperationPhase::EffectStarted;
                    op.file_witness = Some(witness);
                }
                push_log(
                    self.shared,
                    state,
                    project.id(),
                    LogEvent::SecretDelivered,
                    None,
                    None,
                );
                Ok(())
            })
            .is_err()
            {
                return Settlement::failed(FailureCode::StoreFailed);
            }
        }

        // 4. Execute `--force-recreate` ONLY on the affected services.
        let rot_file = layout::rotation_compose_file(project.id(), revision, self.operation_id);
        let invocation = engine::compose_recreate_file(project.id(), revision, &rot_file, services);
        if let Err(step) = self.apply(&invocation, FailureCode::UpFailed) {
            let mut settlement = Settlement::from(step);
            if settlement.state == OperationState::Failed {
                settlement.recovery_required = Some(true);
            }
            return settlement;
        }

        // 5. Verify fresh container IDs AND exact read-only mount witness of
        // the new immutable secret file AND health of all services.
        self.stage(OperationStage::HealthCheck);
        match self.prove_health_recreate(
            project,
            revision,
            slot,
            &new_bindings,
            services,
            &fresh_prior,
        ) {
            Ok(containers) => {
                // Refresh `compose.yaml` to match the promoted live binding.
                let _ = lock(&self.shared.store).write_revision(project.id(), revision, &rot_yaml);
                let mut s = Settlement::succeeded(LogEvent::Recreated, Some(Desired::Started))
                    .observed_containers(&containers);
                s.file_witness = Some(witness);
                s.upsert_secret = Some(new_binding);
                s.committed_bindings = Some(new_bindings);
                s
            }
            Err(step) => {
                let mut settlement = Settlement::from(step);
                if settlement.state == OperationState::Failed {
                    settlement.recovery_required = Some(true);
                }
                let observed = std::mem::take(&mut self.last_observed);
                settlement.observed_containers(&observed)
            }
        }
    }

    /// Render only from verified instance names and pinned file witnesses.
    /// Every launching path writes this projection to its selected Compose file.
    fn render_pinned(
        &self,
        project: &Project,
        revision: u64,
        slot: u8,
        bindings: &BTreeMap<String, SecretBinding>,
    ) -> Result<String, StoreError> {
        let mut store = lock(&self.shared.store);
        for name in &project.manifest().secrets {
            let binding = bindings.get(name).ok_or(StoreError)?;
            let owner = project.slot_owner(name).ok_or(StoreError)?;
            if binding.project_id != project.id()
                || binding.slot != *name
                || (binding.uid, binding.gid) != owner
                || binding.instance
                    != secret_instance(
                        name,
                        &binding.release_digest,
                        binding.epoch,
                        binding.generation,
                    )
                || store.stat_secret(project.id(), &binding.instance, owner)
                    != Some(binding.witness)
            {
                return Err(StoreError);
            }
        }
        Ok(render(
            project,
            revision,
            slot,
            &secret_instance_map(bindings),
        ))
    }

    fn write_pinned_revision(
        &self,
        project: &Project,
        revision: u64,
        slot: u8,
        bindings: &BTreeMap<String, SecretBinding>,
    ) -> Result<(), StoreError> {
        let yaml = self.render_pinned(project, revision, slot, bindings)?;
        lock(&self.shared.store).write_revision(project.id(), revision, &yaml)
    }

    fn deploy(
        &mut self,
        project: &Project,
        slot: u8,
        revision: u64,
        bindings: &BTreeMap<String, SecretBinding>,
        prior: Option<(Project, u64, Desired, BTreeMap<String, SecretBinding>)>,
    ) -> Settlement {
        let id = project.id();
        self.stage(OperationStage::Rendering);
        if self
            .write_pinned_revision(project, revision, slot, bindings)
            .is_err()
        {
            return Settlement::failed(FailureCode::StoreFailed);
        }
        {
            let mut inner = lock(&self.shared.inner);
            if commit_state(self.shared, &mut inner, |state| {
                if let Some(record) = state.projects.get_mut(id) {
                    record.last_rendered = Some(revision);
                }
                Ok(())
            })
            .is_err()
            {
                return Settlement::failed(FailureCode::StoreFailed);
            }
        }
        if let Err(step) = self.prepare(project) {
            return step.into();
        }
        if let Err(step) = self.checkpoint_effect_started() {
            return step.into();
        }
        self.stage(OperationStage::Applying);
        match self.apply(&engine::compose_up(id, revision), FailureCode::UpFailed) {
            Ok(()) => self.log(id, LogEvent::Applied, None),
            Err(Step::Ambiguous) => return Settlement::ambiguous(),
            Err(Step::Failed(code)) => return self.restore(code, project, slot, revision, prior),
        }
        self.stage(OperationStage::HealthCheck);
        match self.prove_health(project, revision, slot, bindings, true) {
            Ok(containers) => {
                self.log(id, LogEvent::HealthPassed, None);
                let mut settlement =
                    Settlement::succeeded(LogEvent::Promoted, Some(Desired::Started))
                        .observed_containers(&containers);
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

    /// After a failed apply or health proof: preserve the prior lifecycle
    /// state and bindings. A previously started compatible revision is restored
    /// with its own pinned bindings; a deliberately stopped or down release
    /// stays stopped or down (using the shared Down proof). Data is kept.
    fn restore(
        &mut self,
        code: FailureCode,
        project: &Project,
        slot: u8,
        revision: u64,
        prior: Option<(Project, u64, Desired, BTreeMap<String, SecretBinding>)>,
    ) -> Settlement {
        let id = project.id();
        let failed_views = std::mem::take(&mut self.last_observed);
        let mut settlement = Settlement::failed(code);
        settlement.clear_pending = true;
        match prior {
            Some((previous, previous_revision, Desired::Started, previous_bindings))
                if compatible_layout(&previous, project) =>
            {
                self.stage(OperationStage::Restoring);
                let restored = self
                    .write_pinned_revision(&previous, previous_revision, slot, &previous_bindings)
                    .is_ok()
                    && self
                        .apply(
                            &engine::compose_up(id, previous_revision),
                            FailureCode::UpFailed,
                        )
                        .is_ok()
                    && match self.prove_health(
                        &previous,
                        previous_revision,
                        slot,
                        &previous_bindings,
                        true,
                    ) {
                        Ok(containers) => {
                            settlement = settlement.observed_containers(&containers);
                            true
                        }
                        Err(_) => false,
                    };
                if restored {
                    self.log(id, LogEvent::Restored, None);
                }
                settlement.restored = restored;
                settlement.desired = Some(Desired::Started);
                settlement.recovery_required = Some(!restored);
                if !restored {
                    let observed = std::mem::take(&mut self.last_observed);
                    settlement = settlement.observed_containers(&observed);
                }
                settlement
            }
            Some((_, _, Desired::Stopped, _)) => {
                self.stage(OperationStage::Stopping);
                let stopped = self
                    .apply(&engine::compose_stop(id, revision), FailureCode::StopFailed)
                    .and_then(|()| self.stop_remaining_attributed(id, revision));
                match stopped {
                    Ok(containers) => {
                        self.log(id, LogEvent::Restored, None);
                        settlement.restored = true;
                        settlement.desired = Some(Desired::Stopped);
                        settlement.recovery_required = Some(false);
                        settlement.observed_containers(&containers)
                    }
                    Err(_) => {
                        settlement.restored = false;
                        settlement.desired = Some(Desired::Stopped);
                        settlement.recovery_required = Some(true);
                        let observed = std::mem::take(&mut self.last_observed);
                        if observed.is_empty() {
                            settlement.observed_containers(&failed_views)
                        } else {
                            settlement.observed_containers(&observed)
                        }
                    }
                }
            }
            Some((_, _, Desired::Down, _)) => {
                self.stage(OperationStage::Removing);
                match self.execute_down_with_proof(id, revision) {
                    Ok(()) => {
                        self.log(id, LogEvent::Restored, None);
                        settlement.restored = true;
                        settlement.desired = Some(Desired::Down);
                        settlement.recovery_required = Some(false);
                        settlement.observed_containers(&[])
                    }
                    Err(_) => {
                        settlement.restored = false;
                        settlement.desired = Some(Desired::Down);
                        settlement.recovery_required = Some(true);
                        let observed = std::mem::take(&mut self.last_observed);
                        if observed.is_empty() {
                            settlement.observed_containers(&failed_views)
                        } else {
                            settlement.observed_containers(&observed)
                        }
                    }
                }
            }
            other => {
                let had_started = matches!(other, Some((_, _, Desired::Started, _)));
                self.stage(OperationStage::Stopping);
                let stopped = self
                    .apply(&engine::compose_stop(id, revision), FailureCode::StopFailed)
                    .and_then(|()| self.stop_remaining_attributed(id, revision));
                match stopped {
                    Ok(containers) => {
                        settlement.recovery_required = Some(had_started);
                        settlement.observed_containers(&containers)
                    }
                    Err(_) => {
                        settlement.recovery_required = Some(true);
                        settlement.observed_containers(&failed_views)
                    }
                }
            }
        }
    }

    fn start(
        &mut self,
        project: &Project,
        slot: u8,
        revision: u64,
        bindings: &BTreeMap<String, SecretBinding>,
    ) -> Settlement {
        // Re-render from the current pinned bindings so post-reboot
        // new-generation secret instances are mounted on Start.
        if self
            .write_pinned_revision(project, revision, slot, bindings)
            .is_err()
        {
            return Settlement::failed(FailureCode::StoreFailed);
        }
        if let Err(step) = self.prepare(project) {
            return step.into();
        }
        if let Err(step) = self.checkpoint_effect_started() {
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
        match self.prove_health(project, revision, slot, bindings, true) {
            Ok(containers) => {
                let mut s = Settlement::succeeded(LogEvent::HealthPassed, Some(Desired::Started))
                    .observed_containers(&containers);
                s.committed_bindings = Some(bindings.clone());
                s
            }
            Err(step) => {
                let observed = std::mem::take(&mut self.last_observed);
                Settlement::from(step).observed_containers(&observed)
            }
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
            RunResult::OutputIncomplete { .. } | RunResult::TimedOut | RunResult::Signaled => {
                return Err(Step::Failed(FailureCode::EngineTimedOut));
            }
            RunResult::LaunchFailed => return Err(Step::Failed(FailureCode::EngineLaunchFailed)),
        }
        match self.runner.run(&engine::image_pull(image)) {
            RunResult::Exited { code: 0, .. } => {}
            RunResult::TimedOut => return Err(Step::Failed(FailureCode::PullTimedOut)),
            RunResult::Exited { .. } | RunResult::OutputIncomplete { .. } | RunResult::Signaled => {
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

    fn volume_observation(&mut self, name: &str, expected_token: &str) -> VolumeObservation {
        match self.runner.run(&engine::volume_list()) {
            RunResult::Exited { code: 0, stdout } => {
                parse_volume_inventory(&stdout, name, expected_token)
            }
            _ => VolumeObservation::Unavailable,
        }
    }

    /// Durably charges a volume before creation so subsequent failures can
    /// never leave an uncharged volume past the eight-slot cap.
    fn charge_volume(&self, project: &str, name: &str, volume: &VolumeSpec) -> Result<bool, Step> {
        let mut inner = lock(&self.shared.inner);
        if let Some(existing) = inner.state.volumes.get(name) {
            if existing.project != project
                || (existing.uid, existing.gid) != (volume.uid, volume.gid)
            {
                return Err(Step::Failed(FailureCode::VolumeCollision));
            }
            return Ok(false);
        }
        let result = commit_state(self.shared, &mut inner, |state| {
            if state.volumes.len() >= MAX_VOLUMES {
                return Err(RejectCode::VolumeLimit);
            }
            state.volumes.insert(
                name.to_owned(),
                VolumeRecord {
                    project: project.to_owned(),
                    volume: volume.name.clone(),
                    uid: volume.uid,
                    gid: volume.gid,
                },
            );
            Ok(())
        });
        match result {
            Ok(()) => Ok(true),
            Err(RejectCode::VolumeLimit) => Err(Step::Failed(FailureCode::VolumeFailed)),
            Err(_) => Err(Step::Failed(FailureCode::StoreFailed)),
        }
    }

    /// Releases a newly created volume charge ONLY with durable persistence.
    /// If persistence fails, the charge stays in place and freezes dependent
    /// budget work (`store_uncertain`).
    fn release_volume_charge(&self, name: &str) {
        let mut inner = lock(&self.shared.inner);
        if !inner.state.volumes.contains_key(name) {
            return;
        }
        if commit_state(self.shared, &mut inner, |state| {
            state.volumes.remove(name);
            Ok(())
        })
        .is_err()
        {
            inner.store_uncertain = true;
        }
    }

    /// Conservative volume provisioning: uses a complete `volume ls` inventory
    /// to distinguish Absence, OwnedPresent, Collision, and Unavailable. A
    /// failed or truncated query never authorizes create, reuse, preparation,
    /// or uncharging.
    fn ensure_volume(&mut self, project: &str, volume: &VolumeSpec) -> Result<(), Step> {
        let name = volume_name(project, &volume.name);
        let token = volume_token(project, &volume.name, volume.uid, volume.gid);
        match self.volume_observation(&name, &token) {
            VolumeObservation::Unavailable => return Err(Step::Failed(FailureCode::VolumeFailed)),
            VolumeObservation::Collision => return Err(Step::Failed(FailureCode::VolumeCollision)),
            VolumeObservation::OwnedPresent => {
                self.charge_volume(project, &name, volume)?;
            }
            VolumeObservation::Absent => {
                let newly_charged = self.charge_volume(project, &name, volume)?;
                match self.runner.run(&engine::volume_create(&name, &token)) {
                    RunResult::Exited { code: 0, .. } => {
                        match self.volume_observation(&name, &token) {
                            VolumeObservation::OwnedPresent => {}
                            VolumeObservation::Collision => {
                                return Err(Step::Failed(FailureCode::VolumeCollision));
                            }
                            VolumeObservation::Absent | VolumeObservation::Unavailable => {
                                return Err(Step::Failed(FailureCode::VolumeFailed));
                            }
                        }
                    }
                    RunResult::LaunchFailed => {
                        // Process never started; release ONLY after a fresh
                        // complete absence proof from `volume ls`.
                        if newly_charged
                            && self.volume_observation(&name, &token) == VolumeObservation::Absent
                        {
                            self.release_volume_charge(&name);
                        }
                        return Err(Step::Failed(FailureCode::EngineLaunchFailed));
                    }
                    RunResult::Exited { .. }
                    | RunResult::OutputIncomplete { .. }
                    | RunResult::TimedOut
                    | RunResult::Signaled => {
                        // Creation may have had effects; keep the reservation.
                        return Err(Step::Failed(FailureCode::VolumeFailed));
                    }
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
        self.log(project, LogEvent::VolumeReady, None);
        Ok(())
    }

    /// A deadline kill or foreign signal leaves the effect unknown.
    fn apply(&mut self, invocation: &engine::Invocation, failure: FailureCode) -> Result<(), Step> {
        match self.runner.run(invocation) {
            RunResult::Exited { code: 0, .. } => Ok(()),
            RunResult::Exited { .. } | RunResult::OutputIncomplete { .. } => {
                Err(Step::Failed(failure))
            }
            RunResult::LaunchFailed => Err(Step::Failed(FailureCode::EngineLaunchFailed)),
            RunResult::TimedOut | RunResult::Signaled => Err(Step::Ambiguous),
        }
    }

    fn ps_attributed_strict(
        &mut self,
        project: &str,
        revision: u64,
    ) -> Option<Vec<ObservedContainer>> {
        match self.runner.run(&engine::compose_ps(project, revision)) {
            RunResult::Exited { code: 0, stdout } => parse_ps_attributed(&stdout, project),
            _ => None,
        }
    }

    fn observe_network_strict(&mut self, project: &str) -> NetworkObservation {
        match self.runner.run(&engine::network_list()) {
            RunResult::Exited { code: 0, stdout } => parse_network_inventory(&stdout, project),
            _ => NetworkObservation::Unavailable,
        }
    }

    /// Shared read-only Down proof used across execution, rollback to Down,
    /// and read-only reconciliation: requires BOTH zero broker-attributed
    /// containers (including retired/stopped) AND absence of the generated
    /// project network.
    fn observe_down(&mut self, project: &str, revision: u64) -> DownEvidence {
        let Some(containers) = self.ps_attributed_strict(project, revision) else {
            return DownEvidence::Unavailable;
        };
        match self.observe_network_strict(project) {
            NetworkObservation::Unavailable => DownEvidence::Unavailable,
            NetworkObservation::ForeignConflict => DownEvidence::ForeignConflict { containers },
            NetworkObservation::Absent => {
                if containers.is_empty() {
                    DownEvidence::CompleteAbsent
                } else {
                    DownEvidence::Partial {
                        containers,
                        owned_networks: Vec::new(),
                    }
                }
            }
            NetworkObservation::OwnedPresent(owned_networks) => DownEvidence::Partial {
                containers,
                owned_networks,
            },
        }
    }

    fn execute_down_with_proof(&mut self, project: &str, revision: u64) -> Result<(), Step> {
        self.apply(
            &engine::compose_down(project, revision),
            FailureCode::DownFailed,
        )?;
        match self.observe_down(project, revision) {
            DownEvidence::CompleteAbsent => return Ok(()),
            DownEvidence::Unavailable => return Err(Step::Failed(FailureCode::DownFailed)),
            DownEvidence::ForeignConflict { containers } => {
                // Never remove a foreign exact-name network and never succeed.
                self.last_observed = containers;
                return Err(Step::Failed(FailureCode::DownFailed));
            }
            DownEvidence::Partial {
                containers,
                owned_networks,
            } => {
                for container in &containers {
                    let Some(cid) = container.id.as_deref() else {
                        self.last_observed = containers;
                        return Err(Step::Failed(FailureCode::DownFailed));
                    };
                    if is_active_state(container.view.state) {
                        self.apply(&engine::container_stop(cid), FailureCode::DownFailed)?;
                    }
                    self.apply(&engine::container_rm(cid), FailureCode::DownFailed)?;
                }
                if !containers.is_empty() {
                    // Re-run fixed down once retired containers detach.
                    self.apply(
                        &engine::compose_down(project, revision),
                        FailureCode::DownFailed,
                    )?;
                }
                // Remove any remaining verified broker-owned network by ID.
                match self.observe_down(project, revision) {
                    DownEvidence::CompleteAbsent => return Ok(()),
                    DownEvidence::Partial {
                        containers: remaining,
                        owned_networks: remaining_nets,
                    } if remaining.is_empty() && !remaining_nets.is_empty() => {
                        for net_id in remaining_nets {
                            self.apply(&engine::network_rm(&net_id), FailureCode::DownFailed)?;
                        }
                    }
                    DownEvidence::Partial { containers: c, .. }
                    | DownEvidence::ForeignConflict { containers: c } => {
                        self.last_observed = c;
                        return Err(Step::Failed(FailureCode::DownFailed));
                    }
                    DownEvidence::Unavailable => {
                        return Err(Step::Failed(FailureCode::DownFailed));
                    }
                }
                let _ = owned_networks;
            }
        }
        match self.observe_down(project, revision) {
            DownEvidence::CompleteAbsent => Ok(()),
            DownEvidence::Partial { containers, .. }
            | DownEvidence::ForeignConflict { containers } => {
                self.last_observed = containers;
                Err(Step::Failed(FailureCode::DownFailed))
            }
            DownEvidence::Unavailable => Err(Step::Failed(FailureCode::DownFailed)),
        }
    }

    fn stop_retired_containers(
        &mut self,
        project: &Project,
        containers: &[ObservedContainer],
    ) -> Result<bool, Step> {
        let retired: Vec<&ObservedContainer> = containers
            .iter()
            .filter(|container| {
                !project
                    .manifest()
                    .services
                    .iter()
                    .any(|service| service.name == container.view.name)
            })
            .collect();
        if retired.is_empty() {
            return Ok(false);
        }
        for container in retired {
            let Some(cid) = container.id.as_deref() else {
                return Err(Step::Failed(FailureCode::DownFailed));
            };
            if is_active_state(container.view.state) {
                self.apply(&engine::container_stop(cid), FailureCode::StopFailed)?;
            }
            self.apply(&engine::container_rm(cid), FailureCode::DownFailed)?;
        }
        Ok(true)
    }

    fn stop_remaining_attributed(
        &mut self,
        project: &str,
        revision: u64,
    ) -> Result<Vec<ObservedContainer>, Step> {
        let Some(mut containers) = self.ps_attributed_strict(project, revision) else {
            return Err(Step::Failed(FailureCode::StopFailed));
        };
        let active: Vec<ObservedContainer> = containers
            .iter()
            .filter(|container| is_active_state(container.view.state))
            .cloned()
            .collect();
        if !active.is_empty() {
            for container in &active {
                let Some(cid) = container.id.as_deref() else {
                    self.last_observed = containers;
                    return Err(Step::Failed(FailureCode::StopFailed));
                };
                self.apply(&engine::container_stop(cid), FailureCode::StopFailed)?;
            }
            let Some(rechecked) = self.ps_attributed_strict(project, revision) else {
                self.last_observed = containers;
                return Err(Step::Failed(FailureCode::StopFailed));
            };
            containers = rechecked;
        }
        if containers
            .iter()
            .any(|container| is_active_state(container.view.state))
        {
            self.last_observed = containers;
            return Err(Step::Failed(FailureCode::StopFailed));
        }
        Ok(containers)
    }

    /// Verifies every secret-bearing service mounts its exact pinned immutable
    /// tmpfs source file read-only at `/run/secrets/<slot>`, and that the
    /// immutable tmpfs file still matches its non-secret inode witness.
    fn verify_secret_mounts(
        &mut self,
        project: &Project,
        containers: &[ObservedContainer],
        bindings: &BTreeMap<String, SecretBinding>,
    ) -> Option<bool> {
        for service in &project.manifest().services {
            if service.secrets.is_empty() {
                continue;
            }
            let matching: Vec<&ObservedContainer> = containers
                .iter()
                .filter(|c| c.view.name == service.name)
                .collect();
            if matching.len() != 1 {
                return Some(false);
            }
            let Some(cid) = matching[0].id.as_deref() else {
                return Some(false);
            };
            let mounts = match self.runner.run(&engine::container_inspect_mounts(cid)) {
                RunResult::Exited { code: 0, stdout } => parse_container_mounts(&stdout, cid)?,
                RunResult::OutputIncomplete { .. }
                | RunResult::TimedOut
                | RunResult::Signaled
                | RunResult::LaunchFailed => return None,
                RunResult::Exited { .. } => return None,
            };
            let mut store = lock(&self.shared.store);
            for slot in &service.secrets {
                let Some(binding) = bindings.get(slot) else {
                    return Some(false);
                };
                let expected_owner = (service.uid, service.gid);
                if (binding.uid, binding.gid) != expected_owner {
                    return Some(false);
                }
                if store.stat_secret(project.id(), &binding.instance, expected_owner)
                    != Some(binding.witness)
                {
                    return Some(false);
                }
                let expected_source = secret_file(project.id(), &binding.instance);
                let expected_dest = format!("/run/secrets/{slot}");
                let mounted = mounts.iter().any(|m| {
                    m.mount_type == "bind"
                        && m.source == expected_source
                        && m.destination == expected_dest
                        && !m.rw
                });
                if !mounted {
                    return Some(false);
                }
            }
        }
        Some(true)
    }

    fn ready(
        &mut self,
        project: &Project,
        revision: u64,
        slot: u8,
        containers: &[ObservedContainer],
        bindings: &BTreeMap<String, SecretBinding>,
    ) -> Option<bool> {
        let has_retired = containers.iter().any(|container| {
            !project
                .manifest()
                .services
                .iter()
                .any(|service| service.name == container.view.name)
        });
        if has_retired {
            return Some(false);
        }
        let views = to_service_views(containers);
        let running = project.manifest().services.iter().all(|service| {
            views.iter().any(|view| {
                view.name == service.name
                    && view.state == ServiceState::Running
                    && view.revision == Some(revision)
                    && matches!(view.health, HealthState::None | HealthState::Healthy)
            })
        });
        if !running {
            return Some(false);
        }
        if !self.verify_secret_mounts(project, containers, bindings)? {
            return Some(false);
        }
        let probed = project
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
            );
        Some(probed)
    }

    fn verify_live_services(
        &mut self,
        project: &Project,
        revision: u64,
        slot: u8,
        bindings: &BTreeMap<String, SecretBinding>,
    ) -> LiveEvidence {
        let Some(containers) = self.ps_attributed_strict(project.id(), revision) else {
            return LiveEvidence::Unavailable;
        };
        match self.ready(project, revision, slot, &containers, bindings) {
            None => LiveEvidence::Unavailable,
            Some(true) => LiveEvidence::Satisfied(containers),
            Some(false) => LiveEvidence::NotSatisfied(containers),
        }
    }

    fn verify_live_recreate(
        &mut self,
        project: &Project,
        revision: u64,
        slot: u8,
        bindings: &BTreeMap<String, SecretBinding>,
        services: &[String],
        prior_ids: &BTreeMap<String, Vec<String>>,
    ) -> LiveEvidence {
        let Some(containers) = self.ps_attributed_strict(project.id(), revision) else {
            return LiveEvidence::Unavailable;
        };
        if !containers_replaced(&containers, services, prior_ids) {
            return LiveEvidence::NotSatisfied(containers);
        }
        match self.ready(project, revision, slot, &containers, bindings) {
            None => LiveEvidence::Unavailable,
            Some(true) => LiveEvidence::Satisfied(containers),
            Some(false) => LiveEvidence::NotSatisfied(containers),
        }
    }

    /// Health proof before promotion: every declared service runs `revision`,
    /// retired attributed containers are stopped and removed, every secret
    /// mount matches its pinned immutable tmpfs source read-only, and every
    /// HTTP service answers 2xx on its loopback port.
    fn prove_health(
        &mut self,
        project: &Project,
        revision: u64,
        slot: u8,
        bindings: &BTreeMap<String, SecretBinding>,
        cleanup_retired: bool,
    ) -> Result<Vec<ObservedContainer>, Step> {
        let mut cleaned_retired = false;
        for attempt in 0..HEALTH_ATTEMPTS {
            if attempt > 0 {
                self.runner.pause(HEALTH_INTERVAL);
            }
            let Some(mut containers) = self.ps_attributed_strict(project.id(), revision) else {
                continue;
            };
            if cleanup_retired && !cleaned_retired {
                cleaned_retired = true;
                if self.stop_retired_containers(project, &containers)? {
                    let Some(rechecked) = self.ps_attributed_strict(project.id(), revision) else {
                        continue;
                    };
                    containers = rechecked;
                }
            }
            if self.ready(project, revision, slot, &containers, bindings) == Some(true) {
                return Ok(containers);
            }
            self.last_observed = containers;
        }
        Err(Step::Failed(FailureCode::HealthFailed))
    }

    fn prove_health_recreate(
        &mut self,
        project: &Project,
        revision: u64,
        slot: u8,
        bindings: &BTreeMap<String, SecretBinding>,
        services: &[String],
        prior_container_ids: &BTreeMap<String, Vec<String>>,
    ) -> Result<Vec<ObservedContainer>, Step> {
        for attempt in 0..HEALTH_ATTEMPTS {
            if attempt > 0 {
                self.runner.pause(HEALTH_INTERVAL);
            }
            match self.verify_live_recreate(
                project,
                revision,
                slot,
                bindings,
                services,
                prior_container_ids,
            ) {
                LiveEvidence::Satisfied(containers) => return Ok(containers),
                LiveEvidence::NotSatisfied(containers) => self.last_observed = containers,
                LiveEvidence::Unavailable => {}
            }
        }
        Err(Step::Failed(FailureCode::HealthFailed))
    }
}

fn compatible_layout(previous: &Project, next: &Project) -> bool {
    previous.manifest().volumes == next.manifest().volumes
        && previous.manifest().secrets == next.manifest().secrets
        && previous
            .manifest()
            .secrets
            .iter()
            .all(|slot| previous.slot_owner(slot) == next.slot_owner(slot))
}

fn is_active_state(state: ServiceState) -> bool {
    matches!(
        state,
        ServiceState::Running
            | ServiceState::Restarting
            | ServiceState::Paused
            | ServiceState::Unknown
    )
}

/// Proves that every affected service is running a single container with a
/// verified ID distinct from every container ID observed before `--force-recreate`.
fn containers_replaced(
    containers: &[ObservedContainer],
    services: &[String],
    prior_ids: &BTreeMap<String, Vec<String>>,
) -> bool {
    !services.is_empty()
        && services.iter().all(|service| {
            let Some(prior) = prior_ids.get(service) else {
                return false;
            };
            if prior.is_empty() {
                return false;
            }
            let matching: Vec<&ObservedContainer> = containers
                .iter()
                .filter(|container| container.view.name == *service)
                .collect();
            if matching.len() != 1 {
                return false;
            }
            matching[0]
                .id
                .as_ref()
                .is_some_and(|new_id| !prior.contains(new_id))
        })
}

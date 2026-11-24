//! Host Compose lifecycle. A session is installed only by the verified runtime.
use super::compose_secrets::SecretSource;
use super::{Id, guest::ComposeChannel, model::Clock};
use crate::service::Shutdown;
use crate::store::vm::compose as store;
use anyhow::{Result, ensure};
use arx_guest::compose::{
    ComposeRequest as Req, ComposeResponse as Resp, Desired, Observation, OperationKind,
    OperationStage, OperationState, OperationView, Project, ProjectManifest, ProjectView,
    RejectCode, STAGE_CHUNK, STAGE_TOTAL, encode_base64, is_project_id, sha256_hex,
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::Mutex as AsyncMutex,
    time::{Instant, sleep},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub vm: Id,
    pub project: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Deploy,
    Start,
    Stop,
    Down,
}

impl Kind {
    pub const fn action(self) -> i64 {
        match self {
            Self::Deploy => 20,
            Self::Start => 21,
            Self::Stop => 22,
            Self::Down => 23,
        }
    }

    pub fn wire(self) -> &'static str {
        match self {
            Self::Deploy => "deploy",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Down => "down",
        }
    }

    pub fn parse(raw: &str) -> rusqlite::Result<Self> {
        match raw {
            "deploy" => Ok(Self::Deploy),
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "down" => Ok(Self::Down),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }

    fn guest(self) -> OperationKind {
        match self {
            Self::Deploy => OperationKind::Deploy,
            Self::Start => OperationKind::Start,
            Self::Stop => OperationKind::Stop,
            Self::Down => OperationKind::Down,
        }
    }

    fn desired(self) -> Desired {
        match self {
            Self::Deploy | Self::Start => Desired::Started,
            Self::Stop => Desired::Stopped,
            Self::Down => Desired::Down,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    NotFound,
    NotReady,
    Busy,
    ReplayFull,
    RecoveryRequired(Id),
    StaleKey,
    KeyCollision,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReleaseInput {
    vm_id: String,
    release_id: String,
    manifest: ProjectManifest,
    #[serde(default)]
    secrets: Vec<SecretInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretInput {
    slot: String,
    epoch: u64,
    reference: String,
}

#[derive(serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SecretRef {
    pub slot: String,
    pub epoch: u64,
    pub reference: Id,
}

pub(crate) struct Enrollment {
    pub target: Target,
    pub id: Id,
    pub bytes: Vec<u8>,
    pub digest: String,
    pub project: Project,
    pub secrets: Vec<SecretRef>,
}

impl ReleaseInput {
    pub(crate) fn enroll(self) -> Result<Enrollment> {
        let project = Project::validate(self.manifest)
            .map_err(|_| anyhow::anyhow!("invalid Compose manifest"))?;
        let vm = opaque_id(&self.vm_id)?;
        let id = opaque_id(&self.release_id)?;
        let bytes = serde_json::to_vec(project.manifest())?;
        ensure!(
            bytes.len() <= STAGE_TOTAL,
            "Compose manifest exceeds staging limit"
        );
        let mut secrets = Vec::new();
        for secret in self.secrets {
            ensure!(
                project.slot_owner(&secret.slot).is_some()
                    && (1..=i64::MAX as u64).contains(&secret.epoch)
                    && !secrets.iter().any(|s: &SecretRef| s.slot == secret.slot),
                "invalid Compose secret reference"
            );
            secrets.push(SecretRef {
                slot: secret.slot,
                epoch: secret.epoch,
                reference: opaque_id(&secret.reference)?,
            });
        }
        Ok(Enrollment {
            target: Target {
                vm,
                project: project.id().into(),
            },
            id,
            digest: sha256_hex(&bytes),
            bytes,
            project,
            secrets,
        })
    }
}

fn opaque_id(raw: &str) -> Result<Id> {
    ensure!(
        raw.len() == 32 && raw.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "invalid Compose ID"
    );
    let mut id = [0; 16];
    hex::decode_to_slice(raw, &mut id)?;
    Ok(id)
}

pub(crate) fn nonce() -> Result<Id> {
    let mut id = [0; 16];
    getrandom::fill(&mut id).map_err(|_| anyhow::anyhow!("Compose entropy unavailable"))?;
    Ok(id)
}

pub(crate) struct Mutation {
    pub target: Target,
    pub kind: Kind,
    pub key: super::RequestId,
    pub release: Option<Id>,
}

pub(crate) struct Intent {
    pub id: Id,
    pub target: Target,
    pub generation: Id,
    pub boot: u64,
    pub kind: Kind,
    pub release: Id,
    pub revision: u64,
    pub sent: bool,
    pub closed: Option<String>,
    pub receipt: Option<OperationView>,
    pub secrets: Vec<SecretRef>,
    pub secret_sent: u8,
    pub digest: String,
    pub bytes: Vec<u8>,
}

impl Intent {
    pub(crate) fn replay_matches(&self, request: &Mutation) -> bool {
        self.target == request.target
            && self.kind == request.kind
            && request
                .release
                .is_none_or(|release| release == self.release)
    }

    pub(crate) fn pending_state(&self) -> &str {
        self.closed
            .as_deref()
            .unwrap_or(if self.sent || self.secret_sent != 0 {
                "unresolved"
            } else {
                "pending"
            })
    }

    fn accepts(&self, view: &OperationView) -> bool {
        view.operation_id == hex::encode(self.id)
            && view.project_id == self.target.project
            && view.kind == self.kind.guest()
            && view.revision == self.revision
    }

    fn request(&self, id: u32) -> Req {
        let operation_id = hex::encode(self.id);
        let project_id = self.target.project.clone();
        match self.kind {
            Kind::Deploy => Req::ComposeDeploy {
                id,
                operation_id,
                project_id,
                revision: self.revision,
                digest: self.digest.clone(),
            },
            Kind::Start => Req::ComposeStart {
                id,
                operation_id,
                project_id,
            },
            Kind::Stop => Req::ComposeStop {
                id,
                operation_id,
                project_id,
            },
            Kind::Down => Req::ComposeDown {
                id,
                operation_id,
                project_id,
            },
        }
    }
}

pub(crate) struct ProjectStatus {
    pub desired: Desired,
    pub revision: Option<u64>,
    pub observation: Option<ProjectView>,
    pub generation: Option<Id>,
    pub boot: Option<u64>,
    pub pending: Option<Id>,
    pub recovery_id: Option<Id>,
    pub last_receipt: Option<OperationView>,
}

pub(crate) struct LogRead {
    pub cursor: u64,
    pub limit: u32,
    pub services: Vec<String>,
}

pub(crate) struct Session {
    pub generation: Id,
    pub wire_boot: u64,
    channel: AsyncMutex<ComposeChannel>,
}

#[allow(
    dead_code,
    reason = "reserved for qualified activation and fake peer fixtures"
)]
impl Session {
    pub(crate) fn is_open(&self) -> bool {
        self.channel
            .try_lock()
            .is_ok_and(|channel| channel.is_open())
    }

    pub(crate) fn new(generation: Id, channel: ComposeChannel) -> Self {
        Self {
            generation,
            wire_boot: channel.generation(),
            channel: AsyncMutex::new(channel),
        }
    }
}

#[derive(Default)]
pub(crate) struct Host {
    pub secrets: Mutex<Option<Arc<dyn SecretSource>>>,
    sessions: Mutex<HashMap<Id, Arc<Session>>>,
}

impl Host {
    // The production activation adapter must supply a verified pipe and an
    // attributed boot. Native admission is closed in this slice.
    #[allow(
        dead_code,
        reason = "reserved for qualified activation and fake peer fixtures"
    )]
    pub(crate) fn attach(
        &self,
        store: &crate::store::vm::VmStore,
        vm: Id,
        session: Session,
    ) -> Result<()> {
        use std::collections::hash_map::Entry;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| anyhow::anyhow!("Compose sessions unavailable"))?;
        store.transact(|db| store::register_boot(db, vm, session.generation, session.wire_boot))?;
        match sessions.entry(vm) {
            Entry::Vacant(slot) => {
                slot.insert(Arc::new(session));
            }
            Entry::Occupied(mut slot)
                if slot.get().generation != session.generation
                    || slot.get().wire_boot != session.wire_boot =>
            {
                slot.insert(Arc::new(session));
            }
            Entry::Occupied(slot) => {
                let current = slot.get();
                let mut channel = current
                    .channel
                    .try_lock()
                    .map_err(|_| anyhow::anyhow!("Compose guest exchange in progress"))?;
                ensure!(!channel.is_open(), "Compose session already attached");
                *channel = session.channel.into_inner();
            }
        }
        Ok(())
    }

    pub(crate) fn session(&self, vm: Id) -> Option<Arc<Session>> {
        self.sessions.lock().ok()?.get(&vm).cloned()
    }

    pub(crate) async fn logs(
        &self,
        session: Arc<Session>,
        target: Target,
        page: LogRead,
        cancel: &Shutdown,
    ) -> Option<(Vec<arx_guest::compose::LogEntry>, u64)> {
        let LogRead {
            cursor,
            limit,
            services,
        } = page;
        let mut channel = session.channel.try_lock().ok()?;
        let Resp::ComposeLogPage {
            entries,
            next_cursor,
            ..
        } = channel
            .exchange(cancel, |id| Req::ComposeLogs {
                id,
                project_id: target.project.clone(),
                cursor,
                limit,
            })
            .await
            .ok()?
        else {
            return None;
        };
        let mut next = cursor;
        for entry in &entries {
            if entry.project_id != target.project
                || entry.seq < next
                || !entry
                    .service
                    .as_ref()
                    .is_none_or(|service| services.contains(service))
            {
                return None;
            }
            next = entry.seq.checked_add(1)?;
        }
        (next == next_cursor
            && next_cursor <= i64::MAX as u64
            && entries.len() <= limit as usize
            && serde_json::to_vec(&entries).ok()?.len() <= arx_guest::compose::MAX_LOG_PAGE)
            .then_some((entries, next_cursor))
    }

    pub(crate) async fn drive<C: Clock>(
        &self,
        controller: &super::VmController<C>,
        principal: &str,
        key: Id,
        target: &Target,
        session: Arc<Session>,
        cancel: &Shutdown,
    ) -> Result<()> {
        let mut channel = session.channel.lock().await;
        // Another worker may have completed this operation while we waited.
        let db = controller.compose_store();
        let Some(mut intent) = db.transact(|db| store::replay(db, principal, key))? else {
            return Ok(());
        };
        if intent.closed.as_deref() == Some("retired_boot")
            && intent.target == *target
            && intent.generation == session.generation
            && intent.boot != session.wire_boot
            && intent.kind == Kind::Deploy
            && intent.sent
        {
            return recover_deploy(db, principal, &intent, &session, &mut channel, cancel).await;
        }
        if intent.receipt.is_some()
            || intent.closed.is_some()
            || intent.target != *target
            || intent.generation != session.generation
            || intent.boot != session.wire_boot
            || !db.transact(|db| store::live_boot(db, target.vm, intent.generation, intent.boot))?
            || db.transact(|db| store::boot(db, principal, target))? != Some(intent.generation)
        {
            return Ok(());
        }
        if !intent.sent && matches!(intent.kind, Kind::Stop | Kind::Down) {
            let prior = db.transact(|tx| {
                let row: Option<Id> = tx.query_row(
                    "SELECT o.request FROM vm_compose_projects p LEFT JOIN vm_compose_operations o ON o.id=p.recovery_id
                     WHERE p.vm=?1 AND p.project=?2", rusqlite::params![target.vm,target.project], |r|r.get(0))?;
                row.map(|key|store::replay(tx,principal,key)).transpose().map(Option::flatten)
            })?;
            if let Some(prior) = prior
                && prior.generation == session.generation
                && prior.boot != session.wire_boot
            {
                recover_deploy(db, principal, &prior, &session, &mut channel, cancel).await?;
            }
            // Repair may have selected the newer release. Bind the unsent cleanup
            // atomically before querying or submitting its own operation ID.
            let rebound = db.transact(|tx| {
                ensure!(store::live_boot(tx,target.vm,intent.generation,intent.boot)?, "Compose cleanup boot changed");
                let state=store::status(tx,principal,target)?.ok_or_else(||anyhow::anyhow!("Compose project unavailable"))?;
                if state.revision.is_none() {
                    store::close(tx,&intent,"compose_recovery_required",controller.compose_now()?)?;
                    return Ok(None);
                }
                if state.revision != Some(intent.revision) {
                    ensure!(tx.execute("UPDATE vm_compose_operations SET (release,revision)=(SELECT committed_release,committed FROM vm_compose_projects WHERE vm=?1 AND project=?2)
                       WHERE id=?3 AND sent=0 AND closed IS NULL",rusqlite::params![target.vm,target.project,intent.id])?==1,"Compose cleanup changed");
                }
                store::replay(tx,principal,key)
            })?;
            let Some(current) = rebound else {
                return Ok(());
            };
            intent = current;
        }
        let manifest = Project::parse(&intent.bytes)
            .map_err(|_| anyhow::anyhow!("invalid enrolled Compose manifest"))?;
        ensure!(
            manifest.id() == target.project && sha256_hex(&intent.bytes) == intent.digest,
            "Compose release binding changed"
        );
        let op = hex::encode(intent.id);
        let query = channel
            .exchange(cancel, |id| Req::ComposeOperation {
                id,
                operation_id: op.clone(),
            })
            .await?;
        let mut view = match query {
            Resp::ComposeOperationState { operation, .. } if intent.sent => operation,
            Resp::ComposeRejected {
                code: RejectCode::UnknownOperation,
                ..
            } if !intent.sent => {
                let current = observe(&mut channel, target, cancel).await?;
                let expected = db
                    .transact(|db| store::status(db, principal, target))?
                    .ok_or_else(|| anyhow::anyhow!("Compose project unavailable"))?;
                let refusal = if expected.recovery_id.is_some() {
                    "compose_recovery_required"
                } else {
                    "refused"
                };
                if intent.secret_sent == 0
                    && current.as_ref().is_some_and(|v| {
                        v.running_operation.is_some()
                            || (v.recovery_required
                                && matches!(intent.kind, Kind::Deploy | Kind::Start))
                    })
                {
                    return db.transact(|db| {
                        store::close(db, &intent, refusal, controller.compose_now()?)
                    });
                }
                let guest_revision = current.as_ref().and_then(|view| view.committed_revision);
                if guest_revision != expected.revision
                    || (intent.kind != Kind::Deploy && expected.revision != Some(intent.revision))
                {
                    // Never discard a possibly submitted private effect.
                    ensure!(
                        intent.secret_sent == 0,
                        "Compose revision changed during secret recovery"
                    );
                    return db.transact(|db| {
                        store::close(db, &intent, refusal, controller.compose_now()?)
                    });
                }
                if intent.kind == Kind::Deploy {
                    stage(&mut channel, &intent, cancel).await?;
                }
                if !intent.secrets.is_empty() {
                    let source = self
                        .secrets
                        .lock()
                        .ok()
                        .and_then(|source| source.clone())
                        .ok_or_else(|| anyhow::anyhow!("Compose secret custody unavailable"))?;
                    for (index, slot) in intent.secrets.iter().enumerate() {
                        let bit = 1_u8 << index;
                        let delivery =
                            super::compose_secrets::Delivery::new(&intent, slot, expected.revision);
                        let result = delivery
                            .deliver(
                                source.as_ref(),
                                &mut channel,
                                cancel,
                                intent.secret_sent & bit != 0,
                                || {
                                    db.transact(|db| {
                                        ensure!(
                                            store::boot(db, principal, target)?
                                                == Some(intent.generation),
                                            "Compose boot changed"
                                        );
                                        store::mark_secret_sent(db, &intent, bit)
                                    })
                                },
                            )
                            .await;
                        if result
                            .as_ref()
                            .err()
                            .is_some_and(|e| e.is::<super::compose_secrets::FailedDelivery>())
                        {
                            return db.transact(|db| {
                                store::close(
                                    db,
                                    &intent,
                                    "secret_failed",
                                    controller.compose_now()?,
                                )
                            });
                        }
                        result?;
                    }
                }
                db.transact(|db| {
                    ensure!(
                        store::boot(db, principal, target)? == Some(intent.generation),
                        "Compose boot changed"
                    );
                    store::mark_sent(db, &intent)
                })?;
                match channel.exchange(cancel, |id| intent.request(id)).await? {
                    Resp::ComposeOperationState { operation, .. } => operation,
                    Resp::ComposeRejected {
                        code: RejectCode::ReconcileRequired,
                        ..
                    } if expected.recovery_id.is_some() => {
                        return db.transact(|db| {
                            store::close(
                                db,
                                &intent,
                                "compose_recovery_required",
                                controller.compose_now()?,
                            )
                        });
                    }
                    Resp::ComposeRejected {
                        code:
                            RejectCode::JournalFull
                            | RejectCode::Busy
                            | RejectCode::UnknownRelease
                            | RejectCode::UnknownProject
                            | RejectCode::RevisionNotNewer
                            | RejectCode::NotCommitted
                            | RejectCode::ProjectMismatch
                            | RejectCode::ReconcileRequired
                            | RejectCode::ProjectLimit
                            | RejectCode::VolumeLimit
                            | RejectCode::VolumeCollision
                            | RejectCode::SecretMissing,
                        ..
                    } => {
                        return db.transact(|db| {
                            store::close(db, &intent, "refused", controller.compose_now()?)
                        });
                    }
                    // An unproven rejection is ambiguous after crossing the submission boundary.
                    _ => return Ok(()),
                }
            }
            _ => return Ok(()),
        };
        let until = Instant::now() + Duration::from_secs(120);
        let mut reconciled = false;
        loop {
            ensure!(
                intent.accepts(&view),
                "Compose operation attribution mismatch"
            );
            match view.state {
                OperationState::Succeeded | OperationState::Failed => break,
                OperationState::Ambiguous if !reconciled => {
                    reconciled = true;
                    match channel
                        .exchange(cancel, |id| Req::ComposeReconcile {
                            id,
                            operation_id: op.clone(),
                        })
                        .await?
                    {
                        Resp::ComposeOperationState { operation, .. } => view = operation,
                        _ => return Ok(()),
                    }
                }
                OperationState::Running if Instant::now() < until => {
                    sleep(Duration::from_millis(250)).await;
                    match channel
                        .exchange(cancel, |id| Req::ComposeOperation {
                            id,
                            operation_id: op.clone(),
                        })
                        .await?
                    {
                        Resp::ComposeOperationState { operation, .. } => view = operation,
                        _ => return Ok(()),
                    }
                }
                _ => return Ok(()),
            }
        }
        ensure!(
            view.stage == OperationStage::Settled,
            "Compose receipt is not settled"
        );
        let observed = if view.state == OperationState::Succeeded {
            let Some(mut result) = observe(&mut channel, target, cancel).await? else {
                return Ok(());
            };
            ensure!(
                view.failure.is_none()
                    && !view.restored_previous
                    && success_witness(&intent, &manifest, &result),
                "Compose project attribution mismatch"
            );
            // Stop proves quiescence of retired services too. Project only
            // the currently enrolled service names, at most MAX_SERVICES.
            result.services.retain(|observed| {
                manifest
                    .manifest()
                    .services
                    .iter()
                    .any(|s| s.name == observed.name)
            });
            Some(result)
        } else {
            ensure!(
                view.failure.is_some(),
                "Compose terminal failure missing evidence"
            );
            None
        };
        db.transact(|db| {
            ensure!(
                store::boot(db, principal, target)? == Some(intent.generation),
                "Compose boot changed"
            );
            store::settle(
                db,
                &intent,
                &view,
                observed.as_ref(),
                controller.compose_now()?,
            )
        })
    }
}

// Recover only the committed release pointer; the retired request remains historical.
async fn recover_deploy(
    db: &crate::store::vm::VmStore,
    principal: &str,
    intent: &Intent,
    session: &Session,
    channel: &mut ComposeChannel,
    cancel: &Shutdown,
) -> Result<()> {
    let current = |tx: &rusqlite::Connection| -> Result<ProjectStatus> {
        let state = store::status(tx, principal, &intent.target)?
            .ok_or_else(|| anyhow::anyhow!("Compose project unavailable"))?;
        ensure!(
            state.recovery_id == Some(intent.id)
                && store::boot(tx, principal, &intent.target)? == Some(intent.generation)
                && store::live_boot(tx, intent.target.vm, intent.generation, session.wire_boot)?,
            "Compose recovery boot changed"
        );
        Ok(state)
    };
    db.transact(|tx| current(tx))?;
    let mut reply = channel
        .exchange(cancel, |id| Req::ComposeOperation {
            id,
            operation_id: hex::encode(intent.id),
        })
        .await?;
    if let Resp::ComposeOperationState { operation, .. } = &reply
        && intent.accepts(operation)
        && operation.state == OperationState::Ambiguous
    {
        reply = channel
            .exchange(cancel, |id| Req::ComposeReconcile {
                id,
                operation_id: hex::encode(intent.id),
            })
            .await?;
    }
    let operation = match reply {
        Resp::ComposeOperationState { operation, .. }
            if intent.accepts(&operation) && operation.stage == OperationStage::Settled =>
        {
            Some(operation)
        }
        Resp::ComposeRejected {
            code: RejectCode::UnknownOperation,
            ..
        } => None,
        Resp::ComposeOperationState { operation, .. }
            if intent.accepts(&operation) && operation.state == OperationState::Ambiguous =>
        {
            observe(channel, &intent.target, cancel).await?;
            return Ok(());
        }
        _ => return Ok(()),
    };
    let observed = observe(channel, &intent.target, cancel).await?;
    if observed
        .as_ref()
        .is_some_and(|p| p.running_operation.is_some())
    {
        return Ok(());
    }
    let revision = observed.as_ref().and_then(|p| p.committed_revision);
    db.transact(|tx| {
        let state = current(tx)?;
        let Some(operation) = operation else {
            // Unknown can mean never journaled or trimmed after promotion. Verify
            // the committed revision on the same disk before classifying it.
            if revision == state.revision {
                store::retire_unapplied(tx, intent)?;
            } else if let Some(observed) = observed
                .as_ref()
                .filter(|_| revision == Some(intent.revision))
            {
                store::recover_committed(tx, intent, observed, None)?;
            }
            return Ok(());
        };
        match operation.state {
            OperationState::Succeeded
                if operation.failure.is_none()
                    && !operation.restored_previous
                    && revision == Some(intent.revision) =>
            {
                store::recover_committed(
                    tx,
                    intent,
                    observed.as_ref().expect("committed revision observed"),
                    Some(&operation),
                )?;
            }
            OperationState::Failed if operation.failure.is_some() && revision == state.revision => {
                store::clear_recovery(tx, intent)?;
            }
            _ => {}
        }
        Ok(())
    })
}

fn success_witness(intent: &Intent, manifest: &Project, result: &ProjectView) -> bool {
    use arx_guest::compose::{HealthState, ServiceState};
    result.committed_revision == Some(intent.revision)
        && result.desired == intent.kind.desired()
        && result.observation == Observation::Live
        && !result.recovery_required
        && result.running_operation.is_none()
        && match result.desired {
            Desired::Started => {
                let specs = &manifest.manifest().services;
                result.services.len() == specs.len()
                    && specs.iter().all(|spec| {
                        result.services.iter().any(|service| {
                            service.name == spec.name
                                && service.revision == Some(intent.revision)
                                && service.state == ServiceState::Running
                                && matches!(
                                    service.health,
                                    HealthState::Healthy | HealthState::None
                                )
                        })
                    })
            }
            Desired::Stopped => result.services.iter().all(|s| {
                matches!(
                    s.state,
                    ServiceState::Exited
                        | ServiceState::Created
                        | ServiceState::Dead
                        | ServiceState::Removing
                )
            }),
            Desired::Down => result.services.is_empty(),
            Desired::None => false,
        }
}

fn service_name(name: &str) -> bool {
    name.len() <= 16 && is_project_id(name)
}

async fn observe(
    channel: &mut ComposeChannel,
    target: &Target,
    cancel: &Shutdown,
) -> Result<Option<ProjectView>> {
    match channel
        .exchange(cancel, |id| Req::ComposeStatus {
            id,
            project_id: target.project.clone(),
        })
        .await?
    {
        Resp::ComposeRejected {
            code: RejectCode::UnknownProject,
            ..
        } => Ok(None),
        Resp::ComposeProjectStatus { project, .. }
            if project.project_id == target.project && project.services.len() <= 128 && {
                let mut names = std::collections::BTreeSet::new();
                project
                    .services
                    .iter()
                    .all(|s| service_name(&s.name) && names.insert(&s.name))
            } =>
        {
            Ok(Some(project))
        }
        _ => anyhow::bail!("invalid Compose status observation"),
    }
}

async fn stage(channel: &mut ComposeChannel, intent: &Intent, cancel: &Shutdown) -> Result<()> {
    let total = intent.bytes.len() as u32;
    let chunks = intent.bytes.len().div_ceil(STAGE_CHUNK) as u32;
    let digest = &intent.digest;
    let Resp::ComposeStageProgress {
        digest: echo,
        received_bytes,
        next_index,
        ..
    } = channel
        .exchange(cancel, |id| Req::ComposeStageBegin {
            id,
            digest: digest.clone(),
            total_bytes: total,
            chunks,
        })
        .await?
    else {
        anyhow::bail!("Compose staging refused")
    };
    ensure!(echo == *digest, "Compose staging digest mismatch");
    if (received_bytes, next_index) != (total, chunks) {
        ensure!(
            (received_bytes, next_index) == (0, 0),
            "Compose staging boundary mismatch"
        );
        let mut received = 0;
        for (index, bytes) in intent.bytes.chunks(STAGE_CHUNK).enumerate() {
            let index = index as u32;
            received += bytes.len() as u32;
            ensure!(
                matches!(channel.exchange(cancel, |id| Req::ComposeStageChunk {
                id, digest:digest.clone(), index, data:encode_base64(bytes),
            }).await?, Resp::ComposeStageProgress { digest:echo, received_bytes, next_index, .. }
                if echo == *digest && received_bytes == received && next_index == index + 1),
                "Compose staging progress mismatch"
            );
        }
    }
    ensure!(
        matches!(channel.exchange(cancel, |id| Req::ComposeStageCommit { id, digest:digest.clone() }).await?,
        Resp::ComposeReleaseStaged { digest:echo, project_id, .. } if echo == *digest && project_id == intent.target.project),
        "Compose staged release mismatch"
    );
    Ok(())
}

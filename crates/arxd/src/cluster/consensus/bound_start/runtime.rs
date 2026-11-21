//! Private deployment caller for the existing retained cluster jobs.
use super::*;
use crate::{
    cluster::{
        c13_diagnostics::record,
        enrollment::{
            ClusterId, NodeId,
            durable_join::{self, Destination},
            transfer::LocalRoute,
        },
        restart_history::{RecoveredHistory, Slot},
        startup_plan::{Mode, Plan},
    },
    service::cluster_owner::{Operator, Paste, Reply},
};
use checked_clock::{CheckedClock, HostClock};
use openraft::type_config::async_runtime::watch::WatchReceiver;
use rustls::{pki_types::UnixTime, time_provider::TimeProvider};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Intent {
    CreateFounder,
    Join,
    RecoverFounder,
    RecoverJoiner,
}

impl Intent {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "create-founder" => Some(Self::CreateFounder),
            "join" => Some(Self::Join),
            "recover-founder" => Some(Self::RecoverFounder),
            "recover-joiner" => Some(Self::RecoverJoiner),
            _ => None,
        }
    }

    fn mode(self) -> Mode {
        match self {
            Self::CreateFounder => Mode::CreateFounder,
            Self::Join => Mode::Join,
            Self::RecoverFounder => Mode::RecoverFounder,
            Self::RecoverJoiner => Mode::RecoverJoiner,
        }
    }
}

pub(crate) fn peer_endpoint(address: &str) -> Option<SocketAddr> {
    let address = peer_transport::endpoint(address).ok()?;
    peer_control_wire::qualified(address).then_some(address)
}

pub(crate) struct Installation {
    pub(crate) config: PathBuf,
    pub(crate) workspace: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) credentials: PathBuf,
    pub(crate) history: PathBuf,
    pub(crate) wrap: PathBuf,
    pub(crate) custody: PathBuf,
    pub(crate) peer: SocketAddr,
    pub(crate) enrollment: SocketAddr,
    pub(crate) founder_cluster: Option<ClusterId>,
}

impl Installation {
    fn plan(&self, intent: Intent) -> Plan {
        Plan {
            mode: intent.mode(),
            config: self.config.clone(),
            workspace: self.workspace.clone(),
            state: self.state.clone(),
            credentials: self.credentials.clone(),
            peer: self.peer.to_string(),
            enrollment: self.enrollment.to_string(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Route {
    pub(crate) network: std::net::IpAddr,
    pub(crate) prefix: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Configuration,
    Ownership,
    Recovery,
    Founder,
    Admission,
    History,
    Transfer,
    Issuer,
    Listener,
    Close,
    LearnerLockRetained,
}

pub(super) enum RecoveryHold {
    Refused(Box<startup_recovery::ProcessRecovery>),
    Ready(Box<startup_recovery::HistoryReady>),
    Custody(Box<RecoveredHistory>),
}

struct FreshTime(HostClock);
impl std::fmt::Debug for FreshTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("checked enrollment time")
    }
}
impl TimeProvider for FreshTime {
    fn current_time(&self) -> Option<UnixTime> {
        self.0
            .sample_ms()
            .ok()
            .map(|ms| UnixTime::since_unix_epoch(Duration::from_millis(ms)))
    }
}

/// The two checked foreground sockets are used only by this installation.
/// Fresh Join owns bound sockets that have not entered the listen state.
pub(crate) struct InheritedListeners {
    pub(crate) peer: Option<std::net::TcpListener>,
    pub(crate) enrollment: Option<std::net::TcpListener>,
    pub(crate) listening: bool,
}

#[derive(Default)]
pub(super) struct JoinEvidence {
    pub(super) without_ca: bool,
    pub(super) requests: u8,
    pub(super) applied: Option<LogPos>,
}

/// One install per process. Register before launching a retained worker so a
/// stop observed during startup also fences its first admission.
#[derive(Default)]
pub(crate) struct Admission {
    stop: crate::service::Shutdown,
    job: std::sync::Mutex<Option<&'static Job>>,
    inherited: std::sync::Mutex<Option<InheritedListeners>>,
}

impl Admission {
    pub(crate) fn signal(&self) -> crate::service::Shutdown {
        self.stop.clone()
    }

    pub(crate) fn with_listeners(inherited: Option<InheritedListeners>) -> Self {
        Self {
            inherited: std::sync::Mutex::new(inherited),
            ..Self::default()
        }
    }

    pub(crate) fn check_addresses(&self, peer: SocketAddr, enrollment: SocketAddr) -> bool {
        use crate::service::cluster_runtime::validate_listener;
        self.inherited.lock().unwrap().as_ref().is_none_or(|pair| {
            pair.peer
                .as_ref()
                .is_some_and(|listener| validate_listener(listener, peer, pair.listening).is_ok())
                && pair.enrollment.as_ref().is_some_and(|listener| {
                    validate_listener(listener, enrollment, pair.listening).is_ok()
                })
        })
    }

    pub(crate) fn evidence(&self) -> [u8; 29] {
        let mut bytes = [0; 29];
        if let Some(job) = *self.job.lock().unwrap() {
            let state = job.state();
            bytes[1..3].copy_from_slice(&state.exchange.observe());
            bytes[3] = state.join_evidence.requests;
            bytes[4] = u8::from(state.join_evidence.without_ca);
            if let Some(pos) = state.join_evidence.applied {
                bytes[5..13].copy_from_slice(&pos.term.to_be_bytes());
                bytes[13..21].copy_from_slice(&pos.leader.to_be_bytes());
                bytes[21..29].copy_from_slice(&pos.index.to_be_bytes());
            }
        }
        bytes
    }

    pub(crate) fn time_evidence(&self) -> [u8; crate::service::cluster_owner::TIME_EVIDENCE_LEN] {
        let job = self.job.try_lock().ok().and_then(|slot| *slot);
        let Some(job) = job else {
            return enrollment_ingress::ExchangeCounts::absent_time_evidence();
        };
        job.time_evidence()
    }

    pub(crate) fn stop(&self) {
        let registered = self.job.lock().unwrap();
        self.stop.cancel();
        if let Some(job) = *registered {
            Job::stop_admission(&mut job.state());
        }
    }

    pub(super) fn register(&self, job: &'static Job) {
        let mut registered = self.job.lock().unwrap();
        assert!(registered.replace(job).is_none());
        let mut state = job.state();
        state.process_owned = true;
        state.inherited = self.inherited.lock().unwrap().take();
        if self.stop.is_cancelled() {
            Job::stop_admission(&mut state);
        }
    }
}

pub(crate) struct Running {
    job: &'static Job,
    supervisor: Option<tokio::task::JoinHandle<()>>,
    learner: Option<learner::Run>,
    fresh_founder: bool,
    strict_written: bool,
    transfer_done: bool,
    enrollment_started: bool,
}

impl Running {
    fn founder(
        job: &'static Job,
        supervisor: Option<tokio::task::JoinHandle<()>>,
        fresh_founder: bool,
    ) -> Self {
        Self {
            job,
            supervisor,
            learner: None,
            fresh_founder,
            strict_written: false,
            transfer_done: !fresh_founder,
            enrollment_started: false,
        }
    }

    fn learner(run: learner::Run, recovered: bool) -> Self {
        Self {
            job: run.job,
            supervisor: None,
            learner: Some(run),
            fresh_founder: false,
            strict_written: recovered,
            transfer_done: true,
            enrollment_started: false,
        }
    }

    pub(crate) async fn start(
        installation: Installation,
        intent: Intent,
        fence: &Admission,
    ) -> Result<Self, Error> {
        if intent == Intent::CreateFounder {
            let plan = installation.plan(intent);
            let cluster = installation.founder_cluster.ok_or(Error::Configuration)?;
            let intent = StartIntent {
                mode: plan.mode,
                cluster,
                node: NodeId::new(1).unwrap(),
                peer: installation.peer,
                enrollment: installation.enrollment,
            };
            let (job, _, supervisor) = founder::linux::dispatch_history(
                plan,
                intent,
                Params::default(),
                installation.wrap,
                installation.custody,
                Slot::configured(installation.history),
                Some(fence),
            )
            .map_err(|_| Error::Founder)?;
            return Ok(Self::founder(job, Some(supervisor), true));
        }
        if intent == Intent::Join {
            return Err(Error::Configuration);
        }
        let recovery = tokio::task::spawn_blocking(move || {
            let owner = Owner::acquire(installation.plan(intent)).map_err(|_| Error::Ownership)?;
            Ok(startup_recovery::recover_process(
                owner,
                Slot::configured(installation.history),
                &installation.wrap,
                &installation.custody,
                intent == Intent::RecoverFounder,
            ))
        })
        .await
        .map_err(|_| Error::Ownership)??;
        match recovery.into_ready() {
            Ok(ready) if intent == Intent::RecoverFounder => {
                let (job, _, supervisor) =
                    founder::linux::dispatch_recovered(ready, Params::default(), fence);
                Ok(Self::founder(job, supervisor, false))
            }
            Ok(ready) => Ok(Self::learner(
                learner::recover_process(ready, Params::default(), fence),
                true,
            )),
            Err(recovery) => {
                let job = Job::anchor();
                job.state().recovery_hold = Some(RecoveryHold::Refused(recovery));
                job.set(Status::Unconfirmed);
                Ok(Self::founder(job, None, false))
            }
        }
    }

    pub(crate) async fn join(
        installation: Installation,
        paste: Paste,
        routes: &[Route],
        fence: &Admission,
    ) -> Result<Self, Error> {
        let join = publish(installation, paste, routes).await?;
        let run = learner::start_history(join, Params::default(), Some(fence))
            .map_err(|_| Error::Admission)?;
        Ok(Self::learner(run, false))
    }

    pub(crate) async fn admit(&self, issuer: SocketAddr) -> Result<(), Error> {
        let learner = self.learner.as_ref().ok_or(Error::Admission)?;
        learner
            .set_authenticated_admission_route(issuer)
            .map_err(|reason| {
                record(
                    "admission-route",
                    match reason {
                        Refusal::Unavailable => "unavailable",
                        Refusal::Identity | Refusal::Provenance => "identity",
                        _ => "refused",
                    },
                );
                Error::Admission
            })?;
        if let Some(category) = learner.admit().await.refusal_category() {
            record("join-admission", category);
            return Err(Error::Admission);
        }
        Ok(())
    }

    pub(crate) async fn attached(&self) -> Result<(), Error> {
        let mut observer = self.job.observe();
        loop {
            match observer.status() {
                Status::FounderReady | Status::LearnerReady => return Ok(()),
                Status::Preparing
                | Status::Prepared
                | Status::EngineMayExist
                | Status::Running
                | Status::Founding => {}
                _ => return Err(self.start_error()),
            }
            if observer.changed().await.is_none() {
                return Err(Error::Close);
            }
        }
    }

    fn start_error(&self) -> Error {
        let state = self.job.state();
        if state.recovery_hold.is_some() {
            Error::Recovery
        } else if state.founder_failure.is_some() {
            Error::Founder
        } else {
            Error::Listener
        }
    }

    pub(crate) async fn close(self) -> Result<bool, Error> {
        if let Some(learner) = self.learner {
            return learner.close(false).await.map(|_| false).map_err(|_| {
                if self.job.observe().status() == Status::ClosedAfterLearnerRefusal {
                    Error::LearnerLockRetained
                } else {
                    Error::Close
                }
            });
        }
        self.job.request_close();
        let supervisor = self.supervisor.ok_or(Error::Close)?;
        supervisor.await.map_err(|_| Error::Close)?;
        match self.job.observe().status() {
            Status::Clean => Ok(false),
            Status::ClosedAfterFounderRefusal => Ok(true),
            _ => Err(Error::Close),
        }
    }

    pub(crate) async fn mint(&self, operator: Operator) -> Reply {
        let installation = self.installation_id();
        match installation {
            Some(installation) => self.job.mint_owner(operator, installation).await,
            None => Reply::Refused,
        }
    }

    fn installation_id(&self) -> Option<[u8; 16]> {
        let state = self.job.state();
        if let Some(learner) = &state.learner {
            return learner.installation_id();
        }
        state
            .founder_recovered
            .as_ref()
            .and_then(|history| Some(history.founder()?.installation()))
            .or_else(|| state.founder.as_ref()?.installation_id())
    }

    pub(crate) async fn observe(&mut self) -> Result<Readiness, Error> {
        let status = self.job.observe().status();
        if !matches!(status, Status::FounderReady | Status::LearnerReady) {
            return Err(self.start_error());
        }
        let progress = self
            .learner
            .as_ref()
            .and_then(learner::Run::admission_progress);
        let strict = self.learner.is_none()
            || progress.is_some_and(|(phase, applied)| {
                phase == peer_registry::Phase::Strict && applied.is_some()
            });
        if let Some(learner) = self.learner.as_ref()
            && strict
            && !self.strict_written
        {
            use enrollment_ingress::ActivationError::{Deferred, Refused};
            match learner.persist_strict_history() {
                Ok(()) => self.strict_written = true,
                Err(Deferred) => {
                    return Ok(Readiness {
                        strict: false,
                        issuer: false,
                        transferred: self.transfer_done,
                    });
                }
                Err(Refused) => return Err(Error::History),
            }
        }
        let (leader, candidates) = self.leader_and_transfer_targets();
        if leader && strict && !self.enrollment_started && !self.start_issuer().await? {
            return Ok(Readiness {
                strict,
                issuer: false,
                transferred: self.transfer_done,
            });
        }
        if self.fresh_founder
            && !self.transfer_done
            && let Some(targets) = candidates
        {
            for target in targets {
                let target = NodeId::new(target).ok_or(Error::Transfer)?;
                if self.job.transfer_issuer_to(target).await
                    != peer_transport::TransferOutcome::Installed
                {
                    return Err(Error::Transfer);
                }
            }
            self.transfer_done = true;
        }
        let issuer = self.enrollment_started && self.job.issuer_serving();
        Ok(Readiness {
            strict,
            issuer,
            transferred: self.transfer_done,
        })
    }

    async fn start_issuer(&mut self) -> Result<bool, Error> {
        use enrollment_ingress::ActivationError::{Deferred, Refused};
        if self.learner.is_some() {
            if !self.voter_custody() {
                return Ok(false);
            }
            match self.job.activate_promoted_issuer().await {
                Ok(()) => {}
                Err(Deferred) => return Ok(false),
                Err(Refused) => return Err(Error::Issuer),
            }
        }
        match self.job.start_enrollment(
            #[cfg(test)]
            None,
        ) {
            Ok(()) => {
                self.enrollment_started = true;
                Ok(true)
            }
            Err(Deferred) => {
                if self.learner.is_some() {
                    // Renewal must earn a fresh quorum proof for the next
                    // attempt rather than reuse authority from an old epoch.
                    self.job.state().issuer.take();
                }
                Ok(false)
            }
            Err(Refused) => Err(Error::Listener),
        }
    }

    fn leader_and_transfer_targets(&self) -> (bool, Option<[u64; 2]>) {
        let node = self.job.state().node.clone();
        let Some(node) = node.as_ref().and_then(|n| n.try_lock().ok()) else {
            return (false, None);
        };
        let Some(raft) = node.raft.as_ref() else {
            return (false, None);
        };
        let metrics = raft.metrics();
        let m = metrics.borrow_watched();
        let leader = m.running_state.is_ok()
            && m.state == openraft::ServerState::Leader
            && m.vote.committed
            && m.vote.leader_id.node_id == node.id
            && m.current_leader == Some(node.id)
            && m.last_applied
                .is_some_and(|a| a.leader_id == m.vote.leader_id);
        let candidates = (leader && m.committed_membership_config == m.membership_config)
            .then(|| {
                let membership = (*m.committed_membership_config.log_id())?;
                if !m
                    .last_applied
                    .is_some_and(|applied| covers_log(applied, membership))
                {
                    return None;
                }
                let targets = issuer_transfer::promoted_targets(
                    m.committed_membership_config.membership(),
                    NodeId::new(node.id)?,
                )?;
                targets
                    .iter()
                    .all(|target| {
                        m.replication
                            .as_ref()
                            .and_then(|replicas| replicas.get(target))
                            .and_then(|matched| *matched)
                            .is_some_and(|matched| covers_log(matched, membership))
                    })
                    .then_some(targets)
            })
            .flatten();
        (leader, candidates)
    }

    fn voter_custody(&self) -> bool {
        self.job
            .state()
            .learner
            .as_ref()
            .is_some_and(learner::Operation::voter_custody)
    }
}

fn covers_log(actual: Id, target: Id) -> bool {
    actual.index > target.index || actual == target
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Readiness {
    pub(crate) strict: bool,
    pub(crate) issuer: bool,
    pub(crate) transferred: bool,
}

async fn publish(
    installation: Installation,
    paste: Paste,
    routes: &[Route],
) -> Result<durable_join::HistoryJoin, Error> {
    let plan = installation.plan(Intent::Join);
    let routes = routes
        .iter()
        .map(|r| LocalRoute {
            network: r.network,
            prefix: r.prefix,
        })
        .collect::<Vec<_>>();
    let (join, mut observer) = durable_join::start_history_join(
        Destination {
            plan,
            wrap: installation.wrap,
            staging: installation.custody,
        },
        Slot::configured(installation.history),
        std::str::from_utf8(paste.bytes()).map_err(|_| Error::Configuration)?,
        &routes,
        Arc::new(FreshTime(HostClock::new(0))),
    )
    .map_err(|_| {
        record("join-publication", "refused");
        Error::Admission
    })?;
    drop(paste);
    use durable_join::Status;
    let category = match observer.terminal().await {
        Status::Published => return Ok(join),
        Status::Refused => "refused",
        Status::Incomplete => "incomplete",
        Status::Joining | Status::Unconfirmed => "unconfirmed",
    };
    record("join-publication", category);
    Err(Error::Admission)
}

#[cfg(all(test, target_os = "linux"))]
mod tests;

#[cfg(test)]
pub(crate) fn refused_founder_for_test(
    fence: &Admission,
    confirmed: bool,
) -> (Running, tokio::sync::oneshot::Receiver<()>) {
    let job = Job::anchor();
    fence.register(job);
    founder::refuse(
        job,
        founder::Error::Attach(peer_transport::Refusal::Address),
    );
    let (closed, observed) = tokio::sync::oneshot::channel();
    let supervisor = tokio::spawn(async move {
        job.close.notified().await;
        assert!(job.begin_close());
        job.finish_close(None, confirmed, false);
        closed.send(()).unwrap();
    });
    (Running::founder(job, Some(supervisor), false), observed)
}

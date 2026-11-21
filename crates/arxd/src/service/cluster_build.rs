//! D4 service ownership, grant policy, and admission.
use super::{
    Shutdown,
    cluster_build_executor::BuildBroker,
    executor::{BuildAdmissionError, Execution},
};
use crate::{
    cluster::{
        delegated_build::{self as transport, Authority, IO_BUDGET, Identity, wire::*},
        enrollment::NodeId,
    },
    store::{
        Activity, BuildActivity, Metadata, ObservedKind, Outcome, Reason, Rights, TreeBoundary,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, task::JoinSet};

const LEASE_TTL: Duration = Duration::from_secs(30);
const MAX_LEASES: usize = 64;
const MAX_SOURCE_LEASES: usize = 8;
const MAX_CONNECTIONS: usize = 16;

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    pub(super) socket: PathBuf,
    pub(super) activity: PathBuf,
    pub(super) peer: SocketAddr,
    pub(super) agent_group: u32,
    pub(super) agents: Vec<LocalAgent>,
    routes: Vec<Route>,
    grants: Vec<Grant>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalAgent {
    pub(super) uid: u32,
    pub(super) principal: String,
    pub(super) token_sha256: String,
    agent: String,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    node: NodeId,
    address: SocketAddr,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    source: NodeId,
    key: [u8; 32],
    agent: String,
    commands: Vec<Descriptor>,
}

fn digest(domain: &[u8], fields: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    for field in fields {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field);
    }
    digest.finalize().into()
}

fn encoded<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("D4 typed record serializes")
}

fn name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

impl Settings {
    pub(super) fn valid(&self, peer: SocketAddr, enrollment: SocketAddr) -> bool {
        let mut targets = HashSet::new();
        let mut principals = HashSet::new();
        let mut agent_ids = HashSet::new();
        let mut grants = HashSet::new();
        let mut tokens = HashSet::new();
        self.agent_group > 0
            && self.agent_group != u32::MAX
            && self.peer.ip() == peer.ip()
            && self.peer.port() > 1023
            && self.peer != peer
            && self.peer != enrollment
            && (1..=16).contains(&self.agents.len())
            && self.agents.iter().all(|a| {
                a.uid > 0
                    && a.uid != u32::MAX
                    && name(&a.principal)
                    && name(&a.agent)
                    && principals.insert(&a.principal)
                    && agent_ids.insert(&a.agent)
                    && tokens.insert(&a.token_sha256)
                    && a.token_sha256.len() == 64
                    && a.token_sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            && self.routes.len() <= 16
            && self.routes.iter().all(|r| {
                crate::cluster::consensus::activation::peer_endpoint(&r.address.to_string())
                    == Some(r.address)
                    && r.address.port() > 1023
                    && targets.insert(r.node)
            })
            && self.grants.len() <= 16
            && self.grants.iter().all(|g| {
                g.key != [0; 32]
                    && name(&g.agent)
                    && grants.insert((g.source, g.key, &g.agent))
                    && (1..=8).contains(&g.commands.len())
                    && g.commands.iter().all(|c| c.request().is_some())
            })
    }
}

struct Lease {
    expires: Instant,
    source: NodeId,
    scope: [u8; 32],
    descriptor: [u8; 32],
    secret: Secret,
    ticket: [u8; 32],
}

#[derive(Default)]
struct Leases(Mutex<HashMap<Operation, Lease>>);

impl Leases {
    fn insert(&self, operation: Operation, lease: Lease) -> bool {
        let Ok(mut entries) = self.0.lock() else {
            return false;
        };
        let now = Instant::now();
        entries.retain(|_, l| l.expires > now);
        if entries.len() >= MAX_LEASES
            || entries.contains_key(&operation)
            || entries
                .values()
                .filter(|l| l.source == lease.source)
                .count()
                >= MAX_SOURCE_LEASES
        {
            return false;
        }
        entries.insert(operation, lease);
        true
    }

    fn take(
        &self,
        operation: Operation,
        scope: [u8; 32],
        secret: Secret,
        hash: [u8; 32],
    ) -> Option<Lease> {
        let mut entries = self.0.lock().ok()?;
        let lease = entries.get(&operation)?;
        if lease.expires <= Instant::now()
            || lease.scope != scope
            || lease.descriptor != hash
            || !bool::from(lease.secret.ct_eq(&secret))
        {
            return None;
        }
        entries.remove(&operation)
    }
}

pub(crate) struct BuildService {
    activity: Arc<Activity>,
    execution: Option<Arc<Execution>>,
    broker: Option<BuildBroker>,
    grants: Vec<Grant>,
    routes: BTreeMap<NodeId, Route>,
    agents: BTreeMap<String, String>,
    authority: RwLock<Option<Arc<dyn Authority>>>,
    policy_current: Arc<dyn Fn() -> bool + Send + Sync>,
    leases: Leases,
    stop: Shutdown,
}

fn descriptor_hash(descriptor: &Descriptor) -> [u8; 32] {
    digest(b"arxcovo/d4/descriptor/1", &[&encoded(descriptor)])
}

fn forward_scope(principal: &str, target: NodeId) -> [u8; 32] {
    digest(
        b"arxcovo/d4/forwarded/1",
        &[principal.as_bytes(), &target.get().to_be_bytes()],
    )
}

impl BuildService {
    pub(super) fn new(
        settings: Settings,
        activity: Arc<Activity>,
        broker: Option<BuildBroker>,
        policy_current: impl Fn() -> bool + Send + Sync + 'static,
        stop: Shutdown,
    ) -> Arc<Self> {
        Arc::new(Self {
            execution: activity.dedicated.clone(),
            activity,
            broker,
            grants: settings.grants,
            routes: settings.routes.into_iter().map(|r| (r.node, r)).collect(),
            agents: settings
                .agents
                .into_iter()
                .map(|a| (a.principal, a.agent))
                .collect(),
            authority: RwLock::new(None),
            policy_current: Arc::new(policy_current),
            leases: Leases::default(),
            stop,
        })
    }

    pub(super) fn set_authority(&self, authority: Arc<dyn Authority>) {
        if let Ok(mut stored) = self.authority.write()
            && stored.is_none()
        {
            *stored = Some(authority);
        }
    }

    fn authority(&self) -> Option<Arc<dyn Authority>> {
        if self.stop.is_cancelled() {
            return None;
        }
        self.authority.read().ok()?.clone()
    }

    pub(super) async fn serve(self: Arc<Self>, listener: TcpListener) -> std::io::Result<()> {
        let mut tasks = JoinSet::new();
        let mut result = Ok(());
        loop {
            tokio::select! {
                biased;
                _ = self.stop.cancelled() => break,
                joined = tasks.join_next(), if !tasks.is_empty() => {
                    if joined.is_some_and(|r| r.is_err()) {
                        result = Err(std::io::ErrorKind::Other.into());
                        break;
                    }
                }
                connection = listener.accept(), if tasks.len() < MAX_CONNECTIONS => match connection {
                    Ok((stream, _)) => {
                        let Some(authority) = self.authority() else { continue };
                        let this = self.clone();
                        tasks.spawn(async move {
                            if let Some((mut stream, peer, request)) = transport::accept(stream, &*authority).await {
                                let reply = Receipt {
                                    version: VERSION,
                                    cluster: request.cluster,
                                    source: request.target,
                                    target: request.source,
                                    result: this.handle(authority, peer, request).await,
                                };
                                let _ = tokio::time::timeout(IO_BUDGET, send(&mut stream, &reply)).await;
                            }
                        });
                    }
                    Err(e) => { result = Err(e); break; }
                },
            }
        }
        drop(listener);
        while let Some(joined) = tasks.join_next().await {
            if joined.is_err() {
                result = Err(std::io::ErrorKind::Other.into());
            }
        }
        result
    }

    fn entitled(&self, authority: &dyn Authority, peer: Identity, target: NodeId) -> bool {
        !self.stop.is_cancelled()
            && (self.policy_current)()
            && authority.identity() == Some((peer.cluster, target))
            && authority.binding(peer.cluster, target).is_some()
            && authority.binding(peer.cluster, peer.node) == Some(peer.key)
    }

    async fn handle(
        &self,
        authority: Arc<dyn Authority>,
        peer: Identity,
        request: Envelope,
    ) -> Response {
        let Some(grant) = self
            .grants
            .iter()
            .find(|g| g.source == peer.node && g.key == peer.key && g.agent == request.agent)
        else {
            return Response::Refused;
        };
        if !self.entitled(&*authority, peer, request.target) {
            return Response::Refused;
        }
        let Some(target_key) = authority.binding(peer.cluster, request.target) else {
            return Response::Refused;
        };
        let scope = digest(
            b"arxcovo/d4/target/1",
            &[
                &peer.cluster,
                &request.target.get().to_be_bytes(),
                &target_key,
                &encoded(grant),
            ],
        );
        let allowed = |descriptor: &Descriptor| {
            let hash = descriptor_hash(descriptor);
            grant
                .commands
                .iter()
                .any(|allowed| descriptor_hash(allowed) == hash)
        };
        match request.query {
            Query::Grant(descriptor) if allowed(&descriptor) => {
                self.grant(peer, request.target, authority, scope, descriptor)
                    .await
            }
            Query::Spend {
                operation,
                lease,
                descriptor,
            } if allowed(&descriptor) => {
                let hash = descriptor_hash(&descriptor);
                let Some(reservation) = self.leases.take(operation, scope, lease, hash) else {
                    return Response::Refused;
                };
                self.spend(
                    (authority, peer, request.target),
                    operation,
                    reservation,
                    descriptor,
                )
                .await
            }
            Query::Status { operation } => self.status(operation, scope, None).await,
            _ => Response::Refused,
        }
    }

    async fn grant(
        &self,
        peer: Identity,
        target: NodeId,
        authority: Arc<dyn Authority>,
        scope: [u8; 32],
        descriptor: Descriptor,
    ) -> Response {
        let Some(broker) = self.broker.clone() else {
            return Response::Refused;
        };
        let workspace = descriptor.workspace.clone();
        let Ok(Ok(ticket)) = tokio::task::spawn_blocking(move || broker.check(workspace)).await
        else {
            return Response::Refused;
        };
        let (mut operation, mut secret) = ([0; 32], [0; 32]);
        if !self.entitled(&*authority, peer, target)
            || getrandom::fill(&mut operation).is_err()
            || getrandom::fill(&mut secret).is_err()
        {
            return Response::Refused;
        }
        let lease = Lease {
            expires: Instant::now() + LEASE_TTL,
            source: peer.node,
            scope,
            descriptor: descriptor_hash(&descriptor),
            secret,
            ticket,
        };
        if self.leases.insert(operation, lease) {
            Response::Granted {
                operation,
                lease: secret,
            }
        } else {
            Response::Refused
        }
    }

    async fn spend(
        &self,
        auth: (Arc<dyn Authority>, Identity, NodeId),
        operation: Operation,
        lease: Lease,
        descriptor: Descriptor,
    ) -> Response {
        let (Some(broker), Some(execution), Some(command)) = (
            self.broker.clone(),
            self.execution.clone(),
            descriptor.request(),
        ) else {
            return Response::Refused;
        };
        let activity = self.activity.clone();
        let stop = self.stop.clone();
        let scope = lease.scope;
        let policy_current = self.policy_current.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let claim = execution
                .reserve_build()
                .map_err(BuildAdmissionError::Unavailable)?;
            let prepared = broker
                .prepare(descriptor.workspace, lease.ticket, command.clone())
                .map_err(BuildAdmissionError::Unavailable)?;
            let (authority, peer, target) = auth;
            if lease.expires <= Instant::now()
                || stop.is_cancelled()
                || !policy_current()
                || authority.identity() != Some((peer.cluster, target))
                || authority.binding(peer.cluster, target).is_none()
                || authority.binding(peer.cluster, peer.node) != Some(peer.key)
            {
                return Err(BuildAdmissionError::Unavailable(
                    super::preflight::ExecutionUnavailable::Stopping,
                ));
            }
            Execution::dispatch_build(
                claim,
                activity,
                command,
                move |activity| activity.start_delegated_build(operation, scope, None),
                move |cancel, stop| prepared.execute(cancel, stop),
            )
        })
        .await;
        // This waiter belongs to the listener's joined task set. The socket is
        // only an observer. Dropping the source cannot cancel prepared work.
        match prepared {
            Ok(Ok((_id, work))) => {
                let output = work
                    .await
                    .ok()
                    .map(|v| json!({"stdout": v["stdout"], "stderr": v["stderr"]}));
                self.status(operation, scope, output).await
            }
            Ok(Err(BuildAdmissionError::Unavailable(reason))) => {
                let _ = reason.as_str();
                Response::Refused
            }
            _ => Response::Unknown { operation },
        }
    }

    async fn status(
        &self,
        operation: Operation,
        scope: [u8; 32],
        output: Option<Value>,
    ) -> Response {
        let activity = self.activity.clone();
        match tokio::task::spawn_blocking(move || activity.delegated_build(operation, scope, false))
            .await
        {
            Ok(Ok(Some(activity))) if terminal(activity) => Response::Finished {
                operation,
                activity,
                output,
            },
            Ok(Ok(None)) => Response::Refused,
            _ => Response::Unknown { operation },
        }
    }

    pub(crate) fn targets(&self, principal: &str) -> Value {
        if !self.agents.contains_key(principal) {
            return json!([]);
        }
        let Some(authority) = self.authority() else {
            return json!([]);
        };
        let Some((cluster, source)) = authority.identity() else {
            return json!([]);
        };
        json!(
            self.routes
                .values()
                .filter(|r| { r.node != source && authority.binding(cluster, r.node).is_some() })
                .map(|r| r.node)
                .collect::<Vec<_>>()
        )
    }

    async fn remote(
        &self,
        principal: &str,
        target: NodeId,
        query: Query,
        budget: Duration,
    ) -> Option<Response> {
        let authority = self.authority()?;
        let (cluster, source) = authority.identity()?;
        let route = self.routes.get(&target)?;
        let agent = self.agents.get(principal)?.clone();
        transport::call(
            &*authority,
            route.address,
            authority.binding(cluster, target)?,
            Envelope {
                version: VERSION,
                cluster,
                source,
                target,
                agent,
                query,
            },
            budget,
        )
        .await
    }

    pub(crate) async fn forward(
        &self,
        principal: &str,
        target: NodeId,
        descriptor: Descriptor,
        closed: &Shutdown,
    ) -> Value {
        if descriptor.request().is_none() || closed.is_cancelled() {
            return json!({"outcome": "refused"});
        }
        let Some(Response::Granted { operation, lease }) = self
            .remote(
                principal,
                target,
                Query::Grant(descriptor.clone()),
                IO_BUDGET,
            )
            .await
        else {
            return json!({"outcome": "refused"});
        };
        let scope = forward_scope(principal, target);
        let activity = self.activity.clone();
        let agent = principal.to_owned();
        let Ok(Ok(id)) = tokio::task::spawn_blocking(move || {
            activity.start_delegated_build(operation, scope, Some(&agent))
        })
        .await
        else {
            return json!({"outcome": "refused"});
        };
        let budget = Duration::from_secs(descriptor.timeout_s + 12);
        let reply = tokio::select! {
            biased;
            _ = closed.cancelled() => None,
            reply = self.remote(principal, target, Query::Spend { operation, lease, descriptor }, budget) => reply,
        };
        self.forward_result(operation, id, reply).await
    }

    pub(crate) async fn forwarded_status(
        &self,
        principal: &str,
        target: NodeId,
        operation: Option<Operation>,
    ) -> Value {
        if !self.agents.contains_key(principal) || !self.routes.contains_key(&target) {
            return json!({"outcome": "refused"});
        }
        let scope = forward_scope(principal, target);
        let activity = self.activity.clone();
        let Some(operation) = operation else {
            let result =
                tokio::task::spawn_blocking(move || activity.forwarded_builds(scope)).await;
            return match result {
                Ok(Ok(operations)) => {
                    json!({"operations": operations.iter().map(hex::encode).collect::<Vec<_>>()})
                }
                _ => json!({"outcome": "unknown"}),
            };
        };
        let Ok(Ok(Some(local))) =
            tokio::task::spawn_blocking(move || activity.delegated_build(operation, scope, true))
                .await
        else {
            return json!({"outcome": "refused"});
        };
        let reply = self
            .remote(principal, target, Query::Status { operation }, IO_BUDGET)
            .await;
        if !matches!(local.outcome, Outcome::Started) {
            return public_result(operation, reply.filter(|r| !matches!(r, Response::Refused)));
        }
        // An aged-out or unavailable remote status cannot prove no execution.
        self.forward_result(
            operation,
            local.activity_id,
            reply.filter(|r| !matches!(r, Response::Refused)),
        )
        .await
    }

    async fn forward_result(
        &self,
        operation: Operation,
        id: i64,
        reply: Option<Response>,
    ) -> Value {
        let outcome = match &reply {
            Some(Response::Finished {
                operation: echoed,
                activity,
                ..
            }) if *echoed == operation && terminal(*activity) => Some(*activity),
            Some(Response::Refused) => Some(BuildActivity {
                activity_id: 0,
                outcome: Outcome::Refused,
                exit_code: None,
                cleanup_confirmed: None,
                uid: None,
                gid: None,
            }),
            _ => None,
        };
        if let Some(result) = outcome {
            let activity = self.activity.clone();
            let terminal = tokio::task::spawn_blocking(move || {
                let metadata = Metadata {
                    reason: (result.outcome == Outcome::Refused).then_some(Reason::Unavailable),
                    exit_code: result.exit_code,
                    cleanup_confirmed: result.cleanup_confirmed,
                    observed_kind: result.uid.map(|_| ObservedKind::UnixUid),
                    observed_id: result.uid.map(|uid| uid.to_string()),
                    observed_gid: result.gid,
                    rights: result.uid.map(|_| Rights::Standard),
                    tree_boundary: result.uid.map(|_| TreeBoundary::LinuxCgroup),
                    ..Default::default()
                };
                activity.finish_typed(id, result.outcome, &metadata)
            })
            .await;
            if !matches!(terminal, Ok(Ok(()))) {
                return public_result(operation, None);
            }
        }
        public_result(operation, reply)
    }
}

fn terminal(activity: BuildActivity) -> bool {
    (matches!(activity.outcome, Outcome::Succeeded | Outcome::Failed)
        && activity.cleanup_confirmed == Some(true)
        && activity.uid.is_some()
        && activity.gid.is_some())
        || activity.outcome == Outcome::Refused
}

fn public_result(operation: Operation, response: Option<Response>) -> Value {
    let operation_id = hex::encode(operation);
    match response {
        Some(Response::Finished {
            operation: echoed,
            activity,
            output,
        }) if echoed == operation && terminal(activity) => {
            json!({"operation_id": operation_id, "outcome": activity.outcome,
                "target_activity": activity, "output": output})
        }
        Some(Response::Refused) => json!({"operation_id": operation_id, "outcome": "refused"}),
        _ => json!({"operation_id": operation_id, "outcome": "unknown"}),
    }
}

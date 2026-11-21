//! Leader-local policy for the first three voters and its exact joint recovery.
use super::{
    learner_admission::{self, Controller, Reply, covers},
    membership_observation::{MembershipReadError, PromotionTrigger, PromotionView},
    *,
};
use crate::cluster::enrollment::ClusterId;
use openraft::{raft::Precondition, type_config::async_runtime::watch::WatchReceiver};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
};

const FOUNDER: u64 = 1;
type RetainedNode = tokio::sync::Mutex<ConsensusNode>;

#[derive(Debug, PartialEq, Eq)]
enum Transition {
    Joint(Members),
    Flatten(Members),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Disposition {
    Deferred,
    Unknown,
    Promoted,
}

fn decide(view: &PromotionView) -> Option<Transition> {
    if view
        .members
        .nodes()
        .any(|(_, node)| peer_transport::endpoint(&node.addr).is_err())
    {
        return None;
    }
    let configs = view.members.get_joint_config();
    match configs.as_slice() {
        [old] if old.iter().copied().eq([FOUNDER]) && view.bindings.contains_key(&FOUNDER) => {
            let learners: BTreeSet<_> = view
                .members
                .non_write_voter_ids()
                .filter(|node| view.bindings.contains_key(node))
                .collect();
            if learners.len() != 2
                || learners.iter().any(|node| {
                    !view
                        .matched
                        .get(node)
                        .copied()
                        .flatten()
                        .is_some_and(|matched| covers(matched, view.committed))
                })
            {
                return None;
            }
            let mut target = learners;
            target.insert(FOUNDER);
            with_configs(view, vec![old.clone(), target]).map(Transition::Joint)
        }
        [old, target]
            if old.iter().copied().eq([FOUNDER])
                && target.len() == 3
                && target.contains(&FOUNDER) =>
        {
            with_configs(view, vec![target.clone()]).map(Transition::Flatten)
        }
        _ => None,
    }
}

fn with_configs(view: &PromotionView, configs: Vec<BTreeSet<u64>>) -> Option<Members> {
    Members::new(
        configs,
        view.members
            .nodes()
            .map(|(id, node)| (*id, node.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
    .ok()
}

fn guards(view: &PromotionView) -> [Precondition<Arx>; 3] {
    [
        Precondition::LastMembershipLogId {
            last_membership_log_id: Some(view.watermark),
        },
        Precondition::CommittedLeaderId {
            committed_leader_id: view.leader,
        },
        Precondition::LastLogId {
            last_log_id: Some(view.tail),
        },
    ]
}

/// This watcher borrows the retained driver's lifetime. Every write-capable
/// intent runs under the existing proposal unit and stays owned across a wake
/// or caller drop. The release notification coalesces with metric progress.
pub(super) async fn observe(
    cluster: ClusterId,
    node: Arc<RetainedNode>,
    controller: Arc<Controller>,
) {
    let Some(raft) = node.lock().await.raft.clone() else {
        return;
    };
    let node = Arc::downgrade(&node);
    let possible_joint = Arc::new(AtomicBool::new(false));
    let mut metrics = raft.metrics();
    let mut previous = None;
    let mut pending = false;
    let mut released = false;
    loop {
        let current = PromotionTrigger::from_metrics(&metrics.borrow_and_update());
        let reconcile = released && possible_joint.swap(false, Ordering::AcqRel);
        let due = reconcile || (released && pending) || current != previous;
        #[cfg(test)]
        if controller.hooks.promotion.failure.lock().unwrap().is_some() {
            return;
        }
        if due {
            pending = false;
            if current.is_some() && controller.open() {
                let work = Work {
                    cluster,
                    node: node.clone(),
                    controller: controller.clone(),
                    raft: raft.clone(),
                    possible_joint: possible_joint.clone(),
                    submitted: AtomicBool::new(false),
                };
                let result = controller.register_promotion(async move { work.run().await });
                pending = matches!(result, Err(Reply::Pending));
                #[cfg(test)]
                if reconcile && result.is_ok() {
                    controller
                        .hooks
                        .promotion
                        .reconciliation_wakes
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            previous = current;
        }
        released = tokio::select! {
            biased;
            () = controller.intent_released() => true,
            result = metrics.changed() => {
                if result.is_err() {
                    return;
                }
                false
            }
        };
    }
}

struct Work {
    cluster: ClusterId,
    node: Weak<RetainedNode>,
    controller: Arc<Controller>,
    raft: Engine,
    possible_joint: Arc<AtomicBool>,
    submitted: AtomicBool,
}

impl Work {
    async fn observe(&self) -> Result<PromotionView, MembershipReadError> {
        let node = self
            .node
            .upgrade()
            .ok_or(MembershipReadError::Unavailable)?;
        let node = node
            .try_lock()
            .map_err(|_| MembershipReadError::RetryLater)?;
        node.promotion_observation(self.cluster).await
    }

    async fn run(self) {
        let deadline = Instant::now() + learner_admission::BUDGET;
        let disposition = match timeout_at(deadline, self.promote()).await {
            Ok(Ok(disposition)) => disposition,
            Ok(Err(reason)) => {
                self.record_failure(reason);
                self.deferred()
            }
            Err(_) => {
                self.record_failure("membership intent elapsed");
                self.deferred()
            }
        };
        #[cfg(test)]
        if disposition == Disposition::Unknown {
            self.controller
                .hooks
                .promotion
                .unknown
                .fetch_add(1, Ordering::Relaxed);
        }
        let _ = disposition;
    }

    fn deferred(&self) -> Disposition {
        if self.submitted.load(Ordering::Acquire) {
            Disposition::Unknown
        } else {
            Disposition::Deferred
        }
    }

    async fn promote(&self) -> Result<Disposition, &'static str> {
        let Some(mut view) = self.read().await? else {
            return Ok(self.deferred());
        };
        let Some(transition) = decide(&view) else {
            return Ok(self.deferred());
        };
        let flat = match transition {
            Transition::Joint(joint) => {
                if !self.controller.open() {
                    return Ok(self.deferred());
                }
                self.possible_joint.store(true, Ordering::Release);
                let joint_id = self.append(&view, &joint, true).await?;
                if !self.wait_applied(view.leader, joint_id, &joint).await {
                    return Ok(self.deferred());
                }
                #[cfg(test)]
                if self
                    .controller
                    .hooks
                    .promotion
                    .interrupt
                    .swap(false, Ordering::AcqRel)
                {
                    // The SDK and metrics have both observed J. End this intent as
                    // Unknown and leave the ordinary release wake to reconcile it.
                    *self.controller.hooks.promotion.interrupted.lock().unwrap() =
                        Some((joint_id, joint));
                    return Ok(self.deferred());
                }
                let Some(fresh) = self.read().await? else {
                    return Ok(self.deferred());
                };
                if fresh.watermark != joint_id
                    || fresh.members != joint
                    || fresh.leader != view.leader
                {
                    return Ok(self.deferred());
                }
                view = fresh;
                let Some(Transition::Flatten(flat)) = decide(&view) else {
                    return Ok(self.deferred());
                };
                flat
            }
            Transition::Flatten(flat) => flat,
        };
        if !self.controller.open() {
            return Ok(self.deferred());
        }
        let flat_id = self.append(&view, &flat, false).await?;
        if !self.wait_applied(view.leader, flat_id, &flat).await {
            return Ok(self.deferred());
        }
        let Some(applied) = self.read().await? else {
            return Ok(self.deferred());
        };
        if applied.watermark != flat_id || applied.members != flat || applied.leader != view.leader
        {
            return Ok(self.deferred());
        }
        let (eligible, revoked): (Vec<_>, Vec<_>) = applied
            .members
            .voter_ids()
            .filter(|node| *node != FOUNDER)
            .partition(|node| applied.bindings.contains_key(node));
        if !revoked.is_empty() {
            eprintln!("arxd: PromotedButRevoked voters {revoked:?}");
        }
        eprintln!("arxd: initial promotion confirmed, bound promoted voters {eligible:?}");
        self.possible_joint.store(false, Ordering::Release);
        #[cfg(test)]
        {
            *self.controller.hooks.promotion.confirmed.lock().unwrap() = Some((flat_id, flat));
            self.controller.hooks.promotion.finished.notify_one();
        }
        Ok(Disposition::Promoted)
    }

    async fn read(&self) -> Result<Option<PromotionView>, &'static str> {
        match self.observe().await {
            Ok(view) => Ok(Some(view)),
            Err(MembershipReadError::RetryLater) => Ok(None),
            Err(MembershipReadError::NotLeader) => Err("fresh membership read lost leadership"),
            Err(MembershipReadError::Busy) => Err("fresh membership read busy"),
            Err(MembershipReadError::Unavailable) => Err("fresh membership read unavailable"),
            Err(MembershipReadError::PeerRefused) => Err("exact membership binding refused"),
        }
    }

    async fn append(
        &self,
        view: &PromotionView,
        membership: &Members,
        joint: bool,
    ) -> Result<Id, &'static str> {
        #[cfg(test)]
        {
            let hooks = &self.controller.hooks.promotion;
            let calls = if joint {
                &hooks.joint_calls
            } else {
                &hooks.flatten_calls
            };
            calls.fetch_add(1, Ordering::Relaxed);
        }
        let previously_submitted = self.submitted.swap(true, Ordering::AcqRel);
        self.raft
            .append_membership(membership.clone(), EntryPayload::Blank, guards(view))
            .await
            .map(|response| response.log_id)
            .map_err(|error| {
                #[cfg(test)]
                if self
                    .controller
                    .hooks
                    .promotion
                    .active
                    .load(Ordering::Acquire)
                {
                    eprintln!("C11 frozen guarded append (joint={joint}) refused: {error}");
                }
                if matches!(
                    error,
                    RaftError::APIError(ClientWriteError::PreconditionFailed(_))
                ) {
                    // A definite guard refusal leaves no new entry.
                    self.submitted
                        .store(previously_submitted, Ordering::Release);
                    if joint {
                        self.possible_joint.store(false, Ordering::Release);
                    }
                }
                if joint {
                    "guarded joint append refused"
                } else {
                    "guarded flatten refused"
                }
            })
    }

    async fn wait_applied(&self, leader: LeaderId, position: Id, expected: &Members) -> bool {
        let mut metrics = self.raft.metrics();
        loop {
            {
                let m = metrics.borrow_and_update();
                if m.running_state.is_err()
                    || m.current_leader != Some(leader.node_id)
                    || !m.vote.committed
                    || m.vote.leader_id != leader
                {
                    return false;
                }
                if *m.membership_config.log_id() == Some(position)
                    && *m.committed_membership_config.log_id() == Some(position)
                    && m.membership_config.membership() == expected
                    && m.committed_membership_config.membership() == expected
                    && m.last_applied
                        .is_some_and(|applied| covers(pos(applied), pos(position)))
                {
                    return true;
                }
            }
            if metrics.changed().await.is_err() {
                return false;
            }
        }
    }

    fn record_failure(&self, reason: &'static str) {
        let _ = reason;
        #[cfg(test)]
        if self
            .controller
            .hooks
            .promotion
            .active
            .load(Ordering::Acquire)
        {
            self.controller
                .hooks
                .promotion
                .failure
                .lock()
                .unwrap()
                .get_or_insert(reason);
            self.controller.hooks.promotion.finished.notify_one();
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct Hooks {
    pub(super) active: AtomicBool,
    pub(super) interrupt: AtomicBool,
    pub(super) interrupted: Mutex<Option<(Id, Members)>>,
    pub(super) confirmed: Mutex<Option<(Id, Members)>>,
    pub(super) failure: Mutex<Option<&'static str>>,
    pub(super) joint_calls: std::sync::atomic::AtomicUsize,
    pub(super) flatten_calls: std::sync::atomic::AtomicUsize,
    pub(super) reconciliation_wakes: std::sync::atomic::AtomicUsize,
    pub(super) unknown: std::sync::atomic::AtomicUsize,
    pub(super) finished: tokio::sync::Notify,
}

#[cfg(test)]
mod tests;

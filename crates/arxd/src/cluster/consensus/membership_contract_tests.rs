//! Engine-only cancellation seam. Equivalent controls are not compiled mutants.
use super::mem_net::{Factory, MemRouter};
use super::three_node::APPLY_PAUSE_OWNER;
use super::*;
use crate::service::application::tests::private_temp;
use openraft::ChangeMembers;
use openraft::errors::{ChangeMembershipError, PreconditionFailed};
use openraft::raft::{ClientWriteResponse, Precondition};
use openraft::type_config::async_runtime::watch::WatchReceiver;
use std::future::{Future, poll_fn};
use std::task::Poll;

type Failure = String;
type Write = Result<ClientWriteResponse<Arx>, RaftError<Arx, ClientWriteError<Arx>>>;

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Positive,
    OmitMembershipGuard,
    OmitTailGuard,
    OmitLeaderGuard,
    FirstConfig,
    DropLearner4,
    ResumeFuture,
}

#[derive(Debug, PartialEq, Eq)]
enum Violation {
    InProgress,
    MembershipRefusalExpected,
    TailRefusalExpected,
    LeaderRefusalExpected,
    NoUniform,
    FinalVoters,
    Metadata,
    State,
    Logs,
}

#[derive(Debug)]
struct Observation {
    tail: Option<Id>,
    vote: Vote<LeaderId>,
    effective: (Option<Id>, Members),
    committed: (Option<Id>, Members),
    applied: (Id, Members),
    applied_tail: Option<Id>,
    local_committed: Option<Id>,
    logs: Vec<(Id, Members)>,
}

enum Expectation<'a> {
    Isolated {
        u: &'a Observation,
        j: &'a Observation,
    },
    Parked(&'a Observation),
    Refusal {
        j: &'a Observation,
        mismatch: PreconditionFailed<Arx>,
        violation: Violation,
    },
    Final {
        j: &'a Observation,
        target: &'a Members,
    },
}

// This oracle has no scenario input. All expectations are saved before substitution.
fn check(e: Expectation<'_>, a: &Observation, api: Option<&Write>) -> Result<(), Violation> {
    match e {
        Expectation::Isolated { u, j } => {
            if !matches!(api, Some(Err(RaftError::APIError(ClientWriteError::ChangeMembershipError(ChangeMembershipError::InProgress(p)))))
                if p.committed == u.effective.0 && p.membership_log_id == j.effective.0)
            {
                return Err(Violation::InProgress);
            }
            if a.tail != j.tail
                || a.effective != j.effective
                || a.committed != u.committed
                || a.applied != u.applied
                || a.vote != u.vote
            {
                return Err(Violation::State);
            }
        }
        Expectation::Parked(j) => {
            if a.tail != j.effective.0 || a.effective != j.effective {
                return Err(Violation::NoUniform);
            }
            if a.committed != j.effective
                || Some(a.applied.0) != j.effective.0
                || a.applied.1 != j.effective.1
                || a.vote != j.vote
                || a.local_committed < j.effective.0
                || a.applied_tail < j.effective.0
            {
                return Err(Violation::State);
            }
        }
        Expectation::Refusal {
            j,
            mismatch,
            violation,
        } => {
            if !matches!(api, Some(Err(RaftError::APIError(ClientWriteError::PreconditionFailed(p)))) if *p == mismatch)
            {
                return Err(violation);
            }
            check(Expectation::Parked(j), a, None)?;
            if a.logs != j.logs {
                return Err(Violation::Logs);
            }
        }
        Expectation::Final { j, target } => {
            let Some(Ok(reply)) = api else {
                return Err(Violation::State);
            };
            let f = reply.log_id;
            if f.index != j.effective.0.ok_or(Violation::State)?.index + 1
                || f.leader_id != j.vote.leader_id
                || a.vote != j.vote
                || a.tail != Some(f)
                || a.effective.0 != Some(f)
                || a.committed != a.effective
                || a.applied != (f, a.effective.1.clone())
                || a.applied_tail < Some(f)
                || a.local_committed < Some(f)
                || reply.membership.as_ref() != Some(&a.effective.1)
            {
                return Err(Violation::State);
            }
            if a.effective.1.get_joint_config() != target.get_joint_config() {
                return Err(Violation::FinalVoters);
            }
            if a.effective.1.nodes().collect::<Vec<_>>() != target.nodes().collect::<Vec<_>>() {
                return Err(Violation::Metadata);
            }
            if a.logs
                != vec![
                    (j.effective.0.ok_or(Violation::Logs)?, j.effective.1.clone()),
                    (f, a.effective.1.clone()),
                ]
            {
                return Err(Violation::Logs);
            }
        }
    }
    Ok(())
}

async fn cap<T>(end: Instant, f: impl Future<Output = T>) -> Result<T, Failure> {
    let limit = end.min(Instant::now() + Duration::from_secs(5));
    if limit <= Instant::now() {
        return Err("shared deadline".into());
    }
    timeout_at(limit, f)
        .await
        .map_err(|_| "operation deadline".into())
}
fn good<T, E: std::fmt::Debug>(r: Result<T, E>) -> Result<T, Failure> {
    r.map_err(|e| format!("unexpected API/setup error: {e:?}"))
}
fn oracle(r: Result<(), Violation>) -> Result<(), Failure> {
    good(r)
}

struct Fixture {
    dirs: Vec<tempfile::TempDir>,
    nodes: Vec<ConsensusNode>,
    router: Arc<MemRouter>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            dirs: (0..3).map(|_| private_temp()).collect(),
            nodes: Vec::new(),
            router: Arc::default(),
        }
    }
    async fn start(&mut self, body_end: Instant, end: Instant) -> Result<(), Failure> {
        for i in 0..3 {
            if Instant::now() >= body_end {
                return Err("startup admission deadline".into());
            }
            let mut startup = Box::pin(ConsensusNode::start(
                i as u64 + 1,
                self.dirs[i].path(),
                Init {
                    cluster: [1; 16],
                    first_free: std::num::NonZeroU64::new(4).unwrap(),
                },
                Params::default(),
                Factory {
                    source: i as u64 + 1,
                    router: self.router.clone(),
                },
            ));
            let first = cap(body_end, startup.as_mut()).await;
            let (node, late) = match first {
                Ok(result) => (good(result)?, false),
                Err(_) => {
                    // Retain ownership through reserve; drop only caller if still pending.
                    match timeout_at(end, startup.as_mut()).await {
                        Ok(result) => (good(result)?, true),
                        Err(_) => return Err("StartupIncomplete; cleanup=unresolved; runtime blocking work may outlive body".into()),
                    }
                }
            };
            drop(startup);
            self.router
                .nodes
                .lock()
                .unwrap()
                .insert(i as u64 + 1, node.raft.as_ref().unwrap().clone());
            self.nodes.push(node);
            if late {
                return Err("startup exceeded operation/body budget".into());
            }
        }
        Ok(())
    }
    async fn cleanup(&mut self, end: Instant) -> Result<(), Failure> {
        self.router.nodes.lock().unwrap().clear();
        async fn stop(n: Option<&mut ConsensusNode>) -> Result<(), Failure> {
            if let Some(n) = n {
                good(n.shutdown().await)?;
                if !n.log_released.is_set()
                    || !n.sm_released.is_set()
                    || n.log_weak.strong_count() != 0
                    || n.sm_weak.strong_count() != 0
                {
                    return Err("store release unresolved".into());
                }
            }
            Ok(())
        }
        let mut iter = self.nodes.iter_mut();
        let results = cap(end, async {
            tokio::join!(stop(iter.next()), stop(iter.next()), stop(iter.next()))
        })
        .await?;
        // Inspect every independent result, even when another shutdown failed.
        match results {
            (Ok(()), Ok(()), Ok(())) => Ok(()),
            r => Err(format!("cleanup: {r:?}")),
        }
    }
    async fn observe(&self, i: usize, from: u64, end: Instant) -> Result<Observation, Failure> {
        let n = &self.nodes[i];
        let raft = n.raft.as_ref().unwrap();
        // Public log_ids.last() is exactly the private LogStateReader delegation.
        let tail = good(cap(end, raft.with_raft_state(|st| st.log_ids.last().cloned())).await?)?;
        let m = raft.metrics().borrow_watched().clone();
        let applied = cap(end, async {
            loop {
                if let Ok(s) = n.sm.as_ref().unwrap().0.try_lock() {
                    let (p, blob) = s.membership().ok_or("missing applied membership")?;
                    return Ok::<_, Failure>(((id(*p), good(decode(blob))?), s.applied().map(id)));
                }
                tokio::task::yield_now().await;
            }
        })
        .await??;
        let logs = cap(end, async {
            let log = n.log_weak.upgrade().ok_or("missing log owner")?;
            loop {
                if let Ok(mut store) = log.try_lock() {
                    return good(store.entries(from, tail.map_or(from, |p| p.index + 1), 64))?
                        .into_iter()
                        .filter_map(|(p, e)| match e {
                            StoredEntry::Membership(b) => {
                                Some(good(decode(&b)).map(|m| (id(p), m)))
                            }
                            _ => None,
                        })
                        .collect::<Result<Vec<_>, Failure>>();
                }
                tokio::task::yield_now().await;
            }
        })
        .await??;
        Ok(Observation {
            tail,
            vote: m.vote,
            effective: (
                *m.membership_config.log_id(),
                m.membership_config.membership().clone(),
            ),
            committed: (
                *m.committed_membership_config.log_id(),
                m.committed_membership_config.membership().clone(),
            ),
            applied: applied.0,
            applied_tail: applied.1,
            local_committed: m.local_committed,
            logs,
        })
    }
    async fn converge(
        &self,
        p: Id,
        membership: &Members,
        from: u64,
        end: Instant,
    ) -> Result<Observation, Failure> {
        cap(end, async {
            loop {
                let mut all = true;
                for i in 0..3 {
                    let a = self.observe(i, from, end).await?;
                    all &= a.effective == (Some(p), membership.clone())
                        && a.committed == a.effective
                        && a.applied == (p, membership.clone())
                        && a.local_committed >= Some(p)
                        && a.applied_tail >= Some(p);
                }
                if all {
                    return self.observe(0, from, end).await;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?
    }
}

fn guards(a: &Observation) -> Vec<Precondition<Arx>> {
    vec![
        Precondition::LastMembershipLogId {
            last_membership_log_id: a.effective.0,
        },
        Precondition::LastLogId {
            last_log_id: a.tail,
        },
        Precondition::CommittedLeaderId {
            committed_leader_id: a.vote.leader_id,
        },
    ]
}

async fn body(f: &Fixture, scenario: Scenario, end: Instant) -> Result<(), Failure> {
    let raft = f.nodes[0].raft.as_ref().unwrap();
    good(
        cap(
            end,
            raft.initialize(BTreeMap::from([(1, BasicNode::new("mem:1"))])),
        )
        .await?,
    )?;
    cap(end, async {
        loop {
            let m = raft.metrics().borrow_watched().clone();
            if m.state == openraft::ServerState::Leader
                && m.vote.committed
                && m.local_committed == m.last_applied
                && m.last_applied.is_some()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    for i in 2..=4 {
        good(
            cap(
                end,
                raft.add_learner(i, BasicNode::new(format!("mem:{i}")), i != 4),
            )
            .await?,
        )?;
    }
    let u = cap(end, async {
        loop {
            let a = f.observe(0, 0, end).await?;
            if a.effective == a.committed
                && Some(a.applied.0) == a.effective.0
                && a.applied.1 == a.effective.1
                && a.local_committed == a.tail
                && a.applied_tail == a.tail
                && a.vote.committed
            {
                return Ok::<_, Failure>(a);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    if u.effective.1.get_joint_config() != &vec![BTreeSet::from([1])]
        || u.effective
            .1
            .nodes()
            .map(|(i, n)| (*i, n.addr.clone()))
            .collect::<BTreeMap<_, _>>()
            != (1..=4).map(|i| (i, format!("mem:{i}"))).collect()
    {
        return Err("uniform setup contents".into());
    }
    f.router.isolated.lock().unwrap().extend([2, 3]);
    let mut parked = Box::pin(raft.change_membership_if(
        ChangeMembers::AddVoterIds(BTreeSet::from([2, 3])),
        true,
        [guards(&u)[0].clone(), guards(&u)[2].clone()],
    ));
    let first = poll_fn(|cx| {
        Poll::Ready(std::pin::pin!(tokio::task::unconstrained(parked.as_mut())).poll(cx))
    })
    .await;
    if !first.is_pending() {
        return Err("single poll unexpectedly Ready".into());
    }
    let j = cap(end, async {
        loop {
            let a = f.observe(0, 0, end).await?;
            if a.effective.1.get_joint_config()
                == &vec![BTreeSet::from([1]), BTreeSet::from([1, 2, 3])]
            {
                return Ok::<_, Failure>(a);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    if j.effective.0 <= u.effective.0
        || j.vote != u.vote
        || j.committed != u.committed
        || j.effective.1.nodes().collect::<Vec<_>>() != u.effective.1.nodes().collect::<Vec<_>>()
    {
        return Err("isolated joint seam".into());
    }
    let target = good(Membership::new(
        vec![
            j.effective
                .1
                .get_joint_config()
                .last()
                .ok_or("missing last config")?
                .clone(),
        ],
        j.effective
            .1
            .nodes()
            .map(|(i, n)| (*i, n.clone()))
            .collect::<BTreeMap<_, _>>(),
    ))?;
    let refused = cap(
        end,
        raft.append_membership(target.clone(), EntryPayload::Blank, guards(&j)),
    )
    .await?;
    let a = f.observe(0, 0, end).await?;
    oracle(check(
        Expectation::Isolated { u: &u, j: &j },
        &a,
        Some(&refused),
    ))?;
    f.router.isolated.lock().unwrap().clear();
    let joint = j.effective.0.ok_or("missing J id")?;
    let j = f.converge(joint, &j.effective.1, joint.index, end).await?;
    oracle(check(Expectation::Parked(&j), &j, None))?;
    if matches!(scenario, Scenario::ResumeFuture) {
        let reply = cap(end, parked.as_mut()).await?;
        drop(parked);
        let p = good(reply.as_ref().map(|r| r.log_id))?;
        let a = f.converge(p, &target, joint.index, end).await?;
        oracle(check(
            Expectation::Final {
                j: &j,
                target: &target,
            },
            &a,
            Some(&reply),
        ))?;
        return good(match check(Expectation::Parked(&j), &a, None) {
            Err(Violation::NoUniform) => Ok(()),
            other => Err(format!("resume oracle: {other:?}")),
        });
    }
    drop(parked);
    oracle(check(
        Expectation::Parked(&j),
        &f.observe(0, joint.index, end).await?,
        None,
    ))?;
    for index in 0..3 {
        let fresh = f.observe(0, joint.index, end).await?;
        oracle(check(Expectation::Parked(&j), &fresh, None))?;
        let mut request = guards(&fresh);
        let (stale, mismatch, violation) = match index {
            0 => (
                Precondition::LastMembershipLogId {
                    last_membership_log_id: u.effective.0,
                },
                PreconditionFailed::LastMembershipLogIdMismatch {
                    expected: u.effective.0,
                    actual: fresh.effective.0,
                },
                Violation::MembershipRefusalExpected,
            ),
            1 => (
                Precondition::LastLogId {
                    last_log_id: u.tail,
                },
                PreconditionFailed::LastLogIdMismatch {
                    expected: u.tail,
                    actual: fresh.tail,
                },
                Violation::TailRefusalExpected,
            ),
            _ => {
                let old = LeaderId {
                    term: fresh
                        .vote
                        .leader_id
                        .term
                        .checked_sub(1)
                        .ok_or("no older term")?,
                    node_id: fresh.vote.leader_id.node_id,
                };
                (
                    Precondition::CommittedLeaderId {
                        committed_leader_id: old,
                    },
                    PreconditionFailed::CommittedLeaderIdMismatch {
                        expected: old,
                        actual: Some(fresh.vote.leader_id),
                    },
                    Violation::LeaderRefusalExpected,
                )
            }
        };
        request[index] = stale;
        let omit = matches!(
            (scenario, index),
            (Scenario::OmitMembershipGuard, 0)
                | (Scenario::OmitTailGuard, 1)
                | (Scenario::OmitLeaderGuard, 2)
        );
        if omit {
            request.remove(index);
        }
        let reply = cap(
            end,
            raft.append_membership(target.clone(), EntryPayload::Blank, request),
        )
        .await?;
        if omit {
            let p = good(reply.as_ref().map(|r| r.log_id))?;
            let a = f.converge(p, &target, joint.index, end).await?;
            oracle(check(
                Expectation::Final {
                    j: &j,
                    target: &target,
                },
                &a,
                Some(&reply),
            ))?;
            let expected = match index {
                0 => Violation::MembershipRefusalExpected,
                1 => Violation::TailRefusalExpected,
                _ => Violation::LeaderRefusalExpected,
            };
            return if check(
                Expectation::Refusal {
                    j: &j,
                    mismatch,
                    violation,
                },
                &a,
                Some(&reply),
            ) == Err(expected)
            {
                Ok(())
            } else {
                Err("omission oracle did not reject exactly".into())
            };
        }
        let a = f.observe(0, joint.index, end).await?;
        oracle(check(
            Expectation::Refusal {
                j: &j,
                mismatch,
                violation,
            },
            &a,
            Some(&reply),
        ))?;
    }
    let mut configs = target.get_joint_config().clone();
    let mut nodes = target
        .nodes()
        .map(|(i, n)| (*i, n.clone()))
        .collect::<BTreeMap<_, _>>();
    match scenario {
        Scenario::FirstConfig => configs = vec![j.effective.1.get_joint_config()[0].clone()],
        Scenario::DropLearner4 => {
            nodes.remove(&4);
        }
        _ => (),
    }
    let submitted = good(Membership::new(configs, nodes))?;
    let fresh = f.observe(0, joint.index, end).await?;
    oracle(check(Expectation::Parked(&j), &fresh, None))?;
    let reply = cap(
        end,
        raft.append_membership(submitted.clone(), EntryPayload::Blank, guards(&fresh)),
    )
    .await?;
    let p = good(reply.as_ref().map(|r| r.log_id))?;
    let a = f.converge(p, &submitted, joint.index, end).await?;
    // Validate unaffected fields separately, preserving saved target for rejection.
    oracle(check(
        Expectation::Final {
            j: &j,
            target: &submitted,
        },
        &a,
        Some(&reply),
    ))?;
    let result = check(
        Expectation::Final {
            j: &j,
            target: &target,
        },
        &a,
        Some(&reply),
    );
    let expected = match scenario {
        Scenario::FirstConfig => Err(Violation::FinalVoters),
        Scenario::DropLearner4 => Err(Violation::Metadata),
        _ => Ok(()),
    };
    if result != expected {
        return Err(format!("final oracle: {result:?}, expected {expected:?}"));
    }
    // Lost-reply readback: discard successful response, observe without another write.
    drop(reply);
    let readback = f.observe(0, joint.index, end).await?;
    if readback.tail != a.tail
        || readback.effective != a.effective
        || readback.applied != a.applied
        || readback.logs != a.logs
    {
        return Err("lost reply readback changed".into());
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[cfg_attr(
    target_os = "windows",
    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"
)]
async fn interrupted_joint_exact_guarded_flatten() {
    let end = Instant::now() + Duration::from_secs(75);
    let body_end = end - Duration::from_secs(5);
    let _owner = cap(body_end, APPLY_PAUSE_OWNER.lock())
        .await
        .expect("pause owner admission");
    for scenario in [
        Scenario::Positive,
        Scenario::OmitMembershipGuard,
        Scenario::OmitTailGuard,
        Scenario::OmitLeaderGuard,
        Scenario::FirstConfig,
        Scenario::DropLearner4,
        Scenario::ResumeFuture,
    ] {
        let mut f = Fixture::new();
        let result = match f.start(body_end, end).await {
            Ok(()) => body(&f, scenario, body_end).await,
            Err(e) => Err(e),
        };
        // Capture bounded observations before cleanup without a borrowed engine/store.
        let metrics: Vec<_> = f
            .nodes
            .iter()
            .map(|n| {
                n.raft
                    .as_ref()
                    .map(|r| r.metrics().borrow_watched().clone())
            })
            .collect();
        let cleanup = f.cleanup(end).await;
        assert!(
            cleanup.is_ok() && result.is_ok(),
            "{scenario:?}: body={result:?}; cleanup={cleanup:?}; metrics={metrics:?}"
        );
    }
}

//! One retained owner Mint. The owner socket is wired to this job in 13d.
use crate::cluster::{
    enrollment::{JoinAnchor, Outcome},
    state_store::LogPos,
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct View {
    term: u64,
    leader: u64,
    membership: LogPos,
    anchor: JoinAnchor,
    root: [u8; 32],
    endpoint: std::net::SocketAddr,
    listener: u64,
}

fn first_expiry(
    before: View,
    floor: LogPos,
    position: LogPos,
    result: Option<Outcome>,
) -> Option<u64> {
    match result {
        Some(Outcome::Minted {
            expires_ms,
            replay: false,
        }) if (position.term, position.leader) == (before.term, before.leader)
            && position.index > floor.index
            && expires_ms <= crate::cluster::enrollment::MAX_TIME_MS =>
        {
            Some(expires_ms)
        }
        _ => None,
    }
}

fn may_reveal(
    before: View,
    after: View,
    applied: LogPos,
    mint: LogPos,
    now: u64,
    expiry: u64,
) -> bool {
    before == after
        && super::learner_admission::covers(applied, mint)
        && now
            .checked_add(2 * crate::cluster::enrollment::SKEW_MS)
            .is_some_and(|usable| usable < expiry)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::{
        cluster::{
            consensus::{
                bound_start::{Job, Status, enrollment_ingress::ReadyIssuer},
                *,
            },
            enrollment::{ActorId, BoundVoters, CODE_TTL_MS, CodeId, JoinCode, transfer},
        },
        service::cluster_owner::{Operator, Paste, Reply},
    };
    use checked_clock::CheckedClock;
    use openraft::type_config::async_runtime::watch::WatchReceiver;
    use sha2::{Digest, Sha256};
    use std::{net::SocketAddr, time::Duration};
    use tokio::time::timeout;

    const MINT_WAIT: Duration = Duration::from_secs(10);

    fn actor(owner: &Operator, installation: [u8; 16], cluster: [u8; 16]) -> ActorId {
        let bytes = Sha256::new()
            .chain_update(b"arxcovo/owner-local/mint-actor/v1")
            .chain_update(owner.uid().to_be_bytes())
            .chain_update(installation)
            .chain_update(cluster)
            .finalize();
        bytes[..16].try_into().expect("fixed digest")
    }

    #[derive(PartialEq, Eq)]
    struct Epoch {
        leader: LeaderId,
        at: Id,
        members: Members,
    }

    fn epoch(node: &ConsensusNode) -> Option<Epoch> {
        let metrics = node.raft.as_ref()?.metrics();
        let m = metrics.borrow_watched();
        let effective = &m.membership_config;
        let committed = &m.committed_membership_config;
        let at = (*effective.log_id())?;
        let members = effective.membership();
        if m.running_state.is_err()
            || m.state != openraft::ServerState::Leader
            || m.current_leader != Some(node.id)
            || !m.vote.committed
            || m.vote.leader_id.node_id != node.id
            || committed.log_id() != &Some(at)
            || committed.membership() != members
            || members.get_joint_config().len() != 1
        {
            return None;
        }
        encode(members).ok()?; // Existing bounds before cloning.
        Some(Epoch {
            leader: m.vote.leader_id,
            at,
            members: members.clone(),
        })
    }

    fn is_current(node: &ConsensusNode, view: View) -> bool {
        epoch(node).is_some_and(|e| {
            e.leader.term == view.term
                && e.leader.node_id == view.leader
                && pos(e.at) == view.membership
        })
    }

    struct Serving {
        issuer: Arc<ReadyIssuer>,
        listener: u64,
        endpoint: SocketAddr,
    }

    impl Job {
        // installation is retained custody context from the future lifecycle
        // caller. There is no installation field in the owner protocol.
        pub(in crate::cluster::consensus) async fn mint_owner(
            &self,
            owner: Operator,
            installation: [u8; 16],
        ) -> Reply {
            let Ok(_lease) = self.mint_lease.try_lock() else {
                return Reply::Refused;
            };
            let (serving, node) = {
                let Ok(s) = self.state.lock() else {
                    return Reply::Refused;
                };
                if s.close_requested
                    || !matches!(s.status, Status::FounderReady | Status::LearnerReady)
                    || s.ingress.is_none()
                {
                    return Reply::Refused;
                }
                let (Some(issuer), Some(node), Some(listener)) =
                    (&s.issuer, &s.node, s.serving_enrollment)
                else {
                    return Reply::Refused;
                };
                if !listener.active() {
                    return Reply::Refused;
                }
                (
                    Serving {
                        issuer: issuer.clone(),
                        listener: listener.cookie,
                        endpoint: listener.endpoint,
                    },
                    node.clone(),
                )
            };
            let Ok(node) = node.try_lock() else {
                return Reply::Refused;
            };
            mint(self, &node, &serving, &owner, installation).await
        }

        fn still_serving(&self, serving: &Serving) -> bool {
            self.state.lock().is_ok_and(|s| {
                !s.close_requested
                    && matches!(s.status, Status::FounderReady | Status::LearnerReady)
                    && s.ingress.is_some()
                    && s.issuer
                        .as_ref()
                        .is_some_and(|issuer| Arc::ptr_eq(issuer, &serving.issuer))
                    && s.serving_enrollment.is_some_and(|w| {
                        w.cookie == serving.listener && w.endpoint == serving.endpoint && w.active()
                    })
            })
        }
    }

    impl Serving {
        async fn view(
            &self,
            node: &ConsensusNode,
            code: Option<CodeId>,
        ) -> Option<(View, LogPos, u64)> {
            let before = epoch(node)?;
            let (cluster, root, key) = self.issuer.transfer_identity();
            let member = self.issuer.member();
            let expected = before.at;
            let expected_members = before.members.clone();
            let clock = self.issuer.mint_clock();
            let (leader, (anchor, applied, now)) = node
                .fresh_read(move |store| {
                    let (at, blob) = store.membership().ok_or(StoreError::Membership)?;
                    let applied = store.applied().ok_or(StoreError::Membership)?;
                    if id(*at) != expected || !learner_admission::covers(applied, *at) {
                        return Err(StoreError::Membership);
                    }
                    if decode(blob)? != expected_members {
                        return Err(StoreError::Membership);
                    }
                    let members =
                        WireMembership::decode(blob.bytes()).map_err(|_| StoreError::Membership)?;
                    store
                        .state()
                        .authorize_peer(cluster, member, key)
                        .map_err(|_| StoreError::Membership)?;
                    let now = clock.sample_ms().map_err(|_| StoreError::Io)?;
                    let voters = match code {
                        Some(code) => store
                            .state()
                            .pending_voters(cluster, code, now, Some(&members))
                            .map_err(|_| StoreError::Membership)?,
                        None => BoundVoters::observe(store.state(), &members)
                            .map_err(|_| StoreError::Membership)?,
                    };
                    if !voters.contains(member, key) {
                        return Err(StoreError::Membership);
                    }
                    Ok((voters.anchor(cluster), applied, now))
                })
                .await
                .ok()?;
            if leader != before.leader
                || before != epoch(node)?
                || self.issuer.available(node).is_err()
            {
                return None;
            }
            Some((
                View {
                    term: leader.term,
                    leader: leader.node_id,
                    membership: pos(before.at),
                    anchor,
                    root,
                    endpoint: self.endpoint,
                    listener: self.listener,
                },
                applied,
                now,
            ))
        }
    }

    async fn mint(
        job: &Job,
        node: &ConsensusNode,
        serving: &Serving,
        owner: &Operator,
        installation: [u8; 16],
    ) -> Reply {
        let Some((before, floor, proposed_at_ms)) = serving.view(node, None).await else {
            return Reply::Refused;
        };
        let (command, code_id, paste) = {
            let (cluster, root, _) = serving.issuer.transfer_identity();
            let Ok(code) = JoinCode::generate(cluster, root, before.anchor, getrandom::fill) else {
                return Reply::Refused;
            };
            let mut request = [0; 16];
            if getrandom::fill(&mut request).is_err() {
                return Reply::Refused;
            }
            // Prepare the protected value before the irreversible boundary, including
            // ADR0040's fixed endpoint and root checks.
            let Ok(value) = transfer::encode(before.endpoint, serving.issuer.mint_root(), &code)
            else {
                return Reply::Refused;
            };
            let paste = Paste::copy(value.expose_for_owner_transfer().as_bytes());
            drop(value);
            let Some(paste) = paste else {
                return Reply::Refused;
            };
            let code_id = code.code_id();
            let command = Command::Mint {
                cluster,
                actor: actor(owner, installation, cluster),
                request,
                code: code_id,
                verifier: code.verifier(),
                proposed_at_ms,
            };
            (command, code_id, paste)
        };
        if !job.still_serving(serving)
            || !is_current(node, before)
            || serving.issuer.available(node).is_err()
        {
            return Reply::Refused;
        }
        let (position, outcome) = match timeout(MINT_WAIT, node.propose(command)).await {
            Ok(Ok((_, Some(Outcome::Rejected(_))))) => return Reply::Refused,
            Ok(Ok(result)) => result,
            Ok(Err(ProposeError::Busy | ProposeError::Stopped | ProposeError::NotLeader(_))) => {
                return Reply::Refused;
            }
            _ => return Reply::Unknown,
        };
        let Some(expiry) = first_expiry(before, floor, position, outcome) else {
            return Reply::Unknown;
        };
        let Some((after, applied, now)) = serving.view(node, Some(code_id)).await else {
            return Reply::Unknown;
        };
        if proposed_at_ms
            .checked_add(CODE_TTL_MS)
            .is_none_or(|minimum| expiry < minimum)
            || !may_reveal(before, after, applied, position, now, expiry)
            || !job.still_serving(serving)
            || !is_current(node, after)
        {
            return Reply::Unknown;
        }
        Reply::Minted(paste)
    }
}

#[cfg(test)]
mod tests;

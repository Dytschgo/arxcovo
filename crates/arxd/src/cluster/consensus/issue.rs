//! Trusted adapter: only this module constructs issuance tickets.
use super::checked_clock::{CheckedClock, ClockError};
use super::*;
use crate::cluster::{
    enrollment::{
        ClusterId, CodeId, EnrollmentGrant, IssueRefusal, JoinAnchor, JoinerReply, KeyDigest,
        NodeId, PeerRefusal, RedeemProof, Reject, RequestId,
    },
    pki::{
        ca::ClusterCa,
        csr::{CsrError, ProvenCsr},
    },
};
pub(super) const MAX_ISSUING: usize = 4;
const ISSUE_BUDGET: Duration = Duration::from_secs(5);
pub trait SignerClock: Send + Sync {
    fn now_ms(&self) -> u64;
}
pub struct RedeemInput {
    pub cluster: ClusterId,
    pub request: RequestId,
    pub code: CodeId,
    pub proof: RedeemProof,
    pub anchor: JoinAnchor,
}
pub struct IssuedLeaf {
    pub der: Vec<u8>,
    pub node: NodeId,
    pub identity_commit: LogPos,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueError {
    Busy,
    NotLeader(Option<u64>),
    Unavailable,
    BadCsr(CsrError),
    Rejected(Reject),
    WrongNode,
    StaleIssuer,
    Revoked,
    Signer(IssueRefusal),
    ClockRollback,
    CaExpiring,
    Sign,
}
impl std::fmt::Display for IssueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Busy => "issuer busy",
            Self::NotLeader(_) => "not leader",
            Self::Unavailable => "issuer unavailable",
            Self::BadCsr(_) => "CSR refused",
            Self::Rejected(_) => "enrollment rejected",
            Self::WrongNode => "wrong issuing node",
            Self::StaleIssuer => "stale issuer",
            Self::Revoked => "member revoked",
            Self::Signer(_) => "signer time refused",
            Self::ClockRollback => "clock rollback",
            Self::CaExpiring => "CA expiring",
            Self::Sign => "signing failed",
        })
    }
}
impl std::error::Error for IssueError {}
impl IssueError {
    pub fn joiner_reply(&self) -> JoinerReply {
        match self {
            Self::NotLeader(_) | Self::Unavailable | Self::Busy | Self::StaleIssuer => {
                JoinerReply::RetryLater
            }
            _ => JoinerReply::Rejected,
        }
    }
}
pub(in crate::cluster) struct IssueTicket {
    grant: EnrollmentGrant,
    leader: LeaderId,
    index: u64,
}
impl IssueTicket {
    pub(in crate::cluster) fn cluster(&self) -> ClusterId {
        self.grant.cluster()
    }
    pub(in crate::cluster) fn node(&self) -> NodeId {
        self.grant.node()
    }
    pub(in crate::cluster) fn key(&self) -> KeyDigest {
        self.grant.key()
    }
}
impl ConsensusNode {
    fn committed(
        &self,
        resp: openraft::raft::ClientWriteResponse<Arx>,
    ) -> Result<IssueTicket, IssueError> {
        if resp.log_id.leader_id.node_id != self.id {
            return Err(IssueError::WrongNode);
        }
        match resp.data.0 {
            Some(Outcome::Enrolled(grant)) => {
                let origin = grant.origin();
                let current = LogPos {
                    term: resp.log_id.leader_id.term,
                    leader: resp.log_id.leader_id.node_id,
                    index: resp.log_id.index,
                };
                if origin.index > current.index
                    || origin.term > current.term
                    || (origin.index == current.index && origin != current)
                {
                    return Err(IssueError::Rejected(Reject::UnknownCode));
                }
                Ok(IssueTicket {
                    grant,
                    leader: resp.log_id.leader_id,
                    index: resp.log_id.index,
                })
            }
            Some(Outcome::Rejected(r)) => Err(IssueError::Rejected(r)),
            _ => Err(IssueError::Rejected(Reject::UnknownCode)),
        }
    }
    async fn issue_guard(
        &self,
        ticket: &IssueTicket,
        code: CodeId,
        key: KeyDigest,
    ) -> Result<(), IssueError> {
        let grant = ticket.grant.clone();
        let (leader, result) = self
            .fresh_read(move |s| {
                Ok((|| {
                    s.state()
                        .check_grant(&grant, &code)
                        .map_err(IssueError::Rejected)?;
                    s.state()
                        .authorize_peer(grant.cluster(), grant.node(), grant.key())
                        .map_err(|e| match e {
                            PeerRefusal::WrongCluster => IssueError::Rejected(Reject::WrongCluster),
                            PeerRefusal::NotMember | PeerRefusal::KeyMismatch => {
                                IssueError::Revoked
                            }
                        })?;
                    if grant.key() != key {
                        return Err(IssueError::Revoked);
                    }
                    Ok(())
                })())
            })
            .await
            .map_err(|e| match e {
                ReadError::Busy => IssueError::Busy,
                _ => IssueError::Unavailable,
            })?;
        if leader != ticket.leader {
            return Err(IssueError::StaleIssuer);
        }
        result
    }
    /// Caller-owned max-four admission. Inline bounded crypto is not preemptible;
    /// the absolute budget bounds waiting at await points, not crypto execution.
    pub async fn enroll(
        &self,
        ca: &ClusterCa,
        clock: &dyn CheckedClock,
        input: RedeemInput,
        csr: &[u8],
    ) -> Result<IssuedLeaf, IssueError> {
        let deadline = Instant::now() + ISSUE_BUDGET;
        let _permit = self
            .issue_permits
            .try_acquire()
            .map_err(|_| IssueError::Busy)?;
        let csr = ProvenCsr::parse(csr).map_err(IssueError::BadCsr)?;
        let (code, command) = {
            let RedeemInput {
                cluster,
                request,
                code,
                proof,
                anchor,
            } = input;
            let command = Command::redeem(
                cluster,
                request,
                code,
                &proof,
                csr.digest(),
                anchor,
                clock.sample_ms().map_err(issue_clock_error)?,
            );
            // The proof's scope ends before any await; this is not erasure.
            (code, command)
        };
        let work = async {
            let resp = self.propose_response(command).await.map_err(|e| match e {
                ProposeError::Busy => IssueError::Busy,
                ProposeError::NotLeader(l) => IssueError::NotLeader(l),
                ProposeError::Stopped | ProposeError::Unknown => IssueError::Unavailable,
            })?;
            let ticket = self.committed(resp)?;
            #[cfg(test)]
            self.issue_pause(PausePoint::AfterCommit).await;
            self.issue_guard(&ticket, code, csr.digest()).await?;
            let pre_now = clock.sample_ms().map_err(issue_clock_error)?;
            ticket
                .grant
                .check_issuable(pre_now)
                .map_err(IssueError::Signer)?;
            let der = ca.sign_leaf(&ticket, &csr, pre_now)?;
            #[cfg(test)]
            self.issue_pause(PausePoint::AfterSign).await;
            self.issue_guard(&ticket, code, csr.digest()).await?;
            let post_now = clock.sample_ms().map_err(issue_clock_error)?;
            ticket
                .grant
                .check_issuable(post_now)
                .map_err(IssueError::Signer)?;
            if post_now < pre_now {
                return Err(IssueError::ClockRollback);
            }
            Ok(IssuedLeaf {
                der,
                node: ticket.node(),
                identity_commit: ticket.grant.origin(),
            })
        };
        match timeout_at(deadline, work).await {
            Ok(r) if Instant::now() < deadline => r,
            _ => Err(IssueError::Unavailable),
        }
    }
}
fn issue_clock_error(error: ClockError) -> IssueError {
    match error {
        ClockError::Rollback => IssueError::ClockRollback,
        _ => IssueError::Unavailable,
    }
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PausePoint {
    AfterCommit,
    AfterSign,
}
#[cfg(test)]
struct IssuePause {
    store: usize,
    point: PausePoint,
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}
#[cfg(test)]
static ISSUE_PAUSE: Mutex<Option<IssuePause>> = Mutex::new(None);
#[cfg(test)]
impl ConsensusNode {
    async fn issue_pause(&self, point: PausePoint) {
        let store = Arc::as_ptr(&self.sm.as_ref().unwrap().0) as usize;
        let pause = {
            let mut slot = ISSUE_PAUSE.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|p| p.store == store && p.point == point)
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(p) = pause {
            let _ = p.entered.send(());
            let _ = p.release.await;
        }
    }
}
#[cfg(test)]
#[path = "issue_tests.rs"]
mod issue_tests;
#[cfg(test)]
pub(in crate::cluster) fn test_ticket(key: KeyDigest) -> IssueTicket {
    use crate::cluster::enrollment::{EnrollmentState, FoundMembers, JoinCode};
    let test_membership = crate::cluster::membership::Membership::new(
        vec![[1].into()],
        [(1, "mem:1".to_owned())].into(),
    )
    .unwrap();
    let mut s = EnrollmentState::new([1; 16], NodeId::new(4).unwrap());
    s.apply(
        &Command::Found {
            cluster: [1; 16],
            members: FoundMembers::new([(
                NodeId::new(1).unwrap(),
                KeyDigest::of_spki(&[250]).unwrap(),
            )])
            .unwrap(),
            proposed_at_ms: 0,
        },
        LogPos {
            term: 1,
            leader: 1,
            index: 1,
        },
        Some(&test_membership),
    );
    let c = JoinCode::generate(
        [1; 16],
        [2; 32],
        s.voter_anchor(&test_membership).unwrap(),
        |b| {
            b.fill(19);
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    s.apply(
        &Command::Mint {
            cluster: [1; 16],
            actor: [3; 16],
            request: [4; 16],
            code: c.code_id(),
            verifier: c.verifier(),
            proposed_at_ms: 1000,
        },
        LogPos {
            term: 1,
            leader: 1,
            index: 1,
        },
        Some(&test_membership),
    );
    let Outcome::Enrolled(grant) = s.apply(
        &Command::redeem(
            [1; 16],
            [5; 16],
            c.code_id(),
            &c.proof(),
            key,
            c.anchor(),
            1000,
        ),
        LogPos {
            term: 1,
            leader: 1,
            index: 1,
        },
        Some(&test_membership),
    ) else {
        panic!("test enrollment");
    };
    IssueTicket {
        grant,
        leader: LeaderId {
            term: 1,
            node_id: 1,
        },
        index: 1,
    }
}

mod sealed {
    pub trait Sealed {}
}
pub(in crate::cluster) trait LeafSubject: sealed::Sealed {
    fn cluster(&self) -> ClusterId;
    fn node(&self) -> NodeId;
    fn key(&self) -> KeyDigest;
}
impl sealed::Sealed for IssueTicket {}
impl LeafSubject for IssueTicket {
    fn cluster(&self) -> ClusterId {
        self.cluster()
    }
    fn node(&self) -> NodeId {
        self.node()
    }
    fn key(&self) -> KeyDigest {
        self.key()
    }
}
#[path = "found.rs"]
pub(in crate::cluster) mod found;

impl ConsensusNode {
    /// Local applied registry only; stale acceptance and lock waiting are unbounded.
    pub fn peer_registry(&self) -> crate::cluster::pki::verify::Registry {
        use crate::cluster::pki::verify::VerifyRefusal;
        let weak = self.sm_weak.clone();
        Arc::new(move |cluster, node, key| {
            let store = weak.upgrade().ok_or(VerifyRefusal::Unavailable)?;
            let result = {
                let guard = store.lock().map_err(|_| VerifyRefusal::Unavailable)?;
                guard
                    .state()
                    .authorize_peer(cluster, node, key)
                    .map_err(|e| match e {
                        PeerRefusal::WrongCluster => VerifyRefusal::WrongCluster,
                        PeerRefusal::NotMember => VerifyRefusal::NotMember,
                        PeerRefusal::KeyMismatch => VerifyRefusal::KeyMismatch,
                    })
            };
            drop(store);
            result
        })
    }
}
#[cfg(test)]
#[path = "tls_engine_tests.rs"]
mod tls_engine_tests;

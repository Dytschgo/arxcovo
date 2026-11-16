use super::mem_net::{Factory, MemRouter};
use super::*;
use crate::cluster::enrollment::{CODE_TTL_MS, JoinCode, KeyDigest, Reject};
use crate::service::application::tests::private_temp;
use std::sync::atomic::Ordering;
use tokio::time::timeout;

// Own the single adapter pause slot across each complete async fixture.
static APPLY_PAUSE_OWNER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Cluster {
    dirs: Vec<tempfile::TempDir>,
    nodes: Vec<Option<Arc<ConsensusNode>>>,
    router: Arc<MemRouter>,
    params: Params,
    acknowledged: Mutex<Vec<LogPos>>,
}
impl Cluster {
    async fn new(params: Params) -> Self {
        let c = Self::unfounded(params).await;
        c.write(founders()).await;
        c
    }
    async fn unfounded(params: Params) -> Self {
        let mut c = Self {
            dirs: (0..3).map(|_| private_temp()).collect(),
            nodes: vec![None, None, None],
            router: Arc::default(),
            params,
            acknowledged: Mutex::default(),
        };
        for i in 0..3 {
            c.start(i).await;
        }
        let members = (1..=3)
            .map(|n| (n, BasicNode::new(format!("mem:{n}"))))
            .collect::<BTreeMap<_, _>>();
        encode(&Membership::new(vec![[1, 2, 3].into()], members.clone()).unwrap()).unwrap();
        c.node(0)
            .raft
            .as_ref()
            .unwrap()
            .initialize(members)
            .await
            .unwrap();
        c.leader().await;
        c
    }
    fn node(&self, i: usize) -> &Arc<ConsensusNode> {
        self.nodes[i].as_ref().unwrap()
    }
    async fn start(&mut self, i: usize) {
        let n = ConsensusNode::start(
            i as u64 + 1,
            self.dirs[i].path(),
            Init {
                cluster: [1; 16],
                first_free: std::num::NonZeroU64::new(4).unwrap(),
            },
            self.params,
            Factory {
                source: i as u64 + 1,
                router: self.router.clone(),
            },
        )
        .await
        .unwrap();
        self.router
            .nodes
            .lock()
            .unwrap()
            .insert(i as u64 + 1, n.raft.as_ref().unwrap().clone());
        self.router.isolated.lock().unwrap().remove(&(i as u64 + 1));
        self.nodes[i] = Some(Arc::new(n));
    }
    async fn stop(&mut self, i: usize) {
        self.router.isolated.lock().unwrap().insert(i as u64 + 1);
        self.router.nodes.lock().unwrap().remove(&(i as u64 + 1));
        let mut n = Arc::try_unwrap(self.nodes[i].take().unwrap())
            .ok()
            .expect("caller still owns node");
        n.shutdown().await.unwrap();
        assert!(n.log_weak.upgrade().is_none());
        assert!(n.sm_weak.upgrade().is_none());
        assert_eq!(n.permits.available_permits(), 64);
        assert!(n.owners.lock().unwrap().set.is_empty());
        assert_eq!(n.propose(unknown(250)).await, Err(ProposeError::Stopped));
    }
    async fn leader(&self) -> usize {
        timeout(Duration::from_secs(5), async {
            loop {
                for (i, n) in self.nodes.iter().enumerate() {
                    if let Some(n) = n
                        && n.raft.as_ref().unwrap().metrics().borrow_watched().state
                            == openraft::ServerState::Leader
                    {
                        return i;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("leader deadline")
    }
    async fn write(&self, cmd: Command) -> (LogPos, Option<Outcome>) {
        timeout(Duration::from_secs(5), async {
            loop {
                let i = self.leader().await;
                match self.node(i).propose(cmd.clone()).await {
                    Ok(r) => {
                        self.acknowledged.lock().unwrap().push(r.0);
                        return r;
                    }
                    Err(ProposeError::NotLeader(_)) => tokio::task::yield_now().await,
                    e => panic!("proposal: {e:?}"),
                }
            }
        })
        .await
        .unwrap()
    }
    async fn converge(&self) {
        let l = self.leader().await;
        let (p, b) = self.node(l).linearizable_state().await.unwrap();
        assert!(
            self.acknowledged
                .lock()
                .unwrap()
                .iter()
                .all(|a| p.is_some_and(|p| p.index >= a.index))
        );
        for n in self.nodes.iter().flatten() {
            n.raft
                .as_ref()
                .unwrap()
                .wait(Some(Duration::from_secs(5)))
                .applied_index_at_least(p.map(|p| p.index), "convergence")
                .await
                .unwrap();
            let sm = n.sm.as_ref().unwrap().clone();
            let expected = b.clone();
            blocking(move || {
                let s = sm.0.lock().unwrap();
                assert_eq!(s.applied(), p);
                assert_eq!(s.snapshot(), expected);
                Ok(())
            })
            .await
            .unwrap();
        }
    }
    async fn finish(&mut self) {
        for i in 0..3 {
            if self.nodes[i].is_some() {
                self.stop(i).await;
            }
        }
        eprintln!(
            "RPC counters: mismatch={}, append_max={}, snapshot_max={}, snapshots={}",
            self.router.mismatch.load(Ordering::Relaxed),
            self.router.append_max.load(Ordering::Relaxed),
            self.router.snapshot_max.load(Ordering::Relaxed),
            self.router.snapshots.load(Ordering::Relaxed)
        );
        assert_eq!(self.router.mismatch.load(Ordering::Relaxed), 0);
        assert!(self.router.append_max.load(Ordering::Relaxed) <= 64);
        assert!(self.router.snapshot_max.load(Ordering::Relaxed) <= 16384);
        for dir in &self.dirs {
            for file in std::fs::read_dir(dir.path()).unwrap() {
                let bytes = std::fs::read(file.unwrap().path()).unwrap();
                for seed in [1u8, 2, 3, 4, 11, 12, 13, 100] {
                    use sha2::{Digest, Sha256};
                    let c = code(seed);
                    let secret = vec![seed.wrapping_add(128); 32];
                    let proof = Sha256::new()
                        .chain_update(b"arxcovo/join/proof/v1")
                        .chain_update(c.cluster())
                        .chain_update(c.code_id())
                        .chain_update(&secret)
                        .finalize()
                        .to_vec();
                    for marker in [secret, proof, c.expose_for_owner_transfer().into_bytes()] {
                        for m in [&marker, &hex::encode(&marker).into_bytes()] {
                            let contains = |bytes: &[u8]| bytes.windows(m.len()).any(|w| w == m);
                            let control = [b"prefix".as_slice(), m, b"suffix"].concat();
                            assert!(contains(&control), "privacy scanner positive control");
                            assert!(!contains(&bytes), "persisted secret marker");
                        }
                    }
                }
            }
        }
    }
}

// Real RPC catch-up: no direct snapshot installation or fabricated acknowledgments.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t3_snapshot_catchup_then_consecutive_append() {
    timeout(Duration::from_secs(45), async {
        let started = Instant::now();
        let params = Params {
            snapshot_logs: 8,
            keep: 0,
            purge_batch: 1,
            lag: 16,
            ..Params::default()
        };
        params.config().unwrap();
        let mut c = Cluster::new(params).await;
        c.write(unknown(180)).await;
        c.converge().await;
        let l = c.leader().await;
        let f = (l + 1) % 3;
        let follower_applied = c.node(f).raft.as_ref().unwrap()
            .metrics().borrow_watched().last_applied.unwrap();
        c.stop(f).await;
        for request in 181..221 {
            assert_eq!(c.write(unknown(request)).await.1,
                Some(Outcome::Rejected(Reject::UnknownCode)));
        }
        let l = c.leader().await;
        let purged = c.node(l).raft.as_ref().unwrap()
            .wait(Some(Duration::from_secs(5)))
            .metrics(|m| m.purged.is_some_and(|p| p.index > follower_applied.index),
                "leader purged beyond stopped follower")
            .await.unwrap().purged.unwrap();
        assert_eq!(c.router.snapshots_to.lock().unwrap().get(&(f as u64 + 1)), None);
        c.start(f).await; // Same persistent directory, mandatory ConsensusNode preflight.
        c.converge().await;
        let snapshots = *c.router.snapshots_to.lock().unwrap().get(&(f as u64 + 1))
            .expect("catch-up requires full_snapshot RPC to the restarted follower");
        assert!(snapshots >= 1);
        assert!(c.router.snapshot_max.load(Ordering::Relaxed) > 0);
        let (before, _) = c.node(c.leader().await).linearizable_state().await.unwrap();
        let (after, outcome) = c.write(unknown(221)).await;
        assert_eq!(after.index, before.unwrap().index + 1);
        assert_eq!(outcome, Some(Outcome::Rejected(Reject::UnknownCode)));
        c.converge().await;
        eprintln!("T3 follower={}, stopped_applied={}, leader_purged={}, snapshots_to_follower={}, final_applied={}",
            f + 1, follower_applied.index, purged.index, snapshots, after.index);
        c.finish().await;
        eprintln!("T3 elapsed={:?}", started.elapsed());
    }).await.unwrap();
}
impl Drop for Cluster {
    fn drop(&mut self) {
        self.router.nodes.lock().unwrap().clear();
    }
}
fn code(seed: u8) -> JoinCode {
    JoinCode::generate([1; 16], [2; 32], |b| {
        b.fill(if b.len() == 32 {
            seed.wrapping_add(128)
        } else {
            seed
        });
        Ok::<_, ()>(())
    })
    .unwrap()
}
fn mint(c: &JoinCode, r: u8, t: u64) -> Command {
    Command::Mint {
        cluster: [1; 16],
        actor: [3; 16],
        request: [r; 16],
        code: c.code_id(),
        verifier: c.verifier(),
        proposed_at_ms: t,
    }
}
fn redeem(c: &JoinCode, r: u8, k: u8, t: u64) -> Command {
    Command::redeem(
        [1; 16],
        [r; 16],
        c.code_id(),
        &c.proof(),
        KeyDigest::of_spki(&[k]).unwrap(),
        t,
    )
}
fn unknown(r: u8) -> Command {
    redeem(&code(100), r, 1, 1000)
}
fn grant(o: Option<Outcome>, n: u64, replay: bool) {
    match o {
        Some(Outcome::Enrolled(g)) => {
            assert_eq!(g.node().get(), n);
            assert_eq!(g.replay(), replay);
        }
        e => panic!("expected enrollment: {e:?}"),
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t1_follower_offline_enrollment_and_reopen() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let a = code(1);
        c.write(mint(&a, 1, 1000)).await;
        let cmd = redeem(&a, 2, 1, 1001);
        grant(c.write(cmd.clone()).await.1, 4, false);
        grant(c.write(cmd).await.1, 4, true);
        let l = c.leader().await;
        let f = (l + 1) % 3;
        c.stop(f).await;
        assert_eq!(
            c.write(redeem(&a, 3, 2, 1002)).await.1,
            Some(Outcome::Rejected(Reject::Consumed))
        );
        let b = code(2);
        c.write(mint(&b, 4, 1003)).await;
        let (x, y) = tokio::join!(
            c.write(redeem(&b, 5, 2, 1004)),
            c.write(redeem(&b, 6, 3, 1004))
        );
        let outcomes = [x.1, y.1];
        assert_eq!(
            outcomes
                .iter()
                .filter(|o| matches!(o, Some(Outcome::Enrolled(_))))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|o| **o == Some(Outcome::Rejected(Reject::Consumed)))
                .count(),
            1
        );
        grant(
            outcomes
                .into_iter()
                .find(|o| matches!(o, Some(Outcome::Enrolled(_))))
                .unwrap(),
            5,
            false,
        );
        let z = code(3);
        c.write(mint(&z, 7, 1005)).await;
        let wrong = Command::redeem(
            [1; 16],
            [8; 16],
            z.code_id(),
            &code(4).proof(),
            KeyDigest::of_spki(b"wrong").unwrap(),
            1006,
        );
        assert_eq!(
            c.write(wrong).await.1,
            Some(Outcome::Rejected(Reject::BadProof))
        );
        assert_eq!(
            c.write(redeem(&z, 9, 4, 1005 + CODE_TTL_MS)).await.1,
            Some(Outcome::Rejected(Reject::Expired))
        );
        c.start(f).await;
        c.converge().await;
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t2_leader_offline_reads_and_unknown_retry() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        let a = code(11);
        let mut ack = vec![c.write(mint(&a, 11, 1000)).await.0];
        ack.push(c.write(redeem(&a, 12, 1, 1001)).await.0);
        let old = c.leader().await;
        c.stop(old).await;
        let dir = c.dirs[old].path().to_owned();
        let last = *ack.last().unwrap();
        blocking(move || {
            let s = StateStore::open(
                &dir,
                Init {
                    cluster: [1; 16],
                    first_free: std::num::NonZeroU64::new(4).unwrap(),
                },
            )?;
            assert!(s.applied().unwrap().index >= last.index);
            Ok(())
        })
        .await
        .unwrap();
        let new = c.leader().await;
        ack.push(c.write(unknown(13)).await.0);
        let (p, _) = c.node(new).linearizable_state().await.unwrap();
        assert!(ack.iter().all(|a| p.unwrap().index >= a.index));
        c.start(old).await;
        c.converge().await;
        let follower = (new + 1) % 3;
        assert_eq!(
            c.node(follower).linearizable_state().await,
            Err(ReadError::NotLeader)
        );
        let b = code(12);
        c.write(mint(&b, 14, 1002)).await;
        let unacked = redeem(&b, 15, 2, 1003);
        for i in 0..3 {
            if i != new {
                c.stop(i).await;
            }
        }
        let result = timeout(Duration::from_secs(2), c.node(new).propose(unacked.clone())).await;
        assert!(matches!(
            result,
            Err(_) | Ok(Err(ProposeError::Unknown | ProposeError::NotLeader(_)))
        ));
        let read = timeout(Duration::from_secs(3), c.node(new).linearizable_state())
            .await
            .unwrap();
        assert!(matches!(
            read,
            Err(ReadError::NoQuorum | ReadError::NotLeader | ReadError::Unavailable)
        ));
        c.start((new + 1) % 3).await;
        let result = c.write(unacked.clone()).await;
        grant(
            result.1.clone(),
            5,
            matches!(result.1,Some(Outcome::Enrolled(ref g)) if g.replay()),
        );
        grant(c.write(unacked).await.1, 5, true);
        let z = code(13);
        c.write(mint(&z, 16, 1004)).await;
        grant(c.write(redeem(&z, 17, 3, 1005)).await.1, 6, false);
        c.start((new + 2) % 3).await;
        c.converge().await;
        c.finish().await;
    })
    .await
    .unwrap();
}

use openraft::type_config::async_runtime::watch::WatchReceiver;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn u6_cancelled_callers_keep_engine_permits_until_resolution() {
    timeout(Duration::from_secs(45), async {
        for restore in [false, true] {
            let mut c = Cluster::new(Params {
                election_min: 3000,
                election_max: 6000,
                ..Params::default()
            })
            .await;
            c.write(unknown(199)).await;
            c.converge().await;
            let l = c.leader().await;
            let base = c
                .node(l)
                .raft
                .as_ref()
                .unwrap()
                .metrics()
                .borrow_watched()
                .last_log_index
                .unwrap();
            for i in 0..3 {
                if i != l {
                    c.router.isolated.lock().unwrap().insert(i as u64 + 1);
                }
            }
            let mut callers = JoinSet::new();
            for r in 0..64 {
                let node = c.node(l).clone();
                callers.spawn(async move { node.propose(unknown(r)).await });
            }
            timeout(Duration::from_secs(2), async {
                loop {
                    let raft = c.node(l).raft.as_ref().unwrap();
                    let metrics = raft.metrics();
                    let m = metrics.borrow_watched();
                    let tail = m.last_log_index.unwrap();
                    let committed = m.local_committed.map(|p| p.index).unwrap_or(0);
                    if tail >= base + 64 && tail - committed >= 64 {
                        break;
                    }
                    drop(m);
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "64 uncommitted: {e:?}, base={base}, permits={}, metrics={:?}, callers={}",
                    c.node(l).permits.available_permits(),
                    c.node(l).raft.as_ref().unwrap().metrics().borrow_watched(),
                    callers.len()
                )
            });
            callers.abort_all();
            while callers.join_next().await.is_some() {}
            assert_eq!(c.node(l).permits.available_permits(), 0);
            assert_eq!(
                c.node(l).propose(unknown(200)).await,
                Err(ProposeError::Busy)
            );
            if restore {
                c.router.isolated.lock().unwrap().clear();
                let all_resolved = timeout(Duration::from_secs(8), async {
                    loop {
                        let n = c.node(l).permits.available_permits();
                        assert!(n <= 64);
                        if n == 64 {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .is_ok();
                let resolved = c.node(l).permits.available_permits();
                assert!(resolved <= 64);
                eprintln!(
                    "U6 routing restored: resolved={resolved}/64, all_within_window={all_resolved}; shutdown must join remaining owners"
                );
            }
            c.finish().await;
        }
    })
    .await
    .unwrap();
}

fn founders() -> Command {
    use crate::cluster::enrollment::{FoundMembers, NodeId};
    Command::Found {
        cluster: [1; 16],
        members: FoundMembers::new((1..=3).map(|n| {
            (
                NodeId::new(n).unwrap(),
                KeyDigest::of_spki(&[250, n as u8]).unwrap(),
            )
        }))
        .unwrap(),
        proposed_at_ms: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn t4_registry_revocation_snapshot_and_prune() {
    use crate::cluster::enrollment::{FoundMembers, NodeId, PeerRefusal, TOMBSTONE_MS};
    timeout(Duration::from_secs(45), async {
        let started = Instant::now();
        let mut c = Cluster::unfounded(Params {
            snapshot_logs: 8,
            keep: 0,
            purge_batch: 1,
            lag: 16,
            ..Params::default()
        })
        .await;
        let pair = |n| {
            (
                NodeId::new(n).unwrap(),
                KeyDigest::of_spki(&[250, n as u8]).unwrap(),
            )
        };
        let found = |pairs| Command::Found {
            cluster: [1; 16],
            members: FoundMembers::new(pairs).unwrap(),
            proposed_at_ms: 0,
        };
        let invalid = vec![
            vec![],
            (1..=17).map(pair).collect(),
            (1..=64).map(pair).collect(),
            vec![pair(2), pair(1)],
            vec![pair(1), (NodeId::new(2).unwrap(), pair(1).1)],
            vec![pair(4)],
        ];
        for pairs in &invalid {
            let l = c.leader().await;
            let sm = c.node(l).sm.as_ref().unwrap().clone();
            let before = blocking(move || Ok(sm.0.lock().unwrap().state().clone()))
                .await
                .unwrap();
            assert_eq!(
                c.write(found(pairs.clone())).await.1,
                Some(Outcome::Rejected(Reject::BadFounders))
            );
            let sm = c.node(c.leader().await).sm.as_ref().unwrap().clone();
            assert_eq!(
                blocking(move || Ok(sm.0.lock().unwrap().state().clone()))
                    .await
                    .unwrap(),
                before
            );
        }
        assert_eq!(
            c.write(founders()).await.1,
            Some(Outcome::Founded { replay: false })
        );
        for pairs in [
            vec![],
            (1..=17).map(pair).collect(),
            (1..=64).map(pair).collect(),
        ] {
            assert_eq!(
                c.write(found(pairs)).await.1,
                Some(Outcome::Rejected(Reject::BadFounders))
            );
        }
        assert_eq!(
            c.write(found(vec![pair(1)])).await.1,
            Some(Outcome::Founded { replay: true })
        );
        assert_eq!(
            c.write(found(vec![(
                pair(1).0,
                KeyDigest::of_spki(b"conflict").unwrap()
            )]))
            .await
            .1,
            Some(Outcome::Rejected(Reject::AlreadyFounded))
        );
        let code = code(1);
        c.write(mint(&code, 1, 1000)).await;
        let redeem = redeem(&code, 2, 1, 1001);
        grant(c.write(redeem.clone()).await.1, 4, false);
        c.converge().await;
        let k = KeyDigest::of_spki(&[1]).unwrap();
        let n = NodeId::new(4).unwrap();
        for i in 0..3 {
            assert_eq!(c.node(i).local_authorize([1; 16], n, k).await, Ok(()));
        }
        let l = c.leader().await;
        let f = (l + 1) % 3;
        let stopped = c
            .node(f)
            .raft
            .as_ref()
            .unwrap()
            .metrics()
            .borrow_watched()
            .last_applied
            .unwrap();
        c.stop(f).await;
        assert_eq!(
            c.write(Command::Remove {
                cluster: [1; 16],
                node: n,
                proposed_at_ms: 1002
            })
            .await
            .1,
            Some(Outcome::Removed { replay: false })
        );
        assert_eq!(
            c.write(redeem).await.1,
            Some(Outcome::Rejected(Reject::Revoked))
        );
        for r in 180..220 {
            assert_eq!(
                c.write(unknown(r)).await.1,
                Some(Outcome::Rejected(Reject::UnknownCode))
            );
        }
        let l = c.leader().await;
        let purged = c
            .node(l)
            .raft
            .as_ref()
            .unwrap()
            .wait(Some(Duration::from_secs(5)))
            .metrics(
                |m| m.purged.is_some_and(|p| p.index > stopped.index),
                "T4 purged beyond follower",
            )
            .await
            .unwrap()
            .purged
            .unwrap();
        assert_eq!(
            c.router.snapshots_to.lock().unwrap().get(&(f as u64 + 1)),
            None
        );
        c.start(f).await;
        c.converge().await;
        let snapshots = *c
            .router
            .snapshots_to
            .lock()
            .unwrap()
            .get(&(f as u64 + 1))
            .unwrap();
        assert!(snapshots >= 1);
        assert_eq!(
            c.node(f).local_authorize([1; 16], n, k).await,
            Err(PeerRefusal::NotMember)
        );
        for j in 1..=3 {
            assert_eq!(
                c.node(f)
                    .local_authorize([1; 16], pair(j).0, pair(j).1)
                    .await,
                Ok(())
            );
        }
        let prune = 1000 + CODE_TTL_MS + TOMBSTONE_MS;
        let mut expired = unknown(220);
        if let Command::Redeem { proposed_at_ms, .. } = &mut expired {
            *proposed_at_ms = prune;
        }
        assert_eq!(
            c.write(expired).await.1,
            Some(Outcome::Rejected(Reject::UnknownCode))
        );
        c.converge().await;
        for i in 0..3 {
            assert_eq!(
                c.node(i).local_authorize([1; 16], n, k).await,
                Err(PeerRefusal::NotMember)
            );
            for j in 1..=3 {
                assert_eq!(
                    c.node(i)
                        .local_authorize([1; 16], pair(j).0, pair(j).1)
                        .await,
                    Ok(())
                );
            }
            let sm = c.node(i).sm.as_ref().unwrap().clone();
            blocking(move || {
                let s = sm.0.lock().unwrap();
                let membership = decode(&s.membership().unwrap().1)?;
                assert!(registry_covers(&membership, s.state()));
                let mut state_bytes = Vec::new();
                crate::cluster::enrollment::codec::encode(s.state(), &mut state_bytes);
                assert_eq!(&state_bytes[32..34], &[0, 0]); // All records pruned.
                Ok(())
            })
            .await
            .unwrap();
        }
        // Every store reopens; convergence compares complete snapshot bytes.
        for i in 0..3 {
            c.stop(i).await;
        }
        for i in 0..3 {
            c.start(i).await;
        }
        c.converge().await;
        eprintln!(
            "T4 follower={} stopped={} purged={} snapshots={} elapsed={:?}",
            f + 1,
            stopped.index,
            purged.index,
            snapshots,
            started.elapsed()
        );
        c.finish().await;
    })
    .await
    .unwrap();
}

// A0 uses the actual engine: apply is paused before acquiring the store mutex.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r1_apply_wait_deadline() {
    timeout(Duration::from_secs(45), async {
        let _pause_owner = APPLY_PAUSE_OWNER.lock().await;
        let mut c = Cluster::new(Params::default()).await;
        c.converge().await;
        let l = c.leader().await;
        let (entered, wait) = oneshot::channel();
        let (release, resume) = std::sync::mpsc::channel();
        *adapter::APPLY_PAUSE.lock().unwrap() = Some(adapter::ApplyPause {
            store: Arc::as_ptr(&c.node(l).sm.as_ref().unwrap().0) as usize,
            entered,
            release: resume,
        });
        let n = c.node(l).clone();
        let write = tokio::spawn(async move { n.propose(unknown(233)).await });
        timeout(Duration::from_secs(2), wait)
            .await
            .unwrap()
            .unwrap();
        let committed = c
            .node(l)
            .raft
            .as_ref()
            .unwrap()
            .metrics()
            .borrow_watched()
            .last_log_index
            .unwrap();
        for i in 0..3 {
            if i != l {
                c.node(i)
                    .raft
                    .as_ref()
                    .unwrap()
                    .wait(Some(Duration::from_secs(2)))
                    .applied_index_at_least(
                        Some(committed),
                        "followers commit while leader apply paused",
                    )
                    .await
                    .unwrap();
            }
        }
        let started = Instant::now();
        // Keep the old four-second hang detector for controlled pre-fix reproduction.
        let result = timeout(Duration::from_secs(4), c.node(l).linearizable_state()).await;
        let elapsed = started.elapsed();
        release.send(()).unwrap();
        write.await.unwrap().unwrap();
        assert_eq!(result.unwrap(), Err(ReadError::Unavailable));
        assert!(elapsed <= Duration::from_millis(2500));
        eprintln!("R1 apply caller elapsed={elapsed:?}");
        c.converge().await;
        c.finish().await;
    })
    .await
    .unwrap();
}

// RAII release also makes failed assertions safe: no Tokio worker holds this lock.
struct StoreGate {
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl StoreGate {
    async fn hold(sm: SmAdapter) -> Self {
        let (release, wait) = std::sync::mpsc::channel();
        let (entered, ready) = oneshot::channel();
        let thread = std::thread::spawn(move || {
            let guard = sm.0.lock().unwrap();
            let _ = entered.send(());
            let _ = wait.recv();
            drop(guard);
            drop(sm);
        });
        ready.await.unwrap();
        Self {
            release: Some(release),
            thread: Some(thread),
        }
    }
    fn open(mut self) {
        self.release.take().unwrap().send(()).unwrap();
        self.thread.take().unwrap().join().unwrap();
    }
}
impl Drop for StoreGate {
    fn drop(&mut self) {
        self.release.take();
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
async fn read_units_return(n: &ConsensusNode, budget: Duration) {
    timeout(budget, async {
        loop {
            let done = {
                let mut owners = n.owners.lock().unwrap();
                while owners.set.try_join_next().is_some() {}
                n.read_permits.available_permits() == READ_PERMITS && owners.set.is_empty()
            };
            if done {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r2_isolated_read_refusal() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        c.converge().await;
        let l = c.leader().await;
        assert_eq!(c.node(l).id, l as u64 + 1);
        c.router.isolated.lock().unwrap().insert(l as u64 + 1);
        let started = Instant::now();
        let result = c.node(l).linearizable_state().await;
        let elapsed = started.elapsed();
        timeout_at(
            started + Duration::from_millis(2500),
            read_units_return(c.node(l), Duration::from_millis(2500)),
        )
        .await
        .unwrap();
        eprintln!(
            "R2 refusal={result:?}, caller={elapsed:?}, units returned by={:?}",
            started.elapsed()
        );
        c.router.isolated.lock().unwrap().clear();
        c.finish().await;
        assert!(match result {
            Err(ReadError::NotLeader | ReadError::NoQuorum) => true,
            Err(ReadError::Unavailable) => elapsed >= READ_BUDGET,
            _ => false,
        });
        assert!(elapsed <= Duration::from_millis(2500));
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r3_store_wait_deadline_and_heartbeat() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        c.converge().await;
        let l = c.leader().await;
        let gate = StoreGate::hold(c.node(l).sm.as_ref().unwrap().clone()).await;
        let ticks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = ticks.clone();
        let heartbeat = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(10));
            loop {
                interval.tick().await;
                counter.fetch_add(1, Ordering::Relaxed);
            }
        });
        let started = Instant::now();
        let result = timeout(Duration::from_secs(4), c.node(l).linearizable_state()).await;
        let elapsed = started.elapsed();
        let count = ticks.load(Ordering::Relaxed);
        heartbeat.abort();
        assert_eq!(result.unwrap(), Err(ReadError::Unavailable));
        assert!(elapsed <= Duration::from_millis(2500));
        assert!(count >= 50, "Tokio heartbeat ticks={count}");
        assert_eq!(c.node(l).read_permits.available_permits(), READ_PERMITS - 1);
        gate.open();
        read_units_return(c.node(l), Duration::from_secs(1)).await;
        c.write(unknown(234)).await;
        c.converge().await;
        eprintln!("R3 caller elapsed={elapsed:?}, heartbeat ticks={count}");
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r4_read_admission_and_caller_abort() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        c.converge().await;
        let l = c.leader().await;
        let gate = StoreGate::hold(c.node(l).sm.as_ref().unwrap().clone()).await;
        let baseline = c.node(l).read_blocking.load(Ordering::SeqCst);
        let mut callers = Vec::new();
        for _ in 0..READ_PERMITS {
            let n = c.node(l).clone();
            callers.push(tokio::spawn(async move { n.linearizable_state().await }));
        }
        timeout(Duration::from_secs(1), async {
            while c.node(l).read_permits.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        // Observe every actual blocking closure before aborting a caller.
        timeout(Duration::from_secs(1), async {
            while c.node(l).read_blocking.load(Ordering::SeqCst) < baseline + READ_PERMITS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let aborted = callers.remove(0);
        aborted.abort();
        assert!(aborted.await.unwrap_err().is_cancelled());
        let started = Instant::now();
        assert_eq!(c.node(l).linearizable_state().await, Err(ReadError::Busy));
        assert!(started.elapsed() <= Duration::from_millis(50));
        assert_eq!(c.node(l).permits.available_permits(), 64);
        for caller in callers {
            assert_eq!(caller.await.unwrap(), Err(ReadError::Unavailable));
        }
        assert_eq!(c.node(l).read_permits.available_permits(), 0);
        assert_eq!(c.node(l).permits.available_permits(), 64);
        gate.open();
        read_units_return(c.node(l), Duration::from_secs(2)).await;
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r5_shutdown_read_owners() {
    timeout(Duration::from_secs(45), async {
        for gated in [false, true] {
            let mut c = Cluster::new(Params::default()).await;
            c.converge().await;
            let l = c.leader().await;
            let gate = if gated {
                Some(StoreGate::hold(c.node(l).sm.as_ref().unwrap().clone()).await)
            } else {
                c.router.isolated.lock().unwrap().insert(l as u64 + 1);
                None
            };
            let n = c.node(l).clone();
            let caller = tokio::spawn(async move { n.linearizable_state().await });
            timeout(Duration::from_secs(1), async {
                while c.node(l).read_permits.available_permits() == READ_PERMITS {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            if gated {
                assert_eq!(caller.await.unwrap(), Err(ReadError::Unavailable));
                assert_eq!(c.node(l).read_permits.available_permits(), READ_PERMITS - 1);
            } else {
                caller.abort();
                let _ = caller.await;
            }
            c.router.nodes.lock().unwrap().remove(&(l as u64 + 1));
            let mut n = Arc::try_unwrap(c.nodes[l].take().unwrap()).ok().unwrap();
            let started = Instant::now();
            let result = n.shutdown().await;
            assert!(started.elapsed() <= Duration::from_millis(5500));
            if gated {
                let Err(Orphaned(r)) = &result else {
                    panic!("gated read must fail shutdown: {result:?}")
                };
                assert_eq!(r.first, Phase::Owners);
                assert_eq!(r.engine, EngineEnd::Joined);
                assert_eq!(r.read_held, 1);
                assert_eq!(r.propose_held, 0);
                assert_eq!(r.owners_left, 1);
                assert!(r.sm_strong >= 1);
                assert_eq!(r.log_strong, 0);
            } else {
                assert_eq!(result, Ok(()));
            }
            eprintln!(
                "R5 gated={gated}, shutdown={:?}, elapsed={:?}",
                result,
                started.elapsed()
            );
            if let Some(gate) = gate {
                gate.open();
            }
            timeout(Duration::from_secs(2), async {
                while n.sm_weak.upgrade().is_some()
                    || n.log_weak.upgrade().is_some()
                    || n.read_permits.available_permits() != READ_PERMITS
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(n.owners.lock().unwrap().set.is_empty());
            assert_eq!(n.linearizable_state().await, Err(ReadError::Unavailable));
            c.finish().await;
        }
    })
    .await
    .unwrap();
}

// These are attribution tests, not acceptance of Orphaned as healthy shutdown.
async fn report_cleanup(n: &ConsensusNode) {
    timeout(Duration::from_secs(2), async {
        while n.log_weak.strong_count() != 0
            || n.sm_weak.strong_count() != 0
            || n.permits.available_permits() != 64
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual stores and permits released after gate release");
}
fn report_take_node(c: &mut Cluster, i: usize) -> ConsensusNode {
    c.router.isolated.lock().unwrap().insert(i as u64 + 1);
    c.router.nodes.lock().unwrap().remove(&(i as u64 + 1));
    Arc::try_unwrap(c.nodes[i].take().unwrap())
        .ok()
        .expect("caller joined and node Arc dropped")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r6_shutdown_report_cancelled_proposal() {
    timeout(Duration::from_secs(45), async {
        let _pause_owner = APPLY_PAUSE_OWNER.lock().await;
        let mut c = Cluster::new(Params::default()).await;
        c.converge().await;
        let l = c.leader().await;
        let (entered_tx, entered) = oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        *adapter::APPLY_PAUSE.lock().unwrap() = Some(adapter::ApplyPause {
            store: Arc::as_ptr(&c.node(l).sm.as_ref().unwrap().0) as usize,
            entered: entered_tx,
            release: gate,
        });
        let node = c.node(l).clone();
        let caller = tokio::spawn(async move { node.propose(unknown(240)).await });
        timeout(Duration::from_secs(2), entered)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(c.node(l).permits.available_permits(), 63);
        caller.abort();
        assert!(
            timeout(Duration::from_secs(2), caller)
                .await
                .unwrap()
                .unwrap_err()
                .is_cancelled()
        );
        assert_eq!(c.node(l).permits.available_permits(), 63);
        {
            let mut owners = c.node(l).owners.lock().unwrap();
            assert_eq!(owners.set.len(), 1);
            assert!(
                owners.set.try_join_next().is_none(),
                "owner still pending after caller cancellation"
            );
            // The apply gate prevents resolution after cancellation.
        }
        let mut n = report_take_node(&mut c, l);
        let start = Instant::now();
        let result = timeout(Duration::from_millis(5500), n.shutdown())
            .await
            .unwrap();
        let elapsed = start.elapsed();
        // Always unblock the real worker before any report assertion can panic.
        release.send(()).unwrap();
        report_cleanup(&n).await;
        let Err(Orphaned(r)) = result else {
            panic!("paused apply must fail shutdown: {result:?}")
        };
        eprintln!("R6 proposal: {r:?}, elapsed={elapsed:?}");
        assert!(elapsed <= Duration::from_millis(5500));
        assert_eq!(r.first, Phase::Owners);
        assert_eq!(r.engine, EngineEnd::Joined);
        assert_eq!(r.owners_waited, 1);
        assert_eq!(r.owners_left, 1);
        assert_eq!(r.propose_held, 1);
        assert_eq!(r.read_held, 0);
        assert_eq!(r.owners_panicked, 0);
        assert!(r.sm_strong >= 1);
        assert!((5000..=5500).contains(&r.total_ms));
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r6_shutdown_report_store_only() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        c.converge().await;
        let l = c.leader().await;
        let held = c.node(l).sm.as_ref().unwrap().clone();
        let mut n = report_take_node(&mut c, l);
        let result = timeout(Duration::from_millis(5500), n.shutdown())
            .await
            .unwrap();
        drop(held);
        report_cleanup(&n).await;
        let Err(Orphaned(r)) = result else {
            panic!("held store must fail shutdown: {result:?}")
        };
        eprintln!("R6 store: {r:?}");
        assert_eq!(r.first, Phase::StoreRelease);
        assert_eq!(r.engine, EngineEnd::Joined);
        assert_eq!(r.owners_waited, 0);
        assert_eq!(r.owners_left, 0);
        assert_eq!(r.propose_held, 0);
        assert_eq!(r.read_held, 0);
        assert_eq!(r.log_strong, 0);
        assert_eq!(r.sm_strong, 1);
        c.finish().await;
    })
    .await
    .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn r6_shutdown_report_normal_and_reaped_panic() {
    timeout(Duration::from_secs(45), async {
        let mut c = Cluster::new(Params::default()).await;
        c.converge().await;
        // Normal shutdown remains Ok and releases actual stores/permits.
        let l = c.leader().await;
        c.stop(l).await;
        let i = (l + 1) % 3;
        let mut n = report_take_node(&mut c, i);
        let task = n
            .owners
            .lock()
            .unwrap()
            .set
            .spawn(async { panic!("test owner panic") });
        timeout(Duration::from_secs(2), async {
            loop {
                if task.is_finished() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let Err(Orphaned(r)) = n.shutdown().await else {
            panic!("reaped panic must fail shutdown")
        };
        eprintln!("R6 reaped panic: {r:?}");
        assert_eq!(r.first, Phase::Owners);
        assert_eq!(r.engine, EngineEnd::Joined);
        assert_eq!(r.owners_done_at_start, 1);
        assert_eq!(r.owners_panicked, 1);
        assert_eq!(r.owners_waited, 0);
        assert_eq!(r.owners_left, 0);
        report_cleanup(&n).await;
        c.finish().await;
    })
    .await
    .unwrap();
}

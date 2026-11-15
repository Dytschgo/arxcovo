use super::mem_net::{Factory, MemRouter};
use super::*;
use crate::cluster::enrollment::{CODE_TTL_MS, JoinCode, KeyDigest, Reject};
use crate::service::application::tests::private_temp;
use std::sync::atomic::Ordering;
use tokio::time::timeout;
struct Cluster {
    dirs: Vec<tempfile::TempDir>,
    nodes: Vec<Option<Arc<ConsensusNode>>>,
    router: Arc<MemRouter>,
    params: Params,
    acknowledged: Mutex<Vec<LogPos>>,
}
impl Cluster {
    async fn new(params: Params) -> Self {
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
            Err(ReadError::NoQuorum | ReadError::NotLeader)
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

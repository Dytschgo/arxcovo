use super::*;
use crate::cluster::{
    enrollment::{
        durable_join::{self, Destination, RetainedJoin},
        transfer::{self, LocalRoute},
    },
    pki::ca::fingerprint_of,
};
use learner::{HostedCatchUp, Run};
use openraft::{
    network::{RPCOption, RaftNetworkFactory, RaftNetworkV2},
    raft::AppendEntriesRequest,
    type_config::async_runtime::watch::WatchReceiver,
};
use peer_registry::{Lane, Phase};
use peer_transport::{Event, PeerFactory};

type Result<T> = std::result::Result<T, &'static str>;
const OBSERVE: Duration = Duration::from_secs(10);
const CLEANUP: Duration = Duration::from_secs(12);
const CLUSTER: [u8; 16] = [1; 16];

fn private_ip() -> Result<std::net::IpAddr> {
    let route = UdpSocket::bind("0.0.0.0:0").map_err(|_| "route bind")?;
    route.connect("192.0.2.1:9").map_err(|_| "route lookup")?;
    let addr = route.local_addr().map_err(|_| "local route")?;
    if !matches!(addr.ip(), std::net::IpAddr::V4(ip) if ip.is_private()) {
        return Err("fixture needs a private non-loopback route");
    }
    Ok(addr.ip())
}

fn plan(layout: &Layout, mode: &str, ip: std::net::IpAddr) -> Result<(Plan, [SocketAddr; 2])> {
    let peer = StdListener::bind(SocketAddr::new(ip, 0)).map_err(|_| "reserve peer")?;
    let enroll = StdListener::bind(SocketAddr::new(ip, 0)).map_err(|_| "reserve enrollment")?;
    let peer = peer.local_addr().map_err(|_| "peer address")?;
    let enroll = enroll.local_addr().map_err(|_| "enrollment address")?;
    let mut plan = layout.plan(mode);
    plan.peer = peer.to_string();
    plan.enrollment = enroll.to_string();
    peer_transport::endpoint(&plan.peer).map_err(|_| "qualify peer")?;
    peer_transport::endpoint(&plan.enrollment).map_err(|_| "qualify enrollment")?;
    Ok((plan, [peer, enroll]))
}

fn custody(layout: &Layout) -> Result<[std::path::PathBuf; 2]> {
    let paths = ["wrap", "custody"].map(|name| layout.root.path().join(name));
    for path in &paths {
        fs::create_dir(path).map_err(|_| "create private fixture directory")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "qualify private fixture directory")?;
    }
    Ok(paths)
}

async fn ready(job: &Job, observer: &mut Observer, desired: Status) -> Result<()> {
    timeout(OBSERVE, async {
        loop {
            let status = observer.status();
            if status == desired {
                return Ok(());
            }
            if !matches!(
                status,
                Status::Preparing
                    | Status::Prepared
                    | Status::EngineMayExist
                    | Status::Running
                    | Status::Founding,
            ) {
                let state = job.state();
                eprintln!(
                    "C9 startup phase={status:?} refusal={:?} frozen-founder={:?}",
                    state.refusal, state.founder_failure
                );
                return Err("retained startup refused");
            }
            observer
                .changed()
                .await
                .ok_or("startup observation closed")?;
        }
    })
    .await
    .map_err(|_| "retained startup observation elapsed")?
}

async fn mint_and_join(
    issuer: &Job,
    layout: &Layout,
    endpoint: SocketAddr,
    root: &[u8],
    observed: &Arc<Observation>,
    sequence: u8,
) -> Result<(RetainedJoin, SocketAddr)> {
    let (node, ready_issuer) = {
        let state = issuer.state();
        (
            state.node.as_ref().ok_or("issuer node missing")?.clone(),
            state.issuer.as_ref().ok_or("issuer not ready")?.clone(),
        )
    };
    let (code, voters) = {
        let node = node.lock().await;
        let voters = {
            let store = node
                .sm
                .as_ref()
                .ok_or("issuer store missing")?
                .0
                .lock()
                .map_err(|_| "issuer store poisoned")?;
            let membership = WireMembership::decode(
                store
                    .membership()
                    .ok_or("issuer membership missing")?
                    .1
                    .bytes(),
            )
            .map_err(|_| "issuer membership invalid")?;
            BoundVoters::observe(store.state(), &membership).map_err(|_| "issuer binding absent")?
        };
        let code = JoinCode::generate(
            CLUSTER,
            fingerprint_of(root),
            voters.anchor(CLUSTER),
            getrandom::fill,
        )
        .map_err(|_| "fixture code generation")?;
        let minted = node
            .propose(Command::Mint {
                cluster: CLUSTER,
                actor: [3; 16],
                request: [sequence; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: ready_issuer
                    .clock
                    .sample_ms()
                    .map_err(|_| "issuer clock refused")?,
            })
            .await
            .map_err(|error| {
                eprintln!("C9 frozen Mint sequence={sequence} refused: {error:?}");
                "Mint refused"
            })?;
        if !matches!(minted.1, Some(Outcome::Minted { replay: false, .. })) {
            return Err("Mint outcome refused");
        }
        (code, voters)
    };
    if sequence == 1 {
        issuer
            .start_enrollment(Some(observed.clone()))
            .map_err(|_| "issuer ingress refused")?;
    }
    let (plan, [address, _]) = plan(layout, "join", endpoint.ip())?;
    let [wrap, staging] = custody(layout)?;
    let paste = transfer::encode(endpoint, root, &code).map_err(|_| "paste encoding")?;
    let (join, mut observer) = durable_join::start_retained(
        Destination {
            plan,
            wrap,
            staging,
        },
        paste.expose_for_owner_transfer(),
        &[LocalRoute {
            network: endpoint.ip(),
            prefix: 32,
        }],
        ready_issuer.tls.time.clone(),
    )
    .map_err(|_| "join launch refused")?;
    timeout(OBSERVE, async {
        tokio::select! {
            phase = observer.terminal() => {
                eprintln!("C9 early join phase={phase:?}");
                return Err("join ended before actual issuer Redeem completion");
            }
            () = observed.finished.notified() => {}
        }
        let result = *observed.result.lock().unwrap();
        observed.release.notify_one();
        if result != Some(Ok(())) {
            eprintln!("C9 frozen Redeem sequence={sequence} refused: {result:?}");
            return Err("issuer Redeem refused");
        }
        let anchors = observed.anchor_outcomes.lock().unwrap();
        let count = usize::from(sequence);
        if observed.anchor_calls.load(Relaxed) != count
            || observed.redeem_calls.load(Relaxed) != count
            || anchors.len() != count
            || anchors[count - 1] != Ok(voters.encode())
        {
            return Err("unexpected AnchorQuery/Redeem observations");
        }
        Ok(())
    })
    .await
    .map_err(|_| "issuer exchange elapsed")??;
    if timeout(OBSERVE, observer.terminal())
        .await
        .map_err(|_| "publication elapsed")?
        != durable_join::Status::Published
    {
        return Err("retained publication refused");
    }
    Ok((join, address))
}

fn stored(node: &ConsensusNode, end: u64) -> Result<Vec<(LogPos, StoredEntry)>> {
    node.log_weak
        .upgrade()
        .ok_or("retained log missing")?
        .lock()
        .map_err(|_| "retained log unavailable")?
        .entries(0, end, 16)
        .map_err(|_| "bounded log read refused")
}

async fn retained_node(job: &Job) -> Result<Arc<tokio::sync::Mutex<ConsensusNode>>> {
    job.state().node.clone().ok_or("retained node missing")
}

async fn engine(job: &Job) -> Result<Engine> {
    retained_node(job)
        .await?
        .lock()
        .await
        .raft
        .clone()
        .ok_or("Engine unavailable")
}

async fn dispatch(events: &mut tokio::sync::mpsc::Receiver<Event>, wanted: Lane) -> Result<()> {
    timeout(OBSERVE, async {
        while let Some(event) = events.recv().await {
            match event {
                Event::LearnerDispatch(lane) if lane == wanted => return Ok(()),
                Event::LearnerDispatch(_) => return Err("wrong final dispatch lane"),
                Event::Finished(Some(reason)) | Event::Refused(reason) => {
                    eprintln!("C9 peer dispatch refused: {reason:?}");
                    return Err("peer dispatch refused");
                }
                _ => {}
            }
        }
        Err("dispatch observation closed")
    })
    .await
    .map_err(|_| "dispatch observation elapsed")?
}

async fn append(
    factory: &mut PeerFactory,
    run: &Run,
    address: SocketAddr,
    request: AppendEntriesRequest<Arx>,
    lane: Lane,
) -> Result<()> {
    let mut observed = run.dispatch_observer().await;
    let mut client = factory
        .new_client(run.binding().0.get(), &BasicNode::new(address.to_string()))
        .await;
    let last = request.leader_commit;
    let reply = client
        .append_entries(request, RPCOption::new(OBSERVE))
        .await;
    dispatch(&mut observed, lane).await?;
    if !matches!(reply, Ok(Ok(id)) if id == last) {
        eprintln!("C9 Append reply refused: {reply:?}");
        return Err("Append reply refused");
    }
    Ok(())
}

async fn applied(
    run: &Run,
    expected: LogPos,
    phase: Phase,
) -> Result<Option<(LogPos, MembershipBlob)>> {
    let raft = engine(run.job).await?;
    let mut progress = raft.watch_apply_progress();
    timeout(OBSERVE, progress.wait_until_ge(&Some(id(expected))))
        .await
        .map_err(|_| "durable apply observation elapsed")?
        .map_err(|_| "apply observation closed")?;
    drop(raft);
    if run.phase() != Some(phase) {
        return Err("applied phase mismatch");
    }
    let node = retained_node(run.job).await?;
    let node = node.lock().await;
    let store = node
        .sm
        .as_ref()
        .ok_or("learner store unavailable")?
        .0
        .lock()
        .map_err(|_| "learner store mutex unavailable")?;
    let (id, key) = run.binding();
    if store.applied() != Some(expected)
        || (phase == Phase::Strict && store.state().authorize_peer(CLUSTER, id, key).is_err())
        || (phase == Phase::Bootstrap && store.state().founded())
    {
        return Err("durable learner image mismatch");
    }
    Ok(store.membership().cloned())
}

fn check_locks(path: &std::path::Path, held: bool) -> Result<()> {
    let custody = path
        .parent()
        .ok_or("fixture root missing")?
        .join("custody/custody-lock");
    for lock_path in [
        path.join("cluster-owner.lock"),
        path.join("cluster-state.lock"),
        path.join("cluster-log.lock"),
        custody,
    ] {
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "lock proof open")?;
        if lock.try_lock().is_err() != held {
            return Err("outer or store lock lifetime mismatch");
        }
    }
    Ok(())
}

async fn close_run(run: Run, reopen: bool) -> Result<Option<learner::Reopen>> {
    let job = run.job;
    let path = run.state_path();
    let held = if run.observer.status() == Status::LearnerReady {
        path.as_deref()
            .ok_or("learner path missing")
            .and_then(|path| check_locks(path, true))
    } else {
        Ok(())
    };
    let closed = timeout(CLEANUP, run.close(reopen))
        .await
        .map_err(|_| "learner close elapsed")?;
    if let Err(reason) = closed {
        let state = job.state();
        eprintln!(
            "C9 learner close refused: {reason:?} at {:?}, shutdown {:?}",
            state.status, state.close_failure
        );
        return Err("learner close refused");
    }
    held?;
    if let Some(path) = path.as_deref() {
        check_locks(path, false)?;
    }
    closed.map_err(|_| "learner close refused")
}

#[derive(Default)]
struct Learners {
    one: Option<Run>,
    two: Option<Run>,
    recovered: Option<Run>,
}

async fn campaign(
    issuer: &Job,
    issuer_observer: &mut Observer,
    layouts: [&Layout; 2],
    endpoint: SocketAddr,
    observed: &Arc<Observation>,
    learners: &mut Learners,
) -> Result<()> {
    ready(issuer, issuer_observer, Status::FounderReady).await?;
    let root = issuer
        .state()
        .owner
        .as_ref()
        .ok_or("issuer owner missing")?
        .read("ca.der", 4096)
        .map_err(|_| "issuer CA unavailable")?;
    let transport = {
        let state = issuer.state();
        state
            .founder
            .as_ref()
            .ok_or("founder transport missing")?
            .transport_for_test()
            .try_lock()
            .map_err(|_| "issuer transport busy")?
            .factory_for_test()
    };
    let mut factory = transport;
    let (one, address) = mint_and_join(issuer, layouts[0], endpoint, &root, observed, 1).await?;
    learners.one = Some(
        learner::start_published(one, Params::default(), Some(HostedCatchUp(())))
            .map_err(|_| "consume first publication")?,
    );
    let one = learners.one.as_mut().unwrap();
    ready(one.job, &mut one.observer, Status::LearnerReady).await?;
    if one.phase() != Some(Phase::Bootstrap) {
        return Err("initial learner was not virgin");
    }
    let e1 = one.original();
    let (vote, rows) = {
        let issuer_node = retained_node(issuer).await?;
        let node = issuer_node.lock().await;
        let vote = node
            .raft
            .as_ref()
            .ok_or("issuer Engine missing")?
            .metrics()
            .borrow_watched()
            .vote;
        let rows = stored(&node, e1.index + 1)?;
        if rows.first().is_none_or(|(p, _)| p.index != 0)
            || rows.last().is_none_or(|(p, _)| *p != e1)
            || !vote.committed
            || vote.leader_id.node_id != FOUNDER.get()
        {
            return Err("invalid bounded issuer history");
        }
        (vote, rows)
    };
    let split = rows
        .iter()
        .position(|(_, e)| matches!(e, StoredEntry::Enrollment(Command::Found { .. })))
        .filter(|split| *split > 0)
        .ok_or("Found split missing")?;
    let prefix = rows[split - 1].0;
    let (before, through) = rows.split_at(split);
    let before = before
        .iter()
        .cloned()
        .map(from_stored)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| "encode prefix")?;
    let through = through
        .iter()
        .cloned()
        .map(from_stored)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| "encode crossing entries")?;
    append(
        &mut factory,
        one,
        address,
        AppendEntriesRequest {
            vote,
            prev_log_id: None,
            leader_commit: Some(id(prefix)),
            entries: before,
        },
        Lane::Receipt,
    )
    .await?;
    let founding_membership = applied(one, prefix, Phase::Bootstrap)
        .await?
        .ok_or("applied founder membership missing")?;
    {
        let key = issuer.state().issuer.as_ref().ok_or("issuer missing")?.key;
        let learner_node = retained_node(one.job).await?;
        let node = learner_node.lock().await;
        let store = node
            .sm
            .as_ref()
            .ok_or("learner store missing")?
            .0
            .lock()
            .map_err(|_| "learner store unavailable")?;
        if store.state().authorize_peer(CLUSTER, FOUNDER, key)
            != Err(crate::cluster::enrollment::PeerRefusal::NotMember)
        {
            return Err("prefix installed an issuer binding");
        }
    }
    append(
        &mut factory,
        one,
        address,
        AppendEntriesRequest {
            vote,
            prev_log_id: Some(id(prefix)),
            leader_commit: Some(id(e1)),
            entries: through,
        },
        Lane::Receipt,
    )
    .await?;
    if applied(one, e1, Phase::Strict).await? != Some(founding_membership.clone()) {
        return Err("E1 membership changed");
    }
    eprintln!("C9 Append replies and durable apply checked at {prefix:?} and {e1:?}");

    let (two, address2) = mint_and_join(issuer, layouts[1], endpoint, &root, observed, 2).await?;
    learners.two = Some(
        learner::start_published(two, Params::default(), Some(HostedCatchUp(())))
            .map_err(|_| "consume second publication")?,
    );
    let two = learners.two.as_mut().unwrap();
    ready(two.job, &mut two.observer, Status::LearnerReady).await?;
    let e2 = two.original();
    if two.phase() != Some(Phase::Bootstrap) || e2.index <= e1.index {
        return Err("second learner not virgin or E2 not distinct");
    }
    let raft = engine(issuer).await?;
    let mut applied_progress = raft.watch_apply_progress();
    timeout(OBSERVE, applied_progress.wait_until_ge(&Some(id(e2))))
        .await
        .map_err(|_| "issuer E2 apply elapsed")?
        .map_err(|_| "issuer apply closed")?;
    {
        let issuer_node = retained_node(issuer).await?;
        let node = issuer_node.lock().await;
        let history = stored(&node, e2.index + 1)?;
        let mut membership = 0;
        let mut found = 0;
        let mut minted = 0;
        let mut redeemed = Vec::new();
        for (pos, entry) in history {
            match entry {
                StoredEntry::Membership(_) if pos.index == 0 => membership += 1,
                StoredEntry::Membership(_) => {
                    return Err("new membership row after Found initialization");
                }
                StoredEntry::Enrollment(Command::Found { .. }) => found += 1,
                StoredEntry::Enrollment(Command::Mint { .. }) => minted += 1,
                StoredEntry::Enrollment(Command::Redeem { .. }) => redeemed.push(pos),
                _ => {}
            }
        }
        if (membership, found, minted) != (1, 1, 2) || redeemed != [e1, e2] {
            return Err("issuer invocation inventory mismatch");
        }
    }
    let mut snapshots = raft.watch_snapshot_progress();
    raft.trigger()
        .snapshot()
        .await
        .map_err(|_| "issuer snapshot trigger refused")?;
    let built = timeout(OBSERVE, snapshots.wait_until_ge(&Some(id(e2))))
        .await
        .map_err(|_| "issuer snapshot observation elapsed")?
        .map_err(|_| "issuer snapshot closed")?;
    let metrics = raft.metrics().borrow_watched().clone();
    let snapshot = raft
        .get_snapshot()
        .await
        .map_err(|_| "issuer snapshot image refused")?
        .ok_or("issuer snapshot image missing")?;
    let (snap_pos, snap_members) = StateStore::inspect_snapshot(&snapshot.snapshot, CLUSTER)
        .map_err(|_| "issuer image validation refused")?;
    let snap_pos = snap_pos.ok_or("issuer snapshot position missing")?;
    if snapshot.meta.last_log_id != built
        || built != Some(id(snap_pos))
        || snap_pos != e2
        || snap_members != Some(founding_membership.clone())
        || metrics.vote != vote
    {
        return Err("issuer snapshot differed from observed build");
    }
    drop(raft);
    let mut snapshot_dispatch = two.dispatch_observer().await;
    let mut client = factory
        .new_snapshot_client(two.binding().0.get(), &BasicNode::new(address2.to_string()))
        .await;
    let reply = RaftNetworkV2::full_snapshot(
        &mut client,
        vote,
        snapshot,
        std::future::pending(),
        RPCOption::new(OBSERVE),
    )
    .await;
    dispatch(&mut snapshot_dispatch, Lane::Receipt).await?;
    if !matches!(&reply, Ok(reply) if reply.vote == vote) {
        eprintln!("C9 full snapshot reply refused: {reply:?}");
        return Err("full snapshot reply refused");
    }
    if applied(two, snap_pos, Phase::Strict).await? != Some(founding_membership) {
        return Err("installed membership mismatch");
    }
    eprintln!("C9 snapshot reply and durable install checked at {snap_pos:?}");

    let expectation = close_run(learners.one.take().unwrap(), true)
        .await?
        .ok_or("clean post-E close didn't yield expectation")?;
    learners.recovered = Some(learner::recover(
        expectation,
        Params::default(),
        Some(HostedCatchUp(())),
    ));
    let recovered = learners.recovered.as_mut().unwrap();
    ready(recovered.job, &mut recovered.observer, Status::LearnerReady).await?;
    if recovered.phase() != Some(Phase::Strict) || recovered.has_receipt() {
        return Err("reopened learner not strict");
    }
    let rows_before = {
        let node = retained_node(recovered.job).await?;
        let node = node.lock().await;
        stored(&node, i64::MAX as u64 + 1)?
            .into_iter()
            .map(|(pos, _)| pos)
            .collect::<Vec<_>>()
    };
    append(
        &mut factory,
        recovered,
        address,
        AppendEntriesRequest {
            vote,
            prev_log_id: Some(id(e1)),
            leader_commit: Some(id(e1)),
            entries: Vec::new(),
        },
        Lane::Strict,
    )
    .await?;
    let rows_after = {
        let node = retained_node(recovered.job).await?;
        let node = node.lock().await;
        stored(&node, i64::MAX as u64 + 1)?
            .into_iter()
            .map(|(pos, _)| pos)
            .collect::<Vec<_>>()
    };
    if rows_before != rows_after {
        return Err("strict heartbeat changed the log");
    }
    eprintln!("C9 strict heartbeat dispatched at {e1:?}, no new log row");
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn retained_learner_append_snapshot_and_recover() {
    let [issuer_layout, one, two] = std::array::from_fn(|_| Layout::new());
    let (plan, [peer, endpoint]) =
        plan(&issuer_layout, "create-founder", private_ip().unwrap()).unwrap();
    let issuer_state = plan.state.clone();
    let intent = StartIntent {
        mode: plan.mode,
        cluster: CLUSTER,
        node: FOUNDER,
        peer,
        enrollment: endpoint,
    };
    let [wrap, staging] = custody(&issuer_layout).unwrap();
    let (job, mut observer, supervisor) = dispatch_founder(
        plan,
        intent,
        Params::default(),
        wrap,
        staging,
        HostedFixture(()),
    )
    .expect("C9 retained founder dispatch");
    let observed = Arc::new(Observation::default());
    let mut learners = Learners::default();
    let first = campaign(
        job,
        &mut observer,
        [&one, &two],
        endpoint,
        &observed,
        &mut learners,
    )
    .await;
    if let Err(reason) = first {
        eprintln!("C9 first refusal before cleanup: {reason}");
        observed.release.notify_one();
    }

    let mut cleanup = Vec::new();
    for run in [learners.recovered, learners.two, learners.one]
        .into_iter()
        .flatten()
    {
        cleanup.push(close_run(run, false).await.map(|_| ()));
    }
    if !job.state().close_requested {
        drop(job.request_close());
    }
    let issuer_closed = matches!(timeout(CLEANUP, supervisor).await, Ok(Ok(())))
        && matches!(
            observer.status(),
            Status::Clean | Status::ClosedAfterFounderRefusal
        );
    if issuer_closed {
        cleanup.push(check_locks(&issuer_state, false));
    } else {
        eprintln!(
            "C9 frozen issuer close refused: {:?} {:?}",
            observer.status(),
            job.state().close_failure
        );
        cleanup.push(Err("issuer close unproven"));
    }
    for result in &cleanup {
        if let Err(reason) = result {
            eprintln!("C9 cleanup refusal: {reason}");
        }
    }
    first.expect("C9 first refusal");
    assert!(cleanup.into_iter().all(|r| r.is_ok()), "C9 cleanup refusal");
    eprintln!(
        "C9 observed four Engine starts and owned closes, one Found, two Mints, two AnchorQuery/Redeem exchanges, two data Appends, one strict heartbeat, one snapshot trigger, one image retrieval, one full snapshot transfer, zero post-initialization membership mutations"
    );
}

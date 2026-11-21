use super::*;
use crate::cluster::{
    enrollment::{
        durable_join::{self, Destination, RetainedJoin},
        transfer::{self, LocalRoute},
    },
    pki::ca::fingerprint_of,
};
use learner::{HostedCatchUp, Run};
use learner_admission::{Controller, Reply, covers};
use openraft::type_config::async_runtime::watch::WatchReceiver;
use peer_transport::{Event, GatedCallTrace, GatedCallerOutcome, Stage};
use std::collections::BTreeSet;

type Check<T> = std::result::Result<T, &'static str>;
const CLUSTER: [u8; 16] = [1; 16];
const OBSERVE: Duration = Duration::from_secs(10);
const CLEANUP: Duration = Duration::from_secs(12);
// The pinned replication RPC is 100 ms hard / 75 ms soft. Bound gate
// observation from the first inbound event and release even on a refusal.
const GATE_OBSERVE: Duration = Duration::from_millis(30);

fn fixture_ip() -> Check<std::net::IpAddr> {
    let route = UdpSocket::bind("0.0.0.0:0").map_err(|_| "route bind")?;
    route.connect("192.0.2.1:9").map_err(|_| "route lookup")?;
    let ip = route.local_addr().map_err(|_| "route address")?.ip();
    match ip {
        std::net::IpAddr::V4(ip) if ip.is_private() => Ok(ip.into()),
        _ => Err("fixture requires a private unicast address"),
    }
}

fn plan(layout: &Layout, mode: &str, ip: std::net::IpAddr) -> Check<(Plan, [SocketAddr; 2])> {
    let peer = StdListener::bind(SocketAddr::new(ip, 0)).map_err(|_| "reserve peer")?;
    let enrollment = StdListener::bind(SocketAddr::new(ip, 0)).map_err(|_| "reserve enrollment")?;
    let addresses = [
        peer.local_addr().map_err(|_| "peer address")?,
        enrollment.local_addr().map_err(|_| "enrollment address")?,
    ];
    let mut plan = layout.plan(mode);
    [plan.peer, plan.enrollment] = addresses.map(|addr| addr.to_string());
    Ok((plan, addresses))
}

fn custody(layout: &Layout) -> Check<[std::path::PathBuf; 2]> {
    let paths = ["wrap", "custody"].map(|name| layout.root.path().join(name));
    for path in &paths {
        fs::create_dir(path).map_err(|_| "create fixture directory")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "qualify fixture directory")?;
    }
    Ok(paths)
}

async fn ready(job: &Job, observer: &mut Observer, desired: Status) -> Check<()> {
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
                    | Status::Founding
            ) {
                let state = job.state();
                eprintln!(
                    "C11 startup {status:?}: {:?}, founder {:?}",
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

fn node(job: &Job) -> Check<Arc<tokio::sync::Mutex<ConsensusNode>>> {
    job.state().node.clone().ok_or("retained node missing")
}

async fn mint_and_join(
    issuer: &Job,
    layout: &Layout,
    endpoint: SocketAddr,
    root: &[u8],
    exchange: &Observation,
    pair: u8,
) -> Check<(RetainedJoin, SocketAddr)> {
    let ready = issuer.state().issuer.clone().ok_or("issuer not ready")?;
    let live = node(issuer)?;
    let (code, voters) = {
        let live = live.lock().await;
        let voters = {
            let sm = live.sm.as_ref().ok_or("no issuer store")?;
            let store = sm.0.lock().map_err(|_| "issuer store unavailable")?;
            let blob = &store.membership().ok_or("issuer membership missing")?.1;
            let members =
                WireMembership::decode(blob.bytes()).map_err(|_| "issuer membership invalid")?;
            BoundVoters::observe(store.state(), &members)
                .map_err(|_| "issuer bindings unavailable")?
        };
        let code = JoinCode::generate(
            CLUSTER,
            fingerprint_of(root),
            voters.anchor(CLUSTER),
            getrandom::fill,
        )
        .map_err(|_| "fixture code generation refused")?;
        let (_, outcome) = live
            .propose(Command::Mint {
                cluster: CLUSTER,
                actor: [3; 16],
                request: [pair; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: ready
                    .clock
                    .sample_ms()
                    .map_err(|_| "checked Mint clock refused")?,
            })
            .await
            .map_err(|error| {
                eprintln!("C11 frozen Mint {pair} refused: {error:?}");
                "Mint refused"
            })?;
        if !matches!(outcome, Some(Outcome::Minted { replay: false, .. })) {
            return Err("Mint outcome refused");
        }
        (code, voters)
    };

    let (plan, [address, _]) = plan(layout, "join", endpoint.ip())?;
    let [wrap, staging] = custody(layout)?;
    let paste = transfer::encode(endpoint, root, &code).map_err(|_| "paste encoding refused")?;
    let (join, mut reader) = durable_join::start_retained(
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
        ready.tls.time.clone(),
    )
    .map_err(|_| "join launch refused")?;
    timeout(OBSERVE, async {
        tokio::select! {
            phase = reader.terminal() => {
                eprintln!("C11 early join {pair}: {phase:?}");
                return Err("join ended before issuer Redeem completion");
            }
            () = exchange.finished.notified() => {}
        }
        let result = *exchange.result.lock().unwrap();
        exchange.release.notify_one();
        if result != Some(Ok(())) {
            eprintln!("C11 frozen Redeem {pair} refused: {result:?}");
            return Err("Redeem refused");
        }
        let count = usize::from(pair);
        let answers = exchange.anchor_outcomes.lock().unwrap();
        if exchange.anchor_calls.load(Relaxed) != count
            || exchange.redeem_calls.load(Relaxed) != count
            || answers.len() != count
            || answers[count - 1] != Ok(voters.encode())
        {
            return Err("unexpected AnchorQuery/Redeem inventory");
        }
        Ok(())
    })
    .await
    .map_err(|_| "issuer exchange elapsed")??;
    if timeout(OBSERVE, reader.terminal())
        .await
        .map_err(|_| "publication elapsed")?
        != durable_join::Status::Published
    {
        return Err("retained publication refused");
    }
    Ok((join, address))
}

async fn start_learner(
    run: &mut Option<Run>,
    join: RetainedJoin,
    issuer: SocketAddr,
) -> Check<&Run> {
    *run = Some(
        learner::start_published(join, Params::default(), Some(HostedCatchUp(())))
            .map_err(|_| "consume retained publication")?,
    );
    let run = run.as_mut().unwrap();
    ready(run.job, &mut run.observer, Status::LearnerReady).await?;
    run.set_admission_route(issuer)
        .map_err(|_| "explicit issuer route refused")?;
    Ok(run)
}

async fn admit(run: &Run, address: SocketAddr) -> Check<LogPos> {
    match run.admit().await {
        Reply::Applied(position, actual) if actual == address => Ok(position),
        Reply::Applied(..) => Err("learner admission applied at a different address"),
        Reply::Revoked(..) => Err("learner admission revoked"),
        reply @ (Reply::Pending
        | Reply::AddressConflict
        | Reply::StaleTail
        | Reply::Refused
        | Reply::Unknown) => {
            eprintln!("C11 frozen admission did not confirm apply: {reply:?}");
            Err("learner admission refused")
        }
    }
}

async fn engine(job: &Job) -> Check<Engine> {
    node(job)?
        .lock()
        .await
        .raft
        .clone()
        .ok_or("retained Engine missing")
}

async fn membership(job: &Job) -> Check<(LogPos, Members)> {
    let node = node(job)?;
    let live = node.lock().await;
    let sm = live.sm.as_ref().ok_or("retained applied store missing")?;
    let store =
        sm.0.lock()
            .map_err(|_| "retained applied store unavailable")?;
    let (at, blob) = store.membership().ok_or("applied membership missing")?;
    Ok((*at, decode(blob).map_err(|_| "invalid applied membership")?))
}

async fn match_through(raft: &Engine, node: u64, minimum: LogPos) -> Check<()> {
    let mut metrics = raft.metrics();
    timeout(OBSERVE, async {
        loop {
            {
                let metrics = metrics.borrow_and_update();
                if let Some(commit) = metrics.cluster_committed
                    && covers(pos(commit), minimum)
                    && metrics
                        .replication
                        .as_ref()
                        .and_then(|r| r.get(&node).copied().flatten())
                        .is_some_and(|matched| covers(pos(matched), pos(commit)))
                {
                    return Ok(());
                }
            }
            metrics
                .changed()
                .await
                .map_err(|_| "match observation closed")?;
        }
    })
    .await
    .map_err(|_| "match observation elapsed")?
}

// Enforce the uninterrupted event sequence on the selected caller's socket.
async fn transport_event(
    trace: &GatedCallTrace,
    cursor: &mut usize,
    boundary: &str,
    expected: impl FnOnce(Event) -> bool,
) -> Check<(SocketAddr, Instant)> {
    let (peer, at, event) = trace.next_server_event(cursor).await?;
    match event {
        Event::Refused(reason) | Event::Finished(Some(reason)) => {
            eprintln!("C11 frozen gated replication refused at {boundary}: {reason:?}");
            Err("gated peer replication refused")
        }
        event if expected(event) => Ok((peer, at)),
        event => {
            eprintln!("C11 unexpected event at gated replication {boundary}: {event:?}");
            Err("gated peer replication event sequence differed")
        }
    }
}

async fn stage(
    trace: &GatedCallTrace,
    cursor: &mut usize,
    target: Stage,
) -> Check<(SocketAddr, Instant)> {
    transport_event(trace, cursor, "transport stage", |event| {
        event == Event::Stage(target)
    })
    .await
}

fn issuer_caller_observer(
    job: &Job,
    target: NodeId,
    trace: Arc<GatedCallTrace>,
) -> Check<oneshot::Receiver<GatedCallerOutcome>> {
    let state = job.state();
    let transport = state
        .founder
        .as_ref()
        .ok_or("retained founder missing")?
        .transport_for_test()
        .try_lock()
        .map_err(|_| "issuer transport busy")?;
    Ok(transport.observe_gated_caller(target, trace))
}

async fn caller_outcome(receiver: oneshot::Receiver<GatedCallerOutcome>) -> Check<SocketAddr> {
    let outcome = receiver
        .await
        .map_err(|_| "gated RPC caller observation closed")?;
    if let Err(reason) = outcome.result {
        eprintln!("C11 frozen gated RPC caller refused: {reason:?}");
        return Err("gated RPC caller refused");
    }
    outcome
        .source
        .ok_or("gated RPC caller source was unavailable")
}

async fn admit_below_target(
    run: &Run,
    address: SocketAddr,
    raft: &Engine,
    controller: &Controller,
    caller: oneshot::Receiver<GatedCallerOutcome>,
    trace: &GatedCallTrace,
) -> Check<LogPos> {
    let mut cursor = 0;
    run.arm_before_dispatch_gate().await;
    let mut gate_held = true;
    let mut request = std::pin::pin!(admit(run, address));
    let evidence = timeout(OBSERVE, async {
        tokio::try_join!(
            biased;
            caller_outcome(caller),
            async {
                // Poll real admission while waiting for its first replication RPC.
                let (arrival, already_admitted) = tokio::select! {
                    result = stage(trace, &mut cursor, Stage::ServerHandshake) => (result?.1, None),
                    result = &mut request => {
                        let admitted = result?;
                        (stage(trace, &mut cursor, Stage::ServerHandshake).await?.1, Some(admitted))
                    }
                };
                let deadline = arrival + GATE_OBSERVE;
                let (admitted, server_peer) = tokio::select! {
                    biased;
                    _ = tokio::time::sleep_until(deadline) => {
                        return Err("short transport gate observation elapsed");
                    }
                    result = async {
                        stage(trace, &mut cursor, Stage::ServerAuthenticated).await?;
                        stage(trace, &mut cursor, Stage::RequestRead).await?;
                        let (server_peer, _) = stage(trace, &mut cursor, Stage::BeforeDispatch).await?;
                        let admitted = match already_admitted {
                            Some(admitted) => admitted,
                            None => request.await?,
                        };
                        {
                            let metrics = raft.metrics();
                            let current = metrics.borrow_watched();
                            let committed = current
                                .cluster_committed
                                .map(pos)
                                .ok_or("fresh target missing")?;
                            let matched = current
                                .replication
                                .as_ref()
                                .and_then(|r| r.get(&run.binding().0.get()))
                                .copied()
                                .flatten()
                                .map(pos);
                            if *current.committed_membership_config.log_id() != Some(id(admitted))
                                || !current
                                    .committed_membership_config
                                    .membership()
                                    .voter_ids()
                                    .eq([FOUNDER.get()])
                                || !covers(committed, admitted)
                                || matched.is_some_and(|matched| covers(matched, committed))
                                || controller.hooks.promotion.joint_calls.load(Relaxed) != 0
                                || controller.hooks.promotion.flatten_calls.load(Relaxed) != 0
                            {
                                return Err("gated second learner did not prove the below-target state");
                            }
                        }
                        let released = run.release_before_dispatch_gate().await
                            .ok_or("gated dispatch gate was unavailable")?;
                        gate_held = false;
                        trace.caller("gate release observed", None);
                        // A buffered branch can finish in one poll even after the timer was polled.
                        if released > deadline {
                            eprintln!("C11 gate released after {:?}", released.duration_since(arrival));
                            return Err("short transport gate released after deadline");
                        }
                        Ok((admitted, server_peer))
                    } => result?,
                };

                transport_event(trace, &mut cursor, "dispatch", |event| {
                    matches!(event, Event::LearnerDispatch(_))
                }).await?;
                stage(trace, &mut cursor, Stage::BeforeReply).await?;
                transport_event(trace, &mut cursor, "reply result", |event| {
                    event == Event::Finished(None)
                }).await?;
                Ok((admitted, server_peer))
            }
        )
    })
    .await
    .map_err(|_| "gated replication observation elapsed");

    if gate_held {
        let _ = run.release_before_dispatch_gate().await;
        trace.caller("cleanup gate release observed", None);
    }
    eprintln!(
        "C11 first call (soft TTL, caller stages, matched inbound stages, unrelated inbound events, omitted unrelated count) from caller entry: {:?}",
        trace.summary()
    );
    let (caller_source, (admitted, server_peer)) = evidence??;
    if caller_source != server_peer {
        return Err("gated dispatch and caller result came from different RPC connections");
    }
    Ok(admitted)
}

async fn applied(
    job: &Job,
    position: Id,
    members: &Members,
    bindings: [(NodeId, crate::cluster::enrollment::KeyDigest); 2],
) -> Check<()> {
    let raft = engine(job).await?;
    let mut progress = raft.watch_apply_progress();
    timeout(OBSERVE, progress.wait_until_ge(&Some(position)))
        .await
        .map_err(|_| "uniform apply observation elapsed")?
        .map_err(|_| "uniform apply observation closed")?;
    let retained = node(job)?;
    let live = retained.lock().await;
    let sm = live.sm.as_ref().ok_or("retained applied store missing")?;
    let store =
        sm.0.lock()
            .map_err(|_| "retained applied store unavailable")?;
    let (at, blob) = store.membership().ok_or("applied membership missing")?;
    if id(*at) != position
        || decode(blob).map_err(|_| "invalid applied membership")? != *members
        || bindings
            .iter()
            .any(|(node, key)| store.state().authorize_peer(CLUSTER, *node, *key).is_err())
    {
        return Err("retained uniform membership or exact bindings differed");
    }
    Ok(())
}

async fn inventory(job: &Job, redeems: [LogPos; 2], expected: [(LogPos, Members); 4]) -> Check<()> {
    let live = node(job)?;
    let live = live.lock().await;
    let log = live.log_weak.upgrade().ok_or("retained log missing")?;
    let rows = log
        .lock()
        .map_err(|_| "retained log unavailable")?
        .entries(0, expected[3].0.index + 2, 16)
        .map_err(|_| "bounded log read refused")?;
    let mut found = 0;
    let mut minted = 0;
    let mut redeemed = Vec::new();
    let mut memberships = Vec::new();
    for (position, row) in rows {
        match row {
            StoredEntry::Membership(blob) => {
                memberships.push((
                    position,
                    decode(&blob).map_err(|_| "membership row invalid")?,
                ));
            }
            StoredEntry::Enrollment(Command::Found { .. }) => found += 1,
            StoredEntry::Enrollment(Command::Mint { .. }) => minted += 1,
            StoredEntry::Enrollment(Command::Redeem { .. }) => redeemed.push(position),
            _ => {}
        }
    }
    if found != 1
        || minted != 2
        || redeemed != redeems
        || memberships.len() != 5
        || memberships[1..] != expected
    {
        return Err("durable invocation count or exact membership row differed");
    }
    Ok(())
}

async fn campaign(
    job: &Job,
    observer: &mut Observer,
    [peer, enrollment]: [SocketAddr; 2],
    layouts: [&Layout; 2],
    exchange: &Arc<Observation>,
    [first, second]: &mut [Option<Run>; 2],
) -> Check<()> {
    ready(job, observer, Status::FounderReady).await?;
    let (controller, root) = {
        let state = job.state();
        let controller = state
            .admission
            .clone()
            .ok_or("admission controller missing")?;
        let root = state
            .owner
            .as_ref()
            .ok_or("owner missing")?
            .read("ca.der", 4096)
            .map_err(|_| "issuer CA unavailable")?;
        (controller, root)
    };
    let hooks = &controller.hooks.promotion;
    hooks.active.store(true, Relaxed);
    hooks.interrupt.store(true, Relaxed);
    controller.hooks.active.store(true, Relaxed);
    job.start_enrollment(Some(exchange.clone()))
        .map_err(|_| "issuer ingress refused")?;
    let issuer = engine(job).await?;

    let (join, addr1) = mint_and_join(job, layouts[0], enrollment, &root, exchange, 1).await?;
    let first = start_learner(first, join, peer).await?;
    let learner1 = first.binding().0.get();
    let l1 = admit(first, addr1).await?;
    match_through(&issuer, learner1, l1).await?;
    let (at1, members1) = membership(job).await?;
    if at1 != l1
        || !members1.voter_ids().eq([FOUNDER.get()])
        || !members1.non_write_voter_ids().eq([learner1])
        || hooks.joint_calls.load(Relaxed) != 0
    {
        return Err("the first admitted host did not stay a learner");
    }

    let (join, addr2) = mint_and_join(job, layouts[1], enrollment, &root, exchange, 2).await?;
    let second = start_learner(second, join, peer).await?;
    match_through(&issuer, learner1, second.original()).await?;
    let second_id = second.binding().0;
    let learner2 = second_id.get();
    let trace = Arc::new(GatedCallTrace::default());
    second.observe_gated_inbound(&trace).await;
    let caller = issuer_caller_observer(job, second_id, trace.clone())?;
    let l2 = admit_below_target(second, addr2, &issuer, &controller, caller, &trace).await?;

    timeout(OBSERVE, hooks.finished.notified())
        .await
        .map_err(|_| "promotion did not confirm uniform application")?;
    if let Some(reason) = *hooks.failure.lock().unwrap() {
        return Err(reason);
    }
    let (joint_at, joint) = hooks
        .interrupted
        .lock()
        .unwrap()
        .clone()
        .ok_or("no witnessed interruption at committed J")?;
    let (flat_at, flat) = hooks
        .confirmed
        .lock()
        .unwrap()
        .clone()
        .ok_or("reconciliation did not confirm uniform F")?;
    let target = BTreeSet::from([FOUNDER.get(), learner1, learner2]);
    let voters = BTreeSet::from([FOUNDER.get()]);
    let nodes = [(FOUNDER.get(), peer), (learner1, addr1), (learner2, addr2)]
        .into_iter()
        .map(|(id, addr)| (id, BasicNode::new(addr.to_string())))
        .collect::<BTreeMap<_, _>>();
    if target.len() != 3
        || joint.get_joint_config() != &vec![voters.clone(), target.clone()]
        || flat.get_joint_config() != &vec![target]
        || !joint.nodes().eq(nodes.iter())
        || !flat.nodes().eq(nodes.iter())
        || !members1
            .nodes()
            .eq(nodes.iter().filter(|(id, _)| **id != learner2))
        || joint_at.index != l2.index + 1
        || flat_at.index != joint_at.index + 1
        || hooks.unknown.load(Relaxed) != 1
        || hooks.reconciliation_wakes.load(Relaxed) != 1
        || hooks.joint_calls.load(Relaxed) != 1
        || hooks.flatten_calls.load(Relaxed) != 1
        || controller.hooks.requests.load(Relaxed) != 2
        || controller.hooks.calls.load(Relaxed) != 2
    {
        return Err("exact interrupted joint, release wake, uniform or SDK inventory differed");
    }
    for retained in [job, first.job, second.job] {
        applied(
            retained,
            flat_at,
            &flat,
            [first.binding(), second.binding()],
        )
        .await?;
    }
    let members2 =
        Members::new(vec![voters], nodes).map_err(|_| "expected learner metadata invalid")?;
    inventory(
        job,
        [first.original(), second.original()],
        [
            (l1, members1),
            (l2, members2),
            (pos(joint_at), joint),
            (pos(flat_at), flat),
        ],
    )
    .await?;
    eprintln!("C11 l1={l1:?} l2={l2:?} interrupted J={joint_at:?} reconciled F={flat_at:?}");
    Ok(())
}

async fn close_learner(run: Option<Run>) -> Check<()> {
    let Some(run) = run else { return Ok(()) };
    let job = run.job;
    match timeout(CLEANUP, run.close(false)).await {
        Ok(Ok(None)) => Ok(()),
        outcome => {
            let failure = match outcome {
                Ok(Err(reason)) => Some(reason),
                _ => None,
            };
            let state = job.state();
            eprintln!(
                "C11 learner close {failure:?}, state {:?}, shutdown {:?}",
                state.status, state.close_failure
            );
            Err("owned learner close refused")
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn retained_joiners_promote_and_reconcile() {
    let [founder, first, second] = std::array::from_fn(|_| Layout::new());
    let (plan, addresses @ [peer, enrollment]) =
        plan(&founder, "create-founder", fixture_ip().unwrap()).unwrap();
    let [wrap, store] = custody(&founder).unwrap();
    let intent = StartIntent {
        mode: plan.mode,
        cluster: CLUSTER,
        node: FOUNDER,
        peer,
        enrollment,
    };
    let (job, mut observer, supervisor) = dispatch_founder(
        plan,
        intent,
        Params::default(),
        wrap,
        store,
        HostedFixture(()),
    )
    .expect("C11 retained founder dispatch");
    let exchange = Arc::new(Observation::default());
    let mut learners = [None, None];
    let first_failure = campaign(
        job,
        &mut observer,
        addresses,
        [&first, &second],
        &exchange,
        &mut learners,
    )
    .await;
    if let Err(reason) = first_failure {
        eprintln!("C11 first refusal before cleanup: {reason}");
        exchange.release.notify_one();
    }
    if !job.state().close_requested {
        drop(job.request_close());
    }
    let [first, second] = learners;
    let (first_close, second_close, issuer_close) = tokio::join!(
        close_learner(first),
        close_learner(second),
        timeout(CLEANUP, supervisor),
    );
    let issuer_closed = matches!(issuer_close, Ok(Ok(())))
        && matches!(
            observer.status(),
            Status::Clean | Status::ClosedAfterFounderRefusal
        );
    eprintln!(
        "C11 owned closes: {first_close:?}, {second_close:?}, issuer {issuer_closed}, shutdown {:?}",
        job.state().close_failure
    );
    first_failure.expect("C11 first refusal");
    first_close.expect("C11 first learner close");
    second_close.expect("C11 second learner close");
    assert!(issuer_closed, "C11 issuer close unproven");
    eprintln!(
        "C11 observed three Engine starts and owned closes, one unchanged Found, two fixture Mints, two actual AnchorQuery/Redeem exchanges, two AddNodes, one guarded joint append and one guarded flatten"
    );
}

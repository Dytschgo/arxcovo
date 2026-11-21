use super::*;
use crate::cluster::{
    enrollment::{
        durable_join::{self, Destination, RetainedJoin},
        transfer::{self, LocalRoute},
    },
    pki::ca::fingerprint_of,
};
use learner::{HostedCatchUp, Run};
use learner_admission::{Reply, covers};
use openraft::type_config::async_runtime::watch::WatchReceiver;
use std::collections::BTreeSet;

type Check<T> = std::result::Result<T, &'static str>;
const CLUSTER: [u8; 16] = [1; 16];
const OBSERVE: Duration = Duration::from_secs(10);
const CLEANUP: Duration = Duration::from_secs(12);

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
                    "C12 startup {status:?}: {:?}, founder {:?}",
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
    let prior = exchange.anchor_calls.load(Relaxed);
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
                eprintln!("C12 frozen Mint {pair} refused: {error:?}");
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
                eprintln!("C12 early join {pair}: {phase:?}");
                return Err("join ended before issuer Redeem completion");
            }
            () = exchange.finished.notified() => {}
        }
        let result = *exchange.result.lock().unwrap();
        exchange.release.notify_one();
        if result != Some(Ok(())) {
            eprintln!("C12 frozen Redeem {pair} refused: {result:?}");
            return Err("Redeem refused");
        }
        let count = prior + 1;
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
        reply => {
            eprintln!("C12 frozen admission did not confirm apply: {reply:?}");
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

// The transport events have no connection ID. Require a single uninterrupted
// event sequence on the second learner, through the gated RPC's terminal event.
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
                "C12 learner close {failure:?}, state {:?}, shutdown {:?}",
                state.status, state.close_failure
            );
            Err("owned learner close refused")
        }
    }
}

async fn membership_rows(
    job: &Job,
    end: LogPos,
    redeems: &[LogPos],
) -> Check<Vec<(LogPos, Members)>> {
    let retained = node(job)?;
    let live = retained.lock().await;
    let log = live.log_weak.upgrade().ok_or("retained log missing")?;
    let rows = log
        .lock()
        .map_err(|_| "retained log unavailable")?
        .entries(0, end.index + 1, 32)
        .map_err(|_| "bounded log read refused")?;
    let mut found = 0;
    let mut minted = 0;
    let mut redeemed = Vec::new();
    let mut memberships = Vec::new();
    for (position, row) in rows {
        match row {
            StoredEntry::Membership(blob) => memberships.push((
                position,
                decode(&blob).map_err(|_| "membership row invalid")?,
            )),
            StoredEntry::Enrollment(Command::Found { .. }) => found += 1,
            StoredEntry::Enrollment(Command::Mint { .. }) => minted += 1,
            StoredEntry::Enrollment(Command::Redeem { .. }) => redeemed.push(position),
            StoredEntry::Blank => (),
            _ => return Err("unexpected log record"),
        }
    }
    if found != 1 || minted != redeems.len() || redeemed != redeems || memberships.len() != 5 {
        return Err("durable operation inventory differed");
    }
    Ok(memberships)
}

async fn prepare_voters(
    job: &'static Job,
    observer: &mut Observer,
    [peer, endpoint]: [SocketAddr; 2],
    [first_layout, second_layout]: [&Layout; 2],
    [one, two]: &mut [Option<Run>; 2],
    exchange: &Arc<Observation>,
) -> Check<(Vec<u8>, LogPos)> {
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
            .ok_or("retained founder owner missing")?
            .read("ca.der", 4096)
            .map_err(|_| "original root unavailable")?;
        (controller, root)
    };
    let hooks = &controller.hooks.promotion;
    hooks.active.store(true, Relaxed);
    controller.hooks.active.store(true, Relaxed);
    job.start_enrollment(Some(exchange.clone()))
        .map_err(|_| "founder ingress refused")?;
    let (join, first_address) =
        mint_and_join(job, first_layout, endpoint, &root, exchange, 1).await?;
    let first = start_learner(one, join, peer).await?;
    let first_key = first.binding();
    let add_first = admit(first, first_address).await?;
    let founder = engine(job).await?;
    match_through(&founder, first_key.0.get(), add_first).await?;
    let (at_first, one_learner) = membership(job).await?;
    if at_first != add_first
        || !one_learner.voter_ids().eq([1])
        || !one_learner.non_write_voter_ids().eq([first_key.0.get()])
        || hooks.joint_calls.load(Relaxed) != 0
    {
        return Err("first join was not a replication-only learner");
    }
    let (join, second_address) =
        mint_and_join(job, second_layout, endpoint, &root, exchange, 2).await?;
    let second = start_learner(two, join, peer).await?;
    let second_key = second.binding();
    match_through(&founder, first_key.0.get(), second.original()).await?;
    let add_second = admit(second, second_address).await?;
    timeout(OBSERVE, hooks.finished.notified())
        .await
        .map_err(|_| "uniform promotion not confirmed")?;
    if let Some(reason) = *hooks.failure.lock().unwrap() {
        return Err(reason);
    }
    let (flat_id, flat) = hooks
        .confirmed
        .lock()
        .unwrap()
        .clone()
        .ok_or("no confirmed promotion")?;
    let flat_position = pos(flat_id);
    let target = BTreeSet::from([1, first_key.0.get(), second_key.0.get()]);
    let addresses = [
        (1, peer),
        (first_key.0.get(), first_address),
        (second_key.0.get(), second_address),
    ]
    .map(|(id, address)| (id, BasicNode::new(address.to_string())));
    if !flat.get_joint_config().iter().eq([&target])
        || !flat
            .nodes()
            .eq(addresses.iter().map(|(id, address)| (id, address)))
        || hooks.joint_calls.load(Relaxed) != 1
        || hooks.flatten_calls.load(Relaxed) != 1
        || hooks.unknown.load(Relaxed) != 0
        || controller.hooks.requests.load(Relaxed) != 2
        || controller.hooks.calls.load(Relaxed) != 2
    {
        return Err("expected one uninterrupted joint and flatten");
    }
    for retained in [job, first.job, second.job] {
        applied(retained, flat_id, &flat, [first_key, second_key]).await?;
    }
    let rows = membership_rows(job, flat_position, &[first.original(), second.original()]).await?;
    let [(..), (p1, a1), (p2, a2), (j, joint), (f, last)] = rows.as_slice() else {
        return Err("unexpected initial membership inventory");
    };
    if rows[..4]
        .iter()
        .any(|(_, members)| issuer_transfer::promoted_targets(members, FOUNDER).is_some())
        || *p1 != add_first
        || *p2 != add_second
        || *a1 != one_learner
        || !a2.voter_ids().eq([1])
        || !a2.nodes().eq(flat.nodes())
        || joint.get_joint_config().as_slice() != [BTreeSet::from([1]), target]
        || !joint.nodes().eq(flat.nodes())
        || j.index != add_second.index + 1
        || f.index != j.index + 1
        || *f != flat_position
        || *last != flat
    {
        return Err("the applied AddNodes, J and F rows differed");
    }
    for run in [first, second] {
        if job.transfer_issuer_to(run.binding().0).await
            != peer_transport::TransferOutcome::Installed
        {
            return Err("custody transfer did not acknowledge");
        }
        if run
            .loaded_voter_root()
            .map_err(|_| "local voter custody load refused")?
            != root
        {
            return Err("local voter custody changed the original CA");
        }
    }
    eprintln!("C12 two AddNodes, J={j:?}, F={f:?} and two custody acknowledgements");
    Ok((root, flat_position))
}

async fn survivor<'a>(first: &'a Run, second: &'a Run) -> Check<(&'a Run, &'a Run)> {
    let [left, right] = [engine(first.job).await?, engine(second.job).await?];
    let [mut a, mut b] = [left.metrics(), right.metrics()];
    timeout(OBSERVE, async {
        loop {
            for (metrics, leader, follower) in [(&mut a, first, second), (&mut b, second, first)] {
                let m = metrics.borrow_and_update();
                if m.running_state.is_err() {
                    return Err("surviving Engine refused");
                }
                if m.current_leader == Some(leader.binding().0.get())
                    && m.state == openraft::ServerState::Leader
                    && m.vote.committed
                    && m.last_applied
                        .is_some_and(|p| p.leader_id == m.vote.leader_id)
                {
                    return Ok((leader, follower));
                }
            }
            tokio::select! {
                change = a.changed() => change,
                change = b.changed() => change,
            }
            .map_err(|_| "surviving metrics closed")?;
        }
    })
    .await
    .map_err(|_| "surviving leadership did not establish")?
}

async fn final_join(
    first: &Run,
    second: &Run,
    layout: &Layout,
    root: &[u8],
    flat: LogPos,
) -> Check<()> {
    let (leader, other) = survivor(first, second).await?;
    leader
        .job
        .activate_promoted_issuer()
        .await
        .map_err(|_| "promoted issuer activation refused")?;
    let exchange = Arc::new(Observation::default());
    let endpoint = leader
        .job
        .state()
        .issuer
        .as_ref()
        .ok_or("issuer missing")?
        .endpoint;
    leader
        .job
        .start_enrollment(Some(exchange.clone()))
        .map_err(|_| "promoted ingress refused")?;
    let result = mint_and_join(leader.job, layout, endpoint, root, &exchange, 3).await;
    if result.is_err() {
        exchange.release.notify_one();
    }
    let (published, _) = result?;
    let publication = published
        .consume()
        .map_err(|_| "final retained publication missing")?;
    let e = publication.expected.origin.position();
    if publication.expected.node.get() != 4
        || e.leader != leader.binding().0.get()
        || publication.expected.ca != fingerprint_of(root)
        || publication
            .owner
            .read("ca.der", 4096)
            .map_err(|_| "published root missing")?
            != root
        || e.index <= flat.index
    {
        return Err("promoted issuance changed the root, real allocation or original E");
    }
    let now = publication
        .time
        .current_time()
        .ok_or("publication clock refused")?
        .as_secs()
        * 1000;
    let copy = publication
        .custody
        .member()
        .map_err(|_| "final member custody refused")?;
    crate::cluster::issuance_publication::check_retained(
        &publication.owner,
        Some(&publication.expected),
        publication.custody.installation(),
        &copy,
        Some(now),
    )
    .map_err(|_| "original CA did not verify the final leaf and retained publication")?;
    let rows = membership_rows(leader.job, e, &[first.original(), second.original(), e]).await?;
    if rows.last().map(|(position, _)| *position) != Some(flat)
        || membership(leader.job).await?.0 != flat
        || other
            .loaded_voter_root()
            .map_err(|_| "other voter no longer has qualified custody")?
            != root
    {
        return Err("membership or surviving custody changed after founder close");
    }
    for retained in [leader.job, other.job] {
        let state = retained.state();
        let owner = state.owner.as_ref().ok_or("retained voter owner missing")?;
        let mut files = fs::read_dir(&owner.plan.credentials)
            .map_err(|_| "credential inventory unavailable")?
            .take(4)
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|_| "credential inventory refused")?;
        files.sort();
        if files != ["ca.der", "identity.json", "member-key"] {
            return Err("private custody entered the published credentials");
        }
    }
    eprintln!(
        "C12 surviving issuer={} allocated node=4 at E={e:?}; latest membership={flat:?}",
        leader.binding().0.get()
    );
    Ok(())
}

async fn close_founder(
    job: &Job,
    observer: &Observer,
    supervisor: &mut Option<tokio::task::JoinHandle<()>>,
) -> Check<()> {
    let Some(supervisor) = supervisor.take() else {
        return Ok(());
    };
    if !job.state().close_requested {
        drop(job.request_close());
    }
    let outcome = timeout(CLEANUP, supervisor).await;
    if !matches!(outcome, Ok(Ok(()))) || observer.status() != Status::Clean {
        eprintln!(
            "C12 founder close status={:?}, shutdown={:?}",
            observer.status(),
            job.state().close_failure
        );
        return Err("founder owned close refused");
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn promoted_voter_issues_after_founder_close() {
    let [founder, first, second, final_layout] = std::array::from_fn(|_| Layout::new());
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
    .expect("C12 retained founder dispatch");
    let mut supervisor = Some(supervisor);
    let exchange = Arc::new(Observation::default());
    let mut voters = [None, None];
    let result = async {
        let (root, flat) = prepare_voters(
            job,
            &mut observer,
            addresses,
            [&first, &second],
            &mut voters,
            &exchange,
        )
        .await?;
        close_founder(job, &observer, &mut supervisor).await?;
        let [first, second] = &voters;
        final_join(
            first.as_ref().unwrap(),
            second.as_ref().unwrap(),
            &final_layout,
            &root,
            flat,
        )
        .await
    }
    .await;
    if let Err(reason) = result {
        eprintln!("C12 first refusal before remaining cleanup: {reason}");
        exchange.release.notify_one();
    }
    let [first, second] = voters;
    let (a, b, founder) = tokio::join!(
        close_learner(first),
        close_learner(second),
        close_founder(job, &observer, &mut supervisor),
    );
    eprintln!("C12 remaining owned closes: {a:?}, {b:?}, founder {founder:?}");
    result.expect("C12 first refusal");
    a.expect("C12 first voter close");
    b.expect("C12 second voter close");
    founder.expect("C12 founder close");
    eprintln!(
        "C12 observed 3 retained Engine starts and owned closes, 1 Found, 3 fixture Mints, 3 AnchorQuery/Redeem exchanges, 2 AddNodes, 1 joint append, 1 flatten and 2 custody transfers. Final destination only published."
    );
}

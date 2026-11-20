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
use peer_registry::Phase;
use tokio::sync::oneshot;

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
                    "C10 startup phase={status:?} refusal={:?} frozen-founder={:?}",
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
                request: [1; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: ready_issuer
                    .clock
                    .sample_ms()
                    .map_err(|_| "issuer clock refused")?,
            })
            .await
            .map_err(|error| {
                eprintln!("C10 frozen Mint refused: {error:?}");
                "Mint refused"
            })?;
        if !matches!(minted.1, Some(Outcome::Minted { replay: false, .. })) {
            return Err("Mint outcome refused");
        }
        (code, voters)
    };
    issuer
        .start_enrollment(Some(observed.clone()))
        .map_err(|_| "issuer ingress refused")?;
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
                eprintln!("C10 early join phase={phase:?}");
                return Err("join ended before actual issuer Redeem completion");
            }
            () = observed.finished.notified() => {}
        }
        let result = *observed.result.lock().unwrap();
        observed.release.notify_one();
        if result != Some(Ok(())) {
            eprintln!("C10 frozen Redeem refused: {result:?}");
            return Err("issuer Redeem refused");
        }
        let anchors = observed.anchor_outcomes.lock().unwrap();
        let count = 1;
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
            "C10 learner close refused: {reason:?} at {:?}, shutdown {:?}",
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

fn arm(controller: &Controller) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    assert!(
        controller
            .hooks
            .barrier
            .lock()
            .unwrap()
            .replace((entered_tx, release_rx))
            .is_none()
    );
    (entered_rx, release_tx)
}

async fn advance_tail(issuer: &Job, root: &[u8]) -> Result<LogPos> {
    let retained = retained_node(issuer).await?;
    let ready = issuer.state().issuer.clone().ok_or("issuer not ready")?;
    let node = retained.lock().await;
    let anchor = {
        let sm = node.sm.as_ref().ok_or("issuer store missing")?;
        let store = sm.0.lock().map_err(|_| "issuer store poisoned")?;
        let (_, blob) = store.membership().ok_or("issuer membership missing")?;
        let membership =
            WireMembership::decode(blob.bytes()).map_err(|_| "issuer membership invalid")?;
        BoundVoters::observe(store.state(), &membership)
            .map_err(|_| "issuer binding absent")?
            .anchor(CLUSTER)
    };
    let code = JoinCode::generate(CLUSTER, fingerprint_of(root), anchor, getrandom::fill)
        .map_err(|_| "second fixture code generation")?;
    let (position, outcome) = node
        .propose(Command::Mint {
            cluster: CLUSTER,
            actor: [3; 16],
            request: [2; 16],
            code: code.code_id(),
            verifier: code.verifier(),
            proposed_at_ms: ready
                .clock
                .sample_ms()
                .map_err(|_| "checked Mint clock refused")?,
        })
        .await
        .map_err(|error| {
            eprintln!("C10 frozen second Mint refused: {error:?}");
            "second fixture Mint refused"
        })?;
    if !matches!(outcome, Some(Outcome::Minted { replay: false, .. })) {
        return Err("second fixture Mint outcome refused");
    }
    Ok(position)
}

async fn issuer_membership(issuer: &Job) -> Result<(LogPos, MembershipBlob)> {
    let node = retained_node(issuer).await?;
    let node = node.lock().await;
    let sm = node.sm.as_ref().ok_or("issuer store missing")?;
    let store = sm.0.lock().map_err(|_| "issuer store poisoned")?;
    store
        .membership()
        .cloned()
        .ok_or("issuer membership missing")
}

async fn stale_tail(
    issuer: &Job,
    run: &Run,
    root: &[u8],
    controller: &Controller,
) -> Result<LogPos> {
    let before = issuer_membership(issuer).await?;
    let (entered, release) = arm(controller);
    let request = run.admit();
    tokio::pin!(request);
    tokio::select! {
        answer = &mut request => {
            eprintln!("C10 first response before stale-tail barrier: {answer:?}");
            return Err("first admission did not reserve a guarded intent");
        }
        entered = timeout(OBSERVE, entered) => {
            entered.map_err(|_| "first admission barrier elapsed")?.map_err(|_| "first admission barrier lost")?;
        }
    }
    let advanced = advance_tail(issuer, root).await?;
    release.send(()).map_err(|_| "stale-tail barrier lost")?;
    let reply = request.await;
    let e = run.original();
    if reply != Reply::StaleTail {
        eprintln!("C10 first guarded reply: {reply:?}");
        return Err("SDK did not give definite stale-tail refusal");
    }
    let rejected = *controller.hooks.stale.lock().unwrap();
    if rejected != Some((e, advanced))
        || advanced.index != e.index + 1
        || controller.hooks.calls.load(Relaxed) != 1
        || issuer_membership(issuer).await? != before
    {
        return Err("stale-tail no-write evidence differed");
    }
    let retained = retained_node(issuer).await?;
    let node = retained.lock().await;
    let rows = stored(&node, advanced.index + 2)?;
    if rows.last().map(|(p, _)| *p) != Some(advanced) {
        return Err("guarded stale-tail refusal wrote a row");
    }
    Ok(advanced)
}

async fn confirm_replication(run: &Run, position: LogPos, address: SocketAddr) -> Result<()> {
    let raft = engine(run.job).await?;
    let mut progress = raft.watch_apply_progress();
    timeout(OBSERVE, progress.wait_until_ge(&Some(id(position))))
        .await
        .map_err(|_| "autonomous learner apply elapsed")?
        .map_err(|_| "learner apply observation closed")?;
    drop(progress);
    drop(raft);
    if run.admission_progress() != Some((Phase::Strict, Some(position))) {
        return Err("learner did not durably cross E and apply L");
    }
    let retained = retained_node(run.job).await?;
    let node = retained.lock().await;
    let rows = stored(&node, position.index + 1)?;
    if !rows.iter().any(|(p, _)| *p == run.original()) {
        return Err("learner retained log did not cover the exact original E");
    }
    let sm = node.sm.as_ref().ok_or("learner store missing")?;
    let store = sm.0.lock().map_err(|_| "learner store poisoned")?;
    let (id, key) = run.binding();
    let (watermark, members) = store.membership().ok_or("learner membership missing")?;
    let members = decode(members).map_err(|_| "learner applied malformed membership")?;
    if *watermark != position
        || members.get_joint_config().len() != 1
        || !members.voter_ids().eq([FOUNDER.get()])
        || members
            .get_node(&id.get())
            .is_none_or(|m| m.addr != address.to_string())
        || store.state().authorize_peer(CLUSTER, id, key).is_err()
    {
        return Err("learner readback did not confirm exact learner membership and binding");
    }
    Ok(())
}

async fn complete(
    run: &Run,
    controller: &Controller,
    address: SocketAddr,
    advanced: LogPos,
) -> Result<LogPos> {
    let (entered, release) = arm(controller);
    let admit = run.admit();
    tokio::pin!(admit);
    tokio::select! {
        answer = &mut admit => {
            eprintln!("C10 second reply before pending barrier: {answer:?}");
            return Err("new same-address request did not reserve an intent");
        }
        entered = timeout(OBSERVE, entered) => {
            entered.map_err(|_| "pending barrier elapsed")?.map_err(|_| "pending barrier lost")?;
        }
    }
    let pending = run.admit().await;
    if pending != Reply::Pending || controller.hooks.calls.load(Relaxed) != 1 {
        eprintln!("C10 overlap reply: {pending:?}");
        return Err("same-address overlap did not refuse before SDK");
    }
    controller.hooks.suppress_success.store(true, Relaxed);
    release.send(()).map_err(|_| "pending barrier lost")?;
    let reply = admit.await;
    let lost = *controller.hooks.lost_reply.lock().unwrap();
    let Some(Reply::Applied(position, applied_address)) = lost else {
        eprintln!("C10 second guarded request ended {reply:?} without confirmed apply");
        return Err("second guarded request did not confirm applied membership");
    };
    if reply != Reply::Unknown
        || applied_address != address
        || position.index != advanced.index + 1
        || controller.hooks.calls.load(Relaxed) != 2
    {
        return Err("suppressed response did not preserve the one completed mutation");
    }
    confirm_replication(run, position, address).await?;
    let restored = run.admit().await;
    if restored != Reply::Applied(position, address) || controller.hooks.calls.load(Relaxed) != 2 {
        eprintln!("C10 reconciliation reply: {restored:?}");
        return Err("same-address reconciliation did not return applied watermark");
    }
    let mut conflict = address;
    conflict.set_port(if address.port() == u16::MAX {
        address.port() - 1
    } else {
        address.port() + 1
    });
    let response = run.conflicting_admission(conflict).await;
    if response != Reply::AddressConflict
        || controller.hooks.calls.load(Relaxed) != 2
        || controller.hooks.requests.load(Relaxed) != 5
    {
        eprintln!("C10 conflicting-address reply: {response:?}");
        return Err("conflicting address did not refuse before SDK");
    }
    Ok(position)
}

async fn campaign(
    issuer: &Job,
    observer: &mut Observer,
    layout: &Layout,
    [peer, enrollment]: [SocketAddr; 2],
    exchange: &Arc<Observation>,
    learner: &mut Option<Run>,
) -> Result<()> {
    ready(issuer, observer, Status::FounderReady).await?;
    let (controller, root) = {
        let state = issuer.state();
        (
            state
                .admission
                .clone()
                .ok_or("issuer admission route unavailable")?,
            state
                .owner
                .as_ref()
                .ok_or("issuer owner missing")?
                .read("ca.der", 4096)
                .map_err(|_| "issuer CA unavailable")?,
        )
    };
    controller.hooks.active.store(true, Relaxed);
    let (join, address) = mint_and_join(issuer, layout, enrollment, &root, exchange).await?;
    *learner = Some(
        learner::start_published(join, Params::default(), Some(HostedCatchUp(())))
            .map_err(|_| "consume retained publication")?,
    );
    let run = learner.as_mut().unwrap();
    ready(run.job, &mut run.observer, Status::LearnerReady).await?;
    run.set_admission_route(peer)
        .map_err(|_| "explicit issuer route refused")?;
    if run.admission_progress() != Some((Phase::Bootstrap, None)) {
        return Err("learner startup did not begin with virgin history");
    }

    let advanced = stale_tail(issuer, run, &root, &controller).await?;
    let position = complete(run, &controller, address, advanced).await?;
    let matched = timeout(OBSERVE, async {
        loop {
            if let Some((committed, Some(matched))) = controller.progress(run.binding().0.get())
                && covers(matched, position)
                && covers(committed, position)
            {
                break (committed, matched);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "issuer match observation elapsed")?;
    if issuer_membership(issuer).await?.0 != position {
        return Err("issuer did not retain exact admission membership");
    }
    let retained = retained_node(issuer).await?;
    let node = retained.lock().await;
    let rows = stored(&node, position.index + 2)?;
    let found = rows
        .iter()
        .filter(|(_, entry)| matches!(entry, StoredEntry::Enrollment(Command::Found { .. })))
        .count();
    let minted = rows
        .iter()
        .filter(|(_, entry)| matches!(entry, StoredEntry::Enrollment(Command::Mint { .. })))
        .count();
    let redeemed: Vec<_> = rows
        .iter()
        .filter(|(_, entry)| matches!(entry, StoredEntry::Enrollment(Command::Redeem { .. })))
        .map(|(p, _)| *p)
        .collect();
    let membership: Vec<_> = rows
        .iter()
        .filter(|(_, entry)| matches!(entry, StoredEntry::Membership(..)))
        .map(|(p, _)| *p)
        .collect();
    if found != 1
        || minted != 2
        || redeemed != [run.original()]
        || membership.len() != 2
        || membership[1] != position
        || rows.last().map(|(p, _)| *p) != Some(position)
    {
        return Err("issuer durable invocation inventory differed");
    }
    eprintln!(
        "C10 original E={:?}, stale advance={advanced:?}, applied L={position:?}, commit/match={matched:?}",
        run.original()
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn retained_learner_self_admits_once() {
    let [issuer, learner_layout] = std::array::from_fn(|_| Layout::new());
    let (plan, [peer, enrollment]) =
        plan(&issuer, "create-founder", private_ip().unwrap()).unwrap();
    let issuer_state = plan.state.clone();
    let intent = StartIntent {
        mode: plan.mode,
        cluster: CLUSTER,
        node: FOUNDER,
        peer,
        enrollment,
    };
    let [wrap, staging] = custody(&issuer).unwrap();
    let (job, mut observer, supervisor) = dispatch_founder(
        plan,
        intent,
        Params::default(),
        wrap,
        staging,
        HostedFixture(()),
    )
    .expect("C10 retained founder dispatch");

    let exchange = Arc::new(Observation::default());
    let mut learner = None;
    let first = campaign(
        job,
        &mut observer,
        &learner_layout,
        [peer, enrollment],
        &exchange,
        &mut learner,
    )
    .await;
    if let Err(reason) = first {
        eprintln!("C10 first refusal before cleanup: {reason}");
        exchange.release.notify_one();
    }
    let learner_close = match learner {
        Some(learner) => close_run(learner, false).await.map(|_| ()),
        None => Ok(()),
    };
    if !job.state().close_requested {
        drop(job.request_close());
    }
    let issuer_closed = matches!(timeout(CLEANUP, supervisor).await, Ok(Ok(())))
        && matches!(
            observer.status(),
            Status::Clean | Status::ClosedAfterFounderRefusal
        );
    let issuer_close = if issuer_closed {
        check_locks(&issuer_state, false)
    } else {
        eprintln!(
            "C10 frozen issuer close refused: {:?} {:?}",
            observer.status(),
            job.state().close_failure
        );
        Err("issuer close unproven")
    };
    eprintln!("C10 owned close results: learner={learner_close:?}, issuer={issuer_close:?}");
    first.expect("C10 first refusal");
    learner_close.expect("C10 learner close");
    issuer_close.expect("C10 issuer close");
    eprintln!(
        "C10 observed two Engine starts and owned closes, one Found, two fixture Mints, one AnchorQuery/Redeem exchange, five admission requests, two guarded AddNodes calls, one definite stale-tail no-write refusal and one new membership entry"
    );
}

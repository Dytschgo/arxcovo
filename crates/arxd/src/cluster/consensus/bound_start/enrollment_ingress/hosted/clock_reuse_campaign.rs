//! C15: one retained issuer, real ingress and a per-instance controlled clock.
//! Exact replay uses two genuine same-code continuations (R8). Expiry uses only
//! this issuer's injected test source. Destinations publish only.
use super::*;
use crate::{
    cluster::{
        consensus::{anchor_query::AnchorQueryError, checked_clock::TestShift},
        enrollment::{
            CODE_TTL_MS, CodeId, RequestId, SKEW_MS,
            durable_join::{self, Destination},
            session::SOCKET_BUDGET,
            transfer::{self, LocalRoute},
        },
        pki::preproof::{ExchangeError, anchor},
        state_store::StateStore,
    },
    service::cluster_owner::{Operator, Paste, Reply as OwnerReply},
};
use std::{
    path::{Path, PathBuf},
    sync::Weak,
};

const CASE: &str = "C15";
const LOG_WINDOW: u64 = 32;
// Cross the conservative usability edge by one second, far below 2*SKEW_MS.
const EDGE_MARGIN_MS: u64 = 1_000;
// Mirrors session.rs WORK_BUDGET: Redeem work is admitted only if it fits.
const SERVER_WORK_RESERVE: Duration = Duration::from_secs(5);

fn private_dirs(layout: &Layout, names: [&str; 2]) -> [PathBuf; 2] {
    names.map(|name| {
        let path = layout.root.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Found,
    Mint {
        code: CodeId,
        proposed_at_ms: u64,
    },
    Redeem {
        request: RequestId,
        code: CodeId,
        key: KeyDigest,
    },
    Membership,
}

struct Inventory {
    rows: Vec<(LogPos, Row)>,
    applied: Option<LogPos>,
    next_node: NodeId,
    committed_ms: u64,
}

impl Inventory {
    fn redeems(&self) -> Vec<(LogPos, RequestId, CodeId, KeyDigest)> {
        self.rows
            .iter()
            .filter_map(|(at, row)| match *row {
                Row::Redeem { request, code, key } => Some((*at, request, code, key)),
                _ => None,
            })
            .collect()
    }

    fn mints(&self) -> Vec<(LogPos, CodeId, u64)> {
        self.rows
            .iter()
            .filter_map(|(at, row)| match *row {
                Row::Mint {
                    code,
                    proposed_at_ms,
                } => Some((*at, code, proposed_at_ms)),
                _ => None,
            })
            .collect()
    }

    fn kinds(&self) -> Vec<(&'static str, LogPos)> {
        self.rows
            .iter()
            .map(|(at, row)| {
                let kind = match row {
                    Row::Found => "Found",
                    Row::Mint { .. } => "Mint",
                    Row::Redeem { .. } => "Redeem",
                    Row::Membership => "Membership",
                };
                (kind, *at)
            })
            .collect()
    }
}

fn read_rows(log: &mut VoteLogStore, cluster: ClusterId) -> Result<Vec<(LogPos, Row)>, String> {
    let state = log.log_state().map_err(|e| format!("{e:?}"))?;
    if state.purged.is_some() {
        return Err("retained log purged".into());
    }
    if state.last.is_some_and(|last| last.index >= LOG_WINDOW) {
        return Err("retained log exceeds bounded inventory".into());
    }
    let mut rows = Vec::new();
    for (at, entry) in log
        .entries(0, LOG_WINDOW, LOG_WINDOW as usize)
        .map_err(|e| format!("{e:?}"))?
    {
        let row = match entry {
            StoredEntry::Enrollment(Command::Found { cluster: c, .. }) if c == cluster => {
                Row::Found
            }
            StoredEntry::Enrollment(Command::Mint {
                cluster: c,
                code,
                proposed_at_ms,
                ..
            }) if c == cluster => Row::Mint {
                code,
                proposed_at_ms,
            },
            StoredEntry::Enrollment(Command::Redeem {
                cluster: c,
                request,
                code,
                key,
                ..
            }) if c == cluster => Row::Redeem { request, code, key },
            StoredEntry::Enrollment(_) => return Err("unexpected enrollment command".into()),
            StoredEntry::Membership(_) => Row::Membership,
            StoredEntry::Blank => continue,
        };
        rows.push((at, row));
    }
    Ok(rows)
}

fn inventory(node: &ConsensusNode, cluster: ClusterId) -> Result<Inventory, String> {
    let log = node.log_weak.upgrade().ok_or("retained log unavailable")?;
    let rows = read_rows(&mut *log.lock().map_err(|_| "log lock poisoned")?, cluster)?;
    let store = node.sm.as_ref().ok_or("applied store unavailable")?;
    let store = store.0.lock().map_err(|_| "applied store lock poisoned")?;
    if store.is_poisoned() {
        return Err("applied store poisoned".into());
    }
    Ok(Inventory {
        rows,
        applied: store.applied(),
        next_node: store.state().next_node_id(),
        committed_ms: store.state().committed_ms(),
    })
}

type RetainedStores = (Weak<Mutex<VoteLogStore>>, Weak<Mutex<StateStore>>);

#[derive(Default)]
struct Evidence {
    phase: &'static str,
    owner_mints: usize,
    original: Option<LogPos>,
    replay: Option<LogPos>,
    shift: Option<TestShift>,
    stores: Option<RetainedStores>,
    cluster: ClusterId,
}

impl Evidence {
    fn record_before_close(&self, job: &Job, observed: &Observation) {
        eprintln!(
            "{CASE} pre-close: phase={}, issuer={:?}, owner Mints={}, actual AnchorQuery={}, Redeem={}, original E={:?}, replay log ID={:?}, test shift={:?}",
            self.phase,
            job.observe().status(),
            self.owner_mints,
            observed.anchor_calls.load(Relaxed),
            observed.redeem_calls.load(Relaxed),
            self.original,
            self.replay,
            self.shift,
        );
        let Some((log, state)) = &self.stores else {
            eprintln!("{CASE} pre-close retained stores were not captured");
            return;
        };
        // Bounded, non-waiting reads; a busy store is reported, never awaited.
        match log.upgrade().map(|log| match log.try_lock() {
            Ok(mut log) => read_rows(&mut log, self.cluster),
            Err(_) => Err("log busy or poisoned".into()),
        }) {
            Some(Ok(rows)) => eprintln!(
                "{CASE} pre-close retained log: {:?}",
                Inventory {
                    rows,
                    applied: None,
                    next_node: NodeId::new(1).unwrap(),
                    committed_ms: 0,
                }
                .kinds()
            ),
            Some(Err(error)) => eprintln!("{CASE} pre-close retained log unavailable: {error}"),
            None => eprintln!("{CASE} pre-close retained log dropped"),
        }
        match state.upgrade().and_then(|state| {
            state
                .try_lock()
                .ok()
                .map(|s| (s.is_poisoned(), s.applied(), s.state().next_node_id().get()))
        }) {
            Some(applied) => eprintln!(
                "{CASE} pre-close applied store: poisoned={}, position={:?}, next node={}",
                applied.0, applied.1, applied.2
            ),
            _ => eprintln!("{CASE} pre-close applied store unavailable"),
        }
    }
}

fn preserve_first_failure<E>(first: Result<(), E>, record: impl FnOnce()) -> Result<(), E> {
    if first.is_err() && std::panic::catch_unwind(AssertUnwindSafe(record)).is_err() {
        eprintln!("{CASE} pre-close evidence observation panicked");
    }
    first
}

async fn owner_mint(
    job: &Job,
    installation: [u8; 16],
    evidence: &mut Evidence,
) -> Result<Paste, String> {
    // SAFETY: identity query takes no pointers.
    let uid = unsafe { libc::geteuid() };
    let reply = job
        .mint_owner(Operator::for_campaign(uid), installation)
        .await;
    evidence.owner_mints += 1;
    match reply {
        OwnerReply::Minted(paste) => Ok(paste),
        OwnerReply::Refused => Err(format!("{CASE} {} refused", evidence.phase)),
        OwnerReply::Unknown => Err(format!("{CASE} {} unknown", evidence.phase)),
        OwnerReply::Joined => Err(format!("{CASE} {} unexpected reply", evidence.phase)),
    }
}

fn parse_paste(
    paste: &Paste,
    routes: &[LocalRoute],
    endpoint: SocketAddr,
    root: &[u8],
) -> Result<JoinCode, String> {
    let text = std::str::from_utf8(paste.bytes()).map_err(|_| "owner paste encoding")?;
    let parsed = transfer::parse(text, routes).map_err(|_| "owner paste refused")?;
    if parsed.endpoint != endpoint || parsed.root != root {
        return Err("owner paste names another issuer".into());
    }
    Ok(parsed.code)
}

fn verify_leaf(
    leaf: &[u8],
    root: &[u8],
    cluster: ClusterId,
    node: NodeId,
    key: KeyDigest,
) -> Result<(), String> {
    let (rest, leaf) = X509Certificate::from_der(leaf).map_err(|_| "leaf DER")?;
    let (_, ca) = X509Certificate::from_der(root).map_err(|_| "root DER")?;
    if !rest.is_empty() || leaf.verify_signature(Some(ca.public_key())).is_err() {
        return Err("leaf not signed by the retained CA".into());
    }
    if KeyDigest::of_spki(leaf.public_key().raw).ok() != Some(key) {
        return Err("leaf key differs from the original CSR".into());
    }
    let names = leaf
        .subject_alternative_name()
        .ok()
        .flatten()
        .ok_or("leaf SAN missing")?;
    if names.value.general_names.as_slice() != [GeneralName::URI(&node_uri(cluster, node))] {
        return Err("leaf names another node".into());
    }
    Ok(())
}

fn expect_counts(
    observed: &Observation,
    anchors: usize,
    redeems: usize,
    stage: &str,
) -> Result<(), String> {
    let actual = (
        observed.anchor_calls.load(Relaxed),
        observed.redeem_calls.load(Relaxed),
    );
    if actual != (anchors, redeems) {
        return Err(format!(
            "{CASE} {stage}: AnchorQuery/Redeem dispatches {actual:?}, expected ({anchors}, {redeems})"
        ));
    }
    Ok(())
}

fn expect_anchor(
    observed: &Observation,
    index: usize,
    expected: &Result<Vec<u8>, AnchorQueryError>,
    stage: &str,
) -> Result<(), String> {
    let outcomes = observed.anchor_outcomes.lock().unwrap();
    match outcomes.get(index) {
        Some(actual) if actual == expected => Ok(()),
        Some(Ok(_)) => Err(format!(
            "{CASE} {stage}: actual AnchorQuery returned voters"
        )),
        Some(Err(error)) => Err(format!("{CASE} {stage}: actual AnchorQuery {error:?}")),
        None => Err(format!("{CASE} {stage}: AnchorQuery outcome missing")),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn retained_issuer_replays_exact_request_and_refuses_controlled_expiry() {
    let layout = Layout::new();
    let mut plan = layout.plan("create-founder");
    let route = UdpSocket::bind("0.0.0.0:0").unwrap();
    route.connect("192.0.2.1:9").unwrap(); // No datagram is sent.
    let ip = route.local_addr().unwrap().ip();
    let peer = StdListener::bind(SocketAddr::new(ip, 0)).unwrap();
    let enrollment = StdListener::bind(SocketAddr::new(ip, 0)).unwrap();
    let peer_addr = peer.local_addr().unwrap();
    let endpoint = enrollment.local_addr().unwrap();
    plan.peer = peer_addr.to_string();
    plan.enrollment = endpoint.to_string();
    let cluster = [1; 16];
    let intent = StartIntent {
        mode: plan.mode,
        node: FOUNDER,
        cluster,
        peer: peer_addr,
        enrollment: endpoint,
    };
    let [wrap, staging] = private_dirs(&layout, ["wrap", "custody"]);
    drop((peer, enrollment));

    let (job, mut observer, supervisor) = dispatch_founder(
        plan,
        intent,
        Params::default(),
        wrap,
        staging,
        HostedFixture(()),
    )
    .expect("C15 retained founder dispatch");
    let observed = Arc::new(Observation::default());
    let mut evidence = Evidence {
        cluster,
        ..Evidence::default()
    };
    let first = AssertUnwindSafe(drive(
        job,
        &mut observer,
        &observed,
        &mut evidence,
        endpoint,
        ip,
        cluster,
    ))
    .catch_unwind()
    .await;

    let first = preserve_first_failure(latch_attempt(CASE, first), || {
        evidence.record_before_close(job, &observed);
    });
    let (first, close) = finish_attempt(
        first,
        &mut None,
        || observed.release.notify_one(),
        || observe_or_request_close(job, || {}),
        async |observer| observe_close(supervisor, observer).await,
    )
    .await;
    {
        let state = job.state();
        eprintln!(
            "{CASE} owned job: engine_may_exist={}, close_dispatched={}; owner Mints={}, actual AnchorQuery={}, Redeem={}",
            state.engine_may_exist,
            state.close_dispatched,
            evidence.owner_mints,
            observed.anchor_calls.load(Relaxed),
            observed.redeem_calls.load(Relaxed)
        );
    }
    if !matches!(
        close,
        CloseObservation::Joined(Status::Clean | Status::ClosedAfterFounderRefusal)
    ) {
        let _ = layout.root.keep();
    }
    require_attempt(CASE, first, &close, job);
}

#[allow(clippy::too_many_lines)]
async fn drive(
    job: &Job,
    observer: &mut Observer,
    observed: &Arc<Observation>,
    evidence: &mut Evidence,
    endpoint: SocketAddr,
    ip: std::net::IpAddr,
    cluster: ClusterId,
) -> Result<(), String> {
    evidence.phase = "founder";
    observe_retained_status(observer, Status::FounderReady, job).await?;
    eprintln!("{CASE} founder ready; issuer Engine started");
    let (node, root, clock, time, installation) = {
        let state = job.state();
        let ready = state.issuer.as_ref().ok_or("issuer missing")?;
        (
            state.node.as_ref().ok_or("node missing")?.clone(),
            state
                .owner
                .as_ref()
                .ok_or("owner missing")?
                .read("ca.der", 4096)
                .map_err(|_| "root unreadable")?,
            ready.clock.clone(),
            // The issuer's own checked TLS clock: server, Query and destination
            // clients share this one controlled instance.
            ready.tls.time.clone(),
            state
                .founder
                .as_ref()
                .and_then(|founder| founder.installation_id())
                .ok_or("installation unavailable")?,
        )
    };
    let (voters, first_free, initial) = {
        let node = node.lock().await;
        evidence.stores = Some((node.log_weak.clone(), node.sm_weak.clone()));
        let store = node
            .sm
            .as_ref()
            .ok_or("applied store missing")?
            .0
            .lock()
            .unwrap();
        let membership = store.membership().ok_or("membership missing")?.clone();
        let voters = BoundVoters::observe(
            store.state(),
            &WireMembership::decode(membership.1.bytes()).map_err(|_| "membership decode")?,
        )
        .map_err(|_| "voters unavailable")?;
        (voters, store.state().next_node_id(), membership)
    };
    let membership = WireMembership::decode(initial.1.bytes()).map_err(|_| "membership decode")?;
    job.start_enrollment(Some(observed.clone()))
        .map_err(|reason| format!("{CASE} issuer ingress: {reason:?}"))?;
    let routes = [LocalRoute {
        network: ip,
        prefix: 32,
    }];
    let limits = SocketLimits::default;

    // Code A: one owner Mint, one held continuation, one ordinary destination.
    evidence.phase = "owner Mint A";
    let paste_a = owner_mint(job, installation, evidence).await?;
    let code_a = parse_paste(&paste_a, &routes, endpoint, &root)?;
    expect_counts(observed, 0, 0, "after Mint A")?;

    // Prepare destination A before opening the held continuation so that only
    // the ordinary join itself runs inside the held socket budget.
    evidence.phase = "A destination setup";
    let destination_a = Layout::new();
    let [wrap, staging] = private_dirs(&destination_a, ["wrap", "staging"]);
    let destination = Destination {
        plan: destination_a.plan("join"),
        wrap,
        staging,
    };
    let paste_text = std::str::from_utf8(paste_a.bytes()).map_err(|_| "owner paste encoding")?;

    evidence.phase = "A held continuation Query";
    let held_at = Instant::now();
    let held = anchor(endpoint, &code_a, &root, time.clone(), limits())
        .await
        .map_err(|e| format!("{CASE} held continuation Query: {e:?}"))?;
    let anchored_ms = held_at.elapsed().as_millis();
    expect_counts(observed, 1, 0, "held continuation")?;
    expect_anchor(observed, 0, &Ok(voters.encode()), "held continuation")?;

    evidence.phase = "A destination";
    let (_join_a, mut result_a) =
        durable_join::start_retained(destination, paste_text, &routes, time.clone())
            .map_err(|status| format!("{CASE} destination A dispatch: {status:?}"))?;
    drop(paste_a);
    // The destination owns this reservation beyond the observer and the campaign.
    let _destination_a = destination_a.root.keep();
    tokio::select! {
        biased;
        result = observe_enroll(observed) => result?,
        status = result_a.terminal() => {
            return Err(format!("{CASE} destination A ended before enrollment witness: {status:?}"));
        }
    }
    expect_counts(observed, 2, 1, "original Redeem")?;
    expect_anchor(observed, 1, &Ok(voters.encode()), "destination A Query")?;
    let (request, csr) = observed.redeem_intents.lock().unwrap()[0].clone();
    let key = crate::cluster::pki::csr::ProvenCsr::parse(&csr)
        .map_err(|_| "dispatched CSR")?
        .digest();
    // Verify allocation and E while the issuer still holds the original reply.
    let original = {
        let node = node.lock().await;
        let inventory = inventory(&node, cluster)?;
        let [(at, r, c, k)] = inventory.redeems()[..] else {
            return Err(format!("{CASE} original Redeem not uniquely applied"));
        };
        let store = node.sm.as_ref().unwrap().0.lock().unwrap();
        if (r, c, k) != (request, code_a.code_id(), key)
            || inventory.applied != Some(at)
            || inventory.next_node.get() != first_free.get() + 1
            || store.state().bound_key(cluster, first_free) != Ok(key)
        {
            return Err(format!(
                "{CASE} original Redeem differs from the dispatched attempt"
            ));
        }
        at
    };
    evidence.original = Some(original);
    let original_ms = held_at.elapsed().as_millis();
    observed.release.notify_one();

    // R8: once the original Redeem is committed, applied and its E verified and
    // released, the already-authorized second continuation sends the same intent
    // once. The server admits Redeem work only while its full work budget still
    // fits the socket budget; the client-side age below overstates server age.
    evidence.phase = "A exact replay";
    let age = held_at.elapsed();
    let window = SOCKET_BUDGET - SERVER_WORK_RESERVE;
    eprintln!(
        "{CASE} R8 window (ms since held Query start): anchored +{anchored_ms}, original verified +{original_ms}, replay send +{}; admission window {}, margin {}",
        age.as_millis(),
        window.as_millis(),
        i128::try_from(window.as_millis()).unwrap_or(i128::MAX)
            - i128::try_from(age.as_millis()).unwrap_or(i128::MAX),
    );
    if age + SERVER_WORK_RESERVE >= SOCKET_BUDGET {
        return Err(format!(
            "{CASE} replay window elapsed before the second Redeem"
        ));
    }
    let mut replay = Box::pin(held.redeem(request, &csr));
    tokio::select! {
        biased;
        result = observe_enroll(observed) => result?,
        outcome = &mut replay => {
            return Err(format!("{CASE} replay ended before dispatch witness: {:?}", outcome.err()));
        }
    }
    expect_counts(observed, 2, 2, "replay Redeem")?;
    if observed.redeem_intents.lock().unwrap().get(1) != Some(&(request, csr.clone())) {
        return Err(format!(
            "{CASE} replay did not dispatch the original intent"
        ));
    }
    let replay_at = {
        let node = node.lock().await;
        let inventory = inventory(&node, cluster)?;
        let [(first, ..), (second, r, c, k)] = inventory.redeems()[..] else {
            return Err(format!("{CASE} replay Redeem not applied once"));
        };
        if first != original
            || (r, c, k) != (request, code_a.code_id(), key)
            || second.index <= original.index
            || inventory.next_node.get() != first_free.get() + 1
        {
            return Err(format!(
                "{CASE} replay allocated or differs from the original"
            ));
        }
        second
    };
    evidence.replay = Some(replay_at);
    observed.release.notify_one();
    let completed = replay
        .await
        .map_err(|e| format!("{CASE} replay result: {e:?}"))?;
    let (node_id, e, leaf) = completed.reply().parts();
    if (node_id, e) != (first_free, original) {
        return Err(format!(
            "{CASE} replay returned node {} at {e:?}",
            node_id.get()
        ));
    }
    verify_leaf(leaf, &root, cluster, node_id, key)?;
    let status = timeout(Duration::from_secs(10), result_a.terminal())
        .await
        .map_err(|_| "destination A terminal observation elapsed")?;
    if status != durable_join::Status::Published {
        return Err(format!("{CASE} destination A terminal: {status:?}"));
    }
    eprintln!(
        "{CASE} exact replay: original E {original:?}, replay log ID {replay_at:?}, node {}, same key, one allocation; destination A published once",
        first_free.get()
    );

    // A fresh attempt on the consumed code is refused at Query; no proof leaves.
    evidence.phase = "A consumed reconnect";
    let other = rcgen::KeyPair::generate().map_err(|_| "reconnect key")?;
    let other_csr = rcgen::CertificateParams::default()
        .serialize_request(&other)
        .map_err(|_| "reconnect CSR")?;
    let mut other_request = [7; 16];
    if other_request == request {
        other_request[0] ^= 1;
    }
    let reconnect = exchange(
        endpoint,
        &code_a,
        &root,
        time.clone(),
        other_request,
        other_csr.der(),
        limits(),
    )
    .await;
    if !matches!(reconnect, Err(ExchangeError::Refused)) {
        return Err(format!("{CASE} consumed code reached the continuation"));
    }
    expect_counts(observed, 3, 2, "consumed reconnect")?;
    expect_anchor(
        observed,
        2,
        &Err(AnchorQueryError::Refused),
        "consumed reconnect",
    )?;

    // Code B: mint, then advance only this issuer instance across its edge.
    evidence.phase = "owner Mint B";
    let paste_b = owner_mint(job, installation, evidence).await?;
    let code_b = parse_paste(&paste_b, &routes, endpoint, &root)?;
    drop(paste_b);
    let (expires_b, pre, applied_b) = {
        let node = node.lock().await;
        let inventory = inventory(&node, cluster)?;
        if inventory.mints().last().map(|m| m.1) != Some(code_b.code_id()) {
            return Err(format!("{CASE} Mint B is not the last applied Mint"));
        }
        // Mint B was the last applied enrollment command: committed_ms is its
        // effective time and the committed expiry follows from the fixed TTL.
        let expires = inventory
            .committed_ms
            .checked_add(CODE_TTL_MS)
            .ok_or("expiry overflow")?;
        let store = node.sm.as_ref().unwrap().0.lock().unwrap();
        let pre = clock
            .sample_ms()
            .map_err(|e| format!("{CASE} pre-edge clock: {e}"))?;
        store
            .state()
            .pending_voters(cluster, code_b.code_id(), pre, Some(&membership))
            .map_err(|_| format!("{CASE} B was not pending and usable before the edge"))?;
        (expires, pre, inventory.applied)
    };
    let edge = expires_b.checked_sub(2 * SKEW_MS).ok_or("edge underflow")?;
    evidence.phase = "B controlled advance";
    let shift = clock
        .advance_for_test(edge.saturating_sub(pre) + EDGE_MARGIN_MS)
        .map_err(|e| format!("{CASE} test source: {e}"))?;
    evidence.shift = Some(shift);
    let post = clock
        .sample_ms()
        .map_err(|e| format!("{CASE} advanced clock refused: {e}"))?;
    if post < edge || post >= expires_b {
        return Err(format!("{CASE} advance missed the conservative window"));
    }
    {
        let node = node.lock().await;
        let store = node.sm.as_ref().unwrap().0.lock().unwrap();
        if store.applied() != applied_b
            || store
                .state()
                .pending_voters(cluster, code_b.code_id(), post, Some(&membership))
                .is_ok()
        {
            return Err(format!(
                "{CASE} B policy did not change at the advanced sample"
            ));
        }
    }
    eprintln!(
        "{CASE} B edge (relative ms): pre {}, post {}, nominal expiry margin {}, test shift {shift:?}",
        i128::from(pre) - i128::from(edge),
        post - edge,
        expires_b - post,
    );

    evidence.phase = "B expired Query";
    let expired = anchor(endpoint, &code_b, &root, time.clone(), limits()).await;
    if !matches!(expired, Err(ExchangeError::Refused)) {
        return Err(format!("{CASE} expired B reached a continuation"));
    }
    // Dispatch proves healthy TLS and framing. Refused (not Unavailable or Busy)
    // is the checked policy; the independent readback above attributes it.
    expect_counts(observed, 4, 2, "expired B")?;
    expect_anchor(observed, 3, &Err(AnchorQueryError::Refused), "expired B")?;
    let healthy = clock
        .sample_ms()
        .map_err(|e| format!("{CASE} clock after B refusal: {e}"))?;
    if healthy < post {
        return Err(format!("{CASE} clock moved backwards after B"));
    }

    // Code C: fresh positive on the same still-healthy issuer.
    evidence.phase = "owner Mint C";
    let paste_c = owner_mint(job, installation, evidence).await?;
    let code_c = parse_paste(&paste_c, &routes, endpoint, &root)?;
    {
        let node = node.lock().await;
        let inventory = inventory(&node, cluster)?;
        match inventory.mints().last() {
            Some((_, code, proposed)) if *code == code_c.code_id() && *proposed >= post => {}
            _ => {
                return Err(format!(
                    "{CASE} Mint C did not use the advanced checked sample"
                ));
            }
        }
    }
    evidence.phase = "C destination";
    let destination_c = Layout::new();
    let [wrap, staging] = private_dirs(&destination_c, ["wrap", "staging"]);
    let (_join_c, mut result_c) = durable_join::start_retained(
        Destination {
            plan: destination_c.plan("join"),
            wrap,
            staging,
        },
        std::str::from_utf8(paste_c.bytes()).map_err(|_| "owner paste encoding")?,
        &routes,
        time.clone(),
    )
    .map_err(|status| format!("{CASE} destination C dispatch: {status:?}"))?;
    drop(paste_c);
    let _destination_c = destination_c.root.keep();
    tokio::select! {
        biased;
        result = observe_enroll(observed) => result?,
        status = result_c.terminal() => {
            return Err(format!("{CASE} destination C ended before enrollment witness: {status:?}"));
        }
    }
    expect_counts(observed, 5, 3, "positive C")?;
    expect_anchor(observed, 4, &Ok(voters.encode()), "positive C")?;
    let (request_c, csr_c) = observed.redeem_intents.lock().unwrap()[2].clone();
    let key_c = crate::cluster::pki::csr::ProvenCsr::parse(&csr_c)
        .map_err(|_| "dispatched CSR C")?
        .digest();
    observed.release.notify_one();
    let status = timeout(Duration::from_secs(10), result_c.terminal())
        .await
        .map_err(|_| "destination C terminal observation elapsed")?;
    if status != durable_join::Status::Published {
        return Err(format!("{CASE} destination C terminal: {status:?}"));
    }

    evidence.phase = "final inventory";
    let node = node.lock().await;
    let inventory = inventory(&node, cluster)?;
    let second = NodeId::new(first_free.get() + 1).ok_or("node overflow")?;
    let rows = &inventory.rows;
    let enrollment: Vec<_> = rows
        .iter()
        .filter(|(_, row)| *row != Row::Membership)
        .collect();
    let membership_rows: Vec<_> = rows
        .iter()
        .filter(|(_, row)| *row == Row::Membership)
        .collect();
    let shape_ok = matches!(
        enrollment.as_slice(),
        [
            (_, Row::Found),
            (_, Row::Mint { code: a, .. }),
            (o, Row::Redeem { request: r1, code: c1, key: k1 }),
            (p, Row::Redeem { request: r2, code: c2, key: k2 }),
            (_, Row::Mint { code: b, .. }),
            (_, Row::Mint { code: c, .. }),
            (q, Row::Redeem { request: r3, code: c3, key: k3 }),
        ] if *a == code_a.code_id()
            && (*o, *r1, *c1, *k1) == (original, request, code_a.code_id(), key)
            && (*p, *r2, *c2, *k2) == (replay_at, request, code_a.code_id(), key)
            && *b == code_b.code_id()
            && *c == code_c.code_id()
            && (*r3, *c3, *k3) == (request_c, code_c.code_id(), key_c)
            && Some(*q) == inventory.applied
    );
    let store = node.sm.as_ref().unwrap().0.lock().unwrap();
    if !shape_ok
        || membership_rows.len() != 1
        || membership_rows[0].0 != initial.0
        || store.membership() != Some(&initial)
        || inventory.next_node.get() != first_free.get() + 2
        || store.state().bound_key(cluster, first_free) != Ok(key)
        || store.state().bound_key(cluster, second) != Ok(key_c)
    {
        return Err(format!(
            "{CASE} final inventory differs: {:?}",
            inventory.kinds()
        ));
    }
    eprintln!(
        "{CASE} retained log {:?}; 3 owner Mints, 5 AnchorQuery and 3 Redeem dispatches, 7 enrollment proposals, 2 allocations, 1 exact replay at original E {original:?} (replay log ID {replay_at:?}), 0 membership changes after initialization, destinations publish only, no destination Engine",
        inventory.kinds()
    );
    Ok(())
}

// A13: the test-only Operator constructor has one cfg-gated definition and one
// caller, this campaign's owner Mint. The needle is assembled so this guard's
// own source never matches it.
const CONSTRUCTOR: &str = concat!("for", "_campaign");
const OWNER_FILE: &str = "service/cluster_owner.rs";
const CAMPAIGN_FILE: &str =
    "cluster/consensus/bound_start/enrollment_ingress/hosted/clock_reuse_campaign.rs";

fn compact(source: &str) -> String {
    source.chars().filter(|c| !c.is_whitespace()).collect()
}

fn constructor_reference_allowed(relative: &str, source: &str) -> bool {
    let references = source.matches(CONSTRUCTOR).count();
    let source = compact(source);
    match relative {
        OWNER_FILE => {
            let definition = compact(&format!(
                "#[cfg(all(test, target_os = \"linux\"))] pub(crate) fn {CONSTRUCTOR}(uid: u32) -> Self {{ Self(uid) }}"
            ));
            references == 1 && source.matches(&definition).count() == 1
        }
        CAMPAIGN_FILE => {
            let call = compact(&format!(
                ".mint_owner(Operator::{CONSTRUCTOR}(uid), installation)"
            ));
            references == 1 && source.matches(&call).count() == 1
        }
        _ => references == 0,
    }
}

#[test]
fn a13_operator_constructor_has_one_definition_and_one_campaign_caller() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let (mut count, mut owner, mut campaign) = (0, false, false);
    let mut directories = vec![root.clone()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.path().symlink_metadata().unwrap();
            assert!(
                !metadata.file_type().is_symlink(),
                "source inventory follows no links"
            );
            if metadata.is_dir() {
                directories.push(entry.path());
                continue;
            }
            if entry.path().extension().is_none_or(|v| v != "rs") {
                continue;
            }
            count += 1;
            assert!(
                count <= 1024 && metadata.len() <= 1_048_576,
                "source inventory bounds"
            );
            let relative = entry
                .path()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            owner |= relative == OWNER_FILE;
            campaign |= relative == CAMPAIGN_FILE;
            let source = fs::read_to_string(entry.path()).unwrap();
            assert!(
                constructor_reference_allowed(&relative, &source),
                "A13 constructor reference outside the reviewed pair: {relative}"
            );
        }
    }
    assert!(count > 20 && owner && campaign, "crate-wide inventory");

    let call = format!("fn p() {{ Operator::{CONSTRUCTOR}(1); }}");
    let definition = format!(
        "#[cfg(all(test, target_os = \"linux\"))]\npub(crate) fn {CONSTRUCTOR}(uid: u32) -> Self {{\n    Self(uid)\n}}"
    );
    let campaign_call = format!(".mint_owner(Operator::{CONSTRUCTOR}(uid), installation)");
    for (relative, source, allowed) in [
        ("new_module.rs", call.clone(), false),
        ("service/cluster_runtime.rs", call.clone(), false),
        (OWNER_FILE, definition.clone(), true),
        (
            OWNER_FILE,
            definition.replace("all(test, target_os = \"linux\")", "test"),
            false,
        ),
        (OWNER_FILE, format!("{definition}\n{call}"), false),
        (CAMPAIGN_FILE, campaign_call.clone(), true),
        (
            CAMPAIGN_FILE,
            format!("{campaign_call}\n{campaign_call}"),
            false,
        ),
        (CAMPAIGN_FILE, call, false),
        ("new_module.rs", String::from("fn p() {}"), true),
    ] {
        assert_eq!(
            constructor_reference_allowed(relative, &source),
            allowed,
            "A13 guard control for {relative}"
        );
    }
}

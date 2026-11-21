use super::*;
use crate::cluster::{
    consensus::anchor_query::AnchorQueryError,
    enrollment::{
        durable_join::{self, Destination},
        session::{SuppressedReply, TerminalLoss},
        transfer::{self, LocalRoute},
    },
    log_store::LogState,
    pki::{
        ca::fingerprint_of,
        preproof::{ExchangeError, anchor},
    },
    startup_plan::tests::objects,
    state_store::StateStore,
};
use std::{path::Path, sync::Weak};

fn private_dirs(layout: &Layout, names: [&str; 2]) -> [std::path::PathBuf; 2] {
    names.map(|name| {
        let path = layout.root.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    })
}

type FileObservation = (String, u64, u64, bool, u64);

#[derive(Debug, PartialEq, Eq)]
struct DestinationObjects {
    directories: [Vec<FileObservation>; 4],
}

fn retained_destination(path: &Path) -> DestinationObjects {
    let directories =
        ["state", "credentials", "staging", "wrap"].map(|dir| objects(&path.join(dir)));
    for (actual, expected) in directories.iter().zip([
        &["cluster-owner.lock"][..],
        &["identity.json"],
        &["custody-lock", "member-key"],
        &["install-context", "seal-key"],
    ]) {
        assert!(actual.iter().all(|object| object.3));
        assert!(
            actual
                .iter()
                .map(|object| object.0.as_str())
                .eq(expected.iter().copied())
        );
    }
    assert_eq!(
        directories[1][0].4, 0,
        "identity slot must still be a reservation"
    );
    assert!(directories[2][1].4 > 0, "staged member missing");
    assert!(directories[3].iter().all(|object| object.4 > 0));
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.join("state/cluster-owner.lock"))
        .unwrap();
    assert!(matches!(
        lock.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    DestinationObjects { directories }
}

const LOG_WINDOW: u64 = 16;

struct LogReadback {
    state: LogState,
    rows: Vec<(LogPos, StoredEntry)>,
}

fn read_log(log: &mut VoteLogStore) -> Result<LogReadback, String> {
    let state = log.log_state().map_err(|error| format!("{error:?}"))?;
    let rows = log
        .entries(0, LOG_WINDOW, LOG_WINDOW as usize)
        .map_err(|error| format!("{error:?}"))?;
    Ok(LogReadback { state, rows })
}

#[derive(Clone)]
struct IssuerStores {
    log: Weak<Mutex<VoteLogStore>>,
    state: Weak<Mutex<StateStore>>,
}

impl IssuerStores {
    fn from_node(node: &ConsensusNode) -> Self {
        Self {
            log: node.log_weak.clone(),
            state: node.sm_weak.clone(),
        }
    }

    fn from_job(job: &Job) -> Option<Self> {
        let state = job.state.try_lock().ok()?;
        if let Some((log, sm)) = &state.adapters {
            return Some(Self {
                log: Arc::downgrade(&log.0),
                state: Arc::downgrade(&sm.0),
            });
        }
        let node = state.node.as_ref()?.clone();
        drop(state);
        let node = node.try_lock().ok()?;
        Some(Self::from_node(&node))
    }
}

fn read_retained<T, U>(
    store: &Weak<Mutex<T>>,
    read: impl FnOnce(&mut T) -> Result<U, String>,
) -> Result<U, String> {
    let store = store.upgrade().ok_or("store unavailable")?;
    let mut guard = store.try_lock().map_err(|error| match error {
        std::sync::TryLockError::WouldBlock => "store busy",
        std::sync::TryLockError::Poisoned(_) => "store lock poisoned",
    })?;
    read(&mut guard)
}

#[derive(Debug, PartialEq, Eq)]
struct AppliedReadback {
    position: Option<LogPos>,
    next_node: NodeId,
    allocation_delta: Option<u64>,
}

fn read_applied(state: &StateStore, baseline: Option<NodeId>) -> Result<AppliedReadback, String> {
    if state.is_poisoned() {
        return Err("applied store poisoned".into());
    }
    let next_node = state.state().next_node_id();
    Ok(AppliedReadback {
        position: state.applied(),
        next_node,
        allocation_delta: baseline.and_then(|first| next_node.get().checked_sub(first.get())),
    })
}

fn proposal_positions(log: &LogReadback) -> Vec<(&'static str, LogPos)> {
    log.rows
        .iter()
        .filter_map(|(at, row)| {
            let kind = match row {
                StoredEntry::Enrollment(Command::Found { .. }) => "Found",
                StoredEntry::Enrollment(Command::Mint { .. }) => "Mint",
                StoredEntry::Enrollment(Command::Redeem { .. }) => "Redeem",
                StoredEntry::Enrollment(_) => "other enrollment",
                StoredEntry::Membership(_) => "Membership",
                StoredEntry::Blank => return None,
            };
            Some((kind, *at))
        })
        .collect()
}

struct IssuerReadback {
    log: Result<LogReadback, String>,
    applied: Result<AppliedReadback, String>,
}

#[derive(Default)]
struct AttemptEvidence {
    stores: Option<IssuerStores>,
    first_free: Option<NodeId>,
    mint_submitted: bool,
    destination: Option<durable_join::Observer>,
    fresh_query_started: bool,
    loss: Option<Arc<TerminalLoss>>,
}

impl AttemptEvidence {
    fn record_before_close(&mut self, job: &Job, observed: &Observation) {
        let destination = self.destination.as_mut().map(|observer| {
            observer
                .terminal()
                .now_or_never()
                .unwrap_or(durable_join::Status::Joining)
        });
        let suppression = self
            .loss
            .as_ref()
            .and_then(|loss| loss.observed())
            .map(|reply| (reply.node, reply.original));
        eprintln!(
            "J4 pre-close: issuer={:?}, destination={destination:?}, Mint submitted={}, fresh Query started={}, actual AnchorQuery={}, Redeem={}, suppressed={suppression:?}",
            job.observe().status(),
            self.mint_submitted,
            self.fresh_query_started,
            observed.anchor_calls.load(Relaxed),
            observed.redeem_calls.load(Relaxed),
        );
        let Some(readback) = self.read_issuer(job) else {
            eprintln!("J4 pre-close retained log and applied store unavailable");
            return;
        };
        match readback.log {
            Ok(log) => eprintln!(
                "J4 pre-close retained log: window=0..{LOG_WINDOW}, tail={:?}, purged={:?}, entries={:?}",
                log.state.last,
                log.state.purged,
                proposal_positions(&log)
            ),
            Err(error) => eprintln!("J4 pre-close retained log unavailable: {error}"),
        }
        match readback.applied {
            Ok(applied) => eprintln!("J4 pre-close applied store: {applied:?}"),
            Err(error) => eprintln!("J4 pre-close applied store unavailable: {error}"),
        }
    }

    fn read_issuer(&self, job: &Job) -> Option<IssuerReadback> {
        let stores = self
            .stores
            .clone()
            .or_else(|| IssuerStores::from_job(job))?;
        Some(IssuerReadback {
            log: read_retained(&stores.log, read_log),
            applied: read_retained(&stores.state, |state| read_applied(state, self.first_free)),
        })
    }
}

fn preserve_first_failure<E>(first: Result<(), E>, record: impl FnOnce()) -> Result<(), E> {
    if first.is_err() && std::panic::catch_unwind(AssertUnwindSafe(record)).is_err() {
        eprintln!("J4 pre-close evidence observation panicked");
    }
    first
}

fn inventory(
    node: &ConsensusNode,
    code: &JoinCode,
    mint: LogPos,
    suppressed: SuppressedReply,
    first_free: NodeId,
    original_membership: &(LogPos, MembershipBlob),
) -> LogPos {
    let log = node.log_weak.upgrade().expect("J4 retained log");
    let LogReadback { state, rows } = read_log(&mut log.lock().unwrap()).unwrap();
    assert!(state.purged.is_none());
    let last = state.last.expect("J4 nonempty log");
    assert!(last.index < LOG_WINDOW, "J4 log exceeds bounded inventory");
    let key = suppressed.key.expect("J4 accepted CSR key");
    let mut enroll = Vec::new();
    let mut membership = Vec::new();
    for (at, row) in rows {
        match row {
            StoredEntry::Enrollment(command) => {
                match command {
                    Command::Found { cluster, .. } => assert_eq!(cluster, code.cluster()),
                    Command::Mint { code: id, .. } => {
                        assert_eq!((at, id), (mint, code.code_id()));
                    }
                    Command::Redeem {
                        cluster,
                        request,
                        code: id,
                        key: applied_key,
                        anchor,
                        ..
                    } => {
                        assert!(
                            (at, cluster, request, id, applied_key, anchor)
                                == (
                                    suppressed.original,
                                    code.cluster(),
                                    suppressed.request,
                                    code.code_id(),
                                    key,
                                    code.anchor()
                                ),
                            "J4 applied Redeem differs from dispatched attempt"
                        );
                    }
                    _ => panic!("J4 unexpected enrollment command"),
                }
                enroll.push((at, command));
            }
            StoredEntry::Membership(blob) => membership.push((at, blob)),
            StoredEntry::Blank => {}
        }
    }
    let [
        (found, Command::Found { .. }),
        (_, Command::Mint { .. }),
        (_, Command::Redeem { .. }),
    ] = enroll.as_slice()
    else {
        panic!("J4 expected exactly Found, Mint, Redeem");
    };
    assert!(found.index < mint.index && mint.index < suppressed.original.index);
    assert!(membership.as_slice() == std::slice::from_ref(original_membership));

    let store = node.sm.as_ref().unwrap().0.lock().unwrap();
    assert_eq!(store.applied(), Some(suppressed.original));
    assert!(store.membership() == Some(original_membership));
    assert_eq!(first_free.get(), 2);
    assert_eq!(suppressed.node, first_free);
    assert!(store.state().bound_key(code.cluster(), suppressed.node) == Ok(key));
    assert_eq!(store.state().next_node_id().get(), first_free.get() + 1);
    *found
}

#[tokio::test(flavor = "current_thread")]
async fn retained_issuer_lost_reply_leaves_original_destination_spent() {
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
    .expect("J4 retained founder dispatch");
    let observed = Arc::new(Observation::default());
    let mut evidence = AttemptEvidence::default();
    let first = AssertUnwindSafe(async {
        observe_retained_status(&mut observer, Status::FounderReady, job).await?;
        eprintln!("J4 founder ready; issuer Engine started");
        let (node, root, clock, time) = {
            let state = job.state();
            let ready = state.issuer.as_ref().unwrap();
            (
                state.node.as_ref().unwrap().clone(),
                state.owner.as_ref().unwrap().read("ca.der", 4096).unwrap(),
                ready.clock.clone(),
                ready.tls.time.clone(),
            )
        };
        let (voters, code, mint, first_free, membership) = {
            let node = node.lock().await;
            evidence.stores = Some(IssuerStores::from_node(&node));
            let (voters, first_free, membership) = {
                let store = node.sm.as_ref().unwrap().0.lock().unwrap();
                let membership = store.membership().unwrap().clone();
                let voters = BoundVoters::observe(
                    store.state(),
                    &WireMembership::decode(membership.1.bytes()).unwrap(),
                ).unwrap();
                (voters, store.state().next_node_id(), membership)
            };
            evidence.first_free = Some(first_free);
            let code = JoinCode::generate(cluster, fingerprint_of(&root), voters.anchor(cluster), getrandom::fill)
                .map_err(|_| "J4 fixture code creation refused")?;
            let proposed_at_ms = clock.sample_ms().map_err(|_| "J4 fixture Mint clock refused")?;
            evidence.mint_submitted = true;
            let (mint, minted) = node.propose(Command::Mint {
                cluster,
                actor: [3; 16],
                request: [4; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms,
            }).await.map_err(|reason| format!("J4 fixture Mint refused: {reason:?}"))?;
            if !matches!(minted, Some(Outcome::Minted { replay: false, .. })) {
                return Err("J4 fixture Mint did not allocate a code".into());
            }
            (voters, code, mint, first_free, membership)
        };
        eprintln!("J4 fixture Mint applied: {mint:?}");
        job.start_enrollment(Some(observed.clone()))
            .map_err(|reason| format!("J4 issuer ingress: {reason:?}"))?;
        let loss = evidence.loss.insert(job.state().ingress.as_ref().unwrap().arm_terminal_loss()
            .map_err(|_| "J4 listener loss already armed")?);

        let destination = Layout::new();
        let [wrap, staging] = private_dirs(&destination, ["wrap", "staging"]);
        let plan = destination.plan("join");
        let paste = transfer::encode(endpoint, &root, &code).map_err(|_| "J4 fixture paste refused")?;
        let (_join, result) = durable_join::start_retained(
            Destination { plan, wrap, staging },
            paste.expose_for_owner_transfer(),
            &[LocalRoute { network: ip, prefix: 32 }],
            time.clone(),
        ).map_err(|status| format!("J4 destination dispatch: {status:?}"))?;
        let result = evidence.destination.insert(result);
        // The destination owns this reservation beyond the observer and the campaign.
        let destination = destination.root.keep();

        tokio::select! {
            biased;
            result = observe_enroll(&observed) => result?,
            status = result.terminal() => return Err(format!("J4 destination ended before enrollment witness: {status:?}")),
        }
        assert_eq!(observed.anchor_calls.load(Relaxed), 1);
        assert_eq!(observed.redeem_calls.load(Relaxed), 1);
        assert!(*observed.anchor_outcomes.lock().unwrap() == [Ok(voters.encode())]);
        assert!(loss.observed().is_none(), "J4 reply escaped the owned work hold");
        let original = retained_destination(&destination);

        observed.release.notify_one();
        let status = timeout(Duration::from_secs(10), result.terminal())
            .await
            .map_err(|_| "J4 destination terminal observation elapsed")?;
        if status != durable_join::Status::Unconfirmed {
            return Err(format!("J4 destination terminal: {status:?}"));
        }
        let suppressed = loss.observed().ok_or("J4 successful Result was not suppressed")?;
        assert_eq!(retained_destination(&destination), original);

        let found = {
            let node = node.lock().await;
            inventory(&node, &code, mint, suppressed, first_free, &membership)
        };
        eprintln!(
            "J4 first flow unconfirmed; one successful Result suppressed at {:?}, allocated node {}",
            suppressed.original, suppressed.node.get()
        );

        // A new TLS flow sends Query only. The real issuer must refuse before
        // any challenge handoff. No method can spend a second time here.
        evidence.fresh_query_started = true;
        let fresh = anchor(endpoint, &code, &root, time, SocketLimits::default()).await;
        if !matches!(fresh, Err(ExchangeError::Refused)) {
            return Err("J4 consumed code unexpectedly reached the fresh continuation".into());
        }
        assert_eq!(observed.anchor_calls.load(Relaxed), 2);
        assert_eq!(observed.redeem_calls.load(Relaxed), 1);
        assert!(
            *observed.anchor_outcomes.lock().unwrap() ==
            [Ok(voters.encode()), Err(AnchorQueryError::Refused)],
            "J4 fresh Query did not reach the issuer's consumed-code refusal"
        );
        assert_eq!(loss.observed(), Some(suppressed));
        assert_eq!(retained_destination(&destination), original);
        {
            let node = node.lock().await;
            assert_eq!(inventory(&node, &code, mint, suppressed, first_free, &membership), found);
        }
        eprintln!(
            "J4 two client flows; Found {found:?}, Mint {mint:?}, Redeem {:?}; 3 applied enrollment proposals, 1 allocation, 0 membership changes after initialization, 0 successful terminal replies, no destination Engine",
            suppressed.original
        );
        Ok(())
    })
    .catch_unwind()
    .await;

    let first = preserve_first_failure(latch_attempt("J4", first), || {
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
            "J4 owned job: engine_may_exist={}, close_dispatched={}; actual AnchorQuery={}, Redeem={}",
            state.engine_may_exist,
            state.close_dispatched,
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
    require_attempt("J4", first, &close, job);
}

#[tokio::test]
async fn applied_redeem_then_issuance_refusal_keeps_effect_evidence() {
    use crate::cluster::{enrollment::FoundMembers, state_store::Init};

    let layout = Layout::new();
    let dir = layout.root.path().join("state");
    let cluster = [1; 16];
    let first_free = NodeId::new(2).unwrap();
    let mut store = StateStore::open(
        &dir,
        Init {
            cluster,
            first_free,
        },
    )
    .unwrap();
    let position = |index| LogPos {
        term: 4,
        leader: 1,
        index,
    };
    let founder_key = KeyDigest::of_spki(b"J4 synthetic founder").unwrap();
    let membership = WireMembership::new(
        vec![[1].into_iter().collect()],
        [(1, "127.0.0.1:7001".into())].into_iter().collect(),
    )
    .unwrap();
    let mut rows = vec![
        (
            position(0),
            StoredEntry::Membership(MembershipBlob::new(membership.encode().unwrap()).unwrap()),
        ),
        (
            position(1),
            StoredEntry::Enrollment(Command::Found {
                cluster,
                members: FoundMembers::new([(FOUNDER, founder_key)]).unwrap(),
                proposed_at_ms: 10,
            }),
        ),
    ];
    for (at, entry) in &rows {
        store.apply(*at, entry).unwrap();
    }
    let first_free = store.state().next_node_id();
    let anchor = store.state().voter_anchor(&membership).unwrap();
    let code = JoinCode::generate(cluster, [2; 32], anchor, |bytes| {
        bytes.fill(7);
        Ok::<_, ()>(())
    })
    .unwrap();
    let redeemed = position(3);
    rows.extend([
        (
            position(2),
            StoredEntry::Enrollment(Command::Mint {
                cluster,
                actor: [3; 16],
                request: [4; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: 11,
            }),
        ),
        (
            redeemed,
            StoredEntry::Enrollment(Command::redeem(
                cluster,
                [5; 16],
                code.code_id(),
                &code.proof(),
                KeyDigest::of_spki(b"J4 synthetic learner").unwrap(),
                anchor,
                12,
            )),
        ),
    ]);
    for (at, entry) in &rows[2..] {
        store.apply(*at, entry).unwrap();
    }
    let mut log = VoteLogStore::open(&dir, cluster).unwrap();
    log.append(&rows).unwrap();

    let log = Arc::new(Mutex::new(log));
    let state = Arc::new(Mutex::new(store));
    let evidence = AttemptEvidence {
        stores: Some(IssuerStores {
            log: Arc::downgrade(&log),
            state: Arc::downgrade(&state),
        }),
        first_free: Some(first_free),
        ..AttemptEvidence::default()
    };
    let observation = Observation::default();
    observation
        .complete(Err(issue::IssueError::Unavailable))
        .await;
    let job = Job::anchor();
    let mut readback = None;
    let first = preserve_first_failure(observe_enroll(&observation).await, || {
        readback = evidence.read_issuer(job);
    });
    let readback = readback.unwrap();
    assert_eq!(first.unwrap_err(), "actual enroll refused: Unavailable");
    assert_eq!(
        proposal_positions(&readback.log.unwrap()),
        [
            ("Membership", position(0)),
            ("Found", position(1)),
            ("Mint", position(2)),
            ("Redeem", redeemed),
        ]
    );
    assert_eq!(
        readback.applied.unwrap(),
        AppliedReadback {
            position: Some(redeemed),
            next_node: NodeId::new(3).unwrap(),
            allocation_delta: Some(1),
        }
    );

    // A busy retained log cannot hide the independently readable applied effect.
    let _held = log.lock().unwrap();
    let readback = evidence.read_issuer(job).unwrap();
    assert_eq!(readback.log.err().as_deref(), Some("store busy"));
    assert_eq!(readback.applied.unwrap().allocation_delta, Some(1));
}

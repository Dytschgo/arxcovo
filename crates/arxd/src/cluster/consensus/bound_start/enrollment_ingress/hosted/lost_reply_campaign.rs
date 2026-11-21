use super::*;
use crate::cluster::{
    consensus::anchor_query::AnchorQueryError,
    enrollment::{
        durable_join::{self, Destination},
        session::SuppressedReply,
        transfer::{self, LocalRoute},
    },
    pki::{
        ca::fingerprint_of,
        preproof::{ExchangeError, anchor},
    },
    startup_plan::tests::objects,
};
use std::path::Path;

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

fn inventory(
    node: &ConsensusNode,
    code: &JoinCode,
    mint: LogPos,
    suppressed: SuppressedReply,
    first_free: NodeId,
    original_membership: &(LogPos, MembershipBlob),
) -> LogPos {
    let log = node.log_weak.upgrade().expect("J4 retained log");
    let rows = {
        let mut log = log.lock().unwrap();
        let state = log.log_state().unwrap();
        assert!(state.purged.is_none());
        let last = state.last.expect("J4 nonempty log");
        assert!(last.index < 16, "J4 log exceeds bounded inventory");
        log.entries(0, last.index + 1, 16).unwrap()
    };
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
            let (voters, first_free, membership) = {
                let store = node.sm.as_ref().unwrap().0.lock().unwrap();
                let membership = store.membership().unwrap().clone();
                let voters = BoundVoters::observe(
                    store.state(),
                    &WireMembership::decode(membership.1.bytes()).unwrap(),
                ).unwrap();
                (voters, store.state().next_node_id(), membership)
            };
            let code = JoinCode::generate(cluster, fingerprint_of(&root), voters.anchor(cluster), getrandom::fill)
                .map_err(|_| "J4 fixture code creation refused")?;
            let (mint, minted) = node.propose(Command::Mint {
                cluster,
                actor: [3; 16],
                request: [4; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: clock.sample_ms().map_err(|_| "J4 fixture Mint clock refused")?,
            }).await.map_err(|reason| format!("J4 fixture Mint refused: {reason:?}"))?;
            if !matches!(minted, Some(Outcome::Minted { replay: false, .. })) {
                return Err("J4 fixture Mint did not allocate a code".into());
            }
            (voters, code, mint, first_free, membership)
        };
        eprintln!("J4 fixture Mint applied: {mint:?}");
        job.start_enrollment(Some(observed.clone()))
            .map_err(|reason| format!("J4 issuer ingress: {reason:?}"))?;
        let loss = job.state().ingress.as_ref().unwrap().arm_terminal_loss()
            .map_err(|_| "J4 listener loss already armed")?;

        let destination = Layout::new();
        let [wrap, staging] = private_dirs(&destination, ["wrap", "staging"]);
        let plan = destination.plan("join");
        let paste = transfer::encode(endpoint, &root, &code).map_err(|_| "J4 fixture paste refused")?;
        let (_join, mut result) = durable_join::start_retained(
            Destination { plan, wrap, staging },
            paste.expose_for_owner_transfer(),
            &[LocalRoute { network: ip, prefix: 32 }],
            time.clone(),
        ).map_err(|status| format!("J4 destination dispatch: {status:?}"))?;
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

    let (first, close) = finish_attempt(
        latch_attempt("J4", first),
        &mut None,
        || observed.release.notify_one(),
        || job.request_close(),
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

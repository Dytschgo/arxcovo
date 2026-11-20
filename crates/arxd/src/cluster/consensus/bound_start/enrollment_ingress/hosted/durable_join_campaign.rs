use super::*;
use crate::cluster::{
    consensus::anchor_query::AnchorQueryError, enrollment::durable_join::exercise_retained_issuer,
    pki::ca::fingerprint_of,
};

#[tokio::test(flavor = "current_thread")]
async fn retained_issuer_durable_join_and_consumed_code_refusals() {
    let layout = Layout::new();
    let mut plan = layout.plan("create-founder");
    // Resolve the fixture's local non-loopback address without sending a datagram.
    let route = UdpSocket::bind("0.0.0.0:0").unwrap();
    route.connect("192.0.2.1:9").unwrap();
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
    let [wrap, staging] = ["wrap", "custody"].map(|name| {
        let path = layout.root.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    });
    drop(peer);
    drop(enrollment);
    let (job, mut observer, supervisor) = dispatch_founder(
        plan,
        intent,
        Params::default(),
        wrap,
        staging,
        HostedFixture(()),
    )
    .expect("J1 retained founder dispatch");
    wait_for(&mut observer, Status::FounderReady, job).await;
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
    let (voters, code, mint_e) = {
        let node = node.lock().await;
        let voters = {
            let store = node.sm.as_ref().unwrap().0.lock().unwrap();
            let membership = WireMembership::decode(store.membership().unwrap().1.bytes()).unwrap();
            BoundVoters::observe(store.state(), &membership).unwrap()
        };
        let code = JoinCode::generate(
            cluster,
            fingerprint_of(&root),
            voters.anchor(cluster),
            getrandom::fill,
        )
        .unwrap();
        let (mint_e, minted) = node
            .propose(Command::Mint {
                cluster,
                actor: [3; 16],
                request: [4; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: clock.sample_ms().unwrap(),
            })
            .await
            .expect("J1 sole fixture Mint proposal refused");
        assert!(matches!(
            minted,
            Some(Outcome::Minted { replay: false, .. })
        ));
        (voters, code, mint_e)
    };

    let observed = Arc::new(Observation::default());
    job.start_enrollment(Some(observed.clone()))
        .expect("J1 issuer ingress");
    let exercise = exercise_retained_issuer(endpoint, &code, &root, time, &voters);
    tokio::pin!(exercise);
    tokio::select! {
        _ = &mut exercise => panic!("J1 ended before the retained issuer's dispatch"),
        observed_dispatch = timeout(Duration::from_secs(10), observed.finished.notified()) => {
            observed_dispatch.expect("J1 retained dispatch observation elapsed");
            assert_eq!(*observed.result.lock().unwrap(), Some(Ok(())), "J1 frozen enroll refused");
            assert_eq!(observed.anchor_calls.load(Relaxed), 1);
            assert_eq!(observed.redeem_calls.load(Relaxed), 1);
            observed.release.notify_one();
        }
    }
    let witness = exercise.await;
    assert_eq!(observed.anchor_calls.load(Relaxed), 3);
    assert_eq!(observed.redeem_calls.load(Relaxed), 1, "J2 reached Redeem");
    assert_eq!(
        *observed.anchor_outcomes.lock().unwrap(),
        [
            Ok(voters.encode()),
            Err(AnchorQueryError::Refused), // J2 original request/key reconnect.
            Err(AnchorQueryError::Refused), // J2 distinct request/key reconnect.
        ]
    );

    {
        let node = node.lock().await;
        let log = node.log_weak.upgrade().unwrap();
        let entries = log.lock().unwrap().entries(0, 16, 16).unwrap();
        let mut found = 0;
        let mut minted = 0;
        let mut redeemed = 0;
        for (e, entry) in entries {
            match entry {
                StoredEntry::Enrollment(Command::Found { .. }) => found += 1,
                StoredEntry::Enrollment(Command::Mint { code: id, .. }) => {
                    assert_eq!((e, id), (mint_e, code.code_id()));
                    minted += 1;
                }
                StoredEntry::Enrollment(Command::Redeem {
                    request,
                    cluster: applied_cluster,
                    code: id,
                    key,
                    anchor,
                    ..
                }) => {
                    assert_eq!(
                        (e, request, applied_cluster, id, key, anchor),
                        (
                            witness.original,
                            witness.request,
                            cluster,
                            code.code_id(),
                            witness.key,
                            code.anchor()
                        )
                    );
                    redeemed += 1;
                }
                _ => {}
            }
        }
        assert_eq!((found, minted, redeemed), (1, 1, 1));
        assert!(witness.original.index > mint_e.index);
        let store = node.sm.as_ref().unwrap().0.lock().unwrap();
        assert_eq!(witness.node.get(), 2);
        assert_eq!(
            store
                .state()
                .authorize_peer(cluster, witness.node, witness.key),
            Ok(())
        );
    }

    let observer = job.request_close();
    timeout(Duration::from_secs(10), supervisor)
        .await
        .expect("J1/J2 owned issuer close elapsed")
        .unwrap();
    assert_eq!(
        observer.status(),
        Status::Clean,
        "shutdown: {:?}",
        job.state().close_failure
    );
}

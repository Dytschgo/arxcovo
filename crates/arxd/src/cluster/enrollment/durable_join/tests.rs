use super::*;
use crate::cluster::{
    enrollment::{
        BoundVoters, ClusterId, JoinCode, KeyDigest, NodeId,
        session::{Backend, Close, ListenerOwner, Query, Redeem, Reply},
    },
    join_receipt::PublicIdentity,
    pki::preproof_tests::ProbeTime,
    startup_plan::tests::{Layout, guards, objects},
    state_store::LogPos,
};
use rustls::pki_types::PrivatePkcs8KeyDer;
use std::{
    fs,
    net::{IpAddr, SocketAddr, UdpSocket},
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering::SeqCst},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Notify,
    time::timeout,
};

const OBSERVATION: Duration = Duration::from_secs(5);

fn route(endpoint: SocketAddr) -> [LocalRoute; 1] {
    [LocalRoute {
        network: endpoint.ip(),
        prefix: if endpoint.is_ipv4() { 32 } else { 128 },
    }]
}

fn destination(layout: &Layout) -> Destination {
    let plan = layout.plan("join");
    let [wrap, staging] = ["wrap", "staging"].map(|name| {
        let path = layout.root.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    });
    Destination {
        plan,
        wrap,
        staging,
    }
}

fn start_fixture(
    destination: Destination,
    endpoint: SocketAddr,
    code: &JoinCode,
    root: &[u8],
    time: Arc<dyn TimeProvider>,
) -> (&'static Job, Observer) {
    let paste = transfer::encode(endpoint, root, code).unwrap();
    let intent = Intent::parse(
        destination,
        paste.expose_for_owner_transfer(),
        &route(endpoint),
        time,
    )
    .expect("fixture paste and route");
    launch(intent).unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    objects(dir).into_iter().map(|entry| entry.0).collect()
}

fn lock_refuses(state: &Path) {
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.join("cluster-owner.lock"))
        .unwrap();
    assert!(matches!(
        lock.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
}

fn assert_retained_reservation(job: &Job) {
    let retained = job.retained();
    let owner = retained.owner.as_ref().unwrap();
    owner.publication_originals().unwrap();
    lock_refuses(&owner.plan.state);
    assert_eq!(names(&owner.plan.state), ["cluster-owner.lock"]);
    assert_eq!(names(&owner.plan.credentials), ["identity.json"]);
    assert_eq!(
        fs::metadata(owner.plan.credentials.join("identity.json"))
            .unwrap()
            .len(),
        0
    );
    let staging = owner.plan.state.parent().unwrap().join("staging");
    assert_eq!(names(&staging), ["custody-lock", "member-key"]);
    assert!(retained.live.is_none());
}

pub(in crate::cluster) struct Witness {
    pub request: RequestId,
    pub key: KeyDigest,
    pub original: LogPos,
    pub node: NodeId,
}

pub(in crate::cluster) async fn exercise_retained_issuer(
    endpoint: SocketAddr,
    code: &JoinCode,
    root: &[u8],
    time: Arc<dyn TimeProvider>,
    original_voters: &BoundVoters,
) -> Witness {
    let layout = Layout::new();
    let destination = destination(&layout);
    let (job, mut observer) = start_fixture(destination, endpoint, code, root, time.clone());
    assert_eq!(
        observer.terminal().await,
        Status::Published,
        "J1 durable join"
    );
    let (witness, csr) = {
        let retained = job.retained();
        let owner = retained.owner.as_ref().unwrap();
        let custody = retained.custody.as_ref().unwrap();
        let LiveMember { expected, leaf } = retained.live.as_ref().unwrap();
        lock_refuses(&owner.plan.state);
        assert_eq!(
            names(&owner.plan.state),
            ["cluster-owner.lock"],
            "no joiner stores or Engine"
        );
        assert_eq!(
            names(&owner.plan.credentials),
            ["ca.der", "identity.json", "member-key"]
        );
        assert_eq!(
            names(&layout.root.path().join("staging")),
            ["custody-lock", "member-key"]
        );
        assert_eq!(
            names(&layout.root.path().join("wrap")),
            ["install-context", "seal-key"]
        );
        owner.publication_originals().unwrap();
        let root_readback = owner.read("ca.der", 4096).unwrap();
        assert_eq!(root_readback, root);
        assert_eq!(
            owner.read("member-key", 16384).unwrap(),
            custody.member().unwrap().bytes()
        );
        let record = PublicIdentity::decode(&owner.read("identity.json", 8192).unwrap()).unwrap();
        assert_eq!(record.leaf(), leaf);
        let original = expected.origin.position();
        let Origin::Redeem { anchors, .. } = &expected.origin else {
            panic!("joiner origin")
        };
        let mut observed = vec![u8::try_from(anchors.len()).unwrap()];
        for (node, key) in anchors {
            observed.extend_from_slice(&node.to_be_bytes());
            observed.extend_from_slice(key);
        }
        assert_eq!(observed, original_voters.encode());
        record
            .verify(expected, root, time.current_time().unwrap())
            .unwrap();
        custody.prove_original_key(leaf);
        assert_eq!(expected.node.get(), 2);
        (
            Witness {
                request: job.request,
                key: expected.key,
                original,
                node: expected.node,
            },
            retained.original_csr.clone(),
        )
    };

    // J2 names two distinct invocations. The issuer must stop both at Query
    // before the prepared request/key could be sent.
    let distinct_key = rcgen::KeyPair::generate().unwrap();
    let distinct_csr = rcgen::CertificateParams::default()
        .serialize_request(&distinct_key)
        .unwrap();
    assert_ne!(
        ProvenCsr::parse(distinct_csr.der()).unwrap().digest(),
        witness.key
    );
    let mut distinct_request = job.request;
    distinct_request[0] = if distinct_request[0] == 1 { 2 } else { 1 };
    for (label, request, csr) in [
        (
            "J2 consumed-code reconnect with original request/key",
            job.request,
            csr.as_slice(),
        ),
        (
            "J2 consumed-code reconnect with distinct request/key",
            distinct_request,
            distinct_csr.der().as_ref(),
        ),
    ] {
        let result = preproof::exchange(
            endpoint,
            code,
            root,
            time.clone(),
            request,
            csr,
            SocketLimits::default(),
        )
        .await;
        assert!(
            matches!(result, Err(preproof::ExchangeError::Refused)),
            "{label}"
        );
    }
    witness
}

struct Synthetic {
    ca: crate::cluster::pki::ca::ClusterCa,
    code: JoinCode,
    voters: Vec<u8>,
    observation: Arc<Observed>,
}

#[derive(Default)]
struct Observed {
    queries: AtomicUsize,
    request: Mutex<Option<(RequestId, [u8; 32])>>,
    dispatched: Notify,
    release: Notify,
}

const SYNTHETIC_E: LogPos = LogPos {
    term: 2,
    leader: 1,
    index: 3,
};
const NOW: u64 = 1_000_000;
const CLUSTER: ClusterId = [1; 16];

impl Backend for Synthetic {
    async fn anchor(
        &self,
        query: Query,
        _: tokio::time::Instant,
    ) -> Result<BoundVoters, crate::cluster::pki::preproof::Refused> {
        self.observation.queries.fetch_add(1, SeqCst);
        assert_eq!(
            query.body(),
            [self.code.cluster(), self.code.code_id()]
                .concat()
                .as_slice()
        );
        Ok(BoundVoters::decode(&self.voters).unwrap())
    }

    async fn redeem(
        &self,
        request: Redeem,
        _: tokio::time::Instant,
    ) -> Result<Reply, crate::cluster::pki::preproof::Refused> {
        use crate::cluster::consensus::issue::{IssuedLeaf, test_ticket};
        let (input, csr) = request.into_issuer();
        assert_eq!(input.code, self.code.code_id());
        assert_eq!(input.anchor, self.code.anchor());
        let key = ProvenCsr::parse(&csr).unwrap();
        let old = self
            .observation
            .request
            .lock()
            .unwrap()
            .replace((input.request, Sha256::digest(&csr).into()));
        assert!(old.is_none(), "second Redeem");
        self.observation.dispatched.notify_one();
        self.observation.release.notified().await;
        let ticket = test_ticket(key.digest());
        let leaf = self.ca.sign_leaf(&ticket, &key, NOW).unwrap();
        Ok(Reply::issued(IssuedLeaf {
            der: leaf,
            node: ticket.node(),
            identity_commit: SYNTHETIC_E,
        }))
    }
}

fn local_ip() -> IpAddr {
    let route = UdpSocket::bind("0.0.0.0:0").unwrap();
    route.connect("192.0.2.1:9").unwrap(); // Route lookup only.
    route.local_addr().unwrap().ip()
}

async fn synthetic() -> (ListenerOwner, SocketAddr, JoinCode, Vec<u8>, Arc<Observed>) {
    use crate::cluster::pki::{ca::ClusterCa, node_uri};
    use rcgen::{
        CertificateParams, ExtendedKeyUsagePurpose, Issuer, KeyPair, PublicKeyData, SanType,
    };
    let ca = ClusterCa::ephemeral(CLUSTER, NOW);
    let root = ca.cert_der().to_vec();
    let signing_key = KeyPair::try_from(ca.private_marker()).unwrap();
    let issuer = Issuer::from_ca_cert_der(&root.as_slice().into(), signing_key).unwrap();
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::default();
    params.not_before = time::OffsetDateTime::UNIX_EPOCH;
    params.not_after = time::OffsetDateTime::from_unix_timestamp(100_000).unwrap();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let voter = NodeId::new(1).unwrap();
    params.subject_alt_names = vec![SanType::URI(node_uri(CLUSTER, voter).try_into().unwrap())];
    let leaf = params.signed_by(&key, &issuer).unwrap();
    let mut voters = vec![1];
    voters.extend_from_slice(&voter.get().to_be_bytes());
    voters.extend_from_slice(
        KeyDigest::of_spki(&key.subject_public_key_info())
            .unwrap()
            .as_bytes(),
    );
    let anchor = BoundVoters::decode(&voters).unwrap().anchor(CLUSTER);
    let code = JoinCode::generate(CLUSTER, ca.fingerprint(), anchor, getrandom::fill).unwrap();
    let observation = Arc::new(Observed::default());
    let listener = TcpListener::bind(SocketAddr::new(local_ip(), 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let owner = ListenerOwner::start(
        listener,
        leaf.der().clone(),
        PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        Arc::new(ProbeTime),
        Synthetic {
            ca,
            code: code.clone(),
            voters,
            observation: observation.clone(),
        },
    )
    .unwrap();
    (owner, address, code, root, observation)
}

fn assert_one_send(job: &Job, observation: &Observed) {
    let retained = job.retained();
    assert_eq!(observation.queries.load(SeqCst), 1);
    assert_eq!(
        *observation.request.lock().unwrap(),
        Some((job.request, Sha256::digest(&retained.original_csr).into()))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lost_reply_retains_reservation_and_original_lock_without_resubmission() {
    let layout = Layout::new();
    let (mut issuer, destination_addr, code, root, observation) = synthetic().await;
    let relay = TcpListener::bind(SocketAddr::new(local_ip(), 0))
        .await
        .unwrap();
    let address = relay.local_addr().unwrap();
    let (job, observer) = start_fixture(
        destination(&layout),
        address,
        &code,
        &root,
        Arc::new(ProbeTime),
    );
    drop(observer);
    let (mut client, _) = timeout(OBSERVATION, relay.accept()).await.unwrap().unwrap();
    let mut issuer_socket = TcpStream::connect(destination_addr).await.unwrap();
    timeout(OBSERVATION, async {
        tokio::select! {
            _ = observation.dispatched.notified() => (),
            _ = tokio::io::copy_bidirectional(&mut client, &mut issuer_socket) => {
                panic!("relay ended before synthetic dispatch");
            }
        }
    })
    .await
    .unwrap();
    assert_retained_reservation(job);
    let original = layout.snapshot();
    let staged = objects(&layout.root.path().join("staging"));
    let wrapping = objects(&layout.root.path().join("wrap"));
    drop(client);
    drop(issuer_socket);
    let mut observer = Observer(job.status.subscribe());
    assert_eq!(
        timeout(OBSERVATION, observer.terminal()).await.unwrap(),
        Status::Unconfirmed
    );
    assert_retained_reservation(job);
    assert_eq!(layout.snapshot(), original);
    assert_eq!(objects(&layout.root.path().join("staging")), staged);
    assert_eq!(objects(&layout.root.path().join("wrap")), wrapping);
    let reconnect = relay.accept();
    tokio::pin!(reconnect);
    assert!(
        futures_util::poll!(reconnect).is_pending(),
        "joiner reconnected"
    );
    observation.release.notify_one();
    assert_eq!(issuer.close(OBSERVATION).await, Close::Joined);
    assert_one_send(job, &observation);
}

#[tokio::test(flavor = "current_thread")]
async fn sealed_member_replacement_cannot_be_recaptured_for_publication() {
    let layout = Layout::new();
    let (mut issuer, address, code, root, observation) = synthetic().await;
    let destination = destination(&layout);
    let artifact = destination.staging.join("member-key");
    let (job, mut observer) =
        start_fixture(destination, address, &code, &root, Arc::new(ProbeTime));
    timeout(OBSERVATION, observation.dispatched.notified())
        .await
        .unwrap();
    assert_retained_reservation(job);
    let original = artifact.with_file_name("original-member");
    fs::rename(&artifact, &original).unwrap();
    fs::copy(&original, &artifact).unwrap();
    fs::remove_file(original).unwrap();
    observation.release.notify_one();

    assert_eq!(
        timeout(OBSERVATION, observer.terminal()).await.unwrap(),
        Status::Incomplete
    );
    assert_retained_reservation(job);
    assert_eq!(issuer.close(OBSERVATION).await, Close::Joined);
    assert_one_send(job, &observation);
}
#[tokio::test(flavor = "current_thread")]
async fn failed_publication_preserves_partial_and_complete_unconfirmed_tuples() {
    let Some(base) = std::env::var_os("ARX_JOIN_PUBLICATION_FIXTURE") else {
        for seam in ["member-full-write", "replace-parent-sync"] {
            let layout = Layout::new();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "cluster::enrollment::durable_join::tests::failed_publication_preserves_partial_and_complete_unconfirmed_tuples"])
                .env("ARX_JOIN_PUBLICATION_FIXTURE", layout.root.path())
                .env("ARX_JOIN_PUBLICATION_SEAM", seam)
                .status().unwrap();
            assert!(status.success(), "fault child at {seam}");

            let plan = layout.plan("join");
            let lock = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(plan.state.join("cluster-owner.lock"))
                .unwrap();
            lock.try_lock()
                .expect("process exit released original lock");
            drop(lock);
            let before = layout.snapshot();
            let (mut issuer, addr, code, root, observed) = synthetic().await;
            let paste = transfer::encode(addr, &root, &code).unwrap();
            let existing = |plan| Destination {
                plan,
                wrap: layout.root.path().join("wrap"),
                staging: layout.root.path().join("staging"),
            };
            let mut observer = start(
                existing(plan),
                paste.expose_for_owner_transfer(),
                &route(addr),
                Arc::new(ProbeTime),
            )
            .ok()
            .unwrap();
            assert_eq!(
                timeout(OBSERVATION, observer.terminal()).await.unwrap(),
                Status::Refused
            );
            assert!(matches!(
                start(
                    existing(layout.plan("recover-joiner")),
                    paste.expose_for_owner_transfer(),
                    &route(addr),
                    Arc::new(ProbeTime)
                ),
                Err(Status::Refused)
            ));
            assert_eq!(layout.snapshot(), before);
            assert_eq!(observed.queries.load(SeqCst), 0);
            assert!(observed.request.lock().unwrap().is_none());
            assert_eq!(issuer.close(OBSERVATION).await, Close::Joined);
        }
        return;
    };
    let seam = match std::env::var("ARX_JOIN_PUBLICATION_SEAM").unwrap().as_str() {
        "member-full-write" => "member-full-write",
        "replace-parent-sync" => "replace-parent-sync",
        _ => panic!("unknown fixture seam"),
    };
    exercise_publication_fault(&PathBuf::from(base), seam).await;
}

async fn exercise_publication_fault(base: &Path, seam: &'static str) {
    crate::config::checked_path(base).unwrap();
    crate::security::private(base).unwrap();
    assert_eq!(
        base.parent().unwrap().canonicalize().unwrap(),
        std::env::temp_dir().canonicalize().unwrap()
    );
    let plan = Plan {
        mode: Mode::Join,
        config: base.join("config"),
        state: base.join("state"),
        credentials: base.join("credentials"),
        workspace: base.join("workspace"),
        peer: "127.0.0.1:7001".into(),
        enrollment: "127.0.0.1:7002".into(),
    };
    let [wrap, staging] = ["wrap", "staging"].map(|name| {
        let path = base.join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    });
    let (mut issuer, addr, code, root, observation) = synthetic().await;
    let (job, mut observer) = start_fixture(
        Destination {
            plan,
            wrap,
            staging,
        },
        addr,
        &code,
        &root,
        Arc::new(ProbeTime),
    );
    timeout(OBSERVATION, observation.dispatched.notified())
        .await
        .unwrap();
    assert_retained_reservation(job);
    let originals = guards(job.retained().owner.as_ref().unwrap());
    publication::refuse_storage_at(job.retained().owner.as_mut().unwrap(), seam);
    observation.release.notify_one();
    assert_eq!(
        timeout(OBSERVATION, observer.terminal()).await.unwrap(),
        Status::Unconfirmed
    );
    {
        let retained = job.retained();
        assert!(retained.live.is_none());
        let owner = retained.owner.as_ref().unwrap();
        let creds = &owner.plan.credentials;
        lock_refuses(&owner.plan.state);
        assert_eq!(names(&owner.plan.state), ["cluster-owner.lock"]);
        assert_eq!(guards(owner), originals);
        assert_eq!(names(creds), ["ca.der", "identity.json", "member-key"]);
        let record = fs::read(creds.join("identity.json")).unwrap();
        let published_key = fs::read(creds.join("member-key")).unwrap();
        let member = retained.custody.as_ref().unwrap().member().unwrap();
        if seam == "replace-parent-sync" {
            assert!(PublicIdentity::decode(&record).is_ok());
            assert_eq!(published_key, member.bytes());
            assert_eq!(
                publication::check_retained(
                    owner,
                    None,
                    retained.custody.as_ref().unwrap().installation(),
                    &member,
                    Some(NOW)
                ),
                Err(publication::Error::Context)
            );
        } else {
            assert!(record.is_empty());
            assert_eq!(published_key, member.bytes()[..member.bytes().len() / 2]);
        }
    }
    assert_eq!(issuer.close(OBSERVATION).await, Close::Joined);
    assert_one_send(job, &observation);
}

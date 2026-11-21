use super::*;
use crate::cluster::{
    bound_open::{BoundIntent, BoundStores},
    enrollment::{Command, FoundMembers, Outcome},
    issuance_publication as publication,
    log_store::VoteLogStore,
    membership::Membership,
    pki::{ca::ClusterCa, csr::ProvenCsr, custody::voter_transfer::Wipe},
    startup_plan::{Plan, tests::Layout},
    state_store::{Entry, Init, MembershipBlob, StateStore},
};
use rcgen::{
    CertificateParams, CertificateSigningRequestParams, ExtendedKeyUsagePurpose, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command as Subprocess, Stdio},
    time::{Duration, Instant},
};

pub(in crate::cluster) const CLUSTER: ClusterId = [1; 16];
pub(in crate::cluster) const E: LogPos = LogPos {
    term: 1,
    leader: 1,
    index: 2,
};

pub(in crate::cluster) fn private_dir(base: &Path, name: &str) -> PathBuf {
    let path = base.join(name);
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

pub(in crate::cluster) fn slot(base: &Path) -> Slot {
    Slot::configured(base.join("history"))
}

pub(in crate::cluster) fn subprocess(name: &'static str, root: &Path, mode: &str) {
    let mut child = Subprocess::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("ARX_HISTORY_PROBE_ROOT", root)
        .env("ARX_HISTORY_PROBE_MODE", mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "restart history subprocess refused its case"
            );
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("restart history subprocess exceeded its bound");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(in crate::cluster) fn probe() -> Option<(PathBuf, String)> {
    let base = std::env::var_os("ARX_HISTORY_PROBE_ROOT")?;
    let mode = std::env::var("ARX_HISTORY_PROBE_MODE").unwrap();
    Some((base.into(), mode))
}

pub(in crate::cluster) fn plan(base: &Path, mode: Mode) -> Plan {
    Plan {
        mode,
        config: base.join("config"),
        state: base.join("state"),
        credentials: base.join("credentials"),
        workspace: base.join("workspace"),
        peer: "127.0.0.1:7001".into(),
        enrollment: "127.0.0.1:7002".into(),
    }
}

pub(in crate::cluster) fn now() -> u64 {
    u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).unwrap()
}

pub(in crate::cluster) fn learner_record(
    slot: &mut Fresh,
    owner: &Owner,
    learner: &LearnerCustody,
    expected: &ExpectedIdentity,
    leaf: &[u8],
    anchor: (NodeId, KeyDigest),
) -> Live {
    slot.complete(owner, learner.history_auth(), expected, leaf, Some(anchor))
        .unwrap()
}

pub(in crate::cluster) fn learner_stores(owner: &mut Owner) -> (StateStore, VoteLogStore) {
    owner.stage_published_join().unwrap();
    let (mut stores, checked) = BoundStores::operational(
        owner,
        BoundIntent::PublishedJoin(init()),
        |stores| stores.validate_owner_for_start(Owner::published_baseline),
        |(), _, _| Ok(()),
    );
    checked.unwrap();
    stores.take_pair().unwrap()
}

fn init() -> Init {
    Init {
        cluster: CLUSTER,
        first_free: NodeId::new(5).unwrap(),
    }
}

fn leaf(ca: &ClusterCa, csr: &[u8], node: NodeId) -> Vec<u8> {
    let root = rustls::pki_types::CertificateDer::from(ca.cert_der());
    let key = Wipe(ca.private_marker());
    let issuer =
        Issuer::from_ca_cert_der(&root, KeyPair::try_from(key.0.as_slice()).unwrap()).unwrap();
    let csr = CertificateSigningRequestParams::from_der(&csr.into()).unwrap();
    let now = time::OffsetDateTime::now_utc();
    let mut params = CertificateParams::default();
    params.not_before = now - time::Duration::seconds(30);
    params.not_after = now + time::Duration::days(1);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![
        ExtendedKeyUsagePurpose::ServerAuth,
        ExtendedKeyUsagePurpose::ClientAuth,
    ];
    params.subject_alt_names = vec![SanType::URI(
        crate::cluster::pki::node_uri(CLUSTER, node)
            .try_into()
            .unwrap(),
    )];
    params
        .signed_by(&csr.public_key, &issuer)
        .unwrap()
        .der()
        .to_vec()
}

pub(in crate::cluster) struct Fixture {
    pub layout: Layout,
    pub owner: Owner,
    slot: Fresh,
    custody: HistoryCustody,
    live: Option<Live>,
    pub expected: ExpectedIdentity,
    anchor: (NodeId, KeyDigest),
    pub state: Option<StateStore>,
    pub log: Option<VoteLogStore>,
}

impl Fixture {
    pub(in crate::cluster) fn new(founder: bool, history: bool, fault: Option<TestFault>) -> Self {
        let layout = Layout::new();
        let base = layout.root.path();
        let history_dir = private_dir(base, "history");
        let wrap = private_dir(base, "wrap");
        let role = private_dir(base, "custody");
        let mut owner = Owner::acquire(plan(
            base,
            if founder {
                Mode::CreateFounder
            } else {
                Mode::Join
            },
        ))
        .unwrap();
        let mut slot = Slot::configured(history_dir)
            .check_fresh(&owner, &wrap, &role)
            .unwrap();
        let (mut state, mut log) = (None, None);
        let (custody, ca, csr) = if founder {
            let mut stores =
                BoundStores::acquire(&mut owner, BoundIntent::Fresh(init())).into_owned();
            let (s, l) = stores.take_pair().unwrap();
            state = Some(s);
            log = Some(l);
            let mut custody = FounderCustody::create(&owner, &wrap, &role).unwrap();
            let (ca, csr) = custody.seal(CLUSTER, now()).unwrap();
            (HistoryCustody::Founder(custody), ca, csr)
        } else {
            let ca = ClusterCa::ephemeral(CLUSTER, now());
            let mut custody = LearnerCustody::create(&owner, &wrap, &role).unwrap();
            let csr = custody.seal(CLUSTER, ca.fingerprint()).unwrap();
            (HistoryCustody::Learner(Box::new(custody)), ca, csr)
        };
        slot.bind(custody.auth()).unwrap();
        let node = NodeId::new(if founder { 1 } else { 4 }).unwrap();
        let anchor = (NodeId::new(1).unwrap(), KeyDigest::of_spki(&[91]).unwrap());
        let origin = if founder {
            Origin::Found {
                term: E.term,
                leader: E.leader,
                index: E.index,
            }
        } else {
            Origin::Redeem {
                term: E.term,
                leader: E.leader,
                index: E.index,
                anchors: vec![(anchor.0.get(), *anchor.1.as_bytes())],
            }
        };
        let expected = ExpectedIdentity {
            cluster: CLUSTER,
            node,
            key: ProvenCsr::parse(&csr).unwrap().digest(),
            ca: ca.fingerprint(),
            origin,
        };
        let leaf = leaf(&ca, &csr, node);
        publication::reserve(&mut owner).unwrap();
        publication::store(
            &mut owner,
            Some(&expected),
            custody.auth().installation(),
            ca.cert_der(),
            &leaf,
            &custody.member().unwrap(),
            Some(now()),
        )
        .unwrap();
        publication::check_retained(
            &owner,
            Some(&expected),
            custody.auth().installation(),
            &custody.member().unwrap(),
            Some(now()),
        )
        .unwrap();
        if let Some(fault) = fault {
            slot.fault(fault);
        }
        let live = if history {
            let result = slot.complete(
                &owner,
                custody.auth(),
                &expected,
                &leaf,
                (!founder).then_some(anchor),
            );
            if fault.is_some() {
                assert!(matches!(result, Err(Refusal::Unavailable)));
                None
            } else {
                Some(result.unwrap())
            }
        } else {
            None
        };
        if !founder {
            let (s, l) = learner_stores(&mut owner);
            state = Some(s);
            log = Some(l);
        }
        Self {
            layout,
            owner,
            slot,
            custody,
            live,
            expected,
            anchor,
            state,
            log,
        }
    }

    /// Synthetic applied state. No Engine, enrollment listener or proposal.
    pub(in crate::cluster) fn seed(
        &mut self,
        applied: LogPos,
        log_e: LogPos,
        own_route: Option<&str>,
    ) {
        let mut peers = vec![(self.expected.node, self.expected.key)];
        if !self.expected.origin.founder() {
            peers.insert(0, self.anchor);
        }
        let seed = Entry::Enrollment(Command::Found {
            cluster: CLUSTER,
            members: FoundMembers::new(peers).unwrap(),
            proposed_at_ms: 100,
        });
        let mut routes = std::collections::BTreeMap::from([(1, self.owner.plan.peer.clone())]);
        if let Some(route) = own_route {
            routes.insert(self.expected.node.get(), route.into());
        } else if self.expected.origin.founder() {
            routes.insert(1, "127.0.0.1:7010".into());
        }
        let members = Membership::new(vec![[1].into()], routes).unwrap();
        let members = Entry::Membership(MembershipBlob::new(members.encode().unwrap()).unwrap());
        let start = LogPos { index: 0, ..E };
        let state = self.state.as_mut().unwrap();
        state.apply(start, &members).unwrap();
        for index in 1..applied.index {
            state.apply(LogPos { index, ..E }, &Entry::Blank).unwrap();
        }
        assert_eq!(
            state.apply(applied, &seed).unwrap(),
            Some(Outcome::Founded { replay: false })
        );
        let log = self.log.as_mut().unwrap();
        let mut entries = vec![(start, members)];
        for index in 1..=applied.index {
            let at = if index == E.index {
                log_e
            } else if index == applied.index {
                applied
            } else {
                LogPos { index, ..E }
            };
            let entry = if index == applied.index {
                seed.clone()
            } else {
                Entry::Blank
            };
            entries.push((at, entry));
        }
        log.append(&entries).unwrap();
    }

    pub(in crate::cluster) fn strict(&mut self, position: LogPos) {
        let HistoryCustody::Learner(custody) = &self.custody else {
            panic!("learner fixture")
        };
        self.slot
            .persist_strict_position(&self.owner, custody, self.live.as_mut().unwrap(), position)
            .unwrap();
    }

    pub(in crate::cluster) fn release(self) -> Layout {
        self.layout
    }
}

#[test]
fn history_slot_rejects_alias_and_nonempty_inventory_before_custody() {
    let layout = Layout::new();
    let base = layout.root.path();
    let owner = Owner::acquire(plan(base, Mode::Join)).unwrap();
    let wrap = private_dir(base, "wrap");
    let store = private_dir(base, "custody");
    let history = private_dir(base, "history");
    let check = |p| Slot::configured(p).check_fresh(&owner, &wrap, &store);
    assert!(check(wrap.clone()).is_err());
    assert!(check(owner.plan.state.clone()).is_err());
    layout.write("history/foreign", b"residue");
    assert!(check(history).is_err());
    assert!(fs::read_dir(wrap).unwrap().next().is_none());
    assert!(fs::read_dir(store).unwrap().next().is_none());
    assert!(
        fs::read_dir(&owner.plan.credentials)
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn history_refuses_unknown_version_and_cross_installation_mac() {
    let source = Fixture::new(true, true, None);
    let victim = Fixture::new(true, true, None);
    let read = || read_record::<Issuance>(&victim.slot.roots, &victim.custody.auth(), &ISSUANCE);
    assert!(read().is_ok());
    let victim_path = victim.layout.root.path().join("history/issuance");
    let mut bytes = fs::read(&victim_path).unwrap();
    bytes[6] += 1;
    fs::write(&victim_path, bytes).unwrap();
    assert!(matches!(read(), Err(Refusal::Provenance)));
    let bytes = fs::read(source.layout.root.path().join("history/issuance")).unwrap();
    fs::write(&victim_path, bytes).unwrap();
    assert!(matches!(read(), Err(Refusal::Identity)));
}

#[test]
fn history_refuses_authentic_but_unlinked_strict_record() {
    let mut fixture = Fixture::new(false, true, None);
    let auth = fixture.custody.auth();
    let strict = Strict {
        issuance: [42; 32],
        applied: position(E),
        stores: store_objects(&fixture.owner).unwrap(),
    };
    write_record(
        &fixture.slot.roots,
        &auth,
        &STRICT,
        &["issuance"],
        strict,
        None,
    )
    .unwrap();
    fixture.seed(E, E, None);
    let layout = fixture.release();
    let path = layout.root.path();
    let owner = Owner::acquire(plan(path, Mode::RecoverJoiner)).unwrap();
    let mut history = Recovering::open(
        &owner,
        slot(path),
        &path.join("wrap"),
        &path.join("custody"),
    )
    .unwrap();
    assert_eq!(history.authenticate(&owner), Err(Refusal::Provenance));
}

#[test]
fn history_refuses_same_bytes_on_replaced_original_objects() {
    for name in [
        "wrap/install-context",
        "credentials/ca.der",
        "state/cluster-log.lock",
        "history/strict",
    ] {
        let mut fixture = Fixture::new(false, true, None);
        fixture.seed(E, E, None);
        fixture.strict(E);
        let layout = fixture.release();
        let path = layout.root.path();
        let original = path.join(name);
        let old = path.join("displaced");
        fs::rename(&original, &old).unwrap();
        fs::copy(&old, &original).unwrap();
        let owner = Owner::acquire(plan(path, Mode::RecoverJoiner)).unwrap();
        let mut history = Recovering::open(
            &owner,
            slot(path),
            &path.join("wrap"),
            &path.join("custody"),
        )
        .unwrap();
        assert!(history.authenticate(&owner).is_err());
        drop((history, owner));
    }
}
#[test]
fn history_authentication_still_requires_role_anchor_publication_and_layout_binding() {
    let mut fixture = Fixture::new(false, true, None);
    fixture.seed(E, E, None);
    fixture.strict(E);
    let receipt = fixture.owner.read("identity.json", 8192).unwrap();
    let names = fixture.layout.root.path();
    let auth = fixture.custody.auth();
    let original = || read_record::<Issuance>(&fixture.slot.roots, &auth, &ISSUANCE).unwrap();
    for field in [
        "installation",
        "role",
        "anchor",
        "root",
        "leaf",
        "key",
        "route",
        "config",
    ] {
        let mut evidence = original();
        match field {
            "installation" => evidence.record.installation[0] ^= 1,
            "role" => {
                evidence.record.origin = Origin::Found {
                    term: E.term,
                    leader: E.leader,
                    index: E.index,
                }
            }
            "anchor" => {
                let Origin::Redeem { anchors, .. } = &mut evidence.record.origin else {
                    unreachable!()
                };
                anchors[0].1[0] ^= 1;
            }
            "root" => evidence.record.root[0] ^= 1,
            "leaf" => evidence.record.leaf[0] ^= 1,
            "key" => evidence.record.key[0] ^= 1,
            "route" => evidence.record.peer = "127.0.0.1:7443".into(),
            "config" => evidence.record.configuration[0] ^= 1,
            _ => unreachable!(),
        }
        assert!(
            validate_issuance(&fixture.slot.roots, &fixture.owner, &auth, &evidence).is_err(),
            "{field}"
        );
    }
    assert!(validate_issuance(&fixture.slot.roots, &fixture.owner, &auth, &original()).is_ok());
    assert_eq!(
        fs::read(names.join("credentials/identity.json")).unwrap(),
        receipt
    );
}

#[test]
fn history_final_boundary_refuses_second_commit_without_creating_temp() {
    let fixture = Fixture::new(true, true, None);
    let auth = fixture.custody.auth();
    let first = read_record::<Issuance>(&fixture.slot.roots, &auth, &ISSUANCE).unwrap();
    let digest = first.digest;
    assert!(matches!(
        write_record(
            &fixture.slot.roots,
            &auth,
            &ISSUANCE,
            &[],
            first.record,
            None
        ),
        Err(Refusal::Provenance)
    ));
    let current = read_record::<Issuance>(&fixture.slot.roots, &auth, &ISSUANCE).unwrap();
    assert_eq!(current.digest, digest);
    assert_eq!(fixture.slot.roots.entries().unwrap(), ["issuance"]);
}

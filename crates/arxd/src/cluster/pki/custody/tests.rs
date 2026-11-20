use super::*;
const NOW: u64 = 1_000_000;
fn fixture(s: &str) -> Vec<u8> {
    hex::decode(s.split_whitespace().collect::<String>()).unwrap()
}
fn context(kind: Kind) -> Context {
    Context {
        kind,
        install: std::array::from_fn(|i| i as u8),
        cluster: std::array::from_fn(|i| i as u8 + 16),
        root: std::array::from_fn(|i| i as u8 + 32),
        spki: std::array::from_fn(|i| i as u8 + 64),
    }
}
const I1: &str = "4152584354583031 01 000102030405060708090a0b0c0d0e0f";
const E1: &str = "4152584b45593031 01 02
000102030405060708090a0b0c0d0e0f 101112131415161718191a1b1c1d1e1f
202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f
404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f
606162636465666768696a6b 00000005 a0a1a2a3a4 b0b1b2b3b4b5b6b7b8b9babbbcbdbebf";
const E2: &str = "4152584b45593031 01 01
000102030405060708090a0b0c0d0e0f 101112131415161718191a1b1c1d1e1f
202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f
404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f
606162636465666768696a6b 0000000a a0a1a2a3a4a5a6a7a8a9 b0b1b2b3b4b5b6b7b8b9babbbcbdbebf";
#[test]
fn literal_i_p_e_r_z_fixtures() {
    assert_eq!(codec::install(context(Kind::Ca).install), fixture(I1));
    assert_eq!(
        codec::installation(&fixture(I1)).unwrap(),
        context(Kind::Ca).install
    );
    assert_eq!(
        codec::payload(Kind::Member, &[0x30], &[]).unwrap(),
        fixture("00000001 30")
    );
    assert_eq!(
        codec::payload(Kind::Ca, &[0x30], &[0x31]).unwrap(),
        fixture("00000001 30 00000001 31")
    );
    assert_eq!(
        codec::parts(Kind::Member, &fixture("00000001 30")).unwrap(),
        (&[0x30][..], &[][..])
    );
    assert_eq!(
        codec::parts(Kind::Ca, &fixture("00000001 30 00000001 31")).unwrap(),
        (&[0x30][..], &[0x31][..])
    );
    for (kind, literal, len) in [(Kind::Member, E1, 5), (Kind::Ca, E2, 10)] {
        let bytes = fixture(literal);
        let nonce = std::array::from_fn(|i| i as u8 + 96);
        assert_eq!(codec::frame(&bytes).unwrap(), (context(kind), nonce));
        let mut encoded = codec::header(context(kind), nonce, len).unwrap();
        encoded.extend((0..len).map(|i| 0xa0 + i as u8));
        encoded.extend(0xb0..=0xbf);
        assert_eq!(encoded, bytes);
        assert_eq!(
            &bytes[122..122 + len],
            &(0..len).map(|i| 0xa0 + i as u8).collect::<Vec<_>>()
        );
        assert!(codec::open(&[0; 32], context(kind), &bytes).is_err());
    }
    let raw = fixture("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
    assert_eq!(codec::wrapping(&raw).unwrap().as_slice(), raw);
    assert!(codec::wrapping(&raw[..31]).is_err());
    assert!(codec::wrapping(&[raw.as_slice(), &[0]].concat()).is_err());
    assert!(codec::lock(&[]).is_ok());
    assert!(codec::lock(&[0]).is_err());
}
#[test]
fn malformed_fixture_recipes() {
    let i = fixture(I1);
    let mut cases = vec![
        [i.as_slice(), &[0]].concat(),
        i[..24].to_vec(),
        [&[0xef, 0xbb, 0xbf], i.as_slice()].concat(),
    ];
    for (at, value) in [(8, 2), (7, b'0')] {
        let mut b = i.clone();
        b[at] = value;
        cases.push(b);
    }
    assert_eq!(cases.len(), 5);
    for b in cases {
        assert!(codec::installation(&b).is_err());
    }
    for (kind, hex) in [
        (Kind::Member, "00000000"),
        (Kind::Member, "00001001"),
        (Kind::Member, "0100000030"),
        (Kind::Member, "000000013000"),
        (Kind::Ca, "000000013000000000"),
        (Kind::Ca, "000000013000001001"),
        (Kind::Ca, "000000013000000001"),
        (Kind::Ca, "0000000130000000013100"),
    ] {
        assert!(codec::parts(kind, &fixture(hex)).is_err());
    }
    let e = fixture(E1);
    let mut cases = vec![
        e[..142].to_vec(),
        [e.as_slice(), &[0]].concat(),
        e[..121].to_vec(),
    ];
    for (at, value) in [(7, b'0'), (8, 2), (9, 0), (9, 3)] {
        let mut b = e.clone();
        b[at] = value;
        cases.push(b);
    }
    for (base, len) in [
        (E1, "05000000"),
        (E1, "00000015"),
        (E1, "ffffffff"),
        (E1, "00000000"),
        (E2, "00002009"),
        (E1, "00001005"),
    ] {
        let mut b = fixture(base);
        b[118..122].copy_from_slice(&fixture(len));
        cases.push(b);
    }
    assert_eq!(cases.len(), 13);
    for b in cases {
        assert!(codec::frame(&b).is_err());
    }
}
#[test]
fn aead_control_and_all_field_mutations() {
    let ctx = context(Kind::Member);
    let p = codec::payload(Kind::Member, &[0x30], &[]).unwrap();
    let b = codec::seal(&[7; 32], ctx, &p).unwrap();
    assert_eq!(codec::open(&[7; 32], ctx, &b).unwrap().0, p);
    assert!(codec::open(&[8; 32], ctx, &b).is_err());
    for at in [0, 8, 9, 10, 26, 42, 74, 106, 118, 122, b.len() - 1] {
        let mut edited = b.clone();
        edited[at] ^= 1;
        assert!(codec::open(&[7; 32], ctx, &edited).is_err());
        if let Ok((edited_ctx, _)) = codec::frame(&edited) {
            assert!(codec::open(&[7; 32], edited_ctx, &edited).is_err());
        }
    }
    for at in [0, 8, 9, 10, 26, 42, 74, 106, 118, 121, 122, 127, 142] {
        assert!(codec::open(&[7; 32], ctx, &b[..at.min(b.len())]).is_err());
    }
    for field in 0..5 {
        let mut expected = ctx;
        match field {
            0 => expected.kind = Kind::Ca,
            1 => expected.install[0] ^= 1,
            2 => expected.cluster[0] ^= 1,
            3 => expected.root[0] ^= 1,
            _ => expected.spki[0] ^= 1,
        }
        assert!(codec::open(&[7; 32], expected, &b).is_err());
    }
    assert_eq!(codec::open(&[7; 32], ctx, &b).unwrap().0, p);
}
fn real_member(root: [u8; 32]) -> (MemberKey, codec::Secret) {
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let context = Context {
        kind: Kind::Member,
        install: [9; 16],
        cluster: [1; 16],
        root,
        spki: digest(&key),
    };
    let p = codec::Secret(codec::payload(Kind::Member, &key.serialize_der(), &[]).unwrap());
    (MemberKey { key, context }, p)
}
#[test]
fn production_ca_member_reload_sign_and_verify() {
    use crate::cluster::consensus::issue::test_ticket;
    let (ca, key) = ClusterCa::generate(NOW).unwrap();
    let key = codec::Secret(key);
    let ctx = Context {
        kind: Kind::Ca,
        install: [9; 16],
        cluster: [1; 16],
        root: ca.fingerprint(),
        spki: ca.spki_digest().unwrap(),
    };
    let sealed = codec::seal(
        &[4; 32],
        ctx,
        &codec::payload(Kind::Ca, &key.0, ca.cert_der()).unwrap(),
    )
    .unwrap();
    let original = ca.cert_der().to_vec();
    drop(ca);
    drop(key);
    let p = codec::open(&[4; 32], ctx, &sealed).unwrap();
    let (k, c) = codec::parts(Kind::Ca, &p.0).unwrap();
    let ca = ClusterCa::load(k, c, ctx.root, ctx.spki, NOW).unwrap();
    assert_eq!(ca.cert_der(), original);
    let (member, p) = real_member(ctx.root);
    let expected = member.context;
    let sealed = codec::seal(&[4; 32], expected, &p.0).unwrap();
    drop(member);
    drop(p);
    let member = MemberKey::from_payload(
        expected,
        &codec::open(&[4; 32], expected, &sealed).unwrap().0,
    )
    .unwrap();
    let csr = ProvenCsr::parse(&member.csr().unwrap()).unwrap();
    let ticket = test_ticket(csr.digest());
    let leaf = ca.sign_leaf(&ticket, &csr, NOW).unwrap();
    member
        .check_leaf(ticket.node(), &original, &leaf, Some(NOW))
        .unwrap();
    let (other, _) = real_member(ctx.root);
    assert!(
        other
            .check_leaf(ticket.node(), &original, &leaf, Some(NOW))
            .is_err()
    );
}
#[test]
fn authenticated_key_certificate_mismatch_and_bad_der() {
    let (a, ka) = ClusterCa::generate(NOW).unwrap();
    let (_, kb) = ClusterCa::generate(NOW).unwrap();
    let ctx = Context {
        kind: Kind::Ca,
        install: [9; 16],
        cluster: [1; 16],
        root: a.fingerprint(),
        spki: a.spki_digest().unwrap(),
    };
    let p = codec::Secret(codec::payload(Kind::Ca, &kb, a.cert_der()).unwrap());
    let sealed = codec::seal(&[4; 32], ctx, &p.0).unwrap();
    let p = codec::open(&[4; 32], ctx, &sealed).unwrap();
    let (k, c) = codec::parts(Kind::Ca, &p.0).unwrap();
    assert!(matches!(
        ClusterCa::load(k, c, ctx.root, ctx.spki, NOW),
        Err(Error::KeyMismatch)
    ));
    assert!(ClusterCa::load(&ka, c, ctx.root, ctx.spki, NOW).is_ok());
    let mut bad = c.to_vec();
    *bad.last_mut().unwrap() ^= 1;
    assert!(
        ClusterCa::load(
            &ka,
            &bad,
            super::super::ca::fingerprint_of(&bad),
            ctx.spki,
            NOW
        )
        .is_err()
    );
    let mut extra = ka.clone();
    extra.push(0);
    assert!(parse_key(&extra).is_err());
    let mut extra = c.to_vec();
    extra.push(0);
    assert!(
        ClusterCa::load(
            &ka,
            &extra,
            super::super::ca::fingerprint_of(&extra),
            ctx.spki,
            NOW
        )
        .is_err()
    );
}
#[test]
fn clocks_and_full_ca_interval_boundaries() {
    let (ca, key) = ClusterCa::generate(NOW).unwrap();
    let root = ca.fingerprint();
    let spki = ca.spki_digest().unwrap();
    // nb=880, na=31537000, leaf nb=floor(now/1000)-60.
    for (ms, fits) in [
        (939_000, false),
        (940_000, true),
        (28_945_060_000, true),
        (28_945_061_000, false),
    ] {
        assert_eq!(ca.leaf_fits(ms), fits);
        assert_eq!(
            ClusterCa::load(&key, ca.cert_der(), root, spki, ms).is_ok(),
            fits
        );
    }
    assert_eq!(checked_time(None), Err(Error::Clock));
    assert_eq!(checked_time(Some(u64::MAX)), Err(Error::Clock));
    assert!(ClusterCa::generate(u64::MAX).is_err());
    assert!(ClusterCa::generate(MAX_TIME_MS).is_err());
}
#[test]
fn both_sign_time_bounds_refuse_without_signing_with_equality_controls() {
    use crate::cluster::consensus::issue::{IssueError, test_ticket};
    use std::sync::atomic::Ordering;
    let (ca, _) = ClusterCa::generate(NOW).unwrap();
    let (member, _) = real_member(ca.fingerprint());
    let csr = ProvenCsr::parse(&member.csr().unwrap()).unwrap();
    let ticket = test_ticket(csr.digest());
    for ms in [939_000, 28_945_061_000] {
        assert_eq!(ca.sign_leaf(&ticket, &csr, ms), Err(IssueError::CaExpiring));
        assert_eq!(ca.signs.load(Ordering::SeqCst), 0);
    }
    for ms in [940_000, 28_945_060_000] {
        assert!(ca.sign_leaf(&ticket, &csr, ms).is_ok());
    }
    assert_eq!(ca.signs.load(Ordering::SeqCst), 2);
}
#[test]
fn native_composition_is_explicitly_unqualified() {
    assert_eq!(native(), Err(Error::Unqualified));
}

#[test]
fn failed_rng_releases_no_envelope() {
    assert_eq!(
        codec::failed_random(&[1; 32], context(Kind::Member), &fixture("0000000130")).err(),
        Some(Error::Unavailable)
    );
}

#[test]
fn actual_ca_policy_refusals_with_matching_positive() {
    use rcgen::{BasicConstraints, IsCa, KeyUsagePurpose};
    for variant in 0..6 {
        let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut p = CertificateParams::default();
        p.not_before = time::OffsetDateTime::from_unix_timestamp(880).unwrap();
        p.not_after = time::OffsetDateTime::from_unix_timestamp(31_537_000).unwrap();
        p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        match variant {
            1 => p.is_ca = IsCa::NoCa,
            2 => p.is_ca = IsCa::Ca(BasicConstraints::Constrained(1)),
            3 => p.key_usages = vec![KeyUsagePurpose::DigitalSignature],
            4 => p.not_before = time::OffsetDateTime::from_unix_timestamp(1001).unwrap(),
            5 => p.not_after = time::OffsetDateTime::from_unix_timestamp(999).unwrap(),
            _ => (),
        }
        let der = p.self_signed(&key).unwrap().der().to_vec();
        let loaded = ClusterCa::load(
            &key.serialize_der(),
            &der,
            super::super::ca::fingerprint_of(&der),
            digest(&key),
            NOW,
        );
        assert_eq!(loaded.is_ok(), variant == 0, "variant {variant}");
    }
    let key = KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap();
    let der = CertificateParams::default()
        .self_signed(&key)
        .unwrap()
        .der()
        .to_vec();
    assert!(
        ClusterCa::load(
            &key.serialize_der(),
            &der,
            super::super::ca::fingerprint_of(&der),
            digest(&key),
            NOW
        )
        .is_err()
    );
}

#[cfg(target_os = "linux")]
mod files {
    use super::super::linux::Custody;
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    fn roots() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("wrap");
        let b = t.path().join("store");
        for p in [&a, &b] {
            fs::create_dir(p).unwrap();
            fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
        }
        (t, a, b)
    }
    #[test]
    fn durable_file_restart_and_learner_inventory() {
        let (_t, a, b) = roots();
        let mut owner = Custody::create_fresh_installation(&a, &b).unwrap();
        let ca = owner.create_founder_ca([1; 16], Some(NOW)).unwrap();
        let ca_ctx = Context {
            kind: Kind::Ca,
            install: codec::installation(&fs::read(a.join("install-context")).unwrap()).unwrap(),
            cluster: [1; 16],
            root: ca.fingerprint(),
            spki: ca.spki_digest().unwrap(),
        };
        let (member, _) = owner
            .create_member([1; 16], ca.fingerprint(), false)
            .unwrap();
        let ctx = member.context;
        assert!(owner.effects >= 20);
        drop(member);
        drop(ca);
        drop(owner);
        let owner = Custody::open_existing(&a, &b, ctx.install).unwrap();
        assert!(owner.load_ca(ca_ctx, Some(NOW)).is_ok());
        assert!(owner.load_member(ctx, false).is_ok());
        assert!(owner.load_member(ctx, true).is_err());
        drop(owner);
        let (_t, a, b) = roots();
        let mut learner = Custody::create_fresh_installation(&a, &b).unwrap();
        learner.create_member([1; 16], [3; 32], true).unwrap();
        assert!(!b.join("ca-key").exists());
        assert!(learner.create_founder_ca([1; 16], Some(NOW)).is_err());
    }
    #[test]
    fn partial_write_sync_failures_are_spent_and_recovery_preserves_bytes() {
        for step in 1..=5 {
            let (_t, a, b) = roots();
            let mut owner = Custody::create_fresh_installation(&a, &b).unwrap();
            owner.fail_at = Some(owner.effects + step);
            let first = owner.create_founder_ca([1; 16], Some(NOW)).err().unwrap();
            let before = fs::read(b.join("ca-key")).ok();
            assert_eq!(
                owner.create_founder_ca([1; 16], Some(NOW)).err(),
                Some(first)
            );
            let id = codec::installation(&fs::read(a.join("install-context")).unwrap()).unwrap();
            drop(owner);
            let recovered = Custody::open_existing(&a, &b, id);
            if let Some(bytes) = &before
                && bytes.len() < 138
            {
                assert!(recovered.is_err());
            }
            assert_eq!(before, fs::read(b.join("ca-key")).ok());
        }
    }
    #[test]
    fn lock_permissions_links_and_replacement_refuse() {
        let (_t, a, b) = roots();
        let owner = Custody::create_fresh_installation(&a, &b).unwrap();
        let id = codec::installation(&fs::read(a.join("install-context")).unwrap()).unwrap();
        assert!(Custody::open_existing(&a, &b, id).is_err());
        drop(owner);
        let key = fs::read(a.join("seal-key")).unwrap();
        fs::set_permissions(a.join("seal-key"), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Custody::open_existing(&a, &b, id).is_err());
        fs::set_permissions(a.join("seal-key"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::hard_link(a.join("seal-key"), a.join("alias")).unwrap();
        assert!(Custody::open_existing(&a, &b, id).is_err());
        fs::remove_file(a.join("alias")).unwrap();
        fs::remove_file(a.join("seal-key")).unwrap();
        symlink(b.join("custody-lock"), a.join("seal-key")).unwrap();
        assert!(Custody::open_existing(&a, &b, id).is_err());
        fs::remove_file(a.join("seal-key")).unwrap();
        fs::write(a.join("seal-key"), key).unwrap();
        fs::set_permissions(a.join("seal-key"), fs::Permissions::from_mode(0o600)).unwrap();
        let mut owner = Custody::open_existing(&a, &b, id).unwrap();
        fs::rename(&b, b.with_extension("old")).unwrap();
        fs::create_dir(&b).unwrap();
        fs::set_permissions(&b, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(owner.create_founder_ca([1; 16], Some(NOW)).is_err());
    }
    #[test]
    fn missing_partial_and_unknown_installation_refuse_without_repair() {
        for (slot, bytes) in [
            ("seal-key", vec![1; 31]),
            ("install-context", vec![1; 24]),
            ("unknown", vec![]),
        ] {
            let (_t, a, b) = roots();
            let owner = Custody::create_fresh_installation(&a, &b).unwrap();
            let id = codec::installation(&fs::read(a.join("install-context")).unwrap()).unwrap();
            drop(owner);
            fs::write(a.join(slot), &bytes).unwrap();
            fs::set_permissions(a.join(slot), fs::Permissions::from_mode(0o600)).unwrap();
            assert!(Custody::open_existing(&a, &b, id).is_err());
            assert_eq!(fs::read(a.join(slot)).unwrap(), bytes);
        }
        let (_t, a, b) = roots();
        let owner = Custody::create_fresh_installation(&a, &b).unwrap();
        let id = codec::installation(&fs::read(a.join("install-context")).unwrap()).unwrap();
        drop(owner);
        fs::remove_file(a.join("seal-key")).unwrap();
        assert!(Custody::open_existing(&a, &b, id).is_err());
        assert!(!a.join("seal-key").exists());
        assert!(Custody::create_fresh_installation(&a, &b).is_err());
        assert!(Custody::create_fresh_installation(&a, &a).is_err());
    }
    #[test]
    fn lock_child_probe() {
        let Some(root) = std::env::var_os("ARX_CUSTODY_LOCK_PROBE") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let a = root.join("wrap");
        let b = root.join("store");
        let id = codec::installation(&fs::read(a.join("install-context")).unwrap()).unwrap();
        assert!(Custody::open_existing(&a, &b, id).is_err());
    }
    #[test]
    fn exclusive_lock_other_process_and_reacquisition() {
        let (t, a, b) = roots();
        let owner = Custody::create_fresh_installation(&a, &b).unwrap();
        let id = codec::installation(&fs::read(a.join("install-context")).unwrap()).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cluster::pki::custody::tests::files::lock_child_probe",
            ])
            .env("ARX_CUSTODY_LOCK_PROBE", t.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("lock probe deadline");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(owner);
        assert!(Custody::open_existing(&a, &b, id).is_ok());
    }
}

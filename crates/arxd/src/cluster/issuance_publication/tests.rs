use super::*;
use crate::cluster::startup_plan::tests::Layout;

#[cfg(not(target_os = "linux"))]
#[test]
fn publication_non_linux_refuses_without_io() {
    let layout = Layout::new();
    let mut owner = Owner::acquire(layout.plan("join")).unwrap();
    let before = layout.snapshot();
    assert_eq!(reserve(&mut owner), Err(Error::Unsupported));
    assert_eq!(reserve(&mut owner), Err(Error::Unsupported));
    assert_eq!(layout.snapshot(), before);
    assert!(owner.publication.files.is_empty());
}

#[cfg(target_os = "linux")]
mod files {
    use super::*;
    use crate::cluster::{
        join_receipt::{Origin, PublicIdentity},
        pki,
    };
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    const POINTS: &[&str] = &[
        "reservation-create",
        "reservation-file-sync",
        "reservation-parent-sync",
        "ca-create",
        "ca-partial-write",
        "ca-full-write",
        "ca-file-sync",
        "ca-parent-sync",
        "member-create",
        "member-partial-write",
        "member-full-write",
        "member-file-sync",
        "member-parent-sync",
        "temp-create",
        "temp-partial-write",
        "temp-full-write",
        "temp-file-sync",
        "temp-parent-sync",
        "reservation-recheck",
        "atomic-replace",
        "replace-parent-sync",
        "final-inventory",
        "retained-readback",
        "guard-delta",
    ];
    fn setup() -> Layout {
        let l = Layout::new();
        // Real files for Owner inventory only, no database or Engine claim.
        for name in [
            "cluster-owner.lock",
            "cluster-state.lock",
            "cluster-log.lock",
            "cluster-state.sqlite3",
            "cluster-log.sqlite3",
        ] {
            l.write(&format!("state/{name}"), b"");
        }
        l
    }
    fn probe(owner: &Owner, locked: bool) {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cluster::issuance_publication::tests::files::publication_lock_probe",
                "--nocapture",
            ])
            .env(
                "ARX_PUBLICATION_PROBE",
                owner.plan.state.join("cluster-owner.lock"),
            )
            .env("ARX_PUBLICATION_LOCKED", if locked { "yes" } else { "no" })
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[test]
    fn publication_lock_probe() {
        let Some(path) = std::env::var_os("ARX_PUBLICATION_PROBE") else {
            return;
        };
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        assert_eq!(
            file.try_lock().is_err(),
            std::env::var("ARX_PUBLICATION_LOCKED").unwrap() == "yes"
        );
    }
    #[test]
    fn publication_linux_real_file_roundtrip() {
        let l = setup();
        let (custody, install, root, leaf, expected) = pki::publication_fixture(l.root.path());
        let member = pki::member_copy(&custody).unwrap();
        let mut owner = Owner::acquire(l.plan("join")).unwrap();
        let originals = crate::cluster::startup_plan::tests::guards(&owner);
        reserve(&mut owner).unwrap();
        assert_eq!(owner.publication.phase, Phase::Reserved);
        linux::store(
            &mut owner,
            Some(&expected),
            install,
            &root,
            &leaf,
            &member,
            Some(1_000_000),
        )
        .unwrap();
        for _ in 0..2 {
            assert_eq!(owner.read("ca.der", 4096).unwrap(), root);
            assert_eq!(owner.read("member-key", 16384).unwrap(), member.bytes());
            let bytes = owner.read("identity.json", 8192).unwrap();
            PublicIdentity::decode(&bytes)
                .unwrap()
                .verify(
                    &expected,
                    &root,
                    rustls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(
                        1000,
                    )),
                )
                .unwrap();
        }
        assert!(
            originals
                .iter()
                .all(|g| crate::cluster::startup_plan::tests::guards(&owner).contains(g))
        );
        assert_eq!(owner.publication.trace, POINTS);
        member.prove_possession();
        probe(&owner, true);
        let trace = owner.publication.trace.clone();
        assert_eq!(reserve(&mut owner), Err(Error::Completed));
        assert_eq!(owner.publication.trace, trace);
        drop(owner);
        let recovered = Owner::acquire(l.plan("recover-joiner")).unwrap();
        probe(&recovered, true);
        linux::check_retained(
            &recovered,
            Some(&expected),
            install,
            &member,
            Some(1_000_000),
        )
        .unwrap();
        assert_eq!(
            linux::check_retained(&recovered, None, install, &member, Some(1_000_000)),
            Err(Error::Context)
        );
        assert_eq!(recovered.read("member-key", 16384).unwrap(), member.bytes());
        assert_eq!(recovered.read("member-key", 16384).unwrap(), member.bytes());
    }
    #[test]
    fn publication_durable_boundaries() {
        let fixture = setup();
        let (custody, install, root, leaf, expected) =
            pki::publication_fixture(fixture.root.path());
        let member = pki::member_copy(&custody).unwrap();
        for point in POINTS {
            for unwind in [false, true] {
                let l = setup();
                let mut owner = Owner::acquire(l.plan("join")).unwrap();
                owner.publication.fault = Some((point, unwind));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    reserve(&mut owner)?;
                    linux::store(
                        &mut owner,
                        Some(&expected),
                        install,
                        &root,
                        &leaf,
                        &member,
                        Some(1_000_000),
                    )
                }));
                if unwind {
                    assert!(result.is_err());
                } else {
                    assert_eq!(result.unwrap(), Err(Error::Unconfirmed));
                }
                let before = l.snapshot();
                let trace = owner.publication.trace.clone();
                assert_eq!(reserve(&mut owner), Err(Error::Unconfirmed));
                assert_eq!(
                    linux::store(
                        &mut owner,
                        Some(&expected),
                        install,
                        &root,
                        &leaf,
                        &member,
                        Some(1_000_000)
                    ),
                    Err(Error::Unconfirmed)
                );
                assert_eq!(owner.publication.trace, trace);
                assert_eq!(l.snapshot(), before);
                if *point != "reservation-create" || unwind {
                    assert!(!owner.publication.files.is_empty());
                }
                probe(&owner, true);
                drop(owner);
                let before = l.snapshot();
                if *point != "reservation-create" || unwind {
                    assert!(Owner::acquire(l.plan("join")).is_err());
                }
                if let Ok(recovered) = Owner::acquire(l.plan("recover-joiner")) {
                    let bytes = recovered.read("identity.json", 8192).unwrap();
                    // Generic acquisition is only an inventory check.
                    let complete = PublicIdentity::decode(&bytes).is_ok();
                    assert_eq!(
                        complete,
                        (POINTS.iter().position(|p| p == point).unwrap() > 19
                            || (*point == "atomic-replace" && unwind))
                    );
                }
                assert_eq!(l.snapshot(), before);
            }
        }
    }
    #[test]
    fn publication_restart_partial_and_missing_history() {
        let fixture = setup();
        let (custody, install, root, leaf, expected) =
            pki::publication_fixture(fixture.root.path());
        let member = pki::member_copy(&custody).unwrap();
        for fault in [None, Some("member-partial-write"), Some("temp-full-write")] {
            let l = setup();
            let mut owner = Owner::acquire(l.plan("join")).unwrap();
            reserve(&mut owner).unwrap();
            if let Some(point) = fault {
                owner.publication.fault = Some((point, false));
                assert!(
                    linux::store(
                        &mut owner,
                        Some(&expected),
                        install,
                        &root,
                        &leaf,
                        &member,
                        Some(1_000_000)
                    )
                    .is_err()
                );
            }
            drop(owner);
            let before = l.snapshot();
            assert!(Owner::acquire(l.plan("join")).is_err());
            if let Ok(recovered) = Owner::acquire(l.plan("recover-joiner")) {
                assert!(
                    PublicIdentity::decode(&recovered.read("identity.json", 8192).unwrap())
                        .is_err()
                );
            }
            assert_eq!(l.snapshot(), before);
        }
        let l = setup();
        let mut owner = Owner::acquire(l.plan("join")).unwrap();
        reserve(&mut owner).unwrap();
        let before = l.snapshot();
        assert_eq!(
            linux::store(
                &mut owner,
                None,
                install,
                &root,
                &leaf,
                &member,
                Some(1_000_000)
            ),
            Err(Error::Context)
        );
        assert_eq!(l.snapshot(), before);
        for damage in [
            "root",
            "key",
            "identity",
            "missing-root",
            "missing-key",
            "missing-identity",
            "extra-temp",
            "nonregular",
        ] {
            let l = setup();
            let mut owner = Owner::acquire(l.plan("join")).unwrap();
            reserve(&mut owner).unwrap();
            linux::store(
                &mut owner,
                Some(&expected),
                install,
                &root,
                &leaf,
                &member,
                Some(1_000_000),
            )
            .unwrap();
            drop(owner);
            match damage {
                "root" => l.write("credentials/ca.der", b"corrupt"),
                "key" => l.write("credentials/member-key", b"corrupt"),
                "identity" => l.write("credentials/identity.json", b"{}"),
                "extra-temp" => l.write("credentials/.identity-publish", b"partial"),
                "nonregular" => {
                    fs::remove_file(l.root.path().join("credentials/member-key")).unwrap();
                    fs::create_dir(l.root.path().join("credentials/member-key")).unwrap();
                }
                missing => {
                    let name = match missing {
                        "missing-root" => "ca.der",
                        "missing-key" => "member-key",
                        _ => "identity.json",
                    };
                    fs::remove_file(l.root.path().join("credentials").join(name)).unwrap();
                }
            }
            // Snapshot regular bytes separately: directory fixture has no readable payload.
            let before =
                crate::cluster::startup_plan::tests::objects(&l.root.path().join("credentials"));
            if let Ok(recovered) = Owner::acquire(l.plan("recover-joiner")) {
                assert!(
                    linux::check_retained(
                        &recovered,
                        Some(&expected),
                        install,
                        &member,
                        Some(1_000_000)
                    )
                    .is_err()
                );
            }
            assert_eq!(
                crate::cluster::startup_plan::tests::objects(&l.root.path().join("credentials")),
                before
            );
        }
    }
    #[test]
    fn publication_wrong_context_and_origin() {
        let l = setup();
        let (custody, install, root, leaf, expected) = pki::publication_fixture(l.root.path());
        let member = pki::member_copy(&custody).unwrap();
        let bytes = PublicIdentity::assemble(&expected, &leaf).unwrap();
        for field in [
            "cluster", "node", "key", "ca", "term", "leader", "index", "role", "anchors",
        ] {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            match field {
                "cluster" | "key" | "ca" => value[field][0] = serde_json::json!(99),
                "node" => value[field] = serde_json::json!(2),
                "role" | "anchors" => {
                    value["origin"] = serde_json::json!({"role":"joiner","term":2,"leader":1,"index":3,"anchors":[[1,vec![0;32]]]});
                    value["node"] = serde_json::json!(2);
                }
                _ => value["origin"][field] = serde_json::json!(99),
            }
            let altered = serde_json::to_vec(&value).unwrap();
            if let Ok(record) = PublicIdentity::decode(&altered) {
                assert!(
                    record
                        .verify(
                            &expected,
                            &root,
                            rustls::pki_types::UnixTime::since_unix_epoch(
                                std::time::Duration::from_secs(1000)
                            )
                        )
                        .is_err()
                );
            }
            // Unchanged AEAD/key remains valid despite edited public history.
            member
                .validate_bytes(member.bytes(), &expected, &root, &leaf, Some(1_000_000))
                .unwrap();
        }
        use crate::cluster::{
            enrollment::{KeyDigest, NodeId},
            join_receipt::ExpectedIdentity,
        };
        for field in [
            "cluster", "node", "key", "root", "role", "term", "leader", "index", "anchors", "leaf",
        ] {
            let other = setup();
            let mut owner = Owner::acquire(other.plan("join")).unwrap();
            reserve(&mut owner).unwrap();
            let mut bad = ExpectedIdentity {
                cluster: expected.cluster,
                node: expected.node,
                key: expected.key,
                ca: expected.ca,
                origin: expected.origin.clone(),
            };
            let mut wrong_root = root.clone();
            let mut wrong_leaf = leaf.clone();
            match field {
                "cluster" => bad.cluster[0] ^= 1,
                "node" => bad.node = NodeId::new(55).unwrap(),
                "key" => bad.key = KeyDigest::of_spki(b"different public key").unwrap(),
                "root" => wrong_root[0] ^= 1,
                "role" => {
                    bad.node = NodeId::new(1).unwrap();
                    bad.origin = Origin::Found {
                        term: 2,
                        leader: 1,
                        index: 3,
                    };
                }
                "leaf" => wrong_leaf[0] ^= 1,
                _ => {
                    if let Origin::Redeem {
                        term,
                        leader,
                        index,
                        anchors,
                    } = &mut bad.origin
                    {
                        match field {
                            "term" => *term += 1,
                            "leader" => *leader += 1,
                            "index" => *index += 1,
                            _ => anchors[0].1[0] ^= 1,
                        }
                    }
                }
            }
            // E/anchors are public consistency data: assembly itself does not authenticate them.
            // A separately trusted original expectation must reject the altered receipt.
            if ["term", "leader", "index", "anchors"].contains(&field) {
                let encoded = PublicIdentity::assemble(&bad, &leaf).unwrap();
                assert!(
                    PublicIdentity::decode(&encoded)
                        .unwrap()
                        .verify(
                            &expected,
                            &root,
                            rustls::pki_types::UnixTime::since_unix_epoch(
                                std::time::Duration::from_secs(1000)
                            )
                        )
                        .is_err()
                );
            } else {
                assert_eq!(
                    linux::store(
                        &mut owner,
                        Some(&bad),
                        install,
                        &wrong_root,
                        &wrong_leaf,
                        &member,
                        Some(1_000_000)
                    ),
                    Err(Error::Context)
                );
            }
        }
        let other = setup();
        let mut owner = Owner::acquire(other.plan("join")).unwrap();
        reserve(&mut owner).unwrap();
        let mut wrong_install = install;
        wrong_install[0] ^= 1;
        assert_eq!(
            linux::store(
                &mut owner,
                Some(&expected),
                wrong_install,
                &root,
                &leaf,
                &member,
                Some(1_000_000)
            ),
            Err(Error::Context)
        );
        assert!(matches!(expected.origin, Origin::Redeem { .. }));
    }
    #[test]
    fn publication_original_owner_collision_and_unsafe_objects() {
        let fixture = setup();
        let (custody, install, root, leaf, expected) =
            pki::publication_fixture(fixture.root.path());
        let member = pki::member_copy(&custody).unwrap();
        for destination in ["ca.der", "member-key", ".identity-publish"] {
            let l = setup();
            let mut owner = Owner::acquire(l.plan("join")).unwrap();
            reserve(&mut owner).unwrap();
            l.write(&format!("credentials/{destination}"), b"collision");
            let before = l.snapshot();
            assert!(
                linux::store(
                    &mut owner,
                    Some(&expected),
                    install,
                    &root,
                    &leaf,
                    &member,
                    Some(1_000_000)
                )
                .is_err()
            );
            assert_eq!(l.snapshot(), before);
        }
        for kind in [
            "collision",
            "symlink",
            "hardlink",
            "directory",
            "privacy",
            "oversize",
            "replace",
        ] {
            let l = setup();
            let mut owner = Owner::acquire(l.plan("join")).unwrap();
            let path = owner.plan.credentials.join("identity.json");
            match kind {
                "collision" => l.write("credentials/identity.json", b"existing"),
                "symlink" => symlink(l.root.path().join("config"), &path).unwrap(),
                "hardlink" => fs::hard_link(l.root.path().join("config"), &path).unwrap(),
                "directory" => fs::create_dir(&path).unwrap(),
                "privacy" => {
                    fs::set_permissions(&owner.plan.credentials, fs::Permissions::from_mode(0o777))
                        .unwrap();
                }
                "oversize" => l.write("credentials/identity.json", &vec![0; 8193]),
                "replace" => {
                    fs::rename(
                        l.root.path().join("config"),
                        l.root.path().join("old-config"),
                    )
                    .unwrap();
                    l.write("config", b"{}");
                }
                _ => unreachable!(),
            }
            assert!(reserve(&mut owner).is_err());
            probe(&owner, true);
        }
        // Same bytes in a different original staging object must still refuse.
        let path = fixture.root.path().join("custody/member-key");
        fs::rename(&path, fixture.root.path().join("retired-member")).unwrap();
        fs::write(&path, member.bytes()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let l = setup();
        let mut owner = Owner::acquire(l.plan("join")).unwrap();
        reserve(&mut owner).unwrap();
        let before = l.snapshot();
        assert_eq!(
            linux::store(
                &mut owner,
                Some(&expected),
                install,
                &root,
                &leaf,
                &member,
                Some(1_000_000)
            ),
            Err(Error::Context)
        );
        assert_eq!(l.snapshot(), before);
    }
}

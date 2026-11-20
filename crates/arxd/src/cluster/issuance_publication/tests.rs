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
        pki::custody,
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
    fn probe(path: &std::path::Path, locked: bool) {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cluster::issuance_publication::tests::files::publication_lock_probe",
                "--nocapture",
            ])
            .env("ARX_PUBLICATION_PROBE", path)
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
        let (custody, install, root, leaf, expected) = custody::publication_fixture(l.root.path());
        let member = custody::member_copy(&custody).unwrap();
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
        probe(&owner.plan.state.join("cluster-owner.lock"), true);
        let trace = owner.publication.trace.clone();
        assert_eq!(reserve(&mut owner), Err(Error::Completed));
        assert_eq!(owner.publication.trace, trace);
        let lock_path = owner.plan.state.join("cluster-owner.lock");
        drop(owner);
        probe(&lock_path, false);
        let recovered = Owner::acquire(l.plan("recover-joiner")).unwrap();
        probe(&recovered.plan.state.join("cluster-owner.lock"), true);
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
    fn publication_retained_artifact_must_match() {
        let l = setup();
        let (custody, install, root, leaf, expected) = custody::publication_fixture(l.root.path());
        let member = custody::member_copy(&custody).unwrap();
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
        for artifact in ["member-key", "sealed-key", "member-key"] {
            let path = l.root.path().join("credentials/identity.json");
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["artifact"] = serde_json::json!(artifact);
            l.write(
                "credentials/identity.json",
                &serde_json::to_vec(&value).unwrap(),
            );
            let before = l.snapshot();
            let recovered = Owner::acquire(l.plan("recover-joiner")).unwrap();
            assert_eq!(
                linux::check_retained(
                    &recovered,
                    Some(&expected),
                    install,
                    &member,
                    Some(1_000_000)
                ),
                if artifact == "member-key" {
                    Ok(())
                } else {
                    Err(Error::Context)
                }
            );
            assert_eq!(l.snapshot(), before);
        }
    }
    #[test]
    fn publication_durable_boundaries() {
        let fixture = setup();
        let (custody, install, root, leaf, expected) =
            custody::publication_fixture(fixture.root.path());
        let member = custody::member_copy(&custody).unwrap();
        for (step, point) in POINTS.iter().enumerate() {
            for unwind in [false, true] {
                let l = setup();
                let mut owner = Owner::acquire(l.plan("join")).unwrap();
                let original_guards = crate::cluster::startup_plan::tests::guards(&owner);
                let original_snapshot = l.snapshot();
                let encoded = PublicIdentity::assemble(&expected, &leaf).unwrap();
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
                assert_eq!(owner.publication.trace, POINTS[..=step]);
                assert_eq!(
                    crate::cluster::startup_plan::tests::guards(&owner),
                    original_guards
                );
                let done = |n: usize| step > n || (step == n && unwind);
                let replaced = done(19);
                let mut expected_files: Vec<(&str, &[u8])> = Vec::new();
                if done(0) {
                    expected_files.push(("identity.json", if replaced { &encoded } else { &[] }));
                }
                for (name, bytes, create, partial, full) in [
                    ("ca.der", root.as_slice(), 3, 4, 5),
                    ("member-key", member.bytes(), 8, 9, 10),
                    (".identity-publish", encoded.as_slice(), 13, 14, 15),
                ] {
                    if done(create) && !(name == ".identity-publish" && replaced) {
                        // Partial-write faults are observed after the real first write.
                        let len = if done(full) {
                            bytes.len()
                        } else if step >= partial {
                            bytes.len() / 2
                        } else {
                            0
                        };
                        expected_files.push((name, &bytes[..len]));
                    }
                }
                expected_files.sort_by_key(|(name, _)| *name);
                let objects = crate::cluster::startup_plan::tests::objects(&owner.plan.credentials);
                assert_eq!(objects.len(), expected_files.len());
                for (object, (name, bytes)) in objects.iter().zip(&expected_files) {
                    assert_eq!(object.0, *name);
                    assert!(object.3);
                    assert_eq!(object.4, bytes.len() as u64);
                    assert_eq!(fs::read(owner.plan.credentials.join(name)).unwrap(), *bytes);
                    // Retained descriptor and path must identify the same actual created object.
                    let guard = owner
                        .publication
                        .files
                        .iter()
                        .find(|g| {
                            use std::os::unix::fs::MetadataExt;
                            let m = g.file().metadata().unwrap();
                            m.dev() == object.1 && m.ino() == object.2
                        })
                        .unwrap();
                    guard.recheck().unwrap();
                }
                let before = l.snapshot();
                assert_eq!(
                    before
                        .iter()
                        .filter(|(n, _)| n.starts_with("state/"))
                        .collect::<Vec<_>>(),
                    original_snapshot
                        .iter()
                        .filter(|(n, _)| n.starts_with("state/"))
                        .collect::<Vec<_>>()
                );
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
                probe(&owner.plan.state.join("cluster-owner.lock"), true);
                drop(owner);
                let before = l.snapshot();
                if *point != "reservation-create" || unwind {
                    assert!(Owner::acquire(l.plan("join")).is_err());
                }
                let recovery = Owner::acquire(l.plan("recover-joiner"));
                // Exactly three safe names only after member create and before temp create,
                // or after atomic replacement. Acquisition is inventory-only.
                let inventory_complete = (done(8) && !done(13)) || replaced;
                assert_eq!(recovery.is_ok(), inventory_complete);
                if let Ok(recovered) = recovery {
                    let result = linux::check_retained(
                        &recovered,
                        Some(&expected),
                        install,
                        &member,
                        Some(1_000_000),
                    );
                    assert_eq!(result.is_ok(), replaced);
                    if replaced {
                        member.prove_possession();
                    }
                }
                assert_eq!(l.snapshot(), before);
            }
        }
    }
    #[test]
    fn publication_restart_partial_and_missing_history() {
        let fixture = setup();
        let (custody, install, root, leaf, expected) =
            custody::publication_fixture(fixture.root.path());
        let member = custody::member_copy(&custody).unwrap();
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
        let (custody, install, root, leaf, expected) = custody::publication_fixture(l.root.path());
        let member = custody::member_copy(&custody).unwrap();
        for (node, origin) in [
            (
                1,
                Origin::Found {
                    term: 2,
                    leader: 1,
                    index: 3,
                },
            ),
            (2, expected.origin.clone()),
        ] {
            let (bytes, root, valid) = crate::cluster::join_receipt::tests::fixture(node, origin);
            let record = PublicIdentity::decode(&bytes).unwrap();
            record
                .verify(&valid, &root, rustls::pki_types::UnixTime::now())
                .unwrap();
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let original = serde_json::to_vec(&value).unwrap();
            value["origin"]["role"] =
                serde_json::json!(if node == 1 { "joiner" } else { "founder" });
            let altered = serde_json::to_vec(&value).unwrap();
            assert_ne!(altered, original, "role for node {node}");
            assert!(PublicIdentity::decode(&altered).is_err());
        }
        let bytes = PublicIdentity::assemble(&expected, &leaf).unwrap();
        let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let original_bytes = serde_json::to_vec(&original).unwrap();
        for field in [
            "cluster", "node", "key", "ca", "term", "leader", "index", "role", "anchors",
        ] {
            let mut value = original.clone();
            match field {
                "cluster" | "key" | "ca" => {
                    let byte = &mut value[field][0];
                    *byte = (byte.as_u64().unwrap() ^ 1).into();
                }
                "node" => value[field] = (expected.node.get() + 1).into(),
                "role" => value["origin"]["role"] = "founder".into(),
                "anchors" => {
                    let byte = &mut value["origin"]["anchors"][0][1][0];
                    *byte = (byte.as_u64().unwrap() ^ 1).into();
                }
                "term" | "leader" | "index" => {
                    let number = &mut value["origin"][field];
                    *number = (number.as_u64().unwrap() + 1).into();
                }
                _ => unreachable!(),
            }
            let altered = serde_json::to_vec(&value).unwrap();
            assert_ne!(altered, original_bytes, "{field}");
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
                        .is_err(),
                    "{field}"
                );
            }
            // Unchanged AEAD/key remains valid despite edited public history.
            member
                .validate_bytes(member.bytes(), &expected, &root, &leaf, Some(1_000_000))
                .unwrap();
        }
        use crate::cluster::{
            enrollment::NodeId,
            join_receipt::{ExpectedIdentity, tests::different_key},
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
                "node" => bad.node = NodeId::new(expected.node.get() + 1).unwrap(),
                "key" => bad.key = different_key(expected.key),
                "root" => wrong_root[0] ^= 1,
                "role" => {
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
            custody::publication_fixture(fixture.root.path());
        let member = custody::member_copy(&custody).unwrap();
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
            probe(&owner.plan.state.join("cluster-owner.lock"), true);
        }
        for reserved in [false, true] {
            for name in ["credentials", "state", "ancestor"] {
                let l = setup();
                let mut owner = Owner::acquire(l.plan("join")).unwrap();
                if reserved {
                    reserve(&mut owner).unwrap();
                }
                if name == "ancestor" {
                    let old = l.root.path().join("retired-tree");
                    fs::create_dir(&old).unwrap();
                    for child in ["credentials", "state", "workspace", "config"] {
                        fs::rename(l.root.path().join(child), old.join(child)).unwrap();
                    }
                    // Replace the common ancestor using a nested disposable fixture.
                    let moved = l.root.path().with_extension("retired");
                    fs::rename(l.root.path(), &moved).unwrap();
                    fs::create_dir(l.root.path()).unwrap();
                    fs::set_permissions(l.root.path(), fs::Permissions::from_mode(0o700)).unwrap();
                    for child in ["credentials", "state", "workspace", "config"] {
                        fs::rename(
                            moved.join("retired-tree").join(child),
                            l.root.path().join(child),
                        )
                        .unwrap();
                    }
                    // Keep the retired ancestor alive until after refusal; dispose by TempDir.
                    fs::rename(&moved, l.root.path().join("retired-ancestor")).unwrap();
                } else {
                    let path = l.root.path().join(name);
                    fs::rename(&path, l.root.path().join(format!("retired-{name}"))).unwrap();
                    fs::create_dir(&path).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                }
                let before = l.snapshot();
                let result = if reserved {
                    linux::store(
                        &mut owner,
                        Some(&expected),
                        install,
                        &root,
                        &leaf,
                        &member,
                        Some(1_000_000),
                    )
                } else {
                    reserve(&mut owner)
                };
                assert_eq!(result, Err(Error::Layout));
                assert_eq!(l.snapshot(), before);
                assert!(
                    owner
                        .publication
                        .trace
                        .iter()
                        .all(|p| p.starts_with("reservation-"))
                );
            }
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

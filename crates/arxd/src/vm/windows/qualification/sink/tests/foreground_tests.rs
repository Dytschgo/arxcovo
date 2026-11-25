use super::*;
use crate::vm::windows::qualification::foreground::ReadDossier;

#[test]
fn existing_dossier_retains_read_only_seal_and_every_row() {
    let work = tempfile::tempdir().unwrap();
    let (path, dir) = claimed(work.path(), &[]);
    let (dependencies, recipes, bundle) = complete_test_fixture(&dir);
    for (kind, bytes) in &bundle.files {
        dir.commit(&kind.name(), bytes).unwrap();
    }
    dir.commit(COMPLETE, &bundle.seal).unwrap();
    let before = names(&path);
    let (user, _) = user();
    let owner = sid(&user);
    let dossier = ReadDossier::open(PathPin::open(&path, true, false).unwrap(), owner).unwrap();
    let selection = Selection {
        seal_sha256: records::digest(&bundle.seal),
        scope: dir.scope(),
    };
    let fresh = Fresh {
        now: TEST_BOOT.on_boot(NOW).unwrap(),
        boot_marker: TEST_BOOT.marker,
        dependencies: &dependencies,
    };
    let (verified, rows) = dossier.read_selected(&selection, &fresh).unwrap();
    assert_eq!(verified.recipes, recipes);
    assert_eq!(rows.len(), bundle.files.len() + 1);
    for name in
        std::iter::once(COMPLETE.to_owned()).chain(bundle.files.iter().map(|(kind, _)| kind.name()))
    {
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(path.join(&name).unwrap().as_str())
                .is_err()
        );
    }
    assert_eq!(names(&path), before);
}

#[test]
fn foreground_reader_refuses_a_writable_directory_pin() {
    let work = tempfile::tempdir().unwrap();
    let (owner, path) = private_dir(work.path(), "empty");
    let pin = PathPin::open(&path, true, true).unwrap();
    assert!(ReadDossier::open(pin, sid(&owner)).is_err());
}

#[test]
fn independent_remeasurement_mismatch_refuses_an_intact_seal() {
    let work = tempfile::tempdir().unwrap();
    let (_, dir) = claimed(work.path(), &[]);
    let (dependencies, _, bundle) = complete_test_fixture(&dir);
    let selection = Selection {
        seal_sha256: records::digest(&bundle.seal),
        scope: dir.scope(),
    };
    let fresh = Fresh {
        now: TEST_BOOT.on_boot(NOW).unwrap(),
        boot_marker: TEST_BOOT.marker,
        dependencies: &dependencies,
    };
    assert_eq!(
        records::verify_native(&bundle.seal, &bundle.files, &selection, &fresh),
        Ok(())
    );
    // The intact selected seal offers its own matching key. Verification
    // must use the independently measured projection despite that key.
    let mut drifted = dependencies.clone();
    drifted.firmware = records::Measured::Measured([39; 32]);
    let fresh = Fresh {
        dependencies: &drifted,
        ..fresh
    };
    assert_eq!(
        records::verify_native(&bundle.seal, &bundle.files, &selection, &fresh),
        Err(records::RunError::Untrusted)
    );
}

#[test]
fn read_only_pin_refuses_claim_before_debris_cleanup() {
    let work = tempfile::tempdir().unwrap();
    let (owner, path) = private_dir(work.path(), "unclaimed");
    let debris = temporary_name(CLAIM);
    std::fs::write(format!(r"{}\{debris}", path.as_str()), b"partial").unwrap();
    let cleanup = std::rc::Rc::new(Cell::new(false));
    AFTER_LISTING.with(|hook| {
        let cleanup = cleanup.clone();
        *hook.borrow_mut() = Some(Box::new(move || cleanup.set(true)));
    });
    let pin = PathPin::open(&path, true, false).unwrap();
    let claim = EvidenceDir::claim(pin, sid(&owner), &[], TEST_BOOT);
    drop(AFTER_LISTING.with(|hook| hook.borrow_mut().take()));
    assert!(claim.is_err());
    assert!(!cleanup.get());
    assert_eq!(names(&path), [debris]);
}

#[test]
fn old_contract_is_unselectable_even_with_valid_hash() {
    let work = tempfile::tempdir().unwrap();
    let (_, dir) = claimed(work.path(), &[]);
    let (dependencies, _, bundle) = complete_test_fixture(&dir);
    let fresh = Fresh {
        now: TEST_BOOT.on_boot(NOW).unwrap(),
        boot_marker: TEST_BOOT.marker,
        dependencies: &dependencies,
    };
    let verify = |seal: &[u8], fresh: &Fresh<'_>| {
        records::verify_native(
            seal,
            &bundle.files,
            &Selection {
                seal_sha256: records::digest(seal),
                scope: dir.scope(),
            },
            fresh,
        )
    };
    assert_eq!(verify(&bundle.seal, &fresh), Ok(()));
    let mut seal: records::Seal = records::decode(&bundle.seal).unwrap();
    seal.contract -= 1;
    let old_contract = records::canonical(&seal).unwrap();
    assert!(verify(&old_contract, &fresh).is_err());
    let mut old_layout: serde_json::Value = serde_json::from_slice(&old_contract).unwrap();
    let binding = old_layout["binding"].as_object_mut().unwrap();
    binding.remove("boot");
    binding.remove("issued_mono_ms");
    assert!(verify(&serde_json::to_vec(&old_layout).unwrap(), &fresh).is_err());
}

#[test]
fn reboot_refuses_even_when_utc_and_uptime_match() {
    let work = tempfile::tempdir().unwrap();
    let (_, dir) = claimed(work.path(), &[]);
    let (dependencies, _, bundle) = complete_test_fixture(&dir);
    let selection = Selection {
        seal_sha256: records::digest(&bundle.seal),
        scope: dir.scope(),
    };
    let fresh = Fresh {
        now: TEST_BOOT.on_boot(NOW).unwrap(),
        boot_marker: TEST_BOOT.marker,
        dependencies: &dependencies,
    };
    assert_eq!(
        records::verify_native(&bundle.seal, &bundle.files, &selection, &fresh),
        Ok(())
    );
    assert_eq!(
        records::verify_native(
            &bundle.seal,
            &bundle.files,
            &selection,
            &Fresh {
                boot_marker: [2; 32],
                ..fresh
            }
        ),
        Err(records::RunError::Untrusted)
    );
}

#[test]
fn rebased_completion_must_precede_fresh_uptime() {
    let work = tempfile::tempdir().unwrap();
    let (_, dir) = claimed(work.path(), &[]);
    let (dependencies, _, bundle) = complete_test_fixture(&dir);
    let selection = Selection {
        seal_sha256: records::digest(&bundle.seal),
        scope: dir.scope(),
    };
    let early = records::Instant2 {
        utc: NOW.utc,
        mono_ms: 1,
    };
    assert!(
        records::clocks_agree(ISSUE, early),
        "clock skew alone permits this sample"
    );
    let fresh = Fresh {
        now: TEST_BOOT.on_boot(early).unwrap(),
        boot_marker: TEST_BOOT.marker,
        dependencies: &dependencies,
    };
    assert_eq!(
        records::verify_native(&bundle.seal, &bundle.files, &selection, &fresh),
        Err(records::RunError::Untrusted)
    );
}

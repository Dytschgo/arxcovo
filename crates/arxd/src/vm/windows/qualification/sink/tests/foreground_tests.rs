use super::*;
use crate::vm::windows::qualification::foreground::ReadDossier;

#[test]
fn docker_dossier_reads_only_its_own_class_and_exact_rows() {
    use records::*;
    let work = tempfile::tempdir().unwrap();
    let (path, dir) = claimed(work.path(), &[]);
    let (test_dependencies, _, test_bundle) = complete_test_fixture(&dir);
    let build = crate::vm::public_egress::REVIEWED_EGRESS_BUILDS[0];
    let recipe = DockerRecipe {
        identity: DockerRecipe::identity([9; 32]),
        web: [8; 32],
        closure: [9; 32],
        medium: [10; 32],
    };
    let mut dependencies = DockerDependencies {
        supervisor_image: [1; 32],
        host: HostDependency {
            windows_build: 26_100,
            windows_revision: 1,
            machine: [1; 16],
        },
        owner_logon: [2; 32],
        production_store: [3; 32],
        disposable_store: [4; 32],
        profile: [5; 32],
        installer: [6; 32],
        firmware: [7; 32],
        recipe,
        compose_protocol: arx_guest::compose::COMPOSE_PROTOCOL_VERSION,
        origin_profile: [11; 32],
        manifest: [12; 32],
        executable: build.qemu.sha256,
        receipt: build.receipt.sha256,
        loaded_artifacts: [13; 32],
        policy: [14; 32],
        topology: [15; 32],
    };
    let test_seal: Seal = decode(&test_bundle.seal).unwrap();
    let binding = Binding {
        class: Class::Docker,
        dependencies: dependencies.key(dir.scope()).unwrap(),
        ..test_seal.binding
    };
    let baseline: Vec<Record> = test_bundle
        .files
        .iter()
        .flat_map(|(_, bytes)| decode::<Vec<Record>>(bytes).unwrap())
        .collect();
    let bundle = |policy| {
        let mut prior = [0; 32];
        let rows: Vec<_> = stages(Class::Docker)
            .iter()
            .copied()
            .enumerate()
            .map(|(i, stage)| {
                let result = match stage {
                    Stage::Distribution => Observation::DockerDistribution {
                        manifest: [12; 32],
                        executable: build.qemu.sha256,
                        receipt: build.receipt.sha256,
                        loaded_artifacts: [13; 32],
                    },
                    Stage::DockerRecipe => Observation::DockerRecipeVerified {
                        recipe,
                        restricted_install: true,
                        detached_health: true,
                        independent_pair: true,
                    },
                    Stage::PublicIpv4Network => Observation::DockerPublicIpv4 {
                        policy,
                        topology: [15; 32],
                        receiver_controls: true,
                        public_tcp_udp: true,
                        nonpublic_denied: true,
                        ipv6_icmp_denied: true,
                        route_drift_denied: true,
                    },
                    Stage::InstalledGuestEndpoint => Observation::DockerInstalledEndpoint {
                        recipe: recipe.identity,
                        compose_protocol: arx_guest::compose::COMPOSE_PROTOCOL_VERSION,
                        exact_pipe_peer: true,
                        pinned_origin_tls: true,
                        clean_stop: true,
                    },
                    _ => {
                        let row = baseline.iter().find(|row| row.stage == stage).unwrap();
                        let StageResult::Passed(result) = &row.result else {
                            unreachable!()
                        };
                        result.clone()
                    }
                };
                let row = Record {
                    contract: CONTRACT,
                    collector: COLLECTOR,
                    origin: Origin::Native,
                    binding,
                    stage,
                    started_ms: i as u64 * 100,
                    finished_ms: i as u64 * 100 + 16,
                    prior,
                    result: StageResult::Passed(result),
                };
                prior = digest(&canonical(&row).unwrap());
                row
            })
            .collect();
        produce_bundle(Origin::Native, binding, &rows, NOW.utc).unwrap()
    };
    let good = bundle(dependencies.policy);
    let wrong_policy = bundle([99; 32]);
    for (kind, bytes) in &good.files {
        dir.commit(&kind.name(), bytes).unwrap();
    }
    dir.commit(COMPLETE, &good.seal).unwrap();
    let selection = DockerSelection {
        seal_sha256: digest(&good.seal),
        scope: dir.scope(),
    };
    let now = TEST_BOOT.on_boot(NOW).unwrap();
    let fresh = DockerFresh {
        now,
        boot_marker: TEST_BOOT.marker,
        dependencies: &dependencies,
    };
    let (user, _) = user();
    let dossier = crate::vm::windows::qualification::static_compose::StaticDossier::open(
        &path,
        sid(&user),
        &selection,
        &fresh,
    )
    .unwrap();
    assert_eq!(dossier.evidence.recipe(), recipe);
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(
                path.join(&FileKind::NetworkPublicIpv4.name())
                    .unwrap()
                    .as_str()
            )
            .is_err()
    );

    let read = |bundle: &SealBundle, fresh: &DockerFresh<'_>| {
        read_native_docker(
            bundle.seal.as_slice(),
            bundle.seal.len() as u64,
            |kind| {
                bundle
                    .files
                    .iter()
                    .find(|(file, _)| *file == kind)
                    .map(|(_, data)| data.as_slice())
                    .ok_or(RunError::Output)
            },
            &DockerSelection {
                seal_sha256: digest(&bundle.seal),
                ..selection
            },
            fresh,
        )
    };
    assert_eq!(read(&wrong_policy, &fresh).err(), Some(RunError::Untrusted));
    assert_eq!(read(&test_bundle, &fresh).err(), Some(RunError::Untrusted));
    assert!(
        read(
            &good,
            &DockerFresh {
                boot_marker: [99; 32],
                ..fresh
            }
        )
        .is_err()
    );
    assert!(
        read(
            &good,
            &DockerFresh {
                now: Instant2 {
                    utc: binding.expires_utc,
                    mono_ms: now.mono_ms + MAX_VALIDITY * 1000
                },
                ..fresh
            }
        )
        .is_err()
    );
    let test_fresh = Fresh {
        now,
        boot_marker: TEST_BOOT.marker,
        dependencies: &test_dependencies,
    };
    assert!(
        read_native::<&[u8]>(
            good.seal.as_slice(),
            good.seal.len() as u64,
            |_| panic!("wrong class opened a row"),
            &Selection {
                seal_sha256: selection.seal_sha256,
                scope: selection.scope
            },
            &test_fresh,
        )
        .is_err()
    );
    dependencies.executable = crate::vm::public_egress::STOCK_QEMU_1113.sha256;
    assert!(dependencies.key(dir.scope()).is_none());
}

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

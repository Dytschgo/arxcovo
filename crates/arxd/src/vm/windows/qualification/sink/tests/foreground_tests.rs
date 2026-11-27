use super::*;
use crate::vm::windows::qualification::foreground::ReadDossier;

fn docker_fixture(
    dir: &EvidenceDir,
) -> (
    records::DockerDependencies,
    records::SealBundle,
    records::SealBundle,
) {
    use records::*;
    let (_, _, test_bundle) = complete_test_fixture(dir);
    let build = crate::vm::public_egress::REVIEWED_EGRESS_BUILDS[0];
    let recipe = DockerRecipe {
        identity: DockerRecipe::identity([9; 32]),
        web: [8; 32],
        closure: [9; 32],
        medium: [10; 32],
    };
    let dependencies = DockerDependencies {
        supervisor_image: [1; 32],
        qualifier_image: [16; 32],
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
        guest: [17; 32],
        firmware: [7; 32],
        stock_inventory: [18; 32],
        network_fixture: [19; 32],
        signed_inputs: [21; 32],
        recipe,
        compose_protocol: arx_guest::compose::COMPOSE_PROTOCOL_VERSION,
        origin_profile: [11; 32],
        manifest: build.manifest.sha256,
        executable: build.qemu.sha256,
        receipt: build.receipt.sha256,
        loaded_artifacts: build.dependency_inventory(),
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
                        manifest: build.manifest.sha256,
                        executable: build.qemu.sha256,
                        receipt: build.receipt.sha256,
                        loaded_artifacts: build.dependency_inventory(),
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
                        receiver_witness: [20; 32],
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
    (dependencies, good, wrong_policy)
}

#[test]
fn docker_dossier_reads_only_its_own_class_and_exact_rows() {
    use records::*;

    // Match service startup so the enrollment file is owned by TokenUser on elevated CI.
    crate::service::windows::set_current_token_owner().expect("TokenOwner = TokenUser");
    let work = tempfile::tempdir().unwrap();
    let (path, dir) = claimed(work.path(), &[]);
    let (test_dependencies, _, test_bundle) = complete_test_fixture(&dir);
    let (mut dependencies, good, wrong_policy) = docker_fixture(&dir);
    let recipe = dependencies.recipe;
    let binding = decode::<Seal>(&good.seal).unwrap().binding;
    let cancel = crate::service::Shutdown::default();
    let on_boot = TEST_BOOT.on_boot(NOW).unwrap();
    let sample = || {
        Ok(DockerPublishSample {
            dependencies: &dependencies,
            captured_uptime_ms: on_boot.mono_ms,
            settled: true,
        })
    };
    dir.publish_docker_with_clock(&good, ISSUE, &cancel, sample, || {
        Ok((on_boot, TEST_BOOT.marker))
    })
    .unwrap();
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
    let (_, private) = private_dir(work.path(), "enrollment");
    let enrollment = private.join("docker.json").unwrap();
    std::fs::write(enrollment.as_str(), br#"{"class":"docker"}"#).unwrap();
    let pin = PathPin::open(&path, true, false).unwrap();
    let expected = crate::config::fixture_directory_pin(&pin.leaf);
    let pending = PreparedStaticDossier::hold(&path, &expected, &enrollment, selection).unwrap();
    assert_eq!(
        pending
            .select(Err(NativeRefusal::NetworkFenceUnverified))
            .err(),
        Some(NativeRefusal::NetworkFenceUnverified),
    );
    assert!(
        pending
            .select(Ok(DockerFresh {
                boot_marker: [99; 32],
                ..fresh
            }))
            .is_err()
    );
    let dossier = pending.select(Ok(DockerFresh { ..fresh })).unwrap();
    assert_eq!(dossier.evidence.recipe(), recipe);
    assert_eq!(dossier.seal_sha256, selection.seal_sha256);
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
                    utc: NOW.utc,
                    mono_ms: TEST_BOOT.origin_mono_ms + 1800,
                },
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
    // Give each wrong-class seal the expected key so the class check, rather
    // than a coincidental dependency mismatch, must stop it before any read.
    let mut forged = decode::<Seal>(&good.seal).unwrap();
    forged.binding.dependencies = test_dependencies.key(dir.scope());
    let forged = canonical(&forged).unwrap();
    assert!(
        read_native::<&[u8]>(
            forged.as_slice(),
            forged.len() as u64,
            |_| panic!("wrong class opened a row"),
            &Selection {
                seal_sha256: digest(&forged),
                scope: selection.scope
            },
            &test_fresh,
        )
        .is_err()
    );
    let mut forged = decode::<Seal>(&test_bundle.seal).unwrap();
    forged.binding.dependencies = dependencies.key(dir.scope()).unwrap();
    let forged = canonical(&forged).unwrap();
    assert!(
        read_native_docker::<&[u8]>(
            forged.as_slice(),
            forged.len() as u64,
            |_| panic!("wrong class opened a row"),
            &DockerSelection {
                seal_sha256: digest(&forged),
                scope: dir.scope()
            },
            &fresh,
        )
        .is_err()
    );
    dependencies.executable = crate::vm::public_egress::STOCK_QEMU_1113.sha256;
    assert!(dependencies.key(dir.scope()).is_none());
    dependencies.executable = crate::vm::public_egress::REVIEWED_EGRESS_BUILDS[0]
        .qemu
        .sha256;
    dependencies.manifest = [12; 32];
    assert!(dependencies.key(dir.scope()).is_none());
}

#[test]
fn docker_marker_requires_a_fresh_settled_readback_and_create_new_rename() {
    let work = tempfile::tempdir().unwrap();
    let now = TEST_BOOT.on_boot(NOW).unwrap();
    for case in [
        "cancelled",
        "unsettled",
        "stale",
        "reboot",
        "clock",
        "fault",
        "collision",
        "private",
    ] {
        let canaries: &[&str] = if case == "private" {
            &["public_ipv4_network"]
        } else {
            &[]
        };
        let (owner, path) = private_dir(work.path(), case);
        let dir = EvidenceDir::claim(
            PathPin::open(&path, true, true).unwrap(),
            sid(&owner),
            canaries,
            TEST_BOOT,
        )
        .unwrap();
        let (dependencies, bundle, _) = docker_fixture(&dir);
        let cancel = crate::service::Shutdown::default();
        let samples = Cell::new(0);
        if case == "fault" {
            FAULT.with(|fault| fault.set(Some((COMPLETE, Step::Flush))));
        }
        let result = dir.publish_docker_with_clock(
            &bundle,
            ISSUE,
            &cancel,
            || {
                let step = samples.get() + 1;
                samples.set(step);
                if step == 3 {
                    assert!(
                        std::fs::OpenOptions::new()
                            .write(true)
                            .open(path.join("network-public-ipv4.json").unwrap().as_str())
                            .is_err()
                    );
                    match case {
                        "cancelled" => cancel.cancel(),
                        "collision" => std::fs::write(
                            path.join(COMPLETE).unwrap().as_str(),
                            b"already present",
                        )
                        .unwrap(),
                        _ => {}
                    }
                }
                Ok(DockerPublishSample {
                    dependencies: &dependencies,
                    captured_uptime_ms: now.mono_ms,
                    settled: case != "unsettled" || step != 3,
                })
            },
            || {
                let at_rename = samples.get() == 3;
                let mut clock = now;
                if case == "clock" && at_rename {
                    clock.utc += 20;
                }
                if case == "stale" && at_rename {
                    clock.utc += 6;
                    clock.mono_ms += 6_000;
                }
                Ok((
                    clock,
                    if case == "reboot" && at_rename {
                        [99; 32]
                    } else {
                        TEST_BOOT.marker
                    },
                ))
            },
        );
        FAULT.with(|fault| fault.set(None));
        assert!(result.is_err(), "{case}");
        if case == "collision" {
            assert_eq!(read(&path, COMPLETE), b"already present");
        } else {
            assert!(!names(&path).contains(&COMPLETE.to_owned()), "{case}");
        }
        assert!(!names(&path).contains(&temporary_name(COMPLETE)), "{case}");
    }
}

#[test]
fn docker_selection_rejects_modified_rows_even_with_updated_manifest_pins() {
    use records::*;
    let work = tempfile::tempdir().unwrap();
    let (_, dir) = claimed(work.path(), &[]);
    let (dependencies, bundle, _) = docker_fixture(&dir);
    let fresh = DockerFresh {
        now: TEST_BOOT.on_boot(NOW).unwrap(),
        boot_marker: TEST_BOOT.marker,
        dependencies: &dependencies,
    };
    let original: Seal = decode(&bundle.seal).unwrap();
    let files = &bundle.files;
    for (kind, file) in [
        (DiagnosticFile::Distribution, FileKind::Distribution),
        (
            DiagnosticFile::NetworkPublicIpv4,
            FileKind::NetworkPublicIpv4,
        ),
        (DiagnosticFile::RecipeDocker, FileKind::RecipeDocker),
        (DiagnosticFile::WhpxQmpEndpoint, FileKind::WhpxQmpEndpoint),
    ] {
        let (_, bytes) = files.iter().find(|(id, _)| *id == file).unwrap();
        assert!(schema::check(kind, bytes), "diagnostic schema: {kind:?}");
    }
    let group = FileKind::CustodyJobCapacity;
    let source = &files.iter().find(|(kind, _)| *kind == group).unwrap().1;
    let read = |mut seal: Seal, replacement: &[u8]| {
        let entry = seal
            .manifest
            .iter_mut()
            .find(|entry| entry.file == group)
            .unwrap();
        entry.length = replacement.len() as u64;
        entry.sha256 = digest(replacement);
        let bytes = canonical(&seal).unwrap();
        read_native_docker(
            bytes.as_slice(),
            bytes.len() as u64,
            |kind| {
                if kind == group {
                    Ok(replacement)
                } else {
                    files
                        .iter()
                        .find(|(file, _)| *file == kind)
                        .map(|(_, bytes)| bytes.as_slice())
                        .ok_or(RunError::Output)
                }
            },
            &DockerSelection {
                seal_sha256: digest(&bytes),
                scope: dir.scope(),
            },
            &fresh,
        )
    };
    assert!(read(original.clone(), source).is_ok());
    for case in ["duplicate", "reordered", "missing", "unknown", "fixture"] {
        let mut seal = original.clone();
        let mut rows: Vec<Record> = decode(source).unwrap();
        match case {
            "duplicate" => rows.insert(0, rows[0].clone()),
            "reordered" => rows.swap(0, 1),
            "missing" => {
                rows.pop();
            }
            "fixture" => seal.origin = Origin::Fixture,
            _ => {}
        }
        let bytes = canonical(&rows).unwrap();
        let unknown = String::from_utf8(bytes.clone())
            .unwrap()
            .replace("\"disposable_store\"", "\"unknown_docker_stage\"");
        assert!(
            read(
                seal,
                if case == "unknown" {
                    unknown.as_bytes()
                } else {
                    &bytes
                }
            )
            .is_err(),
            "{case}"
        );
    }
}

#[test]
fn docker_dependency_key_requires_each_input_role() {
    use records::{DockerDependencies, Hash};

    let work = tempfile::tempdir().unwrap();
    let (_, dir) = claimed(work.path(), &[]);
    let (dependencies, _, _) = docker_fixture(&dir);
    assert!(dependencies.key(dir.scope()).is_some());
    let roles: [fn(&mut DockerDependencies) -> &mut Hash; 21] = [
        |d| &mut d.supervisor_image,
        |d| &mut d.qualifier_image,
        |d| &mut d.owner_logon,
        |d| &mut d.production_store,
        |d| &mut d.disposable_store,
        |d| &mut d.profile,
        |d| &mut d.installer,
        |d| &mut d.guest,
        |d| &mut d.firmware,
        |d| &mut d.stock_inventory,
        |d| &mut d.network_fixture,
        |d| &mut d.signed_inputs,
        |d| &mut d.origin_profile,
        |d| &mut d.manifest,
        |d| &mut d.executable,
        |d| &mut d.receipt,
        |d| &mut d.loaded_artifacts,
        |d| &mut d.policy,
        |d| &mut d.topology,
        |d| &mut d.recipe.web,
        |d| &mut d.recipe.medium,
    ];
    for (index, role) in roles.into_iter().enumerate() {
        let mut missing = dependencies.clone();
        *role(&mut missing) = [0; 32];
        assert!(missing.key(dir.scope()).is_none(), "input role {index}");
    }
    let mut inconsistent = dependencies.clone();
    inconsistent.recipe.closure = [22; 32];
    assert!(inconsistent.key(dir.scope()).is_none());
    inconsistent.recipe = dependencies.recipe;
    inconsistent.production_store = inconsistent.disposable_store;
    assert!(inconsistent.key(dir.scope()).is_none());
    inconsistent = dependencies.clone();
    inconsistent.host.machine = [0; 16];
    assert!(inconsistent.key(dir.scope()).is_none());
    inconsistent.host = dependencies.host.clone();
    inconsistent.host.windows_build = 21_999;
    assert!(inconsistent.key(dir.scope()).is_none());
    inconsistent.host = dependencies.host.clone();
    inconsistent.compose_protocol = 0;
    assert!(inconsistent.key(dir.scope()).is_none());
    assert!(dependencies.key([0; 32]).is_none());
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

#[path = "docker_collector_tests.rs"]
mod docker_collector;

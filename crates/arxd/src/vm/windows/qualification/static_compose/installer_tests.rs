use super::*;
use crate::vm::template::{AssetPin, ubuntu_compose::PackagePin, ubuntu_web::PoolPackage};
use std::fs::{self, OpenOptions};

const GUEST: &[u8] = b"fake-arx-guest";
const INSTALLER: &[u8] = b"installer fixture";
const GENERATION: Id = [1; 16];

struct Fixture<'a> {
    base: UbuntuRecipe,
    pool: IsoPoolManifest<'a>,
    closure: Closure<'a>,
    signed: SignedInputPaths<'a>,
    paths: ComposeAssetPaths<'a>,
}

impl Fixture<'_> {
    fn hold(&self) -> Result<ComposeInstallerInputs, NativeRefusal> {
        ComposeInstallerInputs::hold_for(
            &self.base,
            &self.pool,
            &self.closure,
            &self.signed,
            &self.paths,
            GENERATION,
            ExpectedIso::Fixture(digest(INSTALLER)),
        )
    }
}

fn with_fixture(test: impl FnOnce(Fixture<'_>)) {
    fn asset(bytes: &[u8]) -> AssetPin {
        AssetPin {
            length: bytes.len() as u64,
            sha256: digest(bytes),
        }
    }

    fn artifact(bytes: &[u8]) -> ArtifactPin {
        let pin = asset(bytes);
        ArtifactPin {
            size: pin.length,
            sha256: pin.sha256,
        }
    }

    let guest = GUEST;
    let iso = asset(INSTALLER);
    let base = UbuntuRecipe {
        iso: Some(iso),
        kernel: Some(asset(b"kernel")),
        initrd: Some(asset(b"initrd")),
        guest: Some(asset(guest)),
        service: Some(asset(UbuntuRecipe::SERVICE)),
        finalize: Some(asset(UbuntuRecipe::FINALIZE)),
        nocloud_schema: Some(UbuntuRecipe::NOCLOUD_PROFILE_PIN),
        firmware_code: Some(asset(b"code")),
        firmware_vars: Some(asset(b"vars")),
        qemu_receipt: Some(asset(b"receipt")),
    };
    let pool = IsoPoolManifest {
        iso,
        signed_metadata: artifact(b"ubuntu-index"),
        packages: &[PoolPackage {
            name: "nginx",
            version: "1.0",
            relative_path: "pool/nginx.deb",
            artifact: artifact(b"nginx"),
        }],
    };
    let medium_bytes = b"fake read-only Compose dependency ISO";
    let packages = [
        "containerd.io",
        "docker-ce-cli",
        "docker-ce",
        "docker-compose-plugin",
    ]
    .map(|name| PackagePin {
        name,
        version: "5:1.2.3-1~noble",
        archive: artifact(b"deb"),
    });
    let closure = Closure {
        docker_fingerprint: [1; 20],
        docker_signing_key: artifact(b"key"),
        signed_docker_metadata: artifact(b"docker-index"),
        signed_ubuntu_metadata: pool.signed_metadata,
        base_versions: artifact(b"dpkg-query receipt"),
        medium: artifact(medium_bytes),
        packages: &packages,
    };
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        LocalPath::parse(path.to_str().unwrap()).unwrap()
    };
    // This synthetic enrollment is pinned independently of the bytes read by the adapter.
    let hash = |s: &str| -> Hash { hex::decode(s).unwrap().try_into().unwrap() };
    let ubuntu_key = b"ubuntu key";
    let review = SignedInputReview {
        contract: 1,
        web_pool: hash("8c6e6b2f8591b25e0c25b5baf777c298c8fd26e0050f6a70713c2f2b8fb3b785"),
        compose_closure: hash("85d2cc79ba41731f47070ca02acbe610fcb2f0ad5ff9a4c0a539d40b693d232f"),
        ubuntu_keyring_sha256: digest(ubuntu_key),
        ubuntu_keyring_size: ubuntu_key.len() as u64,
    };
    let review_bytes = serde_json::to_vec(&review).unwrap();
    let signed = SignedInputPaths {
        receipt: EnrolledFile {
            path: &write("review", &review_bytes),
            pin: ArtifactPin {
                size: 439,
                sha256: hash("5db13e27c60e6ff616a9a685d31dcefad60fd6946b43168dec4fa709c8c5d1b0"),
            },
        },
        docker_keyring: &write("docker-keyring", b"key"),
        ubuntu_keyring: &write("ubuntu-keyring", ubuntu_key),
        docker_metadata: &write("docker-index", b"docker-index"),
        ubuntu_metadata: &write("ubuntu-index", b"ubuntu-index"),
        web_metadata: &write("web-index", b"ubuntu-index"),
        base_versions: &write("base-versions", b"dpkg-query receipt"),
    };
    let paths = ComposeAssetPaths {
        medium: &write("compose.iso", medium_bytes),
        installer: &write("installer", INSTALLER),
        kernel: &write("kernel", b"kernel"),
        initrd: &write("initrd", b"initrd"),
        qemu_receipt: &write("receipt", b"receipt"),
        guest: &write("guest", guest),
        firmware_code: &write("code", b"code"),
        firmware_vars: &write("vars", b"vars"),
    };
    test(Fixture {
        base,
        pool,
        closure,
        signed,
        paths,
    });
}

#[test]
fn installer_hold_refuses_a_mismatched_selected_recipe() {
    with_fixture(|f| {
        assert!(
            ComposeInstallerInputs::hold(
                &f.base, &f.pool, &f.closure, &f.signed, &f.paths, GENERATION
            )
            .is_err()
        );
        let held = f.hold().unwrap();
        held.recheck().unwrap();
        for contents in [
            GUEST,
            b"instance-id: arx-install-01010101010101010101010101010101",
        ] {
            assert!(
                held.seed
                    .windows(contents.len())
                    .any(|bytes| bytes == contents)
            );
        }
        let matched = held.recipe;
        let hold = |recipe| {
            held.require_selected(&VerifiedDockerEvidence::recipe_fixture(recipe))?;
            Ok::<_, NativeRefusal>(held.detached_health.as_str())
        };
        assert!(hold(matched).unwrap().contains("docker-compose-plugin"));

        let mut wrong_medium = matched;
        wrong_medium.medium[0] ^= 0xff;
        let mut wrong_web = matched;
        wrong_web.web[0] ^= 0xff;
        let mut wrong_identity = matched;
        wrong_identity.identity = DockerRecipe::identity([9; 32]);
        for recipe in [wrong_medium, wrong_web, wrong_identity] {
            assert!(hold(recipe).is_err());
        }
    });
}

#[test]
fn boot_and_receipt_substitution_refused_before_rendering() {
    with_fixture(|f| {
        for path in [f.paths.kernel, f.paths.initrd, f.paths.qemu_receipt] {
            let path = path.as_str();
            let bytes = fs::read(path).unwrap();
            let alias = std::path::Path::new(path).with_extension("alias");
            fs::hard_link(path, &alias).unwrap();
            assert!(f.hold().is_err(), "accepted a multiply linked {path}");
            fs::remove_file(alias).unwrap();
            fs::remove_file(path).unwrap();
            assert!(f.hold().is_err(), "missing {path}");
            fs::write(path, &bytes).unwrap();
            drop(f.hold().unwrap());
            let mut changed = bytes.clone();
            changed[0] ^= 1;
            fs::write(path, changed).unwrap();
            assert!(f.hold().is_err(), "accepted changed bytes at {path}");
            fs::write(path, &bytes[..bytes.len() - 1]).unwrap();
            assert!(f.hold().is_err(), "accepted short file at {path}");
            fs::write(path, &bytes).unwrap();
            drop(f.hold().unwrap());
        }
    });
}

#[test]
fn boot_files_and_receipt_exclude_writers_until_drop_and_require_fresh_hashes() {
    with_fixture(|f| {
        for path in [f.paths.kernel, f.paths.initrd, f.paths.qemu_receipt] {
            let path = std::path::Path::new(path.as_str());
            let open_writer = || OpenOptions::new().write(true).open(path);
            let writer = open_writer().unwrap();
            assert!(f.hold().is_err());
            drop(writer);
            let held = f.hold().unwrap();
            let bytes = fs::read(path).unwrap();
            for refusal in [
                open_writer().unwrap_err(),
                fs::rename(path, path.with_extension("moved")).unwrap_err(),
                fs::remove_file(path).unwrap_err(),
            ] {
                assert_eq!(refusal.raw_os_error(), Some(32));
            }
            drop(held);
            let mut changed = bytes.clone();
            changed[0] ^= 1;
            fs::write(path, changed).unwrap();
            assert!(f.hold().is_err(), "reacquired changed {path:?}");
            fs::write(path, bytes).unwrap();
            drop(f.hold().unwrap());
        }
    });
}

#[test]
fn lost_boot_proof_refuses_selection_and_artifact_projection() {
    with_fixture(|f| {
        for role in [ArtifactRole::UbuntuKernel, ArtifactRole::UbuntuInitrd] {
            let mut held = f.hold().unwrap();
            let selected = VerifiedDockerEvidence::recipe_fixture(held.recipe);
            held.dependency_artifact().unwrap();
            let boot = if role == ArtifactRole::UbuntuKernel {
                &mut held.kernel
            } else {
                &mut held.initrd
            };
            boot.verified_content = None;
            assert!(
                held.require_selected(&selected).is_err(),
                "selected with missing {role:?} proof"
            );
            assert!(
                held.dependency_artifact().is_err(),
                "projected with missing {role:?} proof"
            );
        }
    });
}

#[test]
fn stock_inventory_refuses_a_different_verified_receipt() {
    with_fixture(|f| {
        let held = f.hold().unwrap();
        let inventory = inventory::tests::Fixture::new();
        for bytes in [b"receipt", b"another"] {
            fs::write(inventory.root.path().join("RECEIPT.txt"), bytes).unwrap();
            let (stock, _bundle) = inventory
                .collect_with_receipt(ArtifactPin {
                    size: bytes.len() as u64,
                    sha256: digest(bytes),
                })
                .unwrap();
            assert_eq!(held.stock_inventory(&stock).is_ok(), bytes == b"receipt");
        }
    });
}

struct MeasureEnv {
    dir: tempfile::TempDir,
    stock_fixture: inventory::tests::Fixture,
    patched_fixture: inventory::tests::Fixture,
    scope: test::Scope,
    token: VmToken,
    route: RouteObservation,
    policy_bytes: Vec<u8>,
    topology_bytes: Vec<u8>,
    manifest_bytes: Vec<u8>,
}

impl MeasureEnv {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let scope_dir = dir.path().join("scope");
        fs::create_dir(&scope_dir).unwrap();
        let scope = test::Scope::fixture(&scope_dir);
        let token = VmToken::test_current_user();
        let stock_fixture = inventory::tests::Fixture::new();
        let patched_fixture = inventory::tests::Fixture::new();
        for fixture in [&stock_fixture, &patched_fixture] {
            for path in [fixture.root.path(), &fixture.root.path().join("share")] {
                inventory::tests::set_dacl(path, inventory::tests::READ_ONLY);
            }
        }
        let route = RouteObservation {
            interface_index: 1,
            interface_luid: 2,
            source: "192.0.2.2".parse().unwrap(),
            gateway: "192.0.2.1".parse().unwrap(),
            default_route: true,
        };
        let body = concat!(
            "version=1\ninterface_index=1\ninterface_luid=2\n",
            "source=192.0.2.2\ngateway=192.0.2.1\n",
        );
        let policy_bytes = format!(
            "{body}fingerprint={}\n",
            hex::encode(digest(body.as_bytes()))
        )
        .into_bytes();
        let topology_bytes = b"reviewed-topology".to_vec();
        let manifest_bytes =
            serde_json::to_vec(&super::super::docker_trial::witness::fixture_targets()).unwrap();
        Self {
            dir,
            stock_fixture,
            patched_fixture,
            scope,
            token,
            route,
            policy_bytes,
            topology_bytes,
            manifest_bytes,
        }
    }

    fn hold_named(&self, name: &str, bytes: &[u8]) -> PathPin {
        let path = self.dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        let local_path = local(path.to_str().unwrap()).unwrap();
        hold_file(
            &local_path,
            ArtifactPin {
                size: bytes.len() as u64,
                sha256: digest(bytes),
            },
            MAX_METADATA,
        )
        .unwrap()
    }

    fn inputs<'a>(&'a self, recipe: &'a ComposeInstallerInputs) -> HeldDockerInputs<'a> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let stock = self.stock_fixture.inspect(deadline).unwrap();
        let patched = inventory::PatchedBundle {
            bundle: self.patched_fixture.inspect(deadline).unwrap(),
            review: crate::vm::public_egress::REVIEWED_EGRESS_BUILDS[0],
        };
        HeldDockerInputs {
            release: [
                self.hold_named("arxd.exe", b"daemon-binary"),
                self.hold_named("arx-vm-qualify.exe", b"qualifier-binary"),
            ],
            scope: &self.scope,
            token: &self.token,
            recipe,
            m4_seal: [0x63; 32],
            profile: self.hold_named("profile.json", b"profile"),
            policy: self.hold_named("policy.txt", &self.policy_bytes),
            parsed_policy: PublicEgressPolicy::parse(&self.policy_bytes).unwrap(),
            patched,
            stock,
            stock_inventory: [0x11; 32],
            fixture: [
                self.hold_named("fixture.img", b"fixture-image"),
                self.hold_named("fixture-net.json", b"fixture-network"),
                self.hold_named("probe.img", b"probe-image"),
            ],
            receiver_inventory: self.hold_named("receivers.json", &self.manifest_bytes),
            topology: self.hold_named("topology.json", &self.topology_bytes),
            origin_profile: self.hold_named("origin.json", b"origin-profile"),
        }
    }

    fn live_topology(&self) -> Hash {
        digest(&self.topology_bytes)
    }

    fn assert_measure_passes(&self, inputs: &HeldDockerInputs<'_>, expected_recipe: DockerRecipe) {
        let result = inputs.measure(&self.route, self.live_topology());
        if platform().is_ok() {
            let deps = result.expect("measure should succeed on Windows 11 workstation");
            assert_eq!(deps.recipe, expected_recipe);
        } else {
            // Hosted Windows Server CI fails only at platform()'s workstation check,
            // strictly after recipe.recheck(), route, topology, scope and bundles pass.
            assert_eq!(result.err(), Some(NativeRefusal::UnsupportedPlatform));
        }
    }
}

impl Drop for MeasureEnv {
    fn drop(&mut self) {
        for fixture in [&self.stock_fixture, &self.patched_fixture] {
            for path in [fixture.root.path(), &fixture.root.path().join("share")] {
                inventory::tests::set_dacl(path, inventory::tests::WRITABLE);
            }
        }
    }
}

#[test]
fn held_docker_inputs_measure_rechecks_boot_files_and_receipt() {
    #[derive(Clone, Copy, Debug)]
    enum BootFile {
        Kernel,
        Initrd,
        Receipt,
    }

    fn pin_mut(held: &mut ComposeInstallerInputs, target: BootFile) -> &mut PathPin {
        match target {
            BootFile::Kernel => &mut held.kernel,
            BootFile::Initrd => &mut held.initrd,
            BootFile::Receipt => &mut held.qemu_receipt,
        }
    }

    with_fixture(|f| {
        let env = MeasureEnv::new();
        for target in [BootFile::Kernel, BootFile::Initrd, BootFile::Receipt] {
            let disk_path = match target {
                BootFile::Kernel => f.paths.kernel,
                BootFile::Initrd => f.paths.initrd,
                BootFile::Receipt => f.paths.qemu_receipt,
            };

            // Positive control through HeldDockerInputs::measure.
            let held = f.hold().unwrap();
            let expected_recipe = held.recipe;
            let inputs = env.inputs(&held);
            env.assert_measure_passes(&inputs, expected_recipe);

            // Changed-after-hold custody: writers, renames, deletes and hard links are refused while held.
            let raw_path = std::path::Path::new(disk_path.as_str());
            let original_bytes = fs::read(raw_path).unwrap();
            let alias = raw_path.with_extension("alias");
            for refusal in [
                OpenOptions::new().write(true).open(raw_path).unwrap_err(),
                fs::rename(raw_path, raw_path.with_extension("moved")).unwrap_err(),
                fs::remove_file(raw_path).unwrap_err(),
                fs::hard_link(raw_path, &alias).unwrap_err(),
            ] {
                assert_eq!(refusal.raw_os_error(), Some(32));
            }
            assert!(!alias.exists());
            env.assert_measure_passes(&inputs, expected_recipe);
            drop(inputs);

            // Missing proof (verified_content cleared) must refuse in measure.
            {
                let mut held_missing_proof = f.hold().unwrap();
                pin_mut(&mut held_missing_proof, target).verified_content = None;
                let inputs = env.inputs(&held_missing_proof);
                assert_eq!(
                    inputs.measure(&env.route, env.live_topology()).err(),
                    Some(NativeRefusal::QualificationMissing),
                    "measure accepted missing {target:?} proof"
                );
            }

            // Missing file at recheck path must refuse in measure.
            {
                let mut held_missing_path = f.hold().unwrap();
                let missing = local(raw_path.with_extension("missing").to_str().unwrap()).unwrap();
                pin_mut(&mut held_missing_path, target).path = missing;
                let inputs = env.inputs(&held_missing_path);
                assert_eq!(
                    inputs.measure(&env.route, env.live_topology()).err(),
                    Some(NativeRefusal::QualificationMissing),
                    "measure accepted missing {target:?} file"
                );
            }

            // Same-length changed file at recheck must refuse in measure.
            {
                let mut changed = original_bytes.clone();
                changed[0] ^= 1;
                let impostor = raw_path.with_extension("same-len");
                fs::write(&impostor, &changed).unwrap();
                let mut held_same_len = f.hold().unwrap();
                pin_mut(&mut held_same_len, target).path =
                    local(impostor.to_str().unwrap()).unwrap();
                let inputs = env.inputs(&held_same_len);
                assert_eq!(
                    inputs.measure(&env.route, env.live_topology()).err(),
                    Some(NativeRefusal::QualificationMissing),
                    "measure accepted same-length substituted {target:?}"
                );
                drop(inputs);
                drop(held_same_len);
                fs::remove_file(&impostor).unwrap();
            }

            // Changed length mismatch on proof must refuse in measure.
            {
                let mut held_wrong_len = f.hold().unwrap();
                pin_mut(&mut held_wrong_len, target)
                    .verified_content
                    .as_mut()
                    .unwrap()
                    .size += 1;
                let inputs = env.inputs(&held_wrong_len);
                assert_eq!(
                    inputs.measure(&env.route, env.live_topology()).err(),
                    Some(NativeRefusal::QualificationMissing),
                    "measure accepted length-mismatched {target:?}"
                );
            }

            // Drop hold, then verify missing and same-length changed files cannot re-enter measure.
            drop(held);
            fs::remove_file(raw_path).unwrap();
            assert!(f.hold().is_err(), "re-held missing {target:?}");
            let mut changed = original_bytes.clone();
            changed[0] ^= 1;
            fs::write(raw_path, &changed).unwrap();
            assert!(f.hold().is_err(), "re-held same-length changed {target:?}");
            fs::write(raw_path, &original_bytes).unwrap();
            let reheld = f.hold().unwrap();
            let inputs = env.inputs(&reheld);
            env.assert_measure_passes(&inputs, expected_recipe);
        }
    });
}

/// Standards P2 (PR #402 review 1): the `--full-docker` preflight and the
/// foreground selection both bind held inputs to the selected M4 seal before
/// any live measurement, and a refusal claims nothing in the output directory.
#[test]
fn docker_inputs_bind_the_selected_m4_seal_before_live_measurement() {
    with_fixture(|f| {
        let env = MeasureEnv::new();
        let evidence = env.dir.path().join("evidence");
        fs::create_dir(&evidence).unwrap();
        let held = f.hold().unwrap();
        let mut inputs = env.inputs(&held);
        let selected = inputs.m4_seal;
        let mut substituted = selected;
        substituted[0] ^= 1;
        // The seal binding precedes even the target profile parse.
        assert_eq!(
            inputs.bound_targets(substituted).err(),
            Some(NativeRefusal::QualificationMissing)
        );
        assert_eq!(
            inputs.bound_targets(selected).err(),
            Some(NativeRefusal::NetworkFenceUnverified)
        );
        inputs.fixture[1] = env.hold_named(
            "fixture-targets.json",
            br#"{"public_tcp_target":"8.8.8.8:443","public_dns_target":"9.9.9.9:53"}"#,
        );
        assert_eq!(
            inputs.bound_targets(selected).unwrap().addresses(),
            [
                std::net::Ipv4Addr::new(8, 8, 8, 8),
                std::net::Ipv4Addr::new(9, 9, 9, 9)
            ]
        );
        for seal in [substituted, [0; 32], [0x64; 32]] {
            assert_eq!(
                inputs.bound_targets(seal).err(),
                Some(NativeRefusal::QualificationMissing)
            );
        }
        assert_eq!(fs::read_dir(&evidence).unwrap().count(), 0);
    });
}

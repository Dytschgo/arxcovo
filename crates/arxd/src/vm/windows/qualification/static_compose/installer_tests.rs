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

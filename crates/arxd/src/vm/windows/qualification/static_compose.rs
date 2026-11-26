//! Read-only Docker qualification inputs for the disposable collector and
//! future foreground selection.
#![allow(
    dead_code,
    reason = "consumed by the Part B disposable collector and foreground adapter"
)]

use super::*;
use crate::vm::{
    Id,
    paths::Artifact,
    public_egress::{MAX_POLICY_BYTES, PublicEgressPolicy, RouteObservation},
    qualification_record::{
        DockerDependencies, DockerFresh, DockerRecipe, DockerSelection, Hash, HostDependency,
        VerifiedDockerEvidence, digest,
    },
    template::{
        UbuntuRecipe,
        ubuntu_compose::{Closure, MAX_MEDIUM, OfflineVerifier, UbuntuComposeRecipe},
        ubuntu_web::{IsoPoolManifest, IsoPoolVerifier, PROFILE_PIN, UbuntuWebRecipe},
    },
};

const REFUSAL: NativeRefusal = NativeRefusal::QualificationMissing;
const MAX_METADATA: u64 = 64 * 1024 * 1024;
const MAX_REVIEW: u64 = 16 * 1024;
const MAX_ISO: u64 = 8 * GIB;
const MAX_GUEST: u64 = 64 * 1024 * 1024;
const MAX_FIRMWARE: u64 = 16 * 1024 * 1024;
const MAX_KEYRING: u64 = 1024 * 1024;

pub(super) struct StaticDossier {
    _root: foreground::ReadDossier,
    _rows: Vec<File>,
    pub(super) evidence: VerifiedDockerEvidence,
}

impl StaticDossier {
    pub(super) fn open(
        path: &LocalPath,
        owner: PSID,
        selection: &DockerSelection,
        fresh: &DockerFresh<'_>,
    ) -> Result<Self, NativeRefusal> {
        let root = foreground::ReadDossier::open(PathPin::open(path, true, false)?, owner)?;
        let (evidence, rows) = root
            .read_selected_docker(selection, fresh)
            .map_err(|_| REFUSAL)?;
        Ok(Self {
            _root: root,
            _rows: rows,
            evidence,
        })
    }
}

pub(super) struct ComposeInstallerInputs {
    medium: PathPin,
    installer: PathPin,
    guest: PathPin,
    firmware: [PathPin; 2],
    review: HeldReview,
    recipe: DockerRecipe,
    pub(super) seed: Vec<u8>,
    pub(super) detached_health: String,
}

/// The receipt pin is supplied independently by owner enrollment after the
/// offline review of signatures, indices, base versions and ISO inventory.
/// The fields bind that review to every input the adapter consumes.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedInputReview {
    contract: u16,
    web_pool: Hash,
    compose_closure: Hash,
    ubuntu_keyring_sha256: Hash,
    ubuntu_keyring_size: u64,
}

#[derive(Clone, Copy)]
pub(super) struct EnrolledFile<'a> {
    pub(super) path: &'a LocalPath,
    pub(super) pin: ArtifactPin,
}

pub(super) struct SignedInputPaths<'a> {
    pub(super) receipt: EnrolledFile<'a>,
    pub(super) docker_keyring: &'a LocalPath,
    pub(super) ubuntu_keyring: &'a LocalPath,
    pub(super) docker_metadata: &'a LocalPath,
    pub(super) ubuntu_metadata: &'a LocalPath,
    pub(super) web_metadata: &'a LocalPath,
    pub(super) base_versions: &'a LocalPath,
}

struct HeldReview {
    receipt: SignedInputReview,
    receipt_file: PathPin,
    docker_keyring: PathPin,
    ubuntu_keyring: PathPin,
    docker_metadata: PathPin,
    ubuntu_metadata: PathPin,
    web_metadata: PathPin,
    base_versions: PathPin,
}

impl HeldReview {
    fn hold(
        paths: &SignedInputPaths<'_>,
        pool: &IsoPoolManifest<'_>,
        closure: &Closure<'_>,
    ) -> Result<Self, NativeRefusal> {
        let file = hold_file(paths.receipt.path, paths.receipt.pin, MAX_REVIEW)?;
        let bytes = held_bytes(&file, MAX_REVIEW)?;
        let receipt: SignedInputReview = serde_json::from_slice(&bytes).map_err(|_| REFUSAL)?;
        if receipt.contract != 1
            || receipt.web_pool != pool.canonical_pin().map_err(|_| REFUSAL)?.sha256
            || serde_json::to_vec(&receipt).map_err(|_| REFUSAL)? != bytes
        {
            return Err(REFUSAL);
        }
        let ubuntu_key = ArtifactPin {
            sha256: receipt.ubuntu_keyring_sha256,
            size: receipt.ubuntu_keyring_size,
        };
        Ok(Self {
            receipt,
            receipt_file: file,
            docker_keyring: hold_file(
                paths.docker_keyring,
                closure.docker_signing_key,
                MAX_KEYRING,
            )?,
            ubuntu_keyring: hold_file(paths.ubuntu_keyring, ubuntu_key, MAX_KEYRING)?,
            docker_metadata: hold_file(
                paths.docker_metadata,
                closure.signed_docker_metadata,
                MAX_METADATA,
            )?,
            ubuntu_metadata: hold_file(
                paths.ubuntu_metadata,
                closure.signed_ubuntu_metadata,
                MAX_METADATA,
            )?,
            web_metadata: hold_file(paths.web_metadata, pool.signed_metadata, MAX_METADATA)?,
            base_versions: hold_file(paths.base_versions, closure.base_versions, MAX_METADATA)?,
        })
    }

    fn files(&self) -> [&PathPin; 7] {
        [
            &self.receipt_file,
            &self.docker_keyring,
            &self.ubuntu_keyring,
            &self.docker_metadata,
            &self.ubuntu_metadata,
            &self.web_metadata,
            &self.base_versions,
        ]
    }
}

impl IsoPoolVerifier for HeldReview {
    fn verify(&self, _base: &UbuntuRecipe, pool: &IsoPoolManifest<'_>) -> bool {
        self.web_metadata.verified_content == Some(pool.signed_metadata)
            && pool
                .canonical_pin()
                .is_ok_and(|pin| pin.sha256 == self.receipt.web_pool)
    }
}

impl OfflineVerifier for HeldReview {
    fn verify(&self, web_digest: Hash, closure: &Closure<'_>) -> bool {
        self.docker_keyring.verified_content == Some(closure.docker_signing_key)
            && self.docker_metadata.verified_content == Some(closure.signed_docker_metadata)
            && self.ubuntu_metadata.verified_content == Some(closure.signed_ubuntu_metadata)
            && self.base_versions.verified_content == Some(closure.base_versions)
            && closure
                .receipt(web_digest)
                .is_ok_and(|pin| pin.sha256 == self.receipt.compose_closure)
    }
}

pub(super) struct ComposeAssetPaths<'a> {
    pub(super) medium: &'a LocalPath,
    pub(super) installer: &'a LocalPath,
    pub(super) guest: &'a LocalPath,
    pub(super) firmware_code: &'a LocalPath,
    pub(super) firmware_vars: &'a LocalPath,
}

impl ComposeInstallerInputs {
    /// Measure the recipe before opening a selected dossier. Retain all
    /// custody through use of the projected measurement.
    pub(super) fn hold(
        base: &UbuntuRecipe,
        pool: &IsoPoolManifest<'_>,
        closure: &Closure<'_>,
        signed: &SignedInputPaths<'_>,
        paths: &ComposeAssetPaths<'_>,
        generation: Id,
    ) -> Result<Self, NativeRefusal> {
        if generation == [0; 16] {
            return Err(REFUSAL);
        }
        let review = HeldReview::hold(signed, pool, closure)?;
        let web = UbuntuWebRecipe::qualify(
            base,
            Some(PROFILE_PIN),
            pool,
            Some(pool.canonical_pin().map_err(|_| REFUSAL)?),
            &review,
        )
        .map_err(|_| REFUSAL)?;
        let web_digest = web.recipe_digest();
        let receipt = closure.receipt(web_digest).map_err(|_| REFUSAL)?;
        let recipe =
            UbuntuComposeRecipe::qualify(&web, closure, receipt, &review).map_err(|_| REFUSAL)?;
        let asset = |pin: Option<crate::vm::template::AssetPin>| {
            pin.map(|pin| ArtifactPin {
                size: pin.length,
                sha256: pin.sha256,
            })
            .ok_or(REFUSAL)
        };
        let installer = hold_file(paths.installer, asset(base.iso)?, MAX_ISO)?;
        let guest = hold_file(paths.guest, asset(base.guest)?, MAX_GUEST)?;
        let firmware = [
            hold_file(
                paths.firmware_code,
                asset(base.firmware_code)?,
                MAX_FIRMWARE,
            )?,
            hold_file(
                paths.firmware_vars,
                asset(base.firmware_vars)?,
                MAX_FIRMWARE,
            )?,
        ];
        let medium = Self::hold_medium(paths.medium, closure.medium)?;
        let seed = recipe
            .render_seed(generation, &held_bytes(&guest, MAX_GUEST)?)
            .map_err(|_| REFUSAL)?;
        Ok(Self {
            medium,
            installer,
            guest,
            firmware,
            review,
            recipe: DockerRecipe {
                identity: DockerRecipe::identity(recipe.digest()),
                web: web_digest,
                closure: receipt.sha256,
                medium: closure.medium.sha256,
            },
            seed,
            detached_health: recipe.detached_health(),
        })
    }

    pub(super) fn require_selected(
        &self,
        selected: &VerifiedDockerEvidence,
    ) -> Result<(), NativeRefusal> {
        self.recheck()?;
        (self.recipe == selected.recipe())
            .then_some(())
            .ok_or(REFUSAL)
    }

    fn recheck(&self) -> Result<(), NativeRefusal> {
        for pin in [&self.medium, &self.installer, &self.guest]
            .into_iter()
            .chain(&self.firmware)
            .chain(self.review.files())
        {
            recheck_file(pin)?;
        }
        Ok(())
    }

    fn hold_medium(path: &LocalPath, expected: ArtifactPin) -> Result<PathPin, NativeRefusal> {
        hold_file(path, expected, MAX_MEDIUM)
    }

    pub(super) fn dependency_artifact(&self) -> Result<Artifact, NativeRefusal> {
        inventory::binding(&self.medium)?;
        launch::artifact(ArtifactRole::ComposeDependencyMedium, &self.medium)
    }
}

fn hold_file(path: &LocalPath, expected: ArtifactPin, max: u64) -> Result<PathPin, NativeRefusal> {
    let mut pin = PathPin::open(path, false, false)?;
    pin.verify_content(expected, max)?;
    recheck_file(&pin)?;
    Ok(pin)
}

fn recheck_file(pin: &PathPin) -> Result<(), NativeRefusal> {
    inventory::binding(pin)?;
    let check = pin.reopen_without_writer().map_err(|_| REFUSAL)?;
    let expected = pin.verified_content.ok_or(REFUSAL)?;
    if check.metadata().map_err(|_| REFUSAL)?.len() != expected.size {
        return Err(REFUSAL);
    }
    Ok(())
}

fn held_bytes(pin: &PathPin, max: u64) -> Result<Vec<u8>, NativeRefusal> {
    let reader = pin.reopen_without_writer().map_err(|_| REFUSAL)?;
    let bytes = read_bounded(&reader, max).map_err(|_| REFUSAL)?;
    let expected = pin.verified_content.ok_or(REFUSAL)?;
    if bytes.len() as u64 != expected.size || digest(&bytes) != expected.sha256 {
        return Err(REFUSAL);
    }
    Ok(bytes)
}

/// Authenticated, owner-local enrollment assembled by the native caller.
/// A fixture, topology or trust profile is an input even after the trial
/// releases its child. Their raw content stays under these holds.
pub(super) struct DockerInputEnrollment<'a> {
    pub(super) release: &'a ReleaseEnrollment,
    pub(super) runtime_profile: EnrolledFile<'a>,
    pub(super) policy: &'a LocalPath,
    pub(super) stock_bundle: &'a LocalPath,
    pub(super) patched_bundle: &'a LocalPath,
    pub(super) fixture_image: EnrolledFile<'a>,
    pub(super) fixture_network_profile: EnrolledFile<'a>,
    pub(super) probe_image: EnrolledFile<'a>,
    pub(super) topology: EnrolledFile<'a>,
    pub(super) origin_profile: EnrolledFile<'a>,
}

pub(super) struct HeldDockerInputs<'a> {
    release: [PathPin; 2],
    scope: &'a test::Scope,
    token: &'a VmToken,
    recipe: &'a ComposeInstallerInputs,
    profile: PathPin,
    policy: PathPin,
    parsed_policy: PublicEgressPolicy,
    pub(super) patched: inventory::PatchedBundle,
    pub(super) stock: inventory::Bundle,
    stock_inventory: Hash,
    fixture: [PathPin; 3],
    topology: PathPin,
    origin_profile: PathPin,
}

impl<'a> HeldDockerInputs<'a> {
    pub(super) fn hold(
        enrollment: &DockerInputEnrollment<'_>,
        evidence: &LocalPath,
        mode: CollectMode<'_>,
        token: &'a VmToken,
        scope: &'a test::Scope,
        recipe: &'a ComposeInstallerInputs,
    ) -> Result<Self, NativeRefusal> {
        let image = std::env::current_exe().map_err(|_| REFUSAL)?;
        let image = local(image.to_str().ok_or(REFUSAL)?)?;
        let (_, release) =
            hold_release(enrollment.release, &image, evidence, mode.is_foreground())?;
        let private_input = |input: EnrolledFile<'_>, max| {
            let file = hold_file(input.path, input.pin, max)?;
            if !sink::held_private(&file.leaf, token.account.0)
                || [
                    evidence,
                    &scope.production.root.path,
                    &scope.recipe.root.path,
                ]
                .into_iter()
                .any(|root| root.overlaps(input.path))
            {
                return Err(REFUSAL);
            }
            Ok(file)
        };
        let profile = private_input(enrollment.runtime_profile, MAX_ENROLLMENT_BYTES)?;
        let config =
            crate::config::VmRuntimeProfile::parse(&held_bytes(&profile, MAX_ENROLLMENT_BYTES)?)
                .map_err(|_| REFUSAL)?;
        let policy = private_input(
            EnrolledFile {
                path: enrollment.policy,
                pin: config.public_egress_policy_pin().ok_or(REFUSAL)?,
            },
            MAX_POLICY_BYTES,
        )?;
        let parsed_policy = PublicEgressPolicy::parse(&held_bytes(&policy, MAX_POLICY_BYTES)?)?;
        let binary = config.public_egress_binary().ok_or(REFUSAL)?.qualify()?;
        if [enrollment.stock_bundle, enrollment.patched_bundle]
            .into_iter()
            .any(|bundle| {
                bundle.is_drive_root()
                    || [
                        evidence,
                        &scope.production.root.path,
                        &scope.recipe.root.path,
                    ]
                    .into_iter()
                    .any(|root| root.overlaps(bundle))
            })
            || enrollment.stock_bundle.overlaps(enrollment.patched_bundle)
        {
            return Err(REFUSAL);
        }
        let patched =
            inventory::PatchedBundle::hold(enrollment.patched_bundle, binary, token.account.0)?;
        let (stock_distribution, stock) = inventory::collect(enrollment.stock_bundle)?;
        stock.protected(token.account.0, Instant::now() + Duration::from_secs(2))?;
        let stock_inventory = projection(
            "arx-docker-stock-inventory-v1",
            (stock_distribution.manifest, stock_distribution.receipt),
        )?;
        Ok(Self {
            release,
            scope,
            token,
            recipe,
            profile,
            policy,
            parsed_policy,
            patched,
            stock,
            stock_inventory,
            fixture: [
                private_input(enrollment.fixture_image, MAX_ISO)?,
                private_input(enrollment.fixture_network_profile, MAX_ENROLLMENT_BYTES)?,
                private_input(enrollment.probe_image, MAX_MEDIUM)?,
            ],
            topology: private_input(enrollment.topology, MAX_METADATA)?,
            origin_profile: private_input(enrollment.origin_profile, MAX_ENROLLMENT_BYTES)?,
        })
    }

    /// Recheck independent inputs before selecting or writing evidence. The
    /// caller supplies the current route and full topology measurement.
    pub(super) fn measure(
        &self,
        route: &RouteObservation,
        live_topology: Hash,
    ) -> Result<DockerDependencies, NativeRefusal> {
        self.parsed_policy.verify_route(Some(route))?;
        let topology = measured(&self.topology)?;
        if live_topology != topology {
            return Err(NativeRefusal::NetworkFenceUnverified);
        }
        self.scope.validate_live()?;
        self.recipe.recheck()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        self.stock.protected(self.token.account.0, deadline)?;
        self.patched
            .bundle
            .protected(self.token.account.0, deadline)?;
        let review = self.patched.review;
        let [daemon, qualifier] = &self.release;
        let [image, network, probe] = &self.fixture;
        let [code, vars] = &self.recipe.firmware;
        Ok(DockerDependencies {
            supervisor_image: measured(daemon)?,
            qualifier_image: measured(qualifier)?,
            host: HostDependency {
                windows_build: platform()?,
                windows_revision: windows_revision()?,
                machine: machine_guid()?,
            },
            owner_logon: self.token.profile_key().map_err(|_| REFUSAL)?,
            production_store: projection("arx-docker-production-store-v1", self.scope.key)?,
            disposable_store: projection("arx-docker-disposable-store-v1", self.scope.key)?,
            profile: measured(&self.profile)?,
            installer: measured(&self.recipe.installer)?,
            guest: measured(&self.recipe.guest)?,
            firmware: projection(
                "arx-docker-recipe-firmware-v1",
                (measured(code)?, measured(vars)?),
            )?,
            stock_inventory: self.stock_inventory,
            network_fixture: projection(
                "arx-docker-fixture-v1",
                (measured(image)?, measured(network)?, measured(probe)?),
            )?,
            signed_inputs: measured(&self.recipe.review.receipt_file)?,
            recipe: self.recipe.recipe,
            compose_protocol: arx_guest::compose::COMPOSE_PROTOCOL_VERSION,
            origin_profile: measured(&self.origin_profile)?,
            manifest: review.manifest.sha256,
            executable: review.qemu.sha256,
            receipt: review.receipt.sha256,
            loaded_artifacts: review.dependency_inventory(),
            policy: measured(&self.policy)?,
            topology,
        })
    }
}

fn measured(pin: &PathPin) -> Result<Hash, NativeRefusal> {
    recheck_file(pin)?;
    Ok(pin.verified_content.ok_or(REFUSAL)?.sha256)
}

fn projection(domain: &str, value: impl Serialize) -> Result<Hash, NativeRefusal> {
    serde_json::to_vec(&(domain, value))
        .map(|bytes| digest(&bytes))
        .map_err(|_| REFUSAL)
}

#[cfg(test)]
#[test]
fn static_compose_medium_holds_exact_bytes_and_refuses_substitution() {
    use std::fs::OpenOptions;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("compose.iso");
    let bytes = b"synthetic dependency medium";
    std::fs::write(&path, bytes).unwrap();
    let local = LocalPath::parse(path.to_str().unwrap()).unwrap();
    let expected = ArtifactPin {
        sha256: crate::vm::qualification_record::digest(bytes),
        size: bytes.len() as u64,
    };
    let pin = ComposeInstallerInputs::hold_medium(&local, expected).unwrap();
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    drop(pin);
    let mut changed = *bytes;
    changed[0] ^= 1;
    std::fs::write(&path, changed).unwrap();
    assert!(ComposeInstallerInputs::hold_medium(&local, expected).is_err());
}

#[cfg(test)]
#[test]
fn signed_input_review_binds_held_keyrings_indexes_base_and_package_inventory() {
    use crate::vm::template::{AssetPin, ubuntu_compose::PackagePin, ubuntu_web::PoolPackage};
    use std::fs::{self, OpenOptions};

    let temp = tempfile::tempdir().unwrap();
    let path = |name: &str| local(temp.path().join(name).to_str().unwrap()).unwrap();
    let artifact = |bytes: &[u8]| ArtifactPin {
        size: bytes.len() as u64,
        sha256: digest(bytes),
    };
    let input = |name: &str, bytes: &[u8]| {
        let path = path(name);
        fs::write(path.as_str(), bytes).unwrap();
        (path, artifact(bytes))
    };
    let (docker_keyring, key) = input("docker-keyring", b"docker signing key");
    let (ubuntu_keyring, ubuntu_key) = input("ubuntu-keyring", b"ubuntu signing key");
    let (docker_metadata, docker_index) = input("docker-metadata", b"signed docker metadata");
    let (ubuntu_metadata, ubuntu_index) = input("ubuntu-metadata", b"signed ubuntu metadata");
    let (web_metadata, web_index) = input("web-metadata", b"signed web pool metadata");
    let (base_versions, versions) = input("base-versions", b"dpkg-query inventory");
    let pool = IsoPoolManifest {
        iso: AssetPin {
            length: 1,
            sha256: UbuntuRecipe::ISO_SHA256,
        },
        signed_metadata: web_index,
        packages: &[PoolPackage {
            name: "nginx",
            version: "1.0",
            relative_path: "pool/nginx.deb",
            artifact: artifact(b"nginx"),
        }],
    };
    let packages = [
        "containerd.io",
        "docker-ce",
        "docker-ce-cli",
        "docker-compose-plugin",
    ]
    .map(|name| PackagePin {
        name,
        version: "5:1.2.3-1~noble",
        archive: artifact(b"deb"),
    });
    let closure = Closure {
        docker_fingerprint: [11; 20],
        docker_signing_key: key,
        signed_docker_metadata: docker_index,
        signed_ubuntu_metadata: ubuntu_index,
        base_versions: versions,
        medium: artifact(b"a synthetic, reviewed dependency ISO"),
        packages: &packages,
    };
    let web = [8; 32];
    let reviewed = SignedInputReview {
        contract: 1,
        web_pool: pool.canonical_pin().unwrap().sha256,
        compose_closure: closure.receipt(web).unwrap().sha256,
        ubuntu_keyring_sha256: ubuntu_key.sha256,
        ubuntu_keyring_size: ubuntu_key.size,
    };
    let bytes = serde_json::to_vec(&reviewed).unwrap();
    let (receipt, pin) = input("review", &bytes);
    let paths = SignedInputPaths {
        receipt: EnrolledFile {
            path: &receipt,
            pin,
        },
        docker_keyring: &docker_keyring,
        ubuntu_keyring: &ubuntu_keyring,
        docker_metadata: &docker_metadata,
        ubuntu_metadata: &ubuntu_metadata,
        web_metadata: &web_metadata,
        base_versions: &base_versions,
    };
    let held = HeldReview::hold(&paths, &pool, &closure).unwrap();
    assert!(OfflineVerifier::verify(&held, web, &closure));
    assert!(!OfflineVerifier::verify(&held, [9; 32], &closure));
    for file in [
        &receipt,
        &docker_keyring,
        &ubuntu_keyring,
        &docker_metadata,
        &ubuntu_metadata,
        &web_metadata,
        &base_versions,
    ] {
        assert!(OpenOptions::new().write(true).open(file.as_str()).is_err());
    }
    let wrong_medium = Closure {
        medium: artifact(b"an unreviewed medium inventory"),
        ..closure
    };
    assert!(!OfflineVerifier::verify(&held, web, &wrong_medium));
    let mut modified_packages = packages;
    modified_packages[1].archive = artifact(b"different archive");
    let wrong_package = Closure {
        packages: &modified_packages,
        ..closure
    };
    assert!(!OfflineVerifier::verify(&held, web, &wrong_package));
    drop(held);
    for file in [
        &receipt,
        &docker_keyring,
        &ubuntu_keyring,
        &docker_metadata,
        &ubuntu_metadata,
        &web_metadata,
        &base_versions,
    ] {
        let bytes = fs::read(file.as_str()).unwrap();
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        fs::write(file.as_str(), changed).unwrap();
        assert!(HeldReview::hold(&paths, &pool, &closure).is_err());
        fs::write(file.as_str(), bytes).unwrap();
    }
    let mut modified_versions = packages;
    modified_versions[0].version = "1.2.3-1~noble";
    let wrong_version = Closure {
        packages: &modified_versions,
        ..closure
    };
    let held = HeldReview::hold(&paths, &pool, &wrong_version).unwrap();
    assert!(!OfflineVerifier::verify(&held, web, &wrong_version));
}

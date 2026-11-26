//! Held read-only inputs for the static Compose adapter. Checked creation
//! and live observation are deliberately absent until 7c is available.
#![allow(
    dead_code,
    reason = "consumed by the Part B static foreground composition"
)]

use super::*;
use crate::vm::{
    Id,
    paths::Artifact,
    qualification_record::{DockerFresh, DockerRecipe, DockerSelection, VerifiedDockerEvidence},
    template::{
        ubuntu_compose::{Closure, MAX_MEDIUM, OfflineVerifier, UbuntuComposeRecipe},
        ubuntu_web::UbuntuWebRecipe,
    },
};

const REFUSAL: NativeRefusal = NativeRefusal::QualificationMissing;

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
    pub(super) seed: Vec<u8>,
    pub(super) detached_health: String,
}

impl ComposeInstallerInputs {
    /// The caller supplies an independent verifier and the already qualified
    /// web recipe. This adapter owns the immutable medium read and keeps its
    /// handle from input preparation through plan construction.
    pub(super) fn hold(
        selected: &VerifiedDockerEvidence,
        web: &UbuntuWebRecipe<'_>,
        closure: &Closure<'_>,
        verifier: &impl OfflineVerifier,
        medium: &LocalPath,
        generation: Id,
        guest: &[u8],
    ) -> Result<Self, NativeRefusal> {
        let web_digest = web.recipe_digest();
        let receipt = closure.receipt(web_digest).map_err(|_| REFUSAL)?;
        let recipe =
            UbuntuComposeRecipe::qualify(web, closure, receipt, verifier).map_err(|_| REFUSAL)?;
        if selected.recipe()
            != (DockerRecipe {
                identity: DockerRecipe::identity(recipe.digest()),
                web: web_digest,
                closure: receipt.sha256,
                medium: closure.medium.sha256,
            })
            || generation == [0; 16]
        {
            return Err(REFUSAL);
        }
        let medium = Self::hold_medium(medium, closure.medium)?;
        let seed = recipe.render_seed(generation, guest).map_err(|_| REFUSAL)?;
        Ok(Self {
            medium,
            seed,
            detached_health: recipe.detached_health(),
        })
    }

    fn hold_medium(path: &LocalPath, expected: ArtifactPin) -> Result<PathPin, NativeRefusal> {
        let mut pin = PathPin::open(path, false, false)?;
        pin.verify_content(expected, MAX_MEDIUM)?;
        inventory::binding(&pin)?;
        Ok(pin)
    }

    pub(super) fn dependency_artifact(&self) -> Result<Artifact, NativeRefusal> {
        inventory::binding(&self.medium)?;
        launch::artifact(ArtifactRole::ComposeDependencyMedium, &self.medium)
    }
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
fn installer_hold_refuses_a_mismatched_selected_recipe() {
    use crate::vm::template::{
        AssetPin, UbuntuRecipe,
        ubuntu_compose::{Closure, OfflineVerifier, PackagePin},
        ubuntu_web::{IsoPoolManifest, IsoPoolVerifier, PROFILE_PIN, PoolPackage},
    };
    use sha2::{Digest, Sha256};

    struct Accept;

    impl IsoPoolVerifier for Accept {
        fn verify(&self, _: &UbuntuRecipe, _: &IsoPoolManifest<'_>) -> bool {
            true
        }
    }

    impl OfflineVerifier for Accept {
        fn verify(&self, _: [u8; 32], _: &Closure<'_>) -> bool {
            true
        }
    }

    fn asset(bytes: &[u8]) -> AssetPin {
        AssetPin {
            length: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        }
    }

    fn artifact(bytes: &[u8]) -> ArtifactPin {
        let pin = asset(bytes);
        ArtifactPin {
            size: pin.length,
            sha256: pin.sha256,
        }
    }

    let guest = b"fake-arx-guest";
    let iso = AssetPin {
        length: 1,
        sha256: UbuntuRecipe::ISO_SHA256,
    };
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
    let web = UbuntuWebRecipe::qualify(
        &base,
        Some(PROFILE_PIN),
        &pool,
        Some(pool.canonical_pin().unwrap()),
        &Accept,
    )
    .unwrap();
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
    let path = dir.path().join("compose.iso");
    std::fs::write(&path, medium_bytes).unwrap();
    let local = LocalPath::parse(path.to_str().unwrap()).unwrap();
    let web_digest = web.recipe_digest();
    let receipt = closure.receipt(web_digest).unwrap();
    let matched = DockerRecipe {
        identity: DockerRecipe::identity(receipt.sha256),
        web: web_digest,
        closure: receipt.sha256,
        medium: closure.medium.sha256,
    };
    let hold = |recipe: DockerRecipe| {
        ComposeInstallerInputs::hold(
            &VerifiedDockerEvidence::recipe_fixture(recipe),
            &web,
            &closure,
            &Accept,
            &local,
            [1; 16],
            guest,
        )
    };
    let held = hold(matched).unwrap();
    assert!(held.detached_health.contains("docker-compose-plugin"));
    drop(held);

    let mut wrong_medium = matched;
    wrong_medium.medium[0] ^= 0xff;
    let mut wrong_web = matched;
    wrong_web.web[0] ^= 0xff;
    let mut wrong_identity = matched;
    wrong_identity.identity = DockerRecipe::identity([9; 32]);
    for recipe in [wrong_medium, wrong_web, wrong_identity] {
        assert!(hold(recipe).is_err());
    }
}

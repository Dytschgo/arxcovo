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

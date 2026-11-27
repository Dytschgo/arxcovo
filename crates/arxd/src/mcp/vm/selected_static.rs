//! Static Compose selection is separate from TEST recipe and launch authority.

#[cfg(all(windows, target_arch = "x86_64"))]
use super::selected_test::SelectedTest;
use super::*;
use crate::vm::qualification_record::{Hash, RecipeIdentity, VerifiedDockerEvidence};

#[cfg(all(windows, target_arch = "x86_64"))]
mod native;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(super) use native::Run;

pub(super) struct SelectedStatic(Source);

enum Source {
    #[cfg(all(windows, target_arch = "x86_64"))]
    Native(Arc<crate::vm::qualification::StaticDossier>),
    #[cfg(test)]
    Fixture(Box<FixtureStaticQualification>),
}

impl SelectedStatic {
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(super) fn open(
        custody: &crate::config::VmCustody,
        m4: &SelectedTest,
    ) -> Result<Self, NativeRefusal> {
        let dossier = custody.select_static_dossier(Some(m4.native_source()))?;
        Ok(Self(Source::Native(Arc::new(dossier))))
    }

    pub(super) fn refusal(&self) -> Option<NativeRefusal> {
        match self.0 {
            #[cfg(all(windows, target_arch = "x86_64"))]
            Source::Native(ref dossier) => Some(
                dossier
                    .require_current()
                    .err()
                    .unwrap_or(NativeRefusal::NetworkFenceUnverified),
            ),
            #[cfg(test)]
            Source::Fixture(ref fixture) => (!fixture.network_live.load(Ordering::Acquire))
                .then_some(NativeRefusal::NetworkFenceUnverified),
        }
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(super) fn enroll_runtime(
        &self,
        store: &crate::store::vm::VmStore,
        backup_host: &mut compose_backup::BackupHost,
        profile: &mut TestVms,
        custody: &Arc<crate::config::VmCustody>,
    ) -> anyhow::Result<()> {
        #[cfg(not(test))]
        let Source::Native(dossier) = &self.0;
        #[cfg(test)]
        let Source::Native(dossier) = &self.0 else {
            unreachable!("native foreground selection required")
        };
        let current = || {
            dossier
                .require_current()
                .map_err(|reason| anyhow::anyhow!(native_reason(reason)))?;
            custody.verify()
        };
        // A retired selection still reports its refusal through the catalog.
        if dossier.require_current().is_err() {
            return Ok(());
        }
        let releases = std::mem::take(&mut profile.compose_releases)
            .into_iter()
            .map(compose::ReleaseInput::enroll)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let targets = profile.take_static_backup_targets();
        anyhow::ensure!(
            releases.iter().all(|release| release.secrets.is_empty()) || custody.has_compose_root(),
            "Compose secret custody required"
        );
        let backup = custody.open_static_backup()?;
        if backup.is_some() {
            custody.static_backup_capacity()?;
        }
        if !releases.is_empty() || !targets.is_empty() {
            crate::store::vm::compose::enroll_selected(
                store,
                &releases,
                &targets,
                self.recipe(),
                current,
            )?;
        }
        if let Some(backup) = backup {
            let custody = custody.clone();
            let dossier = dossier.clone();
            backup_host.install_native(store, Arc::new(backup), move || {
                dossier.require_current().ok()?;
                custody.static_backup_capacity().ok()
            })?;
        }
        Ok(())
    }

    fn evidence(&self) -> (&VerifiedDockerEvidence, Hash) {
        // In production on other platforms the source is uninhabited.
        match self.0 {
            #[cfg(all(windows, target_arch = "x86_64"))]
            Source::Native(ref dossier) => (&dossier.evidence, dossier.seal_sha256),
            #[cfg(test)]
            Source::Fixture(ref fixture) => (&fixture.evidence, fixture.seal_sha256),
        }
    }

    pub(super) fn recipe(&self) -> RecipeIdentity {
        self.evidence().0.recipe().identity
    }

    pub(super) fn catalog(&self) -> Value {
        let (evidence, digest) = self.evidence();
        let recipe = evidence.recipe().identity;
        json!({
            "sha256":hex::encode(digest),
            "compose_recipe":{
                "recipe_id":hex::encode(recipe.recipe_id),
                "recipe_sha256":hex::encode(recipe.recipe_sha256),
            },
            "live_available":true,
        })
    }

    #[cfg(test)]
    pub(super) fn retire_fixture_network(&self) {
        match &self.0 {
            #[cfg(all(windows, target_arch = "x86_64"))]
            Source::Native(_) => panic!("expected fixture selection"),
            Source::Fixture(fixture) => fixture.network_live.store(false, Ordering::Release),
        }
    }

    #[cfg(test)]
    pub(super) fn revive_fixture_network(&self) {
        match &self.0 {
            #[cfg(all(windows, target_arch = "x86_64"))]
            Source::Native(_) => panic!("expected fixture selection"),
            Source::Fixture(fixture) => fixture.network_live.store(true, Ordering::Release),
        }
    }

    #[cfg(test)]
    pub(super) fn fixture_evidence(&self) -> Option<&VerifiedDockerEvidence> {
        match &self.0 {
            #[cfg(all(windows, target_arch = "x86_64"))]
            Source::Native(_) => None,
            Source::Fixture(fixture) => Some(&fixture.evidence),
        }
    }
}

#[cfg(test)]
pub(super) struct FixtureStaticQualification {
    network_live: AtomicBool,
    seal_sha256: Hash,
    evidence: VerifiedDockerEvidence,
}

#[cfg(test)]
impl FixtureStaticQualification {
    pub(super) fn select() -> SelectedStatic {
        use crate::vm::qualification_record::DockerRecipe;

        let closure = [0x71; 32];
        SelectedStatic(Source::Fixture(Box::new(Self {
            network_live: AtomicBool::new(true),
            seal_sha256: [0x70; 32],
            evidence: VerifiedDockerEvidence::recipe_fixture(DockerRecipe {
                identity: DockerRecipe::identity(closure),
                web: [0x72; 32],
                closure,
                medium: [0x73; 32],
            }),
        })))
    }
}

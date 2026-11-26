//! Static Compose evidence. This reader has no launch or trial authority.

use super::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DockerRecipe {
    pub identity: RecipeIdentity,
    pub web: Hash,
    pub closure: Hash,
    pub medium: Hash,
}

impl DockerRecipe {
    pub fn identity(recipe_sha256: Hash) -> RecipeIdentity {
        let key = digest(
            &serde_json::to_vec(&(
                crate::vm::template::ubuntu_compose::RECIPE_ID,
                recipe_sha256,
            ))
            .expect("fixed recipe identity"),
        );
        RecipeIdentity {
            recipe_id: key[..16].try_into().expect("fixed recipe ID"),
            recipe_sha256,
        }
    }

    pub(super) fn valid(self) -> bool {
        recipe_id_valid(self.identity)
            && self.identity.recipe_sha256 == self.closure
            && [self.web, self.closure, self.medium]
                .iter()
                .all(|hash| *hash != [0; 32])
    }
}

pub(super) fn recipe_id_valid(identity: RecipeIdentity) -> bool {
    identity.recipe_sha256 != [0; 32]
        && identity == DockerRecipe::identity(identity.recipe_sha256)
        && identity.recipe_id != identity.recipe_sha256[..16]
        && identity.recipe_id != crate::vm::template::windows_recipe_id()
}

/// All hashes are computed from independently held and measured inputs. The
/// key is distinct from TEST even where individual measurements are shared.
#[derive(Clone, Serialize)]
pub struct DockerDependencies {
    pub supervisor_image: Hash,
    pub qualifier_image: Hash,
    pub host: HostDependency,
    pub owner_logon: Hash,
    pub production_store: Hash,
    pub disposable_store: Hash,
    pub profile: Hash,
    pub installer: Hash,
    pub guest: Hash,
    pub firmware: Hash,
    pub stock_inventory: Hash,
    pub network_fixture: Hash,
    pub signed_inputs: Hash,
    pub recipe: DockerRecipe,
    pub compose_protocol: u32,
    pub origin_profile: Hash,
    pub manifest: Hash,
    pub executable: Hash,
    pub receipt: Hash,
    pub loaded_artifacts: Hash,
    pub policy: Hash,
    pub topology: Hash,
}

impl DockerDependencies {
    pub fn key(&self, scope: Hash) -> Option<Hash> {
        use crate::vm::public_egress::REVIEWED_EGRESS_BUILDS;
        let complete = self.host.windows_build >= 22_000
            && self.host.machine != [0; 16]
            && self.recipe.valid()
            && self.production_store != self.disposable_store
            && self.compose_protocol == arx_guest::compose::COMPOSE_PROTOCOL_VERSION
            && REVIEWED_EGRESS_BUILDS.iter().any(|build| {
                build.manifest.sha256 == self.manifest
                    && build.qemu.sha256 == self.executable
                    && build.receipt.sha256 == self.receipt
                    && build.dependency_inventory() == self.loaded_artifacts
            })
            && [
                scope,
                self.supervisor_image,
                self.qualifier_image,
                self.owner_logon,
                self.production_store,
                self.disposable_store,
                self.profile,
                self.installer,
                self.guest,
                self.firmware,
                self.stock_inventory,
                self.network_fixture,
                self.signed_inputs,
                self.origin_profile,
                self.manifest,
                self.loaded_artifacts,
                self.policy,
                self.topology,
            ]
            .iter()
            .all(|hash| *hash != [0; 32]);
        complete.then(|| {
            digest(
                &serde_json::to_vec(&("arx-vm-docker-v1", release(), scope, self))
                    .expect("fixed Docker dependency projection"),
            )
        })
    }
}

#[derive(Clone, Copy)]
pub struct DockerSelection {
    pub seal_sha256: Hash,
    pub scope: Hash,
}

pub struct DockerFresh<'a> {
    pub now: Instant2,
    pub boot_marker: Hash,
    pub dependencies: &'a DockerDependencies,
}

/// An opaque verified read projection. Only an independent dispatcher can
/// bind it to a durable operation, live reserves, and a checked launch.
pub struct VerifiedDockerEvidence {
    seal: Seal,
    recipe: DockerRecipe,
}

impl VerifiedDockerEvidence {
    pub fn seal(&self) -> &Seal {
        &self.seal
    }

    pub fn recipe(&self) -> DockerRecipe {
        self.recipe
    }

    /// The seal is unread by adapters that only call [`Self::recipe`].
    #[cfg(test)]
    pub(crate) fn recipe_fixture(recipe: DockerRecipe) -> Self {
        Self {
            seal: Seal {
                contract: 0,
                collector: 0,
                origin: Origin::Fixture,
                binding: Binding {
                    class: Class::Docker,
                    scope: [1; 32],
                    release: [2; 32],
                    run: [3; 16],
                    dependencies: [4; 32],
                    boot: BootAnchor {
                        marker: [5; 32],
                        origin_mono_ms: 1,
                    },
                    issued_mono_ms: 1,
                    issued_utc: 1,
                    expires_utc: 2,
                },
                finished_utc: 1,
                manifest: Vec::new(),
            },
            recipe,
        }
    }
}

pub fn read_native_docker<R: Read>(
    seal_file: impl Read,
    seal_length: u64,
    mut open_row: impl FnMut(FileKind) -> Result<R, RunError>,
    selection: &DockerSelection,
    fresh: &DockerFresh<'_>,
) -> Result<VerifiedDockerEvidence, RunError> {
    let bytes = read_bounded(seal_file, seal_length)?;
    let dependencies = fresh.dependencies;
    let seal = selected_native_seal(
        &bytes,
        selection.seal_sha256,
        selection.scope,
        Class::Docker,
        fresh.now,
        fresh.boot_marker,
        dependencies.key(selection.scope),
    )?;
    let mut files = Vec::with_capacity(seal.manifest.len());
    for entry in &seal.manifest {
        files.push((
            entry.file,
            read_bounded(open_row(entry.file)?, entry.length)?,
        ));
    }
    let rows = bundle_rows(&seal, &files, Origin::Native)?;
    let finished = seal
        .binding
        .boot
        .origin_mono_ms
        .checked_add(rows.last_finished_ms)
        .ok_or(RunError::Untrusted)?;
    if fresh.now.mono_ms <= finished
        || rows.docker.host_build != Some(dependencies.host.windows_build)
        || rows.docker.distribution
            != Some([
                dependencies.manifest,
                dependencies.executable,
                dependencies.receipt,
                dependencies.loaded_artifacts,
            ])
        || rows.docker.recipe != Some(dependencies.recipe)
        || rows.docker.network != Some((dependencies.policy, dependencies.topology))
        || rows.docker.endpoint
            != Some((dependencies.recipe.identity, dependencies.compose_protocol))
    {
        return Err(RunError::Untrusted);
    }
    Ok(VerifiedDockerEvidence {
        seal,
        recipe: dependencies.recipe,
    })
}

#[derive(Default)]
pub(super) struct Rows {
    host_build: Option<u32>,
    distribution: Option<[Hash; 4]>,
    recipe: Option<DockerRecipe>,
    network: Option<(Hash, Hash)>,
    endpoint: Option<(RecipeIdentity, u32)>,
}

impl Rows {
    pub(super) fn observe(&mut self, class: Class, result: &StageResult) -> Result<(), RunError> {
        use Observation as O;
        match result {
            StageResult::Passed(O::Host { windows_build, .. }) if class == Class::Docker => {
                self.host_build = Some(*windows_build);
            }
            StageResult::Passed(O::DockerDistribution {
                manifest,
                executable,
                receipt,
                loaded_artifacts,
            }) if class == Class::Docker => {
                self.distribution = Some([*manifest, *executable, *receipt, *loaded_artifacts]);
            }
            StageResult::Passed(O::DockerRecipeVerified { recipe, .. })
                if class == Class::Docker =>
            {
                self.recipe = Some(*recipe);
            }
            StageResult::Passed(O::DockerPublicIpv4 {
                policy, topology, ..
            }) if class == Class::Docker => {
                self.network = Some((*policy, *topology));
            }
            StageResult::Passed(O::DockerInstalledEndpoint {
                recipe,
                compose_protocol,
                ..
            }) if class == Class::Docker => {
                self.endpoint = Some((*recipe, *compose_protocol));
            }
            StageResult::Passed(O::Distribution { .. }) if class != Class::Docker => {}
            StageResult::Passed(
                O::DockerDistribution { .. }
                | O::DockerRecipeVerified { .. }
                | O::DockerPublicIpv4 { .. }
                | O::DockerInstalledEndpoint { .. }
                | O::Distribution { .. },
            ) => {
                return Err(RunError::Untrusted);
            }
            _ => {}
        }
        Ok(())
    }
}

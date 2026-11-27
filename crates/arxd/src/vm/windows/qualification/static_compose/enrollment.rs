//! The private Docker enrollment supplies pins and paths, never measured evidence.
use super::*;
use crate::vm::template::{ubuntu_compose::PackagePin, ubuntu_web::PoolPackage};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Enrollment {
    pub(super) class: crate::vm::qualification_record::Class,
    pub(super) release: ReleaseEnrollment,
    pub(in crate::vm::windows::qualification) runtime_profile: Input,
    pub(super) policy: String,
    pub(super) stock_bundle: String,
    pub(super) patched_bundle: String,
    pub(super) fixture_image: Input,
    pub(super) fixture_network_profile: Input,
    pub(super) receiver_inventory: Input,
    pub(super) probe_image: Input,
    pub(super) topology: Input,
    pub(super) origin_profile: Input,
    // The independently reviewed comparison refers to the emitted seal.
    // Collection can precede it. Foreground selection always requires it.
    pub(super) network_witness: Option<Input>,
    pub(in crate::vm::windows::qualification) recipe: Recipe,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Input {
    #[serde(deserialize_with = "local_path")]
    path: LocalPath,
    size: u64,
    #[serde(deserialize_with = "sha256")]
    sha256: Hash,
}

impl Enrollment {
    pub(in crate::vm::windows::qualification) fn parse(
        bytes: &[u8],
    ) -> Result<Self, NativeRefusal> {
        let enrollment: Self = serde_json::from_slice(bytes).map_err(|_| REFUSAL)?;
        if enrollment.class != crate::vm::qualification_record::Class::Docker {
            return Err(REFUSAL);
        }
        Ok(enrollment)
    }

    pub(in crate::vm::windows::qualification) fn hold_inputs<'a>(
        &self,
        evidence: &LocalPath,
        mode: CollectMode<'_>,
        held: &'a Held,
        recipe: &'a ComposeInstallerInputs,
    ) -> Result<HeldDockerInputs<'a>, NativeRefusal> {
        HeldDockerInputs::hold(
            &DockerInputEnrollment {
                release: &self.release,
                runtime_profile: self.runtime_profile.file(),
                policy: &local(&self.policy)?,
                stock_bundle: &local(&self.stock_bundle)?,
                patched_bundle: &local(&self.patched_bundle)?,
                fixture_image: self.fixture_image.file(),
                fixture_network_profile: self.fixture_network_profile.file(),
                receiver_inventory: self.receiver_inventory.file(),
                probe_image: self.probe_image.file(),
                topology: self.topology.file(),
                origin_profile: self.origin_profile.file(),
            },
            evidence,
            mode,
            &held.token,
            held.scope.as_ref().ok_or(REFUSAL)?,
            recipe,
        )
    }
}

impl Input {
    pub(in crate::vm::windows::qualification) fn file(&self) -> EnrolledFile<'_> {
        EnrolledFile {
            path: &self.path,
            pin: self.pin(),
        }
    }

    fn pin(&self) -> ArtifactPin {
        ArtifactPin {
            size: self.size,
            sha256: self.sha256,
        }
    }
}

fn local_path<'de, D: serde::Deserializer<'de>>(de: D) -> Result<LocalPath, D::Error> {
    local(&String::deserialize(de)?).map_err(|_| serde::de::Error::custom("invalid local path"))
}

fn sha256<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Hash, D::Error> {
    let value = String::deserialize(de)?;
    let mut pin = [0; 32];
    if hex::decode_to_slice(&value, &mut pin).is_err()
        || hex::encode(pin) != value
        || pin == [0; 32]
    {
        return Err(serde::de::Error::custom("invalid SHA-256 pin"));
    }
    Ok(pin)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::vm::windows::qualification) struct Recipe {
    medium: Input,
    review: Input,
    docker_keyring: Input,
    ubuntu_keyring: Input,
    docker_metadata: Input,
    ubuntu_metadata: Input,
    web_metadata: Input,
    base_versions: Input,
    docker_fingerprint: [u8; 20],
    web_packages: Vec<WebPackage>,
    compose_packages: Vec<ComposePackage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebPackage {
    name: String,
    version: String,
    relative_path: String,
    size: u64,
    #[serde(deserialize_with = "sha256")]
    sha256: Hash,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComposePackage {
    name: String,
    version: String,
    size: u64,
    #[serde(deserialize_with = "sha256")]
    sha256: Hash,
}

impl Recipe {
    pub(in crate::vm::windows::qualification) fn hold(
        &self,
        held: &Held,
        measured: &launch::RecipeInputs,
        generation: Id,
    ) -> Result<ComposeInstallerInputs, NativeRefusal> {
        let [iso, kernel, initrd] = &held.network.as_ref().ok_or(REFUSAL)?.media;
        let guest = held.ubuntu_guest.as_ref().map_err(|_| REFUSAL)?;
        let base = launch::ubuntu_test_recipe([
            launch::content(iso)?,
            launch::content(kernel)?,
            launch::content(initrd)?,
            guest.expected,
            launch::content(&measured.code)?,
            launch::content(&measured.vars)?,
            launch::content(&measured.receipt)?,
        ]);
        let packages: Vec<_> = self
            .web_packages
            .iter()
            .map(|package| PoolPackage {
                name: &package.name,
                version: &package.version,
                relative_path: &package.relative_path,
                artifact: ArtifactPin {
                    size: package.size,
                    sha256: package.sha256,
                },
            })
            .collect();
        let pool = IsoPoolManifest {
            iso: base.iso.ok_or(REFUSAL)?,
            signed_metadata: self.web_metadata.pin(),
            packages: &packages,
        };
        let packages: Vec<_> = self
            .compose_packages
            .iter()
            .map(|package| PackagePin {
                name: &package.name,
                version: &package.version,
                archive: ArtifactPin {
                    size: package.size,
                    sha256: package.sha256,
                },
            })
            .collect();
        let closure = Closure {
            docker_fingerprint: self.docker_fingerprint,
            docker_signing_key: self.docker_keyring.pin(),
            signed_docker_metadata: self.docker_metadata.pin(),
            signed_ubuntu_metadata: self.ubuntu_metadata.pin(),
            base_versions: self.base_versions.pin(),
            medium: self.medium.pin(),
            packages: &packages,
        };
        ComposeInstallerInputs::hold(
            &base,
            &pool,
            &closure,
            &SignedInputPaths {
                receipt: self.review.file(),
                docker_keyring: &self.docker_keyring.path,
                ubuntu_keyring: &self.ubuntu_keyring.path,
                docker_metadata: &self.docker_metadata.path,
                ubuntu_metadata: &self.ubuntu_metadata.path,
                web_metadata: &self.web_metadata.path,
                base_versions: &self.base_versions.path,
            },
            &ComposeAssetPaths {
                medium: &self.medium.path,
                installer: &iso.path,
                kernel: &kernel.path,
                initrd: &initrd.path,
                qemu_receipt: &measured.receipt.path,
                guest: &guest.pin.path,
                firmware_code: &measured.code.path,
                firmware_vars: &measured.vars.path,
            },
            generation,
        )
    }
}

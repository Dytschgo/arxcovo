//! Daemon selection and custody for the independently measured TEST inputs.

use super::*;
use crate::store::vm::VmStore;
use crate::vm::qualification_record::{
    self as records, Fresh, RunError, Selection, VerifiedEvidence,
};
use crate::vm::selection_epoch::Invalidation::{self, Confirmed, Indeterminate};

pub(crate) struct ForegroundQualification {
    pub(super) held: Held,
    pub(super) recipes: std::sync::Mutex<launch::RecipeInputs>,
    pub(super) store: std::sync::Arc<VmStore>,
    dependencies: Dependencies,
    evidence: VerifiedEvidence,
    seal_sha256: records::Hash,
    _dossier: ReadDossier,
    _rows: Vec<File>,
}

impl ForegroundQualification {
    pub(crate) fn open(
        store: std::sync::Arc<VmStore>,
        enrollment: &str,
        dossier: &str,
        selection: &Selection,
    ) -> Result<Self, NativeRefusal> {
        let (observations, _, held, dossier) = collect_preflight(
            &capacity::MountManager,
            enrollment,
            dossier,
            false,
            true,
            CollectMode::Foreground(&store),
            |pin, owner, _, _| ReadDossier::open(pin, owner),
        )?;
        let mut dependencies = observations.dependencies();
        // This projection precedes all reads of the selected seal and rows.
        let recipes = launch::measure_recipes(&held, [0; 16], [0; 32])?;
        recipes.projection.bind(&mut dependencies);
        let fresh = measure_fresh(&held, CollectMode::Foreground(&store), &dependencies)?;
        let (evidence, rows) = dossier
            .read_selected(selection, &fresh)
            .map_err(|_| NativeRefusal::QualificationMissing)?;
        if recipes.projection.recipes != evidence.recipes {
            return Err(NativeRefusal::QualificationMissing);
        }
        Ok(Self {
            held,
            recipes: std::sync::Mutex::new(recipes),
            store,
            dependencies,
            evidence,
            seal_sha256: selection.seal_sha256,
            _dossier: dossier,
            _rows: rows,
        })
    }

    pub(super) fn owner(&self) -> PSID {
        self.held.token.account.0
    }

    pub(super) fn snapshot_firmware(&self) -> Result<crate::vm::paths::ArtifactPin, NativeRefusal> {
        let inputs = self
            .recipes
            .lock()
            .map_err(|_| NativeRefusal::QualificationMissing)?;
        launch::content(&inputs.code)
    }

    pub(crate) fn evidence(&self) -> &VerifiedEvidence {
        &self.evidence
    }

    pub(in crate::vm::windows::qualification) fn seal_sha256(&self) -> records::Hash {
        self.seal_sha256
    }

    /// A new boot, expiry, clock disagreement or dependency drift is
    /// confirmed. The selected seal was verified at open, so a refused
    /// issuance anchor means the boot marker changed.
    pub(crate) fn fresh(&self) -> Result<Fresh<'_>, Invalidation> {
        let fresh = measure_fresh(
            &self.held,
            CollectMode::Foreground(&self.store),
            &self.dependencies,
        )?;
        require_live(&self.evidence, &fresh)?;
        Ok(fresh)
    }

    /// One bounded epoch observation: the freshness and store-identity
    /// reads, then the high-water transaction, each keeping its class.
    pub(crate) fn epoch_sample(
        &self,
        advance: impl FnOnce() -> Result<crate::vm::ClockReading, Invalidation>,
    ) -> Result<crate::vm::selection_epoch::Sample, Invalidation> {
        let captured = std::time::Instant::now();
        let fresh = self.fresh()?;
        let reading = advance()?;
        Ok(crate::vm::selection_epoch::Sample {
            captured,
            now: fresh.now,
            boot_marker: fresh.boot_marker,
            reading,
        })
    }

    pub(in crate::vm::windows) fn production_root(&self) -> &PathPin {
        &self
            .held
            .scope
            .as_ref()
            .expect("foreground collector requires TEST scope")
            .production
            .root
    }
}

impl crate::vm::test_issuer::SelectedSource for ForegroundQualification {
    fn seal_sha256(&self) -> records::Hash {
        self.seal_sha256
    }

    fn class(&self) -> records::Class {
        self.evidence.seal.binding.class
    }

    fn scope(&self) -> records::Hash {
        self.evidence.seal.binding.scope
    }

    fn recipes(&self) -> records::TestRecipes {
        self.evidence.recipes
    }

    /// Open already bound the live token to the enrollment owner.
    fn owner_matches(&self, configured: &str) -> bool {
        sid_matches(&self.held.token.account, configured)
    }

    fn sample(
        &self,
        advance: &dyn Fn() -> Result<crate::vm::ClockReading, Invalidation>,
    ) -> Result<crate::vm::selection_epoch::Sample, Invalidation> {
        self.epoch_sample(advance)
    }
}

/// The configured SID, in its exact canonical spelling, names `held`.
fn sid_matches(held: &LocalSid, configured: &str) -> bool {
    use windows_sys::Win32::Security::EqualSid;
    // Both SIDs are valid allocations owned for this call.
    canonical_sid(configured).is_ok_and(|sid| unsafe { EqualSid(sid.0, held.0) } != 0)
}

/// Measured profile, custody, volume, asset, DNS-witness, host or profile-key
/// mismatches confirm drift. API or I/O failures, missing observations and an
/// inactive console session fail closed as indeterminate.
fn require_live(evidence: &VerifiedEvidence, fresh: &Fresh<'_>) -> Result<(), Invalidation> {
    let reason = NativeRefusal::QualificationMissing;
    let binding = evidence.seal.binding;
    let issued = binding
        .issued_on_boot(fresh.boot_marker)
        .ok_or(Confirmed(reason))?;
    if !binding.live_at(fresh.now.utc, fresh.dependencies.key(binding.scope))
        || !records::clocks_agree(issued, fresh.now)
    {
        return Err(Confirmed(reason));
    }
    Ok(())
}

fn measure_fresh<'a>(
    held: &Held,
    mode: CollectMode<'_>,
    dependencies: &'a Dependencies,
) -> Result<Fresh<'a>, Invalidation> {
    let reason = NativeRefusal::QualificationMissing;
    held.token
        .verify_current_profile()
        .map_err(TokenCheckError::into_invalidation)?;
    inventory::check_binding(&held._source)?;
    let scope = held.scope.as_ref().ok_or(Indeterminate(reason))?;
    match mode {
        CollectMode::Foreground(store) => scope.validate_foreground(store)?,
        CollectMode::QualifierReview => scope.validate_stopped()?,
        CollectMode::Qualifier => return Err(Indeterminate(reason)),
    }
    for volume in &held.volumes {
        capacity::check_still_bound(&capacity::MountManager, volume)
            .map_err(|error| error.into_invalidation(NativeRefusal::PathCustodyUnverified))?;
    }
    inventory::check_binding(&held.trial)?;
    inventory::check_binding(&held._backing)?;
    launch::revalidate_assets(held)?;
    let live = (
        platform(),
        windows_revision(),
        machine_guid(),
        held.token.profile_key(),
    );
    let (
        Measured::Measured(host),
        Measured::Measured(profile),
        (Ok(build), Ok(revision), Ok(machine), Ok(key)),
    ) = (&dependencies.host, &dependencies.owner_profile, live)
    else {
        return Err(Indeterminate(reason));
    };
    if host.windows_build != build
        || host.windows_revision != revision
        || host.machine != machine
        || &key != profile
    {
        return Err(Confirmed(reason));
    }
    let now = boot::now();
    Ok(Fresh {
        now,
        boot_marker: boot::marker().map_err(Indeterminate)?,
        dependencies,
    })
}

/// Read the TEST seal as a qualifier, with the daemon stopped and both store
/// files held without writer sharing. A Docker run never borrows TEST launch
/// authority and cannot claim the selected TEST evidence directory.
pub(super) struct QualifierTestInputs {
    pub(super) held: Held,
    pub(super) limits: Limits,
    pub(super) recipes: launch::RecipeInputs,
    dependencies: Dependencies,
    evidence: VerifiedEvidence,
    dossier: ReadDossier,
    _rows: Vec<File>,
}

impl QualifierTestInputs {
    pub(super) fn open(
        selected: &crate::config::TestQualificationSelection,
    ) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::QualificationMissing;
        let mode = CollectMode::QualifierReview;
        let (observations, _, held, dossier) = collect_preflight(
            &capacity::MountManager,
            selected.enrollment().as_str(),
            selected.dossier().as_str(),
            false,
            true,
            mode,
            |pin, owner, _, _| ReadDossier::open(pin, owner),
        )?;
        if !sid_matches(&held.token.account, selected.owner_sid()) {
            return Err(NativeRefusal::IdentityUnverified);
        }
        let recipes = launch::measure_recipes(&held, [0; 16], [0; 32])?;
        let mut dependencies = observations.dependencies();
        recipes.projection.bind(&mut dependencies);
        let fresh = measure_fresh(&held, mode, &dependencies)?;
        let (evidence, rows) = dossier
            .read_selected(selected.selection(), &fresh)
            .map_err(|_| reason)?;
        require_live(&evidence, &fresh)?;
        if recipes.projection.recipes != evidence.recipes {
            return Err(reason);
        }
        let limits = *observed(&observations.limits).ok_or(reason)?;
        Ok(Self {
            held,
            limits,
            recipes,
            dependencies,
            evidence,
            dossier,
            _rows: rows,
        })
    }

    pub(super) fn require_current(&self) -> Result<(), NativeRefusal> {
        self.dossier.verify()?;
        let fresh = measure_fresh(&self.held, CollectMode::QualifierReview, &self.dependencies)?;
        require_live(&self.evidence, &fresh)?;
        Ok(())
    }
}

pub(super) struct ReadDossier(PathPin);

impl ReadDossier {
    pub(super) fn open(pin: PathPin, owner: PSID) -> Result<Self, NativeRefusal> {
        if pin.mutable || !sink::held_private(&pin.leaf, owner) {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        Ok(Self(pin))
    }

    pub(super) fn verify(&self) -> Result<(), NativeRefusal> {
        inventory::binding(&self.0)
    }

    pub(super) fn path(&self) -> &LocalPath {
        &self.0.path
    }

    pub(super) fn seal(&self) -> Result<File, NativeRefusal> {
        self.verify()?;
        self.open_file("qualification.json")
            .map_err(|_| NativeRefusal::QualificationMissing)
    }

    fn open_file(&self, name: &str) -> Result<File, RunError> {
        let file = open_relative(&self.0.leaf, name, Node::File, false, false)
            .map_err(|_| RunError::Output)?;
        let id = file_identity(&file, false).map_err(|_| RunError::Output)?.0;
        if id.volume != self.0.identity.volume {
            return Err(RunError::Output);
        }
        Ok(file)
    }

    pub(super) fn read_selected(
        &self,
        selection: &Selection,
        fresh: &Fresh<'_>,
    ) -> Result<(VerifiedEvidence, Vec<File>), RunError> {
        self.read_with(|seal, length, open| {
            records::read_native(seal, length, open, selection, fresh)
        })
    }

    pub(super) fn read_selected_docker(
        &self,
        selection: &records::DockerSelection,
        fresh: &records::DockerFresh<'_>,
    ) -> Result<(records::VerifiedDockerEvidence, Vec<File>), RunError> {
        self.read_with(|seal, length, open| {
            records::read_native_docker(seal, length, open, selection, fresh)
        })
    }

    fn read_with<T>(
        &self,
        read: impl FnOnce(
            &File,
            u64,
            &mut dyn FnMut(records::FileKind) -> Result<File, RunError>,
        ) -> Result<T, RunError>,
    ) -> Result<(T, Vec<File>), RunError> {
        inventory::binding(&self.0).map_err(|_| RunError::Output)?;
        let seal = self.open_file("qualification.json")?;
        let length = seal.metadata().map_err(|_| RunError::Output)?.len();
        let mut rows = Vec::with_capacity(records::FileKind::ALL.len() + 1);
        let verified = read(&seal, length, &mut |kind| {
            let name = kind.name();
            let file = self.open_file(&name)?;
            rows.push(file.try_clone().map_err(|_| RunError::Output)?);
            Ok(file)
        })?;
        rows.push(seal);
        Ok((verified, rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_owner_must_name_the_held_account() {
        let held = LocalSid::parse("S-1-5-21-1-2-3-1001").unwrap();
        assert!(sid_matches(&held, "S-1-5-21-1-2-3-1001"));
        for other in [
            "S-1-5-21-1-2-3-1002",
            "S-1-5-21-1-2-3-01001",
            "S-1-5-18",
            "",
        ] {
            assert!(!sid_matches(&held, other), "{other:?}");
        }
    }
}

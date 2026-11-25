//! Daemon selection and custody for the independently measured TEST inputs.

use super::*;
use crate::store::vm::VmStore;
use crate::vm::qualification_record::{
    self as records, Fresh, RunError, Selection, VerifiedEvidence,
};

pub(crate) struct ForegroundQualification<'a> {
    pub(super) held: Held,
    pub(super) recipes: std::sync::Mutex<launch::RecipeInputs>,
    store: &'a VmStore,
    dependencies: Dependencies,
    evidence: VerifiedEvidence,
    seal_sha256: records::Hash,
    _dossier: ReadDossier,
    _rows: Vec<File>,
}

impl<'a> ForegroundQualification<'a> {
    pub(crate) fn open(
        store: &'a VmStore,
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
            CollectMode::Foreground(store),
            |pin, owner, _, _| ReadDossier::open(pin, owner),
        )?;
        let mut dependencies = observations.dependencies();
        // This projection precedes all reads of the selected seal and rows.
        let recipes = launch::measure_recipes(&held, [0; 16], [0; 32])?;
        recipes.projection.bind(&mut dependencies);
        let fresh = measure_fresh(&held, store, &dependencies)?;
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

    pub(crate) fn evidence(&self) -> &VerifiedEvidence {
        &self.evidence
    }

    pub(in crate::vm::windows::qualification) fn seal_sha256(&self) -> records::Hash {
        self.seal_sha256
    }

    pub(crate) fn fresh(&self) -> Result<Fresh<'_>, NativeRefusal> {
        let fresh = measure_fresh(&self.held, self.store, &self.dependencies)?;
        let binding = self.evidence.seal.binding;
        let issued = binding
            .issued_on_boot(fresh.boot_marker)
            .ok_or(NativeRefusal::QualificationMissing)?;
        if !binding.live_at(fresh.now.utc, fresh.dependencies.key(binding.scope))
            || !records::clocks_agree(issued, fresh.now)
        {
            return Err(NativeRefusal::QualificationMissing);
        }
        Ok(fresh)
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

fn measure_fresh<'a>(
    held: &Held,
    store: &VmStore,
    dependencies: &'a Dependencies,
) -> Result<Fresh<'a>, NativeRefusal> {
    let reason = NativeRefusal::QualificationMissing;
    held.token
        .verify_current_profile()
        .map_err(|_| NativeRefusal::IdentityUnverified)?;
    inventory::binding(&held._source)?;
    held.scope
        .as_ref()
        .ok_or(reason)?
        .validate_foreground(store)?;
    for volume in &held.volumes {
        if !capacity::still_bound(&capacity::MountManager, volume) {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
    }
    inventory::binding(&held.trial)?;
    inventory::binding(&held._backing)?;
    launch::revalidate_assets(held)?;
    if !matches!(&dependencies.host, Measured::Measured(host)
        if Ok(host.windows_build) == platform()
            && Ok(host.windows_revision) == windows_revision()
            && Ok(host.machine) == machine_guid())
        || !matches!(&dependencies.owner_profile, Measured::Measured(profile)
            if held.token.profile_key().is_ok_and(|live| &live == profile))
    {
        return Err(reason);
    }
    Ok(Fresh {
        now: boot::now(),
        boot_marker: boot::marker()?,
        dependencies,
    })
}

pub(super) struct ReadDossier(PathPin);

impl ReadDossier {
    pub(super) fn open(pin: PathPin, owner: PSID) -> Result<Self, NativeRefusal> {
        if pin.mutable || !sink::held_private(&pin.leaf, owner) {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        Ok(Self(pin))
    }

    pub(super) fn read_selected(
        &self,
        selection: &Selection,
        fresh: &Fresh<'_>,
    ) -> Result<(VerifiedEvidence, Vec<File>), RunError> {
        inventory::binding(&self.0).map_err(|_| RunError::Output)?;
        let open = |name: &str| {
            let file = open_relative(&self.0.leaf, name, Node::File, false, false)
                .map_err(|_| RunError::Output)?;
            let id = file_identity(&file, false).map_err(|_| RunError::Output)?.0;
            if id.volume != self.0.identity.volume {
                return Err(RunError::Output);
            }
            Ok(file)
        };
        let seal = open("qualification.json")?;
        let length = seal.metadata().map_err(|_| RunError::Output)?.len();
        let mut rows = Vec::with_capacity(records::FileKind::ALL.len() + 1);
        let verified = records::read_native(
            &seal,
            length,
            |kind| {
                let name = kind.name();
                let file = open(&name)?;
                rows.push(file.try_clone().map_err(|_| RunError::Output)?);
                Ok(file)
            },
            selection,
            fresh,
        )?;
        rows.push(seal);
        Ok((verified, rows))
    }
}

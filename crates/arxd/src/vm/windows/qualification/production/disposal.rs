//! Resumable native TEST disposal through the last lease and empty directory
//! (ADR 0054 / 0072, 7c chunk 07).
//!
//! Ordinary reaper cleanup (`ArtifactCustody`) and transient-build disposal
//! before `publish_build` share this single engine. Every deletion commits a
//! durable intent before handle-based unlink and checkpoints observed absence
//! through the same held parent and bound name. Empty-workspace cleanup removes
//! the exact `generation.lease` and then the exact empty generation directory
//! under a generation-level guard spanning both handle handoffs.

use super::custody::observe;
use super::*;
use crate::store::vm::native::{
    self, Leaf, Presence, WorkspaceLeaf,
    disposal::{self, DisposalScope, GenerationDisposal, GenerationState, SlotDisposal},
};
use crate::vm::{
    BuildSlots,
    reaper::{ArtifactCustody, CleanupArtifact, CleanupKind, CleanupWork},
};
use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

type Q = NativeRefusal;
const PATH: Q = NativeRefusal::PathCustodyUnverified;

/// In-process generation-level guard serializing lease-close/delete and
/// directory-close/delete handoffs alongside the durable ledger fence and
/// the held `production` parent directory handle.
///
/// Exclusion applies only among callers sharing the same instance; the later
/// 7c issuer/reaper wiring PR must share one process-wide `GenerationGuards`
/// instance across all disposal callers.
#[derive(Default)]
pub(crate) struct GenerationGuards {
    active: Mutex<HashSet<Id>>,
}

pub(super) struct GenerationGuard<'g> {
    guards: &'g GenerationGuards,
    generation: Id,
}

impl GenerationGuards {
    pub(super) fn acquire(&self, generation: Id) -> Result<GenerationGuard<'_>, Q> {
        let mut active = self.active.lock().map_err(|_| PATH)?;
        if !active.insert(generation) {
            return Err(PATH);
        }
        Ok(GenerationGuard {
            guards: self,
            generation,
        })
    }
}

impl Drop for GenerationGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.guards.active.lock() {
            active.remove(&self.generation);
        }
    }
}

/// Durable disposal checkpoints over the VM ledger (store v16).
/// Every write commits one exact transition in its own immediate transaction
/// before the next filesystem mutation.
pub(crate) trait DisposalCheckpoints {
    fn generation_state(&self, generation: Id) -> Result<GenerationState, Q>;
    fn slot_disposal(&self, slot: ArtifactSlot) -> Result<(Leaf, SlotDisposal), Q>;
    fn settle_workspace_intent(
        &self,
        generation: Id,
        leaf: WorkspaceLeaf,
        presence: Presence,
    ) -> Result<(), Q>;
    fn settle_slot_intent(
        &self,
        scope: native::Scope,
        slot: ArtifactSlot,
        present: bool,
    ) -> Result<(), Q>;
    fn intend_slot_disposal(
        &self,
        auth: DisposalScope,
        scope: native::Scope,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q>;
    fn slot_disposed(
        &self,
        auth: DisposalScope,
        scope: native::Scope,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q>;
    fn intend_lease_disposal(&self, auth: DisposalScope, scope: native::Scope) -> Result<(), Q>;
    fn intend_directory_disposal(&self, auth: DisposalScope, scope: native::Scope)
    -> Result<(), Q>;
    fn generation_disposed(&self, auth: DisposalScope, scope: native::Scope) -> Result<(), Q>;
}

/// Immediate-transaction disposal checkpoints over `VmStore`.
pub(crate) struct LedgerDisposal<'a> {
    pub store: &'a crate::store::vm::VmStore,
}

impl LedgerDisposal<'_> {
    fn commit<T>(
        &self,
        run: impl FnOnce(&rusqlite::Connection) -> anyhow::Result<T>,
    ) -> Result<T, Q> {
        self.store.transact(|db| run(db)).map_err(|_| PATH)
    }
}

impl DisposalCheckpoints for LedgerDisposal<'_> {
    fn generation_state(&self, generation: Id) -> Result<GenerationState, Q> {
        self.commit(|db| disposal::generation_state(db, generation))
    }

    fn slot_disposal(&self, slot: ArtifactSlot) -> Result<(Leaf, SlotDisposal), Q> {
        self.commit(|db| disposal::slot_disposal(db, slot))
    }

    fn settle_workspace_intent(
        &self,
        generation: Id,
        leaf: WorkspaceLeaf,
        presence: Presence,
    ) -> Result<(), Q> {
        self.commit(|db| native::settle_workspace_intent(db, generation, leaf, presence))
    }

    fn settle_slot_intent(
        &self,
        scope: native::Scope,
        slot: ArtifactSlot,
        present: bool,
    ) -> Result<(), Q> {
        self.commit(|db| native::settle_slot_intent(db, scope, slot, present))
    }

    fn intend_slot_disposal(
        &self,
        auth: DisposalScope,
        scope: native::Scope,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q> {
        self.commit(|db| disposal::intend_slot_disposal(db, auth, scope, slot, identity))
    }

    fn slot_disposed(
        &self,
        auth: DisposalScope,
        scope: native::Scope,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q> {
        self.commit(|db| disposal::slot_disposed(db, auth, scope, slot, identity))
    }

    fn intend_lease_disposal(&self, auth: DisposalScope, scope: native::Scope) -> Result<(), Q> {
        self.commit(|db| disposal::intend_lease_disposal(db, auth, scope))
    }

    fn intend_directory_disposal(
        &self,
        auth: DisposalScope,
        scope: native::Scope,
    ) -> Result<(), Q> {
        self.commit(|db| disposal::intend_directory_disposal(db, auth, scope))
    }

    fn generation_disposed(&self, auth: DisposalScope, scope: native::Scope) -> Result<(), Q> {
        self.commit(|db| disposal::generation_disposed(db, auth, scope))
    }
}

struct PlannedSlot {
    slot: ArtifactSlot,
    expected_identity: Option<FileIdentity>,
    expected_directory: Option<Id>,
    require_identified: bool,
}

struct PlannedGeneration {
    generation: Id,
    remove_workspace: bool,
    slots: Vec<PlannedSlot>,
}

/// Shared native TEST disposal engine for both ordinary sweep (`ArtifactCustody`)
/// and transient-build disposal prior to `publish_build`.
pub(crate) struct NativeDisposal<'a> {
    production: &'a PathPin,
    owner: PSID,
    ledger: &'a dyn DisposalCheckpoints,
    guards: &'a GenerationGuards,
}

impl ForegroundQualification<'_> {
    /// Constructs a scoped native TEST disposal handle bound to the enrolled
    /// production root and owner SID. Keeps production TEST admission closed.
    pub(crate) fn disposal<'a>(
        &'a self,
        ledger: &'a dyn DisposalCheckpoints,
        guards: &'a GenerationGuards,
    ) -> NativeDisposal<'a> {
        NativeDisposal::under(
            self.production_root(),
            self.held.token.account.0,
            ledger,
            guards,
        )
    }
}

impl<'a> NativeDisposal<'a> {
    pub(super) fn under(
        production: &'a PathPin,
        owner: PSID,
        ledger: &'a dyn DisposalCheckpoints,
        guards: &'a GenerationGuards,
    ) -> Self {
        Self {
            production,
            owner,
            ledger,
            guards,
        }
    }

    /// Disposes a pending build's transient `seed` leaf (in primary generation)
    /// and its verification generation (`verification_overlay`, `vars`,
    /// `generation.lease`, empty directory) through the same resumable path as
    /// ordinary sweep. Primary `disk`, `vars`, lease and directory stay intact.
    pub(crate) fn dispose_build_transient(
        &self,
        activity_id: i64,
        slots: BuildSlots,
        deadline: Instant,
    ) -> Result<(), Q> {
        let auth = DisposalScope::build_transient(activity_id, slots);
        let primary = slots.pair.disk.generation;
        let verification = slots.verification.disk.generation;
        if primary == verification
            || slots.seed.generation != primary
            || slots.seed.role != "seed"
            || slots.verification.disk.role != "verification_overlay"
            || slots.verification.vars.generation != verification
            || slots.verification.vars.role != "vars"
        {
            return Err(PATH);
        }
        let plan = [
            PlannedGeneration {
                generation: primary,
                remove_workspace: false,
                slots: vec![PlannedSlot {
                    slot: slots.seed,
                    expected_identity: None,
                    expected_directory: None,
                    require_identified: true,
                }],
            },
            PlannedGeneration {
                generation: verification,
                remove_workspace: true,
                slots: vec![
                    PlannedSlot {
                        slot: slots.verification.disk,
                        expected_identity: None,
                        expected_directory: None,
                        require_identified: true,
                    },
                    PlannedSlot {
                        slot: slots.verification.vars,
                        expected_identity: None,
                        expected_directory: None,
                        require_identified: true,
                    },
                ],
            },
        ];
        self.execute_plan(auth, &plan, deadline)
    }

    /// Reconciles one reaper `CleanupWork` item. Recovery-only work and
    /// retained objects carry no deletion permit; release-only work is refused.
    pub(crate) fn dispose_cleanup(&self, work: &CleanupWork, deadline: Instant) -> Result<(), Q> {
        check_deadline(deadline)?;
        if work.release_only || !work.accounted {
            return Err(PATH);
        }
        if matches!(work.kind, CleanupKind::Recovery { .. }) {
            return self.verify_retained(work, deadline);
        }
        let mut by_gen: BTreeMap<Id, Vec<PlannedSlot>> = BTreeMap::new();
        if let CleanupKind::Snapshot { generation, .. } = work.kind {
            by_gen.entry(generation).or_default();
        }
        for artifact in &work.artifacts {
            if artifact.retain {
                return Err(PATH);
            }
            let Some(role) = disposal::test_role(&artifact.role) else {
                return Err(PATH);
            };
            if artifact.generation == [0; 16] || artifact.key == [0; 16] {
                return Err(PATH);
            }
            let slot = ArtifactSlot {
                generation: artifact.generation,
                role,
                key: artifact.key,
            };
            by_gen
                .entry(artifact.generation)
                .or_default()
                .push(PlannedSlot {
                    slot,
                    expected_identity: artifact.identity,
                    expected_directory: artifact.directory,
                    require_identified: false,
                });
        }
        if by_gen.is_empty() {
            return Err(PATH);
        }
        let plan: Vec<PlannedGeneration> = by_gen
            .into_iter()
            .map(|(generation, slots)| PlannedGeneration {
                generation,
                remove_workspace: true,
                slots,
            })
            .collect();
        self.execute_plan(DisposalScope::Cleanup(work.kind), &plan, deadline)
    }

    /// Read-only verification for `CleanupKind::Recovery`: never commits
    /// disposal checkpoints and never opens any object with `DELETE`.
    fn verify_retained(&self, work: &CleanupWork, deadline: Instant) -> Result<(), Q> {
        if work.artifacts.is_empty() {
            return Err(PATH);
        }
        self.verify_production_root()?;
        let mut by_gen: BTreeMap<Id, Vec<&CleanupArtifact>> = BTreeMap::new();
        for artifact in &work.artifacts {
            if !artifact.retain {
                return Err(PATH);
            }
            if !matches!(artifact.role.as_str(), "disk" | "vars") {
                return Err(PATH);
            }
            by_gen
                .entry(artifact.generation)
                .or_default()
                .push(artifact);
        }
        for (generation, artifacts) in by_gen {
            check_deadline(deadline)?;
            let _guard = self.guards.acquire(generation)?;
            let GenerationState::Bound {
                scope: gen_scope,
                disposal: GenerationDisposal::None,
            } = self.ledger.generation_state(generation)?
            else {
                return Err(PATH);
            };
            if gen_scope.production != self.production.identity {
                return Err(PATH);
            }
            let ws =
                hold_workspace_under(self.production, self.owner, binding_from_scope(gen_scope))?;
            let mut exclusions = Vec::with_capacity(artifacts.len());
            for artifact in artifacts {
                check_deadline(deadline)?;
                let Some(role) = disposal::test_role(&artifact.role) else {
                    return Err(PATH);
                };
                let (Some(identity), Some(directory)) = (artifact.identity, artifact.directory)
                else {
                    return Err(PATH);
                };
                if directory != gen_scope.workspace.file
                    || identity.volume != gen_scope.production.volume
                {
                    return Err(PATH);
                }
                let slot = ArtifactSlot {
                    generation,
                    role,
                    key: artifact.key,
                };
                let (custody, slot_disp) = self.ledger.slot_disposal(slot)?;
                let matches_recorded = matches!(
                    custody,
                    Leaf::Identified(file) | Leaf::Verified(file, _) if file == identity.file
                );
                if !matches_recorded || slot_disp != SlotDisposal::None {
                    return Err(PATH);
                }
                let pin = ws.pin_slot(slot, identity, false)?;
                exclusions.push(pin.reopen_without_writer().map_err(|_| PATH)?);
                if !sink::held_private(&pin.leaf, self.owner)
                    || !has_exact_leaf(&pin.leaf, &leaf_name(slot))
                {
                    return Err(PATH);
                }
            }
            WorkspaceCustody::production(&ws.root, &ws.lease)?;
        }
        Ok(())
    }

    fn execute_plan(
        &self,
        auth: DisposalScope,
        plan: &[PlannedGeneration],
        deadline: Instant,
    ) -> Result<(), Q> {
        self.verify_production_root()?;
        for target in plan {
            check_deadline(deadline)?;
            self.dispose_generation(auth, target, deadline)?;
        }
        Ok(())
    }

    fn verify_production_root(&self) -> Result<(), Q> {
        inventory::binding(self.production)?;
        if file_identity(&self.production.leaf, true)
            .map_err(|_| PATH)?
            .0
            != self.production.identity
        {
            return Err(PATH);
        }
        Ok(())
    }

    fn dispose_generation(
        &self,
        auth: DisposalScope,
        target: &PlannedGeneration,
        deadline: Instant,
    ) -> Result<(), Q> {
        let generation = target.generation;
        if generation == [0; 16] {
            return Err(PATH);
        }
        let _guard = self.guards.acquire(generation)?;
        let gen_name = hex::encode(generation);
        let state = self.ledger.generation_state(generation)?;
        match state {
            GenerationState::Unreserved {
                has_identified_artifact,
            } => {
                // No native provenance: never perform any deletion. Accept only
                // a separately proved uncreated reservation with zero identified
                // artifacts and observed directory absence under the held root.
                if has_identified_artifact
                    || !target.remove_workspace
                    || target.slots.iter().any(|slot| {
                        slot.require_identified
                            || slot.expected_identity.is_some()
                            || slot.expected_directory.is_some()
                    })
                {
                    return Err(PATH);
                }
                self.require_generation_absent(&gen_name)?;
                Ok(())
            }
            GenerationState::Intended { production } => {
                if production != self.production.identity || !target.remove_workspace {
                    return Err(PATH);
                }
                let presence = observe(&self.production.leaf, &gen_name, Node::Directory)?;
                self.ledger.settle_workspace_intent(
                    generation,
                    WorkspaceLeaf::Directory,
                    presence,
                )?;
                if presence == Presence::Absent {
                    Ok(())
                } else {
                    Err(PATH)
                }
            }
            GenerationState::Absent { production } => {
                if production != self.production.identity || !target.remove_workspace {
                    return Err(PATH);
                }
                self.require_generation_absent(&gen_name)?;
                Ok(())
            }
            GenerationState::Directory { .. } | GenerationState::Quarantined => Err(PATH),
            GenerationState::Bound {
                scope: gen_scope,
                disposal,
            } => {
                if gen_scope.production != self.production.identity {
                    return Err(PATH);
                }
                self.dispose_bound_generation(auth, gen_scope, disposal, target, deadline)
            }
        }
    }

    fn dispose_bound_generation(
        &self,
        auth: DisposalScope,
        gen_scope: native::Scope,
        mut disposal: GenerationDisposal,
        target: &PlannedGeneration,
        deadline: Instant,
    ) -> Result<(), Q> {
        let gen_name = hex::encode(gen_scope.generation);
        let mut held_dir: Option<File> = None;

        if disposal == GenerationDisposal::None {
            let ws =
                hold_workspace_under(self.production, self.owner, binding_from_scope(gen_scope))?;
            for planned in &target.slots {
                check_deadline(deadline)?;
                self.dispose_slot(auth, &ws, gen_scope, planned)?;
            }
            if !target.remove_workspace {
                // Slot-only disposal (transient seed in primary generation):
                // keep the workspace lease, directory and remaining slots.
                WorkspaceCustody::production(&ws.root, &ws.lease)?;
                return Ok(());
            }
            // Refuse unexpected extra siblings before entering lease disposal.
            verify_only_lease(&ws.root.leaf, deadline)?;
            WorkspaceCustody::production(&ws.root, &ws.lease)?;
            // Durably close `bound()` and `vm_native_slot_scope` BEFORE closing
            // the exclusive lease handle.
            self.ledger.intend_lease_disposal(auth, gen_scope)?;
            // Release ONLY the lease handle; keep the generation directory,
            // production root and in-process `GenerationGuard` held.
            let ProductionWorkspace {
                root,
                lease,
                binding: _,
            } = ws;
            drop(lease);
            held_dir = Some(root.leaf);
            disposal = GenerationDisposal::Lease;
        } else if !target.remove_workspace {
            // Primary generation of a pending build must never enter workspace
            // disposal.
            return Err(PATH);
        }

        if disposal == GenerationDisposal::Lease {
            check_deadline(deadline)?;
            let dir = match held_dir.take() {
                Some(dir) => {
                    self.verify_generation_dir(&dir, gen_scope, &gen_name)?;
                    dir
                }
                None => {
                    self.ledger.intend_lease_disposal(auth, gen_scope)?;
                    self.open_verified_generation_dir(gen_scope, &gen_name)?
                }
            };
            self.remove_exact_lease(&dir, gen_scope, deadline)?;
            verify_dir_empty(&dir, deadline)?;
            self.verify_generation_dir(&dir, gen_scope, &gen_name)?;
            self.ledger.intend_directory_disposal(auth, gen_scope)?;
            // Release the generation directory handle while keeping the
            // production root and `GenerationGuard` held.
            drop(dir);
            disposal = GenerationDisposal::Directory;
        }

        if disposal == GenerationDisposal::Directory {
            check_deadline(deadline)?;
            self.ledger.intend_directory_disposal(auth, gen_scope)?;
            self.remove_exact_directory(gen_scope, &gen_name, deadline)?;
            self.verify_production_root()?;
            self.ledger.generation_disposed(auth, gen_scope)?;
            return Ok(());
        }

        if disposal == GenerationDisposal::Absent {
            self.require_generation_absent(&gen_name)?;
            return Ok(());
        }

        Err(PATH)
    }

    fn dispose_slot(
        &self,
        auth: DisposalScope,
        ws: &ProductionWorkspace,
        gen_scope: native::Scope,
        planned: &PlannedSlot,
    ) -> Result<(), Q> {
        let slot = planned.slot;
        if slot.generation != gen_scope.generation
            || slot.key == [0; 16]
            || disposal::test_role(slot.role).is_none()
        {
            return Err(PATH);
        }
        let leaf_file = leaf_name(slot);
        let (custody, slot_disp) = self.ledger.slot_disposal(slot)?;
        match (custody, slot_disp) {
            (Leaf::NeverAttempted, SlotDisposal::None) => {
                if planned.require_identified
                    || planned.expected_identity.is_some()
                    || planned.expected_directory.is_some()
                {
                    return Err(PATH);
                }
                WorkspaceCustody::production(&ws.root, &ws.lease)?;
                if observe(&ws.root.leaf, &leaf_file, Node::File)? != Presence::Absent {
                    return Err(PATH);
                }
                Ok(())
            }
            (Leaf::Intended, SlotDisposal::None) => {
                if planned.require_identified
                    || planned.expected_identity.is_some()
                    || planned.expected_directory.is_some()
                {
                    return Err(PATH);
                }
                WorkspaceCustody::production(&ws.root, &ws.lease)?;
                let present = observe(&ws.root.leaf, &leaf_file, Node::File)? != Presence::Absent;
                self.ledger.settle_slot_intent(gen_scope, slot, present)?;
                if present { Err(PATH) } else { Ok(()) }
            }
            (Leaf::Absent, SlotDisposal::None) => {
                if planned.require_identified
                    || planned.expected_identity.is_some()
                    || planned.expected_directory.is_some()
                {
                    return Err(PATH);
                }
                WorkspaceCustody::production(&ws.root, &ws.lease)?;
                if observe(&ws.root.leaf, &leaf_file, Node::File)? != Presence::Absent {
                    return Err(PATH);
                }
                Ok(())
            }
            (Leaf::Unidentified, _) => Err(PATH),
            (Leaf::Identified(recorded_file) | Leaf::Verified(recorded_file, _), disp) => {
                let expected = FileIdentity {
                    volume: gen_scope.production.volume,
                    file: recorded_file,
                };
                if let Some(arg_id) = planned.expected_identity
                    && arg_id != expected
                {
                    return Err(PATH);
                }
                if let Some(arg_dir) = planned.expected_directory
                    && arg_dir != gen_scope.workspace.file
                {
                    return Err(PATH);
                }
                if !planned.require_identified
                    && (planned.expected_identity.is_none() || planned.expected_directory.is_none())
                {
                    return Err(PATH);
                }
                if disp == SlotDisposal::Absent {
                    WorkspaceCustody::production(&ws.root, &ws.lease)?;
                    if observe(&ws.root.leaf, &leaf_file, Node::File)? != Presence::Absent {
                        return Err(PATH);
                    }
                    return Ok(());
                }
                match open_relative_delete(&ws.root.leaf, &leaf_file, Node::File) {
                    Ok(handle) => {
                        // Validate the exact handle before committing delete intent
                        // or marking it for deletion. A replaced object is refused
                        // and left untouched.
                        let (actual, _) = file_identity(&handle, false).map_err(|_| PATH)?;
                        if actual != expected
                            || !has_exact_leaf(&handle, &leaf_file)
                            || !sink::held_private(&handle, self.owner)
                        {
                            return Err(PATH);
                        }
                        WorkspaceCustody::production(&ws.root, &ws.lease)?;
                        self.ledger
                            .intend_slot_disposal(auth, gen_scope, slot, expected)?;
                        crate::vm::publish::windows::delete_on_close(&handle).map_err(|_| PATH)?;
                        drop(handle);
                        if observe(&ws.root.leaf, &leaf_file, Node::File)? != Presence::Absent {
                            return Err(PATH);
                        }
                        WorkspaceCustody::production(&ws.root, &ws.lease)?;
                        self.ledger.slot_disposed(auth, gen_scope, slot, expected)?;
                        Ok(())
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        // Prior absence is accepted ONLY under an exact durable
                        // delete intent for this slot and observed absence from
                        // the same bound parent and name.
                        if disp != SlotDisposal::Intended {
                            return Err(PATH);
                        }
                        if observe(&ws.root.leaf, &leaf_file, Node::File)? != Presence::Absent {
                            return Err(PATH);
                        }
                        WorkspaceCustody::production(&ws.root, &ws.lease)?;
                        self.ledger.slot_disposed(auth, gen_scope, slot, expected)?;
                        Ok(())
                    }
                    Err(_) => Err(PATH),
                }
            }
            _ => Err(PATH),
        }
    }

    fn open_verified_generation_dir(
        &self,
        gen_scope: native::Scope,
        gen_name: &str,
    ) -> Result<File, Q> {
        self.verify_production_root()?;
        let dir = open_relative(
            &self.production.leaf,
            gen_name,
            Node::Directory,
            false,
            false,
        )
        .map_err(|_| PATH)?;
        self.verify_generation_dir(&dir, gen_scope, gen_name)?;
        Ok(dir)
    }

    fn verify_generation_dir(
        &self,
        dir: &File,
        gen_scope: native::Scope,
        gen_name: &str,
    ) -> Result<(), Q> {
        self.verify_production_root()?;
        let (actual, _) = file_identity(dir, true).map_err(|_| PATH)?;
        if actual != gen_scope.workspace
            || actual.volume != self.production.identity.volume
            || !has_exact_leaf(dir, gen_name)
            || !sink::held_private(dir, self.owner)
        {
            return Err(PATH);
        }
        Ok(())
    }

    fn remove_exact_lease(
        &self,
        dir: &File,
        gen_scope: native::Scope,
        deadline: Instant,
    ) -> Result<(), Q> {
        match open_relative_delete(dir, GENERATION_LEASE, Node::File) {
            Ok(lease_del) => {
                let (actual, _) = file_identity(&lease_del, false).map_err(|_| PATH)?;
                if actual != gen_scope.lease
                    || actual.volume != gen_scope.workspace.volume
                    || !has_exact_leaf(&lease_del, GENERATION_LEASE)
                    || lease_del.metadata().map_err(|_| PATH)?.len() != 0
                    || !sink::held_private(&lease_del, self.owner)
                {
                    return Err(PATH);
                }
                lease_del.try_lock().map_err(|_| PATH)?;
                verify_only_lease(dir, deadline)?;
                crate::vm::publish::windows::delete_on_close(&lease_del).map_err(|_| PATH)?;
                drop(lease_del);
                if observe(dir, GENERATION_LEASE, Node::File)? != Presence::Absent {
                    return Err(PATH);
                }
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Accepted only because durable `disposal = 'lease'` is already
                // committed for this generation.
                if observe(dir, GENERATION_LEASE, Node::File)? != Presence::Absent {
                    return Err(PATH);
                }
                Ok(())
            }
            Err(_) => Err(PATH),
        }
    }

    fn remove_exact_directory(
        &self,
        gen_scope: native::Scope,
        gen_name: &str,
        deadline: Instant,
    ) -> Result<(), Q> {
        self.verify_production_root()?;
        match open_relative_delete(&self.production.leaf, gen_name, Node::Directory) {
            Ok(dir_del) => {
                self.verify_generation_dir(&dir_del, gen_scope, gen_name)?;
                verify_dir_empty(&dir_del, deadline)?;
                crate::vm::publish::windows::delete_on_close(&dir_del).map_err(|_| PATH)?;
                drop(dir_del);
                self.require_generation_absent(gen_name)?;
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Accepted only because durable `disposal = 'directory'` is
                // already committed for this generation.
                self.require_generation_absent(gen_name)?;
                Ok(())
            }
            Err(_) => Err(PATH),
        }
    }

    fn require_generation_absent(&self, gen_name: &str) -> Result<(), Q> {
        self.verify_production_root()?;
        if observe(&self.production.leaf, gen_name, Node::Directory)? != Presence::Absent {
            return Err(PATH);
        }
        Ok(())
    }
}

impl ArtifactCustody for NativeDisposal<'_> {
    fn reconcile(&self, work: &CleanupWork, deadline: Instant) -> bool {
        self.dispose_cleanup(work, deadline).is_ok()
    }
}

fn check_deadline(deadline: Instant) -> Result<(), Q> {
    if Instant::now() >= deadline {
        Err(PATH)
    } else {
        Ok(())
    }
}

pub(super) fn leaf_name(slot: ArtifactSlot) -> String {
    format!("{}-{}", slot.role, hex::encode(slot.key))
}

pub(super) fn binding_from_scope(gen_scope: native::Scope) -> WorkspaceBinding {
    WorkspaceBinding {
        generation: gen_scope.generation,
        production: gen_scope.production,
        workspace: gen_scope.workspace,
        lease: gen_scope.lease,
    }
}

fn verify_only_lease(dir: &File, deadline: Instant) -> Result<(), Q> {
    let entries = listing_bounded(dir, 2, Some(deadline)).map_err(|_| PATH)?;
    match entries.as_slice() {
        [(name, attrs)]
            if name == GENERATION_LEASE
                && attrs & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0 =>
        {
            Ok(())
        }
        _ => Err(PATH),
    }
}

fn verify_dir_empty(dir: &File, deadline: Instant) -> Result<(), Q> {
    let entries = listing_bounded(dir, 1, Some(deadline)).map_err(|_| PATH)?;
    if entries.is_empty() {
        Ok(())
    } else {
        Err(PATH)
    }
}

#[cfg(test)]
#[path = "disposal_tests.rs"]
mod tests;

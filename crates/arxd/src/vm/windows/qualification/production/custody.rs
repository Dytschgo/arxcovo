//! Durable custody before writes (ADR 0072): workspace reservation, slot
//! creation, the writable helper target and the ledger checkpoint adapter.

use super::*;
use crate::store::vm::native;
use crate::store::vm::native::{Presence, WorkspaceLeaf};

type Q = NativeRefusal;
const PATH: Q = NativeRefusal::PathCustodyUnverified;

impl ProductionWorkspace {
    fn valid_slot(&self, slot: ArtifactSlot) -> Result<String, Q> {
        if slot.generation != self.binding.generation
            || slot.key == [0; 16]
            || !matches!(slot.role, "disk" | "vars" | "seed" | "verification_overlay")
        {
            return Err(PATH);
        }
        Ok(format!("{}-{}", slot.role, hex::encode(slot.key)))
    }

    /// Creates one new slot relative to the held directory. The ledger
    /// records the create intent before the create-new and the object's
    /// identity before the returned writer can write. An occupied leaf, or
    /// a created leaf whose identity was not recorded, stays quarantined.
    pub(in crate::vm::windows::qualification) fn create_slot(
        &self,
        slot: ArtifactSlot,
        ledger: &dyn CustodyCheckpoints,
    ) -> Result<SlotWriter<'_>, Q> {
        let leaf = self.valid_slot(slot)?;
        let custody = WorkspaceCustody::production(&self.root, &self.lease)?;
        ledger.intend_slot(self.binding, slot)?;
        let identified = open_relative(&self.root.leaf, &leaf, Node::File, true, true)
            .ok()
            .and_then(|file| {
                let (identity, _) = file_identity(&file, false).ok()?;
                (identity.volume == self.binding.workspace.volume
                    && has_exact_leaf(&file, &leaf)
                    && custody.verify().is_ok()
                    && ledger.slot_identified(self.binding, slot, identity).is_ok())
                .then_some((file, identity))
            });
        match identified {
            Some((file, identity)) => Ok(SlotWriter {
                workspace: self,
                slot,
                identity,
                file,
            }),
            None => {
                // Classify the intent only after any creation handle closed.
                let _ = self.settle_intended(slot, ledger);
                Err(PATH)
            }
        }
    }

    /// Settles an unidentified slot intent from the held directory. Only an
    /// exact `NotFound` proves absence. A present object of either kind is
    /// never adopted.
    pub(crate) fn settle_intended(
        &self,
        slot: ArtifactSlot,
        ledger: &dyn CustodyCheckpoints,
    ) -> Result<(), Q> {
        let leaf = self.valid_slot(slot)?;
        WorkspaceCustody::production(&self.root, &self.lease)?;
        let present = observe(&self.root.leaf, &leaf, Node::File)? != Presence::Absent;
        ledger.slot_intent_settled(self.binding, slot, present)
    }

    /// Precreates the empty target of a checked qemu-img write. Its
    /// identity is durable before any helper can open it for writing, and
    /// the checked launch accepts a writable helper only for this target.
    pub(crate) fn precreate_helper_target<'l>(
        &self,
        slot: ArtifactSlot,
        ledger: &'l dyn CustodyCheckpoints,
    ) -> Result<HelperTarget<'l>, Q> {
        if !matches!(slot.role, "disk" | "verification_overlay") {
            return Err(PATH);
        }
        let writer = self.create_slot(slot, ledger)?;
        writer.sync()?;
        let pin = writer.into_pin(true)?;
        if pin.leaf.metadata().map_err(|_| PATH)?.len() != 0 {
            return Err(PATH);
        }
        Ok(HelperTarget {
            binding: self.binding,
            slot,
            pin,
            ledger,
        })
    }
}

/// A bounded, type-agnostic, no-follow observation of one name through the
/// held parent. Only an exact `NotFound` is absence. Any opened object is
/// present. Its identity is reported only when it is the expected kind.
pub(super) fn observe(parent: &File, name: &str, node: Node) -> Result<Presence, Q> {
    match probe_relative(parent, name) {
        Ok(object) => Ok(Presence::Present(
            file_identity(&object, node == Node::Directory)
                .ok()
                .map(|(identity, _)| identity),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Presence::Absent),
        Err(_) => Err(PATH),
    }
}

/// Durable custody checkpoints over the VM ledger (ADR 0072). Each call
/// commits one exact transition before it returns. An error stops the
/// effect before its next filesystem mutation, so the last committed
/// checkpoint keeps the charge and names the recovery route.
pub(crate) trait CustodyCheckpoints {
    /// Records the directory create intent. A committed directory
    /// checkpoint returns its recorded identity for the reservation to
    /// resume at the lease.
    fn intend_workspace(
        &self,
        generation: Id,
        production: FileIdentity,
    ) -> Result<Option<FileIdentity>, Q>;
    /// Records the directory identity and the lease create intent.
    fn workspace_directory(&self, generation: Id, directory: FileIdentity) -> Result<(), Q>;
    fn workspace_bound(&self, binding: WorkspaceBinding) -> Result<(), Q>;
    /// The exact durable scope every hold, slot and dispatch requires.
    fn confirm_bound(&self, binding: WorkspaceBinding) -> Result<(), Q>;
    fn workspace_intent_settled(
        &self,
        generation: Id,
        leaf: WorkspaceLeaf,
        presence: Presence,
    ) -> Result<(), Q>;
    /// The exact start pair names the durable slots of this bound scope.
    fn confirm_start_pair(&self, binding: WorkspaceBinding, pair: StartPair) -> Result<(), Q>;
    fn intend_slot(&self, binding: WorkspaceBinding, slot: ArtifactSlot) -> Result<(), Q>;
    fn slot_identified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q>;
    /// The slot is still identified, and not yet verified, as this object.
    fn confirm_identified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q>;
    fn slot_verified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        evidence: ArtifactEvidence,
    ) -> Result<(), Q>;
    fn slot_intent_settled(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        present: bool,
    ) -> Result<(), Q>;
}

/// Ledger checkpoints for one reserved generation. The issuer supplies the
/// origin protocol of the operation that owns it.
pub(crate) struct LedgerCheckpoints<'a> {
    pub store: &'a crate::store::vm::VmStore,
    pub origin: native::Origin,
}

pub(super) fn scope(binding: WorkspaceBinding) -> native::Scope {
    native::Scope {
        generation: binding.generation,
        production: binding.production,
        workspace: binding.workspace,
        lease: binding.lease,
    }
}

impl LedgerCheckpoints<'_> {
    fn commit<T>(
        &self,
        run: impl FnOnce(&rusqlite::Connection) -> anyhow::Result<T>,
    ) -> Result<T, Q> {
        self.store.transact(|db| run(db)).map_err(|_| PATH)
    }

    fn confirm(
        &self,
        check: impl FnOnce(&rusqlite::Connection) -> anyhow::Result<bool>,
    ) -> Result<(), Q> {
        self.commit(check)?.then_some(()).ok_or(PATH)
    }
}

impl CustodyCheckpoints for LedgerCheckpoints<'_> {
    fn intend_workspace(
        &self,
        generation: Id,
        production: FileIdentity,
    ) -> Result<Option<FileIdentity>, Q> {
        self.commit(|db| native::intend_workspace(db, generation, self.origin, production))
    }

    fn workspace_directory(&self, generation: Id, directory: FileIdentity) -> Result<(), Q> {
        self.commit(|db| native::identify_directory(db, generation, directory))
    }

    fn workspace_bound(&self, binding: WorkspaceBinding) -> Result<(), Q> {
        self.commit(|db| native::bind_workspace(db, scope(binding)))
    }

    fn confirm_bound(&self, binding: WorkspaceBinding) -> Result<(), Q> {
        self.confirm(|db| native::bound(db, scope(binding)))
    }

    fn workspace_intent_settled(
        &self,
        generation: Id,
        leaf: WorkspaceLeaf,
        presence: Presence,
    ) -> Result<(), Q> {
        self.commit(|db| native::settle_workspace_intent(db, generation, leaf, presence))
    }

    fn confirm_start_pair(&self, binding: WorkspaceBinding, pair: StartPair) -> Result<(), Q> {
        // The same shape the held start refusal requires.
        check_pair(binding, pair.slots)?;
        let slots = [pair.slots.disk, pair.slots.vars];
        self.confirm(|db| native::pair_matches(db, scope(binding), slots, pair.evidence))
    }

    fn intend_slot(&self, binding: WorkspaceBinding, slot: ArtifactSlot) -> Result<(), Q> {
        self.commit(|db| native::intend_slot(db, scope(binding), slot))
    }

    fn slot_identified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q> {
        self.commit(|db| native::identify_slot(db, scope(binding), slot, identity))
    }

    fn confirm_identified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        identity: FileIdentity,
    ) -> Result<(), Q> {
        self.confirm(|db| native::identified_as(db, scope(binding), slot, identity))
    }

    fn slot_verified(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        evidence: ArtifactEvidence,
    ) -> Result<(), Q> {
        self.commit(|db| native::verify_slot(db, scope(binding), slot, evidence))
    }

    fn slot_intent_settled(
        &self,
        binding: WorkspaceBinding,
        slot: ArtifactSlot,
        present: bool,
    ) -> Result<(), Q> {
        self.commit(|db| native::settle_slot_intent(db, scope(binding), slot, present))
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm::windows::qualification) enum WriteFault {
    /// Writes the first half of the payload, then fails.
    Partial,
    Sync,
}

#[cfg(test)]
thread_local! {
    pub(in crate::vm::windows::qualification) static WRITE_FAULT:
        std::cell::Cell<Option<WriteFault>> = const { std::cell::Cell::new(None) };
}

/// Write access to one slot whose identity the ledger already holds.
pub(in crate::vm::windows::qualification) struct SlotWriter<'w> {
    workspace: &'w ProductionWorkspace,
    slot: ArtifactSlot,
    identity: FileIdentity,
    file: File,
}

impl SlotWriter<'_> {
    pub(in crate::vm::windows::qualification) fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub(in crate::vm::windows::qualification) fn write_all(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Q> {
        use std::io::Write;
        #[cfg(test)]
        if WRITE_FAULT.get() == Some(WriteFault::Partial) {
            let _ = self.file.write_all(&bytes[..bytes.len() / 2]);
            return Err(PATH);
        }
        self.file.write_all(bytes).map_err(|_| PATH)
    }

    /// The identified object's current length through the writer handle.
    pub(in crate::vm::windows::qualification) fn len(&self) -> Result<u64, Q> {
        self.file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|_| PATH)
    }

    pub(in crate::vm::windows::qualification) fn sync(&self) -> Result<(), Q> {
        #[cfg(test)]
        if WRITE_FAULT.get() == Some(WriteFault::Sync) {
            return Err(PATH);
        }
        self.file.sync_all().map_err(|_| PATH)
    }

    /// Bridges to a held pin without releasing the exclusion against rename
    /// or replacement.
    pub(in crate::vm::windows::qualification) fn into_pin(
        self,
        mutable: bool,
    ) -> Result<PathPin, Q> {
        let pin = self.workspace.pin_slot(self.slot, self.identity, true)?;
        drop(self.file);
        if mutable {
            Ok(pin)
        } else {
            self.workspace.pin_slot(self.slot, self.identity, false)
        }
    }
}

/// An empty, ledger-identified, writable helper target. Only this type can
/// satisfy the writable input of a checked Mutate launch.
pub(crate) struct HelperTarget<'l> {
    binding: WorkspaceBinding,
    slot: ArtifactSlot,
    pub(super) pin: PathPin,
    ledger: &'l dyn CustodyCheckpoints,
}

impl HelperTarget<'_> {
    pub(crate) fn binding(&self) -> WorkspaceBinding {
        self.binding
    }

    pub(crate) fn slot(&self) -> ArtifactSlot {
        self.slot
    }

    pub(crate) fn identity(&self) -> FileIdentity {
        self.pin.identity
    }
}

/// The checked dispatch edge for writable helpers. A Mutate plan must name
/// the exact held helper target as its only writable input, the workspace
/// must still hold that object at its slot, and the ledger must still record
/// it as identified and unverified. Any other plan kind takes no target.
pub(in crate::vm::windows::qualification) fn check_helper_target(
    kind: PlanKind,
    inputs: &[(ArtifactRole, crate::vm::paths::Artifact)],
    workspace: &ProductionWorkspace,
    target: Option<&HelperTarget<'_>>,
) -> Result<(), Q> {
    let target = match (kind, target) {
        (PlanKind::Mutate, Some(target)) => target,
        (PlanKind::Mutate, None) | (_, Some(_)) => return Err(PATH),
        (_, None) => return Ok(()),
    };
    let binding = workspace.binding();
    let identity = target.identity();
    let expected = slot_path(&workspace.root.path, target.slot)?;
    let mut writable = inputs.iter().filter(|(role, _)| role.writable());
    let input = writable.next().ok_or(PATH)?;
    if writable.next().is_some()
        || target.binding != binding
        || input.1.identity() != Some(identity)
        || input.1.path_for(input.0).map_err(|_| PATH)? != &expected
    {
        return Err(PATH);
    }
    workspace.pin_slot(target.slot, identity, true)?;
    target
        .ledger
        .confirm_identified(binding, target.slot, identity)
}

impl ForegroundQualification {
    /// Creates and binds a new generation workspace (ADR 0072).
    pub(crate) fn reserve_workspace(
        &self,
        generation: Id,
        ledger: &dyn CustodyCheckpoints,
    ) -> Result<ProductionWorkspace, Q> {
        reserve_workspace_under(
            self.production_root(),
            self.held.token.account.0,
            generation,
            ledger,
        )
    }
}

/// Intent, create-new and identity for the generation directory, then for
/// its lease, each committed before the next mutation. A committed directory
/// checkpoint resumes at the lease. Bridging handles share write so the
/// exclusive hold can open both objects without ever releasing the exclusion
/// against rename or replacement.
pub(super) fn reserve_workspace_under(
    production: &PathPin,
    owner: PSID,
    generation: Id,
    ledger: &dyn CustodyCheckpoints,
) -> Result<ProductionWorkspace, Q> {
    inventory::binding(production)?;
    if generation == [0; 16]
        || file_identity(&production.leaf, true).map_err(|_| PATH)?.0 != production.identity
    {
        return Err(PATH);
    }
    let name = hex::encode(generation);
    let settle = |parent: &File, leaf: WorkspaceLeaf| {
        let (object, node) = match leaf {
            WorkspaceLeaf::Directory => (name.as_str(), Node::Directory),
            WorkspaceLeaf::Lease => (GENERATION_LEASE, Node::File),
        };
        if let Ok(presence) = observe(parent, object, node) {
            let _ = ledger.workspace_intent_settled(generation, leaf, presence);
        }
    };
    let held_directory = |id: FileIdentity, check: &File| {
        id.volume == production.identity.volume
            && has_exact_leaf(check, &name)
            && sink::held_private(check, owner)
    };
    let resume = ledger.intend_workspace(generation, production.identity)?;
    let directory = match resume {
        // Resume from the durable directory checkpoint: the recorded object
        // must still be the exact private directory. Nothing is created.
        Some(recorded) => open_relative(&production.leaf, &name, Node::Directory, true, false)
            .ok()
            .filter(|bridge| {
                file_identity(bridge, true).is_ok_and(|(held, _)| held == recorded)
                    && held_directory(recorded, bridge)
            })
            .map(|bridge| (bridge, recorded)),
        None => open_relative(&production.leaf, &name, Node::Directory, true, true)
            .ok()
            .and_then(|created| {
                let (id, _) = file_identity(&created, true).ok()?;
                let bridge =
                    open_relative(&production.leaf, &name, Node::Directory, true, false).ok()?;
                (file_identity(&bridge, true).is_ok_and(|(held, _)| held == id)
                    && held_directory(id, &created)
                    && ledger.workspace_directory(generation, id).is_ok())
                .then_some((bridge, id))
            }),
    };
    let Some((directory, workspace)) = directory else {
        settle(&production.leaf, WorkspaceLeaf::Directory);
        return Err(PATH);
    };
    let lease = open_relative(&directory, GENERATION_LEASE, Node::File, true, true)
        .ok()
        .and_then(|created| {
            created.sync_all().ok()?;
            let (lease, _) = file_identity(&created, false).ok()?;
            let bridge =
                open_relative(&directory, GENERATION_LEASE, Node::File, true, false).ok()?;
            let binding = WorkspaceBinding {
                generation,
                production: production.identity,
                workspace,
                lease,
            };
            (lease.volume == workspace.volume
                && file_identity(&bridge, false).is_ok_and(|(held, _)| held == lease)
                && sink::held_private(&created, owner)
                && ledger.workspace_bound(binding).is_ok())
            .then_some((bridge, binding))
        });
    let Some((_lease_bridge, binding)) = lease else {
        settle(&directory, WorkspaceLeaf::Lease);
        return Err(PATH);
    };
    hold_workspace_under(production, owner, binding)
}

/// Bounded observation of the exact released TEST pair. Keep the original
/// lease and both writer exclusions borrowed across every commit attempt.
pub(crate) struct TestRelease<'proof> {
    _held: &'proof HeldWriters,
    observed: native::release::Observed,
}

impl ReleasedGeneration<'_> {
    pub(crate) fn observe_test_release(
        &self,
        target: native::release::Target,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<TestRelease<'_>, NativeRefusal> {
        launch::check_preparation(deadline, cancelled)?;
        self._workspace.verify()?;
        if target.generation != self.binding.generation {
            return Err(PATH);
        }
        let (slots, pins, clean) = match self.targets {
            ReleaseTargets::Unchanged(pair) => (pair.slots, pair.evidence, true),
            ReleaseTargets::Pair { pair, ids } => {
                let clean = match self.stop_disposition {
                    Some(StopDisposition::CleanRelease) => true,
                    Some(StopDisposition::DirtyRelease) => false,
                    _ => return Err(PATH),
                };
                let evidence = |index: usize| -> Result<ArtifactEvidence, NativeRefusal> {
                    let identity = ids[index];
                    let guard = self
                        ._held
                        .1
                        .iter()
                        .find(|guard| {
                            file_identity(guard, false).is_ok_and(|(held, _)| held == identity)
                        })
                        .ok_or(PATH)?;
                    let pin =
                        launch::hash_held(guard, START_PAIR_LIMITS[index], deadline, cancelled)?;
                    Ok(ArtifactEvidence {
                        identity,
                        directory: self.binding.workspace.file,
                        pin,
                    })
                };
                (pair, [evidence(0)?, evidence(1)?], clean)
            }
            ReleaseTargets::Helper { .. } => return Err(PATH),
        };
        check_pair(self.binding, slots)?;
        if slots.disk.role != "disk" {
            return Err(PATH);
        }
        self._workspace.verify()?;
        launch::check_preparation(deadline, cancelled)?;
        Ok(TestRelease {
            _held: &self._held,
            observed: native::release::Observed {
                target,
                scope: scope(self.binding),
                slots,
                pins,
                clean,
            },
        })
    }
}

impl TestRelease<'_> {
    pub(super) fn observed(&self) -> &native::release::Observed {
        &self.observed
    }

    pub(crate) fn commit(
        &self,
        store: &crate::store::vm::VmStore,
        stop: Option<&native::release::StopReceipt>,
        clock: impl FnOnce() -> crate::vm::ClockReading,
    ) -> anyhow::Result<Option<crate::vm::Status>> {
        store.transact(|db| native::release::commit(db, &self.observed, stop, clock))
    }
}

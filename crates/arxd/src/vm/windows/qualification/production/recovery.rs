//! Selection-independent custody for the ordinary TEST recovery sweep.
use super::*;
use crate::store::vm::{
    VmStore,
    native::recovery::{self, Origin},
};
use crate::vm::{
    ClockReading,
    reaper::{ArtifactCustody, CleanupKind, CleanupWork},
};

enum RecoveryOwner {
    TokenUser(Vec<usize>),
    #[cfg(test)]
    Fixture(std::sync::Arc<LocalSid>),
}

impl RecoveryOwner {
    fn current() -> Result<Self, NativeRefusal> {
        current_user()
            .map(Self::TokenUser)
            .map_err(|_| NativeRefusal::PathCustodyUnverified)
    }

    fn sid(&self) -> PSID {
        match self {
            Self::TokenUser(user) => user_sid(user),
            #[cfg(test)]
            Self::Fixture(owner) => owner.0,
        }
    }
}

pub(super) struct StorePins {
    database: PathPin,
    lock: PathPin,
}

impl StorePins {
    pub(super) fn open(production: &PathPin) -> Result<Self, NativeRefusal> {
        let owner = RecoveryOwner::current()?;
        Self::open_for_owner(production, owner.sid())
    }

    #[cfg(test)]
    pub(super) fn open_with_owner(
        production: &PathPin,
        owner: &LocalSid,
    ) -> Result<Self, NativeRefusal> {
        Self::open_for_owner(production, owner.0)
    }

    fn open_for_owner(production: &PathPin, owner: PSID) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        inventory::binding(production)?;
        let pin = |name| {
            let path = production.path.join(name).map_err(|_| reason)?;
            let pin = PathPin::open(&path, false, true)?;
            if !has_exact_leaf(&pin.leaf, name)
                || !pin._ancestors.last().is_some_and(|parent| {
                    file_identity(parent, true).is_ok_and(|(id, _)| id == production.identity)
                })
            {
                return Err(reason);
            }
            if !sink::held_private(&pin.leaf, owner) {
                return Err(reason);
            }
            Ok(pin)
        };
        Ok(Self {
            database: pin("activity.sqlite3")?,
            lock: pin("activity.lock")?,
        })
    }

    pub(super) fn origin(&self, store: &VmStore, boot: Hash) -> Result<Origin, NativeRefusal> {
        super::super::test::read_guarded_host(store, &self.database, &self.lock)
            .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
        Ok(Origin {
            boot,
            database: self.database.identity,
            lock: self.lock.identity,
        })
    }
}

/// Non-launch authority. The root must be the supervisor's held production
/// state root. No dossier, recipe selection, or capability to run a process.
pub(crate) struct NativeRecovery<'a> {
    store: &'a VmStore,
    production: PathPin,
    files: StorePins,
    owner: RecoveryOwner,
    guards: &'a GenerationGuards,
}

impl<'a> NativeRecovery<'a> {
    pub(crate) fn open(
        store: &'a VmStore,
        root: &std::path::Path,
        guards: &'a GenerationGuards,
    ) -> Result<Self, NativeRefusal> {
        Self::open_for_owner(store, root, RecoveryOwner::current()?, guards)
    }

    #[cfg(test)]
    pub(super) fn open_with_owner(
        store: &'a VmStore,
        root: &std::path::Path,
        owner: &std::sync::Arc<LocalSid>,
        guards: &'a GenerationGuards,
    ) -> Result<Self, NativeRefusal> {
        Self::open_for_owner(store, root, RecoveryOwner::Fixture(owner.clone()), guards)
    }

    fn open_for_owner(
        store: &'a VmStore,
        root: &std::path::Path,
        owner: RecoveryOwner,
        guards: &'a GenerationGuards,
    ) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        let root = local(root.to_str().ok_or(reason)?)?;
        let production = PathPin::open(&root, true, false)?;
        if root.is_drive_root() || !sink::held_private(&production.leaf, owner.sid()) {
            return Err(reason);
        }
        let files = StorePins::open_for_owner(&production, owner.sid())?;
        files.origin(store, boot::marker()?)?;
        Ok(Self {
            store,
            production,
            files,
            owner,
            guards,
        })
    }

    fn recover(
        &self,
        work: &CleanupWork,
        deadline: Instant,
        clock: &dyn Fn() -> ClockReading,
    ) -> anyhow::Result<()> {
        let path = || anyhow::anyhow!("unconfirmed TEST restart custody");
        anyhow::ensure!(Instant::now() < deadline, "TEST release expired");
        let origin = self
            .files
            .origin(self.store, boot::marker().map_err(|_| path())?)
            .map_err(|_| path())?;
        let expected = self
            .store
            .try_native_transaction(|db| recovery::orphan(db, work.kind, origin))?
            .ok_or_else(path)?;
        let _claim = self
            .guards
            .acquire(expected.target.generation)
            .map_err(|_| path())?;
        let owner = self.owner.sid();
        let workspace = hold_workspace_under(
            &self.production,
            owner,
            disposal::binding_from_scope(expected.scope),
        )
        .map_err(|_| path())?;
        let slots = [expected.slots.disk, expected.slots.vars];
        let pair_pin = |index: usize| -> anyhow::Result<PathPin> {
            let slot = slots[index];
            let pin = workspace
                .pin_slot(slot, expected.identities[index], false)
                .map_err(|_| path())?;
            anyhow::ensure!(
                has_exact_leaf(&pin.leaf, &disposal::leaf_name(slot))
                    && sink::held_private(&pin.leaf, owner),
                "unconfirmed TEST slot custody"
            );
            Ok(pin)
        };
        let pins = std::sync::Arc::<[PathPin]>::from([pair_pin(0)?, pair_pin(1)?]);
        let writers = pins
            .iter()
            .map(PathPin::reopen_without_writer)
            .collect::<io::Result<Vec<_>>>()?;
        let mut proof = ReleasedGeneration::checked(
            workspace.binding(),
            ReleaseTargets::Pair {
                pair: expected.slots,
                ids: expected.identities,
            },
            WorkspaceCustody::production(&workspace.root, &workspace.lease).map_err(|_| path())?,
            (pins, writers),
        )
        .ok_or_else(path)?;
        // Without a crossed creation edge nothing ran, and the ledger settles
        // only the unchanged recorded pair, so the release may be clean.
        proof.stop_disposition = Some(if expected.created {
            StopDisposition::DirtyRelease
        } else {
            StopDisposition::CleanRelease
        });
        let observation = proof
            .observe_test_release(expected.target, deadline, &|| false)
            .map_err(|_| path())?;
        self.store.try_native_transaction(|db| {
            anyhow::ensure!(
                clock().monotonic < deadline && Instant::now() < deadline,
                "TEST release expired"
            );
            recovery::commit(
                db,
                work.kind,
                origin,
                expected,
                observation.observed(),
                clock,
            )?;
            anyhow::ensure!(
                clock().monotonic < deadline && Instant::now() < deadline,
                "TEST release expired"
            );
            Ok(())
        })
    }
}

impl ArtifactCustody for NativeRecovery<'_> {
    fn release_orphan(
        &self,
        work: &CleanupWork,
        deadline: Instant,
        clock: &dyn Fn() -> ClockReading,
    ) -> bool {
        self.recover(work, deadline, clock).is_ok()
    }

    fn reconcile(&self, work: &CleanupWork, deadline: Instant) -> bool {
        let Ok(origin) = boot::marker().and_then(|boot| self.files.origin(self.store, boot)) else {
            return false;
        };
        // Build, staging and copy rollback must prove the previous writer
        // protocol as well. Unknown, migrated work stays charged.
        let staged = matches!(
            work.kind,
            CleanupKind::Build(_) | CleanupKind::Staging { .. } | CleanupKind::Snapshot { .. }
        );
        if staged {
            let mut generations = work
                .artifacts
                .iter()
                .map(|item| item.generation)
                .collect::<Vec<_>>();
            if let CleanupKind::Snapshot { generation, .. } = work.kind {
                generations.push(generation);
            }
            generations.sort_unstable();
            generations.dedup();
            if !matches!(
                self.store.try_native_transaction(|db| {
                    for generation in generations {
                        if !recovery::staged_released(db, generation, origin)?
                            && !matches!(
                                crate::store::vm::native::disposal::generation_state(
                                    db, generation
                                )?,
                                crate::store::vm::native::disposal::GenerationState::Unreserved {
                                    has_identified_artifact: false
                                }
                            )
                        {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }),
                Ok(true)
            ) {
                return false;
            }
        }
        let ledger = LedgerDisposal { store: self.store };
        NativeDisposal::under(&self.production, self.owner.sid(), &ledger, self.guards)
            .reconcile(work, deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn production_recovery_requires_token_user_on_the_exact_store() {
        let root = crate::service::application::tests::private_temp();
        let store = VmStore::open(root.path(), 100).unwrap();
        for path in [
            root.path(),
            &root.path().join("activity.sqlite3"),
            &root.path().join("activity.lock"),
        ] {
            crate::dashboard::tests::set_owner_private(path);
        }
        let production =
            PathPin::open(&local(root.path().to_str().unwrap()).unwrap(), true, false).unwrap();
        let guards = GenerationGuards::default();
        let pins = StorePins::open(&production).unwrap();
        pins.origin(&store, boot::marker().unwrap()).unwrap();
        NativeRecovery::open(&store, root.path(), &guards).unwrap();

        // Administrators may appear in the allowed DACL and can be the
        // default TokenOwner on an elevated runner. It still cannot stand
        // in for the TokenUser that owns these particular objects.
        let administrators = Arc::new(LocalSid::parse("S-1-5-32-544").unwrap());
        assert!(matches!(
            StorePins::open_with_owner(&production, &administrators),
            Err(NativeRefusal::PathCustodyUnverified)
        ));
        assert!(matches!(
            NativeRecovery::open_with_owner(&store, root.path(), &administrators, &guards),
            Err(NativeRefusal::PathCustodyUnverified)
        ));
    }
}

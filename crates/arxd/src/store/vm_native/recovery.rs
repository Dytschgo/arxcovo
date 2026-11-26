//! Durable provenance for the checked TEST process-creation edge.
use super::{Scope, bound, owner_live, release};
use crate::store::vm::evidence_matches;
use crate::vm::reaper::CleanupKind;
use crate::vm::{ArtifactSlot, ClockReading, Id, PairSlots, paths::FileIdentity};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub(in crate::store::vm) const MIGRATE_V17: &str = "
CREATE TABLE vm_native_writer (
    generation BLOB PRIMARY KEY REFERENCES vm_native_generations(generation) ON DELETE CASCADE,
    epoch BLOB NOT NULL CHECK(length(epoch)=16),
    boot BLOB CHECK(length(boot)=32),
    database_file BLOB CHECK(length(database_file)=16),
    lock_file BLOB CHECK(length(lock_file)=16 AND lock_file<>database_file),
    protocol INTEGER NOT NULL CHECK(protocol=1),
    CHECK((boot IS NULL)=(database_file IS NULL) AND (boot IS NULL)=(lock_file IS NULL))
) STRICT, WITHOUT ROWID;
-- Only newly reserved work can prove no checked creation edge was crossed.
CREATE TRIGGER vm_native_writer_reservation AFTER INSERT ON vm_native_generations
    BEGIN INSERT INTO vm_native_writer(generation,epoch,protocol)
          VALUES(NEW.generation,NEW.generation,1); END;
";

/// Exact physical store files and independently observed kernel boot.
#[derive(Clone, Copy)]
pub(crate) struct Origin {
    pub boot: [u8; 32],
    pub database: FileIdentity,
    pub lock: FileIdentity,
}

impl Origin {
    fn matches_root(self, production: FileIdentity) -> bool {
        self.boot != [0; 32]
            && self.database.volume == production.volume
            && self.lock.volume == production.volume
            && self.database.file != self.lock.file
    }
}

/// Only the checked creation edge can record protocol 1. The workspace is
/// still leased and the old epoch cannot be replaced by the same work.
pub(crate) fn record_job(db: &Connection, scope: Scope, epoch: Id, origin: Origin) -> Result<()> {
    ensure!(
        origin.matches_root(scope.production) && bound(db, scope)?,
        "unbound TEST writer scope"
    );
    let active = super::count(
        db,
        "SELECT count(*) FROM vm_generations g JOIN vm_instances v ON v.id=g.vm_id
         JOIN vm_snapshot_release r ON r.generation=g.id
         JOIN vm_operations o ON o.vm=v.id AND o.operation_id=r.epoch
         WHERE g.id=?1 AND g.active=1 AND g.static_id IS NULL AND v.state IN (1,2)
           AND r.epoch=?2 AND r.disposition IS NULL AND o.action=3 AND o.status=0
           AND o.binding_kind='vm' AND o.subject_id=v.id",
        params![scope.generation, epoch],
    )? == 1;
    ensure!(
        active || (epoch == scope.generation && owner_live(db, scope.generation)?),
        "TEST writer epoch is no longer owned"
    );
    ensure!(
        db.execute(
            "INSERT INTO vm_native_writer VALUES(?1,?2,?3,?4,?5,1)
             ON CONFLICT(generation) DO UPDATE
             SET epoch=excluded.epoch,boot=excluded.boot,database_file=excluded.database_file,
                 lock_file=excluded.lock_file
             WHERE vm_native_writer.epoch<>excluded.epoch OR vm_native_writer.boot IS NULL
                OR (vm_native_writer.boot=excluded.boot
                    AND vm_native_writer.database_file=excluded.database_file
                    AND vm_native_writer.lock_file=excluded.lock_file
                    AND vm_native_writer.protocol=1)",
            params![
                scope.generation,
                epoch,
                origin.boot,
                origin.database.file,
                origin.lock.file
            ],
        )? == 1,
        "conflicting TEST writer origin"
    );
    Ok(())
}

/// Start admission installs its operation as the exact writer epoch in the
/// same transaction that invalidates the released pair. The null origin
/// records that no checked creation edge was crossed for this epoch yet;
/// only [`record_job`] at that edge can set it.
pub(crate) fn admit_epoch(db: &Connection, scope: Scope, epoch: Id) -> Result<()> {
    ensure!(bound(db, scope)?, "unbound TEST writer scope");
    ensure!(
        super::count(
            db,
            "SELECT count(*) FROM vm_snapshot_release
             WHERE generation=?1 AND epoch=?2 AND disposition IS NULL",
            params![scope.generation, epoch],
        )? == 1,
        "TEST writer epoch is not the unresolved release"
    );
    db.execute(
        "INSERT INTO vm_native_writer(generation,epoch,protocol) VALUES(?1,?2,1)
         ON CONFLICT(generation) DO UPDATE
         SET epoch=excluded.epoch,boot=NULL,database_file=NULL,lock_file=NULL",
        params![scope.generation, epoch],
    )?;
    Ok(())
}

pub(crate) fn requires_release(db: &Connection, kind: CleanupKind) -> Result<bool> {
    let (vm, activity) = match kind {
        CleanupKind::Recovery { vm, activity_id } | CleanupKind::Expiry { vm, activity_id } => {
            (vm, activity_id)
        }
        _ => return Ok(false),
    };
    Ok(super::count(
        db,
        "SELECT count(*) FROM vm_instances v JOIN vm_generations g ON g.vm_id=v.id
         JOIN vm_native_generations n ON n.generation=g.id
         LEFT JOIN vm_snapshot_release r ON r.generation=g.id
         WHERE v.id=?1 AND v.cleanup_id=?2 AND g.active=1 AND g.static_id IS NULL
           AND r.disposition IS NULL",
        params![vm, activity],
    )? != 0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Orphan {
    pub target: release::Target,
    pub scope: Scope,
    pub slots: PairSlots,
    pub identities: [FileIdentity; 2],
    /// Whether the exact epoch crossed its checked creation edge.
    pub created: bool,
}

pub(crate) fn orphan(db: &Connection, kind: CleanupKind, origin: Origin) -> Result<Option<Orphan>> {
    use super::disposal::{GenerationDisposal, GenerationState, generation_state};
    let (vm, activity_id, state, route) = match kind {
        CleanupKind::Recovery { vm, activity_id } => (vm, activity_id, 3, None),
        CleanupKind::Expiry { vm, activity_id } => (vm, activity_id, 4, Some("lease")),
        _ => return Ok(None),
    };
    // An epoch that crossed its creation edge needs its exact Job origin.
    // One that never crossed it has a null origin and no process to prove.
    let Some((generation, epoch, created)) = db
        .query_row(
            "SELECT g.id,r.epoch,w.boot IS NOT NULL FROM vm_instances v
         JOIN vm_generations g ON g.vm_id=v.id
         JOIN vm_snapshot_release r ON r.generation=g.id
         JOIN vm_native_writer w ON w.generation=g.id AND w.epoch=r.epoch
         WHERE v.id=?1 AND v.cleanup_id=?2 AND v.state=?3 AND v.cleanup_route IS ?4
           AND g.active=1 AND g.static_id IS NULL AND r.disposition IS NULL AND w.protocol=1
           AND (w.boot IS NULL OR (w.boot=?5 AND w.database_file=?6 AND w.lock_file=?7))",
            params![
                vm,
                activity_id,
                state,
                route,
                origin.boot,
                origin.database.file,
                origin.lock.file
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let GenerationState::Bound {
        scope,
        disposal: GenerationDisposal::None,
    } = generation_state(db, generation)?
    else {
        return Ok(None);
    };
    if !origin.matches_root(scope.production) || !bound(db, scope)? {
        return Ok(None);
    }
    let slot = |role: &'static str| -> Result<(ArtifactSlot, FileIdentity)> {
        let (key, file) = db.query_row(
            "SELECT s.object_key,s.file FROM vm_native_slots s JOIN vm_artifacts a
               ON a.generation_id=s.generation AND a.role=s.role AND a.object_key=s.object_key
             WHERE s.generation=?1 AND s.role=?2 AND a.state='active'
               AND s.disposal IS NULL AND a.file_id=s.file AND a.directory_id=?3 AND a.volume_id=?4",
            params![generation, role, scope.workspace.file, super::volume(scope.production)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((
            ArtifactSlot {
                generation,
                role,
                key,
            },
            FileIdentity {
                volume: scope.production.volume,
                file,
            },
        ))
    };
    let (disk, disk_id) = slot("disk")?;
    let (vars, vars_id) = slot("vars")?;
    ensure!(disk_id != vars_id, "TEST orphan roles alias");
    Ok(Some(Orphan {
        target: release::Target {
            vm,
            generation,
            epoch,
        },
        scope,
        slots: PairSlots { disk, vars },
        identities: [disk_id, vars_id],
        created,
    }))
}

pub(crate) fn commit(
    db: &Transaction<'_>,
    kind: CleanupKind,
    origin: Origin,
    expected: Orphan,
    observed: &release::Observed,
    clock: impl FnOnce() -> ClockReading,
) -> Result<()> {
    ensure!(
        orphan(db, kind, origin)? == Some(expected)
            && observed.target == expected.target
            && observed.scope == expected.scope
            && observed.slots == expected.slots
            && observed.pins.map(|pin| pin.identity) == expected.identities
            // After the creation edge only a dirty release is exact. Before
            // it nothing ran, so only the unchanged recorded pair settles.
            && if expected.created {
                !observed.clean
            } else {
                unchanged(db, observed)?
            },
        "stale TEST orphan release"
    );
    release::commit(db, observed, None, clock)?;
    Ok(())
}

fn unchanged(db: &Connection, observed: &release::Observed) -> Result<bool> {
    for (slot, pin) in [observed.slots.disk, observed.slots.vars]
        .into_iter()
        .zip(observed.pins)
    {
        if !evidence_matches(db, slot, pin, "active")? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A staged generation is safe to hand to checked disposal only if its
/// exact old writer was covered by the protocol, or no process could have
/// crossed a checked create edge. The runtime still needs worker quiescence
/// and disposal still opens the exact workspace and writer-excluded leaves.
pub(crate) fn staged_released(db: &Connection, generation: Id, origin: Origin) -> Result<bool> {
    Ok(super::count(
        db,
        "SELECT count(*) FROM vm_native_generations n JOIN vm_generations g ON g.id=n.generation
         LEFT JOIN vm_native_writer w ON w.generation=g.id
         LEFT JOIN vm_snapshot_release r ON r.generation=g.id
         WHERE g.id=?1 AND g.active=0 AND g.static_id IS NULL AND n.protocol=1
           AND n.host=(SELECT id FROM vm_state_host WHERE singleton=1)
           AND (r.disposition IS NOT NULL
               OR (w.protocol=1 AND w.epoch=g.id AND
                   (w.boot IS NULL OR (w.boot=?2 AND w.database_file=?3 AND w.lock_file=?4))))",
        params![
            generation,
            origin.boot,
            origin.database.file,
            origin.lock.file
        ],
    )? == 1)
}

//! Native TEST custody checkpoints (store v16, ADR 0072).
//!
//! A generation row binds the exact store host, production root, workspace
//! directory and lease identities with the origin protocol before any
//! dispatch. A slot row records create intent before each relative
//! create-new, then the object's identity before any write. Rows exist only
//! for generations the native protocol reserved after this migration. The
//! migration infers no binding for existing rows.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "consumed by the 7c native TEST issuer")
)]
use super::count;
use crate::vm::{ArtifactEvidence, ArtifactSlot, Id, paths::FileIdentity};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};

/// The largest native leaf, the 41 GiB disk ceiling.
const MAX_LEAF_BYTES: u64 = 41 << 30;

/// Additive and forward-only. Identities are immutable once recorded, and a
/// generation's disposal cannot advance past a slot that is still present.
pub(super) const MIGRATE_V16: &str = "
CREATE UNIQUE INDEX vm_artifact_slot ON vm_artifacts(generation_id,role,object_key);

-- 'directory' records the directory identity and the lease create intent.
-- 'absent' is an intent disproved through the held production root.
-- 'quarantined' is a present object without a recorded identity.
CREATE TABLE vm_native_generations (
    generation BLOB PRIMARY KEY REFERENCES vm_generations(id),
    host BLOB NOT NULL REFERENCES vm_state_host(id),
    origin TEXT NOT NULL CHECK(origin IN ('build','clone','restore')),
    protocol INTEGER NOT NULL CHECK(protocol=1),
    volume BLOB NOT NULL CHECK(length(volume)=8),
    production BLOB NOT NULL CHECK(length(production)=16),
    state TEXT NOT NULL CHECK(state IN ('intended','absent','directory','bound','quarantined')),
    directory BLOB CHECK(length(directory)=16 AND directory<>production),
    lease BLOB CHECK(length(lease)=16 AND lease<>production),
    disposal TEXT CHECK(disposal IN ('lease','directory','absent')),
    CHECK(state NOT IN ('intended','absent') OR directory IS NULL),
    CHECK((lease IS NOT NULL)=(state='bound')),
    CHECK(state NOT IN ('directory','bound') OR directory IS NOT NULL),
    CHECK(lease IS NULL OR lease<>directory),
    CHECK(disposal IS NULL OR state='bound')
) STRICT, WITHOUT ROWID;

CREATE TABLE vm_native_slots (
    generation BLOB NOT NULL REFERENCES vm_native_generations(generation),
    role TEXT NOT NULL CHECK(role IN ('disk','vars','seed','verification_overlay')),
    object_key BLOB NOT NULL UNIQUE CHECK(length(object_key)=16),
    state TEXT NOT NULL CHECK(state IN ('intended','absent','quarantined','identified','verified')),
    file BLOB CHECK(length(file)=16),
    bytes INTEGER CHECK(bytes BETWEEN 1 AND 44023414784),
    content_digest BLOB CHECK(length(content_digest)=32),
    disposal TEXT CHECK(disposal IN ('intended','absent')),
    CHECK((file IS NOT NULL)=(state IN ('identified','verified'))),
    CHECK((bytes IS NOT NULL)=(state='verified')),
    CHECK((content_digest IS NOT NULL)=(state='verified')),
    CHECK(disposal IS NULL OR file IS NOT NULL),
    PRIMARY KEY(generation,role),
    UNIQUE(generation,file),
    FOREIGN KEY(generation,role,object_key) REFERENCES vm_artifacts(generation_id,role,object_key)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER vm_native_origin BEFORE INSERT ON vm_native_generations
    WHEN NOT EXISTS(SELECT 1 FROM vm_generations g WHERE g.id=NEW.generation
        AND g.active=0 AND g.static_id IS NULL
        AND (NEW.origin='build')=(g.build_id IS NOT NULL))
    BEGIN SELECT RAISE(ABORT,'invalid native generation origin'); END;
CREATE TRIGGER vm_native_generation_scope BEFORE UPDATE ON vm_native_generations
    WHEN NEW.generation IS NOT OLD.generation OR NEW.host IS NOT OLD.host
      OR NEW.origin IS NOT OLD.origin OR NEW.protocol IS NOT OLD.protocol
      OR NEW.volume IS NOT OLD.volume OR NEW.production IS NOT OLD.production
      OR (OLD.directory IS NOT NULL AND NEW.directory IS NOT OLD.directory)
      OR (OLD.lease IS NOT NULL AND NEW.lease IS NOT OLD.lease)
    BEGIN SELECT RAISE(ABORT,'immutable native generation scope'); END;
CREATE TRIGGER vm_native_slot_scope BEFORE INSERT ON vm_native_slots
    WHEN NOT EXISTS(SELECT 1 FROM vm_native_generations
        WHERE generation=NEW.generation AND state='bound' AND disposal IS NULL)
    BEGIN SELECT RAISE(ABORT,'unbound native generation'); END;
CREATE TRIGGER vm_native_slot_identity BEFORE UPDATE ON vm_native_slots
    WHEN NEW.generation IS NOT OLD.generation OR NEW.role IS NOT OLD.role
      OR NEW.object_key IS NOT OLD.object_key
      OR (OLD.file IS NOT NULL AND NEW.file IS NOT OLD.file)
      OR (OLD.content_digest IS NOT NULL AND NEW.content_digest IS NOT OLD.content_digest)
    BEGIN SELECT RAISE(ABORT,'immutable native slot identity'); END;
CREATE TRIGGER vm_native_disposal_order BEFORE UPDATE OF disposal ON vm_native_generations
    WHEN NEW.disposal IS NOT NULL AND EXISTS(SELECT 1 FROM vm_native_slots
        WHERE generation=NEW.generation AND state<>'absent' AND disposal IS NOT 'absent')
    BEGIN SELECT RAISE(ABORT,'native slots remain'); END;
";

/// The operation protocol that reserved a native generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Origin {
    Build,
    Clone,
    Restore,
}

impl Origin {
    fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Clone => "clone",
            Self::Restore => "restore",
        }
    }
}

/// The exact durable workspace scope, mirroring the held workspace binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Scope {
    pub generation: Id,
    pub production: FileIdentity,
    pub workspace: FileIdentity,
    pub lease: FileIdentity,
}

/// One slot's durable custody. `Intended` and `Absent` share the second
/// class, `Identified` and `Verified` the fourth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Leaf {
    NeverAttempted,
    Intended,
    Absent,
    Unidentified,
    Identified(Id),
    Verified(Id, crate::vm::paths::ArtifactPin),
}

/// The originating work still owns the generation. A stale worker after an
/// abandonment or restart cannot add custody.
fn owner_live(db: &Connection, generation: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_native_generations n JOIN vm_generations g ON g.id=n.generation
         WHERE n.generation=?1 AND g.active=0 AND CASE n.origin
            WHEN 'build' THEN EXISTS(SELECT 1 FROM vm_build_intents b
                WHERE b.id=g.build_id AND b.state='pending')
            WHEN 'clone' THEN EXISTS(SELECT 1 FROM vm_instances v WHERE v.id=g.vm_id AND v.state=0)
            ELSE EXISTS(SELECT 1 FROM vm_snapshot_work w
                WHERE w.candidate=g.id AND w.vm=g.vm_id AND w.phase='copy') END",
        [generation],
    )? == 1)
}

fn volume(identity: FileIdentity) -> [u8; 8] {
    identity.volume.to_be_bytes()
}

/// Durable intent before the generation directory's relative create-new.
/// A replay with the identical scope is accepted. An `absent` intent may
/// be renewed.
pub(crate) fn intend_workspace(
    db: &Connection,
    generation: Id,
    origin: Origin,
    production: FileIdentity,
) -> Result<()> {
    let renewed = db.execute(
        "UPDATE vm_native_generations SET state='intended'
         WHERE generation=?1 AND origin=?2 AND volume=?3 AND production=?4
           AND state IN ('intended','absent')",
        params![
            generation,
            origin.name(),
            volume(production),
            production.file
        ],
    )?;
    if renewed == 0 {
        // Only an untouched generation can be bound. Legacy and attempted
        // rows never gain native custody.
        ensure!(
            count(
                db,
                "SELECT count(*)=sum(state='planned') AND count(*)>0 FROM vm_artifacts
                 WHERE generation_id=?1",
                [generation],
            )? == 1,
            "native custody requires an untouched generation"
        );
        db.execute(
            "INSERT INTO vm_native_generations(generation,host,origin,protocol,volume,production,state)
             SELECT ?1,id,?2,1,?3,?4,'intended' FROM vm_state_host WHERE singleton=1",
            params![generation, origin.name(), volume(production), production.file],
        )?;
    }
    ensure!(owner_live(db, generation)?, "native generation owner ended");
    Ok(())
}

/// Identity of the created directory, before the lease create-new.
pub(crate) fn identify_directory(
    db: &Connection,
    generation: Id,
    directory: FileIdentity,
) -> Result<()> {
    ensure!(owner_live(db, generation)?, "native generation owner ended");
    ensure!(
        db.execute(
            "UPDATE vm_native_generations SET state='directory',directory=?3
             WHERE generation=?1 AND volume=?2
               AND (state='intended' OR (state='directory' AND directory=?3))",
            params![generation, volume(directory), directory.file],
        )? == 1,
        "invalid native directory identity"
    );
    Ok(())
}

/// Binds the exact scope once the synced lease is identified.
pub(crate) fn bind_workspace(db: &Connection, scope: Scope) -> Result<()> {
    ensure!(
        owner_live(db, scope.generation)?
            && scope.workspace.volume == scope.production.volume
            && scope.lease.volume == scope.production.volume,
        "invalid native workspace binding"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_generations SET state='bound',lease=?5
             WHERE generation=?1 AND volume=?2 AND production=?3 AND directory=?4
               AND (state='directory' OR (state='bound' AND lease=?5))",
            params![
                scope.generation,
                volume(scope.production),
                scope.production.file,
                scope.workspace.file,
                scope.lease.file
            ],
        )? == 1,
        "invalid native workspace binding"
    );
    Ok(())
}

/// The exact durable scope a dispatch or hold requires.
pub(crate) fn bound(db: &Connection, scope: Scope) -> Result<bool> {
    Ok(scope.workspace.volume == scope.production.volume
        && scope.lease.volume == scope.production.volume
        && count(
            db,
            "SELECT count(*) FROM vm_native_generations
             WHERE generation=?1 AND state='bound' AND disposal IS NULL AND protocol=1
               AND host=(SELECT id FROM vm_state_host WHERE singleton=1)
               AND volume=?2 AND production=?3 AND directory=?4 AND lease=?5",
            params![
                scope.generation,
                volume(scope.production),
                scope.production.file,
                scope.workspace.file,
                scope.lease.file
            ],
        )? == 1)
}

/// Classifies an unbound workspace intent from an exact held observation:
/// the directory (state `intended`) or the lease (state `directory`).
/// Absence of a directory disproves the intent. A present unidentified
/// object is quarantined. Absence of a lease leaves its intent in place.
pub(crate) fn settle_workspace_intent(
    db: &Connection,
    generation: Id,
    present: bool,
) -> Result<()> {
    let state: Option<String> = db
        .query_row(
            "SELECT state FROM vm_native_generations WHERE generation=?1",
            [generation],
            |row| row.get(0),
        )
        .optional()?;
    let next = match (state.as_deref(), present) {
        (Some("intended"), false) => "absent",
        (Some("intended" | "directory"), true) => "quarantined",
        (Some("directory"), false) => "directory",
        _ => anyhow::bail!("no unsettled native workspace intent"),
    };
    db.execute(
        "UPDATE vm_native_generations SET state=?2 WHERE generation=?1",
        params![generation, next],
    )?;
    Ok(())
}

fn require_scope(db: &Connection, scope: Scope, slot: ArtifactSlot) -> Result<()> {
    ensure!(
        slot.generation == scope.generation
            && bound(db, scope)?
            && owner_live(db, scope.generation)?,
        "native slot is outside its bound generation"
    );
    Ok(())
}

/// Durable intent before one slot's relative create-new. The artifact row
/// moves from `planned` to `created`, keeping the existing attempt
/// semantics for recovery. Only an `absent` intent may be renewed.
pub(crate) fn intend_slot(db: &Connection, scope: Scope, slot: ArtifactSlot) -> Result<()> {
    require_scope(db, scope, slot)?;
    let renewed = db.execute(
        "UPDATE vm_native_slots SET state='intended'
         WHERE generation=?1 AND role=?2 AND object_key=?3 AND state IN ('intended','absent')",
        params![slot.generation, slot.role, slot.key],
    )?;
    if renewed == 0 {
        ensure!(
            db.execute(
                "UPDATE vm_artifacts SET state='created'
                 WHERE generation_id=?1 AND role=?2 AND object_key=?3
                   AND state='planned' AND file_id IS NULL",
                params![slot.generation, slot.role, slot.key],
            )? == 1,
            "native slot already attempted"
        );
        db.execute(
            "INSERT INTO vm_native_slots(generation,role,object_key,state) VALUES(?1,?2,?3,'intended')",
            params![slot.generation, slot.role, slot.key],
        )?;
    }
    Ok(())
}

/// The created object's identity, before any write or writable child.
pub(crate) fn identify_slot(
    db: &Connection,
    scope: Scope,
    slot: ArtifactSlot,
    identity: FileIdentity,
) -> Result<()> {
    require_scope(db, scope, slot)?;
    ensure!(
        identity.volume == scope.production.volume
            && ![
                scope.production.file,
                scope.workspace.file,
                scope.lease.file
            ]
            .contains(&identity.file),
        "invalid native slot identity"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_slots SET state='identified',file=?4
             WHERE generation=?1 AND role=?2 AND object_key=?3
               AND (state='intended' OR (state IN ('identified','verified') AND file=?4))",
            params![slot.generation, slot.role, slot.key, identity.file],
        )? == 1,
        "invalid native slot identity"
    );
    ensure!(
        db.execute(
            "UPDATE vm_artifacts SET volume_id=?4,directory_id=?5,file_id=?6
             WHERE generation_id=?1 AND role=?2 AND object_key=?3 AND state='created'
               AND (file_id IS NULL OR (volume_id=?4 AND directory_id=?5 AND file_id=?6))",
            params![
                slot.generation,
                slot.role,
                slot.key,
                volume(identity),
                scope.workspace.file,
                identity.file
            ],
        )? == 1,
        "invalid native slot identity"
    );
    Ok(())
}

/// Content verified through the identified object after write and sync.
pub(crate) fn verify_slot(
    db: &Connection,
    scope: Scope,
    slot: ArtifactSlot,
    evidence: ArtifactEvidence,
) -> Result<()> {
    require_scope(db, scope, slot)?;
    let bytes = evidence.pin.size;
    ensure!(
        evidence.directory == scope.workspace.file
            && evidence.identity.volume == scope.production.volume
            && (1..=MAX_LEAF_BYTES).contains(&bytes),
        "invalid native slot evidence"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_slots SET state='verified',bytes=?5,content_digest=?6
             WHERE generation=?1 AND role=?2 AND object_key=?3 AND file=?4
               AND (state='identified' OR (state='verified' AND bytes=?5 AND content_digest=?6))",
            params![
                slot.generation,
                slot.role,
                slot.key,
                evidence.identity.file,
                bytes,
                evidence.pin.sha256
            ],
        )? == 1,
        "invalid native slot evidence"
    );
    Ok(())
}

/// Classifies an intended slot from an exact observation through the held
/// workspace directory. Absence disproves the intent. A present object
/// without a recorded identity is quarantined and never adopted.
pub(crate) fn settle_slot_intent(
    db: &Connection,
    scope: Scope,
    slot: ArtifactSlot,
    present: bool,
) -> Result<()> {
    ensure!(
        slot.generation == scope.generation && bound(db, scope)?,
        "native slot is outside its bound generation"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_slots SET state=?4
             WHERE generation=?1 AND role=?2 AND object_key=?3 AND state='intended'",
            params![
                slot.generation,
                slot.role,
                slot.key,
                if present { "quarantined" } else { "absent" }
            ],
        )? == 1,
        "no unsettled native slot intent"
    );
    Ok(())
}

pub(crate) fn leaf(db: &Connection, slot: ArtifactSlot) -> Result<Leaf> {
    let row = db
        .query_row(
            "SELECT state,file,bytes,content_digest FROM vm_native_slots
             WHERE generation=?1 AND role=?2 AND object_key=?3",
            params![slot.generation, slot.role, slot.key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<Id>>(1)?,
                    row.get::<_, Option<u64>>(2)?,
                    row.get::<_, Option<[u8; 32]>>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(match row {
        None => Leaf::NeverAttempted,
        Some((state, file, bytes, digest)) => match (state.as_str(), file, bytes, digest) {
            ("intended", None, None, None) => Leaf::Intended,
            ("absent", None, None, None) => Leaf::Absent,
            ("quarantined", None, None, None) => Leaf::Unidentified,
            ("identified", Some(file), None, None) => Leaf::Identified(file),
            ("verified", Some(file), Some(size), Some(sha256)) => {
                Leaf::Verified(file, crate::vm::paths::ArtifactPin { size, sha256 })
            }
            _ => anyhow::bail!("invalid native slot row"),
        },
    })
}

#[cfg(test)]
#[path = "vm_native_tests.rs"]
pub(crate) mod tests;

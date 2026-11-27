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
#[path = "vm_native/disposal.rs"]
pub(crate) mod disposal;
#[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
#[path = "vm_native/recovery.rs"]
pub(crate) mod recovery;
#[cfg_attr(all(test, not(all(windows, target_arch = "x86_64"))), allow(dead_code))]
#[path = "vm_native/release.rs"]
pub(crate) mod release;

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
-- Legacy writers keep the artifact row's attempt semantics, but once a slot
-- has native custody its artifact identity can only mirror that custody.
CREATE TRIGGER vm_native_artifact_identity BEFORE UPDATE ON vm_artifacts
    WHEN EXISTS(SELECT 1 FROM vm_native_slots WHERE object_key=OLD.object_key)
      AND (NEW.volume_id IS NOT NULL OR NEW.directory_id IS NOT NULL OR NEW.file_id IS NOT NULL)
      AND NOT EXISTS(SELECT 1 FROM vm_native_slots s
          JOIN vm_native_generations n ON n.generation=s.generation
          WHERE s.object_key=OLD.object_key AND s.object_key=NEW.object_key
            AND s.generation=NEW.generation_id AND s.role=NEW.role AND s.file IS NOT NULL
            AND NEW.file_id=s.file AND NEW.directory_id=n.directory AND NEW.volume_id=n.volume)
    BEGIN SELECT RAISE(ABORT,'native slot identity is fenced'); END;
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

/// The workspace object an unbound intent observation names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceLeaf {
    Directory,
    Lease,
}

/// An exact no-follow observation through the held parent. A present object
/// carries its identity only when it opened as the expected kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Presence {
    Absent,
    Present(Option<FileIdentity>),
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
/// abandonment or restart cannot add custody. The `restore` origin names the
/// offline snapshot copy protocol: a TEST capture (action 5) or restore
/// (action 6) candidate, owned only while its copy-phase work row and exact
/// pending snapshot receipt stand (7c 15).
fn owner_live(db: &Connection, generation: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_native_generations n JOIN vm_generations g ON g.id=n.generation
         WHERE n.generation=?1 AND g.active=0 AND CASE n.origin
            WHEN 'build' THEN EXISTS(SELECT 1 FROM vm_build_intents b
                JOIN vm_operations o ON o.operation_id=b.operation_id
                WHERE b.id=g.build_id AND b.state='pending' AND o.action=11 AND o.status=0)
            WHEN 'clone' THEN EXISTS(SELECT 1 FROM vm_instances v WHERE v.id=g.vm_id AND v.state=0)
            ELSE EXISTS(SELECT 1 FROM vm_snapshot_work w
                JOIN vm_operations o ON o.activity_id=w.activity
                WHERE w.candidate=g.id AND w.vm=g.vm_id AND w.action IN (5,6) AND w.phase='copy'
                  AND o.vm=g.vm_id AND o.action=w.action AND o.status=0
                  AND o.binding_kind='snapshot') END",
        [generation],
    )? == 1)
}

fn volume(identity: FileIdentity) -> [u8; 8] {
    identity.volume.to_be_bytes()
}

/// Durable intent before the generation directory's relative create-new.
/// A replay with the identical scope is accepted and an `absent` intent may
/// be renewed. A committed directory checkpoint resumes: the caller gets the
/// recorded directory identity and continues at the lease intent.
pub(crate) fn intend_workspace(
    db: &Connection,
    generation: Id,
    origin: Origin,
    production: FileIdentity,
) -> Result<Option<FileIdentity>> {
    let existing: Option<(String, Option<Id>)> = db
        .query_row(
            "SELECT state,directory FROM vm_native_generations
             WHERE generation=?1 AND origin=?2 AND volume=?3 AND production=?4",
            params![
                generation,
                origin.name(),
                volume(production),
                production.file
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let resume = match existing {
        Some((state, None)) if matches!(state.as_str(), "intended" | "absent") => {
            db.execute(
                "UPDATE vm_native_generations SET state='intended' WHERE generation=?1",
                [generation],
            )?;
            None
        }
        Some((state, Some(file))) if state == "directory" => Some(FileIdentity {
            volume: production.volume,
            file,
        }),
        Some(_) => anyhow::bail!("native workspace intent already settled"),
        None => {
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
            None
        }
    };
    ensure!(owner_live(db, generation)?, "native generation owner ended");
    Ok(resume)
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

/// Classifies an unbound workspace intent from an exact observation of the
/// named object through the held parent. Absence of an intended directory
/// disproves the intent. The recorded directory, observed with its identity,
/// keeps the `directory` checkpoint, so a lost acknowledgement resumes. A
/// missing or replaced recorded directory, or any present lease without a
/// recorded identity, is quarantined. Absence of a lease keeps its intent.
pub(crate) fn settle_workspace_intent(
    db: &Connection,
    generation: Id,
    leaf: WorkspaceLeaf,
    presence: Presence,
) -> Result<()> {
    let row: Option<(String, [u8; 8], Option<Id>)> = db
        .query_row(
            "SELECT state,volume,directory FROM vm_native_generations WHERE generation=?1",
            [generation],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((state, volume_id, directory)) = row else {
        anyhow::bail!("no unsettled native workspace intent");
    };
    let recorded = directory.map(|file| FileIdentity {
        volume: u64::from_be_bytes(volume_id),
        file,
    });
    let next = match (state.as_str(), leaf, presence) {
        ("intended", WorkspaceLeaf::Directory, Presence::Absent) => "absent",
        ("intended", WorkspaceLeaf::Directory, Presence::Present(_)) => "quarantined",
        ("directory", WorkspaceLeaf::Directory, Presence::Present(Some(id)))
            if Some(id) == recorded =>
        {
            "directory"
        }
        ("directory", WorkspaceLeaf::Directory, _) => "quarantined",
        ("directory", WorkspaceLeaf::Lease, Presence::Absent) => "directory",
        ("directory", WorkspaceLeaf::Lease, Presence::Present(_)) => "quarantined",
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

/// A mutable build slot stays `identified` through every writable phase. Its
/// single durable content digest is committed once, after the final writable
/// release of its generation, while the released writer exclusions are held.
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) fn verify_build_slot(
    db: &Connection,
    scope: Scope,
    slot: ArtifactSlot,
    evidence: ArtifactEvidence,
) -> Result<()> {
    require_scope(db, scope, slot)?;
    ensure!(
        matches!(slot.role, "disk" | "vars" | "verification_overlay")
            && count(db,
                "SELECT count(*) FROM vm_native_generations n JOIN vm_generations g ON g.id=n.generation
                 JOIN vm_build_intents b ON b.id=g.build_id
                 WHERE n.generation=?1 AND n.origin='build' AND g.active=0 AND b.state='pending'",
                [scope.generation],
            )? == 1,
        "invalid released build slot"
    );
    verify_slot(db, scope, slot, evidence)?;
    super::identify(db, slot, evidence)
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

/// Whether a start pair names the exact durable slots of its bound scope:
/// both slots identified or verified with the same file IDs, directory and
/// volume. Content pins are compared by the held release proof.
pub(crate) fn pair_matches(
    db: &Connection,
    scope: Scope,
    slots: [ArtifactSlot; 2],
    evidence: [ArtifactEvidence; 2],
) -> Result<bool> {
    if !bound(db, scope)?
        || !matches!(slots[0].role, "disk" | "verification_overlay")
        || slots[1].role != "vars"
        || slots[0].key == slots[1].key
    {
        return Ok(false);
    }
    for (slot, evidence) in slots.into_iter().zip(evidence) {
        let held = matches!(leaf(db, slot)?,
            Leaf::Identified(file) | Leaf::Verified(file, _) if file == evidence.identity.file);
        if slot.generation != scope.generation
            || !held
            || evidence.directory != scope.workspace.file
            || evidence.identity.volume != scope.production.volume
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether a slot of this bound scope is identified, not yet verified, as
/// exactly this object. A writable helper target must still be in this state.
pub(crate) fn identified_as(
    db: &Connection,
    scope: Scope,
    slot: ArtifactSlot,
    identity: FileIdentity,
) -> Result<bool> {
    Ok(slot.generation == scope.generation
        && identity.volume == scope.production.volume
        && bound(db, scope)?
        && leaf(db, slot)? == Leaf::Identified(identity.file))
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

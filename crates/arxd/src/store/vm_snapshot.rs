//! Private schema and settlement for offline VM snapshots.
use super::*;
use crate::vm::{
    snapshot::{MANUAL_STATIC_POINTS, POINTS_PER_HOST, POINTS_PER_VM},
    static_web::StaticState,
};

pub(super) const MIGRATE_V11: &str = "
CREATE TABLE activity_v11 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 19),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 51),
    finished_ms INTEGER CHECK(finished_ms BETWEEN 0 AND 253402300799999),
    terminal_order INTEGER UNIQUE CHECK(terminal_order>0),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    publication_failure INTEGER CHECK(publication_failure BETWEEN 1 AND 17)
        CHECK(publication_failure IS NULL OR (action IN (15,16,17) AND status IN (1,2,3))),
    CHECK(vm IS NOT NULL OR action=11 OR status=4),
    CHECK(action NOT IN (13,14) OR status=4),
    CHECK((status=0)=(finished_ms IS NULL)),
    CHECK((status=0)=(terminal_order IS NULL)),
    CHECK((status=4)=(reason IS NOT NULL)),
    CHECK((actor='daemon')=(action IN (7,9,10,17,19)) AND (actor='agent' OR principal='arxd')),
    CHECK((vm_owner IS NOT NULL)=(action IN (7,10,17,19)))
) STRICT;
INSERT INTO activity_v11(id,principal,actor,vm_owner,vm,action,started_ms,status,reason,
    finished_ms,terminal_order,cleanup_confirmed,publication_failure)
    SELECT id,principal,actor,vm_owner,vm,action,started_ms,status,reason,
        finished_ms,terminal_order,cleanup_confirmed,publication_failure FROM activity;
INSERT INTO sqlite_sequence(name,seq) SELECT 'activity_v11',seq FROM sqlite_sequence
    WHERE name='activity' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='activity_v11');
UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='activity'),0))
    WHERE name='activity_v11';
DROP TABLE activity;
ALTER TABLE activity_v11 RENAME TO activity;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND (action<=6 OR action IN (12,18));

CREATE TABLE vm_operations_v11 (
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    request_key BLOB NOT NULL CHECK(length(request_key)=16),
    operation_id BLOB NOT NULL UNIQUE DEFAULT (randomblob(16)) CHECK(length(operation_id)=16),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 6 OR action IN (8,11,12,15,16,18)),
    activity_id INTEGER NOT NULL UNIQUE CHECK(activity_id>0),
    valid_through_ms INTEGER NOT NULL CHECK(valid_through_ms>=0),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 3),
    lease_end_ms INTEGER CHECK(lease_end_ms BETWEEN 0 AND 253402300799999),
    binding_kind TEXT NOT NULL DEFAULT 'legacy_vm'
        CHECK(binding_kind IN ('legacy_vm','vm','template','recipe','snapshot')),
    subject_id BLOB CHECK(length(subject_id)=16),
    recipe_id TEXT CHECK(length(recipe_id) BETWEEN 1 AND 64
        AND length(recipe_id)=length(CAST(recipe_id AS BLOB))
        AND recipe_id NOT GLOB '*[^a-z0-9-]*'),
    recipe_digest BLOB CHECK(length(recipe_digest)=32),
    request_digest BLOB CHECK(length(request_digest)=32),
    script_result TEXT CHECK(script_result IN
        ('legacy','exited','unavailable','timed_out','launch_failed','terminated','interrupted')),
    script_exit_code INTEGER CHECK(script_exit_code BETWEEN 0 AND 4294967295),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1))
        CHECK(cleanup_confirmed IS NULL OR status BETWEEN 1 AND 3),
    failure_reason INTEGER CHECK(failure_reason BETWEEN 1 AND 17)
        CHECK(failure_reason IS NULL OR status IN (2,3)),
    snapshot_result BLOB CHECK(length(snapshot_result)=16)
        CHECK(snapshot_result IS NULL OR binding_kind='snapshot'),
    CHECK(action<>8 OR lease_end_ms IS NOT NULL),
    CHECK(lease_end_ms IS NULL OR action IN (1,8)),
    CHECK((action=11)=(vm IS NULL)),
    CHECK((action=11)=(binding_kind='recipe')),
    CHECK((binding_kind IN ('vm','template','snapshot'))=(subject_id IS NOT NULL)),
    CHECK((binding_kind='recipe')=(recipe_id IS NOT NULL)
        AND (binding_kind='recipe')=(recipe_digest IS NOT NULL)),
    CHECK(binding_kind NOT IN ('vm','template') OR (action IN (1,12))=(binding_kind='template')),
    CHECK(binding_kind NOT IN ('vm','snapshot') OR subject_id=vm),
    CHECK(binding_kind<>'snapshot' OR action IN (5,6,18)),
    CHECK(action<>18 OR binding_kind='snapshot'),
    CHECK((binding_kind IN ('template','snapshot'))=(request_digest IS NOT NULL)),
    CHECK(action=2 OR script_result IS NULL),
    CHECK(action<>2 OR status=0 OR script_result IS NOT NULL),
    CHECK(script_result IS NULL OR status<>0 OR script_result='legacy'),
    CHECK((script_result IS 'exited')=(script_exit_code IS NOT NULL)),
    PRIMARY KEY(principal,request_key)
) STRICT, WITHOUT ROWID;
INSERT INTO vm_operations_v11(principal,request_key,operation_id,vm,action,activity_id,
    valid_through_ms,status,lease_end_ms,binding_kind,subject_id,recipe_id,recipe_digest,request_digest,
    script_result,script_exit_code,cleanup_confirmed,failure_reason)
    SELECT principal,request_key,operation_id,vm,action,activity_id,valid_through_ms,status,lease_end_ms,
        binding_kind,subject_id,recipe_id,recipe_digest,request_digest,script_result,script_exit_code,
        cleanup_confirmed,failure_reason FROM vm_operations;
DROP TABLE vm_operations;
ALTER TABLE vm_operations_v11 RENAME TO vm_operations;

ALTER TABLE vm_static_instances ADD COLUMN origin_generation BLOB
    CHECK(length(origin_generation)=16);
UPDATE vm_static_instances SET origin_generation=(
    SELECT id FROM vm_generations WHERE static_id=vm_static_instances.id AND active=1
);
CREATE TRIGGER vm_static_origin_on_activate AFTER UPDATE OF active ON vm_generations
    WHEN NEW.static_id IS NOT NULL AND NEW.active=1
    BEGIN UPDATE vm_static_instances SET origin_generation=NEW.id
        WHERE id=NEW.static_id AND origin_generation IS NULL; END;
CREATE TRIGGER vm_static_origin_immutable BEFORE UPDATE OF origin_generation ON vm_static_instances
    WHEN OLD.origin_generation IS NOT NULL AND NEW.origin_generation IS NOT OLD.origin_generation
    BEGIN SELECT RAISE(ABORT,'immutable static origin identity'); END;

CREATE TABLE vm_restore_points (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    vm BLOB REFERENCES vm_instances(id),
    static_vm BLOB REFERENCES vm_static_instances(id),
    host BLOB NOT NULL REFERENCES vm_state_host(id),
    name TEXT CHECK(length(name) BETWEEN 1 AND 48 AND name NOT GLOB '*[^a-z0-9-]*'
        AND name NOT GLOB '-*' AND name NOT GLOB '*-' AND name NOT GLOB '*--*'),
    captured_ms INTEGER NOT NULL CHECK(captured_ms BETWEEN 0 AND 253402300799999),
    state TEXT NOT NULL CHECK(state IN ('copying','ready','dirty_release','deleting','quarantined')),
    provenance BLOB NOT NULL CHECK(length(provenance)=16),
    generation BLOB NOT NULL UNIQUE REFERENCES vm_generations(id),
    lineage BLOB NOT NULL CHECK(length(lineage)=32),
    charge INTEGER NOT NULL CHECK(charge>0),
    CHECK((vm IS NOT NULL)+(static_vm IS NOT NULL)=1),
    CHECK(state<>'dirty_release' OR static_vm IS NOT NULL),
    CHECK(state<>'deleting' OR vm IS NOT NULL),
    UNIQUE(vm,name),
    UNIQUE(static_vm,name)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER vm_protected_restore_point BEFORE DELETE ON vm_restore_points
    WHEN OLD.static_vm IS NOT NULL AND OLD.state IN ('ready','dirty_release')
    BEGIN SELECT RAISE(ABORT,'protected static restore point'); END;

CREATE TABLE vm_snapshot_release (
    generation BLOB PRIMARY KEY REFERENCES vm_generations(id) ON DELETE CASCADE,
    epoch BLOB NOT NULL CHECK(length(epoch)=16),
    disposition TEXT CHECK(disposition IN ('clean','dirty'))
) STRICT, WITHOUT ROWID;

CREATE TABLE vm_snapshot_work (
    activity INTEGER PRIMARY KEY CHECK(activity>0),
    vm BLOB REFERENCES vm_instances(id),
    static_vm BLOB REFERENCES vm_static_instances(id),
    action INTEGER NOT NULL CHECK(action IN (5,6,18)),
    target BLOB NOT NULL CHECK(length(target)=16),
    name TEXT CHECK(length(name) BETWEEN 1 AND 48 AND name NOT GLOB '*[^a-z0-9-]*'),
    displaced BLOB CHECK(length(displaced)=16),
    expected BLOB NOT NULL REFERENCES vm_generations(id),
    source BLOB NOT NULL REFERENCES vm_generations(id),
    candidate BLOB UNIQUE REFERENCES vm_generations(id),
    epoch BLOB NOT NULL CHECK(length(epoch)=16),
    lineage BLOB NOT NULL CHECK(length(lineage)=32),
    phase TEXT NOT NULL CHECK(phase IN ('copy','delete','abandoned','retire')),
    persistent INTEGER NOT NULL CHECK(persistent>=0),
    staging INTEGER NOT NULL CHECK(staging>=0),
    CHECK((vm IS NOT NULL)+(static_vm IS NOT NULL)=1),
    CHECK((action=18)=(phase='delete')),
    CHECK((candidate IS NULL)=(phase IN ('delete','retire'))),
    CHECK(phase<>'retire' OR (vm IS NOT NULL AND action=6)),
    CHECK(phase<>'delete' OR (vm IS NOT NULL AND persistent=0 AND staging=0)),
    CHECK((displaced IS NOT NULL)=(static_vm IS NOT NULL AND action=6)),
    UNIQUE(vm),
    UNIQUE(static_vm)
) STRICT;

CREATE TABLE vm_snapshot_expiry (
    vm BLOB NOT NULL REFERENCES vm_instances(id),
    generation BLOB NOT NULL UNIQUE REFERENCES vm_generations(id),
    expiry INTEGER NOT NULL,
    PRIMARY KEY(vm,generation)
) STRICT, WITHOUT ROWID;

";

#[derive(Clone, Copy)]
pub(super) enum Owner {
    Test { state: i64, expires: i64 },
    Static(StaticState),
}

pub(super) fn owner(db: &Connection, principal: &str, vm: Id) -> Result<Option<Owner>> {
    if let Some(state) = static_vm::owned_state(db, principal, vm)? {
        return Ok(Some(Owner::Static(state)));
    }
    Ok(db
        .query_row(
            "SELECT state,expires_ms FROM vm_instances WHERE id=?1 AND principal=?2",
            params![vm, principal],
            |row| {
                Ok(Owner::Test {
                    state: row.get(0)?,
                    expires: row.get(1)?,
                })
            },
        )
        .optional()?)
}

/// Durable half of the route fence, re-read in every mutating transaction.
/// B2 retains the intent until its connector and owned DNS are both settled.
pub(super) fn route_quiescent(db: &Connection, vm: Id) -> Result<bool> {
    Ok(publish::current(db)?.is_none_or(|intent| intent.vm != vm)
        && count(
            db,
            "SELECT count(*) FROM activity WHERE vm=?1 AND status=0 AND action IN (15,16)",
            [vm],
        )? == 0)
}

pub(crate) struct Point {
    pub id: Id,
    pub name: Option<String>,
    pub captured_ms: i64,
    pub charge: u64,
    pub protected: bool,
    pub restore_eligible: bool,
}

pub(crate) fn list(
    db: &Connection,
    principal: &str,
    vm: Id,
    after: Option<Id>,
    limit: u32,
) -> Result<Result<Vec<Point>, Refusal>> {
    if owner(db, principal, vm)?.is_none() {
        return Ok(Err(Refusal::NotFound));
    }
    let mut stmt = db.prepare(
        "SELECT id,name,captured_ms,charge,static_vm IS NOT NULL,state='ready'
         FROM vm_restore_points WHERE (vm=?1 OR static_vm=?1)
           AND host=(SELECT id FROM vm_state_host WHERE singleton=1)
           AND state IN ('ready','dirty_release') AND (?2 IS NULL OR id>?2)
         ORDER BY id LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![vm, after, limit], |row| {
        Ok(Point {
            id: row.get(0)?,
            name: row.get(1)?,
            captured_ms: row.get(2)?,
            charge: row.get(3)?,
            protected: row.get(4)?,
            restore_eligible: row.get(5)?,
        })
    })?;
    Ok(Ok(rows.collect::<rusqlite::Result<_>>()?))
}

pub(crate) fn precheck(
    db: &Connection,
    principal: &str,
    vm: Id,
    request: &crate::vm::snapshot::Request,
    now: i64,
) -> Result<Result<bool, Refusal>> {
    use crate::vm::snapshot::Request;
    let Some(owner) = owner(db, principal, vm)? else {
        return Ok(Err(Refusal::NotFound));
    };
    let is_static = matches!(owner, Owner::Static(_));
    if let Request::Restore(id) | Request::Delete(id) = request {
        let state: Option<String> = db.query_row(
            "SELECT state FROM vm_restore_points
             WHERE id=?1 AND (vm=?2 OR static_vm=?2) AND host=(SELECT id FROM vm_state_host WHERE singleton=1)",
            params![id, vm], |row| row.get(0),
        ).optional()?;
        if is_static && matches!(request, Request::Delete(_)) && state.is_some() {
            return Ok(Err(Refusal::StaticBackupDeleteForbidden));
        }
        if state.as_deref() != Some("ready") {
            return Ok(Err(Refusal::NotFound));
        }
    }
    if is_static && !route_quiescent(db, vm)? {
        return Ok(Err(Refusal::SnapshotPublishNotQuiesced));
    }
    match owner {
        Owner::Test { state, expires } if state == 4 || expires <= now => {
            return Ok(Err(Refusal::Expired));
        }
        Owner::Static(StaticState::Recovering) | Owner::Test { state: 3, .. } => {
            return Ok(Err(Refusal::CleanupUnconfirmed));
        }
        Owner::Static(StaticState::Stopped) | Owner::Test { state: 1, .. } => {}
        _ => return Ok(Err(Refusal::SnapshotOfflineRequired)),
    }
    if count(
        db,
        "SELECT count(*) FROM activity WHERE vm=?1 AND status=0 AND (action<=6 OR action IN (12,18))",
        [vm],
    )? != 0
    {
        return Ok(Err(Refusal::Busy));
    }
    if unsettled(db, vm)? {
        return Ok(Err(Refusal::CleanupUnconfirmed));
    }
    let create = matches!(request, Request::Create(_));
    if create || (is_static && matches!(request, Request::Restore(_))) {
        let [per_vm, host] = point_counts(db, vm)?;
        let limit = if create && is_static {
            MANUAL_STATIC_POINTS
        } else {
            POINTS_PER_VM
        };
        if per_vm >= limit || host >= POINTS_PER_HOST {
            return Ok(Err(Refusal::SnapshotLimit));
        }
    }
    if let Request::Create(Some(name)) = request
        && count(
            db,
            "SELECT count(*) FROM vm_restore_points WHERE (vm=?1 OR static_vm=?1) AND name=?2",
            params![vm, name.as_str()],
        )? != 0
    {
        return Ok(Err(Refusal::NameUnavailable));
    }
    Ok(Ok(is_static))
}

fn point_counts(db: &Connection, vm: Id) -> Result<[u32; 2]> {
    Ok(db.query_row(
        "SELECT
            (SELECT count(*) FROM vm_restore_points WHERE vm=?1 OR static_vm=?1)
                +(SELECT count(*) FROM vm_snapshots WHERE vm=?1),
            (SELECT count(*) FROM vm_restore_points)+(SELECT count(*) FROM vm_snapshots)
                +(SELECT count(*) FROM vm_snapshot_work WHERE action=5 OR displaced IS NOT NULL)",
        [vm],
        |row| Ok([row.get(0)?, row.get(1)?]),
    )?)
}

pub(crate) fn unsettled(db: &Connection, vm: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_snapshot_work WHERE vm=?1 OR static_vm=?1",
        [vm],
    )? != 0)
}

pub(crate) fn invalidate_release(db: &Connection, vm: Id, epoch: Id) -> Result<()> {
    db.execute(
        "INSERT INTO vm_snapshot_release(generation,epoch)
         SELECT id,?2 FROM vm_generations WHERE (vm_id=?1 OR static_id=?1) AND active=1
         ON CONFLICT(generation) DO UPDATE SET epoch=excluded.epoch,disposition=NULL",
        params![vm, epoch],
    )?;
    Ok(())
}

pub(super) fn initial_release(db: &Connection, generation: Id) -> Result<()> {
    db.execute(
        "INSERT INTO vm_snapshot_release(generation,epoch,disposition) VALUES(?1,?1,'clean')",
        [generation],
    )?;
    Ok(())
}

pub(super) fn recover(db: &Connection, now: i64) -> Result<()> {
    let mut stmt = db.prepare(
        "SELECT w.activity FROM vm_snapshot_work w JOIN vm_operations o ON o.activity_id=w.activity
         WHERE o.binding_kind='snapshot' AND o.status=0 ORDER BY w.activity LIMIT ?1",
    )?;
    let pending = stmt
        .query_map([MAX_CALLER_PENDING], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for activity_id in pending {
        finish(db, activity_id, Status::Interrupted, now, Some(false))?;
    }
    abandon_copies(db, None)?;
    Ok(())
}

pub(super) fn abandon_copies(db: &Connection, vm: Option<Id>) -> Result<()> {
    db.execute(
        "UPDATE vm_artifacts SET state=CASE WHEN file_id IS NULL THEN 'quarantined'
            ELSE 'pending_disposal' END
         WHERE generation_id IN (SELECT candidate FROM vm_snapshot_work
            WHERE phase='copy' AND (?1 IS NULL OR vm=?1))",
        [vm],
    )?;
    db.execute(
        "UPDATE vm_snapshot_work SET phase='abandoned' WHERE phase='copy' AND (?1 IS NULL OR vm=?1)",
        [vm],
    )?;
    Ok(())
}

/// Expiry captures the exact non-active TEST work owned by this instance.
/// The active pair is disposed last by the ordinary lease cleanup.
pub(super) fn claim_expiry(db: &Connection, vm: Id, activity_id: i64, now: i64) -> Result<()> {
    let pending = db.query_row(
        "SELECT w.activity FROM vm_snapshot_work w JOIN vm_operations o ON o.activity_id=w.activity
         WHERE w.vm=?1 AND o.status=0",
        [vm], |row| row.get::<_, i64>(0),
    ).optional()?;
    if let Some(pending) = pending {
        finish(db, pending, Status::Interrupted, now, Some(false))?;
    }
    abandon_copies(db, Some(vm))?;
    db.execute(
        "INSERT INTO vm_snapshot_expiry(vm,generation,expiry)
         SELECT ?1,g.id,?2 FROM vm_generations g
         WHERE g.vm_id=?1 AND g.active=0 AND (
             EXISTS(SELECT 1 FROM vm_restore_points p WHERE p.vm=?1 AND p.generation=g.id)
             OR EXISTS(SELECT 1 FROM vm_snapshot_work w WHERE w.vm=?1 AND
                 (w.candidate=g.id OR (w.phase='retire' AND w.expected=g.id))))",
        params![vm, activity_id],
    )?;
    Ok(())
}

pub(super) fn expiry_waits(db: &Connection, vm: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_snapshot_expiry WHERE vm=?1",
        [vm],
    )? != 0
        || unsettled(db, vm)?)
}

/// At most one destructive snapshot pair per VM per turn. Resolve the
/// operation first because the intent can hold a reference to its source.
pub(super) fn cleanup_kinds(db: &Connection) -> Result<Vec<CleanupKind>> {
    let mut stmt = db.prepare(
        "SELECT coalesce(vm,static_vm),
            CASE phase WHEN 'abandoned' THEN candidate WHEN 'retire' THEN expected ELSE source END
         FROM vm_snapshot_work WHERE phase<>'copy'
         UNION ALL
         SELECT e.vm,min(e.generation) FROM vm_snapshot_expiry e
           WHERE NOT EXISTS(SELECT 1 FROM vm_snapshot_work w WHERE w.vm=e.vm)
         GROUP BY e.vm ORDER BY 1 LIMIT 16",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CleanupKind::Snapshot {
            vm: row.get(0)?,
            generation: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

struct CleanupAuthority {
    activity: Option<i64>,
    point: Option<Id>,
    is_static: bool,
    phase: String,
}

fn cleanup_authority(db: &Connection, vm: Id, generation: Id) -> Result<Option<CleanupAuthority>> {
    Ok(db.query_row(
        "SELECT w.activity,
             CASE WHEN w.phase='delete' THEN w.target END, w.static_vm IS NOT NULL,w.phase
         FROM vm_snapshot_work w JOIN vm_generations g ON g.id=?2 AND g.active=0
         WHERE (
            (w.vm=?1 AND g.vm_id=?1 AND (
                (w.phase='abandoned' AND w.candidate=g.id)
                OR (w.phase='retire' AND w.expected=g.id)
                OR (w.phase='delete' AND w.source=g.id AND EXISTS(
                    SELECT 1 FROM vm_restore_points p
                    WHERE p.id=w.target AND p.vm=w.vm AND p.generation=g.id AND p.state='deleting'))))
            OR (w.static_vm=?1 AND g.static_id=?1 AND w.phase='abandoned' AND w.candidate=g.id))
           AND (w.phase='delete' OR NOT EXISTS(SELECT 1 FROM vm_restore_points WHERE generation=g.id))
         UNION ALL
         SELECT NULL,p.id,0,'expiry'
         FROM vm_snapshot_expiry e JOIN vm_instances v ON v.id=e.vm
         JOIN vm_generations g ON g.id=e.generation AND g.vm_id=e.vm AND g.active=0
         JOIN vm_restore_points p ON p.vm=e.vm AND p.generation=g.id
         WHERE e.vm=?1 AND e.generation=?2 AND v.cleanup_id=e.expiry
           AND v.state=4 AND v.cleanup_route='lease' AND p.state='ready'
           AND NOT EXISTS(SELECT 1 FROM vm_snapshot_work w WHERE w.vm=e.vm)",
        params![vm, generation],
        |row| Ok(CleanupAuthority {
            activity: row.get(0)?, point: row.get(1)?, is_static: row.get(2)?, phase: row.get(3)?,
        }),
    ).optional()?)
}

pub(super) fn cleanup_work(
    db: &Connection,
    kind: CleanupKind,
    vm: Id,
    generation: Id,
) -> Result<Option<CleanupWork>> {
    let Some(authority) = cleanup_authority(db, vm, generation)? else {
        return Ok(None);
    };
    let mut stmt = db.prepare(
        "SELECT role,object_key,state,volume_id,directory_id,file_id
         FROM vm_artifacts WHERE generation_id=?1 ORDER BY role LIMIT 3",
    )?;
    let rows = stmt
        .query_map([generation], |row| {
            let volume = row.get::<_, Option<[u8; 8]>>(3)?.map(u64::from_be_bytes);
            let file = row.get::<_, Option<Vec<u8>>>(5)?.and_then(identity_bytes);
            Ok((
                row.get::<_, String>(2)?,
                CleanupArtifact {
                    generation,
                    role: row.get(0)?,
                    key: row.get(1)?,
                    directory: row.get::<_, Option<Vec<u8>>>(4)?.and_then(identity_bytes),
                    identity: volume
                        .zip(file)
                        .map(|(volume, file)| FileIdentity { volume, file }),
                    retain: false,
                },
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let expected_roles = if authority.is_static {
        ["static_disk", "static_vars"]
    } else {
        ["disk", "vars"]
    };
    let accounted = rows.len() == 2
        && rows
            .iter()
            .zip(expected_roles)
            .all(|((state, leaf), role)| {
                leaf.role == role
                    && leaf.directory.is_some()
                    && leaf.identity.is_some()
                    && match authority.phase.as_str() {
                        "expiry" | "delete" => state == "sealed",
                        "abandoned" | "retire" => state == "pending_disposal",
                        _ => false,
                    }
            });
    Ok(Some(CleanupWork {
        kind,
        pending_worker: authority.activity,
        artifacts: rows.into_iter().map(|(_, leaf)| leaf).collect(),
        accounted,
    }))
}

pub(super) fn settle_cleanup(
    db: &Connection,
    work: &CleanupWork,
    vm: Id,
    generation: Id,
    now: i64,
    confirmed: bool,
) -> Result<bool> {
    let Some(current) = cleanup_work(db, work.kind, vm, generation)? else {
        return Ok(false);
    };
    if !confirmed || !current.accounted || current.artifacts != work.artifacts {
        return Ok(false);
    }
    let Some(authority) = cleanup_authority(db, vm, generation)? else {
        return Ok(false);
    };
    if let Some(activity) = authority.activity {
        db.execute("DELETE FROM vm_snapshot_work WHERE activity=?1", [activity])?;
    }
    if let Some(point) = authority.point {
        db.execute(
            "DELETE FROM vm_restore_points WHERE id=?1 AND vm=?2 AND generation=?3",
            params![point, vm, generation],
        )?;
    }
    db.execute(
        "DELETE FROM vm_snapshot_expiry WHERE vm=?1 AND generation=?2",
        params![vm, generation],
    )?;
    forget_generation(db, generation)?;
    let caller_delete = authority.activity.filter(|_| authority.phase == "delete");
    if let Some(activity) = caller_delete
        && count(
            db,
            "SELECT count(*) FROM activity WHERE id=?1 AND status=0",
            [activity],
        )? == 1
    {
        finish(db, activity, Status::Succeeded, now, Some(true))?;
    } else {
        db.execute(
            "INSERT INTO activity(principal,actor,vm_owner,vm,action,started_ms)
             SELECT 'arxd','daemon',principal,id,19,?2 FROM vm_instances WHERE id=?1
             UNION ALL
             SELECT 'arxd','daemon',principal,id,19,?2 FROM vm_static_instances WHERE id=?1",
            params![vm, now],
        )?;
        finish(
            db,
            db.last_insert_rowid(),
            Status::Succeeded,
            now,
            Some(true),
        )?;
    }
    Ok(true)
}

#[cfg(test)]
pub(crate) use effects::*;

#[cfg(test)]
mod effects {
    use super::*;
    use crate::vm::{
        DiskClaim,
        runtime::publication::SnapshotRoute,
        snapshot::{Request, SnapshotName},
    };

    pub(crate) type Pair = (PairSlots, [ArtifactEvidence; 2]);

    #[derive(Clone, Copy)]
    pub(crate) struct Active {
        pub pair: Pair,
        pub host: Id,
        pub template: Id,
        pub recipe: [u8; 32],
        pub origin: Id,
        pub golden: Option<Pair>,
        pub lineage: [u8; 32],
        pub epoch: Id,
        pub clean: bool,
    }

    pub(crate) struct Work {
        pub activity: i64,
        pub vm: Id,
        pub is_static: bool,
        pub action: i64,
        pub active: Active,
        pub source: Pair,
        pub target: Id,
        pub claim: DiskClaim,
        pub points: [u32; 2],
        pub candidate: Option<PairSlots>,
    }

    fn roles(is_static: bool) -> [&'static str; 2] {
        if is_static {
            ["static_disk", "static_vars"]
        } else {
            ["disk", "vars"]
        }
    }

    fn pair(db: &Connection, generation: Id, is_static: bool, state: &str) -> Result<Pair> {
        let [disk_role, vars_role] = roles(is_static);
        let mut stmt = db.prepare(
            "SELECT role,object_key,volume_id,directory_id,file_id,content_digest,bytes
             FROM vm_artifacts WHERE generation_id=?1 ORDER BY role LIMIT 3",
        )?;
        let mut rows = stmt.query([generation])?;
        let mut leaf = |role| -> Result<(ArtifactSlot, ArtifactEvidence)> {
            let row = rows
                .next()?
                .ok_or_else(|| anyhow::anyhow!("missing snapshot file"))?;
            ensure!(row.get::<_, String>(0)? == role, "invalid snapshot role");
            let slot = ArtifactSlot {
                generation,
                role,
                key: row.get(1)?,
            };
            let evidence = ArtifactEvidence {
                identity: FileIdentity {
                    volume: u64::from_be_bytes(row.get(2)?),
                    file: identity_bytes(row.get(4)?)
                        .ok_or_else(|| anyhow::anyhow!("invalid file identity"))?,
                },
                directory: identity_bytes(row.get(3)?)
                    .ok_or_else(|| anyhow::anyhow!("invalid directory identity"))?,
                pin: crate::vm::paths::ArtifactPin {
                    sha256: row.get(5)?,
                    size: row.get(6)?,
                },
            };
            ensure!(
                evidence_matches(db, slot, evidence, state)?,
                "invalid snapshot artifact state"
            );
            Ok((slot, evidence))
        };
        let (disk, disk_proof) = leaf(disk_role)?;
        let (vars, vars_proof) = leaf(vars_role)?;
        ensure!(
            rows.next()?.is_none()
                && disk_proof.identity != vars_proof.identity
                && disk_proof.directory == vars_proof.directory
                && disk_proof.identity.volume == vars_proof.identity.volume,
            "invalid snapshot pair"
        );
        Ok((PairSlots { disk, vars }, [disk_proof, vars_proof]))
    }

    pub(crate) fn active(db: &Connection, vm: Id, is_static: bool) -> Result<Option<Active>> {
        use sha2::{Digest, Sha256};
        let image = db
            .query_row(
                "SELECT g.id,r.epoch,r.disposition,v.template,t.recipe_digest,
                t.disk_digest,t.vars_digest,b.id
             FROM vm_generations g JOIN vm_snapshot_release r ON r.generation=g.id
             JOIN vm_instances v ON v.id=g.vm_id JOIN vm_templates t ON t.id=v.template
             JOIN vm_build_intents b ON b.template_id=t.id AND b.state='sealed'
             WHERE v.id=?1 AND g.active=1 AND r.disposition IS NOT NULL AND NOT ?2
             UNION ALL
             SELECT g.id,r.epoch,r.disposition,s.template,s.recipe_digest,
                s.disk_digest,s.vars_digest,s.origin_generation
             FROM vm_generations g JOIN vm_snapshot_release r ON r.generation=g.id
             JOIN vm_static_instances s ON s.id=g.static_id
             WHERE s.id=?1 AND g.active=1 AND r.disposition IS NOT NULL AND ?2",
                params![vm, is_static],
                |row| {
                    Ok((
                        row.get::<_, Id>(0)?,
                        row.get::<_, Id>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Id>(3)?,
                        row.get::<_, [u8; 32]>(4)?,
                        row.get::<_, [u8; 32]>(5)?,
                        row.get::<_, [u8; 32]>(6)?,
                        row.get::<_, Id>(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((generation, epoch, disposition, template, recipe, disk, vars, origin)) = image
        else {
            return Ok(None);
        };
        let host = static_vm::host_id(db)?;
        let golden = if is_static {
            None
        } else {
            let generation = db.query_row(
                "SELECT g.id FROM vm_generations g WHERE g.build_id=?1 AND g.active=1",
                [origin],
                |row| row.get::<_, Id>(0),
            )?;
            let golden = pair(db, generation, false, "sealed")?;
            ensure!(
                golden.1[0].pin.sha256 == disk && golden.1[1].pin.sha256 == vars,
                "golden pair changed"
            );
            Some(golden)
        };
        let mut lineage = Sha256::new()
            .chain_update(b"arxcovo.vm.snapshot.lineage.v1\0")
            .chain_update([u8::from(is_static)])
            .chain_update(host)
            .chain_update(vm)
            .chain_update(template)
            .chain_update(recipe)
            .chain_update(disk)
            .chain_update(vars)
            .chain_update(origin);
        if let Some((slots, [disk, vars])) = golden {
            lineage.update(slots.disk.generation);
            for (slot, evidence) in [(slots.disk, disk), (slots.vars, vars)] {
                lineage.update(slot.key);
                lineage.update(evidence.identity.volume.to_be_bytes());
                lineage.update(evidence.identity.file);
                lineage.update(evidence.pin.size.to_be_bytes());
            }
        }
        Ok(Some(Active {
            pair: pair(db, generation, is_static, "active")?,
            host,
            template,
            recipe,
            origin,
            golden,
            lineage: lineage.finalize().into(),
            epoch,
            clean: disposition == "clean",
        }))
    }

    pub(crate) fn selected(
        db: &Connection,
        vm: Id,
        id: Id,
        is_static: bool,
        lineage: [u8; 32],
        state: &str,
    ) -> Result<Option<Pair>> {
        let id = db.query_row(
            "SELECT p.generation FROM vm_restore_points p JOIN vm_generations g ON g.id=p.generation
             WHERE p.id=?1 AND p.state=?2 AND p.lineage=?3 AND g.active=0
                AND (p.vm=g.vm_id AND p.vm=?4 OR p.static_vm=g.static_id AND p.static_vm=?4)
                AND p.host=(SELECT id FROM vm_state_host WHERE singleton=1)",
            params![id, state, lineage, vm],
            |row| row.get::<_, Id>(0),
        ).optional()?;
        id.map(|id| pair(db, id, is_static, "sealed")).transpose()
    }

    pub(crate) fn prove_stop(db: &Connection, vm: Id, pair: Pair) -> Result<()> {
        let (slots, proof) = pair;
        let charge: u64 = db.query_row(
            "SELECT persistent FROM vm_disk_claims WHERE vm=?1 AND snapshot IS NULL AND persistent>0
             UNION ALL SELECT persistent FROM vm_static_instances WHERE id=?1",
            [vm], |row| row.get(0),
        )?;
        ensure!(
            slots.disk.generation == slots.vars.generation
                && proof[0]
                    .pin
                    .size
                    .checked_add(proof[1].pin.size)
                    .is_some_and(|size| size <= charge),
            "stopped pair exceeds its active claim"
        );
        for (slot, observed) in [slots.disk, slots.vars].into_iter().zip(proof) {
            ensure!(
                db.execute(
                    "UPDATE vm_artifacts SET content_digest=?4,bytes=?5
                 WHERE generation_id=?1 AND object_key=?2 AND role=?3 AND state='active'
                   AND volume_id=?6 AND directory_id=?7 AND file_id=?8
                   AND generation_id IN (SELECT id FROM vm_generations
                        WHERE id=?1 AND (vm_id=?9 OR static_id=?9) AND active=1)",
                    params![
                        slot.generation,
                        slot.key,
                        slot.role,
                        observed.pin.sha256,
                        observed.pin.size,
                        observed.identity.volume.to_be_bytes(),
                        observed.directory,
                        observed.identity.file,
                        vm
                    ],
                )? == 1,
                "snapshot stop artifact identity changed"
            );
        }
        ensure!(db.execute(
            "UPDATE vm_snapshot_release SET disposition='clean'
             WHERE generation=?2 AND disposition IS NULL AND generation IN (
                SELECT id FROM vm_generations WHERE id=?2 AND active=1 AND (vm_id=?1 OR static_id=?1))",
            params![vm,slots.disk.generation],
        )? == 1, "snapshot stop writer epoch changed");
        Ok(())
    }

    pub(crate) fn check_route(
        db: &Connection,
        vm: Id,
        is_static: bool,
        route: Option<&SnapshotRoute>,
    ) -> Result<bool> {
        Ok(!is_static || (route.is_some_and(|route| route.vm() == vm) && route_quiescent(db, vm)?))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reserve(
        db: &Connection,
        vm: Id,
        is_static: bool,
        binding: Binding<'_>,
        request: &Request,
        active: Active,
        source: Pair,
        claim: DiskClaim,
        now: i64,
    ) -> Result<(Work, OperationRef)> {
        let (action, target, name, displaced) = match request {
            Request::Create(name) => (5, new_id()?, name.as_ref().map(SnapshotName::as_str), None),
            Request::Restore(id) => (6, *id, None, is_static.then(new_id).transpose()?),
            Request::Delete(id) => (18, *id, None, None),
        };
        ensure!(
            binding.action == action
                && binding.subject == request.binding(vm)
                && (action != 18 || !is_static),
            "invalid snapshot authority"
        );
        let points = point_counts(db, vm)?;
        let operation = binding.record(db, now, Some(vm), None)?;
        let activity = operation.receipt.activity_id;
        let test_vm = (!is_static).then_some(vm);
        let static_vm = is_static.then_some(vm);
        let candidate = if action == 18 {
            ensure!(db.execute(
                "UPDATE vm_restore_points SET state='deleting' WHERE id=?1 AND vm=?2 AND generation=?3 AND state='ready'",
                params![target, vm, source.0.disk.generation],
            )? == 1, "invalid TEST deletion target");
            None
        } else {
            let generation = new_id()?;
            db.execute(
                "INSERT INTO vm_generations(id,vm_id,static_id,generation)
                 SELECT ?1,?2,?3,coalesce(max(generation),0)+1 FROM vm_generations
                 WHERE vm_id=?2 OR static_id=?3",
                params![generation, test_vm, static_vm],
            )?;
            let [disk, vars] = roles(is_static);
            Some(PairSlots {
                disk: plan(db, generation, disk)?,
                vars: plan(db, generation, vars)?,
            })
        };
        db.execute(
            "INSERT INTO vm_snapshot_work(activity,vm,static_vm,action,target,name,displaced,expected,
                source,candidate,epoch,lineage,phase,persistent,staging)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![activity, test_vm, static_vm, action, target, name, displaced,
                active.pair.0.disk.generation, source.0.disk.generation, candidate.map(|p| p.disk.generation),
                active.epoch, active.lineage, if action == 18 { "delete" } else { "copy" },
                claim.persistent(), claim.staging],
        )?;
        if action != 5 {
            result_id(db, activity, target)?;
        }
        require_headroom(db)?;
        Ok((
            Work {
                activity,
                vm,
                is_static,
                action,
                active,
                source,
                target,
                claim,
                points,
                candidate,
            },
            operation,
        ))
    }

    fn result_id(db: &Connection, activity: i64, id: Id) -> Result<()> {
        ensure!(db.execute(
            "UPDATE vm_operations SET snapshot_result=?2
             WHERE activity_id=?1 AND status=0 AND binding_kind='snapshot' AND snapshot_result IS NULL",
            params![activity, id],
        )? == 1, "invalid snapshot receipt");
        Ok(())
    }

    fn pending(db: &Connection, work: &Work) -> Result<bool> {
        Ok(count(db,
            "SELECT count(*) FROM vm_snapshot_work w JOIN vm_operations o ON o.activity_id=w.activity
             WHERE w.activity=?1 AND coalesce(w.vm,w.static_vm)=?2 AND w.action=?3
               AND w.expected=?4 AND w.source=?5 AND w.candidate IS ?6 AND w.epoch=?7 AND w.lineage=?8
               AND o.vm=?2 AND o.status=0 AND o.binding_kind='snapshot' AND w.phase=?9",
            params![work.activity, work.vm, work.action, work.active.pair.0.disk.generation,
                work.source.0.disk.generation, work.candidate.map(|p| p.disk.generation),
                work.active.epoch, work.active.lineage, if work.action == 18 {"delete"} else {"copy"}],
        )? == 1)
    }

    pub(crate) fn can_copy(db: &Connection, work: &Work, now: i64) -> Result<bool> {
        Ok(pending(db, work)? && count(db,
            "SELECT EXISTS(SELECT 1 FROM vm_instances WHERE id=?1 AND state=1 AND expires_ms>?2)
              OR EXISTS(SELECT 1 FROM vm_static_instances WHERE id=?1 AND state=1)",
            params![work.vm,now],
        )? == 1)
    }

    pub(crate) fn fail_copy(db: &Connection, work: &Work, now: i64) -> Result<()> {
        if !pending(db, work)? {
            return Ok(());
        }
        db.execute(
            "UPDATE vm_artifacts SET state=CASE WHEN file_id IS NULL THEN 'quarantined'
                ELSE 'pending_disposal' END WHERE generation_id=?1",
            [work.candidate.map(|slots| slots.disk.generation)],
        )?;
        db.execute(
            "UPDATE vm_snapshot_work SET phase='abandoned' WHERE activity=?1",
            [work.activity],
        )?;
        finish_with_script(
            db,
            work.activity,
            Status::Failed,
            now,
            Some(false),
            None,
            Some(FailureReason::CleanupUnconfirmed),
        )
    }

    pub(crate) fn record_identity(
        db: &Connection,
        work: &Work,
        slot: ArtifactSlot,
        identity: FileIdentity,
        directory: Id,
    ) -> Result<()> {
        ensure!(
            pending(db, work)?
                && work
                    .candidate
                    .is_some_and(|p| [p.disk, p.vars].contains(&slot)),
            "snapshot worker no longer owns staging"
        );
        ensure!(
            db.execute(
                "UPDATE vm_artifacts SET state='created',volume_id=?4,directory_id=?5,file_id=?6
             WHERE generation_id=?1 AND role=?2 AND object_key=?3 AND state='planned'",
                params![
                    slot.generation,
                    slot.role,
                    slot.key,
                    identity.volume.to_be_bytes(),
                    directory,
                    identity.file
                ],
            )? == 1,
            "snapshot creation identity already recorded"
        );
        Ok(())
    }

    pub(crate) fn identify_leaf(
        db: &Connection,
        work: &Work,
        slot: ArtifactSlot,
        evidence: ArtifactEvidence,
    ) -> Result<()> {
        ensure!(
            pending(db, work)?
                && work
                    .candidate
                    .is_some_and(|p| [p.disk, p.vars].contains(&slot)),
            "snapshot worker no longer owns staging"
        );
        ensure!(count(db,
            "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1 AND role=?2 AND object_key=?3
             AND state='created' AND volume_id=?4 AND directory_id=?5 AND file_id=?6",
            params![slot.generation,slot.role,slot.key,evidence.identity.volume.to_be_bytes(),
                evidence.directory,evidence.identity.file],
        )? == 1, "snapshot staging identity changed");
        identify(db, slot, evidence)
    }

    fn ready_point(
        db: &Connection,
        activity: i64,
        new_generation: Id,
        provenance: Id,
        charge: u64,
        state: &str,
    ) -> Result<()> {
        db.execute(
            "INSERT INTO vm_restore_points(id,vm,static_vm,host,name,captured_ms,state,provenance,generation,lineage,charge)
             SELECT CASE WHEN action=5 THEN target ELSE displaced END,
                vm,static_vm,(SELECT id FROM vm_state_host WHERE singleton=1),name,
                (SELECT started_ms FROM activity WHERE id=?1),?5,?3,?2,lineage,?4
             FROM vm_snapshot_work WHERE activity=?1",
            params![activity, new_generation, provenance, charge, state],
        )?;
        Ok(())
    }

    pub(crate) fn commit(
        db: &Connection,
        work: &Work,
        proof: [ArtifactEvidence; 2],
        now: i64,
        route: Option<&SnapshotRoute>,
    ) -> Result<()> {
        ensure!(
            can_copy(db, work, now)? && check_route(db, work.vm, work.is_static, route)?,
            "snapshot commit fenced"
        );
        let current = active(db, work.vm, work.is_static)?
            .ok_or_else(|| anyhow::anyhow!("active release invalidated"))?;
        ensure!(
            current.pair == work.active.pair
                && current.epoch == work.active.epoch
                && current.lineage == work.active.lineage,
            "active generation changed"
        );
        let (target, charge): (Id, u64) = db.query_row(
            "SELECT target,persistent FROM vm_snapshot_work WHERE activity=?1",
            [work.activity],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let slots = work
            .candidate
            .ok_or_else(|| anyhow::anyhow!("missing staging pair"))?;
        let old = work.active.pair.0.disk.generation;
        let new_generation = slots.disk.generation;
        for ((slot, evidence), source) in [(slots.disk, proof[0]), (slots.vars, proof[1])]
            .into_iter()
            .zip(work.source.1)
        {
            ensure!(
                evidence.pin == source.pin
                    && evidence.identity != source.identity
                    && evidence_matches(db, slot, evidence, "identified")?,
                "snapshot copy changed"
            );
        }
        ensure!(
            proof[0].identity != proof[1].identity
                && proof[0].directory == proof[1].directory
                && proof[0].identity.volume == proof[1].identity.volume,
            "invalid snapshot staging pair"
        );
        if work.action == 5 {
            ensure!(
                current.clean && work.source == current.pair,
                "capture requires clean release"
            );
            db.execute(
                "UPDATE vm_artifacts SET state='sealed' WHERE generation_id=?1",
                [new_generation],
            )?;
            ready_point(db, work.activity, new_generation, old, charge, "ready")?;
            result_id(db, work.activity, target)?;
        } else {
            ensure!(
                work.action == 6
                    && selected(
                        db,
                        work.vm,
                        target,
                        work.is_static,
                        current.lineage,
                        "ready"
                    )? == Some(work.source),
                "restore source changed"
            );
            ensure!(db.execute(
                "UPDATE vm_generations SET active=0 WHERE id=?1 AND active=1 AND (vm_id=?2 OR static_id=?2)",
                params![old, work.vm],
            )? == 1, "active generation CAS lost");
            activate(db, slots, "active")?;
            initial_release(db, new_generation)?;
            if work.is_static {
                let old_charge: u64 = db.query_row(
                    "SELECT persistent FROM vm_static_instances WHERE id=?1",
                    [work.vm],
                    |row| row.get(0),
                )?;
                db.execute(
                    "UPDATE vm_static_instances SET persistent=?2 WHERE id=?1",
                    params![work.vm, charge],
                )?;
                db.execute(
                    "UPDATE vm_artifacts SET state='sealed' WHERE generation_id=?1",
                    [old],
                )?;
                ready_point(
                    db,
                    work.activity,
                    old,
                    old,
                    old_charge,
                    if current.clean {
                        "ready"
                    } else {
                        "dirty_release"
                    },
                )?;
            } else {
                let old_charge: u64 = db.query_row(
                    "SELECT persistent FROM vm_disk_claims WHERE vm=?1 AND snapshot IS NULL AND staging=0",
                    [work.vm], |row| row.get(0),
                )?;
                ensure!(db.execute(
                    "UPDATE vm_disk_claims SET persistent=?2 WHERE vm=?1 AND snapshot IS NULL AND staging=0",
                    params![work.vm, charge],
                )? == 1, "ambiguous TEST active charge");
                db.execute(
                    "UPDATE vm_artifacts SET state='pending_disposal' WHERE generation_id=?1",
                    [old],
                )?;
                db.execute(
                    "UPDATE vm_snapshot_work SET phase='retire',candidate=NULL,persistent=?2,staging=0
                     WHERE activity=?1",
                    params![work.activity, old_charge],
                )?;
                finish(db, work.activity, Status::Succeeded, now, Some(false))?;
                return require_headroom(db);
            }
        }
        db.execute(
            "DELETE FROM vm_snapshot_work WHERE activity=?1",
            [work.activity],
        )?;
        finish(db, work.activity, Status::Succeeded, now, Some(true))?;
        require_headroom(db)
    }
}

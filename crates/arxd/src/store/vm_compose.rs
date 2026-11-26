//! Durable host intent, immutable owner enrollment and conservative volume charges.
use super::{VmStore, count, require_headroom};
use crate::vm::{Id, compose::*, compose_backup};
use anyhow::{Result, ensure};
use arx_guest::compose::{Desired, MAX_PROJECTS, MAX_VOLUMES, OperationState, OperationView};
use rusqlite::{Connection, OptionalExtension, params};

/// Owner maintenance keeps the original backup row immutable. A prepared
/// retirement is charged until the owner has proved removal.
pub(super) const MIGRATE_V15: &str = "
ALTER TABLE vm_compose_backups ADD COLUMN namespace_tracked INTEGER NOT NULL DEFAULT 0
    CHECK(namespace_tracked IN (0,1));
CREATE TABLE vm_compose_backup_retirements (
    backup_id BLOB PRIMARY KEY REFERENCES vm_compose_backups(id) CHECK(length(backup_id)=16),
    owner TEXT NOT NULL CHECK(length(owner) BETWEEN 1 AND 184),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    finished_ms INTEGER CHECK(finished_ms BETWEEN started_ms AND 253402300799999),
    sha256 BLOB CHECK(sha256 IS NULL OR length(sha256)=32)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER vm_compose_backup_retirements_no_delete
BEFORE DELETE ON vm_compose_backup_retirements BEGIN
    SELECT RAISE(ABORT,'backup retirement audit cannot be deleted');
END;
CREATE TRIGGER vm_compose_backup_retirements_complete_only
BEFORE UPDATE ON vm_compose_backup_retirements WHEN
    OLD.finished_ms IS NOT NULL OR NEW.finished_ms IS NULL OR
    NEW.backup_id IS NOT OLD.backup_id OR NEW.owner IS NOT OLD.owner OR
    NEW.started_ms IS NOT OLD.started_ms OR NEW.sha256 IS NOT OLD.sha256
BEGIN
    SELECT RAISE(ABORT,'backup retirement audit is immutable');
END;
";

/// Store v14 migration: protected Compose SQLite backups (ADR 0070, leg E4).
pub(super) const MIGRATE_V14: &str = "
CREATE TABLE activity_v14 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 24),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 53),
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
INSERT INTO activity_v14(id,principal,actor,vm_owner,vm,action,started_ms,status,reason,
    finished_ms,terminal_order,cleanup_confirmed,publication_failure)
    SELECT id,principal,actor,vm_owner,vm,action,started_ms,status,reason,
        finished_ms,terminal_order,cleanup_confirmed,publication_failure FROM activity;
INSERT INTO sqlite_sequence(name,seq) SELECT 'activity_v14',seq FROM sqlite_sequence
    WHERE name='activity' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='activity_v14');
UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='activity'),0))
    WHERE name='activity_v14';
DROP TABLE activity;
ALTER TABLE activity_v14 RENAME TO activity;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND (action<=6 OR action IN (12,18));

CREATE TABLE vm_compose_backup_targets (
    vm BLOB NOT NULL,
    project TEXT NOT NULL,
    volume TEXT NOT NULL CHECK(length(volume) BETWEEN 1 AND 16),
    database TEXT NOT NULL CHECK(length(database) BETWEEN 1 AND 64),
    max_bytes INTEGER NOT NULL DEFAULT 536870912 CHECK(max_bytes BETWEEN 1 AND 536870912),
    PRIMARY KEY(vm,project),
    FOREIGN KEY(vm,project,volume) REFERENCES vm_compose_volumes(vm,project,slot)
) STRICT, WITHOUT ROWID;

CREATE TABLE vm_compose_backups (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    operation_id BLOB NOT NULL UNIQUE CHECK(length(operation_id)=16),
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB NOT NULL REFERENCES vm_static_instances(id) CHECK(length(vm)=16),
    project TEXT NOT NULL,
    generation BLOB NOT NULL REFERENCES vm_generations(id) CHECK(length(generation)=16),
    boot INTEGER NOT NULL CHECK(boot>0),
    revision INTEGER NOT NULL CHECK(revision>0),
    volume TEXT NOT NULL CHECK(length(volume) BETWEEN 1 AND 16),
    database TEXT NOT NULL CHECK(length(database) BETWEEN 1 AND 64),
    state TEXT NOT NULL CHECK(state IN ('staging','settled','partial_uncertain','cleaned')),
    host_staging_charge INTEGER NOT NULL CHECK(host_staging_charge>=0),
    guest_staging_charge INTEGER NOT NULL CHECK(guest_staging_charge>=0),
    retained_charge INTEGER NOT NULL DEFAULT 0 CHECK(retained_charge>=0),
    size_bytes INTEGER CHECK(size_bytes IS NULL OR size_bytes BETWEEN 1 AND 536870912),
    sha256 BLOB CHECK(sha256 IS NULL OR length(sha256)=32),
    volume_id BLOB CHECK(volume_id IS NULL OR length(volume_id)=8),
    directory_id BLOB CHECK(directory_id IS NULL OR length(directory_id)=16),
    file_id BLOB CHECK(file_id IS NULL OR length(file_id)=16),
    created_ms INTEGER NOT NULL CHECK(created_ms BETWEEN 0 AND 253402300799999),
    settled_ms INTEGER CHECK(settled_ms IS NULL OR settled_ms BETWEEN 0 AND 253402300799999),
    FOREIGN KEY(vm,project) REFERENCES vm_compose_projects(vm,project),
    CHECK((state='settled')=(settled_ms IS NOT NULL AND sha256 IS NOT NULL AND size_bytes IS NOT NULL AND retained_charge>0 AND host_staging_charge=0 AND guest_staging_charge=0)),
    CHECK(state<>'cleaned' OR (host_staging_charge=0 AND guest_staging_charge=0 AND retained_charge=0)),
    CHECK(state<>'partial_uncertain' OR (host_staging_charge>0 AND retained_charge=0))
) STRICT, WITHOUT ROWID;
CREATE INDEX vm_compose_backups_project ON vm_compose_backups(vm,project,state);

CREATE TRIGGER vm_compose_backups_no_delete BEFORE DELETE ON vm_compose_backups
WHEN OLD.state='settled' BEGIN
    SELECT RAISE(ABORT,'settled compose backup cannot be deleted');
END;
CREATE TRIGGER vm_compose_backups_no_unsettle BEFORE UPDATE ON vm_compose_backups
WHEN OLD.state='settled' BEGIN
    SELECT RAISE(ABORT,'settled compose backup is immutable');
END;

CREATE TABLE vm_compose_backup_ops (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    backup_id BLOB NOT NULL UNIQUE REFERENCES vm_compose_backups(id) CHECK(length(backup_id)=16),
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    request BLOB NOT NULL CHECK(length(request)=16),
    vm BLOB NOT NULL REFERENCES vm_static_instances(id) CHECK(length(vm)=16),
    project TEXT NOT NULL,
    generation BLOB NOT NULL REFERENCES vm_generations(id) CHECK(length(generation)=16),
    boot INTEGER NOT NULL CHECK(boot>0),
    revision INTEGER NOT NULL CHECK(revision>0),
    volume TEXT NOT NULL CHECK(length(volume) BETWEEN 1 AND 16),
    database TEXT NOT NULL CHECK(length(database) BETWEEN 1 AND 64),
    max_bytes INTEGER NOT NULL CHECK(max_bytes BETWEEN 1 AND 536870912),
    valid_through_ms INTEGER NOT NULL CHECK(valid_through_ms>=0),
    activity_id INTEGER UNIQUE REFERENCES activity(id) ON DELETE SET NULL,
    sent INTEGER NOT NULL DEFAULT 0 CHECK(sent IN (0,1)),
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','succeeded','failed','interrupted')),
    failure TEXT,
    UNIQUE(principal,request),
    FOREIGN KEY(vm,project) REFERENCES vm_compose_projects(vm,project),
    CHECK((status IN ('failed','interrupted'))=(failure IS NOT NULL))
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX vm_compose_backup_inflight ON vm_compose_backup_ops(vm,project) WHERE status='pending';

DROP TRIGGER vm_compose_retire;
CREATE TRIGGER vm_compose_retire AFTER UPDATE OF state ON vm_static_instances
WHEN OLD.state IN (2,3) AND NEW.state=1 BEGIN
    UPDATE vm_compose_boots SET retired=1 WHERE vm=NEW.id AND retired=0;
    UPDATE vm_compose_projects SET recovery_id=(
        SELECT id FROM vm_compose_operations o WHERE o.vm=vm_compose_projects.vm AND o.project=vm_compose_projects.project
            AND o.kind='deploy' AND o.sent=1 AND o.receipt IS NULL AND o.closed IS NULL)
        WHERE vm=NEW.id AND recovery_id IS NULL;
    UPDATE vm_compose_operations SET closed='retired_boot'
        WHERE vm=NEW.id AND receipt IS NULL AND closed IS NULL;
    UPDATE vm_compose_backup_ops SET status='interrupted', failure='retired_boot'
        WHERE vm=NEW.id AND status='pending';
    UPDATE vm_compose_backups SET state='partial_uncertain', guest_staging_charge=0
        WHERE vm=NEW.id AND state='staging';
END;
";

pub(super) const MIGRATE_V12: &str = "
CREATE TABLE activity_v12 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 23),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 52),
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
INSERT INTO activity_v12(id,principal,actor,vm_owner,vm,action,started_ms,status,reason,
    finished_ms,terminal_order,cleanup_confirmed,publication_failure)
    SELECT id,principal,actor,vm_owner,vm,action,started_ms,status,reason,
        finished_ms,terminal_order,cleanup_confirmed,publication_failure FROM activity;
INSERT INTO sqlite_sequence(name,seq) SELECT 'activity_v12',seq FROM sqlite_sequence
    WHERE name='activity' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='activity_v12');
UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='activity'),0))
    WHERE name='activity_v12';
DROP TABLE activity;
ALTER TABLE activity_v12 RENAME TO activity;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND (action<=6 OR action IN (12,18));

CREATE TABLE vm_compose_projects (
    vm BLOB NOT NULL REFERENCES vm_static_instances(id),
    project TEXT NOT NULL,
    desired TEXT NOT NULL DEFAULT 'none' CHECK(desired IN ('none','started','stopped','down')),
    recovery_id BLOB CHECK(length(recovery_id)=16),
    rendered_revision INTEGER CHECK(rendered_revision>0),
    rendered_release BLOB CHECK(length(rendered_release)=16),
    last_revision INTEGER NOT NULL DEFAULT 0 CHECK(last_revision>=0),
    committed INTEGER,
    committed_release BLOB,
    settled_generation BLOB,
    settled_boot INTEGER,
    observed TEXT,
    last_receipt TEXT,
    PRIMARY KEY(vm,project),
    FOREIGN KEY(vm,project,committed_release) REFERENCES vm_compose_releases(vm,project,id),
    FOREIGN KEY(vm,project,rendered_release) REFERENCES vm_compose_releases(vm,project,id),
    CHECK((rendered_revision IS NULL)=(rendered_release IS NULL)),
    CHECK((committed IS NULL)=(committed_release IS NULL))
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_compose_releases (
    vm BLOB NOT NULL,
    project TEXT NOT NULL,
    id BLOB NOT NULL CHECK(length(id)=16),
    digest TEXT NOT NULL CHECK(length(digest)=64),
    manifest BLOB NOT NULL CHECK(length(manifest) BETWEEN 1 AND 131072),
    PRIMARY KEY(vm,project,id),
    FOREIGN KEY(vm,project) REFERENCES vm_compose_projects(vm,project)
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_compose_volumes (
    vm BLOB NOT NULL,
    project TEXT NOT NULL,
    slot TEXT NOT NULL,
    uid INTEGER NOT NULL CHECK(uid>0),
    gid INTEGER NOT NULL CHECK(gid>0),
    charge INTEGER NOT NULL DEFAULT 1 CHECK(charge=1),
    PRIMARY KEY(vm,project,slot),
    FOREIGN KEY(vm,project) REFERENCES vm_compose_projects(vm,project)
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_compose_secrets (
    vm BLOB NOT NULL,
    project TEXT NOT NULL,
    slot TEXT NOT NULL,
    epoch INTEGER NOT NULL CHECK(epoch>0),
    reference BLOB NOT NULL UNIQUE CHECK(length(reference)=16),
    PRIMARY KEY(vm,project,slot,epoch),
    FOREIGN KEY(vm,project) REFERENCES vm_compose_projects(vm,project)
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_compose_release_epochs (
    vm BLOB NOT NULL, project TEXT NOT NULL, release BLOB NOT NULL, slot TEXT NOT NULL, epoch INTEGER NOT NULL,
    PRIMARY KEY(vm,project,release,slot,epoch),
    FOREIGN KEY(vm,project,release) REFERENCES vm_compose_releases(vm,project,id),
    FOREIGN KEY(vm,project,slot,epoch) REFERENCES vm_compose_secrets(vm,project,slot,epoch)
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_compose_boots (
    vm BLOB NOT NULL REFERENCES vm_static_instances(id), generation BLOB NOT NULL, boot INTEGER NOT NULL,
    retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN (0,1)),
    PRIMARY KEY(vm,generation,boot)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX vm_compose_current_boot ON vm_compose_boots(vm) WHERE retired=0;
CREATE TABLE vm_compose_compacted_keys (
    principal TEXT NOT NULL, request BLOB NOT NULL CHECK(length(request)=16),
    valid_through_ms INTEGER NOT NULL, PRIMARY KEY(principal,request)
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_compose_operations (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    principal TEXT NOT NULL,
    request BLOB NOT NULL CHECK(length(request)=16),
    vm BLOB NOT NULL,
    project TEXT NOT NULL,
    generation BLOB NOT NULL CHECK(length(generation)=16),
    boot INTEGER NOT NULL CHECK(boot>0),
    kind TEXT NOT NULL CHECK(kind IN ('deploy','start','stop','down')),
    release BLOB NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    valid_through_ms INTEGER NOT NULL CHECK(valid_through_ms>=0),
    emergency INTEGER NOT NULL DEFAULT 0 CHECK(emergency IN (0,1)),
    activity_id INTEGER UNIQUE REFERENCES activity(id) ON DELETE SET NULL,
    sent INTEGER NOT NULL DEFAULT 0 CHECK(sent IN (0,1)),
    receipt TEXT,
    closed TEXT CHECK(closed IN ('refused','secret_failed','retired_boot','not_applied','compose_recovery_required')),
    secret_refs TEXT NOT NULL,
    secret_sent INTEGER NOT NULL DEFAULT 0 CHECK(secret_sent BETWEEN 0 AND 15),
    UNIQUE(principal,request),
    FOREIGN KEY(vm,project,release) REFERENCES vm_compose_releases(vm,project,id)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX vm_compose_emergency ON vm_compose_operations(vm,project) WHERE emergency=1;
CREATE UNIQUE INDEX vm_compose_inflight ON vm_compose_operations(vm,project) WHERE receipt IS NULL AND closed IS NULL;
-- Running/Recovering -> Stopped is committed only after host release proof.
CREATE TRIGGER vm_compose_retire AFTER UPDATE OF state ON vm_static_instances
WHEN OLD.state IN (2,3) AND NEW.state=1 BEGIN
    UPDATE vm_compose_boots SET retired=1 WHERE vm=NEW.id AND retired=0;
    UPDATE vm_compose_projects SET recovery_id=(
        SELECT id FROM vm_compose_operations o WHERE o.vm=vm_compose_projects.vm AND o.project=vm_compose_projects.project
            AND o.kind='deploy' AND o.sent=1 AND o.receipt IS NULL AND o.closed IS NULL)
        WHERE vm=NEW.id AND recovery_id IS NULL;
    UPDATE vm_compose_operations SET closed='retired_boot'
        WHERE vm=NEW.id AND receipt IS NULL AND closed IS NULL;
END;
";

pub(crate) fn enroll(store: &VmStore, releases: &[Enrollment]) -> Result<()> {
    enroll_with_backup_targets(store, releases, &[])
}

pub(crate) fn enroll_with_backup_targets(
    store: &VmStore,
    releases: &[Enrollment],
    targets: &[compose_backup::TargetEnrollment],
) -> Result<()> {
    compose_backup::validate_targets(targets)?;
    store.transact(|db| {
        for release in releases {
            let target = &release.target;
            let project = &target.project;
            let vm = target.vm;
            ensure!(
                count(db, "SELECT count(*) FROM vm_static_instances WHERE id=?1 AND state IN (1,2,3)", [vm])? == 1,
                "Compose enrollment requires a committed static VM"
            );
            db.execute("INSERT OR IGNORE INTO vm_compose_projects(vm,project) VALUES(?1,?2)", params![vm, project])?;
            ensure!(count(db, "SELECT count(*) FROM vm_compose_projects WHERE vm=?1", [vm])? <= MAX_PROJECTS as i64,
                "Compose project limit");
            db.execute(
                "INSERT OR IGNORE INTO vm_compose_releases VALUES(?1,?2,?3,?4,?5)",
                params![vm, project, release.id, release.digest, release.bytes],
            )?;
            ensure!(
                count(db, "SELECT count(*) FROM vm_compose_releases WHERE vm=?1 AND project=?2 AND id=?3 AND digest=?4 AND manifest=?5",
                    params![vm, project, release.id, release.digest, release.bytes])? == 1,
                "Compose release ID already bound"
            );
            for volume in &release.project.manifest().volumes {
                db.execute(
                    "INSERT OR IGNORE INTO vm_compose_volumes(vm,project,slot,uid,gid) VALUES(?1,?2,?3,?4,?5)",
                    params![vm, project, volume.name, volume.uid, volume.gid],
                )?;
                ensure!(
                    count(db, "SELECT count(*) FROM vm_compose_volumes WHERE vm=?1 AND project=?2 AND slot=?3 AND uid=?4 AND gid=?5",
                        params![vm, project, volume.name, volume.uid, volume.gid])? == 1,
                    "Compose volume ownership changed"
                );
            }
            ensure!(
                count(db, "SELECT coalesce(sum(charge),0) FROM vm_compose_volumes WHERE vm=?1", [vm])? <= MAX_VOLUMES as i64,
                "Compose volume limit"
            );
            for secret in &release.secrets {
                let latest: Option<u64> = db.query_row(
                    "SELECT max(epoch) FROM vm_compose_secrets WHERE vm=?1 AND project=?2 AND slot=?3",
                    params![vm, project, secret.slot], |row| row.get(0),
                )?;
                let existing = count(db, "SELECT count(*) FROM vm_compose_secrets WHERE vm=?1 AND project=?2 AND slot=?3 AND epoch=?4 AND reference=?5",
                    params![vm, project, secret.slot, secret.epoch, secret.reference])? == 1;
                if !existing {
                    ensure!(latest.is_none_or(|epoch| secret.epoch > epoch), "Compose secret epoch already bound");
                    db.execute("INSERT INTO vm_compose_secrets VALUES(?1,?2,?3,?4,?5)",
                        params![vm, project, secret.slot, secret.epoch, secret.reference])?;
                }
                db.execute("INSERT OR IGNORE INTO vm_compose_release_epochs VALUES(?1,?2,?3,?4,?5)",
                    params![vm, project, release.id, secret.slot, secret.epoch])?;
            }
        }
        for target in targets {
            enroll_backup_target(
                db,
                target.vm,
                &target.project,
                &target.volume,
                &target.database,
                target.max_bytes,
            )?;
        }
        require_headroom(db)
    })
}

pub(crate) fn register_boot(db: &Connection, vm: Id, generation: Id, boot: u64) -> Result<()> {
    ensure!(
        count(
            db,
            "SELECT count(*) FROM vm_static_instances s JOIN vm_generations g ON g.static_id=s.id
        WHERE s.id=?1 AND g.id=?2 AND g.active=1 AND s.state=2",
            params![vm, generation]
        )? == 1,
        "Compose generation unavailable"
    );
    db.execute(
        "INSERT INTO vm_compose_boots(vm,generation,boot) VALUES(?1,?2,?3)
        ON CONFLICT(vm,generation,boot) DO NOTHING",
        params![vm, generation, boot],
    )?;
    ensure!(live_boot(db, vm, generation, boot)?, "Compose boot retired");
    Ok(())
}

pub(crate) fn live_boot(db: &Connection, vm: Id, generation: Id, boot: u64) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_compose_boots WHERE vm=?1 AND generation=?2 AND boot=?3 AND retired=0",
        params![vm, generation, boot],
    )? == 1)
}

pub(crate) fn boot(db: &Connection, principal: &str, target: &Target) -> Result<Option<Id>> {
    Ok(db
        .query_row(
            "SELECT g.id FROM vm_compose_projects p JOIN vm_static_instances s ON s.id=p.vm
         JOIN vm_generations g ON g.static_id=s.id AND g.active=1
         WHERE p.vm=?1 AND p.project=?2 AND s.principal=?3 AND s.state=2",
            params![target.vm, target.project, principal],
            |row| row.get(0),
        )
        .optional()?)
}

pub(crate) fn status(
    db: &Connection,
    principal: &str,
    target: &Target,
) -> Result<Option<ProjectStatus>> {
    Ok(db.query_row(
        "SELECT p.desired,p.committed,p.observed,p.settled_generation,p.settled_boot,p.last_receipt,
            (SELECT id FROM vm_compose_operations o WHERE o.vm=p.vm AND o.project=p.project AND o.receipt IS NULL AND o.closed IS NULL),p.recovery_id
         FROM vm_compose_projects p JOIN vm_static_instances s ON s.id=p.vm
         WHERE p.vm=?1 AND p.project=?2 AND s.principal=?3",
        params![target.vm,target.project,principal],
        |r| {
            let raw: Option<String> = r.get(2)?;
            let last: Option<String> = r.get(5)?;
            Ok(ProjectStatus {
                desired: parse_desired(&r.get::<_, String>(0)?)?,
                revision: r.get(1)?,
                observation: raw.map(|raw| decode(&raw)).transpose()?,
                generation: r.get(3)?,
                boot: r.get(4)?,
                pending: r.get(6)?, recovery_id:r.get(7)?,
                last_receipt: last.map(|raw| decode(&raw)).transpose()?,
            })
        },
    ).optional()?)
}

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> rusqlite::Result<T> {
    serde_json::from_str(raw).map_err(|_| rusqlite::Error::InvalidQuery)
}

pub(crate) fn replay(db: &Connection, principal: &str, request: Id) -> Result<Option<Intent>> {
    Ok(db.query_row(
        "SELECT o.id,o.vm,o.project,o.generation,o.boot,o.kind,o.release,o.revision,o.sent,o.receipt,
                r.digest,r.manifest,o.secret_refs,o.secret_sent,o.closed
         FROM vm_compose_operations o JOIN vm_compose_releases r
           ON (r.vm,r.project,r.id)=(o.vm,o.project,o.release)
         WHERE o.principal=?1 AND o.request=?2", params![principal,request], |r| {
            let raw: Option<String> = r.get(9)?;
            Ok(Intent {
                id:r.get(0)?, target:Target{vm:r.get(1)?,project:r.get(2)?},
                generation:r.get(3)?, boot:r.get(4)?, kind:Kind::parse(&r.get::<_, String>(5)?)?,
                release:r.get(6)?, revision:r.get(7)?, sent:r.get(8)?,
                receipt:raw.map(|raw| decode(&raw)).transpose()?,
                digest:r.get(10)?, bytes:r.get(11)?, secrets:decode(&r.get::<_, String>(12)?)?, secret_sent:r.get(13)?, closed:r.get(14)?,
            })
        },
    ).optional()?)
}

// The receipt identified by recovery_id is pinned across retry-window collection.
pub(crate) fn cleanup_binding(
    db: &Connection,
    target: &Target,
    generation: Id,
) -> Result<Option<(Id, u64)>> {
    Ok(db.query_row(
        "SELECT coalesce(p.committed_release,o.release,CASE WHEN p.settled_generation=?3 THEN p.rendered_release END),
                coalesce(p.committed,o.revision,CASE WHEN p.settled_generation=?3 THEN p.rendered_revision END)
         FROM vm_compose_projects p LEFT JOIN vm_compose_operations o ON o.id=p.recovery_id AND o.generation=?3
         WHERE p.vm=?1 AND p.project=?2 AND coalesce(p.committed,o.revision,CASE WHEN p.settled_generation=?3 THEN p.rendered_revision END) IS NOT NULL",
        params![target.vm,target.project,generation], |r|Ok((r.get(0)?,r.get(1)?)),
    ).optional()?)
}

pub(crate) fn admit(
    db: &Connection,
    principal: &str,
    request: &Mutation,
    generation: Id,
    boot: u64,
    now: i64,
) -> Result<Result<Intent, Fault>> {
    let target = &request.target;
    let Some(status) = status(db, principal, target)? else {
        return Ok(Err(Fault::NotFound));
    };
    if let Some(id) = status.recovery_id
        && matches!(request.kind, Kind::Start | Kind::Deploy)
    {
        let key = db.query_row(
            "SELECT request FROM vm_compose_operations WHERE id=?1 AND principal=?2",
            params![id, principal],
            |r| r.get(0),
        )?;
        return Ok(Err(Fault::RecoveryRequired(key)));
    }
    if status.pending.is_some()
        || count(
            db,
            "SELECT count(*) FROM vm_compose_backup_ops WHERE vm=?1 AND project=?2 AND status='pending'",
            params![target.vm, target.project],
        )? != 0
    {
        return Ok(Err(Fault::Busy));
    }
    // Stop and Down remain usable under normal application capacity pressure.
    if matches!(request.kind, Kind::Deploy | Kind::Start) {
        require_headroom(db)?;
    }
    let high_water = super::retry_time(db, now)?;
    let valid_through = request.key.issued_at_ms + crate::vm::RETRY_WINDOW_MS;
    if request.key.issued_at_ms > now
        || valid_through < high_water
        || compacted_key(db, principal, request.key.key)?
    {
        return Ok(Err(Fault::StaleKey));
    }
    db.execute("DELETE FROM vm_compose_operations WHERE valid_through_ms<?1 AND (receipt IS NOT NULL OR closed IS NOT NULL) AND id NOT IN (SELECT recovery_id FROM vm_compose_projects WHERE recovery_id IS NOT NULL)",[high_water])?;
    let receipts = count(
        db,
        "SELECT count(*) FROM vm_compose_operations WHERE principal=?1 AND emergency=0",
        [principal],
    )?;
    let limit = if matches!(request.kind, Kind::Deploy | Kind::Start) {
        240
    } else {
        256
    };
    let cleanup = matches!(request.kind, Kind::Stop | Kind::Down);
    let emergency = cleanup && receipts >= limit;
    if !cleanup && receipts >= limit {
        return Ok(Err(Fault::ReplayFull));
    }
    let (release, revision) = if let Some(release) = request.release {
        let next: u64 = db.query_row(
            "SELECT last_revision+1 FROM vm_compose_projects WHERE vm=?1 AND project=?2",
            params![target.vm, target.project],
            |r| r.get(0),
        )?;
        (release, next)
    } else if cleanup {
        let Some(binding) = cleanup_binding(db, target, generation)? else {
            return Ok(Err(Fault::NotReady));
        };
        binding
    } else {
        let Some(revision) = status.revision else {
            return Ok(Err(Fault::NotReady));
        };
        let release = db.query_row(
            "SELECT committed_release FROM vm_compose_projects WHERE vm=?1 AND project=?2",
            params![target.vm, target.project],
            |r| r.get(0),
        )?;
        (release, revision)
    };
    let Some(bytes) = db
        .query_row(
            "SELECT manifest FROM vm_compose_releases WHERE vm=?1 AND project=?2 AND id=?3",
            params![target.vm, target.project, release],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
    else {
        return Ok(Err(Fault::NotFound));
    };
    let secret_refs = if matches!(request.kind, Kind::Deploy | Kind::Start) {
        let project = arx_guest::compose::Project::parse(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid enrolled Compose manifest"))?;
        let slots = &project.manifest().secrets;
        let collected = secrets(db, target, release, slots)?;
        if collected.len() != slots.len() {
            return Ok(Err(Fault::NotReady));
        }
        collected
    } else {
        Vec::new()
    };
    db.execute(
        "DELETE FROM vm_compose_compacted_keys WHERE valid_through_ms<?1",
        [high_water],
    )?;
    if emergency {
        // The ordinary replay window stays intact. Rotate only this project's
        // extra terminal cleanup key. A compacted key can never become fresh.
        let old:Option<Id>=db.query_row("SELECT request FROM vm_compose_operations WHERE vm=?1 AND project=?2 AND emergency=1",
            params![target.vm,target.project],|r|r.get(0)).optional()?;
        if let Some(old) = old {
            db.execute(
                "INSERT INTO vm_compose_compacted_keys(principal,request,valid_through_ms)
                 SELECT principal,request,valid_through_ms FROM vm_compose_operations WHERE principal=?1 AND request=?2",
                params![principal,old],
            )?;
            ensure!(db.execute("DELETE FROM vm_compose_operations WHERE principal=?1 AND request=?2 AND emergency=1 AND (closed IS NOT NULL OR receipt IS NOT NULL)",
                params![principal,old])?==1,"Compose cleanup still active");
        }
    }
    if request.kind == Kind::Deploy {
        db.execute(
            "UPDATE vm_compose_projects SET last_revision=?3 WHERE vm=?1 AND project=?2",
            params![target.vm, target.project, revision],
        )?;
    }
    db.execute(
        "INSERT INTO activity(principal,vm,action,started_ms,actor) VALUES(?1,?2,?3,?4,'agent')",
        params![principal, target.vm, request.kind.action(), now],
    )?;
    let activity = db.last_insert_rowid();
    let id = nonce()?;
    db.execute(
        "INSERT INTO vm_compose_operations(id,principal,request,vm,project,generation,boot,kind,release,revision,secret_refs,valid_through_ms,activity_id,emergency)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![id,principal,request.key.key,target.vm,target.project,generation,boot,request.kind.wire(),release,revision,serde_json::to_string(&secret_refs)?,valid_through,activity,emergency],
    )?;
    Ok(Ok(replay(db, principal, request.key.key)?.ok_or_else(
        || anyhow::anyhow!("Compose intent unavailable"),
    )?))
}

fn secrets(
    db: &Connection,
    target: &Target,
    release: Id,
    slots: &[String],
) -> Result<Vec<SecretRef>> {
    let mut refs = Vec::with_capacity(slots.len());
    for slot in slots {
        if let Some(secret) = db.query_row(
            "SELECT epoch,reference FROM vm_compose_secrets JOIN vm_compose_release_epochs USING(vm,project,slot,epoch) WHERE vm=?1 AND project=?2 AND slot=?3 AND release=?4 ORDER BY epoch DESC LIMIT 1",
            params![target.vm,target.project,slot,release],
            |r| Ok(SecretRef { slot:slot.clone(), epoch:r.get(0)?, reference:r.get(1)? }),
        ).optional()? {
            refs.push(secret);
        }
    }
    Ok(refs)
}

fn finish_audit(db: &Connection, id: Id, status: crate::vm::Status, now: i64) -> Result<()> {
    let activity: i64 = db.query_row(
        "SELECT activity_id FROM vm_compose_operations WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    super::finish(db, activity, status, now, None)
}

pub(super) fn finish_retired(db: &Connection, vm: Id, now: i64) -> Result<()> {
    let mut stmt = db.prepare(
        "SELECT o.activity_id FROM vm_compose_operations o JOIN activity a ON a.id=o.activity_id
       WHERE o.vm=?1 AND o.closed='retired_boot' AND a.status=0
       UNION ALL
       SELECT b.activity_id FROM vm_compose_backup_ops b JOIN activity a ON a.id=b.activity_id
       WHERE b.vm=?1 AND b.status='interrupted' AND a.status=0",
    )?;
    let ids = stmt
        .query_map([vm], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        super::finish(db, id, crate::vm::Status::Interrupted, now, None)?;
    }
    Ok(())
}

pub(crate) fn close(db: &Connection, intent: &Intent, reason: &str, now: i64) -> Result<()> {
    ensure!(db.execute("UPDATE vm_compose_operations SET closed=?2 WHERE id=?1 AND receipt IS NULL AND closed IS NULL",
        params![intent.id,reason])? == 1, "Compose intent already closed");
    finish_audit(db, intent.id, crate::vm::Status::Failed, now)
}

pub(crate) fn bind_previous_render(db: &Connection, intent: &Intent, revision: u64) -> Result<()> {
    ensure!(
        live_boot(db, intent.target.vm, intent.generation, intent.boot)?,
        "Compose cleanup boot changed"
    );
    let changed=db.execute("UPDATE vm_compose_operations AS o SET release=p.rendered_release,revision=p.rendered_revision
        FROM vm_compose_projects p WHERE o.id=?1 AND o.vm=p.vm AND o.project=p.project AND o.sent=1
        AND o.receipt IS NULL AND o.closed IS NULL AND o.kind IN ('stop','down') AND p.committed IS NULL
        AND p.settled_generation=?2 AND p.rendered_revision=?3",
        params![intent.id,intent.generation,revision])?;
    ensure!(changed == 1, "Compose cleanup revision not attributed");
    Ok(())
}

pub(crate) fn mark_sent(db: &Connection, intent: &Intent) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE vm_compose_operations SET sent=1 WHERE id=?1 AND sent=0 AND receipt IS NULL AND closed IS NULL",
            [intent.id]
        )? == 1,
        "Compose intent already submitted"
    );
    Ok(())
}

pub(crate) fn settle(
    db: &Connection,
    intent: &Intent,
    receipt: &OperationView,
    observed: Option<&arx_guest::compose::ProjectView>,
    now: i64,
) -> Result<()> {
    ensure!(
        db.execute("UPDATE vm_compose_operations SET receipt=?2 WHERE id=?1 AND sent=1 AND receipt IS NULL AND closed IS NULL",
            params![intent.id, serde_json::to_string(receipt)?])? == 1,
        "Compose intent is not unsettled"
    );
    db.execute(
        "UPDATE vm_compose_projects SET last_receipt=?3 WHERE vm=?1 AND project=?2",
        params![
            intent.target.vm,
            intent.target.project,
            serde_json::to_string(receipt)?
        ],
    )?;
    if intent.kind == Kind::Deploy && receipt.state == OperationState::Failed {
        db.execute("UPDATE vm_compose_projects SET recovery_id=?3 WHERE vm=?1 AND project=?2 AND committed IS NULL",
                params![intent.target.vm,intent.target.project,intent.id])?;
    }
    if receipt.state == OperationState::Succeeded {
        let project = observed.ok_or_else(|| anyhow::anyhow!("Compose observation missing"))?;
        let cleanup = matches!(intent.kind, Kind::Stop | Kind::Down);
        let changed=db.execute(
            "UPDATE vm_compose_projects SET desired=?3,
                committed=CASE WHEN ?9 THEN committed ELSE ?4 END,
                committed_release=CASE WHEN ?9 THEN committed_release ELSE ?5 END,
                rendered_revision=?4,rendered_release=?5,recovery_id=NULL,
                observed=?6,settled_generation=?7,settled_boot=?8 WHERE vm=?1 AND project=?2 AND (NOT ?9 OR committed IS ?10)",
            params![
                intent.target.vm,
                intent.target.project,
                desired_wire(project.desired),
                intent.revision,
                intent.release,
                serde_json::to_string(project)?,
                intent.generation,
                intent.boot, cleanup, project.committed_revision
            ],
        )?;
        ensure!(changed == 1, "Compose committed revision changed");
    }
    finish_audit(
        db,
        intent.id,
        if receipt.state == OperationState::Succeeded {
            crate::vm::Status::Succeeded
        } else {
            crate::vm::Status::Failed
        },
        now,
    )
}

pub(crate) fn clear_recovery(db: &Connection, intent: &Intent) -> Result<()> {
    ensure!(db.execute("UPDATE vm_compose_projects SET recovery_id=NULL WHERE vm=?1 AND project=?2 AND recovery_id=?3",
        params![intent.target.vm,intent.target.project,intent.id])?==1,"Compose recovery changed");
    Ok(())
}

pub(crate) fn retire_unapplied(db: &Connection, intent: &Intent) -> Result<()> {
    clear_recovery(db, intent)?;
    ensure!(db.execute(
        "UPDATE vm_compose_operations SET closed='not_applied' WHERE id=?1 AND closed='retired_boot'",
        [intent.id],
    )? == 1, "Compose retirement changed");
    Ok(())
}

pub(crate) fn recover_committed(
    db: &Connection,
    intent: &Intent,
    observed: &arx_guest::compose::ProjectView,
    receipt: Option<&OperationView>,
) -> Result<()> {
    clear_recovery(db, intent)?;
    db.execute("UPDATE vm_compose_projects SET committed=?3,committed_release=?4,desired=?5,
        last_receipt=?6,observed=NULL,settled_boot=NULL,settled_generation=NULL WHERE vm=?1 AND project=?2",
        params![intent.target.vm,intent.target.project,intent.revision,intent.release,desired_wire(observed.desired),receipt.map(serde_json::to_string).transpose()?])?;
    Ok(())
}

fn parse_desired(raw: &str) -> rusqlite::Result<Desired> {
    Ok(match raw {
        "none" => Desired::None,
        "started" => Desired::Started,
        "stopped" => Desired::Stopped,
        "down" => Desired::Down,
        _ => return Err(rusqlite::Error::InvalidQuery),
    })
}

fn desired_wire(desired: Desired) -> &'static str {
    match desired {
        Desired::None => "none",
        Desired::Started => "started",
        Desired::Stopped => "stopped",
        Desired::Down => "down",
    }
}

pub(crate) fn service_names(db: &Connection, target: &Target) -> Result<Vec<String>> {
    let mut query = db.prepare(
        "SELECT DISTINCT json_extract(service.value,'$.name') AS name
         FROM vm_compose_releases r, json_each(CAST(r.manifest AS TEXT),'$.services') AS service
         WHERE r.vm=?1 AND r.project=?2 ORDER BY name LIMIT 128",
    )?;
    let names = query.query_map(params![target.vm, target.project], |r| r.get(0))?;
    Ok(names.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn mark_secret_sent(db: &Connection, intent: &Intent, bit: u8) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE vm_compose_operations SET secret_sent=secret_sent | ?2
             WHERE id=?1 AND sent=0 AND receipt IS NULL AND closed IS NULL AND secret_sent & ?2=0",
            params![intent.id, bit],
        )? == 1,
        "Compose secret delivery already submitted"
    );
    Ok(())
}

pub(super) fn compacted_key(db: &Connection, principal: &str, key: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_compose_compacted_keys WHERE principal=?1 AND request=?2",
        params![principal, key],
    )? != 0)
}

pub(super) fn uses_key(db: &Connection, principal: &str, key: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT EXISTS(
            SELECT 1 FROM vm_compose_operations WHERE principal=?1 AND request=?2
            UNION ALL
            SELECT 1 FROM vm_compose_backup_ops WHERE principal=?1 AND request=?2
        )",
        params![principal, key],
    )? != 0)
}

/// Owner-only enrollment of a project's fixed SQLite backup target (volume slot,
/// database basename, and per-backup size cap). Never exposed to agents.
pub(crate) fn enroll_backup_target(
    db: &Connection,
    vm: Id,
    project: &str,
    volume: &str,
    database: &str,
    max_bytes: u64,
) -> Result<()> {
    compose_backup::validate_target(project, volume, database, max_bytes)?;
    ensure!(
        count(
            db,
            "SELECT count(*) FROM vm_compose_volumes WHERE vm=?1 AND project=?2 AND slot=?3",
            params![vm, project, volume],
        )? == 1,
        "Compose backup target requires an enrolled project volume"
    );
    db.execute(
        "INSERT INTO vm_compose_backup_targets(vm,project,volume,database,max_bytes)
         VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(vm,project) DO UPDATE SET volume=excluded.volume,database=excluded.database,max_bytes=excluded.max_bytes",
        params![vm, project, volume, database, max_bytes],
    )?;
    require_headroom(db)
}

pub(crate) fn backup_replay(
    db: &Connection,
    principal: &str,
    request: Id,
) -> Result<Option<compose_backup::BackupIntent>> {
    Ok(db
        .query_row(
            "SELECT o.id,o.backup_id,o.vm,o.project,o.generation,o.boot,o.revision,
                    o.volume,o.database,o.max_bytes,o.sent,o.status,o.failure,
                    b.size_bytes,b.sha256,b.state,b.host_staging_charge,b.retained_charge,b.file_id IS NOT NULL
             FROM vm_compose_backup_ops o
             JOIN vm_compose_backups b ON b.id=o.backup_id
             WHERE o.principal=?1 AND o.request=?2",
            params![principal, request],
            |r| {
                let sha: Option<[u8; 32]> = r.get(14)?;
                Ok(compose_backup::BackupIntent {
                    id: r.get(0)?,
                    backup_id: r.get(1)?,
                    target: Target {
                        vm: r.get(2)?,
                        project: r.get(3)?,
                    },
                    generation: r.get(4)?,
                    boot: r.get(5)?,
                    revision: r.get(6)?,
                    volume: r.get(7)?,
                    database: r.get(8)?,
                    max_bytes: r.get(9)?,
                    sent: r.get(10)?,
                    status: compose_backup::BackupOpStatus::parse(&r.get::<_, String>(11)?)?,
                    failure: r.get(12)?,
                    size_bytes: r.get(13)?,
                    sha256: sha,
                    backup_state: compose_backup::BackupRecordState::parse(
                        &r.get::<_, String>(15)?,
                    )?,
                    has_staging_witness: r.get(18)?,
                    host_staging_charge: r.get(16)?,
                    retained_charge: r.get(17)?,
                })
            },
        )
        .optional()?)
}

/// True when `id` names a live backup record of a static VM this principal
/// owns. Never true for another principal's VM or cleaned partials.
pub(crate) fn owned_backup(db: &Connection, principal: &str, vm: Id, id: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_compose_backups b JOIN vm_static_instances s ON s.id=b.vm
         WHERE b.id=?3 AND b.vm=?2 AND s.principal=?1 AND b.state<>'cleaned'",
        params![principal, vm, id],
    )? != 0)
}

pub(crate) fn settled_backup_count(db: &Connection, target: &Target) -> Result<i64> {
    count(
        db,
        "SELECT count(*) FROM vm_compose_backups b WHERE vm=?1 AND project=?2 AND state='settled'
         AND NOT EXISTS (SELECT 1 FROM vm_compose_backup_retirements r
                         WHERE r.backup_id=b.id AND r.finished_ms IS NOT NULL)",
        params![target.vm, target.project],
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn admit_backup(
    db: &Connection,
    principal: &str,
    target: &Target,
    key: crate::vm::RequestId,
    generation: Id,
    boot: u64,
    now: i64,
    check_disk: impl FnOnce(&Connection, u64) -> Result<Result<(), crate::vm::Refusal>>,
) -> Result<Result<compose_backup::BackupIntent, compose_backup::BackupFault>> {
    use compose_backup::{BackupFault, MAX_RETAINED_BACKUPS_PER_PROJECT};

    if let Some(known) = backup_replay(db, principal, key.key)? {
        return Ok(if known.target == *target {
            Ok(known)
        } else {
            Err(BackupFault::KeyCollision)
        });
    }
    if count(
        db,
        "SELECT EXISTS(
            SELECT 1 FROM vm_operations WHERE principal=?1 AND request_key=?2
            UNION ALL
            SELECT 1 FROM vm_compose_operations WHERE principal=?1 AND request=?2
        )",
        params![principal, key.key],
    )? != 0
    {
        return Ok(Err(BackupFault::KeyCollision));
    }

    let Some(proj) = status(db, principal, target)? else {
        return Ok(Err(BackupFault::NotFound));
    };
    if proj.recovery_id.is_some() {
        return Ok(Err(BackupFault::RecoveryRequired));
    }
    if proj.pending.is_some()
        || count(
            db,
            "SELECT count(*) FROM vm_compose_backup_ops WHERE vm=?1 AND project=?2 AND status='pending'",
            params![target.vm, target.project],
        )? != 0
    {
        return Ok(Err(BackupFault::Busy));
    }
    let Some(revision) = proj.revision else {
        return Ok(Err(BackupFault::NotReady));
    };
    let Some((volume, database, max_bytes)): Option<(String, String, u64)> = db
        .query_row(
            "SELECT volume,database,max_bytes FROM vm_compose_backup_targets WHERE vm=?1 AND project=?2",
            params![target.vm, target.project],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
    else {
        return Ok(Err(BackupFault::NotReady));
    };

    // Enforce retention cap (2 settled backups per project) before reserving staging.
    if settled_backup_count(db, target)? >= MAX_RETAINED_BACKUPS_PER_PROJECT {
        return Ok(Err(BackupFault::BackupLimit));
    }

    require_headroom(db)?;

    let high_water = super::retry_time(db, now)?;
    let valid_through = key.issued_at_ms + crate::vm::RETRY_WINDOW_MS;
    if key.issued_at_ms > now
        || valid_through < high_water
        || compacted_key(db, principal, key.key)?
    {
        return Ok(Err(BackupFault::StaleKey));
    }

    let receipts = count(
        db,
        "SELECT count(*) FROM vm_compose_backup_ops WHERE principal=?1",
        [principal],
    )?;
    if receipts >= 240 {
        // Reclaim expired terminal backup op keys and prune orphaned cleaned
        // backup rows; settled copies and uncertain partials are NEVER deleted.
        db.execute(
            "DELETE FROM vm_compose_backup_ops WHERE valid_through_ms<?1 AND status<>'pending'",
            [high_water],
        )?;
        prune_orphaned_cleaned_backups(db)?;
        if count(
            db,
            "SELECT count(*) FROM vm_compose_backup_ops WHERE principal=?1",
            [principal],
        )? >= 240
        {
            return Ok(Err(BackupFault::ReplayFull));
        }
    } else {
        prune_orphaned_cleaned_backups(db)?;
    }

    // Host pre-reserves host staging capacity against the aggregate cap and reserve.
    match check_disk(db, max_bytes)? {
        Ok(()) => {}
        Err(crate::vm::Refusal::Disk) => return Ok(Err(BackupFault::Disk)),
        Err(crate::vm::Refusal::UnknownHost) => return Ok(Err(BackupFault::NotReady)),
        Err(_) => return Ok(Err(BackupFault::Disk)),
    }

    db.execute(
        "INSERT INTO activity(principal,vm,action,started_ms,actor) VALUES(?1,?2,?3,?4,'agent')",
        params![principal, target.vm, crate::vm::COMPOSE_BACKUP_ACTION, now],
    )?;
    let activity = db.last_insert_rowid();
    let op_id = nonce()?;
    let backup_id = nonce()?;
    db.execute(
        "INSERT INTO vm_compose_backups(
            id,operation_id,principal,vm,project,generation,boot,revision,volume,database,
            state,host_staging_charge,guest_staging_charge,retained_charge,created_ms,namespace_tracked
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'staging',?11,?11,0,?12,1)",
        params![
            backup_id,
            op_id,
            principal,
            target.vm,
            target.project,
            generation,
            boot,
            revision,
            volume,
            database,
            max_bytes,
            now
        ],
    )?;
    db.execute(
        "INSERT INTO vm_compose_backup_ops(
            id,backup_id,principal,request,vm,project,generation,boot,revision,
            volume,database,max_bytes,valid_through_ms,activity_id
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            op_id,
            backup_id,
            principal,
            key.key,
            target.vm,
            target.project,
            generation,
            boot,
            revision,
            volume,
            database,
            max_bytes,
            valid_through,
            activity
        ],
    )?;
    Ok(Ok(backup_replay(db, principal, key.key)?.ok_or_else(
        || anyhow::anyhow!("Compose backup intent unavailable"),
    )?))
}

pub(crate) fn mark_backup_sent(db: &Connection, op_id: Id) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE vm_compose_backup_ops SET sent=1 WHERE id=?1 AND sent=0 AND status='pending'",
            [op_id],
        )? == 1,
        "Compose backup already submitted"
    );
    Ok(())
}

pub(crate) fn owner_backups(
    db: &Connection,
    id: Option<Id>,
) -> Result<Vec<compose_backup::OwnerBackup>> {
    let mut statement = db.prepare(
        "SELECT b.id,b.operation_id,b.state,b.directory_id,b.volume_id,b.file_id,
                b.namespace_tracked,b.sha256,r.owner,r.started_ms,r.finished_ms
         FROM vm_compose_backups b
         LEFT JOIN vm_compose_backup_retirements r ON r.backup_id=b.id
         WHERE (?1 IS NULL OR b.id=?1) AND (b.state='settled' OR
                (b.state='partial_uncertain' AND b.volume_id IS NULL AND b.file_id IS NULL))
         ORDER BY b.created_ms,b.id",
    )?;
    let rows = statement.query_map([id], |r| {
        let directory_id = r.get(3)?;
        let volume_id: Option<[u8; 8]> = r.get(4)?;
        let file_id = r.get(5)?;
        let witness = match (directory_id, volume_id, file_id) {
            (Some(directory_id), Some(volume_id), Some(file)) => {
                Some(compose_backup::HostFileWitness {
                    identity: crate::vm::paths::FileIdentity {
                        volume: u64::from_be_bytes(volume_id),
                        file,
                    },
                    directory_id,
                })
            }
            _ => None,
        };
        Ok(compose_backup::OwnerBackup {
            id: r.get(0)?,
            operation_id: r.get(1)?,
            state: compose_backup::BackupRecordState::parse(&r.get::<_, String>(2)?)?,
            directory_id,
            witness,
            namespace_tracked: r.get(6)?,
            sha256: r.get(7)?,
            owner: r.get(8)?,
            started_ms: r.get(9)?,
            finished_ms: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn prepare_owner_retirement(
    db: &Connection,
    id: Id,
    owner: &str,
    now: i64,
) -> Result<()> {
    ensure!(
        db.execute(
            "INSERT INTO vm_compose_backup_retirements(backup_id,owner,started_ms,sha256)
             SELECT id,?2,?3,sha256 FROM vm_compose_backups
             WHERE id=?1 AND (state='settled' OR
                (state='partial_uncertain' AND volume_id IS NULL AND file_id IS NULL))",
            params![id, owner, now],
        )? == 1,
        "backup unavailable for retirement"
    );
    Ok(())
}

pub(crate) fn complete_owner_retirement(
    db: &Connection,
    proof: compose_backup::OwnerRetirementProof,
    now: i64,
) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE vm_compose_backup_retirements SET finished_ms=max(started_ms,?2)
             WHERE backup_id=?1 AND finished_ms IS NULL",
            params![proof.backup_id(), now],
        )? == 1,
        "backup retirement was not prepared"
    );
    Ok(())
}

pub(crate) fn bind_backup_namespace(db: &Connection, id: Id, directory_id: Id) -> Result<bool> {
    Ok(db.execute(
        "UPDATE vm_compose_backups SET directory_id=?2
         WHERE id=?1 AND state='staging' AND (directory_id IS NULL OR directory_id=?2)",
        params![id, directory_id],
    )? == 1)
}

pub(crate) fn record_backup_staging_identity(
    db: &Connection,
    backup_id: Id,
    volume_id: u64,
    directory_id: Id,
    file_id: Id,
) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE vm_compose_backups SET volume_id=?2,directory_id=?3,file_id=?4
             WHERE id=?1 AND state IN ('staging','partial_uncertain')
               AND (directory_id IS NULL OR directory_id=?3) AND file_id IS NULL",
            params![backup_id, volume_id.to_be_bytes(), directory_id, file_id],
        )? == 1,
        "Compose backup not staging"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn settle_backup_succeeded(
    db: &Connection,
    intent: &compose_backup::BackupIntent,
    size_bytes: u64,
    sha256: [u8; 32],
    volume_id: u64,
    directory_id: Id,
    file_id: Id,
    now: i64,
) -> Result<()> {
    ensure!(
        size_bytes >= 1 && size_bytes <= intent.max_bytes,
        "Compose backup size out of bounds"
    );
    ensure!(
        settled_backup_count(db, &intent.target)?
            < compose_backup::MAX_RETAINED_BACKUPS_PER_PROJECT,
        "Compose backup retention cap exceeded"
    );
    ensure!(
        db.execute(
            "UPDATE vm_compose_backups SET
                state='settled',
                host_staging_charge=0,
                guest_staging_charge=0,
                retained_charge=?2,
                size_bytes=?2,
                sha256=?3,
                volume_id=?4,
                directory_id=?5,
                file_id=?6,
                settled_ms=?7
             WHERE id=?1 AND state='staging'",
            params![
                intent.backup_id,
                size_bytes,
                sha256,
                volume_id.to_be_bytes(),
                directory_id,
                file_id,
                now
            ],
        )? == 1,
        "Compose backup not in staging state"
    );
    ensure!(
        db.execute(
            "UPDATE vm_compose_backup_ops SET status='succeeded', failure=NULL
             WHERE id=?1 AND status='pending'",
            [intent.id],
        )? == 1,
        "Compose backup operation not pending"
    );
    let activity_id: Option<i64> = db.query_row(
        "SELECT activity_id FROM vm_compose_backup_ops WHERE id=?1",
        [intent.id],
        |r| r.get(0),
    )?;
    if let Some(id) = activity_id {
        super::finish(db, id, crate::vm::Status::Succeeded, now, Some(true))?;
    }
    require_headroom(db)
}

pub(crate) fn settle_backup_failed(
    db: &Connection,
    intent: &compose_backup::BackupIntent,
    failure: &str,
    interrupted: bool,
    staging_cleaned: bool,
    now: i64,
) -> Result<()> {
    let op_status = if interrupted { "interrupted" } else { "failed" };
    let changed = db.execute(
        "UPDATE vm_compose_backup_ops SET status=?2, failure=?3
         WHERE id=?1 AND status='pending'",
        params![intent.id, op_status, failure],
    )?;
    // Update the backup row even if the retire trigger already interrupted the
    // operation row, so confirmed cleanup releases host_staging_charge while
    // settled rows remain untouched.
    if staging_cleaned {
        db.execute(
            "UPDATE vm_compose_backups SET state='cleaned', host_staging_charge=0, guest_staging_charge=0
             WHERE id=?1 AND state IN ('staging','partial_uncertain')",
            [intent.backup_id],
        )?;
    } else {
        // Uncertain partials keep host_staging_charge until attributed cleanup proves removal.
        db.execute(
            "UPDATE vm_compose_backups SET state='partial_uncertain', guest_staging_charge=0
             WHERE id=?1 AND state='staging'",
            [intent.backup_id],
        )?;
    }
    if changed == 0 {
        return Ok(());
    }
    let activity_id: Option<i64> = db.query_row(
        "SELECT activity_id FROM vm_compose_backup_ops WHERE id=?1",
        [intent.id],
        |r| r.get(0),
    )?;
    if let Some(id) = activity_id {
        let audit_status = if interrupted {
            crate::vm::Status::Interrupted
        } else {
            crate::vm::Status::Failed
        };
        super::finish(db, id, audit_status, now, Some(staging_cleaned))?;
    }
    Ok(())
}

fn prune_orphaned_cleaned_backups(db: &Connection) -> Result<()> {
    db.execute(
        "DELETE FROM vm_compose_backups
         WHERE state='cleaned'
           AND NOT EXISTS (SELECT 1 FROM vm_compose_backup_ops o WHERE o.backup_id=vm_compose_backups.id)",
        [],
    )?;
    Ok(())
}

/// Lists all `partial_uncertain` backup rows with their recorded file identity
/// for attributed custody reconciliation.
pub(crate) fn list_partial_uncertain_backups(
    db: &Connection,
) -> Result<Vec<compose_backup::PartialBackupRecord>> {
    let mut stmt = db.prepare(
        "SELECT id, operation_id, volume_id, directory_id, file_id
         FROM vm_compose_backups
         WHERE state='partial_uncertain'
           AND NOT EXISTS (SELECT 1 FROM vm_compose_backup_retirements r
                           WHERE r.backup_id=vm_compose_backups.id)
         ORDER BY created_ms ASC, id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        let vol_bytes: Option<[u8; 8]> = r.get(2)?;
        let dir_id: Option<Id> = r.get(3)?;
        let file_id: Option<Id> = r.get(4)?;
        let witness = match (vol_bytes, dir_id, file_id) {
            (Some(v), Some(directory_id), Some(file)) => Some(compose_backup::HostFileWitness {
                identity: crate::vm::paths::FileIdentity {
                    volume: u64::from_be_bytes(v),
                    file,
                },
                directory_id,
            }),
            _ => None,
        };
        Ok(compose_backup::PartialBackupRecord {
            backup_id: r.get(0)?,
            operation_id: r.get(1)?,
            witness,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Verify the guest-reported staging owner is a terminal operation on this boot.
pub(crate) fn terminal_backup_on_boot(
    db: &Connection,
    target: &Target,
    generation: Id,
    boot: u64,
    owner: Id,
) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM vm_compose_backup_ops WHERE vm=?1 AND generation=?2 AND boot=?3 AND id=?4 AND status<>'pending')",
        params![target.vm, generation, boot, owner], |r| r.get(0))?)
}

/// Releases the charge of a `partial_uncertain` backup ONLY when presented with
/// a `PartialCleanupProof` constructed by attributed custody after confirming
/// removal of the staging leaf and any never-settled leaf. Settled backups are
/// unaffected and guarded by SQLite immutability triggers.
pub(crate) fn reconcile_partial_backup(
    db: &Connection,
    proof: compose_backup::PartialCleanupProof,
) -> Result<bool> {
    let changed = db.execute(
        "UPDATE vm_compose_backups SET state='cleaned', host_staging_charge=0, guest_staging_charge=0
         WHERE id=?1 AND state='partial_uncertain' AND volume_id=?2 AND directory_id=?3 AND file_id=?4",
        params![proof.backup_id(),proof.witness().identity.volume.to_be_bytes(),proof.witness().directory_id,proof.witness().identity.file],
    )?;
    prune_orphaned_cleaned_backups(db)?;
    Ok(changed == 1)
}

#[allow(
    dead_code,
    reason = "reserved for owner backup inspection and E4 tests"
)]
pub(crate) fn list_project_backups(
    db: &Connection,
    target: &Target,
) -> Result<Vec<compose_backup::BackupRecord>> {
    let mut stmt = db.prepare(
        "SELECT id,operation_id,vm,project,generation,boot,revision,volume,database,
                state,host_staging_charge,guest_staging_charge,retained_charge,
                size_bytes,sha256,volume_id,directory_id,file_id,created_ms,settled_ms
         FROM vm_compose_backups WHERE vm=?1 AND project=?2 ORDER BY created_ms ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![target.vm, target.project], |r| {
        let vol_bytes: Option<[u8; 8]> = r.get(15)?;
        Ok(compose_backup::BackupRecord {
            id: r.get(0)?,
            operation_id: r.get(1)?,
            vm: r.get(2)?,
            project: r.get(3)?,
            generation: r.get(4)?,
            boot: r.get(5)?,
            revision: r.get(6)?,
            volume: r.get(7)?,
            database: r.get(8)?,
            state: compose_backup::BackupRecordState::parse(&r.get::<_, String>(9)?)?,
            host_staging_charge: r.get(10)?,
            guest_staging_charge: r.get(11)?,
            retained_charge: r.get(12)?,
            size_bytes: r.get(13)?,
            sha256: r.get(14)?,
            volume_id: vol_bytes.map(u64::from_be_bytes),
            directory_id: r.get(16)?,
            file_id: r.get(17)?,
            created_ms: r.get(18)?,
            settled_ms: r.get(19)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

//! Durable host intent, immutable owner enrollment and conservative volume charges.
use super::{VmStore, count, require_headroom};
use crate::vm::{Id, compose::*};
use anyhow::{Result, ensure};
use arx_guest::compose::{Desired, MAX_PROJECTS, MAX_VOLUMES, OperationState, OperationView};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) const MIGRATE_V12: &str = "
CREATE TABLE vm_compose_projects (
    vm BLOB NOT NULL REFERENCES vm_static_instances(id),
    project TEXT NOT NULL,
    desired TEXT NOT NULL DEFAULT 'none' CHECK(desired IN ('none','started','stopped','down')),
    last_revision INTEGER NOT NULL DEFAULT 0 CHECK(last_revision>=0),
    committed INTEGER,
    committed_release BLOB,
    settled_generation BLOB,
    settled_boot INTEGER,
    observed TEXT,
    last_receipt TEXT,
    PRIMARY KEY(vm,project),
    FOREIGN KEY(vm,project,committed_release) REFERENCES vm_compose_releases(vm,project,id),
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
    sent INTEGER NOT NULL DEFAULT 0 CHECK(sent IN (0,1)),
    receipt TEXT,
    closed TEXT CHECK(closed IN ('refused','secret_failed','retired_boot')),
    secret_refs TEXT NOT NULL,
    secret_sent INTEGER NOT NULL DEFAULT 0 CHECK(secret_sent BETWEEN 0 AND 15),
    UNIQUE(principal,request),
    FOREIGN KEY(vm,project,release) REFERENCES vm_compose_releases(vm,project,id)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX vm_compose_inflight ON vm_compose_operations(vm,project) WHERE receipt IS NULL AND closed IS NULL;
-- Running/Recovering -> Stopped is committed only after host release proof.
CREATE TRIGGER vm_compose_retire AFTER UPDATE OF state ON vm_static_instances
WHEN OLD.state IN (2,3) AND NEW.state=1 BEGIN
    UPDATE vm_compose_boots SET retired=1 WHERE vm=NEW.id AND retired=0;
    UPDATE vm_compose_operations SET closed='retired_boot'
        WHERE vm=NEW.id AND receipt IS NULL AND closed IS NULL;
END;
";

pub(crate) fn enroll(store: &VmStore, releases: &[Enrollment]) -> Result<()> {
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
            (SELECT id FROM vm_compose_operations o WHERE o.vm=p.vm AND o.project=p.project AND o.receipt IS NULL AND o.closed IS NULL)
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
                pending: r.get(6)?,
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
    if status.pending.is_some() {
        return Ok(Err(Fault::Busy));
    }
    // Stop and Down remain usable under normal application capacity pressure.
    if matches!(request.kind, Kind::Deploy | Kind::Start) {
        require_headroom(db)?;
    }
    let high_water = super::retry_time(db, now)?;
    let valid_through = request.key.issued_at_ms + crate::vm::RETRY_WINDOW_MS;
    if request.key.issued_at_ms > now || valid_through < high_water {
        return Ok(Err(Fault::StaleKey));
    }
    db.execute("DELETE FROM vm_compose_operations WHERE valid_through_ms<?1 AND (receipt IS NOT NULL OR closed IS NOT NULL)",[high_water])?;
    let receipts = count(
        db,
        "SELECT count(*) FROM vm_compose_operations WHERE principal=?1",
        [principal],
    )?;
    let limit = if matches!(request.kind, Kind::Deploy | Kind::Start) {
        240
    } else {
        256
    };
    if receipts >= limit {
        return Ok(Err(Fault::ReplayFull));
    }
    let (release, revision) = if let Some(release) = request.release {
        let next: u64 = db.query_row(
            "SELECT last_revision+1 FROM vm_compose_projects WHERE vm=?1 AND project=?2",
            params![target.vm, target.project],
            |r| r.get(0),
        )?;
        (release, next)
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
    if request.kind == Kind::Deploy {
        db.execute(
            "UPDATE vm_compose_projects SET last_revision=?3 WHERE vm=?1 AND project=?2",
            params![target.vm, target.project, revision],
        )?;
    }
    let id = nonce()?;
    db.execute(
        "INSERT INTO vm_compose_operations(id,principal,request,vm,project,generation,boot,kind,release,revision,secret_refs,valid_through_ms)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![id,principal,request.key.key,target.vm,target.project,generation,boot,request.kind.wire(),release,revision,serde_json::to_string(&secret_refs)?,valid_through],
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

pub(crate) fn close(db: &Connection, intent: &Intent, reason: &str) -> Result<()> {
    ensure!(db.execute("UPDATE vm_compose_operations SET closed=?2 WHERE id=?1 AND receipt IS NULL AND closed IS NULL",
        params![intent.id,reason])? == 1, "Compose intent already closed");
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
    if receipt.state == OperationState::Succeeded {
        let project = observed.ok_or_else(|| anyhow::anyhow!("Compose observation missing"))?;
        db.execute(
            "UPDATE vm_compose_projects SET desired=?3,committed=?4,committed_release=?5,
                observed=?6,settled_generation=?7,settled_boot=?8 WHERE vm=?1 AND project=?2",
            params![
                intent.target.vm,
                intent.target.project,
                desired_wire(project.desired),
                intent.revision,
                intent.release,
                serde_json::to_string(project)?,
                intent.generation,
                intent.boot
            ],
        )?;
    }
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

pub(super) fn uses_key(db: &Connection, principal: &str, key: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_compose_operations WHERE principal=?1 AND request=?2",
        params![principal, key],
    )? != 0)
}

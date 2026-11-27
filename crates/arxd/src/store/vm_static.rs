//! Static generations have independent custody roles and no TEST cleanup route.
use super::*;
pub(super) const MIGRATE_V18: &str = "
CREATE TABLE vm_static_custody (
    generation BLOB PRIMARY KEY REFERENCES vm_generations(id),
    volume BLOB NOT NULL CHECK(length(volume)=8),
    production BLOB NOT NULL CHECK(length(production)=16),
    directory BLOB NOT NULL CHECK(length(directory)=16 AND directory<>production),
    lease BLOB NOT NULL CHECK(length(lease)=16 AND lease<>directory AND lease<>production)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER vm_static_custody_insert BEFORE INSERT ON vm_static_custody
    WHEN NOT EXISTS (SELECT 1 FROM vm_generations g JOIN vm_static_instances s ON s.id=g.static_id
        WHERE g.id=NEW.generation AND g.active=1 AND s.state=1)
    BEGIN SELECT RAISE(ABORT,'invalid static custody'); END;
CREATE TRIGGER vm_static_custody_update BEFORE UPDATE ON vm_static_custody
    BEGIN SELECT RAISE(ABORT,'immutable static custody'); END;
";

use crate::vm::static_web::{StaticName, StaticProvenance, StaticState};
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
use crate::vm::{CandidateSlot, CopyFailure};

pub(crate) struct StaticRow {
    pub id: Id,
    pub name: StaticName,
    pub template: Id,
    pub state: StaticState,
    pub provenance: StaticProvenance,
}

pub(crate) fn host_id(db: &Connection) -> Result<Id> {
    Ok(db.query_row(
        "SELECT id FROM vm_state_host WHERE singleton=1",
        [],
        |row| row.get(0),
    )?)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn origin_generation(db: &Connection, vm: Id) -> Result<Id> {
    Ok(db.query_row(
        "SELECT origin_generation FROM vm_static_instances WHERE id=?1",
        [vm],
        |row| row.get(0),
    )?)
}

fn state(code: i64) -> rusqlite::Result<StaticState> {
    match code {
        0 => Ok(StaticState::Creating),
        1 => Ok(StaticState::Stopped),
        2 => Ok(StaticState::Running),
        3 => Ok(StaticState::Recovering),
        4 => Ok(StaticState::Failed),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

pub(crate) fn rows(
    db: &Connection,
    principal: &str,
    exact: Option<Id>,
    after: Option<Id>,
    limit: u32,
) -> Result<Vec<StaticRow>> {
    let mut query = db.prepare(
        "SELECT s.id,s.name,s.template,s.state,s.source_generation,s.recipe_digest,s.disk_digest,s.vars_digest
         FROM vm_static_instances s JOIN vm_state_host h ON s.host=h.id
         WHERE s.principal=?1 AND (?2 IS NULL OR s.id=?2) AND (?3 IS NULL OR s.id>?3)
         ORDER BY s.id LIMIT ?4",
    )?;
    let rows = query.query_map(params![principal, exact, after, limit], |row| {
        Ok(StaticRow {
            id: row.get(0)?,
            name: StaticName::parse(&row.get::<_, String>(1)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            template: row.get(2)?,
            state: state(row.get(3)?)?,
            provenance: StaticProvenance {
                source_generation: row.get(4)?,
                recipe_digest: row.get(5)?,
                disk_digest: row.get(6)?,
                vars_digest: row.get(7)?,
            },
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn owned_state(db: &Connection, principal: &str, vm: Id) -> Result<Option<StaticState>> {
    Ok(db
        .query_row(
            "SELECT s.state FROM vm_static_instances s JOIN vm_state_host h ON s.host=h.id
         WHERE s.id=?1 AND s.principal=?2",
            params![vm, principal],
            |row| state(row.get(0)?),
        )
        .optional()?)
}

pub(crate) fn instance_count(db: &Connection) -> Result<i64> {
    count(
        db,
        "SELECT (SELECT count(*) FROM vm_instances)+(SELECT count(*) FROM vm_static_instances)",
        [],
    )
}

/// The sealed build binds this class by its domain-derived recipe ID. Legacy
/// templates with no build row remain eligible for the existing TEST path.
pub(crate) fn is_compose_template(db: &Connection, template: Id) -> Result<bool> {
    let recipe: Option<(String, [u8; 32])> = db
        .query_row(
            "SELECT b.recipe_id,t.recipe_digest FROM vm_templates t
         JOIN vm_build_intents b ON b.template_id=t.id WHERE t.id=?1",
            [template],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(recipe.is_some_and(|(recipe, digest)| {
        recipe
            == hex::encode(
                crate::vm::qualification_record::DockerRecipe::identity(digest).recipe_id,
            )
    }))
}

pub(crate) fn launch_busy(db: &Connection) -> Result<bool> {
    Ok(count(
        db,
        "SELECT EXISTS(SELECT 1 FROM vm_instances WHERE state IN (2,3,4))
            OR EXISTS(SELECT 1 FROM vm_static_instances WHERE state IN (0,2,3))
            OR EXISTS(SELECT 1 FROM activity WHERE action=3 AND status=0)
            OR EXISTS(SELECT 1 FROM vm_build_intents WHERE state<>'sealed')
            OR EXISTS(SELECT 1 FROM vm_operations WHERE status=0 AND binding_kind='template')",
        [],
    )? != 0)
}

/// Mark interrupted static work for recovery. A committed generation is
/// never deleted here; a Creating candidate's effects are unknown, so it is
/// quarantined and stays charged.
pub(super) fn recover(db: &Connection, now: i64) -> Result<()> {
    db.execute(
        "UPDATE vm_artifacts SET state='quarantined'
         WHERE generation_id IN (SELECT id FROM vm_generations WHERE active=0
            AND static_id IN (SELECT id FROM vm_static_instances WHERE state=0))",
        [],
    )?;
    db.execute("UPDATE vm_static_instances SET state=4 WHERE state=0", [])?;
    let mut stmt = db.prepare(
        "SELECT id FROM vm_static_instances WHERE state IN (2,3) OR (state=1 AND
            id IN (SELECT vm FROM activity WHERE status=0 AND action IN (2,3,4)))",
    )?;
    let interrupted = stmt
        .query_map([], |row| row.get::<_, Id>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for vm in interrupted {
        begin_recovery(db, vm, now)?;
    }
    Ok(())
}

/// Fence the VM in `Recovering` with one daemon recovery receipt.
fn begin_recovery(db: &Connection, vm: Id, now: i64) -> Result<()> {
    db.execute(
        "UPDATE vm_static_instances SET state=3 WHERE id=?1 AND state IN (1,2,3)",
        [vm],
    )?;
    if count(
        db,
        "SELECT EXISTS(SELECT 1 FROM activity WHERE vm=?1 AND action=10 AND status=0)",
        [vm],
    )? == 0
    {
        activity(db, "arxd", vm, Action::Recover, now)?;
    }
    Ok(())
}

/// A committed generation awaiting non-deleting release proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StaticRecovery {
    pub vm: Id,
    pub generation: Id,
    pub activity_id: i64,
}

const RECOVERY_CLAIM: &str = "SELECT s.id,g.id,a.id FROM vm_static_instances s
     JOIN vm_generations g ON g.static_id=s.id AND g.active=1
     JOIN activity a ON a.vm=s.id AND a.action=10 AND a.status=0
        AND a.actor='daemon' AND a.vm_owner=s.principal
     WHERE s.state=3 AND (?1 IS NULL OR s.id=?1)
        AND NOT EXISTS(SELECT 1 FROM activity p WHERE p.vm=s.id AND p.status=0 AND p.action IN (2,3,4))
        AND (SELECT count(*) FROM vm_artifacts x WHERE x.generation_id=g.id AND x.state='active'
            AND x.file_id IS NOT NULL AND x.role IN ('static_disk','static_vars'))=2
     ORDER BY s.id LIMIT 8";

fn recovery_rows(db: &Connection, vm: Option<Id>) -> Result<Vec<StaticRecovery>> {
    let mut stmt = db.prepare(RECOVERY_CLAIM)?;
    Ok(stmt
        .query_map([vm], |row| {
            Ok(StaticRecovery {
                vm: row.get(0)?,
                generation: row.get(1)?,
                activity_id: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn recovery_claims(db: &Connection) -> Result<Vec<StaticRecovery>> {
    recovery_rows(db, None)
}

/// Settle one recovery claim. Release proof returns the same generation to
/// `Stopped`; anything else, including a changed claim, keeps the launch
/// slot fenced. This never removes or retires a static artifact.
pub(crate) fn settle_recovery(
    db: &Connection,
    claim: StaticRecovery,
    released: bool,
    now: i64,
) -> Result<bool> {
    if recovery_rows(db, Some(claim.vm))? != [claim] {
        return Ok(false);
    }
    if !released {
        db.execute(
            "UPDATE activity SET cleanup_confirmed=0 WHERE id=?1 AND status=0",
            [claim.activity_id],
        )?;
        return Ok(false);
    }
    use crate::vm::static_web::StaticEvent;
    let next = StaticState::Recovering
        .settle(StaticEvent::RecoveredStopped)
        .map_err(|_| anyhow::anyhow!("invalid static VM transition"))?;
    ensure!(
        db.execute(
            "UPDATE vm_static_instances SET state=?2 WHERE id=?1 AND state=3",
            params![claim.vm, next as i64],
        )? == 1,
        "invalid static recovery"
    );
    super::compose::finish_retired(db, claim.vm, now)?;
    finish(db, claim.activity_id, Status::Succeeded, now, Some(true))?;
    Ok(true)
}

pub(crate) fn recovering(db: &Connection) -> Result<bool> {
    Ok(count(
        db,
        "SELECT EXISTS(SELECT 1 FROM vm_static_instances WHERE state=3)",
        [],
    )? != 0)
}

/// Rehydrate the committed pair of a stopped static VM after reopen. The
/// caller re-verifies identity and content before any boot.
#[cfg(test)]
pub(crate) fn active_pair(
    db: &Connection,
    principal: &str,
    vm: Id,
) -> Result<Option<(PairSlots, [ArtifactEvidence; 2])>> {
    pair_in_state(db, principal, vm, StaticState::Stopped)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn pair_in_state(
    db: &Connection,
    principal: &str,
    vm: Id,
    state: StaticState,
) -> Result<Option<(PairSlots, [ArtifactEvidence; 2])>> {
    let mut stmt = db.prepare(
        "SELECT g.id,a.role,a.object_key,a.volume_id,a.directory_id,a.file_id,a.content_digest,a.bytes
         FROM vm_static_instances s JOIN vm_generations g ON g.static_id=s.id AND g.active=1
         JOIN vm_artifacts a ON a.generation_id=g.id AND a.state='active'
         WHERE s.id=?1 AND s.principal=?2 AND s.state=?3 AND a.role IN ('static_disk','static_vars')
         ORDER BY a.role",
    )?;
    let rows = stmt
        .query_map(params![vm, principal, state as i64], |row| {
            let role = match row.get::<_, String>(1)?.as_str() {
                "static_disk" => "static_disk",
                "static_vars" => "static_vars",
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            let slot = ArtifactSlot {
                generation: row.get(0)?,
                role,
                key: row.get(2)?,
            };
            let volume = row.get::<_, Option<[u8; 8]>>(3)?.map(u64::from_be_bytes);
            let directory = row.get::<_, Option<Vec<u8>>>(4)?.and_then(identity_bytes);
            let file = row.get::<_, Option<Vec<u8>>>(5)?.and_then(identity_bytes);
            let digest = row.get::<_, Option<[u8; 32]>>(6)?;
            let size = row.get::<_, Option<u64>>(7)?;
            let evidence = match (volume, directory, file, digest, size) {
                (Some(volume), Some(directory), Some(file), Some(sha256), Some(size)) => {
                    Some(ArtifactEvidence {
                        identity: FileIdentity { volume, file },
                        directory,
                        pin: crate::vm::paths::ArtifactPin { size, sha256 },
                    })
                }
                _ => None,
            };
            Ok((slot, evidence))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(match rows[..] {
        [(disk, Some(disk_proof)), (vars, Some(vars_proof))]
            if disk.role == "static_disk" && vars.role == "static_vars" =>
        {
            Some((PairSlots { disk, vars }, [disk_proof, vars_proof]))
        }
        _ => None,
    })
}

pub(super) fn settle_operation(
    db: &Connection,
    activity: i64,
    now: i64,
    completion: Completion,
    script: Option<ScriptResult>,
    cause: Option<FailureReason>,
) -> Result<Option<Status>> {
    let target = db
        .query_row(
            "SELECT s.id,s.state,a.action FROM activity a JOIN vm_static_instances s
         ON a.vm=s.id AND a.principal=s.principal
         WHERE a.id=?1 AND a.status=0 AND a.action IN (2,3,4)",
            [activity],
            |row| {
                Ok((
                    row.get::<_, Id>(0)?,
                    state(row.get(1)?)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((vm, state, action)) = target else {
        return Ok(None);
    };
    use crate::vm::static_web::StaticEvent;
    ensure!(
        script.is_none() || action == 2,
        "invalid static script settlement"
    );
    // The worker's proven cause is recorded independently of cleanup proof.
    let (status, event, failure) = match (completion, action) {
        (Completion::CleanupUnconfirmed, _) => (
            Status::Interrupted,
            Some(StaticEvent::NeedsRecovery),
            Some(cause.unwrap_or(FailureReason::CleanupUnconfirmed)),
        ),
        (Completion::Succeeded, 3) => (Status::Succeeded, Some(StaticEvent::Started), None),
        (Completion::Succeeded, 4) => (Status::Succeeded, Some(StaticEvent::Stopped), None),
        (Completion::Succeeded, _) => (Status::Succeeded, None, None),
        (Completion::Failed, 3) => (
            Status::Failed,
            None,
            Some(cause.unwrap_or(FailureReason::BootFailed)),
        ),
        (Completion::Failed, 4) => (
            Status::Failed,
            Some(StaticEvent::NeedsRecovery),
            Some(cause.unwrap_or(FailureReason::DirtyStop)),
        ),
        (Completion::Failed, _) => (Status::Failed, None, cause),
    };
    if let Some(event) = event {
        let next = state
            .settle(event)
            .map_err(|_| anyhow::anyhow!("invalid static VM transition"))?;
        db.execute(
            "UPDATE vm_static_instances SET state=?2 WHERE id=?1",
            params![vm, next as i64],
        )?;
    }
    if event == Some(StaticEvent::Stopped) {
        super::compose::finish_retired(db, vm, now)?;
    }
    // A failed stop leaves the boot unproven, like an interrupted effect.
    let cleanup = status != Status::Interrupted && !(status == Status::Failed && action == 4);
    finish_with_script(db, activity, status, now, Some(cleanup), script, failure)?;
    if event == Some(StaticEvent::NeedsRecovery) {
        begin_recovery(db, vm, now)?;
    }
    Ok(Some(status))
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn allocate_id(db: &Connection) -> Result<Id> {
    for _ in 0..4 {
        let id = new_id()?;
        if count(
            db,
            "SELECT EXISTS(
            SELECT 1 FROM vm_instances WHERE id=?1 UNION ALL
            SELECT 1 FROM vm_static_instances WHERE id=?1 UNION ALL
            SELECT 1 FROM vm_tombstones WHERE id=?1 UNION ALL
            SELECT 1 FROM vm_operations WHERE vm=?1
        )",
            [id],
        )? == 0
        {
            return Ok(id);
        }
    }
    anyhow::bail!("VM identity unavailable")
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn name_occupied(db: &Connection, name: &StaticName) -> Result<bool> {
    Ok(count(
        db,
        "SELECT EXISTS(SELECT 1 FROM vm_static_instances WHERE name=?1)",
        [name.as_str()],
    )? != 0)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn source(
    db: &Connection,
    template: Id,
    (slots, [disk, vars]): (PairSlots, [ArtifactEvidence; 2]),
    web_digest: [u8; 32],
) -> Result<Option<StaticProvenance>> {
    if slots.disk.generation != slots.vars.generation
        || slots.disk.role != "disk"
        || slots.vars.role != "vars"
        || disk.identity == vars.identity
        || disk.identity.volume != vars.identity.volume
        || !evidence_matches(db, slots.disk, disk, "sealed")?
        || !evidence_matches(db, slots.vars, vars, "sealed")?
    {
        return Ok(None);
    }
    Ok(db
        .query_row(
            "SELECT t.recipe_digest,t.disk_digest,t.vars_digest
         FROM vm_templates t JOIN vm_build_intents b ON b.template_id=t.id
         JOIN vm_generations g ON g.build_id=b.id
         WHERE t.id=?1 AND t.guest=1 AND t.recipe_digest=?2 AND b.state='sealed'
            AND b.health_verified=1 AND g.active=1 AND g.id=?3
            AND t.disk_digest=?4 AND t.vars_digest=?5",
            params![
                template,
                web_digest,
                slots.disk.generation,
                disk.pin.sha256,
                vars.pin.sha256
            ],
            |row| {
                Ok(StaticProvenance {
                    source_generation: slots.disk.generation,
                    recipe_digest: row.get(0)?,
                    disk_digest: row.get(1)?,
                    vars_digest: row.get(2)?,
                })
            },
        )
        .optional()?)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn compose_source(
    db: &Connection,
    template: Id,
    inputs: (PairSlots, [ArtifactEvidence; 2]),
    selected: &crate::vm::qualification_record::VerifiedDockerEvidence,
) -> Result<Option<StaticProvenance>> {
    let recipe = selected.recipe().identity;
    let sealed = sealed_pair(db, &hex::encode(recipe.recipe_id), recipe.recipe_sha256)?;
    if sealed != Some((template, inputs.0)) {
        return Ok(None);
    }
    source(db, template, inputs, recipe.recipe_sha256)
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn reserve(
    db: &Connection,
    vm: Id,
    name: &StaticName,
    principal: &str,
    template: Id,
    provenance: StaticProvenance,
    claim: crate::vm::DiskClaim,
) -> Result<PairSlots> {
    let persistent = claim
        .persistent()
        .filter(|&bytes| bytes > 0)
        .ok_or_else(|| anyhow::anyhow!("invalid static reservation"))?;
    db.execute(
        "INSERT INTO vm_static_instances
           (id,name,principal,host,template,source_generation,recipe_digest,disk_digest,vars_digest,state,persistent,staging)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0,?10,?11)",
        params![vm,name.as_str(),principal,host_id(db)?,template,provenance.source_generation,
            provenance.recipe_digest,provenance.disk_digest,provenance.vars_digest,persistent,claim.staging],
    )?;
    let generation = new_id()?;
    db.execute(
        "INSERT INTO vm_generations(id,static_id,generation) VALUES(?1,?2,1)",
        params![generation, vm],
    )?;
    Ok(PairSlots {
        disk: plan(db, generation, "static_disk")?,
        vars: plan(db, generation, "static_vars")?,
    })
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) fn settle_create(
    db: &Connection,
    activity: i64,
    slots: PairSlots,
    outcome: Result<[ArtifactEvidence; 2], CopyFailure>,
    now: i64,
) -> Result<Status> {
    let (vm, source, disk_digest, vars_digest, persistent): (Id, Id, [u8; 32], [u8; 32], u64) = db
        .query_row(
            "SELECT s.id,s.source_generation,s.disk_digest,s.vars_digest,s.persistent
         FROM vm_operations o JOIN vm_static_instances s ON s.id=o.vm AND s.principal=o.principal
         JOIN vm_generations g ON g.static_id=s.id
         WHERE o.activity_id=?1 AND o.status=0 AND o.action=12 AND o.subject_id=s.template
            AND s.state=0 AND g.active=0 AND g.id=?2",
            params![activity, slots.disk.generation],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
    ensure!(
        slots.disk.generation == slots.vars.generation
            && slots.disk.role == "static_disk"
            && slots.vars.role == "static_vars",
        "invalid static settlement pair"
    );
    let verified = match outcome {
        Ok([disk, vars])
            if disk.pin.sha256 == disk_digest
                && vars.pin.sha256 == vars_digest
                && disk.identity != vars.identity
                && disk.identity.volume == vars.identity.volume
                && disk
                    .pin
                    .size
                    .checked_add(vars.pin.size)
                    .is_some_and(|size| size <= persistent)
                && independent_pair(db, source, [disk, vars])? =>
        {
            Ok([disk, vars])
        }
        // Mismatched evidence still names objects the worker created.
        Ok([disk, vars]) => Err(CopyFailure {
            reason: FailureReason::CloneFailed,
            slots: [CandidateSlot::Retained(disk), CandidateSlot::Retained(vars)],
        }),
        Err(failure) => Err(failure),
    };
    let (status, failure, cleanup) = match verified {
        Ok(pair) => {
            for (slot, evidence) in [slots.disk, slots.vars].into_iter().zip(pair) {
                created(db, slot)?;
                identify(db, slot, evidence)?;
            }
            activate(db, slots, "active")?;
            snapshot::initial_release(db, slots.disk.generation)?;
            db.execute(
                "UPDATE vm_static_instances SET state=1,staging=0 WHERE id=?1",
                [vm],
            )?;
            (Status::Succeeded, None, true)
        }
        Err(failure) => {
            let untouched = failure.slots == [CandidateSlot::Absent; 2];
            if untouched {
                release_candidate(db, vm, slots)?;
            } else {
                for (slot, custody) in [slots.disk, slots.vars].into_iter().zip(failure.slots) {
                    if let CandidateSlot::Retained(evidence) = custody {
                        created(db, slot)?;
                        identify(db, slot, evidence)?;
                    }
                }
                db.execute(
                    "UPDATE vm_artifacts SET state='quarantined' WHERE generation_id=?1",
                    [slots.disk.generation],
                )?;
                db.execute("UPDATE vm_static_instances SET state=4 WHERE id=?1", [vm])?;
            }
            (Status::Failed, Some(failure.reason), untouched)
        }
    };
    finish_with_script(db, activity, status, now, Some(cleanup), None, failure)?;
    require_headroom(db)?;
    Ok(status)
}

/// Dispose of a never-activated candidate whose worker proved both slots
/// absent: only its own planned rows, generation and reservation. Any
/// recorded effect keeps the whole candidate quarantined and charged.
#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn release_candidate(db: &Connection, vm: Id, slots: PairSlots) -> Result<()> {
    ensure!(
        count(
            db,
            "SELECT count(*) FROM vm_artifacts a JOIN vm_generations g ON g.id=a.generation_id
             JOIN vm_static_instances s ON s.id=g.static_id
             WHERE g.id=?1 AND g.active=0 AND s.id=?2 AND s.state=0 AND a.state='planned'
               AND a.file_id IS NULL AND a.object_key IN (?3,?4)
               AND a.role IN ('static_disk','static_vars')",
            params![slots.disk.generation, vm, slots.disk.key, slots.vars.key],
        )? == 2
            && count(
                db,
                "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1",
                [slots.disk.generation],
            )? == 2,
        "invalid untouched static candidate"
    );
    forget_generation(db, slots.disk.generation)?;
    ensure!(
        db.execute(
            "DELETE FROM vm_static_instances WHERE id=?1 AND state=0",
            [vm]
        )? == 1,
        "invalid untouched static candidate"
    );
    Ok(())
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn independent_pair(db: &Connection, source: Id, pair: [ArtifactEvidence; 2]) -> Result<bool> {
    let mut query = db.prepare("SELECT volume_id,file_id FROM vm_artifacts WHERE generation_id=?1 AND role IN ('disk','vars')")?;
    let files = query
        .query_map([source], |row| {
            Ok((row.get::<_, [u8; 8]>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(files.len() == 2
        && files.into_iter().all(|(volume, file)| {
            let volume = u64::from_be_bytes(volume);
            pair.iter().all(|evidence| {
                volume == evidence.identity.volume
                    && identity_bytes(file.clone()) != Some(evidence.identity.file)
            })
        }))
}

#[cfg(test)]
#[test]
fn compose_source_refuses_a_swapped_sealed_pair() -> Result<()> {
    use crate::vm::{
        paths::ArtifactPin,
        qualification_record::{DockerRecipe, VerifiedDockerEvidence},
    };

    struct Sealed {
        template: Id,
        slots: PairSlots,
        evidence: [ArtifactEvidence; 2],
    }

    struct Row {
        template: Id,
        build: Id,
        generation: Id,
        disk_key: Id,
        vars_key: Id,
        digest: [u8; 32],
        disk_digest: [u8; 32],
        vars_digest: [u8; 32],
        recipe_id: String,
    }

    fn proof(file: u8, digest: [u8; 32]) -> ArtifactEvidence {
        ArtifactEvidence {
            identity: FileIdentity {
                volume: 7,
                file: [file; 16],
            },
            directory: [0x11; 16],
            pin: ArtifactPin {
                size: 4,
                sha256: digest,
            },
        }
    }

    fn insert(db: &Connection, row: &Row) -> Result<Sealed> {
        let disk = proof(row.disk_key[0], row.disk_digest);
        let vars = proof(row.vars_key[0], row.vars_digest);
        db.execute(
            "INSERT INTO vm_templates (id,guest,recipe_digest,disk_digest,vars_digest,bytes)
             VALUES (?1,1,?2,?3,?4,64)",
            params![row.template, row.digest, row.disk_digest, row.vars_digest],
        )?;
        db.execute(
            "INSERT INTO vm_build_intents
                (id,recipe_id,recipe_digest,staging_bytes,template_id,health_verified,state)
             VALUES (?1,?2,?3,0,?4,1,'sealed')",
            params![row.build, row.recipe_id, row.digest, row.template],
        )?;
        db.execute(
            "INSERT INTO vm_generations (id,build_id,generation,active) VALUES (?1,?2,1,1)",
            params![row.generation, row.build],
        )?;
        for (role, key, item) in [("disk", row.disk_key, disk), ("vars", row.vars_key, vars)] {
            db.execute(
                "INSERT INTO vm_artifacts
                    (generation_id,role,object_key,state,volume_id,directory_id,file_id,
                     content_digest,bytes)
                 VALUES (?1,?2,?3,'sealed',?4,?5,?6,?7,?8)",
                params![
                    row.generation,
                    role,
                    key,
                    item.identity.volume.to_be_bytes(),
                    item.directory,
                    item.identity.file,
                    item.pin.sha256,
                    item.pin.size
                ],
            )?;
        }
        Ok(Sealed {
            template: row.template,
            slots: PairSlots {
                disk: ArtifactSlot {
                    generation: row.generation,
                    role: "disk",
                    key: row.disk_key,
                },
                vars: ArtifactSlot {
                    generation: row.generation,
                    role: "vars",
                    key: row.vars_key,
                },
            },
            evidence: [disk, vars],
        })
    }

    let dir = crate::service::application::tests::private_temp();
    let store = VmStore::open(dir.path(), 1)?;
    store.transact(|db| {
        let digest = [0x21; 32];
        let identity = DockerRecipe::identity(digest);
        let selected = VerifiedDockerEvidence::recipe_fixture(DockerRecipe {
            identity,
            web: [0x41; 32],
            closure: [0x42; 32],
            medium: [0x43; 32],
        });
        let sealed = insert(
            db,
            &Row {
                template: [0xA1; 16],
                build: [0xB1; 16],
                generation: [0xC1; 16],
                disk_key: [0xD1; 16],
                vars_key: [0xD2; 16],
                digest,
                disk_digest: [0x51; 32],
                vars_digest: [0x52; 32],
                recipe_id: hex::encode(identity.recipe_id),
            },
        )?;
        let other_digest = [0x31; 32];
        let other = insert(
            db,
            &Row {
                template: [0xA2; 16],
                build: [0xB2; 16],
                generation: [0xC2; 16],
                disk_key: [0xD3; 16],
                vars_key: [0xD4; 16],
                digest: other_digest,
                disk_digest: [0x61; 32],
                vars_digest: [0x62; 32],
                recipe_id: hex::encode(DockerRecipe::identity(other_digest).recipe_id),
            },
        )?;
        let found = compose_source(
            db,
            sealed.template,
            (sealed.slots, sealed.evidence),
            &selected,
        )?
        .unwrap();
        assert_eq!(found.source_generation, sealed.slots.disk.generation);
        assert_eq!(found.recipe_digest, digest);
        assert_eq!(found.disk_digest, sealed.evidence[0].pin.sha256);
        assert_eq!(found.vars_digest, sealed.evidence[1].pin.sha256);

        let swapped_slots = PairSlots {
            disk: sealed.slots.vars,
            vars: sealed.slots.disk,
        };
        let swapped_evidence = [sealed.evidence[1], sealed.evidence[0]];
        assert!(
            compose_source(
                db,
                sealed.template,
                (swapped_slots, swapped_evidence),
                &selected,
            )?
            .is_none()
        );
        assert!(
            compose_source(db, other.template, (other.slots, other.evidence), &selected)?.is_none()
        );
        assert!(
            compose_source(
                db,
                sealed.template,
                (other.slots, other.evidence),
                &selected
            )?
            .is_none()
        );

        // Files and digest still match the selected recipe. The sealed build's
        // recipe id now names a different pair. `source` still accepts the
        // files; the adapter must refuse them.
        db.execute(
            "UPDATE vm_build_intents SET recipe_id=?1 WHERE template_id=?2",
            params!["swapped-sealed-pair", sealed.template],
        )?;
        assert!(source(db, sealed.template, (sealed.slots, sealed.evidence), digest)?.is_some());
        assert!(
            compose_source(
                db,
                sealed.template,
                (sealed.slots, sealed.evidence),
                &selected
            )?
            .is_none()
        );
        Ok(())
    })
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
pub(crate) mod native {
    use super::*;
    use crate::store::vm::native::Scope;

    pub(crate) fn bind(db: &Connection, scope: Scope) -> Result<()> {
        let volume = scope.production.volume;
        ensure!(
            scope.workspace.volume == volume && scope.lease.volume == volume,
            "invalid static custody volume"
        );
        db.execute(
            "INSERT INTO vm_static_custody(generation,volume,production,directory,lease)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                scope.generation,
                volume.to_be_bytes(),
                scope.production.file,
                scope.workspace.file,
                scope.lease.file
            ],
        )?;
        Ok(())
    }

    type Custody = (Scope, PairSlots, [ArtifactEvidence; 2]);

    pub(crate) fn custody(
        db: &Connection,
        principal: &str,
        vm: Id,
        state: StaticState,
    ) -> Result<Option<Custody>> {
        let Some((slots, pair)) = pair_in_state(db, principal, vm, state)? else {
            return Ok(None);
        };
        let generation = slots.disk.generation;
        let scope = db.query_row(
            "SELECT volume,production,directory,lease FROM vm_static_custody WHERE generation=?1",
            [generation],
            |row| {
                let volume = u64::from_be_bytes(row.get(0)?);
                let identity = |column| -> rusqlite::Result<FileIdentity> {
                    Ok(FileIdentity { volume, file: row.get(column)? })
                };
                Ok(Scope {
                    generation,
                    production: identity(1)?,
                    workspace: identity(2)?,
                    lease: identity(3)?,
                })
            },
        ).optional()?;
        Ok(scope
            .filter(|scope| {
                pair.iter().all(|file| {
                    file.directory == scope.workspace.file
                        && file.identity.volume == scope.workspace.volume
                })
            })
            .map(|scope| (scope, slots, pair)))
    }

    pub(crate) fn recovery(db: &Connection, claim: StaticRecovery) -> Result<Option<Custody>> {
        if recovery_rows(db, Some(claim.vm))?.as_slice() != [claim] {
            return Ok(None);
        }
        let principal: String = db.query_row(
            "SELECT principal FROM vm_static_instances WHERE id=?1",
            [claim.vm],
            |row| row.get(0),
        )?;
        Ok(custody(db, &principal, claim.vm, StaticState::Recovering)?
            .filter(|(scope, _, _)| scope.generation == claim.generation))
    }

    pub(crate) fn runtime_lost(db: &Connection, vm: Id, now: i64) -> Result<()> {
        if count(
            db,
            "SELECT count(*) FROM vm_static_instances WHERE id=?1 AND state=2",
            [vm],
        )? == 1
        {
            super::super::compose::finish_retired(db, vm, now)?;
            begin_recovery(db, vm, now)?;
        }
        Ok(())
    }
}

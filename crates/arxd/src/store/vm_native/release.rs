//! Exact TEST release, independent of the optional stop receipt.

use super::{Scope, pair_matches};
use crate::store::vm::{
    begin_recovery, evidence_matches, finish_with_script, settle_test_operation,
};
use crate::vm::{
    ArtifactEvidence, ClockReading, Completion, FailureReason, Id, MAX_UTC_MS, PairSlots, Status,
};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, Transaction, params};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Target {
    pub vm: Id,
    pub generation: Id,
    pub epoch: Id,
}

pub(crate) struct Observed {
    pub target: Target,
    pub scope: Scope,
    pub slots: PairSlots,
    pub pins: [ArtifactEvidence; 2],
    pub clean: bool,
}

/// This stamp is issued alongside a fresh stop receipt, before the admission
/// transaction commits. A receipt lookup or historical replay cannot mint it.
pub(crate) struct StopReceipt {
    target: Target,
    activity: i64,
    operation: Id,
    deadline: Instant,
}

impl StopReceipt {
    pub(crate) fn at_admission(
        db: &Transaction<'_>,
        vm: Id,
        activity: i64,
        deadline: Instant,
    ) -> Result<Self> {
        let (generation, epoch, operation) = db.query_row(
            "SELECT g.id,r.epoch,o.operation_id
             FROM vm_generations g JOIN vm_snapshot_release r ON r.generation=g.id
             JOIN vm_native_generations n ON n.generation=g.id
             JOIN vm_operations o ON o.vm=g.vm_id
             JOIN activity a ON a.id=o.activity_id AND a.vm=o.vm AND a.action=o.action
             WHERE g.vm_id=?1 AND g.active=1 AND g.static_id IS NULL
               AND n.state='bound' AND r.disposition IS NULL
               AND o.activity_id=?2 AND o.action=4 AND o.status=0 AND a.status=0
               AND o.binding_kind='vm' AND o.subject_id=g.vm_id",
            params![vm, activity],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(Self {
            target: Target {
                vm,
                generation,
                epoch,
            },
            activity,
            operation,
            deadline,
        })
    }

    /// The stop may still succeed only before this monotonic deadline.
    #[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    fn status(&self, db: &Transaction<'_>, target: Target) -> Result<Option<Status>> {
        ensure!(
            (self.target.vm, self.target.generation, self.target.epoch)
                == (target.vm, target.generation, target.epoch),
            "invalid TEST stop receipt scope"
        );
        let receipt = db.query_row(
            "SELECT o.status,
                    o.vm IS ?2 AND o.action=4 AND o.binding_kind='vm'
                    AND o.subject_id IS ?2 AND o.operation_id=?3
                    AND (a.id IS NULL AND o.status<>0 OR
                         a.vm IS ?2 AND a.action=4 AND a.status=o.status AND a.principal=o.principal)
             FROM vm_operations o LEFT JOIN activity a ON a.id=o.activity_id
             WHERE o.activity_id=?1",
            params![self.activity, target.vm, self.operation],
            |row| Ok((Status::from_code(row.get(0)?)?, row.get::<_, bool>(1)?)),
        ).optional()?;
        match receipt {
            Some((status, true)) => Ok(Some(status)),
            None => Ok(None),
            _ => anyhow::bail!("invalid TEST stop receipt binding"),
        }
    }
}

/// Immediate transaction owned by the caller. Errors roll back the release,
/// pins and receipt together. The caller still owns the held proof, and the
/// unresolved epoch and bound slots are the durable retry pointer.
pub(crate) fn commit(
    db: &Transaction<'_>,
    observed: &Observed,
    stop: Option<&StopReceipt>,
    clock: impl FnOnce() -> ClockReading,
) -> Result<Option<Status>> {
    let Observed {
        target,
        scope,
        slots,
        pins,
        clean,
    } = *observed;
    let Target {
        vm,
        generation,
        epoch,
    } = target;
    let (state, previous, charge): (i64, Option<String>, u64) = db.query_row(
        "SELECT v.state,r.disposition,
                (SELECT min(persistent) FROM vm_disk_claims
                 WHERE vm=v.id AND snapshot IS NULL AND persistent>0
                 HAVING count(*)=1 AND min(staging)=0)
         FROM vm_instances v JOIN vm_generations g ON g.vm_id=v.id AND g.active=1
         JOIN vm_snapshot_release r ON r.generation=g.id
         WHERE v.id=?1 AND g.id=?2 AND r.epoch=?3 AND g.static_id IS NULL AND v.state<>0",
        params![vm, generation, epoch],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    ensure!(
        scope.generation == generation
            && slots.disk.role == "disk"
            && slots.vars.role == "vars"
            && pins[0].identity != pins[1].identity
            && pins.iter().all(|file| file.pin.size > 0)
            && pins[0]
                .pin
                .size
                .checked_add(pins[1].pin.size)
                .is_some_and(|size| size <= charge)
            && pair_matches(db, scope, [slots.disk, slots.vars], pins)?,
        "invalid TEST release custody or charge"
    );
    let disposition = if clean { "clean" } else { "dirty" };
    ensure!(
        previous
            .as_deref()
            .is_none_or(|committed| committed == disposition),
        "conflicting TEST release disposition"
    );
    let receipt = stop
        .map(|stop| stop.status(db, target))
        .transpose()?
        .flatten();

    for (slot, pin) in [slots.disk, slots.vars].into_iter().zip(pins) {
        if previous.is_some() {
            ensure!(
                evidence_matches(db, slot, pin, "active")?,
                "conflicting TEST release pin"
            );
            continue;
        }
        let changed = db.execute(
            "UPDATE vm_artifacts SET content_digest=?7,bytes=?8
             WHERE generation_id=?1 AND role=?2 AND object_key=?3 AND state='active'
               AND volume_id=?4 AND directory_id=?5 AND file_id=?6",
            params![
                generation,
                slot.role,
                slot.key,
                pin.identity.volume.to_be_bytes(),
                pin.directory,
                pin.identity.file,
                pin.pin.sha256,
                pin.pin.size,
            ],
        )?;
        ensure!(changed == 1, "invalid or conflicting TEST release pin");
    }
    if previous.is_none() {
        db.execute(
            "UPDATE vm_snapshot_release SET disposition=?2 WHERE generation=?1",
            params![generation, disposition],
        )?;
    }

    if let Some(stop) = stop.filter(|_| receipt == Some(Status::Pending)) {
        let now = clock();
        ensure!((0..=MAX_UTC_MS).contains(&now.utc_ms), "invalid VM clock");
        let status = if state == 3 {
            // Recovery already owns the VM. Preserve its original intent.
            finish_with_script(
                db,
                stop.activity,
                Status::Interrupted,
                now.utc_ms,
                Some(false),
                None,
                (!clean).then_some(FailureReason::DirtyStop),
            )?;
            Status::Interrupted
        } else {
            // A stop admitted while the start was still pending (stopped,
            // state 1) settles exactly like one admitted while running.
            let completion = if clean && now.monotonic < stop.deadline && matches!(state, 1 | 2) {
                Completion::Succeeded
            } else {
                Completion::CleanupUnconfirmed
            };
            settle_test_operation(
                db,
                stop.activity,
                now.utc_ms,
                completion,
                None,
                (!clean).then_some(FailureReason::DirtyStop),
            )?
        };
        return Ok(Some(status));
    }

    if previous.is_none() && state == 2 {
        let now = clock();
        ensure!((0..=MAX_UTC_MS).contains(&now.utc_ms), "invalid VM clock");
        begin_recovery(db, vm, now.utc_ms)?;
    }
    Ok(receipt)
}

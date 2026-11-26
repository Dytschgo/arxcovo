//! Resumable native TEST disposal checkpoints (store v16, ADR 0054 / 0072).
//!
//! Transient-build disposal and the ordinary reaper sweep share this single
//! transaction boundary. Every destructive transition requires durable typed
//! TEST provenance (`vm_native_generations` / `vm_native_slots`), exact scope
//! and identity binding, and monotonic ordering:
//! - slots: `NULL -> 'intended' -> 'absent'` (never arbitrary absence)
//! - generations: `NULL -> 'lease' -> 'directory' -> 'absent'` (blocked by
//!   `vm_native_disposal_order` while any slot remains present)
//!
//! Recovery-only work, retained slots, release-only expiry, sealed goldens,
//! Static VMs/roles and backups never receive deletion authority.
use super::{Leaf, Scope, count, leaf, volume};
use crate::vm::{
    ArtifactSlot, BuildSlots, Id,
    paths::FileIdentity,
    reaper::{CleanupKind, CleanupWork},
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};

/// Durable disposal progress of one bound native generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GenerationDisposal {
    None,
    Lease,
    Directory,
    Absent,
}

/// Durable disposal progress of one identified or verified native slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotDisposal {
    None,
    Intended,
    Absent,
}

/// Durable generation custody state used to plan or resume disposal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GenerationState {
    /// No `vm_native_generations` row exists. Only an uncreated reservation
    /// with zero identified artifacts can be proved absent; no deletion is
    /// ever authorized without durable native provenance.
    Unreserved {
        has_identified_artifact: bool,
    },
    Intended {
        production: FileIdentity,
    },
    Absent {
        production: FileIdentity,
    },
    Directory {
        production: FileIdentity,
        workspace: FileIdentity,
    },
    Quarantined,
    Bound {
        scope: Scope,
        disposal: GenerationDisposal,
    },
}

/// Typed authorization shared by ordinary sweep cleanup and transient-build
/// disposal before `publish_build`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DisposalScope {
    Cleanup(CleanupKind),
    BuildTransient {
        build: Id,
        activity_id: i64,
        primary_generation: Id,
        seed_key: Id,
        verification_generation: Id,
    },
}

impl DisposalScope {
    pub(crate) fn build_transient(activity_id: i64, slots: BuildSlots) -> Self {
        Self::BuildTransient {
            build: slots.id,
            activity_id,
            primary_generation: slots.pair.disk.generation,
            seed_key: slots.seed.key,
            verification_generation: slots.verification.disk.generation,
        }
    }
}

fn valid_test_role(role: &str) -> bool {
    matches!(role, "disk" | "vars" | "seed" | "verification_overlay")
}

/// Verifies that `auth` grants deletion authority for `generation` (and
/// `slot`, when targeting a leaf rather than the generation's lease/dir).
/// Refuses Static VMs/roles, sealed goldens, recovery, release-only expiry,
/// and non-seed mutations on a pending build's primary generation.
pub(crate) fn authorized(
    db: &Connection,
    auth: DisposalScope,
    generation: Id,
    slot: Option<ArtifactSlot>,
) -> Result<bool> {
    if let Some(slot) = slot
        && (slot.generation != generation || slot.key == [0; 16] || !valid_test_role(slot.role))
    {
        return Ok(false);
    }
    // Static generations and sealed golden templates are never disposable.
    if count(
        db,
        "SELECT count(*) FROM vm_generations g
         WHERE g.id=?1 AND g.static_id IS NULL
           AND NOT EXISTS(SELECT 1 FROM vm_build_intents b
               WHERE b.id=g.build_id AND (b.state='sealed' OR b.template_id IS NOT NULL))
           AND NOT EXISTS(SELECT 1 FROM vm_templates t WHERE t.id=g.build_id)",
        [generation],
    )? != 1
    {
        return Ok(false);
    }
    let slot_exists = |db: &Connection, slot: Option<ArtifactSlot>| -> Result<bool> {
        let Some(slot) = slot else {
            return Ok(true);
        };
        Ok(count(
            db,
            "SELECT count(*) FROM vm_artifacts
             WHERE generation_id=?1 AND role=?2 AND object_key=?3",
            params![slot.generation, slot.role, slot.key],
        )? == 1)
    };
    match auth {
        DisposalScope::Cleanup(CleanupKind::Recovery { .. }) => Ok(false),
        DisposalScope::Cleanup(CleanupKind::Build(build)) => Ok(count(
            db,
            "SELECT count(*) FROM vm_build_intents b
             JOIN vm_generations g ON g.build_id=b.id
             WHERE b.id=?1 AND g.id=?2 AND g.active=0 AND g.static_id IS NULL
               AND b.state IN ('pending_disposal','quarantined') AND b.template_id IS NULL
               AND NOT EXISTS(SELECT 1 FROM vm_templates WHERE id=b.id)",
            params![build, generation],
        )? == 1
            && slot_exists(db, slot)?),
        DisposalScope::Cleanup(CleanupKind::Staging { vm, activity_id }) => Ok(count(
            db,
            "SELECT count(*) FROM vm_instances v
             JOIN vm_generations g ON g.vm_id=v.id
             WHERE v.id=?1 AND v.cleanup_id=?2 AND v.state=4 AND v.cleanup_route='staging'
               AND g.id=?3 AND g.active=0 AND g.static_id IS NULL",
            params![vm, activity_id, generation],
        )? == 1
            && slot_exists(db, slot)?),
        DisposalScope::Cleanup(CleanupKind::Expiry { vm, activity_id }) => {
            if super::super::snapshot::expiry_waits(db, vm)?
                || super::recovery::requires_release(db, CleanupKind::Expiry { vm, activity_id })?
            {
                return Ok(false);
            }
            Ok(count(
                db,
                "SELECT count(*) FROM vm_instances v
                 JOIN vm_generations g ON g.vm_id=v.id
                 WHERE v.id=?1 AND v.cleanup_id=?2 AND v.state=4 AND v.cleanup_route='lease'
                   AND g.id=?3 AND g.active=1 AND g.static_id IS NULL
                   AND (SELECT count(*) FROM vm_generations WHERE vm_id=v.id)=1
                   AND (SELECT count(*) FROM vm_snapshots WHERE vm=v.id)=0",
                params![vm, activity_id, generation],
            )? == 1
                && slot_exists(db, slot)?)
        }
        DisposalScope::Cleanup(CleanupKind::Snapshot {
            vm,
            generation: target,
        }) => Ok(generation == target
            && super::super::snapshot::test_disposal_authorized(db, vm, generation)?
            && slot_exists(db, slot)?),
        DisposalScope::BuildTransient {
            build,
            activity_id,
            primary_generation,
            seed_key,
            verification_generation,
        } => {
            if primary_generation == verification_generation
                || seed_key == [0; 16]
                || count(
                    db,
                    "SELECT count(*) FROM vm_build_intents b
                     JOIN vm_operations o ON o.operation_id=b.operation_id
                     WHERE b.id=?1 AND b.state='pending' AND b.template_id IS NULL
                       AND o.activity_id=?2 AND o.action=11 AND o.status=0",
                    params![build, activity_id],
                )? != 1
            {
                return Ok(false);
            }
            if generation == primary_generation {
                // Only the transient seed leaf in the primary generation may be
                // disposed; disk, vars, lease and directory stay sealed/intact.
                let Some(slot) = slot else {
                    return Ok(false);
                };
                return Ok(slot.role == "seed"
                    && slot.key == seed_key
                    && count(
                        db,
                        "SELECT count(*) FROM vm_generations g
                         JOIN vm_artifacts a ON a.generation_id=g.id
                         WHERE g.id=?1 AND g.build_id=?2 AND g.generation=1
                           AND g.active=0 AND g.static_id IS NULL
                           AND a.role='seed' AND a.object_key=?3",
                        params![primary_generation, build, seed_key],
                    )? == 1);
            }
            if generation == verification_generation {
                if let Some(slot) = slot
                    && !matches!(slot.role, "verification_overlay" | "vars")
                {
                    return Ok(false);
                }
                return Ok(count(
                    db,
                    "SELECT count(*) FROM vm_generations g
                     WHERE g.id=?1 AND g.build_id=?2 AND g.generation=2
                       AND g.active=0 AND g.static_id IS NULL",
                    params![verification_generation, build],
                )? == 1
                    && slot_exists(db, slot)?);
            }
            Ok(false)
        }
    }
}

/// Reads the durable native custody state of `generation` for this store host.
/// A row belonging to another host or protocol version is refused.
pub(crate) fn generation_state(db: &Connection, generation: Id) -> Result<GenerationState> {
    let row = db
        .query_row(
            "SELECT host=(SELECT id FROM vm_state_host WHERE singleton=1),
                    protocol,volume,production,state,directory,lease,disposal
             FROM vm_native_generations WHERE generation=?1",
            [generation],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, [u8; 8]>(2)?,
                    row.get::<_, Id>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<Id>>(5)?,
                    row.get::<_, Option<Id>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((same_host, protocol, volume_bytes, prod_file, state, directory, lease, disposal)) =
        row
    else {
        let has_identified_artifact = count(
            db,
            "SELECT count(*) FROM vm_artifacts
             WHERE generation_id=?1
               AND (file_id IS NOT NULL OR directory_id IS NOT NULL OR volume_id IS NOT NULL)",
            [generation],
        )? > 0;
        return Ok(GenerationState::Unreserved {
            has_identified_artifact,
        });
    };
    ensure!(
        same_host && protocol == 1,
        "native generation belongs to another store host or protocol"
    );
    let vol = u64::from_be_bytes(volume_bytes);
    let production = FileIdentity {
        volume: vol,
        file: prod_file,
    };
    Ok(
        match (state.as_str(), directory, lease, disposal.as_deref()) {
            ("intended", None, None, None) => GenerationState::Intended { production },
            ("absent", None, None, None) => GenerationState::Absent { production },
            ("directory", Some(dir), None, None) => GenerationState::Directory {
                production,
                workspace: FileIdentity {
                    volume: vol,
                    file: dir,
                },
            },
            ("quarantined", _, None, None) => GenerationState::Quarantined,
            ("bound", Some(dir), Some(lease_file), disposal) => {
                let disposal = match disposal {
                    None => GenerationDisposal::None,
                    Some("lease") => GenerationDisposal::Lease,
                    Some("directory") => GenerationDisposal::Directory,
                    Some("absent") => GenerationDisposal::Absent,
                    _ => anyhow::bail!("invalid native generation disposal state"),
                };
                GenerationState::Bound {
                    scope: Scope {
                        generation,
                        production,
                        workspace: FileIdentity {
                            volume: vol,
                            file: dir,
                        },
                        lease: FileIdentity {
                            volume: vol,
                            file: lease_file,
                        },
                    },
                    disposal,
                }
            }
            _ => anyhow::bail!("invalid native generation custody row"),
        },
    )
}

/// Reads one slot's durable custody class and disposal progress.
pub(crate) fn slot_disposal(db: &Connection, slot: ArtifactSlot) -> Result<(Leaf, SlotDisposal)> {
    let custody = leaf(db, slot)?;
    let disposal: Option<Option<String>> = db
        .query_row(
            "SELECT disposal FROM vm_native_slots
             WHERE generation=?1 AND role=?2 AND object_key=?3",
            params![slot.generation, slot.role, slot.key],
            |row| row.get(0),
        )
        .optional()?;
    let disposal = match disposal.flatten().as_deref() {
        None => SlotDisposal::None,
        Some("intended") => SlotDisposal::Intended,
        Some("absent") => SlotDisposal::Absent,
        Some(_) => anyhow::bail!("invalid native slot disposal state"),
    };
    Ok((custody, disposal))
}

fn bound_with_disposal(db: &Connection, scope: Scope, allowed: &[Option<&str>]) -> Result<bool> {
    if scope.workspace.volume != scope.production.volume
        || scope.lease.volume != scope.production.volume
    {
        return Ok(false);
    }
    let current: Option<Option<String>> = db
        .query_row(
            "SELECT disposal FROM vm_native_generations
             WHERE generation=?1 AND state='bound' AND protocol=1
               AND host=(SELECT id FROM vm_state_host WHERE singleton=1)
               AND volume=?2 AND production=?3 AND directory=?4 AND lease=?5",
            params![
                scope.generation,
                volume(scope.production),
                scope.production.file,
                scope.workspace.file,
                scope.lease.file
            ],
            |row| row.get(0),
        )
        .optional()?;
    Ok(current.is_some_and(|disposal| allowed.contains(&disposal.as_deref())))
}

/// Commits checked deletion intent for one identified/verified slot **before**
/// unlink. Idempotent when the same slot and identity already hold `'intended'`.
pub(crate) fn intend_slot_disposal(
    db: &Connection,
    auth: DisposalScope,
    scope: Scope,
    slot: ArtifactSlot,
    identity: FileIdentity,
) -> Result<()> {
    ensure!(
        authorized(db, auth, scope.generation, Some(slot))?
            && bound_with_disposal(db, scope, &[None])?
            && identity.volume == scope.production.volume,
        "native slot disposal is unauthorized or outside its bound scope"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_slots SET disposal='intended'
             WHERE generation=?1 AND role=?2 AND object_key=?3
               AND state IN ('identified','verified') AND file=?4
               AND (disposal IS NULL OR disposal='intended')",
            params![slot.generation, slot.role, slot.key, identity.file],
        )? == 1,
        "invalid native slot disposal intent"
    );
    Ok(())
}

/// Checkpoints observed absence of one slot from the bound parent and name.
/// Requires a prior durable `'intended'` (or idempotent `'absent'`) checkpoint.
pub(crate) fn slot_disposed(
    db: &Connection,
    auth: DisposalScope,
    scope: Scope,
    slot: ArtifactSlot,
    identity: FileIdentity,
) -> Result<()> {
    ensure!(
        authorized(db, auth, scope.generation, Some(slot))?
            && bound_with_disposal(db, scope, &[None])?
            && identity.volume == scope.production.volume,
        "native slot absence is unauthorized or outside its bound scope"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_slots SET disposal='absent'
             WHERE generation=?1 AND role=?2 AND object_key=?3
               AND state IN ('identified','verified') AND file=?4
               AND disposal IN ('intended','absent')",
            params![slot.generation, slot.role, slot.key, identity.file],
        )? == 1,
        "native slot absence requires prior durable intent"
    );
    Ok(())
}

/// Checks that no artifact in `generation` carries an identity without a
/// matching `vm_native_slots.disposal='absent'` proof, and that every native
/// slot is durably absent (`state='absent'` or `disposal='absent'`).
fn all_slots_durably_absent(db: &Connection, generation: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_native_slots
         WHERE generation=?1 AND state<>'absent' AND disposal IS NOT 'absent'",
        [generation],
    )? == 0
        && count(
            db,
            "SELECT count(*) FROM vm_artifacts a
             WHERE a.generation_id=?1
               AND (a.file_id IS NOT NULL OR a.directory_id IS NOT NULL OR a.volume_id IS NOT NULL)
               AND NOT EXISTS(SELECT 1 FROM vm_native_slots s
                   WHERE s.generation=a.generation_id AND s.role=a.role
                     AND s.object_key=a.object_key AND s.file=a.file_id
                     AND s.disposal='absent')",
            [generation],
        )? == 0)
}

/// Authorizes lease deletion for an empty bound generation (`NULL -> 'lease'`).
/// Durably closes `bound()` and `vm_native_slot_scope` before the lease handle
/// is closed for handle-deletion.
pub(crate) fn intend_lease_disposal(
    db: &Connection,
    auth: DisposalScope,
    scope: Scope,
) -> Result<()> {
    ensure!(
        authorized(db, auth, scope.generation, None)?
            && bound_with_disposal(db, scope, &[None, Some("lease")])?
            && all_slots_durably_absent(db, scope.generation)?,
        "native lease disposal is unauthorized or slots remain"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_generations SET disposal='lease'
             WHERE generation=?1 AND state='bound' AND protocol=1
               AND host=(SELECT id FROM vm_state_host WHERE singleton=1)
               AND volume=?2 AND production=?3 AND directory=?4 AND lease=?5
               AND (disposal IS NULL OR disposal='lease')",
            params![
                scope.generation,
                volume(scope.production),
                scope.production.file,
                scope.workspace.file,
                scope.lease.file
            ],
        )? == 1,
        "invalid native lease disposal transition"
    );
    Ok(())
}

/// Records observed lease absence and authorizes empty-directory deletion
/// (`'lease' -> 'directory'`).
pub(crate) fn intend_directory_disposal(
    db: &Connection,
    auth: DisposalScope,
    scope: Scope,
) -> Result<()> {
    ensure!(
        authorized(db, auth, scope.generation, None)?
            && bound_with_disposal(db, scope, &[Some("lease"), Some("directory")])?
            && all_slots_durably_absent(db, scope.generation)?,
        "native directory disposal requires prior lease disposal"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_generations SET disposal='directory'
             WHERE generation=?1 AND state='bound' AND protocol=1
               AND host=(SELECT id FROM vm_state_host WHERE singleton=1)
               AND volume=?2 AND production=?3 AND directory=?4 AND lease=?5
               AND disposal IN ('lease','directory')",
            params![
                scope.generation,
                volume(scope.production),
                scope.production.file,
                scope.workspace.file,
                scope.lease.file
            ],
        )? == 1,
        "invalid native directory disposal transition"
    );
    Ok(())
}

/// Records observed empty-directory absence through the held production parent
/// (`'directory' -> 'absent'`).
pub(crate) fn generation_disposed(
    db: &Connection,
    auth: DisposalScope,
    scope: Scope,
) -> Result<()> {
    ensure!(
        authorized(db, auth, scope.generation, None)?
            && bound_with_disposal(db, scope, &[Some("directory"), Some("absent")])?
            && all_slots_durably_absent(db, scope.generation)?,
        "native generation absence requires prior directory disposal"
    );
    ensure!(
        db.execute(
            "UPDATE vm_native_generations SET disposal='absent'
             WHERE generation=?1 AND state='bound' AND protocol=1
               AND host=(SELECT id FROM vm_state_host WHERE singleton=1)
               AND volume=?2 AND production=?3 AND directory=?4 AND lease=?5
               AND disposal IN ('directory','absent')",
            params![
                scope.generation,
                volume(scope.production),
                scope.production.file,
                scope.workspace.file,
                scope.lease.file
            ],
        )? == 1,
        "invalid native generation absence transition"
    );
    Ok(())
}

/// Whether `generation` has durable native proof of complete physical absence
/// (either a bound generation whose lease and directory reached `disposal='absent'`,
/// or an uncreated workspace intent settled to `state='absent'`).
fn generation_proven_absent(db: &Connection, generation: Id) -> Result<bool> {
    if count(
        db,
        "SELECT count(*) FROM vm_generations WHERE id=?1 AND static_id IS NULL",
        [generation],
    )? != 1
    {
        return Ok(false);
    }
    match generation_state(db, generation)? {
        GenerationState::Bound {
            disposal: GenerationDisposal::Absent,
            ..
        } => all_slots_durably_absent(db, generation),
        GenerationState::Absent { .. } => Ok(count(
            db,
            "SELECT count(*) FROM vm_native_slots WHERE generation=?1",
            [generation],
        )? == 0
            && count(
                db,
                "SELECT count(*) FROM vm_artifacts
                 WHERE generation_id=?1
                   AND (file_id IS NOT NULL OR directory_id IS NOT NULL OR volume_id IS NOT NULL)",
                [generation],
            )? == 0),
        _ => Ok(false),
    }
}

/// Revalidates durable native proof inside `settle_cleanup` before freeing
/// eligible claims. Legacy generations (no `vm_native_generations` row) keep
/// their existing settlement behavior unchanged.
pub(crate) fn work_disposed(db: &Connection, work: &CleanupWork) -> Result<bool> {
    if work.release_only || super::recovery::requires_release(db, work.kind)? {
        return Ok(false);
    }
    if matches!(work.kind, CleanupKind::Recovery { .. }) {
        // Recovery-only work never disposes; verify any native retained
        // generation and its retained slots remain bound and non-disposing.
        for artifact in &work.artifacts {
            if !artifact.retain {
                return Ok(false);
            }
            match generation_state(db, artifact.generation)? {
                GenerationState::Unreserved { .. } => {}
                GenerationState::Bound {
                    scope,
                    disposal: GenerationDisposal::None,
                } => {
                    let Some(role) = test_role(&artifact.role) else {
                        return Ok(false);
                    };
                    let slot = ArtifactSlot {
                        generation: artifact.generation,
                        role,
                        key: artifact.key,
                    };
                    let (custody, disposal) = slot_disposal(db, slot)?;
                    let Some(identity) = artifact.identity else {
                        return Ok(false);
                    };
                    let matched = matches!(
                        custody,
                        Leaf::Identified(file) | Leaf::Verified(file, _)
                            if file == identity.file
                    );
                    if !matched
                        || disposal != SlotDisposal::None
                        || identity.volume != scope.production.volume
                        || artifact.directory != Some(scope.workspace.file)
                    {
                        return Ok(false);
                    }
                }
                _ => return Ok(false),
            }
        }
        return Ok(true);
    }

    let mut generations = Vec::new();
    if let CleanupKind::Snapshot { generation, .. } = work.kind {
        generations.push(generation);
    }
    for artifact in &work.artifacts {
        if artifact.retain {
            return Ok(false);
        }
        if !generations.contains(&artifact.generation) {
            generations.push(artifact.generation);
        }
    }
    for generation in generations {
        if count(
            db,
            "SELECT count(*) FROM vm_native_generations WHERE generation=?1",
            [generation],
        )? == 0
        {
            continue;
        }
        if !generation_proven_absent(db, generation)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Revalidates and removes native custody rows for `generation` inside
/// `forget_generation`. Refuses any generation whose native custody is not
/// durably proved absent, preserving foreign-key protection for legacy callers.
pub(crate) fn forget_disposed(db: &Connection, generation: Id) -> Result<()> {
    if count(
        db,
        "SELECT count(*) FROM vm_native_generations WHERE generation=?1",
        [generation],
    )? == 0
    {
        return Ok(());
    }
    ensure!(
        generation_proven_absent(db, generation)?,
        "native generation disposal is incomplete"
    );
    db.execute(
        "DELETE FROM vm_native_slots WHERE generation=?1",
        [generation],
    )?;
    db.execute(
        "DELETE FROM vm_native_generations WHERE generation=?1",
        [generation],
    )?;
    Ok(())
}

/// Revalidates durable transient-build disposal inside `publish_build` before
/// deleting the primary seed artifact and verification generation.
pub(crate) fn forget_disposed_seed(db: &Connection, slots: BuildSlots) -> Result<()> {
    let primary = slots.pair.disk.generation;
    let verification = slots.verification.disk.generation;
    if count(
        db,
        "SELECT count(*) FROM vm_native_generations WHERE generation=?1",
        [primary],
    )? == 0
    {
        // Legacy build: still refuse if verification somehow carries native rows.
        ensure!(
            count(
                db,
                "SELECT count(*) FROM vm_native_generations WHERE generation=?1",
                [verification],
            )? == 0,
            "mixed legacy and native build custody"
        );
        return Ok(());
    }
    // Primary generation must stay bound and non-disposing with intact disk/vars.
    let GenerationState::Bound {
        disposal: GenerationDisposal::None,
        ..
    } = generation_state(db, primary)?
    else {
        anyhow::bail!("primary native generation is not bound for publish");
    };
    for kept in [slots.pair.disk, slots.pair.vars] {
        let (custody, disposal) = slot_disposal(db, kept)?;
        ensure!(
            matches!(custody, Leaf::Identified(_) | Leaf::Verified(_, _))
                && disposal == SlotDisposal::None,
            "primary golden pair lacks intact native custody"
        );
    }
    let (seed_custody, seed_disposal) = slot_disposal(db, slots.seed)?;
    ensure!(
        matches!(seed_custody, Leaf::Identified(_) | Leaf::Verified(_, _))
            && seed_disposal == SlotDisposal::Absent,
        "native build seed is not durably disposed"
    );
    ensure!(
        generation_proven_absent(db, verification)?,
        "native verification generation is not durably disposed"
    );
    ensure!(
        db.execute(
            "DELETE FROM vm_native_slots
             WHERE generation=?1 AND role='seed' AND object_key=?2 AND disposal='absent'",
            params![primary, slots.seed.key],
        )? == 1,
        "invalid native build seed disposal row"
    );
    Ok(())
}

pub(crate) fn test_role(role: &str) -> Option<&'static str> {
    match role {
        "disk" => Some("disk"),
        "vars" => Some("vars"),
        "seed" => Some("seed"),
        "verification_overlay" => Some("verification_overlay"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "disposal_tests.rs"]
mod tests;

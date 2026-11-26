use super::super::{
    Binding, VmStore, cancel_untouched_build, forget_generation, open_sqlite, reserve_build,
};
use super::*;
use crate::service::application::tests::private_temp;
use crate::vm::{
    BuildRequest, BuildSlots, DiskClaim, PairSlots, RequestId, Subject, paths::ArtifactPin,
};

const NOW: i64 = 100;

fn identity(file: u8) -> FileIdentity {
    FileIdentity {
        volume: 7,
        file: [file; 16],
    }
}

fn scope(generation: Id) -> Scope {
    Scope {
        generation,
        production: identity(1),
        workspace: identity(2),
        lease: identity(3),
    }
}

pub(crate) fn build(db: &Connection, nonce: u8) -> Result<BuildSlots> {
    let request = BuildRequest {
        principal: "alice",
        key: RequestId::parse(&format!("{nonce:032x}")).unwrap(),
        recipe: "fece98c2030f405fb6402fc92a79935d",
        digest: [nonce; 32],
        retained: DiskClaim::default(),
        transient: DiskClaim::default(),
    };
    let operation = Binding {
        principal: request.principal,
        key: request.key,
        subject: Subject::Recipe(request.recipe, Some(request.digest)),
        action: 11,
    }
    .record(db, NOW, None, None)?;
    reserve_build(db, request, operation, 8)
}

fn bind(db: &Connection, scope: Scope) -> Result<()> {
    assert_eq!(
        intend_workspace(db, scope.generation, Origin::Build, scope.production)?,
        None
    );
    identify_directory(db, scope.generation, scope.workspace)?;
    bind_workspace(db, scope)
}

fn evidence(file: u8, scope: Scope, bytes: &[u8]) -> ArtifactEvidence {
    ArtifactEvidence {
        identity: identity(file),
        directory: scope.workspace.file,
        pin: ArtifactPin {
            size: bytes.len() as u64,
            sha256: crate::vm::qualification_record::digest(bytes),
        },
    }
}

fn dump(db: &Connection) -> Result<Vec<String>> {
    let mut rows = Vec::new();
    for sql in [
        "SELECT hex(id)||hex(build_id)||quote(vm_id)||generation||active FROM vm_generations",
        "SELECT hex(generation_id)||role||hex(object_key)||state||quote(volume_id)
            ||quote(directory_id)||quote(file_id)||quote(content_digest)||quote(bytes) FROM vm_artifacts",
        "SELECT hex(id)||state||staging_bytes||health_verified FROM vm_build_intents",
        "SELECT hex(operation_id)||status||quote(cleanup_confirmed) FROM vm_operations",
        "SELECT name FROM sqlite_schema WHERE name NOT GLOB 'vm_native_*' AND name<>'vm_artifact_slot'",
    ] {
        let mut statement = db.prepare(&format!("{sql} ORDER BY 1"))?;
        let found = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.extend(found);
    }
    Ok(rows)
}

const DROP_V17: &str = "
DROP TRIGGER vm_native_writer_reservation;
DROP TABLE vm_native_writer;
";

fn assert_v17_writer_schema(db: &Connection) -> Result<()> {
    assert_eq!(count(db, "PRAGMA user_version", [])?, 17);
    assert_eq!(
        count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?,
        0
    );
    let mut statement = db.prepare(
        "SELECT type||':'||name,sql FROM sqlite_schema
         WHERE name GLOB 'vm_native_writer*' ORDER BY 1",
    )?;
    let compiled = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    assert_eq!(compiled.len(), 2);
    for ((name, sql), expected) in compiled.iter().zip([
        "table:vm_native_writer",
        "trigger:vm_native_writer_reservation",
    ]) {
        assert_eq!(name, expected);
        assert_eq!(
            recovery::MIGRATE_V17.matches(sql.as_str()).count(),
            1,
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn v17_upgrade_preserves_v16_custody_without_inventing_writer_provenance() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    let (scope, slot, observed) = store.transact(|db| {
        let slot = build(db, 1)?.seed;
        let scope = scope(slot.generation);
        bind(db, scope)?;
        intend_slot(db, scope, slot)?;
        let observed = evidence(10, scope, b"seed");
        identify_slot(db, scope, slot, observed.identity)?;
        verify_slot(db, scope, slot, observed)?;
        assert_eq!(count(db, "SELECT count(*) FROM vm_native_writer", [])?, 1);
        Ok((scope, slot, observed))
    })?;
    drop(store);

    let db = open_sqlite(&dir.path().join("activity.sqlite3"))?;
    db.execute_batch(DROP_V17)?;
    db.pragma_update(None, "user_version", 16)?;
    drop(db);

    for _ in 0..2 {
        let store = VmStore::open(dir.path(), NOW)?;
        store.transact(|db| {
            assert_v17_writer_schema(db)?;
            assert!(bound(db, scope)?);
            assert_eq!(
                leaf(db, slot)?,
                Leaf::Verified(observed.identity.file, observed.pin)
            );
            assert_eq!(
                count(db, "SELECT count(*) FROM vm_native_generations", [])?,
                1
            );
            assert_eq!(count(db, "SELECT count(*) FROM vm_native_writer", [])?, 0);
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
fn v16_upgrade_preserves_v15_rows_and_infers_no_native_binding() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    let legacy = store.transact(|db| {
        let slots = build(db, 1)?;
        // A legacy worker identified the seed with full evidence before the
        // upgrade: identity, digest and length, with the build charge.
        super::super::created(db, slots.seed)?;
        super::super::identify(
            db,
            slots.seed,
            evidence(10, scope(slots.seed.generation), b"seed"),
        )?;
        Ok(slots)
    })?;
    drop(store);
    // Run recovery once before taking the baseline for the upgrade.
    let before = VmStore::open(dir.path(), NOW)?.transact(|db| dump(db))?;
    let db = open_sqlite(&dir.path().join("activity.sqlite3"))?;
    db.execute_batch(DROP_V17)?;
    db.execute_batch(
        "DROP TRIGGER vm_native_artifact_identity;
         DROP TABLE vm_native_slots; DROP TABLE vm_native_generations;
         DROP INDEX vm_artifact_slot; PRAGMA user_version=15;",
    )?;
    drop(db);

    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        assert_v17_writer_schema(db)?;
        assert_eq!(dump(db)?, before);
        assert_eq!(
            count(
                db,
                "SELECT (SELECT count(*) FROM vm_native_generations)
                      + (SELECT count(*) FROM vm_native_slots)",
                []
            )?,
            0
        );
        assert_eq!(count(db, "SELECT count(*) FROM vm_native_writer", [])?, 0);
        // The exact compiled v16 objects, independent of the v17 additions.
        let mut statement = db.prepare(
            "SELECT type||':'||name FROM sqlite_schema
             WHERE (name GLOB 'vm_native_*' AND name NOT GLOB 'vm_native_writer*')
                OR name='vm_artifact_slot' ORDER BY 1",
        )?;
        let objects = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        assert_eq!(
            objects,
            [
                "index:vm_artifact_slot",
                "table:vm_native_generations",
                "table:vm_native_slots",
                "trigger:vm_native_artifact_identity",
                "trigger:vm_native_disposal_order",
                "trigger:vm_native_generation_scope",
                "trigger:vm_native_origin",
                "trigger:vm_native_slot_identity",
                "trigger:vm_native_slot_scope",
            ]
            .map(String::from)
        );
        // Every compiled native statement is the exact v16 DDL text, so a
        // changed constraint, foreign key or trigger body fails here.
        let mut statement = db.prepare(
            "SELECT name,sql FROM sqlite_schema
             WHERE ((name GLOB 'vm_native_*' AND name NOT GLOB 'vm_native_writer*')
                 OR name='vm_artifact_slot') AND sql IS NOT NULL",
        )?;
        let compiled = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        assert_eq!(compiled.len(), 9);
        for (name, sql) in compiled {
            assert_eq!(MIGRATE_V16.matches(sql.as_str()).count(), 1, "{name}");
        }
        // The fence leaves legacy rows writable by their own protocol.
        let fresh = build(db, 2)?;
        super::super::created(db, fresh.seed)?;
        super::super::identify(
            db,
            fresh.seed,
            evidence(11, scope(fresh.seed.generation), b"seed"),
        )?;
        // An attempted legacy generation never acquires native custody.
        assert!(
            intend_workspace(db, legacy.pair.disk.generation, Origin::Build, identity(1)).is_err()
        );
        Ok(())
    })
}

#[test]
fn workspace_scope_binds_exact_identities_in_order() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        let slots = build(db, 1)?;
        let generation = slots.pair.disk.generation;
        let scope = scope(generation);
        // Nothing binds before its intent.
        assert!(identify_directory(db, generation, scope.workspace).is_err());
        assert!(bind_workspace(db, scope).is_err());
        for wrong in [Origin::Clone, Origin::Restore] {
            assert!(intend_workspace(db, generation, wrong, scope.production).is_err());
        }
        assert_eq!(
            intend_workspace(db, generation, Origin::Build, scope.production)?,
            None
        );
        assert_eq!(
            intend_workspace(db, generation, Origin::Build, scope.production)?,
            None
        );
        assert!(intend_workspace(db, generation, Origin::Build, identity(9)).is_err());
        assert!(bind_workspace(db, scope).is_err());
        let foreign = FileIdentity {
            volume: 8,
            ..scope.workspace
        };
        assert!(identify_directory(db, generation, foreign).is_err());
        assert!(identify_directory(db, generation, scope.production).is_err());
        identify_directory(db, generation, scope.workspace)?;
        identify_directory(db, generation, scope.workspace)?;
        assert!(identify_directory(db, generation, identity(4)).is_err());
        for wrong in [
            Scope {
                workspace: identity(4),
                ..scope
            },
            Scope {
                production: identity(4),
                ..scope
            },
            Scope {
                lease: scope.workspace,
                ..scope
            },
        ] {
            assert!(bind_workspace(db, wrong).is_err());
        }
        assert!(!bound(db, scope)?);
        bind_workspace(db, scope)?;
        bind_workspace(db, scope)?;
        assert!(bound(db, scope)?);
        for substitute in [
            Scope {
                lease: identity(4),
                ..scope
            },
            Scope {
                workspace: identity(4),
                ..scope
            },
            Scope {
                production: identity(4),
                ..scope
            },
            Scope {
                generation: slots.verification.disk.generation,
                ..scope
            },
        ] {
            assert!(!bound(db, substitute)?);
        }
        assert!(
            db.execute(
                "UPDATE vm_native_generations SET directory=?2 WHERE generation=?1",
                params![generation, [4u8; 16]],
            )
            .is_err()
        );

        // Unbound intents settle only from an exact observation of the named
        // object. A directory checkpoint survives a lost acknowledgement.
        use Presence::{Absent, Present};
        use WorkspaceLeaf::{Directory, Lease};
        let state = |generation: Id| -> Result<String> {
            Ok(db.query_row(
                "SELECT state FROM vm_native_generations WHERE generation=?1",
                [generation],
                |row| row.get(0),
            )?)
        };
        let verification = slots.verification.disk.generation;
        intend_workspace(db, verification, Origin::Build, scope.production)?;
        assert!(settle_workspace_intent(db, verification, Lease, Absent).is_err());
        settle_workspace_intent(db, verification, Directory, Absent)?;
        assert_eq!(state(verification)?, "absent");
        assert!(settle_workspace_intent(db, verification, Directory, Absent).is_err());
        intend_workspace(db, verification, Origin::Build, scope.production)?;
        identify_directory(db, verification, identity(5))?;
        settle_workspace_intent(db, verification, Directory, Present(Some(identity(5))))?;
        settle_workspace_intent(db, verification, Lease, Absent)?;
        assert_eq!(state(verification)?, "directory");
        // The recorded directory resumes with its identity.
        assert_eq!(
            intend_workspace(db, verification, Origin::Build, scope.production)?,
            Some(identity(5))
        );
        assert_eq!(state(verification)?, "directory");
        for (leaf, presence) in [
            (Directory, Present(None)),
            (Directory, Present(Some(identity(6)))),
            (Directory, Absent),
            (Lease, Present(None)),
            (Lease, Present(Some(identity(6)))),
        ] {
            // Rebuild the recorded directory state for each wrong observation.
            db.execute(
                "UPDATE vm_native_generations SET state='directory' WHERE generation=?1",
                [verification],
            )?;
            settle_workspace_intent(db, verification, leaf, presence)?;
            assert_eq!(state(verification)?, "quarantined", "{leaf:?} {presence:?}");
        }
        assert!(intend_workspace(db, verification, Origin::Build, scope.production).is_err());
        assert!(settle_workspace_intent(db, generation, Directory, Present(None)).is_err());
        Ok(())
    })
}

#[test]
fn slots_need_intent_then_identity_before_evidence() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        let slots = build(db, 1)?;
        let scope = scope(slots.pair.disk.generation);
        let [disk, vars, seed] = [slots.pair.disk, slots.pair.vars, slots.seed];
        // A slot needs its generation bound first.
        assert!(intend_slot(db, scope, seed).is_err());
        bind(db, scope)?;
        assert_eq!(leaf(db, seed)?, Leaf::NeverAttempted);
        // No identity or evidence without an intent.
        assert!(identify_slot(db, scope, seed, identity(10)).is_err());
        assert!(verify_slot(db, scope, seed, evidence(10, scope, b"seed")).is_err());
        let substitutes = [
            ArtifactSlot {
                key: [0xee; 16],
                ..seed
            },
            ArtifactSlot {
                role: "vars",
                ..seed
            },
            slots.verification.disk,
        ];
        for slot in substitutes {
            assert!(intend_slot(db, scope, slot).is_err());
        }
        let parent = Scope {
            workspace: identity(4),
            ..scope
        };
        assert!(intend_slot(db, parent, seed).is_err());

        intend_slot(db, scope, seed)?;
        intend_slot(db, scope, seed)?;
        assert_eq!(leaf(db, seed)?, Leaf::Intended);
        assert_eq!(
            count(
                db,
                "SELECT count(*) FROM vm_artifacts WHERE object_key=?1 AND state='created'",
                [seed.key]
            )?,
            1
        );
        assert!(identify_slot(db, parent, seed, identity(10)).is_err());
        for reserved in [scope.production, scope.workspace, scope.lease] {
            assert!(identify_slot(db, scope, seed, reserved).is_err());
        }
        assert!(verify_slot(db, scope, seed, evidence(10, scope, b"seed")).is_err());
        identify_slot(db, scope, seed, identity(10))?;
        identify_slot(db, scope, seed, identity(10))?;
        assert!(identify_slot(db, scope, seed, identity(11)).is_err());
        assert_eq!(leaf(db, seed)?, Leaf::Identified([10; 16]));
        assert_eq!(
            count(
                db,
                "SELECT count(*) FROM vm_artifacts WHERE object_key=?1 AND state='created'
                 AND directory_id=?2 AND file_id=?3",
                params![seed.key, scope.workspace.file, [10u8; 16]]
            )?,
            1
        );
        // The same object cannot stand in for a second slot.
        intend_slot(db, scope, disk)?;
        assert!(identify_slot(db, scope, disk, identity(10)).is_err());
        let mut wrong_directory = evidence(10, scope, b"seed");
        wrong_directory.directory = [4; 16];
        for wrong in [evidence(11, scope, b"seed"), wrong_directory] {
            assert!(verify_slot(db, scope, seed, wrong).is_err());
        }
        verify_slot(db, scope, seed, evidence(10, scope, b"seed"))?;
        verify_slot(db, scope, seed, evidence(10, scope, b"seed"))?;
        assert!(verify_slot(db, scope, seed, evidence(10, scope, b"other")).is_err());
        assert!(matches!(leaf(db, seed)?, Leaf::Verified(file, pin)
            if file == [10; 16] && pin.size == 4));
        assert!(settle_slot_intent(db, scope, seed, false).is_err());

        // Exact absence disproves an intent; presence quarantines it.
        settle_slot_intent(db, scope, disk, false)?;
        assert_eq!(leaf(db, disk)?, Leaf::Absent);
        assert!(identify_slot(db, scope, disk, identity(12)).is_err());
        intend_slot(db, scope, disk)?;
        settle_slot_intent(db, scope, disk, true)?;
        assert_eq!(leaf(db, disk)?, Leaf::Unidentified);
        assert!(intend_slot(db, scope, disk).is_err());
        assert!(identify_slot(db, scope, disk, identity(12)).is_err());
        assert_eq!(leaf(db, vars)?, Leaf::NeverAttempted);
        Ok(())
    })
}

#[test]
fn reopen_keeps_custody_charges_and_refuses_the_stale_owner() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    let (slots, scope) = store.transact(|db| {
        let slots = build(db, 1)?;
        let scope = scope(slots.pair.disk.generation);
        bind(db, scope)?;
        intend_slot(db, scope, slots.seed)?;
        identify_slot(db, scope, slots.seed, identity(10))?;
        intend_slot(db, scope, slots.pair.vars)?;
        Ok((slots, scope))
    })?;
    drop(store);
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        assert!(bound(db, scope)?);
        assert_eq!(leaf(db, slots.seed)?, Leaf::Identified([10; 16]));
        assert_eq!(leaf(db, slots.pair.vars)?, Leaf::Intended);
        // Ordinary recovery keeps every attempted artifact and its charge.
        assert_eq!(
            count(
                db,
                "SELECT count(*) FROM vm_build_intents WHERE id=?1 AND state='quarantined'
                 AND staging_bytes=8",
                [slots.id]
            )?,
            1
        );
        // The interrupted worker cannot add or advance custody.
        assert!(intend_slot(db, scope, slots.pair.disk).is_err());
        assert!(identify_slot(db, scope, slots.pair.vars, identity(11)).is_err());
        assert!(verify_slot(db, scope, slots.seed, evidence(10, scope, b"seed")).is_err());
        // Exact reconciliation remains available to the owner path.
        settle_slot_intent(db, scope, slots.pair.vars, true)?;
        assert_eq!(leaf(db, slots.pair.vars)?, Leaf::Unidentified);
        Ok(())
    })
}

#[test]
fn legacy_paths_cannot_forget_or_dispose_native_custody() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    let (slots, scope) = store.transact(|db| {
        let slots = build(db, 1)?;
        let scope = scope(slots.pair.disk.generation);
        bind(db, scope)?;
        Ok((slots, scope))
    })?;
    let generation = scope.generation;
    let artifacts = |store: &VmStore| {
        store.transact(|db| {
            count(
                db,
                "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1",
                [generation],
            )
        })
    };
    // Each refused legacy transaction rolls back as a whole.
    assert!(
        store
            .transact(|db| forget_generation(db, generation))
            .is_err()
    );
    assert_eq!(artifacts(&store)?, 3);
    store.transact(|db| intend_slot(db, scope, slots.seed))?;
    let activity: i64 = store.transact(|db| {
        Ok(db.query_row(
            "SELECT activity_id FROM vm_operations WHERE operation_id=
                (SELECT operation_id FROM vm_build_intents WHERE id=?1)",
            [slots.id],
            |row| row.get(0),
        )?)
    })?;
    assert!(
        store
            .transact(|db| cancel_untouched_build(db, slots, activity, NOW))
            .is_err()
    );
    assert!(
        store
            .transact(|db| forget_generation(db, generation))
            .is_err()
    );
    assert_eq!(artifacts(&store)?, 3);
    store.transact(|db| {
        identify_slot(db, scope, slots.seed, identity(10))?;
        let advance = |db: &Connection| {
            db.execute(
                "UPDATE vm_native_generations SET disposal='lease' WHERE generation=?1",
                [generation],
            )
        };
        assert!(advance(db).is_err());
        db.execute(
            "UPDATE vm_native_slots SET disposal='absent' WHERE object_key=?1",
            [slots.seed.key],
        )?;
        advance(db)?;
        // A disposing generation accepts no new slot or dispatch scope.
        assert!(!bound(db, scope)?);
        assert!(intend_slot(db, scope, slots.pair.vars).is_err());
        Ok(())
    })
}

/// Legacy identity writers cannot replace or pre-empt the identity behind a
/// native checkpoint, through the same transaction a legacy worker uses.
#[test]
fn legacy_identify_is_fenced_behind_native_custody() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    let (slots, scope) = store.transact(|db| {
        let slots = build(db, 1)?;
        let scope = scope(slots.pair.disk.generation);
        bind(db, scope)?;
        intend_slot(db, scope, slots.seed)?;
        Ok((slots, scope))
    })?;
    let legacy = |file: u8| {
        store.transact(|db| super::super::identify(db, slots.seed, evidence(file, scope, b"seed")))
    };
    // Before the native identity, no legacy identity is accepted.
    assert!(legacy(10).is_err());
    store.transact(|db| identify_slot(db, scope, slots.seed, identity(10)))?;
    // A second identity for the same slot is refused, and so is the raw UPDATE.
    assert!(legacy(11).is_err());
    assert!(
        store
            .transact(|db| Ok(db.execute(
                "UPDATE vm_artifacts SET file_id=?2 WHERE object_key=?1",
                params![slots.seed.key, [11u8; 16]],
            )?))
            .is_err()
    );
    let mut moved = evidence(10, scope, b"seed");
    moved.directory = [4; 16];
    assert!(
        store
            .transact(|db| super::super::identify(db, slots.seed, moved))
            .is_err()
    );
    // An exactly consistent record remains possible.
    legacy(10)?;
    store.transact(|db| {
        assert_eq!(
            count(
                db,
                "SELECT count(*) FROM vm_artifacts WHERE object_key=?1 AND file_id=?2
                 AND directory_id=?3",
                params![slots.seed.key, [10u8; 16], scope.workspace.file],
            )?,
            1
        );
        assert_eq!(leaf(db, slots.seed)?, Leaf::Identified([10; 16]));
        Ok(())
    })
}

/// A snapshot copy candidate's checkpoints need the exact pending receipt of
/// its own copy action. The disposal fence interrupts that receipt. A capture
/// candidate is owned by its capture receipt (7c 15), and a receipt of
/// another action never owns a candidate.
#[test]
fn restore_custody_requires_the_pending_restore_receipt() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        let candidate = |nonce: u8, action: i64, receipt: i64| -> Result<(Id, i64, PairSlots)> {
            let vm = [nonce; 16];
            let template = [nonce.wrapping_add(100); 16];
            db.execute(
                "INSERT INTO vm_templates VALUES(?1,1,?2,zeroblob(32),zeroblob(32),40)",
                params![template, [nonce; 32]],
            )?;
            db.execute(
                "INSERT INTO vm_instances(id,template,principal,expires_ms,state)
                 VALUES(?1,?2,'alice',?3,1)",
                params![vm, template, NOW + 60_000],
            )?;
            let active = super::super::new_id()?;
            let staged = super::super::new_id()?;
            for (id, ordinal) in [(active, 1), (staged, 2)] {
                db.execute(
                    "INSERT INTO vm_generations(id,vm_id,generation) VALUES(?1,?2,?3)",
                    params![id, vm, ordinal],
                )?;
            }
            let pair = PairSlots {
                disk: super::super::plan(db, staged, "disk")?,
                vars: super::super::plan(db, staged, "vars")?,
            };
            let operation = Binding {
                principal: "alice",
                key: RequestId::parse(&format!("{:032x}", 0x100 + u32::from(nonce))).unwrap(),
                subject: Subject::Snapshot([nonce; 16], [nonce; 32]),
                action: receipt,
            }
            .record(db, NOW, Some(vm), None)?;
            db.execute(
                "INSERT INTO vm_snapshot_work(activity,vm,action,target,expected,source,candidate,
                    epoch,lineage,phase,persistent,staging)
                 VALUES(?1,?2,?3,?4,?5,?5,?6,?7,?8,'copy',0,8)",
                params![
                    operation.receipt.activity_id,
                    vm,
                    action,
                    [nonce; 16],
                    active,
                    staged,
                    [nonce; 16],
                    [nonce; 32]
                ],
            )?;
            Ok((staged, operation.receipt.activity_id, pair))
        };
        let (restore, activity, pair) = candidate(1, 6, 6)?;
        let scope = scope(restore);
        assert!(intend_workspace(db, restore, Origin::Build, scope.production).is_err());
        intend_workspace(db, restore, Origin::Restore, scope.production)?;
        identify_directory(db, restore, scope.workspace)?;
        bind_workspace(db, scope)?;
        intend_slot(db, scope, pair.disk)?;
        // The disposal fence interrupts the receipt while the work stays in
        // its copy phase.
        super::super::finish(
            db,
            activity,
            crate::vm::Status::Interrupted,
            NOW,
            Some(false),
        )?;
        assert!(identify_slot(db, scope, pair.disk, identity(10)).is_err());
        assert!(intend_slot(db, scope, pair.vars).is_err());
        // A capture candidate binds under its pending capture receipt, and
        // loses custody authority once that receipt is interrupted.
        let (capture, capture_activity, capture_pair) = candidate(2, 5, 5)?;
        let capture_scope = Scope {
            generation: capture,
            ..scope
        };
        assert!(intend_workspace(db, capture, Origin::Clone, scope.production).is_err());
        intend_workspace(db, capture, Origin::Restore, scope.production)?;
        super::super::finish(
            db,
            capture_activity,
            crate::vm::Status::Interrupted,
            NOW,
            Some(false),
        )?;
        assert!(identify_directory(db, capture, capture_scope.workspace).is_err());
        assert!(intend_slot(db, capture_scope, capture_pair.disk).is_err());
        // A work row whose receipt names another action owns nothing.
        let (crossed, _, _) = candidate(3, 6, 5)?;
        assert!(intend_workspace(db, crossed, Origin::Restore, scope.production).is_err());
        Ok(())
    })
}

/// The start pair must have the disk-then-VARS shape of a held refusal.
#[test]
fn start_pair_shape_is_checked_against_durable_slots() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        let slots = build(db, 1)?;
        let scope = scope(slots.pair.disk.generation);
        bind(db, scope)?;
        for (slot, file) in [
            (slots.pair.disk, 10),
            (slots.pair.vars, 11),
            (slots.seed, 12),
        ] {
            intend_slot(db, scope, slot)?;
            identify_slot(db, scope, slot, identity(file))?;
        }
        let e = |file| evidence(file, scope, b"x");
        assert!(pair_matches(
            db,
            scope,
            [slots.pair.disk, slots.pair.vars],
            [e(10), e(11)]
        )?);
        for (pair, evidence) in [
            ([slots.pair.vars, slots.pair.disk], [e(11), e(10)]),
            ([slots.seed, slots.pair.vars], [e(12), e(11)]),
            ([slots.pair.disk, slots.pair.vars], [e(11), e(10)]),
            ([slots.pair.disk, slots.pair.disk], [e(10), e(10)]),
        ] {
            assert!(!pair_matches(db, scope, pair, evidence)?);
        }
        Ok(())
    })
}

/// A writable helper target must still be identified, and unverified, as the
/// exact object in its bound scope.
#[test]
fn identified_target_must_be_the_exact_unverified_object() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        let slots = build(db, 1)?;
        let scope = scope(slots.pair.disk.generation);
        bind(db, scope)?;
        let disk = slots.pair.disk;
        assert!(!identified_as(db, scope, disk, identity(10))?);
        intend_slot(db, scope, disk)?;
        assert!(!identified_as(db, scope, disk, identity(10))?);
        identify_slot(db, scope, disk, identity(10))?;
        assert!(identified_as(db, scope, disk, identity(10))?);
        let foreign = FileIdentity {
            volume: 8,
            ..identity(10)
        };
        let parent = Scope {
            workspace: identity(4),
            ..scope
        };
        assert!(!identified_as(db, scope, disk, identity(11))?);
        assert!(!identified_as(db, scope, disk, foreign)?);
        assert!(!identified_as(db, parent, disk, identity(10))?);
        assert!(!identified_as(db, scope, slots.pair.vars, identity(10))?);
        verify_slot(db, scope, disk, evidence(10, scope, b"disk"))?;
        assert!(!identified_as(db, scope, disk, identity(10))?);
        Ok(())
    })
}

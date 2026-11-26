use super::super::{
    Binding, VmStore, cancel_untouched_build, forget_generation, open_sqlite, reserve_build,
};
use super::*;
use crate::service::application::tests::private_temp;
use crate::vm::{BuildRequest, BuildSlots, DiskClaim, RequestId, Subject, paths::ArtifactPin};

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
    intend_workspace(db, scope.generation, Origin::Build, scope.production)?;
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
        "SELECT hex(generation_id)||role||hex(object_key)||state||quote(file_id) FROM vm_artifacts",
        "SELECT hex(id)||state||staging_bytes FROM vm_build_intents",
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

#[test]
fn v16_upgrade_preserves_v15_rows_and_infers_no_native_binding() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    let legacy = store.transact(|db| {
        let slots = build(db, 1)?;
        // A legacy worker reached identity before the upgrade.
        super::super::created(db, slots.seed)?;
        Ok(slots)
    })?;
    drop(store);
    // Recovery is idempotent: settle it at v16 before taking the baseline.
    let before = VmStore::open(dir.path(), NOW)?.transact(|db| dump(db))?;
    let db = open_sqlite(&dir.path().join("activity.sqlite3"))?;
    db.execute_batch(
        "DROP TABLE vm_native_slots; DROP TABLE vm_native_generations;
         DROP INDEX vm_artifact_slot; PRAGMA user_version=15;",
    )?;
    drop(db);

    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        assert_eq!(count(db, "PRAGMA user_version", [])?, 16);
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
        assert_eq!(
            count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?,
            0
        );
        // The exact compiled v16 objects.
        let mut statement = db.prepare(
            "SELECT type||':'||name FROM sqlite_schema
             WHERE name GLOB 'vm_native_*' OR name='vm_artifact_slot' ORDER BY 1",
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
                "trigger:vm_native_disposal_order",
                "trigger:vm_native_generation_scope",
                "trigger:vm_native_origin",
                "trigger:vm_native_slot_identity",
                "trigger:vm_native_slot_scope",
            ]
            .map(String::from)
        );
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
        intend_workspace(db, generation, Origin::Build, scope.production)?;
        intend_workspace(db, generation, Origin::Build, scope.production)?;
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

        // Unbound intents settle only from an exact observation.
        let verification = slots.verification.disk.generation;
        intend_workspace(db, verification, Origin::Build, scope.production)?;
        settle_workspace_intent(db, verification, false)?;
        assert!(settle_workspace_intent(db, verification, false).is_err());
        intend_workspace(db, verification, Origin::Build, scope.production)?;
        identify_directory(db, verification, identity(5))?;
        settle_workspace_intent(db, verification, false)?;
        settle_workspace_intent(db, verification, true)?;
        assert!(intend_workspace(db, verification, Origin::Build, scope.production).is_err());
        assert!(settle_workspace_intent(db, generation, true).is_err());
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

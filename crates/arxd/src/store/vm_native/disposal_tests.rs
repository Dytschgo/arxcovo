use super::*;
use crate::service::application::tests::private_temp;
use crate::store::vm::{
    self as store, Binding, VmStore,
    native::{
        Origin, Presence, WorkspaceLeaf, bind_workspace, bound, identify_directory, identify_slot,
        intend_slot, intend_workspace, settle_slot_intent, settle_workspace_intent, verify_slot,
    },
};
use crate::vm::{
    ArtifactEvidence, BuildRequest, DiskClaim, Guest, RequestId, Status, Subject,
    paths::ArtifactPin,
    reaper::{CleanupArtifact, ReconcileOutcome},
};
use rusqlite::params;

const NOW: i64 = 1_800_000_000_000;

fn id(byte: u8) -> FileIdentity {
    FileIdentity {
        volume: 7,
        file: [byte; 16],
    }
}

fn scope(generation: Id, dir: u8, lease: u8) -> Scope {
    Scope {
        generation,
        production: id(1),
        workspace: id(dir),
        lease: id(lease),
    }
}

fn evidence(file: u8, gen_scope: Scope, bytes: &[u8]) -> ArtifactEvidence {
    ArtifactEvidence {
        identity: id(file),
        directory: gen_scope.workspace.file,
        pin: ArtifactPin {
            size: bytes.len() as u64,
            sha256: crate::vm::qualification_record::digest(bytes),
        },
    }
}

fn bind(db: &Connection, gen_scope: Scope, origin: Origin) -> Result<()> {
    assert_eq!(
        intend_workspace(db, gen_scope.generation, origin, gen_scope.production)?,
        None
    );
    identify_directory(db, gen_scope.generation, gen_scope.workspace)?;
    bind_workspace(db, gen_scope)
}

fn build_request(nonce: u8) -> BuildRequest<'static> {
    BuildRequest {
        principal: "alice",
        key: RequestId::parse(&format!("{nonce:032x}")).unwrap(),
        recipe: "fece98c2030f405fb6402fc92a79935d",
        digest: [nonce; 32],
        retained: DiskClaim {
            images: 4096,
            metadata: 0,
            staging: 0,
        },
        transient: DiskClaim {
            images: 4096,
            metadata: 0,
            staging: 0,
        },
    }
}

fn reserve_pending_build(db: &Connection, nonce: u8) -> Result<(i64, BuildSlots)> {
    let request = build_request(nonce);
    let operation = Binding {
        principal: request.principal,
        key: request.key,
        subject: Subject::Recipe(request.recipe, Some(request.digest)),
        action: 11,
    }
    .record(db, NOW, None, None)?;
    let activity = operation.receipt.activity_id;
    let slots = store::reserve_build(db, request, operation, 8192)?;
    Ok((activity, slots))
}

#[test]
fn slot_and_generation_disposal_enforce_monotonic_intent_and_block_arbitrary_absence() -> Result<()>
{
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        let (activity, slots) = reserve_pending_build(db, 1)?;
        let primary = scope(slots.pair.disk.generation, 2, 3);
        let verify = scope(slots.verification.disk.generation, 4, 5);
        bind(db, primary, Origin::Build)?;
        bind(db, verify, Origin::Build)?;
        for (slot, file, scope) in [
            (slots.pair.disk, 10, primary),
            (slots.pair.vars, 11, primary),
            (slots.seed, 12, primary),
            (slots.verification.disk, 13, verify),
            (slots.verification.vars, 14, verify),
        ] {
            intend_slot(db, scope, slot)?;
            identify_slot(db, scope, slot, id(file))?;
            store::identify(db, slot, evidence(file, scope, b"data"))?;
        }
        let auth = DisposalScope::build_transient(activity, slots);

        // BuildTransient permits ONLY seed on primary (never disk/vars/lease/dir).
        assert!(intend_slot_disposal(db, auth, primary, slots.pair.disk, id(10)).is_err());
        assert!(intend_slot_disposal(db, auth, primary, slots.pair.vars, id(11)).is_err());
        assert!(intend_lease_disposal(db, auth, primary).is_err());

        // Arbitrary absence without prior durable intent is refused.
        assert!(slot_disposed(db, auth, primary, slots.seed, id(12)).is_err());
        // Wrong identity or wrong parent/scope is refused.
        assert!(intend_slot_disposal(db, auth, primary, slots.seed, id(99)).is_err());
        let wrong_parent = Scope {
            production: id(99),
            ..primary
        };
        assert!(intend_slot_disposal(db, auth, wrong_parent, slots.seed, id(12)).is_err());

        // Exact seed disposal: NULL -> intended -> absent (idempotent on replay).
        intend_slot_disposal(db, auth, primary, slots.seed, id(12))?;
        intend_slot_disposal(db, auth, primary, slots.seed, id(12))?;
        assert_eq!(slot_disposal(db, slots.seed)?.1, SlotDisposal::Intended);
        slot_disposed(db, auth, primary, slots.seed, id(12))?;
        slot_disposed(db, auth, primary, slots.seed, id(12))?;
        assert_eq!(slot_disposal(db, slots.seed)?.1, SlotDisposal::Absent);
        // Once absent, cannot revert to intended.
        assert!(intend_slot_disposal(db, auth, primary, slots.seed, id(12)).is_err());

        // Verification generation cannot advance lease disposal while slots remain.
        assert!(intend_lease_disposal(db, auth, verify).is_err());
        assert!(intend_directory_disposal(db, auth, verify).is_err());
        assert!(generation_disposed(db, auth, verify).is_err());

        for (slot, file) in [(slots.verification.disk, 13), (slots.verification.vars, 14)] {
            intend_slot_disposal(db, auth, verify, slot, id(file))?;
            slot_disposed(db, auth, verify, slot, id(file))?;
        }

        // Generation disposal cannot skip lease or directory steps.
        assert!(intend_directory_disposal(db, auth, verify).is_err());
        assert!(generation_disposed(db, auth, verify).is_err());
        intend_lease_disposal(db, auth, verify)?;
        intend_lease_disposal(db, auth, verify)?;
        assert!(!bound(db, verify)?);
        assert!(generation_disposed(db, auth, verify).is_err());

        // publish_build refuses until verification generation is durably absent.
        // Use a savepoint so the refused transaction rolls back like `VmStore::transact`.
        let proof = [
            evidence(10, primary, b"data"),
            evidence(11, primary, b"data"),
        ];
        db.execute_batch("SAVEPOINT try_publish;")?;
        assert!(
            store::publish_build(
                db,
                activity,
                slots,
                build_request(1),
                Guest::UbuntuX86_64,
                proof,
                NOW
            )
            .is_err()
        );
        db.execute_batch("ROLLBACK TO try_publish; RELEASE try_publish;")?;

        intend_directory_disposal(db, auth, verify)?;
        intend_directory_disposal(db, auth, verify)?;
        assert!(intend_lease_disposal(db, auth, verify).is_err());
        generation_disposed(db, auth, verify)?;
        generation_disposed(db, auth, verify)?;
        assert_eq!(
            generation_state(db, verify.generation)?,
            GenerationState::Bound {
                scope: verify,
                disposal: GenerationDisposal::Absent,
            }
        );

        // Now publish_build succeeds, forgets verification and seed native rows,
        // and keeps the sealed golden's native custody intact.
        store::publish_build(
            db,
            activity,
            slots,
            build_request(1),
            Guest::UbuntuX86_64,
            proof,
            NOW,
        )?;
        assert_eq!(
            count(
                db,
                "SELECT count(*) FROM vm_native_generations WHERE generation=?1",
                [verify.generation]
            )?,
            0
        );
        assert_eq!(
            count(
                db,
                "SELECT count(*) FROM vm_native_slots WHERE generation=?1 AND role='seed'",
                [primary.generation]
            )?,
            0
        );
        assert!(bound(db, primary)?);
        // Sealed golden cannot be disposed under any scope.
        assert!(!authorized(
            db,
            DisposalScope::Cleanup(CleanupKind::Build(slots.id)),
            primary.generation,
            Some(slots.pair.disk)
        )?);
        assert!(!authorized(db, auth, primary.generation, Some(slots.seed))?);
        Ok(())
    })
}

#[test]
fn settle_cleanup_revalidates_durable_native_disposal_and_fences_static_and_recovery() -> Result<()>
{
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        // 1. Failed build: settle_cleanup refuses while native disposal is incomplete.
        let (activity, slots) = reserve_pending_build(db, 2)?;
        let primary = scope(slots.pair.disk.generation, 20, 21);
        bind(db, primary, Origin::Build)?;
        intend_slot(db, primary, slots.seed)?;
        identify_slot(db, primary, slots.seed, id(22))?;
        store::identify(db, slots.seed, evidence(22, primary, b"seed"))?;
        // Leave verification generation in `intended` -> `absent` (uncreated reservation).
        intend_workspace(db, slots.verification.disk.generation, Origin::Build, id(1))?;
        store::abandon_build(db, slots.id, activity, NOW)?;

        let kind = CleanupKind::Build(slots.id);
        let work = store::cleanup_work(db, kind)?.expect("build cleanup work");
        let done = ReconcileOutcome {
            released: true,
            disposed: true,
        };
        // Volatile "disposed: true" without durable checkpoints MUST NOT settle.
        assert!(!store::settle_cleanup(db, &work, NOW, done)?);
        assert_eq!(
            count(
                db,
                "SELECT count(*) FROM vm_build_intents WHERE id=?1 AND staging_bytes=8192",
                [slots.id]
            )?,
            1
        );

        // Complete durable disposal for primary and settle uncreated verification.
        let auth = DisposalScope::Cleanup(kind);
        intend_slot_disposal(db, auth, primary, slots.seed, id(22))?;
        slot_disposed(db, auth, primary, slots.seed, id(22))?;
        intend_lease_disposal(db, auth, primary)?;
        intend_directory_disposal(db, auth, primary)?;
        generation_disposed(db, auth, primary)?;
        // Verification is still `intended` (not `absent`), so settlement still refuses.
        assert!(!store::settle_cleanup(db, &work, NOW, done)?);
        settle_workspace_intent(
            db,
            slots.verification.disk.generation,
            WorkspaceLeaf::Directory,
            Presence::Absent,
        )?;
        assert!(store::settle_cleanup(db, &work, NOW, done)?);
        assert_eq!(
            count(
                db,
                "SELECT count(*) FROM vm_build_intents WHERE id=?1",
                [slots.id]
            )?,
            0
        );
        assert_eq!(
            count(db, "SELECT count(*) FROM vm_native_generations", [])?,
            0
        );

        // 2. Recovery work carries no deletion authority.
        let template = [0x31; 16];
        let vm = [0x32; 16];
        db.execute(
            "INSERT INTO vm_templates(id,guest,recipe_digest,disk_digest,vars_digest,bytes)
             VALUES(?1,1,?2,?2,?2,1)",
            params![template, [3u8; 32]],
        )?;
        db.execute(
            "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',?3)",
            params![vm, template, NOW + 60_000],
        )?;
        let clone_act = Binding {
            principal: "alice",
            key: RequestId::parse("00000000000000640000000000000032").unwrap(),
            subject: Subject::Template(template, [3; 32]),
            action: 1,
        }
        .record(db, NOW, Some(vm), Some(NOW + 60_000))?
        .receipt
        .activity_id;
        db.execute(
            "INSERT INTO vm_disk_claims(activity_id,vm,persistent,staging) VALUES(?1,?2,4096,0)",
            params![clone_act, vm],
        )?;
        let pair = store::plan_clone(db, vm)?;
        let clone_scope = scope(pair.disk.generation, 30, 31);
        bind(db, clone_scope, Origin::Clone)?;
        let mut ev = Vec::new();
        for (slot, file) in [(pair.disk, 32), (pair.vars, 33)] {
            intend_slot(db, clone_scope, slot)?;
            identify_slot(db, clone_scope, slot, id(file))?;
            let proof = evidence(file, clone_scope, b"vm");
            verify_slot(db, clone_scope, slot, proof)?;
            ev.push(proof);
        }
        assert_eq!(
            store::settle_clone(db, clone_act, pair, Some([ev[0], ev[1]]), true, NOW, None)?,
            Status::Succeeded
        );
        // Move VM to recovery (state=3).
        store::begin_recovery(db, vm, NOW)?;
        let rec_act: i64 = db.query_row(
            "SELECT cleanup_id FROM vm_instances WHERE id=?1",
            [vm],
            |r| r.get(0),
        )?;
        let rec_kind = CleanupKind::Recovery {
            vm,
            activity_id: rec_act,
        };
        assert!(!authorized(
            db,
            DisposalScope::Cleanup(rec_kind),
            clone_scope.generation,
            Some(pair.disk)
        )?);
        assert!(!authorized(
            db,
            DisposalScope::Cleanup(rec_kind),
            clone_scope.generation,
            None
        )?);
        assert!(
            intend_slot_disposal(
                db,
                DisposalScope::Cleanup(rec_kind),
                clone_scope,
                pair.disk,
                id(32)
            )
            .is_err()
        );
        let rec_work = store::cleanup_work(db, rec_kind)?.expect("recovery work");
        assert!(work_disposed(db, &rec_work)?);
        assert!(store::settle_cleanup(db, &rec_work, NOW, done)?);
        // Active pair and persistent claim are retained after recovery.
        assert_eq!(
            count(db, "SELECT count(*) FROM vm_disk_claims WHERE vm=?1", [vm])?,
            1
        );

        // 3. Static VM roles and static_id generations are strictly refused.
        let static_vm = [0x41; 16];
        let static_gen = [0x42; 16];
        db.execute(
            "INSERT INTO vm_static_instances(id,name,principal,host,template,source_generation,
                recipe_digest,disk_digest,vars_digest,state,persistent,staging)
             SELECT ?1,'static-a','alice',id,?2,?3,?4,?4,?4,1,4096,0
             FROM vm_state_host WHERE singleton=1",
            params![static_vm, template, clone_scope.generation, [4u8; 32]],
        )?;
        db.execute(
            "INSERT INTO vm_generations(id,static_id,generation,active) VALUES(?1,?2,1,1)",
            params![static_gen, static_vm],
        )?;
        let static_slot = ArtifactSlot {
            generation: static_gen,
            role: "static_disk",
            key: [0x43; 16],
        };
        assert!(!authorized(
            db,
            DisposalScope::Cleanup(CleanupKind::Snapshot {
                vm: static_vm,
                generation: static_gen,
            }),
            static_gen,
            Some(static_slot)
        )?);
        assert!(test_role("static_disk").is_none());
        assert!(test_role("static_vars").is_none());
        assert!(test_role("backup").is_none());

        // 4. Release-only expiry work is refused.
        let release_only_work = CleanupWork {
            kind: CleanupKind::Expiry {
                vm,
                activity_id: rec_act,
            },
            pending_worker: None,
            snapshot_phase: None,
            artifacts: vec![CleanupArtifact {
                generation: clone_scope.generation,
                role: "disk".into(),
                key: pair.disk.key,
                directory: Some(clone_scope.workspace.file),
                identity: Some(id(32)),
                retain: false,
            }],
            accounted: true,
            release_only: true,
        };
        assert!(!work_disposed(db, &release_only_work)?);
        Ok(())
    })
}

#[test]
fn intended_slot_settled_absent_unblocks_generation_disposal() -> Result<()> {
    let dir = private_temp();
    let store = VmStore::open(dir.path(), NOW)?;
    store.transact(|db| {
        let (activity, slots) = reserve_pending_build(db, 5)?;
        let primary = scope(slots.pair.disk.generation, 50, 51);
        bind(db, primary, Origin::Build)?;
        intend_slot(db, primary, slots.seed)?;
        identify_slot(db, primary, slots.seed, id(52))?;
        store::identify(db, slots.seed, evidence(52, primary, b"seed"))?;
        intend_slot(db, primary, slots.pair.disk)?; // never identified
        store::abandon_build(db, slots.id, activity, NOW)?;
        let auth = DisposalScope::Cleanup(CleanupKind::Build(slots.id));

        intend_slot_disposal(db, auth, primary, slots.seed, id(52))?;
        slot_disposed(db, auth, primary, slots.seed, id(52))?;
        // Still blocked because pair.disk is `intended`.
        assert!(intend_lease_disposal(db, auth, primary).is_err());
        // Settling pair.disk as absent unblocks lease/dir disposal.
        settle_slot_intent(db, primary, slots.pair.disk, false)?;
        intend_lease_disposal(db, auth, primary)?;
        intend_directory_disposal(db, auth, primary)?;
        generation_disposed(db, auth, primary)?;
        assert!(generation_proven_absent(db, primary.generation)?);
        Ok(())
    })
}

use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn mapped_start_and_dedup_are_atomic_and_scoped() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Activity::open(root.path()).unwrap();
    let row = store.start_delegated_build([1; 32], [2; 32], None).unwrap();
    assert!(store.start_delegated_build([1; 32], [3; 32], None).is_err());
    assert!(
        store
            .delegated_build([1; 32], [3; 32], false)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .delegated_build([1; 32], [2; 32], true)
            .unwrap()
            .is_none()
    );
    let count: u32 = store
        .db
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM activity", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let record = store
        .delegated_build([1; 32], [2; 32], false)
        .unwrap()
        .unwrap();
    assert_eq!(record.activity_id, row);
    assert_eq!(record.outcome, Outcome::Started);
    drop(store);
    let reopened = Activity::open(root.path()).unwrap();
    let old = reopened
        .delegated_build([1; 32], [2; 32], false)
        .unwrap()
        .unwrap();
    assert_eq!(old.outcome, Outcome::Interrupted);
}

#[test]
fn full_retention_protects_cleanup_confirmed_rows_without_identity() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Activity::open(root.path()).unwrap();
    let scope = [7; 32];
    let mut last = 0;
    for i in 1..=128 {
        last = store.start_delegated_build([i; 32], scope, None).unwrap();
        if i != 128 {
            store
                .finish_typed(
                    last,
                    Outcome::Failed,
                    &Metadata {
                        cleanup_confirmed: Some(true),
                        ..Metadata::default()
                    },
                )
                .unwrap();
        }
    }
    assert!(
        store
            .start_delegated_build_checked([129; 32], scope, None, || true)
            .unwrap()
            .is_none()
    );
    let first = store
        .delegated_build([1; 32], scope, false)
        .unwrap()
        .unwrap();
    assert_eq!(first.cleanup_confirmed, Some(true));
    assert_eq!(first.uid, None);
    assert!(!first.terminal());
    store
        .finish_typed(last, Outcome::Refused, &Metadata::default())
        .unwrap();
    store.start_delegated_build([129; 32], scope, None).unwrap();
    assert!(
        store
            .delegated_build([128; 32], scope, false)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .delegated_build([1; 32], scope, false)
            .unwrap()
            .is_some()
    );
}

#[test]
fn reopened_forwarded_completion_requires_the_matching_unresolved_row() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Activity::open(root.path()).unwrap();
    let scope = [7; 32];
    let operation = [1; 32];
    let id = store
        .start_delegated_build(operation, scope, Some("fixture"))
        .unwrap();
    let inbound = store.start_delegated_build([2; 32], scope, None).unwrap();
    let ordinary = store.start_dedicated("fixture", "run_command").unwrap();
    assert_eq!(store.forwarded_builds(scope).unwrap(), [operation]);
    drop(store);

    let store = Activity::open(root.path()).unwrap();
    let lookup = |store: &Activity| {
        store
            .delegated_build(operation, scope, true)
            .unwrap()
            .unwrap()
    };
    assert_eq!(lookup(&store).outcome, Outcome::Interrupted);
    assert_eq!(store.forwarded_builds(scope).unwrap(), [operation]);

    for (id, operation, scope) in [
        (ordinary, operation, scope),
        (inbound, [2; 32], scope),
        (id, operation, [8; 32]),
        (id, [3; 32], scope),
    ] {
        assert!(
            store
                .finish_forwarded_build(
                    id,
                    operation,
                    scope,
                    Outcome::Refused,
                    &Metadata::default()
                )
                .is_err()
        );
    }
    for outcome in [
        Outcome::Started,
        Outcome::Interrupted,
        Outcome::Rejected,
        Outcome::Succeeded,
    ] {
        assert!(
            store
                .finish_forwarded_build(id, operation, scope, outcome, &Metadata::default())
                .is_err()
        );
    }
    assert!(
        store
            .finish_typed(id, Outcome::Refused, &Metadata::default())
            .is_err()
    );
    assert_eq!(lookup(&store).outcome, Outcome::Interrupted);
    for id in [ordinary, inbound] {
        let outcome: String = store
            .db
            .lock()
            .unwrap()
            .query_row("SELECT outcome FROM activity WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(outcome, "interrupted");
    }
    let metadata = Metadata {
        exit_code: Some(0),
        cleanup_confirmed: Some(true),
        observed_kind: Some(ObservedKind::UnixUid),
        observed_id: Some("21002".into()),
        observed_gid: Some(21003),
        ..Default::default()
    };
    assert!(
        store
            .finish_forwarded_build(id, operation, scope, Outcome::Succeeded, &metadata)
            .unwrap()
    );
    assert!(
        !store
            .finish_forwarded_build(id, operation, scope, Outcome::Refused, &Metadata::default())
            .unwrap()
    );
    assert!(store.forwarded_builds(scope).unwrap().is_empty());
    drop(store);

    let store = Activity::open(root.path()).unwrap();
    let result = lookup(&store);
    assert_eq!(result.activity_id, id);
    assert_eq!(result.outcome, Outcome::Succeeded);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.cleanup_confirmed, Some(true));
    assert_eq!(result.uid, Some(21002));
    assert_eq!(result.gid, Some(21003));
    assert!(result.terminal());
}

#[test]
fn forwarded_retention_reclaims_only_the_resolved_recovered_row() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Activity::open(root.path()).unwrap();
    let scope = [7; 32];
    let principal = Some("fixture");
    for i in 1..=128 {
        store
            .start_delegated_build([i; 32], scope, principal)
            .unwrap();
    }
    drop(store);

    let store = Activity::open(root.path()).unwrap();
    assert!(
        store
            .start_delegated_build_checked([129; 32], scope, principal, || true)
            .unwrap()
            .is_none()
    );
    let lookup = |i| store.delegated_build([i; 32], scope, true).unwrap();
    let recovered = lookup(128).unwrap();
    assert_eq!(recovered.outcome, Outcome::Interrupted);
    assert!(
        store
            .finish_forwarded_build(
                recovered.activity_id,
                [128; 32],
                scope,
                Outcome::Refused,
                &Metadata::default(),
            )
            .unwrap()
    );
    store
        .start_delegated_build([129; 32], scope, principal)
        .unwrap();
    assert!(lookup(128).is_none());
    assert_eq!(lookup(129).unwrap().outcome, Outcome::Started);
    for i in 1..128 {
        assert_eq!(lookup(i).unwrap().outcome, Outcome::Interrupted);
    }
}

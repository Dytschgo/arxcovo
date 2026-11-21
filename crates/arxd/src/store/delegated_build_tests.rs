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

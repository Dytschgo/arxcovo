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

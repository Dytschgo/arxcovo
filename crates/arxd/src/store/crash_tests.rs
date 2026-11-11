//! Normal-user Windows crash/recovery evidence, never installed-service proof.
use super::*;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::Instant,
};

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "subprocess-only SQLite crash fixture; parent kills without unwinding"]
fn hot_journal_child() {
    let dir = std::path::PathBuf::from(std::env::var_os("ARXCOVO_CRASH_FIXTURE").unwrap());
    let store = Activity::open_pinned_fixture(&dir).expect("child pinned open");
    store.start("child-agent", "run_command").unwrap();
    let mut db = store.db.lock().unwrap();
    db.execute_batch("PRAGMA cache_size=1; PRAGMA cache_spill=ON;")
        .unwrap();
    let tx = db.transaction().unwrap();
    tx.execute(
        "UPDATE local_host_identity SET host_id=?1",
        ["f".repeat(64)],
    )
    .unwrap();
    tx.execute("UPDATE activity SET outcome='succeeded'", [])
        .unwrap();
    // Force dirty pages to disk while the transaction remains unfinished.
    tx.cache_flush().unwrap();
    std::fs::write(dir.join("ready"), b"hot").unwrap();
    std::thread::sleep(Duration::from_secs(20));
    panic!("parent failed to terminate child before rollback");
}

#[test]
fn pinned_service_recovers_actual_hot_journal() {
    let dir = crate::service::application::tests::private_temp();
    for name in [
        "activity.lock",
        "activity.sqlite3",
        "activity.sqlite3-journal",
    ] {
        std::fs::write(dir.path().join(name), []).unwrap();
    }
    let store = Activity::open_pinned_fixture(dir.path()).expect("initial pinned open");
    let host_id = store.host_id().unwrap();
    let pending = store.start("crash-agent", "run_command").unwrap();
    let foreign = store.start("other-agent", "run_command").unwrap();
    let lifecycle = store.start_lifecycle(Lifecycle::Start).unwrap();
    drop(store);
    let mut child = KillOnDrop(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store::crash_tests::hot_journal_child",
                "--ignored",
                "--nocapture",
            ])
            .env("ARXCOVO_CRASH_FIXTURE", dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !dir.path().join("ready").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child exited before hot journal"
        );
        assert!(Instant::now() < deadline, "child readiness deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    let journal = dir.path().join("activity.sqlite3-journal");
    let mut header = [0u8; 28];
    std::fs::File::open(&journal)
        .unwrap()
        .read_exact(&mut header)
        .unwrap();
    assert_eq!(
        &header[..8],
        &[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7],
        "genuine hot rollback journal magic"
    );
    assert!(
        u32::from_be_bytes(header[8..12].try_into().unwrap()) > 0,
        "journal has page records"
    );
    assert!(std::fs::metadata(&journal).unwrap().len() > 512);
    // Same no-delete-sharing pins are live across SQLite recovery, including
    // the DB and lock. Assert the native fence, rather than assuming it exists.
    let pins = crate::service::native::state_fixture_pins(dir.path()).unwrap();
    for name in [
        "activity.lock",
        "activity.sqlite3",
        "activity.sqlite3-journal",
    ] {
        assert!(std::fs::remove_file(dir.path().join(name)).is_err());
    }
    let store = Activity::open_inner(dir.path(), pins, true).expect("pinned hot-journal reopen");
    assert_eq!(
        store.host_id().unwrap(),
        host_id,
        "uncommitted identity rolled back"
    );
    {
        let db = store.db.lock().unwrap();
        for id in [pending, foreign, lifecycle] {
            let outcome: String = db
                .query_row("SELECT outcome FROM activity WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(outcome, "interrupted");
        }
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            3
        );
        assert_eq!(
            db.pragma_query_value::<String, _>(None, "journal_mode", |r| r.get(0))
                .unwrap(),
            "truncate"
        );
        assert_eq!(
            db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert_eq!(
            db.query_row(
                "SELECT outcome FROM activity WHERE principal='child-agent'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "interrupted"
        );
        assert_eq!(
            db.pragma_query_value::<String, _>(None, "locking_mode", |r| r.get(0))
                .unwrap(),
            "normal"
        );
    }
    let read = store.start("crash-agent", "list_activity").unwrap();
    let page = store
        .list("crash-agent", read, &serde_json::from_str("{}").unwrap())
        .unwrap();
    assert_eq!(page.activities.len(), 1);
    assert_eq!(page.activities[0].id, pending);
    let output = serde_json::to_string(&page).unwrap();
    for forbidden in [
        "other-agent",
        "child-agent",
        "service_start",
        "host_id",
        "argv",
        "environment",
        "stdout",
        &dir.path().to_string_lossy(),
    ] {
        assert!(
            !output.contains(forbidden),
            "private data in activity response"
        );
    }
    store.finish(read, "succeeded").unwrap();
    assert!(
        Activity::open_pinned_fixture(dir.path()).is_err(),
        "exclusive activity lock"
    );
    for name in ["activity.sqlite3-wal", "activity.sqlite3-shm"] {
        assert!(!dir.path().join(name).exists());
    }
    assert_eq!(std::fs::metadata(journal).unwrap().len(), 0);
    drop(store);
    assert_eq!(
        Activity::open_pinned_fixture(dir.path())
            .unwrap()
            .host_id()
            .unwrap(),
        host_id
    );
}

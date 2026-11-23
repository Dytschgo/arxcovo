use super::*;
use crate::development::Execution;
use std::sync::Arc;

struct LockedStore {
    release: Option<std::sync::mpsc::Sender<()>>,
    helper: Option<std::thread::JoinHandle<()>>,
}
impl LockedStore {
    fn hold(activity: Arc<Activity>, id: i64) -> Self {
        let (ready, acquired) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let helper = std::thread::spawn(move || {
            let db = activity.db.lock().unwrap();
            assert_eq!(
                db.query_row::<String, _, _>(
                    "SELECT outcome FROM activity WHERE id=?1",
                    [id],
                    |r| r.get(0)
                )
                .unwrap(),
                "started"
            );
            let _ = ready.send(());
            let _ = wait.recv();
        });
        let held = Self {
            release: Some(release),
            helper: Some(helper),
        };
        acquired.recv_timeout(Duration::from_secs(2)).unwrap();
        held
    }
}
impl Drop for LockedStore {
    fn drop(&mut self) {
        // Always release and join, including assertion/timeout unwinding.
        self.release.take();
        if let Some(helper) = self.helper.take() {
            let _ = helper.join();
        }
    }
}

#[tokio::test]
async fn drain_waits_after_work_until_the_terminal_activity_commit() {
    let dir = crate::service::application::tests::private_temp();
    let activity = Arc::new(Activity::open(dir.path()).unwrap());
    let id = activity
        .start_development("fixture-agent", "run_command")
        .unwrap();
    let (entered, observed) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let execution = Execution::probe_fixture(move |_, _| {
        let _ = entered.lock().unwrap().take().unwrap().send(());
        Err(Reason::Unavailable)
    });
    // Hold the actual store mutex, rather than substituting a fake completion.
    let held = LockedStore::hold(activity.clone(), id);
    let request = serde_json::from_value(serde_json::json!({"tool":"cargo"})).unwrap();
    let work = execution.dispatch(Some(request), activity.clone(), id);
    tokio::time::timeout(Duration::from_secs(2), observed)
        .await
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), execution.drain())
            .await
            .is_err()
    );
    drop(held);
    assert!(matches!(work.await, Err(Reason::Unavailable)));
    assert!(execution.drain().await);
    let db = activity.db.lock().unwrap();
    assert_eq!(
        db.query_row::<String, _, _>("SELECT outcome FROM activity WHERE id=?1", [id], |r| r
            .get(0))
            .unwrap(),
        "refused"
    );
    assert!(
        db.query_row::<Option<i64>, _, _>(
            "SELECT finished_at_ms FROM activity WHERE id=?1",
            [id],
            |r| r.get(0)
        )
        .unwrap()
        .is_some()
    );
}

fn rows(db: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
    let mut names = vec![
        "id",
        "principal",
        "actor_kind",
        "class",
        "tool",
        "outcome",
        "started_at_ms",
        "finished_at_ms",
    ];
    names.extend(FIELDS.iter().map(|(name, _)| *name));
    let mut statement = db
        .prepare(&format!(
            "SELECT {} FROM activity ORDER BY id",
            names.join(",")
        ))
        .unwrap();
    statement
        .query_map([], |row| (0..names.len()).map(|i| row.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn v3_rebuild_preserves_every_populated_column_and_canonical_index() {
    let dir = crate::service::application::tests::private_temp();
    let activity = Activity::open(dir.path()).unwrap();
    let host = activity.host_id().unwrap();
    drop(activity);
    let db = Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    // Exact legacy identity constraints, with all base/provenance fields populated
    // across nullable, dedicated, development and daemon histories.
    db.execute_batch("ALTER TABLE activity DROP COLUMN identity_mode; ALTER TABLE activity ADD COLUMN identity_mode TEXT CHECK(identity_mode IS NULL OR (typeof(identity_mode)='text' AND identity_mode IN ('dedicated_account','development_current_user'))) CHECK(identity_mode IS NOT 'development_current_user' OR (observed_kind IS NULL AND observed_id IS NULL AND observed_gid IS NULL AND rights IS NULL AND (elevation IS NULL OR elevation=0) AND (integrity_rid IS NULL OR integrity_rid=8192) AND (observation_point IS NULL OR observation_point='pre_exec') AND (tree_boundary IS NULL OR tree_boundary='windows_job'))); PRAGMA user_version=3;
        INSERT INTO activity(id,principal,actor_kind,class,tool,outcome,started_at_ms,finished_at_ms,identity_mode,reason,program,argc,rights,observed_kind,observed_id,observed_gid,integrity_rid,elevation,observation_point,identity_matches_pin,stop_reason,exit_code,tree_boundary,cleanup_confirmed) VALUES
        (11,'legacy-agent','agent','action','run_command','failed',101,202,NULL,'execution_failed','cargo',3,'standard','unix_uid','1234',5678,0,0,'post_switch',1,'failed',-7,'linux_cgroup',0),
        (12,'dedicated-agent','agent','action','run_command','succeeded',303,404,'dedicated_account','unavailable','rustc',2,'admin','windows_sid','S-1-5-21-101-102-103-1001',NULL,12288,1,'child',0,'exited',0,'windows_job',1),
        (13,'development-agent','agent','action','run_command','failed',505,606,'development_current_user','cleanup_unconfirmed','git',1,NULL,NULL,NULL,NULL,8192,0,'pre_exec',1,'timeout',259,'windows_job',0),
        (14,'arxd','daemon','lifecycle','service_stop','succeeded',707,808,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
        UPDATE sqlite_sequence SET seq=900 WHERE name='activity';").unwrap();
    let before = rows(&db);
    assert_eq!(before.len(), 4);
    let index: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='activity_visibility'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let activity = Activity::open(dir.path()).unwrap();
    assert_eq!(activity.host_id().unwrap(), host);
    assert_eq!(rows(&db), before);
    assert_eq!(
        db.query_row::<String, _, _>(
            "SELECT sql FROM sqlite_schema WHERE name='activity_visibility'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        index
    );
    assert_eq!(
        db.query_row::<Vec<String>, _, _>(
            "SELECT json_group_array(name) FROM pragma_index_info('activity_visibility')",
            [],
            |r| {
                let text: String = r.get(0)?;
                Ok(serde_json::from_str(&text).unwrap())
            }
        )
        .unwrap(),
        ["principal", "actor_kind", "id"]
    );
    for assignment in [
        "principal='bad principal'",
        "actor_kind='unknown'",
        "class='unknown'",
        "tool='cmd'",
        "outcome='unknown'",
        "argc=129",
        "started_at_ms=-1",
        "finished_at_ms=-1",
        "cleanup_confirmed=2",
        "observed_gid=-1",
        "identity_matches_pin=2",
    ] {
        assert!(
            db.execute(&format!("UPDATE activity SET {assignment} WHERE id=11"), [])
                .is_err(),
            "{assignment}"
        );
    }
    assert_eq!(rows(&db), before);
    assert!(activity.start("new-agent", "list_hosts").unwrap() > 900);
}

#[test]
fn empty_v1_deleted_row_sequence_survives_all_upgrades() {
    let dir = crate::service::application::tests::private_temp();
    let activity = Activity::open(dir.path()).unwrap();
    let host = activity.host_id().unwrap();
    drop(activity);
    let db = Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE activity; CREATE TABLE activity(id INTEGER PRIMARY KEY AUTOINCREMENT,principal TEXT NOT NULL,tool TEXT NOT NULL,outcome TEXT NOT NULL); INSERT INTO activity VALUES(901,'legacy-agent','run_command','succeeded'); DELETE FROM activity; PRAGMA user_version=1;").unwrap();
    let activity = Activity::open(dir.path()).unwrap();
    assert_eq!(activity.host_id().unwrap(), host);
    assert!(activity.start("new-agent", "list_hosts").unwrap() > 901);
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        5
    );
}

/// The released v4 activity DDL: identical to v5 except that its actor CHECK
/// admits only agents and the daemon.
fn released_v4_ddl() -> String {
    let v5_actor = "CHECK(actor_kind IN ('agent','daemon','owner')
        AND (actor_kind<>'daemon' OR principal='arxd')
        AND (actor_kind<>'owner' OR (principal='owner' AND tool='run_command' AND class='action')))";
    let v4_actor =
        "CHECK(actor_kind IN ('agent','daemon') AND (actor_kind<>'daemon' OR principal='arxd'))";
    let current = schema();
    assert_eq!(current.matches(v5_actor).count(), 1);
    format!(
        "{} ALTER TABLE activity ADD COLUMN identity_mode TEXT CHECK(identity_mode IS NULL OR (typeof(identity_mode)='text' AND identity_mode IN ('dedicated_account','development_current_user','current_user'))) CHECK(identity_mode NOT IN ('development_current_user','current_user') OR (observed_kind IS NULL AND observed_id IS NULL AND observed_gid IS NULL AND rights IS NULL AND (elevation IS NULL OR elevation=0) AND (integrity_rid IS NULL OR integrity_rid=8192) AND (observation_point IS NULL OR observation_point='pre_exec') AND (tree_boundary IS NULL OR tree_boundary='windows_job')));",
        current.replace(v5_actor, v4_actor)
    )
}

#[test]
fn released_v4_upgrades_to_v5_preserving_rows_sequence_and_actor_fences() {
    for empty in [false, true] {
        let dir = crate::service::application::tests::private_temp();
        let activity = Activity::open(dir.path()).unwrap();
        let host = activity.host_id().unwrap();
        drop(activity);
        let db = Connection::open(dir.path().join("activity.sqlite3")).unwrap();
        db.execute_batch(&format!(
            "DROP TABLE activity; {} PRAGMA user_version=4;",
            released_v4_ddl()
        ))
        .unwrap();
        db.execute_batch("INSERT INTO activity(id,principal,actor_kind,class,tool,outcome,started_at_ms,finished_at_ms,identity_mode,reason,program,argc,rights,observed_kind,observed_id,observed_gid,integrity_rid,elevation,observation_point,identity_matches_pin,stop_reason,exit_code,tree_boundary,cleanup_confirmed) VALUES
            (21,'legacy-agent','agent','action','run_command','failed',101,202,NULL,'execution_failed','cargo',3,'standard','unix_uid','1234',5678,0,0,'post_switch',1,'failed',-7,'linux_cgroup',0),
            (22,'dedicated-agent','agent','action','run_command','succeeded',303,404,'dedicated_account',NULL,'rustc',2,'admin','windows_sid','S-1-5-21-101-102-103-1001',NULL,12288,1,'child',0,'exited',0,'windows_job',1),
            (23,'development-agent','agent','action','run_command','failed',505,606,'development_current_user','cleanup_unconfirmed','git',1,NULL,NULL,NULL,NULL,8192,0,'pre_exec',1,'timeout',259,'windows_job',0),
            (24,'current-agent','agent','action','run_command','succeeded',707,808,'current_user',NULL,'cargo',1,NULL,NULL,NULL,NULL,8192,0,'pre_exec',1,'exited',0,'windows_job',1),
            (25,'arxd','daemon','lifecycle','service_stop','succeeded',909,1010,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL);
            UPDATE sqlite_sequence SET seq=900 WHERE name='activity';").unwrap();
        // The released v4 actor CHECK refuses owner rows.
        assert!(
            db.execute_batch("INSERT INTO activity(principal,actor_kind,class,tool,outcome) VALUES('owner','owner','action','run_command','started')")
                .is_err()
        );
        if empty {
            db.execute("DELETE FROM activity", []).unwrap();
        }
        let before = rows(&db);
        assert_eq!(before.len(), if empty { 0 } else { 5 });
        let index: String = db
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name='activity_visibility'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        let activity = Activity::open(dir.path()).unwrap();
        assert_eq!(activity.host_id().unwrap(), host);
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            5
        );
        assert_eq!(rows(&db), before);
        assert_eq!(
            db.query_row::<String, _, _>(
                "SELECT sql FROM sqlite_schema WHERE name='activity_visibility'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            index
        );
        assert_eq!(
            db.query_row::<String, _, _>(
                "SELECT json_group_array(name) FROM pragma_index_info('activity_visibility')",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            r#"["principal","actor_kind","id"]"#
        );
        assert_eq!(
            db.query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_schema WHERE name='activity_previous'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
        // The current-user identity guard survives the rebuild.
        if !empty {
            for assignment in [
                "rights='standard'",
                "elevation=1",
                "integrity_rid=12288",
                "observation_point='child'",
                "tree_boundary='process_group'",
                "observed_kind='windows_sid'",
                "observed_gid=1",
                "identity_mode='invalid'",
            ] {
                assert!(
                    db.execute(&format!("UPDATE activity SET {assignment} WHERE id=24"), [])
                        .is_err(),
                    "{assignment}"
                );
            }
        }
        // Only the fixed owner shape is admitted.
        for (principal, tool, class) in [
            ("someone", "run_command", "action"),
            ("owner", "discover_tools", "action"),
            ("owner", "run_command", "read"),
        ] {
            assert!(
                db.execute(
                    "INSERT INTO activity(principal,actor_kind,class,tool,outcome) VALUES(?1,'owner',?3,?2,'started')",
                    [principal, tool, class],
                )
                .is_err(),
                "{principal} {tool} {class}"
            );
        }
        assert_eq!(rows(&db), before);

        // The deleted-row high-water mark is kept for new owner rows.
        let owner = activity
            .start_owner_command(&Metadata {
                identity_mode: Some(IdentityMode::CurrentUser),
                program: Some(Program::Cargo),
                argc: Some(1),
                ..Default::default()
            })
            .unwrap();
        assert!(owner > 900);
        activity
            .finish_typed(
                owner,
                Outcome::Succeeded,
                &Metadata {
                    identity_mode: Some(IdentityMode::CurrentUser),
                    ..Default::default()
                },
            )
            .unwrap();
        let dashboard = activity
            .dashboard_list(
                &ListActivity {
                    limit: 50,
                    before_id: None,
                    include_reads: false,
                },
                None,
                None,
            )
            .unwrap();
        assert_eq!(dashboard[0].id, owner);
        assert_eq!(dashboard[0].actor, "owner");
        assert_eq!(dashboard[0].name, "owner");
        assert_eq!(dashboard.len(), if empty { 1 } else { 6 });
        // An MCP agent that happens to be named owner sees its own agent rows
        // but never the dashboard owner's rows.
        let agent = activity
            .start_execution("owner", "run_command", IdentityMode::CurrentUser)
            .unwrap();
        activity
            .finish_typed(
                agent,
                Outcome::Succeeded,
                &Metadata {
                    identity_mode: Some(IdentityMode::CurrentUser),
                    ..Default::default()
                },
            )
            .unwrap();
        let read = activity.start("owner", "list_activity").unwrap();
        assert!(read > agent && agent > owner);
        let page = activity
            .list(
                "owner",
                read,
                &ListActivity {
                    limit: 50,
                    before_id: None,
                    include_reads: true,
                },
            )
            .unwrap();
        assert!(page.activities.iter().all(|row| row.id != owner));
        assert!(page.activities.iter().any(|row| row.id == agent));
    }
}

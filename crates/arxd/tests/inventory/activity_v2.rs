use super::*;
use arxd::store::{Lifecycle, ListActivity, Metadata, Outcome, StopReason};
use serde_json::{Value, json};

fn args(limit: u8, before_id: Option<i64>, include_reads: bool) -> ListActivity {
    ListActivity {
        limit,
        before_id,
        include_reads,
    }
}
fn db(dir: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.join("activity.sqlite3")).unwrap()
}
fn page(store: &Activity, a: &ListActivity) -> arxd::store::ActivityPage {
    let r = store.start("test-agent", "list_activity").unwrap();
    let page = store.list("test-agent", r, a).unwrap();
    assert!(page.activities.iter().all(|v| v.id < r));
    store.finish(r, "succeeded").unwrap();
    page
}
#[test]
fn keyset_isolation_own_row_and_exact_boundaries() {
    let dir = private_dir();
    let store = Activity::open(dir.path()).unwrap();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let id = store.start("test-agent", "run_command").unwrap();
        store.finish(id, "succeeded").unwrap();
        ids.push(id);
    }
    store.start("second-agent", "run_command").unwrap();
    store.start_lifecycle(Lifecycle::Start).unwrap();
    store.start("test-agent", "list_hosts").unwrap();
    let one = page(&store, &args(2, None, false));
    assert_eq!(
        one.activities.iter().map(|r| r.id).collect::<Vec<_>>(),
        [ids[2], ids[1]]
    );
    assert_eq!(one.next_before_id, Some(ids[1]));
    let two = page(&store, &args(2, one.next_before_id, false));
    assert_eq!(
        two.activities.iter().map(|r| r.id).collect::<Vec<_>>(),
        [ids[0]]
    );
    assert_eq!(two.next_before_id, None);
    assert_eq!(page(&store, &args(3, None, false)).next_before_id, None);
    assert!(page(&store, &args(1, Some(1), true)).activities.is_empty());
    assert!(
        page(&store, &args(50, None, true))
            .activities
            .iter()
            .all(|r| !matches!(r.tool, arxd::store::Tool::ServiceStart))
    );
    let r = store.start("second-agent", "list_activity").unwrap();
    assert!(store.list("test-agent", r, &args(20, None, true)).is_err());
}
#[test]
fn read_flood_preserves_actions_and_lifecycle() {
    let dir = private_dir();
    let store = Activity::open(dir.path()).unwrap();
    for _ in 0..1001 {
        store.start("test-agent", "run_command").unwrap();
    }
    let lifecycle = store.start_lifecycle(Lifecycle::Stop).unwrap();
    for _ in 0..205 {
        store.start("test-agent", "list_hosts").unwrap();
    }
    let db = db(dir.path());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM activity WHERE class='read'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        200
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM activity WHERE class<>'read'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1000
    );
    assert_eq!(
        db.query_row(
            "SELECT actor_kind||':'||principal FROM activity WHERE id=?1",
            [lifecycle],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "daemon:arxd"
    );
    for _ in 0..1001 {
        store.start("test-agent", "discover_tools").unwrap();
    }
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM activity WHERE class='read'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        200
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM activity WHERE class<>'read'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1000
    );
}
#[test]
fn exactly_once_completion_rolls_back_duplicate_missing_and_repeated() {
    let dir = private_dir();
    let store = Activity::open(dir.path()).unwrap();
    assert!(store.finish(999, "succeeded").is_err());
    let id = store.start("test-agent", "run_command").unwrap();
    let metadata = Metadata {
        stop_reason: Some(StopReason::Shutdown),
        ..Default::default()
    };
    store.finish_typed(id, Outcome::Failed, &metadata).unwrap();
    assert!(store.finish(id, "succeeded").is_err());
    assert!(store.finish(id, "shutdown").is_err());
    let db = db(dir.path());
    assert_eq!(
        db.query_row(
            "SELECT outcome||':'||stop_reason FROM activity WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "failed:shutdown"
    );
    for stop_reason in [
        StopReason::Cancelled,
        StopReason::Timeout,
        StopReason::Shutdown,
    ] {
        let stopped = store.start("test-agent", "run_command").unwrap();
        store
            .finish_typed(
                stopped,
                Outcome::Failed,
                &Metadata {
                    stop_reason: Some(stop_reason),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            db.query_row(
                "SELECT stop_reason FROM activity WHERE id=?1",
                [stopped],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            stop_reason.as_str()
        );
    }
    db.execute("DELETE FROM activity WHERE id<>?1", [id])
        .unwrap();
    // Corrupt schema reproduces PR18's multiple-row update: both must roll back.
    db.execute_batch("ALTER TABLE activity RENAME TO saved; CREATE TABLE activity AS SELECT * FROM saved; INSERT INTO activity SELECT * FROM saved; UPDATE activity SET outcome='started';").unwrap();
    assert!(store.finish(id, "succeeded").is_err());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM activity WHERE outcome='started'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}
#[test]
fn v1_migration_is_lossless_and_failure_is_atomic() {
    for corrupt in [false, true] {
        let dir = private_dir();
        let store = Activity::open(dir.path()).unwrap();
        let host = store.host_id().unwrap();
        drop(store);
        let db = db(dir.path());
        db.execute_batch("DROP TABLE activity; CREATE TABLE activity(id INTEGER PRIMARY KEY AUTOINCREMENT,principal TEXT NOT NULL,tool TEXT NOT NULL,outcome TEXT NOT NULL); INSERT INTO activity VALUES(41,'test-agent','list_hosts','succeeded'),(42,'test-agent','unknown_tool','started'); PRAGMA user_version=1;").unwrap();
        if corrupt {
            db.execute("UPDATE activity SET tool='SECRET-MARKER' WHERE id=42", [])
                .unwrap();
        }
        let before = std::fs::read(dir.path().join("activity.sqlite3")).unwrap();
        if corrupt {
            assert!(Activity::open(dir.path()).is_err());
            assert_eq!(
                db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                std::fs::read(dir.path().join("activity.sqlite3")).unwrap(),
                before
            );
            assert_eq!(
                db.query_row("SELECT outcome FROM activity WHERE id=42", [], |r| r
                    .get::<_, String>(0))
                    .unwrap(),
                "started"
            );
        } else {
            let store = Activity::open(dir.path()).unwrap();
            assert_eq!(store.host_id().unwrap(), host);
            assert_eq!(
                db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                    .unwrap(),
                3
            );
            let p = page(&store, &args(50, None, true));
            assert_eq!(p.activities.len(), 2);
            assert_eq!(p.activities[0].id, 42);
            assert_eq!(p.activities[0].outcome, Outcome::Interrupted);
            assert!(p.activities.iter().all(|r| r.started_at_ms.is_none()
                && r.finished_at_ms.is_none()
                && r.metadata.observed_kind.is_none()));
        }
    }
}
#[test]
fn corrupt_values_are_bounded_rejected_and_not_echoed() {
    for mutation in [
        "tool='SECRET-MARKER'",
        "reason=printf('%0100000d',0)",
        "observed_id='S-1--5',observed_kind='windows_sid'",
        "started_at_ms=-1",
        "argc=129",
        "cleanup_confirmed=3",
        "outcome=zeroblob(100000)",
    ] {
        let dir = private_dir();
        let store = Activity::open(dir.path()).unwrap();
        let id = store.start("test-agent", "run_command").unwrap();
        let r = store.start("test-agent", "list_activity").unwrap();
        db(dir.path())
            .execute_batch(&format!(
                "PRAGMA ignore_check_constraints=ON; UPDATE activity SET {mutation} WHERE id={id};"
            ))
            .unwrap();
        let error = store
            .list("test-agent", r, &args(50, None, true))
            .unwrap_err()
            .to_string();
        assert!(!error.contains("SECRET-MARKER"));
    }
}
fn call(value: Value) -> CallToolRequestParams {
    CallToolRequestParams::new("list_activity").with_arguments(value.as_object().unwrap().clone())
}
fn content(response: rmcp::model::CallToolResult) -> Value {
    serde_json::from_str(&response.content[0].as_text().unwrap().text).unwrap()
}
#[tokio::test]
async fn sdk_http_and_real_stdio_activity_visibility_storage_failures_and_bounds() {
    for local in [false, true] {
        let dir = private_dir();
        let (mut cfg, token) = config(dir.path());
        let store = Arc::new(Activity::open(dir.path()).unwrap());
        for _ in 0..51 {
            let id = store.start("test-agent", "discover_tools").unwrap();
            let m = Metadata {
                identity_mode: Some(arxd::store::IdentityMode::DedicatedAccount),
                argc: Some(128),
                observed_kind: Some(arxd::store::ObservedKind::WindowsSid),
                observed_id: Some(format!(
                    "S-1-281474976710655-{}",
                    ["4294967295"; 15].join("-")
                )),
                rights: Some(arxd::store::Rights::Standard),
                reason: Some(arxd::store::Reason::CleanupUnconfirmed),
                program: Some(arxd::store::Program::QemuSystemX8664),
                observed_gid: Some(u32::MAX),
                integrity_rid: Some(u32::MAX),
                elevation: Some(true),
                identity_matches_pin: Some(false),
                observation_point: Some(arxd::store::ObservationPoint::PostSwitch),
                exit_code: Some(i32::MIN),
                tree_boundary: Some(arxd::store::TreeBoundary::MacUidSweep),
                cleanup_confirmed: Some(false),
                stop_reason: Some(StopReason::OutputLimit),
            };
            store.finish_typed(id, Outcome::Failed, &m).unwrap();
        }
        store.start("other-agent", "run_command").unwrap();
        store.start_lifecycle(Lifecycle::Start).unwrap();
        db(dir.path()).execute_batch("UPDATE activity SET id=id+9223372036854775600,started_at_ms=253402300799999,finished_at_ms=253402300799999").unwrap();

        let (peer, task) = if local {
            let endpoint = fixture_endpoint(dir.path());
            cfg.local = Some(endpoint.clone());
            let file = client_file(
                dir.path(),
                &endpoint,
                &token,
                &endpoint.client_identity,
                "activity-client.json",
            );
            let listener = arxd::local::Listener::bind(&endpoint).unwrap();
            let local_store = store.clone();
            let task = tokio::spawn(async move {
                listener.run(Arc::new(cfg), local_store).await.unwrap();
            });
            let peer = stdio_peer(&file).await;
            (peer, task)
        } else {
            let cfg = Arc::new(cfg);
            let listener = arxd::http::bind(cfg.clone()).await.unwrap();
            let addr = listener.local_addr().unwrap();
            let router = arxd::http::router(cfg, addr, store.clone());
            let task = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            (client(&format!("http://{addr}/mcp"), &token).await, task)
        };
        let response = peer.call_tool(call(json!({"limit":50}))).await.unwrap();
        let mut wire =
            serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"x".repeat(8192),"result":response}))
                .unwrap();
        wire.push(b'\n');
        assert!(wire.len() > 8192 && wire.len() <= arxd::local::RESPONSE_LIMIT);
        let p = content(response);
        assert_eq!(p["activities"].as_array().unwrap().len(), 50);
        assert!(p.to_string().find("principal").is_none());
        let q = content(
            peer.call_tool(call(json!({"limit":50,"before_id":p["next_before_id"]})))
                .await
                .unwrap(),
        );
        assert_eq!(q["activities"].as_array().unwrap().len(), 1);
        assert!(q["next_before_id"].is_null());
        assert_eq!(
            content(peer.call_tool(call(json!({}))).await.unwrap())["activities"]
                .as_array()
                .unwrap()
                .len(),
            20
        );
        for bad in [
            json!({"limit":0}),
            json!({"limit":51}),
            json!({"before_id":0}),
            json!({"before_id":-1}),
            json!({"limit":1.5}),
            json!({"include_reads":"true"}),
            json!({"before_id":null}),
            json!({"principal":"SECRET-MARKER"}),
        ] {
            let before = db(dir.path())
                .query_row("SELECT max(id) FROM activity", [], |r| r.get::<_, i64>(0))
                .unwrap();
            assert!(peer.call_tool(call(bad)).await.is_err());
            let rejected: (i64, String, String) = db(dir.path())
                .query_row(
                    "SELECT id,tool,outcome FROM activity ORDER BY id DESC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert!(rejected.0 > before);
            assert_eq!(rejected.1, "list_activity");
            assert_eq!(rejected.2, "rejected");
        }
        assert!(
            peer.call_tool(CallToolRequestParams::new("SECRET-MARKER"))
                .await
                .is_err()
        );
        let db = db(dir.path());
        assert_eq!(
            db.query_row(
                "SELECT tool FROM activity ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "unknown_tool"
        );
        let read = content(
            peer.call_tool(call(json!({"include_reads":true,"limit":1})))
                .await
                .unwrap(),
        );
        assert_eq!(read["activities"][0]["tool"], "unknown_tool");
        db.execute_batch("CREATE TRIGGER fail_start BEFORE INSERT ON activity BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
        assert!(
            peer.call_tool(call(json!({})))
                .await
                .unwrap_err()
                .to_string()
                .contains("activity unavailable")
        );
        db.execute_batch("DROP TRIGGER fail_start; CREATE TRIGGER fail_finish BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
        let error = peer
            .call_tool(call(json!({})))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("activity unavailable")
                && !error.contains("activities")
                && !error.contains("SECRET-MARKER")
        );
        assert_eq!(
            db.query_row(
                "SELECT outcome FROM activity ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "started"
        );
        peer.cancel().await.unwrap();
        task.abort();
        let _ = task.await;
        assert!(
            !std::fs::read(dir.path().join("activity.sqlite3"))
                .unwrap()
                .windows(13)
                .any(|w| w == b"SECRET-MARKER")
        );
    }
}

#[test]
fn extra_or_missing_columns_refuse_without_exposing_values() {
    for sql in [
        "ALTER TABLE activity ADD COLUMN SECRET_MARKER TEXT",
        "ALTER TABLE activity DROP COLUMN reason",
    ] {
        let dir = private_dir();
        let store = Activity::open(dir.path()).unwrap();
        let read = store.start("test-agent", "list_activity").unwrap();
        db(dir.path()).execute_batch(sql).unwrap();
        assert!(
            store
                .list("test-agent", read, &args(20, None, true))
                .is_err()
        );
        drop(store);
        assert!(Activity::open(dir.path()).is_err());
    }
}

#[tokio::test]
async fn list_activity_token_revocation_survives_restart_on_both_transports() {
    let dir = private_dir();
    let (mut cfg, token) = config(dir.path());
    let endpoint = fixture_endpoint(dir.path());
    cfg.local = Some(endpoint.clone());
    let old = client_file(
        dir.path(),
        &endpoint,
        &token,
        &endpoint.client_identity,
        "revoked.json",
    );
    let mut cfg = Arc::new(cfg);
    for restart in [false, true] {
        let credential = if restart {
            let (mut replacement, new_token) = config(dir.path());
            replacement.local = Some(endpoint.clone());
            cfg = Arc::new(replacement);
            new_token
        } else {
            token.clone()
        };
        let store = Arc::new(Activity::open(dir.path()).unwrap());
        let listener = arxd::local::Listener::bind(&endpoint).unwrap();
        let local_store = store.clone();
        let local_cfg = cfg.clone();
        let local_task = tokio::spawn(async move {
            listener.run(local_cfg, local_store).await.unwrap();
        });
        let http_cfg = cfg.clone();
        let listener = arxd::http::bind(http_cfg.clone()).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/mcp");
        let router = arxd::http::router(http_cfg, addr, store.clone());
        let http_task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        if restart {
            let count = db(dir.path())
                .query_row("SELECT count(*) FROM activity", [], |r| r.get::<_, i64>(0))
                .unwrap();
            assert_eq!(reqwest::Client::new().post(&url).bearer_auth(&token).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_activity","arguments":{}}})).send().await.unwrap().status(),401);
            let rejected = tokio::process::Command::new(env!("CARGO_BIN_EXE_arxd"))
                .args(["mcp-stdio", "--client-config"])
                .arg(&old)
                .stdin(std::process::Stdio::null())
                .output();
            let output = tokio::time::timeout(std::time::Duration::from_secs(7), rejected)
                .await
                .unwrap()
                .unwrap();
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert_eq!(
                db(dir.path())
                    .query_row("SELECT count(*) FROM activity", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                count
            );
        }
        let file = client_file(
            dir.path(),
            &endpoint,
            &credential,
            &endpoint.client_identity,
            if restart { "new.json" } else { "first.json" },
        );
        let http = client(&url, &credential).await;
        let local = stdio_peer(&file).await;
        for peer in [&http, &local] {
            let page = content(
                peer.call_tool(call(json!({"include_reads":true})))
                    .await
                    .unwrap(),
            );
            if restart {
                assert!(page["activities"].as_array().unwrap().len() >= 2);
            }
        }
        http.cancel().await.unwrap();
        local.cancel().await.unwrap();
        http_task.abort();
        local_task.abort();
        let _ = http_task.await;
        let _ = local_task.await;
        drop(store);
    }
}

#[test]
fn schema_checks_and_typed_metadata_reject_unsafe_values() {
    let dir = private_dir();
    let store = Activity::open(dir.path()).unwrap();
    let id = store.start("test-agent", "run_command").unwrap();
    let db = db(dir.path());
    for assignment in [
        "reason='SECRET-MARKER'",
        "program='/private/tool'",
        "tool='arbitrary'",
        "outcome='shutdown'",
        "started_at_ms=-1",
        "finished_at_ms=253402300800000",
        "argc=129",
        "observed_id='S-1--5',observed_kind='windows_sid'",
        "observed_id='100',observed_kind=NULL",
        "observed_id='x',observed_kind='unix_uid'",
        "rights='root'",
        "elevation=2",
        "stop_reason='SECRET-MARKER'",
        "class='other'",
        "actor_kind='daemon'",
    ] {
        assert!(
            db.execute_batch(&format!("UPDATE activity SET {assignment} WHERE id={id}"))
                .is_err(),
            "accepted {assignment}"
        );
    }
    let m = Metadata {
        observed_kind: Some(arxd::store::ObservedKind::WindowsSid),
        observed_id: Some("S-1-5-4294967296".into()),
        ..Default::default()
    };
    assert!(store.finish_typed(id, Outcome::Failed, &m).is_err());
    assert_eq!(
        db.query_row("SELECT outcome FROM activity WHERE id=?1", [id], |r| r
            .get::<_, String>(
            0
        ))
        .unwrap(),
        "started"
    );
    let r = store.start("test-agent", "list_activity").unwrap();
    let p = store
        .list("test-agent", r, &args(50, Some(i64::MAX), true))
        .unwrap();
    assert!(p.activities.iter().all(|row| row.id < r));
}

#[test]
fn v3_development_constraints_and_v2_migration_preserve_identity_sequence() {
    let dir = private_dir();
    let store = Activity::open(dir.path()).unwrap();
    let host = store.host_id().unwrap();
    let id = store.start("test-agent", "run_command").unwrap();
    let m = Metadata {
        identity_mode: Some(arxd::store::IdentityMode::DevelopmentCurrentUser),
        ..Default::default()
    };
    store.finish_typed(id, Outcome::Succeeded, &m).unwrap();
    let db = db(dir.path());
    for assignment in [
        "observed_kind='windows_sid'",
        "observed_id='S-1-5-21-1',observed_kind='windows_sid'",
        "rights='standard'",
        "observed_gid=1",
        "identity_mode='invalid'",
        "elevation=1",
        "integrity_rid=16384",
        "observation_point='child'",
        "tree_boundary='process_group'",
    ] {
        assert!(
            db.execute_batch(&format!("UPDATE activity SET {assignment} WHERE id={id}"))
                .is_err()
        );
    }
    // Positive control: the fixed development facts are accepted.
    db.execute_batch(&format!("UPDATE activity SET elevation=0,integrity_rid=8192,observation_point='pre_exec',tree_boundary='windows_job' WHERE id={id}")).unwrap();
    // The start-time mode is authoritative: completion cannot relabel a row.
    let dev = store
        .start_development("test-agent", "run_command")
        .unwrap();
    let dedicated = Metadata {
        identity_mode: Some(arxd::store::IdentityMode::DedicatedAccount),
        ..Default::default()
    };
    assert!(
        store
            .finish_typed(dev, Outcome::Succeeded, &dedicated)
            .is_err()
    );
    assert!(
        store
            .finish_typed(dev, Outcome::Succeeded, &Metadata::default())
            .is_ok()
    );
    let mode: String = db
        .query_row(
            &format!("SELECT identity_mode FROM activity WHERE id={dev}"),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mode, "development_current_user");
    let p = page(&store, &args(50, None, false));
    assert_eq!(
        p.activities[1].claim.as_deref(),
        Some(arxd::development::CLAIM)
    );
    assert_eq!(
        p.activities[0].execution_mode,
        Some(arxd::store::IdentityMode::DevelopmentCurrentUser)
    );
    drop(store);
    db.execute_batch("ALTER TABLE activity DROP COLUMN identity_mode; PRAGMA user_version=2; UPDATE sqlite_sequence SET seq=900 WHERE name='activity';").unwrap();
    let store = Activity::open(dir.path()).unwrap();
    assert_eq!(store.host_id().unwrap(), host);
    assert!(store.start("test-agent", "run_command").unwrap() > 900);
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        3
    );
    assert!(
        page(&store, &args(50, None, false))
            .activities
            .iter()
            .all(|r| r.metadata.identity_mode.is_none())
    );
}

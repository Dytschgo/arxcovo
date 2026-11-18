use super::*;
use crate::store::{Activity, ListActivity};
use rmcp::{ServiceExt, model::*};
use serde_json::{Value, json};

fn normal_settings(root: &Path) -> Value {
    // macOS's trusted temporary root is commonly exposed through /var.
    let root = root.canonicalize().unwrap();
    for name in ["workspace", "state", "tools", "config"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    json!({
        "workspace": root.join("workspace"),
        "environment": {"PATH": root.join("tools"), "CARGO_TARGET_DIR": root.join("target")},
        "limits": {"memory_bytes":16777216,"pids":8}
    })
}

#[test]
fn windows_account_choice_defaults_and_strict_selection() {
    let dir = tempfile::tempdir().unwrap();
    let base = normal_settings(dir.path());
    for mode in [None, Some("current_user"), Some("dedicated_account")] {
        let mut value = base.clone();
        if let Some(mode) = mode {
            value["mode"] = json!(mode);
        }
        if mode == Some("dedicated_account") {
            value["account"] = json!("arx-worker");
        }
        let settings: WindowsSettings = serde_json::from_value(value).unwrap();
        settings.validate().unwrap();
        assert_eq!(
            settings.mode == WindowsMode::CurrentUser,
            mode != Some("dedicated_account")
        );
    }
    for addition in [
        json!({"mode":"system"}),
        json!({"mode":"development_current_user"}),
        json!({"mode":null}),
        json!({"acknowledge":"shares_owner_user_files_and_credentials"}),
        json!({"password":"never-collected"}),
        json!({"account":"worker"}),
        json!({"mode":"dedicated_account"}),
        json!({"mode":"dedicated_account","account":""}),
        json!({"mode":"dedicated_account","account":"DOMAIN\\worker"}),
        json!({"mode":"dedicated_account","account":"worker@example.com"}),
        json!({"mode":"dedicated_account","account":"x".repeat(21)}),
        json!({"mode":"dedicated_account","account":"LocalSystem"}),
    ] {
        let mut value = base.clone();
        value
            .as_object_mut()
            .unwrap()
            .extend(addition.as_object().unwrap().clone());
        assert!(
            serde_json::from_value::<WindowsSettings>(value)
                .map(|s| s.validate().is_err())
                .unwrap_or(true),
            "{addition}"
        );
    }
}

#[test]
fn normal_execution_requires_explicit_configuration_and_local_disjoint_paths() {
    let dir = tempfile::tempdir().unwrap();
    let execution = normal_settings(dir.path());
    let mut base = json!({"state_dir":dir.path().join("state"),"agents":[],
        "local":{"address":"fixture","client_identity":"fixture"}});
    let parse = |v: Value| serde_json::from_value::<crate::config::Config>(v);
    let absent = parse(base.clone()).unwrap();
    assert!(absent.execution_settings.is_none() && absent.execution.is_none());
    absent.validate_development().unwrap();
    base["execution"] = execution.clone();
    assert_eq!(
        parse(base.clone()).unwrap().validate_development().is_ok(),
        cfg!(windows)
    );
    for addition in [
        json!({"listen":"127.0.0.1:8080"}),
        json!({"local":null}),
        json!({"state_dir":dir.path().join("workspace")}),
        json!({"state_dir":dir.path()}),
        json!({"state_dir":dir.path().join("workspace").join("state")}),
    ] {
        let mut value = base.clone();
        value
            .as_object_mut()
            .unwrap()
            .extend(addition.as_object().unwrap().clone());
        assert!(
            parse(value).unwrap().validate_development().is_err(),
            "{addition}"
        );
    }
    let mut legacy = execution.clone();
    legacy["mode"] = json!(MODE);
    legacy["acknowledge"] = json!("shares_owner_user_files_and_credentials");
    base["development_execution"] = legacy;
    assert!(parse(base).unwrap().validate_development().is_err());
    let settings: WindowsSettings = serde_json::from_value(execution).unwrap();
    assert!(settings.disjoint(&dir.path().join("workspace")).is_err());
    assert!(settings.disjoint(dir.path()).is_err());
    assert!(
        settings
            .disjoint(&dir.path().canonicalize().unwrap().join("config"))
            .is_ok()
    );
    assert!(
        settings
            .disjoint(&settings.workspace.canonicalize().unwrap())
            .is_err(),
        "verbatim and ordinary disk spellings must refer to the same workspace"
    );
    let repeated = settings.workspace.to_string_lossy().replace('\\', "\\\\");
    #[cfg(windows)]
    assert!(settings.disjoint(Path::new(&repeated)).is_err());
    #[cfg(not(windows))]
    let _ = repeated;
}

fn output() -> ResultData {
    ResultData {
        execution_mode: MODE,
        claim: CLAIM,
        dedicated: false,
        stop_reason: StopReason::Exited,
        exit_code: Some(0),
        cleanup_confirmed: true,
        matches_current_user: true,
        stdout: "fixture".into(),
        stderr: String::new(),
        stdout_dropped_bytes: 0,
        stderr_dropped_bytes: 0,
    }
}
fn content(response: CallToolResult) -> Value {
    serde_json::from_str(&response.content[0].as_text().unwrap().text).unwrap()
}

#[tokio::test]
async fn normal_current_user_dispatch_and_activity_keep_distinct_legacy_provenance() {
    let dir = crate::service::application::tests::private_temp();
    let activity = Arc::new(Activity::open(dir.path()).unwrap());
    let legacy = activity
        .start_development("fixture-agent", "run_command")
        .unwrap();
    activity
        .finish_typed(legacy, Outcome::Succeeded, &metadata(None, None, None))
        .unwrap();
    let mut executor = Execution::probe_fixture(|_, _| Ok(output()));
    Arc::get_mut(&mut executor).unwrap().identity_mode = IdentityMode::CurrentUser;
    assert_eq!(executor.capability()["mode"], "current_user");
    assert_eq!(executor.capability()["claim"], CURRENT_USER_CLAIM);
    assert_eq!(executor.capability()["dedicated"], false);
    assert_eq!(executor.capability()["isolation"], false);
    assert!(executor.capability()["rights"].is_null());
    let inventory = crate::mcp::Inventory {
        execution: Some(executor),
        activity: activity.clone(),
        principal: Some(crate::mcp::Principal("fixture-agent".into())),
    };
    let (s, c) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
    let client = ().serve(c).await.unwrap();
    let result = content(
        client
            .call_tool(
                CallToolRequestParams::new("run_command")
                    .with_arguments(json!({"tool":"cargo"}).as_object().unwrap().clone()),
            )
            .await
            .unwrap(),
    );
    assert_eq!(result["execution_mode"], "current_user");
    assert_eq!(result["claim"], CURRENT_USER_CLAIM);
    assert_eq!(result["dedicated"], false);
    let discovery = content(
        client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
            .unwrap(),
    );
    assert_eq!(discovery["execution_mode"], "current_user");
    assert_eq!(discovery["outcome"], "succeeded");
    assert!(
        client
            .call_tool(
                CallToolRequestParams::new("run_command")
                    .with_arguments(json!({"tool":"cmd"}).as_object().unwrap().clone())
            )
            .await
            .is_err()
    );
    let read = activity.start("fixture-agent", "list_activity").unwrap();
    let page = activity
        .list(
            "fixture-agent",
            read,
            &ListActivity {
                limit: 50,
                before_id: None,
                include_reads: true,
            },
        )
        .unwrap();
    let old = page.activities.iter().find(|r| r.id == legacy).unwrap();
    assert_eq!(
        old.execution_mode,
        Some(IdentityMode::DevelopmentCurrentUser)
    );
    assert_eq!(old.claim.as_deref(), Some(CLAIM));
    let normal = page
        .activities
        .iter()
        .filter(|r| r.id != legacy && r.metadata.identity_mode.is_some())
        .collect::<Vec<_>>();
    assert_eq!(normal.len(), 3);
    for row in normal {
        assert_eq!(row.execution_mode, Some(IdentityMode::CurrentUser));
        assert_eq!(row.claim.as_deref(), Some(CURRENT_USER_CLAIM));
        assert!(row.metadata.observed_id.is_none() && row.metadata.rights.is_none());
    }
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn dedicated_selection_is_setup_required_and_never_invokes_current_user_backend() {
    let dir = tempfile::tempdir().unwrap();
    let mut value = normal_settings(dir.path());
    value["mode"] = json!("dedicated_account");
    value["account"] = json!("arx-worker");
    let mut executor = Execution::from_windows(serde_json::from_value(value).unwrap()).unwrap();
    Arc::get_mut(&mut executor).unwrap().probe =
        Some(Arc::new(|_, _| panic!("dedicated must never fall back")));
    let capability = executor.capability();
    assert_eq!(capability["available"], false);
    assert_eq!(capability["mode"], "dedicated_account");
    assert_eq!(capability["reason"], "dedicated_account_setup_required");
    assert_eq!(capability["dedicated"], false);
    assert_eq!(capability["isolation"], false);
    assert!(capability["rights"].is_null() && capability["matches_current_user"].is_null());
    let state = crate::service::application::tests::private_temp();
    let activity = Arc::new(Activity::open(state.path()).unwrap());
    let inventory = crate::mcp::Inventory {
        execution: Some(executor),
        activity: activity.clone(),
        principal: Some(crate::mcp::Principal("fixture-agent".into())),
    };
    let (s, c) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
    let client = ().serve(c).await.unwrap();
    for request in [
        CallToolRequestParams::new("run_command")
            .with_arguments(json!({"tool":"cargo"}).as_object().unwrap().clone()),
        CallToolRequestParams::new("discover_tools"),
    ] {
        let error = client.call_tool(request).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("dedicated_account_setup_required")
        );
    }
    let read = activity.start("fixture-agent", "list_activity").unwrap();
    let page = activity
        .list(
            "fixture-agent",
            read,
            &ListActivity {
                limit: 50,
                before_id: None,
                include_reads: true,
            },
        )
        .unwrap();
    assert_eq!(page.activities.len(), 2);
    for row in page.activities {
        assert_eq!(row.execution_mode, Some(IdentityMode::DedicatedAccount));
        assert_eq!(row.outcome, Outcome::Refused);
        assert!(row.metadata.observed_id.is_none() && row.metadata.rights.is_none());
    }
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn drain_budget_reports_unfinished_worker_without_claiming_cleanup() {
    let execution = Execution::probe_fixture(|_, _| panic!("no worker"));
    assert!(execution.drain().await);
    execution.busy.store(true, Ordering::Release);
    assert!(!execution.drain().await);
    execution.busy.store(false, Ordering::Release);
    assert!(execution.drain().await);
}

#[test]
fn activity_v3_migration_preserves_legacy_modes_and_sequence_and_guards_current_user() {
    for empty in [false, true] {
        let dir = crate::service::application::tests::private_temp();
        let activity = Activity::open(dir.path()).unwrap();
        let host = activity.host_id().unwrap();
        let ids = [
            activity.start("fixture-agent", "run_command").unwrap(),
            activity
                .start_development("fixture-agent", "run_command")
                .unwrap(),
            activity
                .start_execution(
                    "fixture-agent",
                    "run_command",
                    IdentityMode::DedicatedAccount,
                )
                .unwrap(),
        ];
        for id in ids {
            activity.finish(id, "succeeded").unwrap();
        }
        drop(activity);
        let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
        // Recreate the released v3 column constraints, keeping the v2 schema.
        db.execute_batch("ALTER TABLE activity DROP COLUMN identity_mode; ALTER TABLE activity ADD COLUMN identity_mode TEXT CHECK(identity_mode IS NULL OR (typeof(identity_mode)='text' AND identity_mode IN ('dedicated_account','development_current_user'))) CHECK(identity_mode IS NOT 'development_current_user' OR (observed_kind IS NULL AND observed_id IS NULL AND observed_gid IS NULL AND rights IS NULL AND (elevation IS NULL OR elevation=0) AND (integrity_rid IS NULL OR integrity_rid=8192) AND (observation_point IS NULL OR observation_point='pre_exec') AND (tree_boundary IS NULL OR tree_boundary='windows_job'))); PRAGMA user_version=3; UPDATE sqlite_sequence SET seq=900 WHERE name='activity';").unwrap();
        db.execute(
            "UPDATE activity SET identity_mode='development_current_user' WHERE id=?1",
            [ids[1]],
        )
        .unwrap();
        db.execute(
            "UPDATE activity SET identity_mode='dedicated_account' WHERE id=?1",
            [ids[2]],
        )
        .unwrap();
        if empty {
            db.execute("DELETE FROM activity", []).unwrap();
        }
        let activity = Activity::open(dir.path()).unwrap();
        assert_eq!(activity.host_id().unwrap(), host);
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            4
        );
        let current = activity
            .start_execution("fixture-agent", "run_command", IdentityMode::CurrentUser)
            .unwrap();
        assert!(current > 900);
        activity
            .finish_typed(
                current,
                Outcome::Succeeded,
                &Metadata {
                    identity_mode: Some(IdentityMode::CurrentUser),
                    ..Default::default()
                },
            )
            .unwrap();
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
                db.execute(
                    &format!("UPDATE activity SET {assignment} WHERE id=?1"),
                    [current]
                )
                .is_err(),
                "{assignment}"
            );
        }
        let read = activity.start("fixture-agent", "list_activity").unwrap();
        let page = activity
            .list(
                "fixture-agent",
                read,
                &ListActivity {
                    limit: 50,
                    before_id: None,
                    include_reads: true,
                },
            )
            .unwrap();
        if !empty {
            for (id, mode) in ids.into_iter().zip([
                None,
                Some(IdentityMode::DevelopmentCurrentUser),
                Some(IdentityMode::DedicatedAccount),
            ]) {
                assert_eq!(
                    page.activities
                        .iter()
                        .find(|r| r.id == id)
                        .unwrap()
                        .execution_mode,
                    mode
                );
            }
        }
        assert_eq!(
            page.activities[0].execution_mode,
            Some(IdentityMode::CurrentUser)
        );
        assert_eq!(
            page.activities[0].claim.as_deref(),
            Some(CURRENT_USER_CLAIM)
        );
    }
}

#[cfg(windows)]
#[test]
fn loaded_configuration_refuses_workspace_overlap_before_constructing_executor() {
    let dir = crate::service::application::tests::private_temp();
    let settings = normal_settings(dir.path());
    let mut config = json!({"state_dir":dir.path().join("state"),
        "agents":[{"principal":"fixture-agent","token_sha256":"0".repeat(64)}],
        "local":{"address":format!(r"\\.\pipe\arxcovo-{}",rand::random::<u64>()),"client_identity":crate::local::current_identity().unwrap()},
        "execution":settings});
    let nested = dir.path().join("workspace").join("config");
    std::fs::create_dir(&nested).unwrap();
    for mode in ["current_user", "dedicated_account"] {
        config["execution"]["mode"] = json!(mode);
        if mode == "dedicated_account" {
            config["execution"]["account"] = json!("arx-worker");
        }
        let file = nested.join("server.json");
        std::fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
        let error = crate::config::Config::load(&file)
            .err()
            .expect("overlap must refuse");
        assert_eq!(error.to_string(), "workspace overlaps private state");
    }
    // The actual foreground loader preserves an unavailable dedicated choice.
    let file = dir.path().join("config").join("server.json");
    std::fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    let loaded = crate::config::Config::load(&file).unwrap();
    assert_eq!(
        loaded.execution.unwrap().capability()["reason"],
        "dedicated_account_setup_required"
    );
}

#[test]
fn failed_v3_migration_rolls_back_schema_and_rows() {
    let dir = crate::service::application::tests::private_temp();
    let activity = Activity::open(dir.path()).unwrap();
    let id = activity
        .start_development("fixture-agent", "run_command")
        .unwrap();
    activity.finish(id, "succeeded").unwrap();
    drop(activity);
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    // A damaged v3 row must not be repaired or silently relabeled by migration.
    db.execute_batch("PRAGMA user_version=3; PRAGMA ignore_check_constraints=ON; UPDATE activity SET rights='standard'; PRAGMA ignore_check_constraints=OFF;").unwrap();
    assert!(Activity::open(dir.path()).is_err());
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row::<String, _, _>(
            "SELECT identity_mode FROM activity WHERE id=?1",
            [id],
            |r| r.get(0)
        )
        .unwrap(),
        MODE
    );
    assert_eq!(
        db.query_row::<String, _, _>("SELECT rights FROM activity WHERE id=?1", [id], |r| r
            .get(0))
            .unwrap(),
        "standard"
    );
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM sqlite_schema WHERE name='activity_v3'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn legacy_v1_v2_activity_upgrade_preserves_host_sequence_and_unknown_identity() {
    for version in [1, 2] {
        let dir = crate::service::application::tests::private_temp();
        let activity = Activity::open(dir.path()).unwrap();
        let host = activity.host_id().unwrap();
        let id = activity.start("fixture-agent", "run_command").unwrap();
        drop(activity);
        let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
        if version == 1 {
            db.execute_batch("DROP TABLE activity; CREATE TABLE activity(id INTEGER PRIMARY KEY AUTOINCREMENT,principal TEXT NOT NULL,tool TEXT NOT NULL,outcome TEXT NOT NULL); INSERT INTO activity VALUES(41,'fixture-agent','run_command','succeeded'),(42,'fixture-agent','unknown_tool','started'); PRAGMA user_version=1;").unwrap();
        } else {
            db.execute_batch(
                "ALTER TABLE activity DROP COLUMN identity_mode; PRAGMA user_version=2;",
            )
            .unwrap();
        }
        db.execute_batch("UPDATE sqlite_sequence SET seq=900 WHERE name='activity';")
            .unwrap();
        let activity = Activity::open(dir.path()).unwrap();
        assert_eq!(activity.host_id().unwrap(), host);
        let read = activity.start("fixture-agent", "list_activity").unwrap();
        assert!(read > 900);
        let page = activity
            .list(
                "fixture-agent",
                read,
                &ListActivity {
                    limit: 50,
                    before_id: None,
                    include_reads: true,
                },
            )
            .unwrap();
        assert_eq!(page.activities.len(), if version == 1 { 2 } else { 1 });
        assert_eq!(page.activities[0].outcome, Outcome::Interrupted);
        assert_eq!(page.activities[0].id, if version == 1 { 42 } else { id });
        assert!(
            page.activities
                .iter()
                .all(|r| r.execution_mode.is_none() && r.claim.is_none())
        );
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            4
        );
    }
}

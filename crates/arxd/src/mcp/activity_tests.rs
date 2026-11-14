use super::*;
use crate::store::{Lifecycle, Outcome};
use rmcp::ServiceExt;
use serde_json::Value;

fn call(args: Value) -> CallToolRequestParams {
    CallToolRequestParams::new("list_activity").with_arguments(args.as_object().unwrap().clone())
}
fn content(response: CallToolResult) -> Value {
    serde_json::from_str(&response.content[0].as_text().unwrap().text).unwrap()
}
fn ids(page: &Value) -> Vec<i64> {
    page["activities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect()
}

// Actual SDK requests over an in-memory duplex; only the existing fake dispatcher
// and a fresh private temporary store are used. No native executor is invoked.
#[tokio::test]
async fn activity_id_command_correlation_visibility_and_pagination() {
    let dir = crate::service::application::tests::private_temp();
    let (executor, calls) = crate::service::executor::tests::fake(false, true);
    let mut activity = Activity::open_service_fixture(dir.path()).unwrap();
    activity.dedicated = Some(executor.clone());
    let activity = Arc::new(activity);
    let inventory = Inventory {
        activity: activity.clone(),
        execution: None,
        principal: Some(Principal("fixture-agent".into())),
    };
    let (s, c) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
    let client = ().serve(c).await.unwrap();
    let schema = client.list_all_tools().await.unwrap();
    let schema = &schema
        .iter()
        .find(|t| t.name == "list_activity")
        .unwrap()
        .input_schema;
    assert_eq!(schema["properties"]["activity_id"]["minimum"], 1);
    assert_eq!(schema["properties"]["activity_id"]["maximum"], i64::MAX);
    assert_eq!(
        schema["not"],
        json!({"required":["activity_id","before_id"]})
    );

    let mut commands = Vec::new();
    for _ in 0..3 {
        let result = content(
            client
                .call_tool(
                    CallToolRequestParams::new("run_command")
                        .with_arguments(json!({"tool":"cargo"}).as_object().unwrap().clone()),
                )
                .await
                .unwrap(),
        );
        commands.push(result["activity_id"].as_i64().unwrap());
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    let exact = content(
        client
            .call_tool(call(json!({"activity_id":commands[0]})))
            .await
            .unwrap(),
    );
    assert_eq!(ids(&exact), [commands[0]]);
    assert_eq!(exact["activities"][0]["tool"], "run_command");
    assert!(exact["next_before_id"].is_null());
    assert_eq!(exact.as_object().unwrap().len(), 2);

    let foreign = activity.start("other-agent", "run_command").unwrap();
    let lifecycle = activity.start_lifecycle(Lifecycle::Start).unwrap();
    let read = activity.start("fixture-agent", "list_activity").unwrap();
    activity.finish(read, "succeeded").unwrap();
    // A deleted, previously allocated ID also returns an empty page.
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    // Match the service fixture's retained journal on this second connection.
    db.pragma_update(None, "journal_mode", "TRUNCATE").unwrap();
    db.execute("DELETE FROM activity WHERE id=?1", [commands[1]])
        .unwrap();
    for id in [commands[1], foreign, lifecycle, read, i64::MAX] {
        let page = content(
            client
                .call_tool(call(json!({"activity_id":id})))
                .await
                .unwrap(),
        );
        assert_eq!(page, json!({"activities":[],"next_before_id":null}));
    }
    // Predict this request's own committed read ID, which remains excluded even
    // when read rows are requested.
    let own = db
        .query_row("SELECT max(id)+1 FROM activity", [], |r| r.get::<_, i64>(0))
        .unwrap();
    let page = content(
        client
            .call_tool(call(json!({"activity_id":own,"include_reads":true})))
            .await
            .unwrap(),
    );
    assert!(ids(&page).is_empty());
    let page = content(
        client
            .call_tool(call(
                json!({"activity_id":read,"include_reads":true,"limit":1}),
            ))
            .await
            .unwrap(),
    );
    assert_eq!(ids(&page), [read]);
    assert!(page["next_before_id"].is_null());
    let first = content(client.call_tool(call(json!({"limit":1}))).await.unwrap());
    assert_eq!(ids(&first), [commands[2]]);
    assert_eq!(first["next_before_id"], commands[2]);
    let second = content(
        client
            .call_tool(call(json!({"limit":1,"before_id":first["next_before_id"]})))
            .await
            .unwrap(),
    );
    assert_eq!(ids(&second), [commands[0]]);
    assert!(second["next_before_id"].is_null());
    assert_eq!(
        ids(&content(client.call_tool(call(json!({}))).await.unwrap())),
        [commands[2], commands[0]]
    );

    for bad in [
        json!({"activity_id":0}),
        json!({"activity_id":-1}),
        json!({"activity_id":null}),
        json!({"activity_id":1.5}),
        json!({"activity_id":"PRIVATE-MARKER"}),
        json!({"activity_id":true}),
        json!({"activity_id":[]}),
        json!({"activity_id":{}}),
        json!({"activity_id":9223372036854775808_u64}),
        json!({"activity_id":1,"before_id":2}),
        json!({"activity_id":1,"before_id":null}),
        json!({"activity_id":1,"limit":0}),
        json!({"activity_id":1,"limit":51}),
        json!({"activity_id":1,"include_reads":null}),
        json!({"activity_id":1,"include_reads":"true"}),
        json!({"activity_id":1,"principal":"PRIVATE-MARKER"}),
    ] {
        let error = client.call_tool(call(bad)).await.unwrap_err().to_string();
        assert!(error.contains("invalid activity arguments"));
        assert!(!error.contains("PRIVATE-MARKER"));
        let outcome: String = db
            .query_row(
                "SELECT outcome FROM activity ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(outcome, "rejected");
    }

    // Store entry point independently enforces the committed caller read and
    // rejects cursor conflicts, even without MCP validation.
    let args = crate::store::ListActivity {
        limit: 1,
        before_id: None,
        include_reads: true,
    };
    let foreign_read = activity.start("other-agent", "list_activity").unwrap();
    assert!(
        activity
            .list_filtered("fixture-agent", foreign_read, &args, Some(read))
            .is_err()
    );
    let own_read = activity.start("fixture-agent", "list_activity").unwrap();
    let later = activity.start("fixture-agent", "run_command").unwrap();
    let future_page = activity
        .list_filtered("fixture-agent", own_read, &args, Some(later))
        .unwrap();
    assert!(future_page.activities.is_empty());
    assert!(future_page.next_before_id.is_none());
    assert!(
        activity
            .list_filtered("fixture-agent", own_read, &args, Some(0))
            .is_err()
    );
    let conflict = crate::store::ListActivity {
        before_id: Some(read),
        ..args
    };
    assert!(
        activity
            .list_filtered("fixture-agent", own_read, &conflict, Some(read))
            .is_err()
    );
    activity
        .finish_typed(own_read, Outcome::Succeeded, &Default::default())
        .unwrap();
    assert!(
        activity
            .list_filtered("fixture-agent", own_read, &conflict, None)
            .is_err()
    );

    // Lookup still decodes through bounded/tampered-record validation.
    db.execute_batch("PRAGMA ignore_check_constraints=ON")
        .unwrap();
    db.execute(
        "UPDATE activity SET tool=?1 WHERE id=?2",
        rusqlite::params!["PRIVATE-MARKER".repeat(1000), commands[0]],
    )
    .unwrap();
    let error = client
        .call_tool(call(json!({"activity_id":commands[0]})))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("activity unavailable") && !error.contains("PRIVATE-MARKER"));
    // A good lookup cannot release a page when audit completion fails.
    db.execute_batch("CREATE TRIGGER fail_finish BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'injected'); END;").unwrap();
    let error = client
        .call_tool(call(json!({"activity_id":commands[2]})))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("activity unavailable") && !error.contains("activities"));
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
    executor.drain().await.unwrap();
}

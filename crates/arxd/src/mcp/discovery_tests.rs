use super::*;
use crate::development::Cancellation;
use crate::service::executor::Request;
use crate::service::executor::tests::{default_fixture, good};
use crate::service::{executor::*, layout::Platform, preflight::ExecutionUnavailable};
use crate::store::Rights;
use rmcp::ServiceExt;
use serde_json::Value;
use std::{collections::VecDeque, sync::Mutex};

// Use the existing dispatcher fixture and typed reports, never Native or a process.
struct Reports(Mutex<VecDeque<Report>>);
impl Executor for Reports {
    fn preflight(&self) -> Result<(), ExecutionUnavailable> {
        Ok(())
    }
    fn run(&self, request: Option<&Request>, _: &Cancellation, _: &Cancellation) -> Report {
        assert!(request.is_none());
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted discovery")
    }
}
fn discovered() -> Report {
    Report::Discovered {
        evidence: good(Platform::Windows, Rights::Standard),
        tools: crate::development::TOOLS
            .iter()
            .map(|name| ToolVersion {
                name: (*name).into(),
                found: *name == "cargo",
                version: (*name == "cargo").then(|| "cargo fixture 1".into()),
            })
            .collect(),
    }
}
fn content(response: CallToolResult) -> Value {
    serde_json::from_str(&response.content[0].as_text().unwrap().text).unwrap()
}

#[tokio::test]
async fn released_discovery_roundtrip_retains_cache_after_failures_and_shutdown() {
    let dir = crate::service::application::tests::private_temp();
    let mut partial = discovered();
    if let Report::Discovered { tools, .. } = &mut partial {
        tools.pop();
    }
    let e = default_fixture(Arc::new(Reports(Mutex::new(VecDeque::from([
        discovered(),
        Report::Failed {
            creation: Creation::NotInvoked,
        },
        Report::Unavailable(ExecutionUnavailable::MechanismUnavailable),
        partial,
    ])))));
    let mut activity = Activity::open_service_fixture(dir.path()).unwrap();
    activity.dedicated = Some(e.clone());
    let activity = Arc::new(activity);
    // An earlier startup observation may finish after the agent's discovery.
    let earlier = activity.discovery_observation().unwrap();
    let inventory = Inventory {
        activity: activity.clone(),
        execution: None,
        principal: Some(Principal("fixture-agent".into())),
    };
    let (s, c) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
    let client = ().serve(c).await.unwrap();
    let before = content(
        client
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap(),
    );
    assert_eq!(before["hosts"][0]["tool_discovery"], "unavailable");
    assert_eq!(before["hosts"][0]["tools"], json!([]));
    let released = content(
        client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
            .unwrap(),
    );
    assert_eq!(released["outcome"], "succeeded");
    assert!(!released["observed_identity"].is_null());
    let cached = activity.tool_cache().unwrap();
    let after = content(
        client
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap(),
    );
    assert_eq!(after["hosts"][0]["tools"], released["tools"]);
    assert_eq!(
        after["hosts"][0]["last_observed_identity"],
        released["observed_identity"]
    );
    assert_eq!(
        after["hosts"][0]["tools_observed_at"],
        cached.observed_at_ms
    );
    assert_eq!(after["hosts"][0]["tool_discovery"], "cached");
    assert_eq!(
        after["hosts"][0]["execution"],
        before["hosts"][0]["execution"]
    );
    assert_eq!(after["hosts"][0]["execution_available"], false);
    let mut stale = released.clone();
    stale["tools"] = json!([]);
    activity.cache_discovery(earlier, &stale);
    activity.cache_discovery((cached.ticket, cached.observed_at_ms), &stale);
    // The development producer's current response has no released outcome.
    // Do not manufacture a successful outcome or a dedicated identity for it.
    let legacy = json!({"execution_mode":crate::development::MODE,
        "claim":crate::development::CLAIM,"tools":released["tools"]});
    activity.cache_discovery(activity.discovery_observation().unwrap(), &legacy);
    for expected in ["failed", "refused", "failed"] {
        let result = client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await;
        if expected == "refused" {
            assert!(result.is_err());
        } else {
            assert_eq!(content(result.unwrap())["outcome"], expected);
        }
        let host = content(
            client
                .call_tool(CallToolRequestParams::new("list_hosts"))
                .await
                .unwrap(),
        );
        assert_eq!(host["hosts"][0]["tools"], released["tools"]);
        assert_eq!(host["hosts"][0]["tools_observed_at"], cached.observed_at_ms);
        assert_eq!(activity.tool_cache().unwrap().ticket, cached.ticket);
    }
    e.shutdown();
    let stopped = content(
        client
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap(),
    );
    assert_eq!(stopped["hosts"][0]["execution_available"], false);
    assert_eq!(stopped["hosts"][0]["execution"]["reason"], "quarantined");
    assert_eq!(stopped["hosts"][0]["tool_discovery"], "cached");
    assert!(
        client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
            .is_err()
    );
    assert!(
        client
            .call_tool(
                CallToolRequestParams::new("run_command")
                    .with_arguments(json!({"tool":"cargo"}).as_object().unwrap().clone())
            )
            .await
            .is_err()
    );
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
    e.drain().await.unwrap();
}

fn development_output() -> crate::development::ResultData {
    crate::development::ResultData {
        execution_mode: crate::development::MODE,
        claim: crate::development::CLAIM,
        dedicated: false,
        stop_reason: crate::store::StopReason::Exited,
        exit_code: Some(0),
        cleanup_confirmed: true,
        matches_current_user: true,
        stdout: "fixture version".into(),
        stderr: String::new(),
        stdout_dropped_bytes: 0,
        stderr_dropped_bytes: 0,
    }
}

#[tokio::test]
async fn development_producer_roundtrip_only_releases_complete_audited_inventory() {
    use crate::store::{Reason, StopReason};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = crate::service::application::tests::private_temp();
    let activity = Arc::new(Activity::open(dir.path()).unwrap());
    let stage = Arc::new(AtomicUsize::new(0));
    let control = stage.clone();
    let executor = crate::development::Execution::probe_fixture(move |r, cancel| {
        if r.tool != "cargo" {
            return Err(Reason::Unavailable);
        }
        match control.load(Ordering::Acquire) {
            1 => {
                let mut output = development_output();
                output.stop_reason = StopReason::Timeout;
                output.exit_code = None;
                Ok(output)
            }
            2 => Err(Reason::ExecutionFailed),
            3 => {
                cancel.cancel();
                Err(Reason::Unavailable)
            }
            4 => {
                let mut output = development_output();
                output.exit_code = Some(1);
                Ok(output)
            }
            _ => Ok(development_output()),
        }
    });
    let inventory = Inventory {
        activity: activity.clone(),
        execution: Some(executor.clone()),
        principal: Some(Principal("fixture-agent".into())),
    };
    let (s, c) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
    let client = ().serve(c).await.unwrap();
    let before = content(
        client
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap(),
    );
    assert_eq!(before["hosts"][0]["tool_discovery"], "discover_tools");
    assert!(activity.tool_cache().is_none());
    let released = content(
        client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
            .unwrap(),
    );
    // This is produced by the real discovery loop and audit dispatcher, not a
    // fixture that injects an outcome into a fabricated response.
    assert_eq!(released["outcome"], "succeeded");
    assert_eq!(released["tools"][0]["found"], true);
    assert_eq!(released["tools"][1]["found"], false);
    assert_eq!(
        released["tools"].as_array().unwrap().len(),
        crate::development::TOOLS.len()
    );
    assert_eq!(released["execution_mode"], crate::development::MODE);
    assert_eq!(released["claim"], crate::development::CLAIM);
    assert!(released["observed_identity"].is_null());
    let cached = activity.tool_cache().expect("completed discovery cached");
    let after = content(
        client
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap(),
    );
    assert_eq!(after["hosts"][0]["tool_discovery"], "cached");
    assert_eq!(after["hosts"][0]["tools"], released["tools"]);
    assert!(after["hosts"][0]["last_observed_identity"].is_null());
    assert_eq!(
        after["hosts"][0]["execution"],
        before["hosts"][0]["execution"]
    );
    for n in 1..=4 {
        stage.store(n, Ordering::Release);
        let response = client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await;
        match n {
            2 => assert!(response.is_err()),
            3 => assert!(content(response.unwrap()).is_null()),
            _ => {
                let partial = content(response.unwrap());
                assert_eq!(partial["outcome"], "failed");
                if n == 1 {
                    assert!(partial["tools"][0]["found"].is_null());
                }
            }
        }
        let kept = activity.tool_cache().unwrap();
        assert_eq!(kept.ticket, cached.ticket);
        assert_eq!(kept.tools, cached.tools);
        assert_eq!(kept.observed_at_ms, cached.observed_at_ms);
    }
    // Fail the actual audit UPDATE after probes finish, without changing store APIs.
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_discovery_finish BEFORE UPDATE ON activity WHEN OLD.tool='discover_tools' BEGIN SELECT RAISE(FAIL, 'fixture'); END;").unwrap();
    stage.store(0, Ordering::Release);
    assert!(
        client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
            .is_err()
    );
    assert_eq!(activity.tool_cache().unwrap().ticket, cached.ticket);
    db.execute_batch("DROP TRIGGER fail_discovery_finish;")
        .unwrap();
    executor.shutdown();
    assert!(
        client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
            .is_err()
    );
    assert_eq!(activity.tool_cache().unwrap().ticket, cached.ticket);
    let stopped = content(
        client
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap(),
    );
    assert_eq!(stopped["hosts"][0]["tool_discovery"], "cached");
    assert_eq!(stopped["hosts"][0]["tools"], released["tools"]);
    assert_eq!(stopped["hosts"][0]["execution_available"], false);
    assert!(stopped["hosts"][0]["last_observed_identity"].is_null());
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
    executor.drain().await;
}

#[tokio::test]
async fn development_discovery_latest_start_wins_when_older_finishes_last() {
    let dir = crate::service::application::tests::private_temp();
    let activity = Arc::new(Activity::open(dir.path()).unwrap());
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered_tx));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release = Mutex::new(release_rx);
    let observed_ticket = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let ticket = observed_ticket.clone();
    let a = activity.clone();
    let old = crate::development::Execution::probe_fixture(move |r, _| {
        if r.tool == "cargo" {
            assert!(a.tool_cache().is_none(), "no cache before release");
            // A ticket allocated inside the first probe must follow MCP's ticket.
            ticket.store(
                a.discovery_observation().unwrap().0,
                std::sync::atomic::Ordering::Release,
            );
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
        let mut result = development_output();
        result.stdout = "older".into();
        Ok(result)
    });
    let new = crate::development::Execution::probe_fixture(|_, _| {
        let mut result = development_output();
        result.stdout = "newer".into();
        Ok(result)
    });
    let mut servers = Vec::new();
    let mut clients = Vec::new();
    for executor in [old, new] {
        let inventory = Inventory {
            activity: activity.clone(),
            execution: Some(executor),
            principal: Some(Principal("fixture-agent".into())),
        };
        let (s, c) = tokio::io::duplex(65536);
        servers.push(tokio::spawn(async move {
            inventory.serve(s).await.unwrap().waiting().await
        }));
        clients.push(Arc::new(().serve(c).await.unwrap()));
    }
    let old_client = clients[0].clone();
    let pending = tokio::spawn(async move {
        old_client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .unwrap();
    let newer = content(
        clients[1]
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
            .unwrap(),
    );
    let cache = activity.tool_cache().unwrap();
    assert!(cache.ticket > observed_ticket.load(std::sync::atomic::Ordering::Acquire));
    release_tx.send(()).unwrap();
    let older = content(pending.await.unwrap().unwrap());
    assert_eq!(older["outcome"], "succeeded");
    assert_ne!(older["tools"], newer["tools"]);
    assert_eq!(activity.tool_cache().unwrap().ticket, cache.ticket);
    assert_eq!(activity.tool_cache().unwrap().tools, newer["tools"]);
    // Equal tickets cannot rewrite a released observation either.
    activity.cache_discovery((cache.ticket, cache.observed_at_ms), &older);
    assert_eq!(activity.tool_cache().unwrap().tools, newer["tools"]);
    let host = content(
        clients[0]
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap(),
    );
    assert_eq!(host["hosts"][0]["tools"], newer["tools"]);
    // First MCP ticket was reserved before the blocking producer began work.
    assert_eq!(
        observed_ticket.load(std::sync::atomic::Ordering::Acquire),
        2
    );
    for client in clients {
        Arc::try_unwrap(client)
            .ok()
            .unwrap()
            .cancel()
            .await
            .unwrap();
    }
    for server in servers {
        server.await.unwrap().unwrap();
    }
}

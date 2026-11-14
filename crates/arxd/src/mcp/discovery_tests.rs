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

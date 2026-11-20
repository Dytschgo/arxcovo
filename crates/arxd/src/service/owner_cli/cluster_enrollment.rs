//! Refusal tests for the existing owner CLI and authenticated agent endpoint.
use super::{Command, parse};
use crate::{
    config::{Agent, Config},
    local::{self, Endpoint, Listener},
    service::{Shutdown, application::tests::private_temp},
    store::Activity,
};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ErrorCode},
    service::ServiceError,
    transport::async_rw::AsyncRwTransport,
};
use sha2::{Digest, Sha256};
use std::{ffi::OsString, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn owner_cluster_verbs_refuse_before_native_authority() {
    let check = |args: &[&str]| parse(&args.iter().map(OsString::from).collect::<Vec<_>>());
    assert!(matches!(check(&["service", "status"]), Ok(Command::Status)));
    for args in [
        &["service", "cluster", "mint"][..],
        &["service", "cluster", "remove"],
        &["service", "mint"],
        &["service", "remove"],
        &["service", "mint", "--verifier", "synthetic"],
    ] {
        assert!(check(args).is_err(), "cluster verb reached native dispatch");
    }
}

#[tokio::test]
async fn authenticated_agent_has_no_mint_or_remove_dispatch() {
    let dir = private_temp();
    #[cfg(unix)]
    let endpoint = Endpoint {
        address: dir.path().join("a.sock").to_str().unwrap().into(),
        client_identity: unsafe { libc::geteuid() }.to_string(),
    };
    #[cfg(windows)]
    let endpoint = Endpoint {
        address: format!(
            r"\\.\pipe\arxcovo-step8a-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ),
        client_identity: local::current_identity().unwrap(),
    };
    let token = "a".repeat(64); // Fixed synthetic local::fixture_connect credential.
    let config = Arc::new(Config {
        state_dir: dir.path().into(),
        agents: vec![Agent {
            principal: "owner".into(),
            token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
        }],
        ..Config::default()
    });
    let activity = Arc::new(Activity::open(dir.path()).unwrap());
    let listener = Listener::bind(&endpoint).unwrap();
    let stop = Shutdown::default();
    let server = tokio::spawn(listener.run_until(config, activity, stop.clone()));

    tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = local::fixture_connect(endpoint).await;
        stream.write_all(format!("ARX1 {token}\n").as_bytes()).await.unwrap();
        let mut ack = [0; 3];
        stream.read_exact(&mut ack).await.unwrap();
        assert_eq!(&ack, b"OK\n");
        let (read, write) = tokio::io::split(stream);
        let agent = ().serve(AsyncRwTransport::new_client(read, write)).await.unwrap();
        let tools = agent.list_tools(None).await.unwrap().tools;
        assert!(tools.iter().any(|tool| tool.name == "list_hosts"));
        for name in ["mint", "remove", "cluster_mint", "cluster_remove"] {
            assert!(tools.iter().all(|tool| tool.name != name));
            let result = agent.call_tool(CallToolRequestParams::new(name)).await;
            assert!(
                matches!(result, Err(ServiceError::McpError(e))
                    if e.code == ErrorCode::INVALID_PARAMS && e.message == "unknown tool or arguments")
            );
        }
        agent.cancel().await.unwrap();
        stop.cancel();
        server.await.unwrap().unwrap();
    })
    .await
    .expect("agent refusal observation elapsed");
}

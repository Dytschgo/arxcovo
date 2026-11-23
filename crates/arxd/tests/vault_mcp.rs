use arxd::{
    config::{Agent, Config},
    local::{Endpoint, Listener},
    service::Shutdown,
    store::Activity,
};
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::async_rw::AsyncRwTransport};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fmt::{self, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

#[path = "vault_fixtures/private.rs"]
mod private;

type Peer = rmcp::service::RunningService<rmcp::RoleClient, ()>;

#[derive(Default)]
struct Capture {
    events: Arc<Mutex<String>>,
    next_span: AtomicU64,
}

struct Fields<'a>(&'a mut String);

impl Visit for Fields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        writeln!(self.0, "{}={value:?}", field.name()).unwrap();
    }
}

impl Subscriber for Capture {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn event(&self, event: &Event<'_>) {
        event.record(&mut Fields(&mut self.events.lock().unwrap()));
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(self.next_span.fetch_add(1, Ordering::Relaxed) + 1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

async fn client(endpoint: &Endpoint, token: &str) -> Peer {
    #[cfg(windows)]
    let mut stream = loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(&endpoint.address) {
            Ok(stream) => break stream,
            Err(err) if err.raw_os_error() == Some(231) => tokio::task::yield_now().await,
            Err(err) => panic!("vault fixture endpoint: {:?}", err.kind()),
        }
    };
    #[cfg(unix)]
    let mut stream = tokio::net::UnixStream::connect(&endpoint.address)
        .await
        .unwrap();
    stream
        .write_all(format!("ARX1 {token}\n").as_bytes())
        .await
        .unwrap();
    let mut ok = [0u8; 3];
    stream.read_exact(&mut ok).await.unwrap();
    assert_eq!(&ok, b"OK\n");
    let (read, write) = tokio::io::split(stream);
    ().serve(AsyncRwTransport::new_client(read, write))
        .await
        .unwrap()
}

async fn call(peer: &Peer, tool: &str, args: Value) -> Value {
    let mut request = CallToolRequestParams::new(tool.to_owned());
    request.arguments = Some(args.as_object().unwrap().clone());
    let response = peer.call_tool(request).await.unwrap();
    let value = response.structured_content.unwrap();
    assert_eq!(response.is_error.unwrap_or(false), value["ok"] == false);
    value
}

#[tokio::test]
async fn vault_mcp_auth_scope_validation_replay_and_declarations() {
    assert_eq!(
        tracing::level_filters::STATIC_MAX_LEVEL,
        tracing::level_filters::LevelFilter::INFO
    );
    assert_eq!(log::STATIC_MAX_LEVEL, log::LevelFilter::Info);
    let capture = Capture::default();
    let events = capture.events.clone();
    // The current-thread test runtime drives both peers and all SDK handlers.
    // This subscriber accepts DEBUG and TRACE if a compiled callsite emits them.
    let _guard = tracing::subscriber::set_default(capture);
    let sentinel = format!("exit 0 # fixture-{}", rand::random::<u64>());
    tokio::time::timeout(Duration::from_secs(5), journey(&sentinel))
        .await
        .unwrap();
    let events = events.lock().unwrap();
    assert!(
        events.contains("Service initialized as server"),
        "SDK INFO capture is live"
    );
    assert!(
        !events.contains(&sentinel),
        "SDK tool payload leaked into captured events"
    );
}

async fn journey(sentinel: &str) {
    let dir = private::private_dir();
    #[cfg(windows)]
    let endpoint = Endpoint {
        address: format!(r"\\.\pipe\arxcovo-vault-{}", rand::random::<u64>()),
        client_identity: private::identity(),
    };
    #[cfg(unix)]
    let endpoint = Endpoint {
        address: dir.path().join("v.sock").to_str().unwrap().into(),
        // SAFETY: geteuid takes no arguments and cannot invalidate Rust state.
        client_identity: unsafe { libc::geteuid() }.to_string(),
    };
    let credentials = [
        (hex::encode(rand::random::<[u8; 32]>()), "first"),
        (hex::encode(rand::random::<[u8; 32]>()), "second"),
    ];
    let config = Arc::new(Config {
        state_dir: dir.path().into(),
        local: Some(endpoint.clone()),
        listen: None,
        dashboard: None,
        execution: None,
        development_execution: None,
        execution_settings: None,
        agents: credentials
            .iter()
            .map(|(token, principal)| Agent {
                principal: (*principal).into(),
                token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
            })
            .collect(),
    });
    let activity = Arc::new(Activity::open(dir.path()).unwrap());
    let _vault = arxd::vault::initialize_foreground(&activity, dir.path()).unwrap();
    let listener = Listener::bind(&endpoint).unwrap();
    let shutdown = Shutdown::default();
    let server = tokio::spawn(listener.run_until(config, activity, shutdown.clone()));
    let first = client(&endpoint, &credentials[0].0).await;
    let second = client(&endpoint, &credentials[1].0).await;

    let tools = first.list_tools(None).await.unwrap().tools;
    for name in [
        "vault_list",
        "vault_get",
        "vault_add_version",
        "vault_test_version",
        "vault_activate",
        "vault_history",
        "template_tools",
    ] {
        assert!(tools.iter().any(|tool| tool.name == name));
    }
    let inventory = call(&first, "template_tools", json!({})).await;
    assert_eq!(inventory["entries"][0]["inventory"], "declared");
    assert_eq!(inventory["entries"][0]["tools"], json!(["arx-guest"]));
    assert_eq!(inventory["entries"][1]["inventory"], "unavailable");
    assert!(!inventory.to_string().contains("observed"));

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let key = |n: u64| format!("{now:016x}{n:016x}");
    let args = json!({"os":"ubuntu","name":"build","script":sentinel,"request_id":key(1)});
    let version = call(&first, "vault_add_version", args.clone()).await;
    assert_eq!(version["ok"], true);
    assert!(!version.to_string().contains(sentinel));
    assert_eq!(
        call(&first, "vault_add_version", args.clone()).await,
        version
    );
    let owned = call(
        &first,
        "vault_get",
        json!({"version_id":version["version_id"],
        "include_content":true}),
    )
    .await;
    assert_eq!(owned["content"], sentinel);
    assert_eq!(owned["author"], "first");
    assert_eq!(
        call(
            &second,
            "vault_get",
            json!({"version_id":version["version_id"],
        "include_content":true})
        )
        .await["reason"],
        "not_found"
    );
    assert_eq!(
        call(
            &second,
            "vault_activate",
            json!({"version_id":version["version_id"],
        "request_id":key(1)})
        )
        .await["reason"],
        "not_found"
    );
    assert_eq!(
        call(
            &first,
            "vault_test_version",
            json!({"version_id":version["version_id"],
        "request_id":key(2)})
        )
        .await["reason"],
        "tester_unavailable"
    );
    assert_eq!(
        call(
            &first,
            "vault_activate",
            json!({"version_id":version["version_id"],
        "request_id":key(3)})
        )
        .await["reason"],
        "pass_required"
    );

    let windows = call(
        &first,
        "vault_add_version",
        json!({"os":"windows","name":"build",
        "script":"exit 0","request_id":key(4)}),
    )
    .await;
    assert_eq!(windows["ok"], true);
    let scripts = call(&first, "vault_list", json!({"limit":1})).await;
    let cursor = &scripts["next_cursor"];
    assert!(cursor.is_string());
    let after = call(&first, "vault_list", json!({"limit":1,"cursor":cursor})).await;
    assert_eq!(after["entries"][0]["os"], "ubuntu");
    assert_eq!(after["entries"][0]["script_id"], owned["script_id"]);
    assert_eq!(
        call(&second, "vault_list", json!({"cursor":cursor})).await["reason"],
        "invalid_cursor"
    );
    assert_eq!(
        call(
            &first,
            "vault_history",
            json!({"script_id":owned["script_id"],
        "cursor":cursor})
        )
        .await["reason"],
        "invalid_cursor"
    );
    let history = call(
        &first,
        "vault_history",
        json!({"script_id":owned["script_id"]}),
    )
    .await;
    assert_eq!(history["entries"][0]["version_id"], version["version_id"]);
    assert_eq!(history["entries"][0]["state"], "draft");
    assert!(history["entries"][0].get("content").is_none());

    for (tool, bad) in [
        ("vault_list", json!({"cursor":null})),
        ("vault_list", json!({"limit":21})),
        ("vault_list", json!({"limit":1.5})),
        (
            "vault_get",
            json!({"version_id":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}),
        ),
        (
            "vault_get",
            json!({"version_id":version["version_id"], "include_content":1}),
        ),
        (
            "vault_add_version",
            json!({"os":"ubuntu","name":"../escape","script":"exit 0","request_id":key(5)}),
        ),
        (
            "vault_add_version",
            json!({"os":"ubuntu","name":"build","script":"\u{0}","request_id":key(5)}),
        ),
        (
            "vault_add_version",
            json!({"os":"ubuntu","name":"build","script":"à".repeat(2049),"request_id":key(5)}),
        ),
        (
            "vault_add_version",
            json!({"os":"macos","name":"build","script":"exit 0","request_id":key(5)}),
        ),
        ("template_tools", json!({"extra":true})),
    ] {
        assert_eq!(
            call(&first, tool, bad).await,
            json!({"ok":false,"reason":"invalid"})
        );
    }
    let audit = call(&first, "list_activity", json!({"source":"vault"})).await;
    assert!(!audit.to_string().contains(sentinel));
    let entries = audit["activities"].as_array().unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["outcome"] == "succeeded")
            .count(),
        2
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["sha256"] == owned["sha256"])
    );
    let foreign = call(&second, "list_activity", json!({"source":"vault"})).await;
    assert_eq!(foreign["activities"].as_array().unwrap().len(), 1);
    assert_eq!(foreign["activities"][0]["reason"], "not_found");
    assert!(foreign["activities"][0].get("version_id").is_none());
    first.cancel().await.unwrap();
    second.cancel().await.unwrap();
    shutdown.cancel();
    server.await.unwrap().unwrap();
}

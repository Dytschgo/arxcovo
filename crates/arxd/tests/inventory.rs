use arxd::{
    config::{Agent, Config},
    store::Activity,
};
use rand::RngCore;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

fn private_dir() -> tempfile::TempDir {
    // macOS exposes its temporary root through /var, a system symlink.
    // Resolve that trusted test root before creating a valid fixture; application
    // paths and the explicit symlink rejection tests still reject links.
    #[cfg(target_os = "macos")]
    let root = std::env::temp_dir().canonicalize().unwrap();
    #[cfg(not(target_os = "macos"))]
    let root = std::env::temp_dir();
    let dir = tempfile::tempdir_in(root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    protect(dir.path());
    dir
}
fn config(path: &Path) -> (Config, String) {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    (
        Config {
            listen: Some("127.0.0.1:0".parse().unwrap()),
            state_dir: path.to_owned(),
            agents: vec![Agent {
                principal: "test-agent".into(),
                token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
            }],
        },
        token,
    )
}
async fn server(config: Config) -> (String, tokio::task::JoinHandle<()>) {
    let config = Arc::new(config);
    let activity = Arc::new(Activity::open(&config.state_dir).unwrap());
    let listener = arxd::http::bind(config.clone()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = arxd::http::router(config, addr, activity);
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{addr}/mcp"), task)
}
async fn client(url: &str, token: &str) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url.to_owned()).auth_header(token),
    );
    tokio::time::timeout(std::time::Duration::from_secs(10), ().serve(transport))
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn sdk_initialize_inventory_activity_restart_and_secrets() {
    let dir = private_dir();
    let (cfg, token) = config(dir.path());
    let (url, task) = server(cfg).await;
    let peer = client(&url, &token).await;
    assert!(peer.peer_info().is_some());
    let tools = peer.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 1);
    assert_eq!(tools.tools[0].name, "list_hosts");
    let response = peer
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap();
    let text = serde_json::to_string(&response).unwrap();
    assert!(text.contains(&sysinfo::System::cpu_arch()));
    assert!(text.contains(std::env::consts::OS));
    assert!(!text.contains(&token));
    assert!(!text.contains("USERPROFILE"));
    let payload: serde_json::Value =
        serde_json::from_str(response.content[0].as_text().unwrap().text.as_str()).unwrap();
    let host = &payload["hosts"][0];
    let host_id = host["host_id"].as_str().unwrap().to_owned();
    assert_host_id(&host_id);
    assert!(host["logical_cpus"].as_u64().unwrap() > 0);
    assert!(host["ram_bytes"].as_u64().unwrap() > 0);
    assert!(host["disk_total_bytes"].as_u64().unwrap() > 0);
    assert_eq!(host["execution_available"], false);
    assert!(
        peer.call_tool(CallToolRequestParams::new("run_command"))
            .await
            .is_err()
    );
    let args = serde_json::json!({"secret_marker":token})
        .as_object()
        .unwrap()
        .clone();
    let rejection = peer
        .call_tool(CallToolRequestParams::new("list_hosts").with_arguments(args))
        .await
        .unwrap_err()
        .to_string();
    assert!(!rejection.contains(&token));
    peer.cancel().await.unwrap();
    task.abort();
    let _ = task.await;
    let dbpath = dir.path().join("activity.sqlite3");
    let db = rusqlite::Connection::open(&dbpath).unwrap();
    let row: (String, String, String) = db
        .query_row(
            "SELECT principal,tool,outcome FROM activity ORDER BY id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        ("test-agent".into(), "list_hosts".into(), "succeeded".into())
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM activity WHERE outcome='rejected'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        2
    );
    drop(db);
    assert!(
        !std::fs::read(&dbpath)
            .unwrap()
            .windows(token.len())
            .any(|w| w == token.as_bytes())
    );
    let (cfg, replacement) = config(dir.path());
    let (url, task) = server(cfg).await;
    assert_eq!(
        reqwest::Client::new()
            .post(&url)
            .bearer_auth(&token)
            .body("{}")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let peer = client(&url, &replacement).await;
    let response = peer
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap();
    let payload: serde_json::Value =
        serde_json::from_str(&response.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(payload["hosts"][0]["host_id"], host_id);
    peer.cancel().await.unwrap();
    task.abort();
    let _ = task.await;
    let db = rusqlite::Connection::open(&dbpath).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM activity", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        4
    );
    drop(db);
    let fresh = private_dir();
    let (cfg, token) = config(fresh.path());
    let (url, task) = server(cfg).await;
    let peer = client(&url, &token).await;
    let response = peer
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap();
    let payload: serde_json::Value =
        serde_json::from_str(&response.content[0].as_text().unwrap().text).unwrap();
    let fresh_id = payload["hosts"][0]["host_id"].as_str().unwrap();
    assert_host_id(fresh_id);
    assert_ne!(fresh_id, host_id);
    assert!(payload["hosts"][0]["ram_bytes"].as_u64().unwrap() > 0);
    peer.cancel().await.unwrap();
    task.abort();
    let _ = task.await;
}
#[tokio::test]
async fn http_boundary_denies_before_sdk_without_activity() {
    let dir = private_dir();
    let (cfg, token) = config(dir.path());
    let (url, task) = server(cfg).await;
    let http = reqwest::Client::new();
    for token_value in [
        None,
        Some("bad"),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ] {
        let mut req = http.post(&url).body("{}");
        if let Some(t) = token_value {
            req = req.bearer_auth(t);
        }
        assert_eq!(req.send().await.unwrap().status(), 401);
    }
    for (name, value) in [
        ("host", "evil.example"),
        ("host", "localhost"),
        ("origin", "https://evil.example"),
        ("origin", "null"),
    ] {
        assert_eq!(
            http.post(&url)
                .bearer_auth(&token)
                .header(name, value)
                .body("{}")
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    assert_eq!(
        http.post(&url)
            .bearer_auth(&token)
            .body(vec![b'x'; 8193])
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    // Streaming body has no Content-Length; limit must apply while reading.
    let (mut tx, rx) = tokio::io::duplex(16384);
    use tokio::io::AsyncWriteExt;
    tokio::spawn(async move {
        tx.write_all(&[b'x'; 8193]).await.unwrap();
    });
    let body = reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(rx));
    assert_eq!(
        http.post(&url)
            .bearer_auth(&token)
            .body(body)
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    task.abort();
    let _ = task.await;
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM activity", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn activity_failure_refuses_inventory() {
    let dir = private_dir();
    let (cfg, token) = config(dir.path());
    let (url, task) = server(cfg).await;
    let peer = client(&url, &token).await;
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON activity BEGIN SELECT RAISE(FAIL, 'storage unavailable'); END;").unwrap();
    let error = peer
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("activity unavailable"));
    assert!(!error.contains("ram_bytes"));
    assert!(!error.contains("host_id"));
    peer.cancel().await.unwrap();
    task.abort();
    let _ = task.await;
}
#[test]
fn activity_retention_and_interrupted_restart() {
    let dir = private_dir();
    let store = Activity::open(dir.path()).unwrap();
    let host_id = store.host_id().unwrap();
    for _ in 0..1005 {
        store.start("test-agent", "list_hosts").unwrap();
    }
    drop(store);
    let store = Activity::open(dir.path()).unwrap();
    assert_eq!(store.host_id().unwrap(), host_id);
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM activity WHERE outcome='interrupted'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1000
    );
    assert!(
        std::fs::metadata(dir.path().join("activity.sqlite3"))
            .unwrap()
            .len()
            <= 4 * 1024 * 1024
    );
    drop(store);
}
#[tokio::test]
async fn configuration_validation_and_disabled_default() {
    let dir = private_dir();
    let (mut cfg, token) = config(dir.path());
    cfg.validate().unwrap();
    assert_eq!(cfg.authenticate(&token).as_deref(), Some("test-agent"));
    cfg.listen = None;
    assert!(arxd::http::bind(Arc::new(cfg)).await.is_err());
    let (mut cfg, _) = config(dir.path());
    cfg.listen = Some("0.0.0.0:8900".parse().unwrap());
    assert!(cfg.validate().is_err());
    let (mut cfg, _) = config(dir.path());
    cfg.agents[0].principal = "../private".into();
    assert!(cfg.validate().is_err());
    let (mut cfg, _) = config(dir.path());
    cfg.agents[0].token_sha256 = "password".into();
    assert!(cfg.validate().is_err());
    assert!(
        serde_json::from_str::<Config>(r#"{"state_dir":"x","agents":[],"execute_as":"owner"}"#)
            .is_err()
    );
    assert!(arxd::config::checked_path(Path::new("relative")).is_err());
    let file = dir.path().join("config.json");
    std::fs::write(&file, vec![b'x'; 16385]).unwrap();
    assert!(Config::load(&file).is_err());
}
#[test]
fn cli_does_not_echo_configuration_secrets() {
    let dir = private_dir();
    let file = dir.path().join("invalid.json");
    let marker = "SENSITIVE-TEST-MARKER-NEVER-LOG";
    std::fs::write(&file, format!("{{\"bad\":\"{marker}\"}}")).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_arxd"))
        .arg("--config")
        .arg(file)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(marker));
}

#[cfg(windows)]
fn protect(path: &Path) {
    protect_with_extra(path, "");
}
#[cfg(windows)]
fn protect_with_extra(path: &Path, extra: &str) {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
    };
    unsafe {
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut old = ptr::null_mut();
        let mut owner = ptr::null_mut();
        assert_eq!(
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut old
            ),
            0
        );
        let mut sid = ptr::null_mut();
        assert_ne!(ConvertSidToStringSidW(owner, &mut sid), 0);
        let mut len = 0;
        while *sid.add(len) != 0 {
            len += 1;
        }
        let sid_text = String::from_utf16(std::slice::from_raw_parts(sid, len)).unwrap();
        let sddl: Vec<u16> =
            format!("D:P(A;OICI;FA;;;{sid_text})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA){extra}")
                .encode_utf16()
                .chain(Some(0))
                .collect();
        let mut descriptor = ptr::null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut()
            ),
            0
        );
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = ptr::null_mut();
        assert_ne!(
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
            0
        );
        assert_eq!(
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut()
            ),
            0
        );
        LocalFree(descriptor);
        LocalFree(sid.cast());
        LocalFree(old);
    }
}

#[test]
fn private_configuration_load_and_metadata_bounds() {
    let dir = private_dir();
    let (cfg, token) = config(dir.path());
    let file = dir.path().join("config.json");
    let value = serde_json::json!({"listen":"127.0.0.1:0","state_dir":dir.path(),"agents":[{"principal":"test-agent","token_sha256":cfg.agents[0].token_sha256}]});
    std::fs::write(&file, value.to_string()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(Config::load(&file).unwrap().authenticate(&token).is_some());
    std::fs::write(&file, vec![b'x'; 16385]).unwrap();
    assert!(Config::load(&file).is_err());
    std::fs::write(
        &file,
        r#"{"state_dir":"private","agents":[],"unknown":"sensitive"}"#,
    )
    .unwrap();
    assert_eq!(
        Config::load(&file).err().unwrap().to_string(),
        "invalid configuration"
    );
    let store = Activity::open(dir.path()).unwrap();
    assert!(Activity::open(dir.path()).is_err());
    assert!(store.start(&"a".repeat(65), "list_hosts").is_err());
    assert!(store.start("agent", "untrusted-tool").is_err());
    assert!(store.finish(1, "untrusted-outcome").is_err());
}

#[tokio::test]
async fn completion_storage_failure_withholds_inventory_and_sdk_bad_token_fails() {
    let dir = private_dir();
    let (cfg, token) = config(dir.path());
    let (url, task) = server(cfg).await;
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url.clone()).auth_header("b".repeat(64)),
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), ().serve(transport))
            .await
            .unwrap()
            .is_err()
    );
    let peer = client(&url, &token).await;
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_update BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'no write'); END;").unwrap();
    let error = peer
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("activity unavailable"));
    assert!(!error.contains("ram_bytes"));
    assert!(!error.contains("host_id"));
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM activity WHERE outcome='started'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        1
    );
    peer.cancel().await.unwrap();
    task.abort();
    let _ = task.await;
}

#[cfg(unix)]
#[test]
fn permissive_files_and_symlinks_are_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = private_dir();
    let file = dir.path().join("config.json");
    std::fs::write(&file, "{}").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Config::load(&file).is_err());
    let link = dir.path().join("link");
    symlink(&file, &link).unwrap();
    assert!(Config::load(&link).is_err());
    let state = dir.path().join("state");
    symlink(dir.path(), &state).unwrap();
    assert!(Activity::open(&state).is_err());
}

#[cfg(windows)]
#[test]
fn inherit_only_shared_acl_refuses_before_creating_activity_files() {
    let dir = private_dir();
    protect_with_extra(dir.path(), "(A;OICIIO;FA;;;WD)");
    let (cfg, _) = config(dir.path());
    let config_refused = cfg.validate().is_err();
    let store_refused = Activity::open(dir.path()).is_err();
    let created_files = std::fs::read_dir(dir.path()).unwrap().count();
    protect(dir.path());
    assert!(
        config_refused,
        "inherit-only grants must invalidate state configuration"
    );
    assert!(
        store_refused,
        "activity creation must also reject inherited child access"
    );
    assert_eq!(
        created_files, 0,
        "refusal must precede private file creation"
    );
}
#[cfg(windows)]
#[test]
fn inherited_shared_acl_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    // A broadly granted ACE is deliberately applied to this disposable fixture only.
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
    };
    unsafe {
        let name: Vec<u16> = dir
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let sddl: Vec<u16> = "D:P(A;OICI;FA;;;WD)"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = ptr::null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut()
            ),
            0
        );
        let (mut present, mut defaulted) = (0, 0);
        let mut acl = ptr::null_mut();
        assert_ne!(
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
            0
        );
        assert_eq!(
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut()
            ),
            0
        );
        LocalFree(descriptor);
    }
    let (cfg, _) = config(dir.path());
    assert!(cfg.validate().is_err());
    protect(dir.path());
}
#[tokio::test]
async fn distinct_sdk_credentials_persist_distinct_principals() {
    let dir = private_dir();
    let (mut cfg, token) = config(dir.path());
    let (mut other, other_token) = config(dir.path());
    other.agents[0].principal = "second-agent".into();
    cfg.agents.push(other.agents.remove(0));
    let (url, task) = server(cfg).await;
    for credential in [&token, &other_token] {
        let peer = client(&url, credential).await;
        peer.call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap();
        peer.cancel().await.unwrap();
    }
    task.abort();
    let _ = task.await;
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    let mut query = db
        .prepare("SELECT principal FROM activity ORDER BY id")
        .unwrap();
    let names: Vec<String> = query
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(names, ["test-agent", "second-agent"]);
}

fn assert_host_id(id: &str) {
    assert_eq!(id.len(), 64);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}

#[test]
fn corrupt_identity_fails_closed_without_replacement() {
    for mutation in [
        "UPDATE local_host_identity SET host_id=''",
        "UPDATE local_host_identity SET host_id='SENSITIVE-IDENTITY-MARKER'",
        "UPDATE local_host_identity SET host_id=replace(printf('%064d',0),'0','A')",
        "UPDATE local_host_identity SET host_id=zeroblob(64)",
        "UPDATE local_host_identity SET host_id=printf('%0100000d',0)",
        "DELETE FROM local_host_identity",
        "DROP TABLE local_host_identity",
        "PRAGMA user_version=2",
        "PRAGMA user_version=0",
        "DROP TABLE local_host_identity; CREATE TABLE local_host_identity(singleton,host_id); INSERT INTO local_host_identity VALUES(1,NULL)",
        "DROP TABLE local_host_identity; CREATE TABLE local_host_identity(singleton,host_id); INSERT INTO local_host_identity VALUES(2,printf('%064d',0))",
        "DROP TABLE local_host_identity; CREATE TABLE local_host_identity(singleton,host_id); INSERT INTO local_host_identity VALUES(1,printf('%064d',0)),(1,printf('%064d',0))",
    ] {
        let dir = private_dir();
        drop(Activity::open(dir.path()).unwrap());
        let path = dir.path().join("activity.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(mutation).unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(
            Activity::open(dir.path()).is_err(),
            "accepted mutation: {mutation}"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "rewrote corrupt metadata"
        );
    }
}

#[test]
fn identity_migration_and_activity_recovery_commit_atomically() {
    let dir = private_dir();
    let store = Activity::open(dir.path()).unwrap();
    store.start("test-agent", "list_hosts").unwrap();
    drop(store);
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    // Reproduce the pre-identity schema, preserving an actual started activity.
    db.execute_batch("DROP TABLE local_host_identity; PRAGMA user_version=0; CREATE TRIGGER fail_recovery BEFORE UPDATE ON activity BEGIN SELECT RAISE(FAIL,'storage unavailable'); END;").unwrap();
    assert!(Activity::open(dir.path()).is_err());
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='local_host_identity'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
    db.execute_batch("DROP TRIGGER fail_recovery").unwrap();
    let store = Activity::open(dir.path()).unwrap();
    let id = store.host_id().unwrap();
    assert_host_id(&id);
    assert_eq!(
        db.query_row("SELECT outcome FROM activity", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "interrupted"
    );
    drop(store);
    assert_eq!(Activity::open(dir.path()).unwrap().host_id().unwrap(), id);
}

#[tokio::test]
async fn identity_corruption_during_server_lifetime_withholds_inventory() {
    let dir = private_dir();
    let (cfg, token) = config(dir.path());
    let (url, task) = server(cfg).await;
    let peer = client(&url, &token).await;
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    db.execute(
        "UPDATE local_host_identity SET host_id=?1",
        ["SENSITIVE-IDENTITY-MARKER"],
    )
    .unwrap();
    let error = peer
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("activity unavailable"));
    for withheld in ["ram_bytes", "host_id", "SENSITIVE-IDENTITY-MARKER", &token] {
        assert!(!error.contains(withheld));
    }
    peer.cancel().await.unwrap();
    task.abort();
    let _ = task.await;
}

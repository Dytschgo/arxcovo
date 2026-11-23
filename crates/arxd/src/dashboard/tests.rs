use super::*;
use crate::config::{Agent, Config};
use axum::body::Body;
use sha2::{Digest, Sha256};
use std::{fs, net::IpAddr, sync::Mutex};
use tower::ServiceExt;

const PASSWORD: &str = "fake-password-sentinel-for-test";
const ORIGIN: &str = "http://localhost:4444";
const NOW: i64 = 1_795_435_200_000;

struct TestClock(Mutex<ClockReading>);
impl Clock for TestClock {
    fn now(&self) -> ClockReading {
        *self.0.lock().unwrap()
    }
}
impl TestClock {
    fn advance(&self, duration: Duration) {
        let mut time = self.0.lock().unwrap();
        time.monotonic += duration;
        time.utc_ms += duration.as_millis() as i64;
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    app: Router,
    activity: Arc<Activity>,
    clock: Arc<TestClock>,
}

fn directory() -> tempfile::TempDir {
    crate::service::application::tests::private_temp()
}

impl Fixture {
    fn new() -> Self {
        Self::with_catalog(false)
    }

    fn with_catalog(catalog: bool) -> Self {
        let dir = directory();
        let assets = dir.path().join("dist");
        fs::create_dir(&assets).unwrap();
        fs::write(assets.join("index.html"), "<!doctype html>").unwrap();
        fs::write(assets.join("script.js"), "export {};").unwrap();
        let activity = Arc::new(Activity::open(dir.path()).unwrap());
        let clock = Arc::new(TestClock(Mutex::new(ClockReading {
            monotonic: Instant::now(),
            utc_ms: NOW,
        })));
        let vm_catalog = catalog.then(|| {
            let root = dir.path().join("vm");
            fs::create_dir(&root).unwrap();
            let _controller =
                crate::vm::VmController::open(&root, crate::vm::VmPolicy::default(), SystemClock)
                    .unwrap();
            let db = rusqlite::Connection::open(root.join("activity.sqlite3")).unwrap();
            db.execute_batch(
                "INSERT INTO vm_templates VALUES(zeroblob(16),1,zeroblob(32),zeroblob(32),zeroblob(32),1024);
                 INSERT INTO vm_instances(id,template,principal,expires_ms,state,dashboard_cpus,dashboard_memory)
                    VALUES(unhex('01010101010101010101010101010101'),zeroblob(16),'other',1795438800000,2,4,4096);
                 INSERT INTO vm_snapshots(vm,id,ready,dashboard_created_ms)
                    VALUES(unhex('01010101010101010101010101010101'),zeroblob(16),1,1795435200000);"
            ).unwrap();
            Arc::new(crate::vm::DashboardCatalog::open(&root).unwrap())
        });
        let app = Dashboard {
            boundary: Boundary::new("localhost:4444".into(), false),
            assets: Arc::new(Assets::load(&assets).unwrap()),
            activity: activity.clone(),
            vm_catalog,
            verifier: Arc::new(Verifier::cheap(PASSWORD)),
            sessions: Arc::new(Sessions::new()),
            clock: clock.clone(),
            capacity: Arc::new(tokio::sync::Semaphore::new(8)),
            _password_lock: None,
        }
        .router();
        Self {
            _dir: dir,
            app,
            activity,
            clock,
        }
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        origin: Option<&str>,
    ) -> axum::http::request::Builder {
        let req = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost:4444")
            .extension("127.0.0.1:1234".parse::<SocketAddr>().unwrap());
        if let Some(origin) = origin {
            req.header("origin", origin)
        } else {
            req
        }
    }

    async fn send(&self, req: axum::http::request::Builder, body: Body) -> Response {
        self.app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap()
    }

    async fn login_at(
        &self,
        peer: IpAddr,
        name: &str,
        password: &str,
        old: Option<&str>,
    ) -> Response {
        let mut req = self.request(Method::POST, "/api/v1/session", Some(ORIGIN));
        req.extensions_mut()
            .unwrap()
            .insert(SocketAddr::new(peer, 1234));
        if let Some(old) = old {
            req = req.header("cookie", old);
        }
        self.send(
            req.header("content-type", "application/json"),
            Body::from(json!({"username": name, "password": password}).to_string()),
        )
        .await
    }

    async fn login(&self, old: Option<&str>) -> (String, String) {
        let response = self
            .login_at(IpAddr::from([127, 0, 0, 1]), "owner", PASSWORD, old)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.ends_with("; Path=/; Secure; HttpOnly; SameSite=Strict"));
        let cookie = cookie.split(';').next().unwrap().to_owned();
        assert_eq!(
            cookie.strip_prefix("__Host-arx_session=").unwrap().len(),
            64
        );
        let data = body(response).await;
        assert_eq!(data["user"]["name"], "owner");
        assert!(data["csrfToken"].as_str().unwrap().len() >= 16);
        assert!(time::OffsetDateTime::parse(data["expiresAt"].as_str().unwrap(), &Rfc3339).is_ok());
        (cookie, data["csrfToken"].as_str().unwrap().to_owned())
    }

    async fn get(&self, path: &str, cookie: &str) -> Response {
        self.send(
            self.request(Method::GET, path, None)
                .header("cookie", cookie),
            Body::empty(),
        )
        .await
    }
}

async fn body(response: Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), RESPONSE_LIMIT)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn error(response: Response, status: StatusCode, code: &str) -> serde_json::Value {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let result = body(response).await;
    assert_eq!(result.as_object().unwrap().len(), 1);
    let envelope = &result["error"];
    assert_eq!(envelope.as_object().unwrap().len(), 2);
    assert_eq!(envelope["code"], code);
    assert!((1..=200).contains(&envelope["message"].as_str().unwrap().len()));
    result
}

#[tokio::test]
async fn credential_failures_are_identical() {
    let f = Fixture::new();
    let ip = IpAddr::from([127, 0, 0, 1]);
    let wrong_user = f.login_at(ip, "other", PASSWORD, None).await;
    let wrong_password = f.login_at(ip, "owner", "wrong", None).await;
    assert!(!wrong_user.headers().contains_key(header::SET_COOKIE));
    assert!(!wrong_password.headers().contains_key(header::SET_COOKIE));
    assert_eq!(
        error(wrong_user, StatusCode::UNAUTHORIZED, "unauthorized").await,
        error(wrong_password, StatusCode::UNAUTHORIZED, "unauthorized").await
    );
    f.login(None).await;
}

#[tokio::test]
async fn throttle_applies_per_client_and_globally() {
    let f = Fixture::new();
    let ip = IpAddr::from([127, 0, 0, 1]);
    for _ in 0..5 {
        assert_eq!(f.login_at(ip, "owner", "wrong", None).await.status(), 401);
    }
    let throttled = f.login_at(ip, "owner", PASSWORD, None).await;
    assert_eq!(throttled.status(), 429);
    assert_eq!(throttled.headers()[header::RETRY_AFTER], "1");
    error(throttled, StatusCode::TOO_MANY_REQUESTS, "rate_limited").await;
    f.clock.advance(Duration::from_secs(1));
    assert_eq!(f.login_at(ip, "owner", "wrong", None).await.status(), 401);
    let retry = f.login_at(ip, "owner", PASSWORD, None).await;
    assert_eq!(retry.status(), 429);
    assert_eq!(retry.headers()[header::RETRY_AFTER], "2");
    for suffix in 2..=7 {
        assert_eq!(
            f.login_at([127, 0, 0, suffix].into(), "owner", "wrong", None)
                .await
                .status(),
            401
        );
    }
    let other = f
        .login_at([127, 0, 0, 8].into(), "owner", PASSWORD, None)
        .await;
    assert_eq!(other.status(), 429);
    f.clock.advance(Duration::from_secs(1));
    assert_eq!(
        f.login_at([127, 0, 0, 8].into(), "owner", PASSWORD, None)
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn login_rotates_cookie_and_csrf_and_logout_revokes() {
    let f = Fixture::new();
    let (old, old_csrf) = f.login(None).await;
    let (new, csrf) = f.login(Some(&old)).await;
    assert_ne!(old, new);
    assert_ne!(old_csrf, csrf);
    error(
        f.get("/api/v1/session", &old).await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await;
    let recovered = body(f.get("/api/v1/session", &new).await).await;
    assert_eq!(recovered["csrfToken"], csrf);
    let logout = f
        .send(
            f.request(Method::DELETE, "/api/v1/session", Some(ORIGIN))
                .header("cookie", &new)
                .header("x-arx-csrf", &csrf),
            Body::empty(),
        )
        .await;
    assert_eq!(logout.status(), 204);
    assert!(
        logout.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .ends_with("Max-Age=0")
    );
    error(
        f.get("/api/v1/session", &new).await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await;
    error(
        f.send(
            f.request(Method::DELETE, "/api/v1/session", Some(ORIGIN))
                .header("cookie", &new)
                .header("x-arx-csrf", &csrf),
            Body::empty(),
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await;
}

#[tokio::test]
async fn expiry_is_idle_and_absolute() {
    let f = Fixture::new();
    let (idle, _) = f.login(None).await;
    f.clock.advance(Duration::from_secs(12 * 60 * 60));
    assert_eq!(f.get("/api/v1/session", &idle).await.status(), 401);
    let (absolute, _) = f.login(None).await;
    for _ in 0..15 {
        f.clock.advance(Duration::from_secs(11 * 60 * 60));
        assert_eq!(f.get("/api/v1/session", &absolute).await.status(), 200);
    }
    f.clock.advance(Duration::from_secs(3 * 60 * 60));
    assert_eq!(f.get("/api/v1/session", &absolute).await.status(), 401);
}

#[tokio::test]
async fn origin_and_csrf_are_enforced() {
    let f = Fixture::new();
    let (session, csrf) = f.login(None).await;
    for origin in [None, Some("http://wrong.invalid"), Some("null")] {
        let login = f
            .send(
                f.request(Method::POST, "/api/v1/session", origin)
                    .header("content-type", "application/json"),
                Body::from(json!({"username": "owner", "password": PASSWORD}).to_string()),
            )
            .await;
        error(login, StatusCode::FORBIDDEN, "forbidden").await;
    }
    for (method, path, origin, token) in [
        (
            Method::DELETE,
            "/api/v1/session",
            "http://wrong.invalid",
            csrf.as_str(),
        ),
        (Method::DELETE, "/api/v1/session", ORIGIN, "wrong"),
        (
            Method::POST,
            "/api/v1/hosts/example/terminal",
            ORIGIN,
            "wrong",
        ),
        (Method::PATCH, "/api/v1/unknown", ORIGIN, "wrong"),
    ] {
        let result = f
            .send(
                f.request(method, path, Some(origin))
                    .header("cookie", &session)
                    .header("x-arx-csrf", token),
                Body::empty(),
            )
            .await;
        error(result, StatusCode::FORBIDDEN, "forbidden").await;
    }
    let stream = f
        .send(
            f.request(Method::POST, "/api/v1/vms/example/console", Some(ORIGIN))
                .header("cookie", &session)
                .header("x-arx-csrf", &csrf),
            Body::empty(),
        )
        .await;
    error(stream, StatusCode::SERVICE_UNAVAILABLE, "unavailable").await;
    assert_eq!(f.get("/api/v1/session", &session).await.status(), 200);
}

#[tokio::test]
async fn rejects_untrusted_authority_and_large_login_body() {
    let f = Fixture::new();
    let foreign = f
        .send(
            axum::http::Request::builder()
                .uri("/")
                .header("host", "wrong.invalid"),
            Body::empty(),
        )
        .await;
    error(foreign, StatusCode::FORBIDDEN, "forbidden").await;
    let large = f
        .send(
            f.request(Method::POST, "/api/v1/session", Some(ORIGIN))
                .header("content-type", "application/json"),
            Body::from(vec![b'x'; BODY_LIMIT + 1]),
        )
        .await;
    error(large, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[tokio::test]
async fn headers_content_types_and_traversal() {
    let f = Fixture::new();
    for (path, status, mime) in [
        ("/", 200, "text/html; charset=utf-8"),
        ("/script.js", 200, "text/javascript; charset=utf-8"),
        ("/%2e%2e/index.html", 404, "application/json"),
        ("/../index.html", 404, "application/json"),
        ("//index.html", 404, "application/json"),
        ("/api/v1/hosts", 401, "application/json"),
    ] {
        let response = f.get(path, "").await;
        assert_eq!(response.status(), status, "{path}");
        let headers = response.headers();
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(headers["referrer-policy"], "no-referrer");
        assert_eq!(headers[header::CONTENT_TYPE], mime);
        assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
        if path == "/" {
            assert_eq!(headers[header::CONTENT_SECURITY_POLICY], CSP);
        } else {
            assert!(!headers.contains_key(header::CONTENT_SECURITY_POLICY));
        }
    }
}

#[tokio::test]
async fn owner_reads_bounds_pagination_and_no_credentials_in_activity() {
    let f = Fixture::new();
    for path in [
        "/api/v1/hosts",
        "/api/v1/vms",
        "/api/v1/activity",
        "/api/v1/session",
    ] {
        error(
            f.get(path, "").await,
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        )
        .await;
    }
    let (cookie, _) = f.login(None).await;
    for _ in 0..51 {
        let id = f.activity.start("other-agent", "list_hosts").unwrap();
        f.activity.finish(id, "succeeded").unwrap();
    }
    for path in [
        "/api/v1/hosts/%2f",
        "/api/v1/vms/.bad",
        "/api/v1/activity?limit=51",
        "/api/v1/activity?limit=0",
        "/api/v1/activity?before_id=0",
        "/api/v1/activity?tool=Not_a_tool",
        "/api/v1/activity?outcome=done",
        "/api/v1/activity?host_id=%2f",
    ] {
        error(
            f.get(path, &cookie).await,
            StatusCode::BAD_REQUEST,
            "invalid_request",
        )
        .await;
    }
    let filtered = body(f.get("/api/v1/activity", &cookie).await).await;
    assert_eq!(filtered["entries"], json!([]));
    let page = body(
        f.get(
            "/api/v1/activity?limit=50&include_reads=true&tool=list_hosts&outcome=succeeded",
            &cookie,
        )
        .await,
    )
    .await;
    let entries = page["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 50);
    assert!(
        entries
            .windows(2)
            .all(|rows| rows[0]["id"].as_i64() > rows[1]["id"].as_i64())
    );
    let cursor = page["nextBeforeId"].as_i64().unwrap();
    assert_eq!(entries.last().unwrap()["id"], cursor);
    let next = body(
        f.get(
            &format!("/api/v1/activity?include_reads=true&before_id={cursor}"),
            &cookie,
        )
        .await,
    )
    .await;
    assert_eq!(next["entries"].as_array().unwrap().len(), 1);
    assert!(next["nextBeforeId"].is_null());
    assert_eq!(entries[0]["actor"]["name"], "other-agent");
    let serialized = page.to_string();
    assert!(!serialized.contains(PASSWORD));
    assert!(!serialized.contains(cookie.split_once('=').unwrap().1));
    let hosts = body(f.get("/api/v1/hosts", &cookie).await).await;
    assert_eq!(hosts["hosts"].as_array().unwrap().len(), 1);
    let id = hosts["hosts"][0]["id"].as_str().unwrap();
    assert_eq!(
        body(f.get(&format!("/api/v1/hosts/{id}"), &cookie).await).await["id"],
        id
    );
    error(
        f.get("/api/v1/hosts/unknown", &cookie).await,
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await;
    assert_eq!(
        body(f.get("/api/v1/vms", &cookie).await).await,
        json!({"vms":[]})
    );
}

#[tokio::test]
async fn refuses_non_loopback_without_tls() {
    let settings = DashboardConfig {
        listen: "0.0.0.0:0".parse().unwrap(),
        dist: PathBuf::new(),
        tls: None,
        vm_state: None,
    };
    assert!(Listener::bind(&settings).await.is_err());
}

#[tokio::test]
async fn configured_host_certificate_serves_https() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = directory();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate_path = dir.path().join("cert.der");
    let key_path = dir.path().join("key.der");
    for (path, bytes) in [
        (&certificate_path, certificate.cert.der().as_ref()),
        (&key_path, certificate.signing_key.serialized_der()),
    ] {
        fs::write(path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    let authority = format!("localhost:{}", address.port());
    let settings = DashboardConfig {
        listen: address,
        dist: PathBuf::new(),
        tls: Some(DashboardTls {
            authority: authority.clone(),
            certificates: vec![certificate_path],
            private_key: key_path,
        }),
        vm_state: None,
    };
    let listener = Listener::bind(&settings).await.unwrap();
    let (stop, shutdown) = tokio::sync::watch::channel(false);
    let router = Router::new().fallback(|| async { "tls fixture" });
    let server = tokio::spawn(listener.run(router, shutdown));
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate.cert.der().clone()).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(client))
        .connect(
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
            tokio::net::TcpStream::connect(address).await.unwrap(),
        )
        .await
        .unwrap();
    tls.write_all(
        format!("GET / HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    let mut response = String::new();
    tls.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.ends_with("tls fixture"));
    stop.send(true).unwrap();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn mcp_and_owner_auth_do_not_cross() {
    let f = Fixture::new();
    let bearer = "a".repeat(64);
    let config = Arc::new(Config {
        agents: vec![Agent {
            principal: "agent".into(),
            token_sha256: hex::encode(Sha256::digest(bearer.as_bytes())),
        }],
        ..Default::default()
    });
    assert_eq!(config.authenticate(&bearer).as_deref(), Some("agent"));
    let mcp = crate::http::router(
        config,
        "127.0.0.1:4445".parse().unwrap(),
        f.activity.clone(),
    );
    let (cookie, _) = f.login(None).await;
    let credentialed = f
        .send(
            f.request(Method::GET, "/api/v1/hosts", None)
                .header("authorization", format!("Bearer {bearer}")),
            Body::empty(),
        )
        .await;
    error(credentialed, StatusCode::UNAUTHORIZED, "unauthorized").await;
    let result = mcp
        .oneshot(
            axum::http::Request::builder()
                .uri("/mcp")
                .header("host", "127.0.0.1:4445")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), 401);
}

#[tokio::test]
async fn vm_list_and_detail_use_the_controller_catalog() {
    let f = Fixture::with_catalog(true);
    let (cookie, _) = f.login(None).await;
    let list = body(f.get("/api/v1/vms", &cookie).await).await;
    let vms = list["vms"].as_array().unwrap();
    assert_eq!(vms.len(), 1);
    let vm = &vms[0];
    let id = vm["id"].as_str().unwrap();
    assert_eq!(vm["cpus"], 4);
    assert_eq!(vm["memoryBytes"], 4096);
    assert_eq!(vm["state"], "running");
    assert_eq!(vm["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(
        body(f.get(&format!("/api/v1/vms/{id}"), &cookie).await).await,
        *vm
    );
    error(
        f.get("/api/v1/vms/unknown", &cookie).await,
        StatusCode::NOT_FOUND,
        "not_found",
    )
    .await;
}

#[test]
fn static_snapshot_is_bounded_and_immutable() {
    let dir = directory();
    let file = dir.path().join("index.html");
    fs::write(&file, "before").unwrap();
    let snapshot = Assets::load(dir.path()).unwrap();
    fs::write(&file, "after").unwrap();
    assert_eq!(&snapshot.get("/").unwrap().content[..], b"before");
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    assert!(Assets::load(dir.path()).is_err());
    fs::write(&file, "ok").unwrap();
    for index in 0..256 {
        fs::write(dir.path().join(format!("file-{index}.js")), "").unwrap();
    }
    assert!(Assets::load(dir.path()).is_err());
}

#[cfg(unix)]
#[test]
fn static_startup_refuses_symlinked_files_and_ancestors() {
    let dir = directory();
    let dist = dir.path().join("dist");
    fs::create_dir(&dist).unwrap();
    fs::write(dist.join("index.html"), "ok").unwrap();
    let link = dir.path().join("dist-link");
    std::os::unix::fs::symlink(&dist, &link).unwrap();
    assert!(Assets::load(&link).is_err());
    std::os::unix::fs::symlink(dist.join("index.html"), dist.join("link.html")).unwrap();
    assert!(Assets::load(&dist).is_err());
}

#[cfg(windows)]
#[test]
fn static_startup_refuses_junctions() {
    use std::os::windows::process::CommandExt;
    let dir = directory();
    let dist = dir.path();
    fs::create_dir(dist.join("target")).unwrap();
    fs::write(dist.join("index.html"), "ok").unwrap();
    assert!(Assets::load(dist).is_ok());
    let cmd = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe");
    let output = std::process::Command::new(cmd)
        .current_dir(dir.path())
        .raw_arg("/d /c mklink /J link target")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(Assets::load(&dist.join("link")).is_err());
    assert!(Assets::load(dist).is_err());
    fs::remove_dir(dist.join("link")).unwrap();
}

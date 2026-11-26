use super::*;
use crate::config::{Agent, Config};
use axum::body::Body;
use sha2::{Digest, Sha256};
use std::{fs, net::IpAddr, sync::Mutex};
use tower::ServiceExt;

const PASSWORD: &str = "fake-password-sentinel-for-test";
pub(super) const ORIGIN: &str = "http://localhost:4444";
const NOW: i64 = 1_795_435_200_000;

pub(super) struct TestClock(Mutex<ClockReading>);
impl Clock for TestClock {
    fn now(&self) -> ClockReading {
        *self.0.lock().unwrap()
    }
}
impl TestClock {
    pub(super) fn advance(&self, duration: Duration) {
        let mut time = self.0.lock().unwrap();
        time.monotonic += duration;
        time.utc_ms += duration.as_millis() as i64;
    }
}

pub(super) struct Fixture {
    pub(super) _dir: tempfile::TempDir,
    pub(super) app: Router,
    pub(super) state: Dashboard,
    pub(super) activity: Arc<Activity>,
    pub(super) clock: Arc<TestClock>,
    pub(super) sessions: Arc<Sessions>,
}

fn directory() -> tempfile::TempDir {
    crate::service::application::tests::private_temp()
}

/// Creates an owner-only child directory regardless of the process umask,
/// then verifies it with the production privacy check.
pub(crate) fn private_dir(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(not(unix))]
    fs::create_dir(path).unwrap();
    #[cfg(windows)]
    set_owner_private(path);
    crate::security::private(path).unwrap();
}

/// Makes the current token user (TokenUser) the owner of a fixture object
/// with a protected owner-only DACL. Elevated runners otherwise default new
/// objects to an Administrators owner, which the private-file check refuses.
#[cfg(windows)]
pub(crate) fn set_owner_private(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
    };
    let identity = crate::local::current_identity().unwrap();
    let sddl: Vec<u16> = format!("O:{identity}D:P(A;OICI;FA;;;{identity})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut sd = ptr::null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut sd,
                ptr::null_mut()
            ),
            0
        );
        let (mut owner, mut owner_defaulted) = (ptr::null_mut(), 0);
        assert_ne!(
            GetSecurityDescriptorOwner(sd, &mut owner, &mut owner_defaulted),
            0
        );
        let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
        assert_ne!(
            GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted),
            0
        );
        let result = SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            ptr::null_mut(),
            acl,
            ptr::null_mut(),
        );
        LocalFree(sd);
        assert_eq!(result, 0, "fixture owner and DACL assignment");
    }
    crate::local::owned(path).unwrap();
    crate::security::private(path).unwrap();
}

impl Fixture {
    fn new() -> Self {
        Self::with_catalog(false)
    }

    fn with_catalog(catalog: bool) -> Self {
        Self::with_streams(catalog, streams::Backends::new(None))
    }

    pub(super) fn with_streams(catalog: bool, streams: streams::Backends) -> Self {
        Self::build(catalog, streams, read::HostSampler::native())
    }

    fn with_host(host: read::HostSampler) -> Self {
        Self::build(false, streams::Backends::new(None), host)
    }

    fn build(catalog: bool, streams: streams::Backends, host: read::HostSampler) -> Self {
        let dir = directory();
        let assets = dir.path().join("dist");
        fs::create_dir(&assets).unwrap();
        fs::write(assets.join("index.html"), "<!doctype html>").unwrap();
        fs::write(assets.join("script.js"), "export {};").unwrap();
        let activity = Arc::new(Activity::open(dir.path()).unwrap());
        let clock = Arc::new(TestClock(Mutex::new(ClockReading {
            monotonic: tokio::time::Instant::now().into_std(),
            utc_ms: NOW,
        })));
        let vm_catalog = catalog.then(|| {
            let root = dir.path().join("vm");
            private_dir(&root);
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
        let sessions = Arc::new(Sessions::new());
        let state = Dashboard {
            boundary: Boundary::new("localhost:4444".into(), false),
            assets: Arc::new(Assets::load(&assets).unwrap()),
            activity: activity.clone(),
            vm_catalog,
            verifier: Arc::new(Verifier::cheap(PASSWORD)),
            sessions: sessions.clone(),
            clock: clock.clone(),
            capacity: Arc::new(tokio::sync::Semaphore::new(8)),
            streams,
            host,
            _password_lock: None,
        };
        let app = state.clone().router();
        Self {
            _dir: dir,
            app,
            state,
            activity,
            clock,
            sessions,
        }
    }

    pub(super) fn request(
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

    pub(super) async fn send(&self, req: axum::http::request::Builder, body: Body) -> Response {
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

    pub(super) async fn login(&self, old: Option<&str>) -> (String, String) {
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

pub(super) async fn body(response: Response) -> serde_json::Value {
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
    error(stream, StatusCode::NOT_FOUND, "not_found").await;
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
    let supported = host_supported();
    if supported {
        bounded("native host sample", f.state.host.settled()).await;
        let hosts = body(f.get("/api/v1/hosts", &cookie).await).await;
        assert_eq!(hosts["hosts"].as_array().unwrap().len(), 1);
        let id = hosts["hosts"][0]["id"].as_str().unwrap();
        assert_eq!(
            body(f.get(&format!("/api/v1/hosts/{id}"), &cookie).await).await["id"],
            id
        );
    } else {
        let id = f.activity.host_id().unwrap();
        for path in ["/api/v1/hosts".to_owned(), format!("/api/v1/hosts/{id}")] {
            error(
                f.get(&path, &cookie).await,
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
            )
            .await;
        }
    }
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

#[test]
fn only_a_dashboard_only_config_may_omit_agents() {
    let dir = directory();
    let config = |dashboard: bool, listen: bool| Config {
        state_dir: dir.path().into(),
        listen: listen.then(|| "127.0.0.1:4445".parse().unwrap()),
        dashboard: dashboard.then(|| DashboardConfig {
            listen: "127.0.0.1:4444".parse().unwrap(),
            dist: PathBuf::new(),
            tls: None,
            vm_state: None,
        }),
        ..Default::default()
    };
    assert!(config(true, false).validate().is_ok());
    // An MCP listener still requires at least one agent credential.
    assert!(config(true, true).validate().is_err());
    assert!(config(false, true).validate().is_err());
    assert!(config(false, false).validate().is_err());
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

#[test]
fn https_authority_uses_the_browser_serialization() {
    let listen = |port: u16| SocketAddr::from(([127, 0, 0, 1], port));
    // Expected values are the WHATWG URL `host` of `https://<raw>/`, taken
    // from Node's URL implementation; the configured host spelling must
    // already equal it, and only a default or zero-padded port is rewritten.
    let accepted = [
        ("owner.example", 443, "owner.example"),
        ("owner.example:443", 443, "owner.example"),
        ("owner.example:0443", 443, "owner.example"),
        ("owner.example:8443", 8443, "owner.example:8443"),
        ("owner.example:08443", 8443, "owner.example:8443"),
        ("owner.example.:443", 443, "owner.example."),
        ("1e.example", 443, "1e.example"),
        ("127.0.0.1:8443", 8443, "127.0.0.1:8443"),
        ("[::1]:443", 443, "[::1]"),
        ("[::1]:8443", 8443, "[::1]:8443"),
        ("[::]", 443, "[::]"),
        ("[1::]", 443, "[1::]"),
        ("[::ffff:c000:201]:8443", 8443, "[::ffff:c000:201]:8443"),
        ("[2001:db8::1:0:0:1]", 443, "[2001:db8::1:0:0:1]"),
        ("[1:0:0:2::3]", 443, "[1:0:0:2::3]"),
    ];
    for (raw, port, browser) in accepted {
        assert_eq!(
            transport::canonical_authority(raw, listen(port)).unwrap(),
            browser,
            "{raw}"
        );
    }
    for (raw, port) in [
        // Noncanonical host spellings a browser would rewrite.
        ("[0:0:0:0:0:0:0:1]", 443),
        ("[0::1]", 443),
        ("[::ffff:192.0.2.1]:8443", 8443),
        ("[2001:0db8::1]", 443),
        ("[2001:db8:0:0:1:0:0:1]", 443),
        ("[1:0:0:2:0:0:0:3]", 443),
        ("127.1", 443),
        ("127.0.0.1.", 443),
        ("01.2.3.4", 443),
        ("0x7f.0.0.1", 443),
        ("4294967295", 443),
        ("a..b", 443),
        (".a", 443),
        // Hosts a browser refuses.
        ("[:::1]", 443),
        ("[1:2:3:4:5:6:7:8:9]", 443),
        ("[::1", 443),
        ("1.2.3.256", 443),
        ("a.1", 443),
        ("a.0x1", 443),
        ("[fe80::1%25eth0]", 443),
        // Spelling, userinfo and port errors.
        ("Owner.Example", 443),
        ("[::FFFF:c000:201]", 443),
        ("owner.example:", 443),
        ("owner.example:8443", 443),
        ("owner.example", 8443),
        ("owner.example:99999", 443),
        ("owner.example:+443", 443),
        ("owner.example:443/", 443),
        ("owner@owner.example", 443),
        (":443", 443),
        ("", 443),
    ] {
        assert!(
            transport::canonical_authority(raw, listen(port)).is_err(),
            "{raw}"
        );
    }

    // Every accepted form passes the boundary with the browser's Host and
    // Origin, and refuses the configured spelling when it differs.
    let check = |boundary: &Boundary, host: &str, origin: &str| {
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/api/v1/session")
            .header("host", host)
            .header("origin", origin)
            .body(Body::empty())
            .unwrap();
        boundary.check(&request, true).is_ok()
    };
    for (raw, port, browser) in accepted {
        let boundary = Boundary::new(
            transport::canonical_authority(raw, listen(port)).unwrap(),
            true,
        );
        assert!(
            check(&boundary, browser, &format!("https://{browser}")),
            "{raw}"
        );
        assert!(
            !check(&boundary, browser, &format!("http://{browser}")),
            "{raw}"
        );
        if raw != browser {
            assert!(
                !check(&boundary, browser, &format!("https://{raw}")),
                "{raw}"
            );
        }
    }
}

#[tokio::test]
async fn configured_host_certificate_serves_https() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = directory();
    #[cfg(windows)]
    set_owner_private(dir.path());
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
        #[cfg(windows)]
        set_owner_private(path);
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

#[tokio::test]
async fn dashboard_vm_catalog_shows_ready_test_snapshots_and_hides_pending_deleting_and_static() {
    let dir = directory();
    let assets = dir.path().join("dist");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("index.html"), "<!doctype html>").unwrap();
    fs::write(assets.join("script.js"), "export {};").unwrap();
    let activity = Arc::new(Activity::open(dir.path()).unwrap());
    let clock = Arc::new(TestClock(Mutex::new(ClockReading {
        monotonic: tokio::time::Instant::now().into_std(),
        utc_ms: NOW,
    })));
    let root = dir.path().join("vm");
    private_dir(&root);
    let _controller =
        crate::vm::VmController::open(&root, crate::vm::VmPolicy::default(), SystemClock).unwrap();
    let db = rusqlite::Connection::open(root.join("activity.sqlite3")).unwrap();
    db.execute_batch(
        "INSERT INTO vm_templates VALUES(zeroblob(16),1,zeroblob(32),zeroblob(32),zeroblob(32),1024);
         INSERT INTO vm_instances(id,template,principal,expires_ms,state,dashboard_cpus,dashboard_memory)
            VALUES(unhex('01010101010101010101010101010101'),zeroblob(16),'alice',1795438800000,1,4,4096);
         INSERT INTO vm_snapshots(vm,id,ready,dashboard_created_ms)
            VALUES(unhex('01010101010101010101010101010101'),unhex('00000000000000000000000000000001'),1,1795435200000);
         INSERT INTO vm_generations(id,vm_id,generation,active)
            VALUES
              (unhex('10000000000000000000000000000001'),unhex('01010101010101010101010101010101'),1,1),
              (unhex('10000000000000000000000000000002'),unhex('01010101010101010101010101010101'),2,0),
              (unhex('10000000000000000000000000000003'),unhex('01010101010101010101010101010101'),3,0),
              (unhex('10000000000000000000000000000004'),unhex('01010101010101010101010101010101'),4,0),
              (unhex('10000000000000000000000000000005'),unhex('01010101010101010101010101010101'),5,0);
         INSERT INTO vm_restore_points(id,vm,static_vm,host,name,captured_ms,state,provenance,generation,lineage,charge)
            SELECT unhex('00000000000000000000000000000002'),unhex('01010101010101010101010101010101'),NULL,
                id,'ready-point',1795435300000,'ready',unhex('10000000000000000000000000000001'),
                unhex('10000000000000000000000000000002'),zeroblob(32),8192
            FROM vm_state_host WHERE singleton=1;
         INSERT INTO vm_restore_points(id,vm,static_vm,host,name,captured_ms,state,provenance,generation,lineage,charge)
            SELECT unhex('00000000000000000000000000000003'),unhex('01010101010101010101010101010101'),NULL,
                id,'copying-point',1795435400000,'copying',unhex('10000000000000000000000000000001'),
                unhex('10000000000000000000000000000003'),zeroblob(32),8192
            FROM vm_state_host WHERE singleton=1;
         INSERT INTO vm_restore_points(id,vm,static_vm,host,name,captured_ms,state,provenance,generation,lineage,charge)
            SELECT unhex('00000000000000000000000000000004'),unhex('01010101010101010101010101010101'),NULL,
                id,'deleting-point',1795435500000,'deleting',unhex('10000000000000000000000000000001'),
                unhex('10000000000000000000000000000004'),zeroblob(32),8192
            FROM vm_state_host WHERE singleton=1;
         INSERT INTO vm_restore_points(id,vm,static_vm,host,name,captured_ms,state,provenance,generation,lineage,charge)
            SELECT unhex('00000000000000000000000000000005'),unhex('01010101010101010101010101010101'),NULL,
                id,'quarantined-point',1795435600000,'quarantined',unhex('10000000000000000000000000000001'),
                unhex('10000000000000000000000000000005'),zeroblob(32),8192
            FROM vm_state_host WHERE singleton=1;
         INSERT INTO vm_static_instances(id,name,principal,host,template,source_generation,
                recipe_digest,disk_digest,vars_digest,state,persistent,staging,origin_generation)
            SELECT unhex('02020202020202020202020202020202'),'web-site','alice',id,zeroblob(16),
                unhex('20000000000000000000000000000001'),zeroblob(32),zeroblob(32),zeroblob(32),1,4096,0,
                unhex('20000000000000000000000000000001')
            FROM vm_state_host WHERE singleton=1;
         INSERT INTO vm_generations(id,static_id,generation,active)
            VALUES
              (unhex('20000000000000000000000000000001'),unhex('02020202020202020202020202020202'),1,1),
              (unhex('20000000000000000000000000000002'),unhex('02020202020202020202020202020202'),2,0);
         INSERT INTO vm_restore_points(id,vm,static_vm,host,name,captured_ms,state,provenance,generation,lineage,charge)
            SELECT unhex('00000000000000000000000000000006'),NULL,unhex('02020202020202020202020202020202'),
                id,'static-ready',1795435700000,'ready',unhex('20000000000000000000000000000001'),
                unhex('20000000000000000000000000000002'),zeroblob(32),8192
            FROM vm_state_host WHERE singleton=1;",
    )
    .unwrap();
    drop(db);
    let vm_catalog = Some(Arc::new(crate::vm::DashboardCatalog::open(&root).unwrap()));
    let sessions = Arc::new(Sessions::new());
    let state = Dashboard {
        boundary: Boundary::new("localhost:4444".into(), false),
        assets: Arc::new(Assets::load(&assets).unwrap()),
        activity: activity.clone(),
        vm_catalog,
        verifier: Arc::new(Verifier::cheap(PASSWORD)),
        sessions: sessions.clone(),
        clock: clock.clone(),
        capacity: Arc::new(tokio::sync::Semaphore::new(8)),
        streams: streams::Backends::new(None),
        host: read::HostSampler::native(),
        _password_lock: None,
    };
    let f = Fixture {
        _dir: dir,
        app: state.clone().router(),
        state,
        activity,
        clock,
        sessions,
    };
    let (cookie, _) = f.login(None).await;
    let list = body(f.get("/api/v1/vms", &cookie).await).await;
    let vms = list["vms"].as_array().unwrap();
    assert_eq!(vms.len(), 1);
    let snapshots = vms[0]["snapshots"].as_array().unwrap();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0]["id"], "00000000000000000000000000000001");
    assert_eq!(snapshots[1]["id"], "00000000000000000000000000000002");
    assert_eq!(snapshots[1]["sizeBytes"], 8192);
    let id = vms[0]["id"].as_str().unwrap();
    assert_eq!(
        body(f.get(&format!("/api/v1/vms/{id}"), &cookie).await).await,
        vms[0]
    );
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
    // Separate components: a forward slash in the image path makes cmd.exe
    // parse its own command line as a switch and run `md` instead.
    let cmd = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("cmd.exe");
    let output = std::process::Command::new(cmd)
        .current_dir(dir.path())
        .raw_arg("/d /c mklink /J link target")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "mklink fixture failed: {}; stderr: {}; stdout: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(Assets::load(&dist.join("link")).is_err());
    assert!(Assets::load(dist).is_err());
    fs::remove_dir(dist.join("link")).unwrap();
}

/// Scripted native boundary for the host sampler. Every refresh reports its
/// number on `entered`. Refresh `gate_at` then blocks until `release` is sent
/// or dropped (bounded by `HOST_BOUND`), and refresh `panic_at` panics. Each
/// requested CPU sample takes the next scripted value.
struct GatedSource {
    cpu: std::vec::IntoIter<Option<f32>>,
    calls: usize,
    gate_at: usize,
    panic_at: usize,
    entered: tokio::sync::mpsc::UnboundedSender<usize>,
    release: Option<std::sync::mpsc::Receiver<()>>,
    finished: Arc<std::sync::atomic::AtomicUsize>,
    dropped: Option<tokio::sync::oneshot::Sender<()>>,
}

impl GatedSource {
    fn new(
        cpu: impl IntoIterator<Item = Option<f32>>,
        gate_at: usize,
        release: Option<std::sync::mpsc::Receiver<()>>,
    ) -> (Self, tokio::sync::mpsc::UnboundedReceiver<usize>) {
        let (entered, calls) = tokio::sync::mpsc::unbounded_channel();
        let source = Self {
            cpu: cpu.into_iter().collect::<Vec<_>>().into_iter(),
            calls: 0,
            gate_at,
            panic_at: usize::MAX,
            entered,
            release,
            finished: Arc::default(),
            dropped: None,
        };
        (source, calls)
    }

    fn finished(&self) -> Arc<std::sync::atomic::AtomicUsize> {
        self.finished.clone()
    }
}

impl read::HostSource for GatedSource {
    fn refresh(&mut self, sample_cpu: bool) -> read::HostReading {
        let call = self.calls;
        self.calls += 1;
        let _ = self.entered.send(call);
        if call == self.gate_at
            && let Some(release) = self.release.take()
        {
            let _ = release.recv_timeout(HOST_BOUND);
        }
        assert_ne!(call, self.panic_at, "injected host refresh panic");
        let cpu = if sample_cpu {
            self.cpu.next().flatten()
        } else {
            None
        };
        self.finished
            .store(call + 1, std::sync::atomic::Ordering::SeqCst);
        read::HostReading {
            model: "Test CPU".into(),
            cores: 4,
            total_memory: 8 << 30,
            used_memory: (call as u64 + 1) << 30,
            cpu,
        }
    }
}

impl Drop for GatedSource {
    fn drop(&mut self) {
        if let Some(dropped) = self.dropped.take() {
            let _ = dropped.send(());
        }
    }
}

/// Failure bound only; the passing path never waits on it.
const HOST_BOUND: Duration = Duration::from_secs(10);

async fn bounded<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(HOST_BOUND, future)
        .await
        .unwrap_or_else(|_| panic!("{what} did not finish within the bound"))
}

/// The v1 host contract covers Windows and Linux on x86_64 and arm64; the
/// reader answers other platforms (macOS CI) with the unavailable envelope.
fn host_supported() -> bool {
    matches!(std::env::consts::OS, "windows" | "linux")
        && matches!(std::env::consts::ARCH, "x86_64" | "aarch64")
}

/// Checks a host route response under the platform contract. Returns the
/// host object where the route is supported, `None` after asserting the
/// unavailable envelope elsewhere.
async fn host_contract(response: Response) -> Option<serde_json::Value> {
    if !host_supported() {
        error(response, StatusCode::SERVICE_UNAVAILABLE, "unavailable").await;
        return None;
    }
    assert_eq!(response.status(), StatusCode::OK);
    let data = body(response).await;
    Some(match data.get("hosts") {
        Some(hosts) => {
            assert_eq!(hosts.as_array().unwrap().len(), 1);
            hosts[0].clone()
        }
        None => data,
    })
}

/// A sampler read on the blocking pool, as a host request makes it, so a
/// read that blocks cannot stall the test runtime.
async fn sampler_read(sampler: &read::HostSampler) -> Result<read::HostFacts, StatusCode> {
    let sampler = sampler.clone();
    tokio::task::spawn_blocking(move || sampler.latest().map_err(|error| error.status))
        .await
        .unwrap()
}

#[tokio::test]
async fn host_reads_never_wait_for_an_in_flight_refresh() {
    let (release, gate) = std::sync::mpsc::channel();
    let (source, mut entered) = GatedSource::new([Some(4.0), Some(37.44)], 1, Some(gate));
    let finished = source.finished();
    let sampler = read::HostSampler::start(source, Duration::ZERO);
    let f = Fixture::with_host(sampler.clone());
    let (cookie, _) = f.login(None).await;
    let list = "/api/v1/hosts".to_owned();
    let detail = format!("/api/v1/hosts/{}", f.activity.host_id().unwrap());
    // The first sample opens the CPU window: usage stays null.
    assert_eq!(bounded("first sample", entered.recv()).await, Some(0));
    bounded("first publish", sampler.settled()).await;
    let first = sampler_read(&sampler).await.unwrap();
    assert_eq!((first.used_memory, first.used_percent), (1 << 30, None));
    if let Some(host) = host_contract(bounded("first read", f.get(&list, &cookie)).await).await {
        assert!(host["cpu"]["usedPercent"].is_null());
        assert_eq!(host["cpu"]["model"], "Test CPU");
        assert_eq!(host["memory"]["usedBytes"], 1u64 << 30);
    }
    // The next refresh is now in flight and held at the gate.
    assert_eq!(bounded("gated refresh", entered.recv()).await, Some(1));
    let (a, b, list_response, detail_response) =
        bounded("concurrent host reads during a refresh", async {
            tokio::join!(
                sampler_read(&sampler),
                sampler_read(&sampler),
                f.get(&list, &cookie),
                f.get(&detail, &cookie)
            )
        })
        .await;
    // Every read finished while the refresh was still held, so none waited.
    assert_eq!(finished.load(std::sync::atomic::Ordering::SeqCst), 1);
    for facts in [a.unwrap(), b.unwrap()] {
        assert_eq!((facts.used_memory, facts.used_percent), (1 << 30, None));
    }
    for response in [list_response, detail_response] {
        if let Some(host) = host_contract(response).await {
            assert!(host["cpu"]["usedPercent"].is_null());
            assert_eq!(host["memory"]["usedBytes"], 1u64 << 30);
        }
    }
    // Releasing the gate publishes the second sample, rounded.
    release.send(()).unwrap();
    assert_eq!(bounded("next refresh", entered.recv()).await, Some(2));
    assert_eq!(
        sampler_read(&sampler).await.unwrap().used_percent,
        Some(37.4)
    );
    if let Some(host) = host_contract(bounded("ready read", f.get(&detail, &cookie)).await).await {
        assert_eq!(host["cpu"]["usedPercent"], json!(37.4));
    }
}

#[tokio::test]
async fn a_stalled_first_sample_does_not_delay_serving() {
    let (release, gate) = std::sync::mpsc::channel();
    let (source, mut entered) = GatedSource::new([Some(4.0)], 0, Some(gate));
    let finished = source.finished();
    // Creating the sampler and the dashboard returns while the first refresh is held.
    let sampler = read::HostSampler::start(source, Duration::ZERO);
    let f = Fixture::with_host(sampler.clone());
    let (cookie, _) = f.login(None).await;
    assert_eq!(bounded("first sample", entered.recv()).await, Some(0));
    assert_eq!(finished.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(
        sampler_read(&sampler).await.err(),
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    // Not yet sampled: hosts answer unavailable on every platform, and the
    // rest of the dashboard serves.
    error(
        bounded("pending host read", f.get("/api/v1/hosts", &cookie)).await,
        StatusCode::SERVICE_UNAVAILABLE,
        "unavailable",
    )
    .await;
    assert_eq!(
        body(bounded("vm read", f.get("/api/v1/vms", &cookie)).await).await,
        json!({"vms": []})
    );
    assert_eq!(finished.load(std::sync::atomic::Ordering::SeqCst), 0);
    release.send(()).unwrap();
    bounded("first publish", sampler.settled()).await;
    assert_eq!(sampler_read(&sampler).await.unwrap().used_memory, 1 << 30);
    if let Some(host) =
        host_contract(bounded("sampled read", f.get("/api/v1/hosts", &cookie)).await).await
    {
        assert!(host["cpu"]["usedPercent"].is_null());
    }
}

#[tokio::test]
async fn a_panicking_refresh_marks_hosts_unavailable_without_stopping_the_dashboard() {
    // Panic in the first sample, then in a later one after figures were published.
    for panic_at in [0, 1] {
        let (mut source, mut entered) = GatedSource::new([Some(4.0)], usize::MAX, None);
        source.panic_at = panic_at;
        let sampler = read::HostSampler::start(source, Duration::ZERO);
        let f = Fixture::with_host(sampler.clone());
        let (cookie, _) = f.login(None).await;
        for call in 0..=panic_at {
            assert_eq!(bounded("refresh", entered.recv()).await, Some(call));
        }
        bounded("sampler failure", sampler.failed()).await;
        // The sampler stopped: no refresh follows the panic.
        bounded("refresh channel close", async {
            while let Some(call) = entered.recv().await {
                panic!("refresh {call} started after the panic");
            }
        })
        .await;
        assert_eq!(
            sampler_read(&sampler).await.err(),
            Some(StatusCode::SERVICE_UNAVAILABLE)
        );
        error(
            bounded("failed host read", f.get("/api/v1/hosts", &cookie)).await,
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
        )
        .await;
        assert_eq!(
            body(bounded("vm read", f.get("/api/v1/vms", &cookie)).await).await,
            json!({"vms": []})
        );
    }
}

#[tokio::test]
async fn host_sampler_stops_when_the_dashboard_is_dropped_during_a_refresh() {
    let (release, gate) = std::sync::mpsc::channel();
    let (mut source, mut entered) = GatedSource::new([], 1, Some(gate));
    let (dropped, stopped) = tokio::sync::oneshot::channel();
    source.dropped = Some(dropped);
    let f = Fixture::with_host(read::HostSampler::start(source, Duration::ZERO));
    // Drop the dashboard while a background refresh is held at the gate.
    assert_eq!(bounded("first sample", entered.recv()).await, Some(0));
    assert_eq!(bounded("gated refresh", entered.recv()).await, Some(1));
    drop(f);
    // Tokio cannot abort a running blocking closure; once it returns, the
    // source is freed and no further refresh starts.
    release.send(()).unwrap();
    bounded("sampler shutdown", stopped)
        .await
        .expect("the sampler dropped its source");
    bounded("refresh channel close", async {
        while let Some(call) = entered.recv().await {
            panic!("refresh {call} started after shutdown");
        }
    })
    .await;
}

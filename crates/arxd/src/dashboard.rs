mod assets;
mod cpu_history;
mod password;
mod read;
mod session;
mod streams;
mod transport;

pub use password::set_password;
pub use transport::Listener;

use self::{assets::Assets, password::Verifier, session::Sessions};
use crate::{
    store::Activity,
    vm::{Clock, ClockReading},
};
use anyhow::ensure;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::File,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};
use time::format_description::well_known::Rfc3339;
use zeroize::Zeroizing;

const COOKIE: &str = "__Host-arx_session";
const COOKIE_FLAGS: &str = "Path=/; Secure; HttpOnly; SameSite=Strict";
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' blob: data:; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; object-src 'none'";
const BODY_LIMIT: usize = 16 * 1024;
const RESPONSE_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardConfig {
    pub listen: SocketAddr,
    pub dist: PathBuf,
    pub tls: Option<DashboardTls>,
    pub vm_state: Option<PathBuf>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardTls {
    pub authority: String,
    pub certificates: Vec<PathBuf>,
    pub private_key: PathBuf,
}

impl DashboardConfig {
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        if let Some(tls) = &self.tls {
            transport::canonical_authority(&tls.authority, self.listen)?;
            ensure!(
                (1..=4).contains(&tls.certificates.len()),
                "dashboard certificate count out of bounds"
            );
        } else {
            ensure!(
                self.listen.ip().is_loopback(),
                "plain dashboard listener must be loopback"
            );
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Boundary {
    authority: String,
    origin: String,
}

impl Boundary {
    fn new(authority: String, tls: bool) -> Self {
        Self {
            origin: format!("{}://{authority}", if tls { "https" } else { "http" }),
            authority,
        }
    }

    fn check(&self, request: &Request, unsafe_method: bool) -> Result<(), ApiError> {
        let headers = request.headers();
        if request.uri().authority().is_some()
            || one(headers, "host") != Some(self.authority.as_str())
            || ((unsafe_method || headers.contains_key("origin"))
                && one(headers, "origin") != Some(self.origin.as_str()))
        {
            return Err(ApiError::FORBIDDEN);
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Dashboard {
    boundary: Boundary,
    assets: Arc<Assets>,
    activity: Arc<Activity>,
    vm_catalog: Option<Arc<crate::vm::DashboardCatalog>>,
    verifier: Arc<Verifier>,
    sessions: Arc<Sessions>,
    clock: Arc<dyn Clock>,
    capacity: Arc<tokio::sync::Semaphore>,
    streams: streams::Backends,
    host: read::HostSampler,
    _password_lock: Option<Arc<File>>,
}

impl Dashboard {
    fn router(self) -> Router {
        Router::new().fallback(dispatch).with_state(self)
    }
}

impl Listener {
    pub fn router(
        &self,
        config: &DashboardConfig,
        activity: Arc<Activity>,
        state_dir: &std::path::Path,
        owner_executor: Option<Arc<crate::development::Execution>>,
    ) -> anyhow::Result<Router> {
        let (verifier, lock) = Verifier::load(state_dir)?;
        let vm_catalog = config
            .vm_state
            .as_deref()
            .map(crate::vm::DashboardCatalog::open)
            .transpose()?;
        Ok(Dashboard {
            boundary: self.boundary.clone(),
            assets: Arc::new(Assets::load(&config.dist)?),
            activity,
            vm_catalog: vm_catalog.map(Arc::new),
            verifier: Arc::new(verifier),
            sessions: self.sessions.clone(),
            clock: Arc::new(SystemClock),
            capacity: Arc::new(tokio::sync::Semaphore::new(8)),
            streams: streams::Backends::new(owner_executor),
            host: read::HostSampler::native(),
            _password_lock: Some(Arc::new(lock)),
        }
        .router())
    }
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            monotonic: tokio::time::Instant::now().into_std(),
            utc_ms: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .ok()
                .and_then(|duration| i64::try_from(duration.as_millis()).ok())
                .unwrap_or(0),
        }
    }
}

fn instant_string(ms: i64) -> Result<String, ApiError> {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .ok()
        .and_then(|time| time.format(&Rfc3339).ok())
        .ok_or(ApiError::UNAVAILABLE)
}

fn random_token() -> anyhow::Result<String> {
    let mut secret = Zeroizing::new([0; 32]);
    getrandom::fill(&mut *secret).map_err(|_| anyhow::anyhow!("randomness unavailable"))?;
    Ok(hex::encode(*secret))
}

#[derive(Clone, Copy)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retry: Option<Duration>,
}

impl ApiError {
    const INVALID: Self = Self::new(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "Invalid dashboard request.",
    );
    const UNAUTHORIZED: Self = Self::new(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "Sign in to continue.",
    );
    const BAD_CREDENTIALS: Self = Self::new(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "Invalid username or password.",
    );
    const FORBIDDEN: Self = Self::new(
        StatusCode::FORBIDDEN,
        "forbidden",
        "The dashboard refused the request.",
    );
    const NOT_FOUND: Self = Self::new(StatusCode::NOT_FOUND, "not_found", "Not found.");
    const UNAVAILABLE: Self = Self::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "unavailable",
        "The dashboard cannot serve this request now.",
    );

    const fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
            retry: None,
        }
    }

    fn rate_limit(retry: Duration) -> Self {
        Self {
            retry: Some(retry),
            ..Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many attempts. Wait and try again.",
            )
        }
    }

    fn response(self) -> Response {
        let mut response = (
            self.status,
            axum::Json(json!({"error": {"code": self.code, "message": self.message}})),
        )
            .into_response();
        if let Some(wait) = self.retry {
            let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}

fn json_response(data: impl Serialize) -> Result<Response, ApiError> {
    let bytes = serde_json::to_vec(&data).map_err(|_| ApiError::UNAVAILABLE)?;
    if bytes.len() > RESPONSE_LIMIT {
        return Err(ApiError::UNAVAILABLE);
    }
    Ok(([(header::CONTENT_TYPE, "application/json")], bytes).into_response())
}

fn one<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut headers = headers.get_all(name).iter();
    let value = headers.next()?.to_str().ok()?;
    headers.next().is_none().then_some(value)
}

fn request_key(headers: &HeaderMap) -> Option<[u8; 32]> {
    let mut values = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(name, _)| *name == COOKIE);
    let (_, value) = values.next()?;
    if values.next().is_some() {
        return None;
    }
    session::cookie_key(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: String,
}

async fn dispatch(State(state): State<Dashboard>, request: Request) -> Response {
    let head = request.method() == Method::HEAD;
    let mut response = route(state, request)
        .await
        .unwrap_or_else(ApiError::response);
    if head {
        *response.body_mut() = Body::empty();
    }
    // Keep B1's one request per connection without having Hyper append
    // "close" to a WebSocket's Connection: upgrade handshake.
    let upgrading = response.status() == StatusCode::SWITCHING_PROTOCOLS;
    let headers = response.headers_mut();
    if !upgrading {
        headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if headers.get(header::CONTENT_TYPE)
        == Some(&HeaderValue::from_static("text/html; charset=utf-8"))
    {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        );
    }
    response
}

async fn route(state: Dashboard, request: Request) -> Result<Response, ApiError> {
    let _permit = state
        .capacity
        .try_acquire()
        .map_err(|_| ApiError::UNAVAILABLE)?;
    let method = request.method().clone();
    let unsafe_method = !matches!(method, Method::GET | Method::HEAD);
    state.boundary.check(&request, unsafe_method)?;
    let path = request.uri().path();
    let login = method == Method::POST && path == "/api/v1/session";
    let logout = method == Method::DELETE && path == "/api/v1/session";
    let key = request_key(request.headers());
    if login {
        return login_request(&state, request, key).await;
    }
    if unsafe_method && !logout {
        state.sessions.authenticate(
            key,
            state.clock.now(),
            Some(one(request.headers(), "x-arx-csrf")),
            false,
        )?;
        if method == Method::POST {
            return streams::mint(&state, request, key).await;
        }
        return Err(ApiError::NOT_FOUND);
    }
    if logout {
        if request.headers().contains_key(header::TRANSFER_ENCODING)
            || one(request.headers(), "content-length").is_some_and(|length| length != "0")
        {
            return Err(ApiError::INVALID);
        }
        state.sessions.authenticate(
            key,
            state.clock.now(),
            Some(one(request.headers(), "x-arx-csrf")),
            true,
        )?;
        let mut response = StatusCode::NO_CONTENT.into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_static(
                "__Host-arx_session=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0",
            ),
        );
        return Ok(response);
    }
    if path == "/api" || path.starts_with("/api/") {
        if let Some(id) = path.strip_prefix("/api/v1/streams/") {
            if method != Method::GET {
                return Err(ApiError::NOT_FOUND);
            }
            let id = id.to_owned();
            return streams::upgrade(state.clone(), request, key, id).await;
        }
        let session = state
            .sessions
            .authenticate(key, state.clock.now(), None, false)?;
        if path == "/api/v1/session" {
            return json_response(session);
        }
        return read::route(&state, request.uri()).await;
    }
    let asset = state.assets.get(path).ok_or(ApiError::NOT_FOUND)?;
    let mut response =
        ([(header::CONTENT_TYPE, asset.mime)], asset.content.clone()).into_response();
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from(asset.content.len()),
    );
    Ok(response)
}

async fn login_request(
    state: &Dashboard,
    request: Request,
    key: Option<[u8; 32]>,
) -> Result<Response, ApiError> {
    let peer = request
        .extensions()
        .get::<SocketAddr>()
        .ok_or(ApiError::UNAVAILABLE)?
        .ip();
    let media = one(request.headers(), "content-type").and_then(|value| value.split(';').next());
    if !media.is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json")) {
        return Err(ApiError::INVALID);
    }
    let body = tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), BODY_LIMIT),
    )
    .await
    .map_err(|_| ApiError::INVALID)?
    .map_err(|_| ApiError::INVALID)?;
    let Credentials { username, password } =
        serde_json::from_slice(&body).map_err(|_| ApiError::INVALID)?;
    let password = Zeroizing::new(password);
    if password.chars().count() > 1024 {
        return Err(ApiError::INVALID);
    }
    let (token, session) = state
        .sessions
        .clone()
        .login(
            state.verifier.clone(),
            state.clock.clone(),
            peer,
            key,
            username,
            password,
        )
        .await?;
    let mut response = json_response(session)?;
    let mut cookie = HeaderValue::from_str(&format!("{COOKIE}={token}; {COOKIE_FLAGS}"))
        .map_err(|_| ApiError::UNAVAILABLE)?;
    cookie.set_sensitive(true);
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    Ok(response)
}

#[cfg(test)]
pub(crate) mod tests;

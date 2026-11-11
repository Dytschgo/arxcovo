use crate::{
    config::Config,
    mcp::{Inventory, Principal},
    store::Activity,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
const BODY_LIMIT: usize = 8192;
#[derive(Clone)]
struct Boundary {
    config: Arc<Config>,
    authority: String,
    capacity: Arc<tokio::sync::Semaphore>,
}
async fn fence(State(state): State<Boundary>, mut req: Request, next: Next) -> Response {
    let Ok(_permit) = state.capacity.try_acquire() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let headers = req.headers();
    if headers.get_all("host").iter().count() != 1
        || headers.get("host").and_then(|v| v.to_str().ok()) != Some(state.authority.as_str())
        || headers.contains_key("origin")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if headers.get_all("authorization").iter().count() != 1 {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let principal = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|t| state.config.authenticate(t));
    let Some(principal) = principal else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    req.headers_mut().remove("authorization");
    let (mut parts, body) = req.into_parts();
    let bytes = match tokio::time::timeout(Duration::from_secs(5), to_bytes(body, BODY_LIMIT)).await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    parts.extensions.insert(Principal(principal));
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}
/// Bind only after validated explicit configuration. No default external listener.
pub async fn bind(config: Arc<Config>) -> anyhow::Result<TcpListener> {
    config.validate()?;
    let address = config
        .listen
        .ok_or_else(|| anyhow::anyhow!("HTTP disabled: explicit loopback listener required"))?;
    Ok(TcpListener::bind(address).await?)
}
pub fn router(
    config: Arc<Config>,
    address: std::net::SocketAddr,
    activity: Arc<Activity>,
) -> Router {
    let mut sdk = StreamableHttpServerConfig::default();
    sdk.legacy_session_mode = false;
    sdk.json_response = true;
    sdk.max_request_body_bytes = BODY_LIMIT;
    let service = StreamableHttpService::new(
        move || {
            Ok(Inventory {
                activity: activity.clone(),
                principal: None,
            })
        },
        Arc::new(LocalSessionManager::default()),
        sdk,
    );
    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(
            Boundary {
                config,
                authority: address.to_string(),
                capacity: Arc::new(tokio::sync::Semaphore::new(8)),
            },
            fence,
        ))
}

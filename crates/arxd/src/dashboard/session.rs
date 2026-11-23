use super::{ApiError, instant_string, password::Verifier, random_token};
use crate::vm::ClockReading;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const IDLE: Duration = Duration::from_secs(12 * 60 * 60);
const ABSOLUTE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const FORGET_AFTER: Duration = Duration::from_secs(15 * 60);
const MAX_CLIENTS: usize = 256;
const MAX_SESSIONS: usize = 256;

type Key = [u8; 32];

pub(super) fn cookie_key(text: &str) -> Option<Key> {
    if text.len() != 64 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    Some(Sha256::digest(text.as_bytes()).into())
}

/// One registry keyed by the session ID's SHA-256 digest. A single lock
/// linearizes lookup, renewal, rotation and removal.
///
/// B2 seam: streams add a per-session generation and registered
/// cancellation (plus expiry wakeups) under this same lock, so logout,
/// rotation and expiry close upgraded connections; stream checks must not
/// renew `touched`.
pub(super) struct Sessions {
    active: Mutex<Data>,
    hash_slot: Arc<tokio::sync::Semaphore>,
}

#[derive(Default)]
struct Data {
    sessions: HashMap<Key, Session>,
    clients: HashMap<IpAddr, Failures>,
    global: Failures,
}

struct Session {
    created: Instant,
    touched: Instant,
    csrf: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SessionView {
    user: User,
    csrf_token: String,
    expires_at: String,
}

#[derive(Serialize)]
struct User {
    name: &'static str,
}

impl Session {
    fn remaining(&self, now: Instant) -> Duration {
        IDLE.saturating_sub(now.saturating_duration_since(self.touched))
            .min(ABSOLUTE.saturating_sub(now.saturating_duration_since(self.created)))
    }

    fn view(&self, now: ClockReading) -> Result<SessionView, ApiError> {
        Ok(SessionView {
            user: User { name: "owner" },
            csrf_token: self.csrf.clone(),
            expires_at: instant_string(now.utc_ms.saturating_add(
                i64::try_from(self.remaining(now.monotonic).as_millis()).unwrap_or(i64::MAX),
            ))?,
        })
    }
}

#[derive(Default)]
struct Failures {
    count: u32,
    last: Option<Instant>,
}

impl Failures {
    fn stale(&self, now: Instant) -> bool {
        self.last
            .is_none_or(|last| now.saturating_duration_since(last) >= FORGET_AFTER)
    }

    fn retry_after(&self, now: Instant, threshold: u32) -> Duration {
        if self.count < threshold || self.stale(now) {
            return Duration::ZERO;
        }
        let delay = Duration::from_secs(1 << (self.count - threshold).min(8));
        delay.saturating_sub(now.saturating_duration_since(self.last.expect("failure timestamp")))
    }

    fn record(&mut self, now: Instant) {
        if self.stale(now) {
            self.count = 0;
        }
        self.count = self.count.saturating_add(1);
        self.last = Some(now);
    }
}

impl Sessions {
    pub(super) fn new() -> Self {
        Self {
            active: Mutex::default(),
            hash_slot: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub(super) async fn login(
        self: Arc<Self>,
        verifier: Arc<Verifier>,
        clock: Arc<dyn crate::vm::Clock>,
        client: IpAddr,
        old: Option<Key>,
        username: String,
        password: Zeroizing<String>,
    ) -> Result<(String, SessionView), ApiError> {
        // Hold through both the hash and the failure update, even if the
        // connection dropped. No blocking-job queue can accumulate here.
        let permit = self
            .hash_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::rate_limit(Duration::from_secs(1)))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let now = clock.now().monotonic;
            {
                let mut data = self.active.lock().map_err(|_| ApiError::UNAVAILABLE)?;
                data.clients.retain(|_, failures| !failures.stale(now));
                let mut wait = data.global.retry_after(now, 12);
                if let Some(failures) = data.clients.get(&client) {
                    wait = wait.max(failures.retry_after(now, 5));
                } else if data.clients.len() == MAX_CLIENTS {
                    wait = wait.max(Duration::from_secs(1));
                }
                if !wait.is_zero() {
                    return Err(ApiError::rate_limit(wait));
                }
            }
            let matches = verifier.accepts(&username, &password);
            let now = clock.now();
            let mut data = self.active.lock().map_err(|_| ApiError::UNAVAILABLE)?;
            if !matches {
                data.global.record(now.monotonic);
                data.clients
                    .entry(client)
                    .or_default()
                    .record(now.monotonic);
                return Err(ApiError::BAD_CREDENTIALS);
            }
            data.clients.remove(&client);
            data.sessions
                .retain(|_, session| !session.remaining(now.monotonic).is_zero());
            let token = random_token().map_err(|_| ApiError::UNAVAILABLE)?;
            let key = cookie_key(&token).expect("generated cookie format");
            if data.sessions.contains_key(&key)
                || (data.sessions.len() >= MAX_SESSIONS
                    && !old.is_some_and(|old| data.sessions.contains_key(&old)))
            {
                return Err(ApiError::UNAVAILABLE);
            }
            let session = Session {
                created: now.monotonic,
                touched: now.monotonic,
                csrf: random_token().map_err(|_| ApiError::UNAVAILABLE)?,
            };
            let view = session.view(now)?;
            if let Some(old) = old {
                data.sessions.remove(&old);
            }
            data.sessions.insert(key, session);
            Ok((token, view))
        })
        .await
        .map_err(|_| ApiError::UNAVAILABLE)?
    }

    pub(super) fn authenticate(
        &self,
        key: Option<Key>,
        now: ClockReading,
        csrf: Option<Option<&str>>,
        revoke: bool,
    ) -> Result<SessionView, ApiError> {
        let key = key.ok_or(ApiError::UNAUTHORIZED)?;
        let mut data = self.active.lock().map_err(|_| ApiError::UNAVAILABLE)?;
        let session = data.sessions.get_mut(&key).ok_or(ApiError::UNAUTHORIZED)?;
        if session.remaining(now.monotonic).is_zero() {
            data.sessions.remove(&key);
            return Err(ApiError::UNAUTHORIZED);
        }
        if let Some(token) = csrf
            && !token
                .is_some_and(|value| bool::from(value.as_bytes().ct_eq(session.csrf.as_bytes())))
        {
            return Err(ApiError::FORBIDDEN);
        }
        session.touched = session.touched.max(now.monotonic);
        let view = session.view(now)?;
        if revoke {
            // This store has no durable backing. A revoked id cannot be
            // recovered from disk, including after a crash or restart.
            data.sessions.remove(&key);
        }
        Ok(view)
    }
}

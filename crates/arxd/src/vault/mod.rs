//! Caller-scoped versioned scripts. MCP has no route to test completion.
mod files;
pub(crate) mod mcp;
mod read;
mod store;

use crate::{store::Activity, vm::RequestId};
use arx_guest::{MAX_SCRIPT, ScriptOutcome};
use rusqlite::{OptionalExtension, params, types};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, OnceLock, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
#[path = "../../tests/vault_fixtures/state.rs"]
mod tests;

type Id = [u8; 16];
type Reply = Result<Value, Refusal>;

const MAX_UTC_MS: i64 = 253_402_300_799_999;

struct Registration {
    activity: Weak<Activity>,
    vault: Weak<Vault>,
}

static FOREGROUND: OnceLock<Registration> = OnceLock::new();

#[doc(hidden)]
pub struct ForegroundVault {
    _vault: Arc<Vault>,
}

/// Composes the foreground local Inventory without changing service/HTTP authority.
#[doc(hidden)]
pub fn initialize_foreground(
    activity: &Arc<Activity>,
    state: &Path,
) -> anyhow::Result<ForegroundVault> {
    let vault = Vault::open(state, None, utc_ms()?)?;
    FOREGROUND
        .set(Registration {
            activity: Arc::downgrade(activity),
            vault: Arc::downgrade(&vault),
        })
        .map_err(|_| anyhow::anyhow!("foreground vault already initialized"))?;
    Ok(ForegroundVault { _vault: vault })
}

pub(crate) fn for_activity(activity: &Arc<Activity>) -> Option<Arc<Vault>> {
    let registered = FOREGROUND.get()?;
    if !Arc::ptr_eq(&registered.activity.upgrade()?, activity) {
        return None;
    }
    registered.vault.upgrade()
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScriptOs {
    Windows,
    Ubuntu,
}

impl ScriptOs {
    fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Ubuntu => "ubuntu",
        }
    }
}

impl types::FromSql for ScriptOs {
    fn column_result(value: types::ValueRef<'_>) -> types::FromSqlResult<Self> {
        match value.as_str()? {
            "windows" => Ok(Self::Windows),
            "ubuntu" => Ok(Self::Ubuntu),
            _ => Err(types::FromSqlError::InvalidType),
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    Add,
    Test,
    Activate,
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Add => "vault_add_version",
            Self::Test => "vault_test_version",
            Self::Activate => "vault_activate",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    Invalid,
    InvalidCursor,
    StaleKey,
    KeyCollision,
    ReplayFull,
    NotFound,
    NotDraft,
    TesterUnavailable,
    PassRequired,
    VersionSuperseded,
    Busy,
    ScriptLimit,
    VersionLimit,
    ByteLimit,
    StateUnavailable,
}

impl Refusal {
    fn as_str(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::InvalidCursor => "invalid_cursor",
            Self::StaleKey => "stale_key",
            Self::KeyCollision => "key_collision",
            Self::ReplayFull => "replay_full",
            Self::NotFound => "not_found",
            Self::NotDraft => "not_draft",
            Self::TesterUnavailable => "tester_unavailable",
            Self::PassRequired => "pass_required",
            Self::VersionSuperseded => "version_superseded",
            Self::Busy => "busy",
            Self::ScriptLimit => "script_limit",
            Self::VersionLimit => "version_limit",
            Self::ByteLimit => "byte_limit",
            Self::StateUnavailable => "state_unavailable",
        }
    }
}

enum Write {
    Add {
        request_id: RequestId,
        os: ScriptOs,
        name: String,
        script: String,
    },
    Test {
        request_id: RequestId,
        version_id: Id,
    },
    Activate {
        request_id: RequestId,
        version_id: Id,
    },
}

impl Write {
    fn request_id(&self) -> RequestId {
        match self {
            Self::Add { request_id, .. }
            | Self::Test { request_id, .. }
            | Self::Activate { request_id, .. } => *request_id,
        }
    }

    fn binding(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"arxcovo vault replay v1\0");
        match self {
            Self::Add {
                os, name, script, ..
            } => {
                digest.update(b"add\0");
                digest.update(os.as_str().as_bytes());
                digest.update([0]);
                digest.update(name.as_bytes());
                digest.update([0]);
                digest.update(Sha256::digest(script.as_bytes()));
            }
            Self::Test { version_id, .. } => {
                digest.update(b"test\0");
                digest.update(version_id);
            }
            Self::Activate { version_id, .. } => {
                digest.update(b"activate\0");
                digest.update(version_id);
            }
        }
        digest.finalize().into()
    }
}

/// Trusted daemon adapter. Enqueue is a prompt handoff after commit. The adapter
/// must reserve a throwaway generation and qualify its teardown before reporting
/// a pass. It must never log the job's body or expose it as guest output.
trait VaultTester: Send + Sync {
    fn enqueue(&self, job: TestJob);
}

// No production composition supplies a tester pending the M4 throwaway journey.
// These adapter methods are exercised by the internal fake until that wiring lands.
#[cfg_attr(not(test), allow(dead_code))]
struct TestJob {
    store: Arc<store::Store>,
    owner: String,
    data: store::JobData,
    settled: bool,
}

#[cfg_attr(not(test), allow(dead_code))]
impl TestJob {
    fn os(&self) -> ScriptOs {
        self.data.os
    }

    fn content(&self) -> &str {
        &self.data.content
    }

    fn sha256(&self) -> [u8; 32] {
        self.data.hash
    }

    fn complete(mut self, outcome: ScriptOutcome, cleanup_confirmed: bool) -> anyhow::Result<()> {
        let (status, exit_code) = match outcome {
            ScriptOutcome::Exited { exit_code } => ("exited", Some(exit_code)),
            ScriptOutcome::Unavailable => ("unavailable", None),
            ScriptOutcome::TimedOut => ("timed_out", None),
            ScriptOutcome::LaunchFailed => ("launch_failed", None),
            ScriptOutcome::Terminated => ("terminated", None),
        };
        self.finish(status, exit_code, cleanup_confirmed)?;
        self.settled = true;
        Ok(())
    }

    fn finish(
        &self,
        status: &str,
        exit_code: Option<u32>,
        cleanup_confirmed: bool,
    ) -> anyhow::Result<()> {
        let passed = status == "exited" && exit_code == Some(0) && cleanup_confirmed;
        let scope = store::WriteScope {
            owner: &self.owner,
            action: Action::Test,
            at_ms: utc_ms()?,
        };
        self.store.write(|db| {
            let changed = db.execute(
                "UPDATE versions SET state=?1,test_status=?2,exit_code=?3,cleanup_confirmed=?4
                 WHERE id=?5 AND test_op=?6 AND hash=?7 AND state='testing' AND test_status IS NULL",
                params![if passed { "testing" } else { "rejected" }, status, exit_code,
                    cleanup_confirmed, self.data.version, self.data.operation, self.data.hash],
            )?;
            anyhow::ensure!(changed == 1, "vault test claim no longer pending");
            scope.audit(db, if passed { "succeeded" } else { "failed" },
                if passed { None } else if status == "exited" && exit_code == Some(0) {
                    Some("cleanup_unconfirmed")
                } else { Some(status) }, Some(self.data.version), Some(self.data.operation))
        })
    }
}

impl Drop for TestJob {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self.finish("interrupted", None, false);
        }
    }
}

pub(crate) struct Vault {
    store: Arc<store::Store>,
    tester: Option<Arc<dyn VaultTester>>,
    cursor_key: ring::hmac::Key,
}

impl Vault {
    fn open(
        dir: &Path,
        tester: Option<Arc<dyn VaultTester>>,
        now: i64,
    ) -> anyhow::Result<Arc<Self>> {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret)
            .map_err(|_| anyhow::anyhow!("vault randomness unavailable"))?;
        Ok(Arc::new(Self {
            store: Arc::new(store::Store::open(dir, now)?),
            tester,
            cursor_key: ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &secret),
        }))
    }

    fn mutate(
        &self,
        owner: &str,
        action: Action,
        write: Result<Write, Refusal>,
        now: i64,
    ) -> Reply {
        let scope = store::WriteScope {
            owner,
            action,
            at_ms: now,
        };
        let (reply, job) = self
            .store
            .write(|db| match write {
                Ok(write) => scope.execute(db, write, self.tester.is_some()),
                Err(reason) => {
                    scope.audit(db, "refused", Some(reason.as_str()), None, None)?;
                    Ok((Err(reason), None))
                }
            })
            .map_err(|_| Refusal::StateUnavailable)?;
        if let Some(data) = job {
            self.tester
                .as_ref()
                .expect("admitted tester")
                .enqueue(TestJob {
                    store: self.store.clone(),
                    owner: owner.into(),
                    settled: false,
                    data,
                });
        }
        reply
    }

    fn get(&self, owner: &str, version: Id, include_content: bool) -> Reply {
        self.store
            .read(|db| {
                let metadata = db
                    .query_row(
                        &format!(
                            "SELECT {} FROM versions v JOIN scripts s ON s.seq=v.script
                          WHERE v.id=?1 AND s.owner=?2",
                            read::VERSION_FIELDS
                        ),
                        params![version, owner],
                        read::version,
                    )
                    .optional()?;
                let Some(mut metadata) = metadata else {
                    return Ok(Err(Refusal::NotFound));
                };
                if include_content {
                    metadata["content"] = Value::String(store::content(db, version)?.0);
                }
                Ok(Ok(metadata))
            })
            .map_err(|_| Refusal::StateUnavailable)?
    }
}

fn utc_ms() -> anyhow::Result<i64> {
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    anyhow::ensure!(now <= MAX_UTC_MS, "invalid vault clock");
    Ok(now)
}

fn new_id() -> anyhow::Result<Id> {
    let mut id = [0; 16];
    getrandom::fill(&mut id).map_err(|_| anyhow::anyhow!("vault randomness unavailable"))?;
    Ok(id)
}

fn valid_script(script: &str) -> bool {
    (1..=MAX_SCRIPT).contains(&script.len()) && !script.as_bytes().contains(&0)
}

fn valid_name(name: &str) -> bool {
    (1..=48).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn parse_id(id: &str) -> Result<Id, Refusal> {
    let mut bytes = [0; 16];
    if id.bytes().any(|b| !matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(Refusal::Invalid);
    }
    hex::decode_to_slice(id, &mut bytes).map_err(|_| Refusal::Invalid)?;
    Ok(bytes)
}

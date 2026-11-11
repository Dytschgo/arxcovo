//! Dedicated-service contract. No native command or privileged probe is installed.
use super::preflight::ExecutionUnavailable as Unavailable;
use crate::{
    development::{Cancellation, CapturedOutput},
    store::{Activity, Metadata, Outcome, Reason, StopReason},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub tool: Option<String>,
    pub executable: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default = "timeout")]
    pub timeout_s: u64,
    #[serde(default = "output")]
    pub output_bytes: usize,
}
fn timeout() -> u64 {
    60
}
fn output() -> usize {
    4096
}
impl Request {
    pub fn validate(&self) -> bool {
        let target = match (&self.tool, &self.executable) {
            (Some(t), None) => crate::development::TOOLS.contains(&t.as_str()),
            (None, Some(p)) => absolute_executable(p),
            _ => false,
        };
        // Reuse the existing argument/cwd/time/output limits without any filesystem IO.
        let bounded = crate::development::Request {
            tool: "cargo".into(),
            args: self.args.clone(),
            cwd: self.cwd.clone(),
            timeout_s: self.timeout_s,
            output_bytes: self.output_bytes,
        };
        target && bounded.validate()
    }
}
/// Lexical only: the eventual identity-switched child must reject links/reparse
/// points while opening. The privileged service must never inspect these paths.
fn absolute_executable(p: &str) -> bool {
    if p.len() > 4096
        || p.is_empty()
        || p.chars().any(char::is_control)
        || p.ends_with(['/', '\\'])
        || p.contains(['\0', '<', '>', '"', '|', '?', '*'])
    {
        return false;
    }
    #[cfg(windows)]
    let absolute = p.len() > 3
        && p.as_bytes()[0].is_ascii_alphabetic()
        && p.as_bytes()[1] == b':'
        && p.as_bytes()[2] == b'\\'
        && !p[2..].contains(':')
        && p.to_ascii_lowercase().ends_with(".exe")
        && p[3..].split(['/', '\\']).all(|part| {
            let base = part
                .split('.')
                .next()
                .unwrap_or("")
                .trim_end_matches(' ')
                .to_ascii_uppercase();
            let port = base
                .strip_prefix("COM")
                .or_else(|| base.strip_prefix("LPT"));
            !["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base.as_str())
                && !matches!(
                    port,
                    Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
                )
        });
    #[cfg(not(windows))]
    let absolute = p.starts_with('/') && !p.starts_with("//") && !p.contains(['\\', ':']);
    absolute
        && p.split(['/', '\\'])
            .all(|c| c != "." && c != ".." && !c.ends_with([' ', '.']))
}
pub fn schema() -> Value {
    json!({"type":"object","oneOf":[{"required":["tool"]},{"required":["executable"]}],"properties":{"tool":{"type":"string","enum":crate::development::TOOLS},"executable":{"type":"string","maxLength":4096},"args":{"type":"array","maxItems":128,"items":{"type":"string","maxLength":8192}},"cwd":{"type":"string","maxLength":4096},"timeout_s":{"type":"integer","minimum":1,"maximum":3600},"output_bytes":{"type":"integer","minimum":2,"maximum":8192}},"additionalProperties":false})
}
/// Cancellation is cooperative, including shutdown. Implementations must bound
/// cleanup and return a terminal report; drain joins every dispatch worker.
/// Recheck request cancellation and shutdown immediately before actual spawn;
/// cancellation before spawn must start no child.
pub trait Executor: Send + Sync + 'static {
    fn preflight(&self) -> Result<(), Unavailable>;
    fn run(
        &self,
        request: Option<&Request>,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Report;
}
pub enum Report {
    Unavailable(Unavailable),
    Failed {
        after_spawn: bool,
    },
    Discovered {
        metadata: Metadata,
        tools: Vec<ToolVersion>,
    },
    Completed {
        metadata: Metadata,
        stdout: CapturedOutput,
        stderr: CapturedOutput,
    },
    #[cfg(test)]
    FakeForTest {
        stop: StopReason,
        cleanup: bool,
    },
}
struct Native {
    reason: Arc<Mutex<Unavailable>>,
    check: Option<Box<dyn Fn() -> Result<(), Unavailable> + Send + Sync>>,
}
impl Executor for Native {
    fn preflight(&self) -> Result<(), Unavailable> {
        if let Some(check) = &self.check {
            check()?;
            return Err(Unavailable::MechanismUnavailable);
        }
        Err(*self.reason.lock().unwrap())
    }
    fn run(&self, _: Option<&Request>, _: &Cancellation, _: &Cancellation) -> Report {
        Report::Unavailable(Unavailable::MechanismUnavailable)
    }
}
pub struct Execution {
    backend: Arc<dyn Executor>,
    reason: Arc<Mutex<Unavailable>>,
    shutdown: Cancellation,
    quarantined: AtomicBool,
    active: AtomicUsize,
    audit_failed: AtomicBool,
    limit: usize,
    workers: Mutex<tokio::task::JoinSet<()>>,
    rights: Option<crate::store::Rights>,
}
pub struct Failure {
    pub reason: Reason,
    pub execution_reason: Option<Unavailable>,
}
struct CancelOnDrop(Cancellation);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl Execution {
    pub(crate) fn unavailable() -> Arc<Self> {
        Self::native(None, None, 1)
    }
    fn native(
        check: Option<Box<dyn Fn() -> Result<(), Unavailable> + Send + Sync>>,
        rights: Option<crate::store::Rights>,
        limit: usize,
    ) -> Arc<Self> {
        let reason = Arc::new(Mutex::new(Unavailable::MechanismUnavailable));
        Arc::new(Self {
            backend: Arc::new(Native {
                reason: reason.clone(),
                check,
            }),
            reason,
            shutdown: Cancellation::default(),
            quarantined: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            audit_failed: AtomicBool::new(false),
            limit,
            workers: Mutex::new(tokio::task::JoinSet::new()),
            rights,
        })
    }
    pub(crate) fn configured(
        layout: super::layout::Layout,
        config: super::config::ServiceConfig,
    ) -> Arc<Self> {
        let rights = match config.rights() {
            super::config::Rights::Standard => crate::store::Rights::Standard,
            super::config::Rights::Admin => crate::store::Rights::Admin,
        };
        let limit = config.limits().0 as usize;
        Self::native(
            Some(Box::new(move || {
                config.execution_preflight(&layout, &super::native::NativeResolver)
            })),
            Some(rights),
            limit,
        )
    }
    pub(crate) fn set_reason(&self, reason: Unavailable) {
        *self.reason.lock().unwrap() = reason;
    }
    fn preflight(&self) -> Result<(), Unavailable> {
        if self.quarantined.load(Ordering::Acquire) {
            return Err(Unavailable::Quarantined);
        }
        if self.shutdown.is_cancelled() {
            return Err(Unavailable::Stopping);
        }
        self.backend.preflight()
    }
    pub fn capability(&self) -> Value {
        // Cached status only: list_hosts must not perform privileged probes.
        let reason = if self.quarantined.load(Ordering::Acquire) {
            Unavailable::Quarantined
        } else if self.shutdown.is_cancelled() {
            Unavailable::Stopping
        } else {
            *self.reason.lock().unwrap()
        };
        json!({"available":false,"reason":reason.as_str(),"rights":self.rights,"service_hardening":null,"service_state_protected":self.rights != Some(crate::store::Rights::Admin)})
    }
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }
    pub async fn drain(&self) -> Result<(), Reason> {
        self.shutdown();
        let mut workers = std::mem::take(&mut *self.workers.lock().unwrap());
        let mut failed = false;
        while let Some(result) = workers.join_next().await {
            failed |= result.is_err();
        }
        if failed || self.audit_failed.load(Ordering::Acquire) {
            Err(Reason::ExecutionFailed)
        } else {
            Ok(())
        }
    }
    /// Called synchronously after the started transaction; worker owns finish.
    pub(crate) fn dispatch(
        self: &Arc<Self>,
        request: Option<Request>,
        activity: Arc<Activity>,
        id: i64,
    ) -> impl std::future::Future<Output = Result<Value, Failure>> + Send + 'static {
        let cancel = Cancellation::default();
        let guard = CancelOnDrop(cancel.clone());
        let this = self.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        let mut workers = self.workers.lock().unwrap();
        while let Some(result) = workers.try_join_next() {
            if result.is_err() {
                self.quarantined.store(true, Ordering::Release);
            }
        }
        workers.spawn_blocking(move || {
            let report = if let Err(r) = this.preflight() {
                Report::Unavailable(r)
            } else if this.active.fetch_add(1, Ordering::AcqRel) >= this.limit {
                this.active.fetch_sub(1, Ordering::AcqRel);
                Report::Unavailable(Unavailable::Busy)
            } else {
                let r = if cancel.is_cancelled() {
                    Report::Completed { metadata: Metadata { stop_reason: Some(StopReason::Cancelled), ..Default::default() }, stdout: CapturedOutput {head:vec![],tail:vec![],dropped_bytes:0}, stderr: CapturedOutput {head:vec![],tail:vec![],dropped_bytes:0} }
                } else if this.shutdown.is_cancelled() {
                    Report::Unavailable(Unavailable::Stopping)
                } else {
                    this.backend.run(request.as_ref(), &cancel, &this.shutdown)
                };
                this.active.fetch_sub(1, Ordering::AcqRel);
                r
            };
            let invalid_discovery = matches!(&report, Report::Discovered { tools, .. } if !valid_tools(tools));
            let post_spawn_error = invalid_discovery || matches!(&report, Report::Failed { after_spawn:true } | Report::Completed { metadata:Metadata { stop_reason:Some(StopReason::Failed), .. }, .. } | Report::Discovered { metadata:Metadata { stop_reason:Some(StopReason::Failed), .. }, .. });
            let (mut outcome, mut metadata, mut value, refusal) = match report {
                Report::Unavailable(r) => (
                    Outcome::Refused,
                    Metadata {
                        reason: Some(r.stored_reason()),
                        ..Default::default()
                    },
                    Value::Null,
                    Some(r),
                ),
                Report::Failed { after_spawn } => {
                    (Outcome::Failed, Metadata { reason:Some(if after_spawn {Reason::CleanupUnconfirmed} else {Reason::ExecutionFailed}), stop_reason:Some(StopReason::Failed), cleanup_confirmed:after_spawn.then_some(false), ..Default::default() }, json!({}), None)
                }
                Report::Discovered { metadata, tools } => {
                    let valid = valid_tools(&tools);
                    if valid {
                        (terminal(&metadata), metadata, json!({"tools":tools}), None)
                    } else {
                        (Outcome::Failed, metadata, json!({}), None)
                    }
                }
                Report::Completed {
                    metadata,
                    stdout,
                    stderr,
                } => {
                    let value =
                        json!({"stdout":output_value(stdout, request.as_ref().map_or(4096, |r| r.output_bytes)),"stderr":output_value(stderr, request.as_ref().map_or(4096, |r| r.output_bytes))});
                    (terminal(&metadata), metadata, value, None)
                }
                #[cfg(test)]
                Report::FakeForTest { stop, cleanup } => (
                    Outcome::Failed,
                    Metadata {
                        stop_reason: Some(stop),
                        cleanup_confirmed: Some(cleanup),
                        ..Default::default()
                    },
                    json!({"test_outcome":"FakeForTest","acceptance":false}),
                    None,
                ),
            };
            metadata.identity_mode = Some(crate::store::IdentityMode::DedicatedAccount);
            normalize(&mut metadata);
            if invalid_discovery && metadata.reason.is_none() { metadata.reason = Some(Reason::ExecutionFailed); }
            if outcome != Outcome::Refused { outcome=terminal(&metadata); }
            if metadata.cleanup_confirmed == Some(false) || post_spawn_error {
                this.quarantined.store(true, Ordering::Release);
            }
            if let Some(r) = &request {
                metadata.program = Some(match &r.tool {
                    Some(t) => serde_json::from_value(json!(t)).unwrap(),
                    None => crate::store::Program::Path,
                });
                metadata.argc = Some(r.args.len() as u16);
            }
            let result = activity
                .finish_typed(id, outcome, &metadata)
                .map_err(|_| Failure {
                    reason: Reason::StorageFailed,
                    execution_reason: None,
                })
                .and_then(|()| {
                    if let Some(r) = refusal {
                        return Err(Failure {
                            reason: r.stored_reason(),
                            execution_reason: Some(r),
                        });
                    }
                    value["activity_id"] = json!(id);
                    value["outcome"] = json!(outcome.as_str());
                    value["reason"] = json!(metadata.reason);
                value["exit_code"] = json!(metadata.exit_code);
                value["rights"] = json!(metadata.rights);
                value["tree_boundary"] = json!(metadata.tree_boundary);
                value["cleanup_confirmed"] = json!(metadata.cleanup_confirmed);
                value["observed_identity"] = if metadata.observed_kind.is_some() { json!({"kind":metadata.observed_kind,"id":metadata.observed_id,"gid":metadata.observed_gid,"integrity_rid":metadata.integrity_rid,"elevation":metadata.elevation,"observation_point":metadata.observation_point}) } else { Value::Null };
                value["metadata"] = serde_json::to_value(metadata).unwrap();
                    Ok(value)
                });
            if result.as_ref().is_err_and(|e| e.reason == Reason::StorageFailed) { this.audit_failed.store(true, Ordering::Release); }
            let _ = send.send(result);
        });
        drop(workers);
        async move {
            let _guard = guard;
            receive.await.map_err(|_| Failure {
                reason: Reason::ExecutionFailed,
                execution_reason: None,
            })?
        }
    }
}
/// Exhaustive stop mapping; response-only preflight details never enter metadata.
pub fn normalize(m: &mut Metadata) {
    if m.cleanup_confirmed == Some(false) {
        m.reason = Some(Reason::CleanupUnconfirmed);
    } else if m.identity_matches_pin == Some(false) {
        m.reason = Some(Reason::IdentityMismatch);
    } else if let Some(stop) = m.stop_reason {
        match stop {
            StopReason::Failed => {
                m.reason = Some(Reason::ExecutionFailed);
            }
            StopReason::Exited
            | StopReason::Timeout
            | StopReason::Cancelled
            | StopReason::OutputLimit
            | StopReason::Shutdown => {
                m.reason = None;
            }
        }
    }
}
pub fn terminal(m: &Metadata) -> Outcome {
    if m.cleanup_confirmed == Some(false) || m.reason.is_some() {
        Outcome::Failed
    } else if m.stop_reason == Some(StopReason::Exited) && m.exit_code == Some(0) {
        Outcome::Succeeded
    } else {
        Outcome::Failed
    }
}
fn output_value(o: CapturedOutput, budget: usize) -> Value {
    // Shared total of 8192 raw bytes per stream; max 3x inner, 4x double
    // JSON escaping. Bound below that further for two streams plus metadata.
    fn text(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|b| match b {
                b' '..=b'~' | b'\t' | b'\n' | b'\r' => *b as char,
                _ => '\u{fffd}',
            })
            .collect()
    }
    let head = &o.head[..o.head.len().min(budget.min(7168) / 2)];
    let tail = &o.tail[..o.tail.len().min(budget.min(7168) / 2)];
    json!({"head":text(head),"tail":text(tail),"dropped_bytes":o.dropped_bytes.saturating_add((o.head.len()-head.len()+o.tail.len()-tail.len()) as u64)})
}

/// Account-scoped probe output only; never absolute paths. Executor validates the
/// complete fixed table before returning it. Production discovery is unavailable.
#[derive(serde::Serialize)]
pub struct ToolVersion {
    pub name: String,
    pub found: bool,
    pub version: Option<String>,
}
/// Pure mapping, separate from native observation. Synthetic facts in tests do
/// not establish that any child was observed on a real host.
pub enum Observation {
    Windows {
        sid: String,
        integrity_rid: u32,
        elevated: bool,
        matches: bool,
    },
    Unix {
        uid: u32,
        gid: u32,
        matches: bool,
    },
}
impl Observation {
    pub fn metadata(self, rights: crate::store::Rights) -> Metadata {
        use crate::store::{IdentityMode, ObservationPoint, ObservedKind};
        let mut m = Metadata {
            identity_mode: Some(IdentityMode::DedicatedAccount),
            rights: Some(rights),
            ..Default::default()
        };
        match self {
            Self::Windows {
                sid,
                integrity_rid,
                elevated,
                matches,
            } => {
                m.observed_kind = Some(ObservedKind::WindowsSid);
                m.observed_id = Some(sid);
                m.integrity_rid = Some(integrity_rid);
                m.elevation = Some(elevated);
                m.observation_point = Some(ObservationPoint::PostSwitch);
                m.identity_matches_pin = Some(matches);
            }
            Self::Unix { uid, gid, matches } => {
                m.observed_kind = Some(ObservedKind::UnixUid);
                m.observed_id = Some(uid.to_string());
                m.observed_gid = Some(gid);
                m.observation_point = Some(ObservationPoint::PreExec);
                m.identity_matches_pin = Some(matches);
            }
        }
        if m.identity_matches_pin == Some(false) {
            m.reason = Some(Reason::IdentityMismatch);
        }
        m
    }
}
#[cfg(test)]
pub(crate) mod tests;

fn valid_tools(tools: &[ToolVersion]) -> bool {
    tools.len() == crate::development::TOOLS.len()
        && tools.iter().zip(crate::development::TOOLS).all(|(t, n)| {
            t.name == *n
                && (t.found || t.version.is_none())
                && t.version.as_ref().is_none_or(|v| {
                    v.len() <= 128
                        && v.bytes().all(|b| (32..=126).contains(&b))
                        && !v.contains(['/', '\\'])
                })
        })
}

//! Dedicated-service contract. No native command or privileged probe is installed.
use super::layout::Platform;
use super::preflight::ExecutionUnavailable as Unavailable;
use crate::store::{IdentityMode, ObservationPoint, ObservedKind, Rights, TreeBoundary};
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
/// ADR0017: creation means the primitive call, never exec. NotInvoked means
/// no call; ProvenNone requires a directly distinguishable result and a cited
/// normative no-child guarantee (no current adapter qualifies). A child handle,
/// PID or child-side signal means Created; all other post-call failures mean
/// Unknown, including Unix Command::spawn Err and Windows CreateProcess* FALSE.
/// Honor no cancellation between that call and the end of the bounded identity
/// read. Protected identifiers must never leave Evidence or enter logs/caches.
/// CapturedOutput must contain only bytes from the observed child's own pipes.
/// Only a full pin-matched child may release either stream; missing or mismatched
/// identity withholds both captures unread, independently of cleanup or outcome.
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
    CancelledBeforeSpawn,
    Failed {
        creation: Creation,
    },
    Discovered {
        evidence: Evidence,
        tools: Vec<ToolVersion>,
    },
    Completed {
        evidence: Evidence,
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
    pin: Result<Pin, Unavailable>,
    configured_rights: Option<Rights>,
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
        Self::native(None, Err(Unavailable::NotConfigured), None, 1)
    }
    fn native(
        check: Option<Box<dyn Fn() -> Result<(), Unavailable> + Send + Sync>>,
        pin: Result<Pin, Unavailable>,
        configured_rights: Option<Rights>,
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
            pin,
            configured_rights,
        })
    }
    pub(crate) fn configured(
        layout: super::layout::Layout,
        config: super::config::ServiceConfig,
    ) -> Arc<Self> {
        let pin = Pin::from_config(&config, layout.platform());
        let configured_rights = Some(match config.rights() {
            super::config::Rights::Standard => Rights::Standard,
            super::config::Rights::Admin => Rights::Admin,
        });
        let limit = config.limits().0 as usize;
        Self::native(
            Some(Box::new(move || {
                config.execution_preflight(&layout, &super::native::NativeResolver)
            })),
            pin,
            configured_rights,
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
        self.backend.preflight()?;
        self.pin.as_ref().map(|_| ()).map_err(|r| *r)
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
        let rights = self.configured_rights;
        json!({"available":false,"reason":reason.as_str(),"rights":rights,"service_hardening":null,"service_state_protected":rights != Some(crate::store::Rights::Admin)})
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
        // Serialize the stopping decision with drain's JoinSet transfer. A
        // stopping-only refusal must finish before returning, without a worker
        // that could land in the replacement set after drain took ownership.
        if self.shutdown.is_cancelled() {
            let refusal = self.preflight().err().unwrap_or(Unavailable::Stopping);
            let result = self.complete(
                Report::Unavailable(refusal),
                request.as_ref(),
                &activity,
                id,
            );
            let _ = send.send(result);
        } else {
            workers.spawn_blocking(move || {
                let report = if let Err(r) = this.preflight() {
                    Report::Unavailable(r)
                } else if this.active.fetch_add(1, Ordering::AcqRel) >= this.limit {
                    this.active.fetch_sub(1, Ordering::AcqRel);
                    Report::Unavailable(Unavailable::Busy)
                } else {
                    let r = if cancel.is_cancelled() {
                        Report::CancelledBeforeSpawn
                    } else if this.shutdown.is_cancelled() {
                        Report::Unavailable(Unavailable::Stopping)
                    } else {
                        this.backend.run(request.as_ref(), &cancel, &this.shutdown)
                    };
                    this.active.fetch_sub(1, Ordering::AcqRel);
                    r
                };
                let result = this.complete(report, request.as_ref(), &activity, id);
                let _ = send.send(result);
            });
        }
        drop(workers);
        async move {
            let _guard = guard;
            receive.await.map_err(|_| Failure {
                reason: Reason::ExecutionFailed,
                execution_reason: None,
            })?
        }
    }
    /// Shared terminal persistence for accepted workers and synchronous refusals.
    fn complete(
        &self,
        report: Report,
        request: Option<&Request>,
        activity: &Activity,
        id: i64,
    ) -> Result<Value, Failure> {
        let (outcome, mut metadata, mut value, refusal, quarantine) = match report {
            Report::Unavailable(r) => (
                Outcome::Refused,
                Metadata {
                    reason: Some(r.stored_reason()),
                    ..Default::default()
                },
                Value::Null,
                Some(r),
                false,
            ),
            Report::CancelledBeforeSpawn => (
                Outcome::Failed,
                Metadata {
                    stop_reason: Some(StopReason::Cancelled),
                    ..Default::default()
                },
                json!({}),
                None,
                false,
            ),
            Report::Failed { creation } => {
                let spawned = match creation {
                    Creation::NotInvoked | Creation::ProvenNone => false,
                    Creation::Created | Creation::Unknown => true,
                };
                (
                    Outcome::Failed,
                    Metadata {
                        reason: Some(if spawned {
                            Reason::CleanupUnconfirmed
                        } else {
                            Reason::ExecutionFailed
                        }),
                        stop_reason: Some(StopReason::Failed),
                        rights: if spawned {
                            self.pin.as_ref().ok().map(|p| p.rights)
                        } else {
                            None
                        },
                        cleanup_confirmed: spawned.then_some(false),
                        ..Default::default()
                    },
                    json!({}),
                    None,
                    spawned,
                )
            }
            Report::Discovered { evidence, tools } => {
                let (outcome, metadata, quarantine) = assess(
                    self.pin.as_ref().expect("preflight pin"),
                    evidence,
                    Some(valid_tools(&tools)),
                );
                let value = if outcome == Outcome::Succeeded {
                    json!({"tools":tools})
                } else {
                    json!({})
                };
                (outcome, metadata, value, None, quarantine)
            }
            Report::Completed {
                evidence,
                stdout,
                stderr,
            } => {
                let (outcome, metadata, quarantine) =
                    assess(self.pin.as_ref().expect("preflight pin"), evidence, None);
                let value = match metadata.identity_matches_pin {
                    Some(true) => {
                        json!({"stdout":output_value(stdout, request.map_or(4096, |r| r.output_bytes)),"stderr":output_value(stderr, request.map_or(4096, |r| r.output_bytes))})
                    }
                    Some(false) | None => json!({
                        "stdout":{"head":"","tail":"","dropped_bytes":null},
                        "stderr":{"head":"","tail":"","dropped_bytes":null}
                    }),
                };
                (outcome, metadata, value, None, quarantine)
            }
            #[cfg(test)]
            Report::FakeForTest { stop, cleanup } => (
                Outcome::Failed,
                Metadata {
                    stop_reason: Some(stop),
                    cleanup_confirmed: Some(cleanup),
                    reason: (!cleanup).then_some(Reason::CleanupUnconfirmed),
                    ..Default::default()
                },
                json!({"test_outcome":"FakeForTest","acceptance":false}),
                None,
                !cleanup,
            ),
        };
        metadata.identity_mode = Some(IdentityMode::DedicatedAccount);
        if quarantine {
            self.quarantined.store(true, Ordering::Release);
        }
        if let Some(r) = request {
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
                value["observed_identity"] = if metadata.observed_kind.is_some() {
                    json!({"kind":metadata.observed_kind,"id":metadata.observed_id,"gid":metadata.observed_gid,"integrity_rid":metadata.integrity_rid,"elevation":metadata.elevation,"observation_point":metadata.observation_point})
                } else {
                    Value::Null
                };
                value["metadata"] = serde_json::to_value(metadata).unwrap();
                Ok(value)
            });
        if result
            .as_ref()
            .is_err_and(|e| e.reason == Reason::StorageFailed)
        {
            self.audit_failed.store(true, Ordering::Release);
        }
        result
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Creation {
    NotInvoked,
    ProvenNone,
    Created,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationType {
    Default,
    Full,
    Limited,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupState {
    Absent,
    DenyOnly,
    Enabled,
    PresentOther,
}
#[derive(Debug, Default, Clone)]
pub struct Evidence {
    pub observation: Option<Observation>,
    pub point: Option<ObservationPoint>,
    pub stop: Option<StopReason>,
    pub exit_code: Option<i32>,
    pub tree_boundary: Option<TreeBoundary>,
    pub cleanup_confirmed: Option<bool>,
}
#[derive(Clone)]
pub enum Observation {
    Windows {
        sid: String,
        integrity_rid: u32,
        elevated: bool,
        elevation_type: ElevationType,
        administrators: GroupState,
    },
    Unix {
        uid: u32,
        gid: u32,
    },
}
impl std::fmt::Debug for Observation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Windows {
                integrity_rid,
                elevated,
                elevation_type,
                administrators,
                ..
            } => f
                .debug_struct("Windows")
                .field("sid", &"<withheld>")
                .field("integrity_rid", integrity_rid)
                .field("elevated", elevated)
                .field("elevation_type", elevation_type)
                .field("administrators", administrators)
                .finish(),
            Self::Unix { .. } => f
                .debug_struct("Unix")
                .field("uid", &"<withheld>")
                .field("gid", &"<withheld>")
                .finish(),
        }
    }
}
pub(crate) struct Pin {
    platform: Platform,
    identity: String,
    gid: Option<u32>,
    rights: Rights,
}
impl Pin {
    fn new(
        platform: Platform,
        identity: String,
        gid: Option<u32>,
        rights: Rights,
    ) -> Result<Self, Unavailable> {
        let shape = match (platform, rights) {
            (Platform::Macos, Rights::Admin) => {
                return Err(Unavailable::AdminUnsupportedOnPlatform);
            }
            (Platform::Windows, Rights::Standard | Rights::Admin) => {
                gid.is_none()
                    && crate::store::observed_id_valid(ObservedKind::WindowsSid, &identity)
            }
            (Platform::Linux, Rights::Standard | Rights::Admin)
            | (Platform::Macos, Rights::Standard) => {
                gid.is_some() && crate::store::observed_id_valid(ObservedKind::UnixUid, &identity)
            }
        };
        if !shape {
            return Err(Unavailable::PreflightFailed);
        }
        Ok(Self {
            platform,
            identity,
            gid,
            rights,
        })
    }
    fn from_config(
        config: &super::config::ServiceConfig,
        platform: Platform,
    ) -> Result<Self, Unavailable> {
        let rights = match config.rights() {
            super::config::Rights::Standard => Rights::Standard,
            super::config::Rights::Admin => Rights::Admin,
        };
        Self::new(
            platform,
            config.build_identity().into(),
            config.build_gid(),
            rights,
        )
    }
    #[cfg(test)]
    pub(crate) fn for_test(
        platform: Platform,
        identity: &str,
        gid: Option<u32>,
        rights: Rights,
    ) -> Self {
        Self::new(platform, identity.into(), gid, rights)
            .ok()
            .unwrap()
    }
}
/// Compare unredacted facts first; only this sanitized value leaves assessment.
fn assess(
    pin: &Pin,
    evidence: Evidence,
    discovery_valid: Option<bool>,
) -> (Outcome, Metadata, bool) {
    let mut m = Metadata {
        rights: Some(pin.rights),
        identity_mode: Some(IdentityMode::DedicatedAccount),
        stop_reason: evidence.stop,
        exit_code: evidence.exit_code,
        tree_boundary: evidence.tree_boundary,
        cleanup_confirmed: evidence.cleanup_confirmed,
        ..Default::default()
    };
    let point_valid = match (pin.platform, evidence.point) {
        (Platform::Windows, Some(ObservationPoint::PostSwitch))
        | (Platform::Linux | Platform::Macos, Some(ObservationPoint::PreExec)) => true,
        (Platform::Windows, Some(ObservationPoint::PreExec | ObservationPoint::Child) | None)
        | (
            Platform::Linux | Platform::Macos,
            Some(ObservationPoint::PostSwitch | ObservationPoint::Child) | None,
        ) => false,
    };
    if point_valid {
        match (pin.platform, evidence.observation) {
            (
                Platform::Windows,
                Some(Observation::Windows {
                    sid,
                    integrity_rid,
                    elevated,
                    elevation_type,
                    administrators,
                }),
            ) if crate::store::observed_id_valid(ObservedKind::WindowsSid, &sid) => {
                let class = match (pin.rights, elevation_type, administrators) {
                    (Rights::Standard, ElevationType::Default, GroupState::Absent) => {
                        integrity_rid == 8192 && !elevated
                    }
                    (Rights::Admin, ElevationType::Full, GroupState::Enabled) => {
                        integrity_rid == 12288 && elevated
                    }
                    (
                        Rights::Standard,
                        ElevationType::Full | ElevationType::Limited,
                        GroupState::Absent,
                    )
                    | (
                        Rights::Standard,
                        ElevationType::Default | ElevationType::Full | ElevationType::Limited,
                        GroupState::DenyOnly | GroupState::Enabled | GroupState::PresentOther,
                    )
                    | (
                        Rights::Admin,
                        ElevationType::Default | ElevationType::Limited,
                        GroupState::Enabled,
                    )
                    | (
                        Rights::Admin,
                        ElevationType::Default | ElevationType::Full | ElevationType::Limited,
                        GroupState::Absent | GroupState::DenyOnly | GroupState::PresentOther,
                    ) => false,
                };
                m.identity_matches_pin = Some(sid == pin.identity && class);
                m.observed_kind = Some(ObservedKind::WindowsSid);
                m.observed_id = (sid == pin.identity).then_some(sid);
                m.integrity_rid = Some(integrity_rid);
                m.elevation = Some(elevated);
                m.observation_point = evidence.point;
            }
            (Platform::Linux | Platform::Macos, Some(Observation::Unix { uid, gid })) => {
                let id = uid.to_string();
                m.identity_matches_pin = Some(id == pin.identity && Some(gid) == pin.gid);
                m.observed_kind = Some(ObservedKind::UnixUid);
                m.observed_id = (id == pin.identity).then_some(id);
                m.observed_gid = (Some(gid) == pin.gid).then_some(gid);
                m.observation_point = evidence.point;
            }
            (
                Platform::Windows,
                Some(Observation::Windows { .. } | Observation::Unix { .. }) | None,
            )
            | (Platform::Linux | Platform::Macos, Some(Observation::Windows { .. }) | None) => {}
        }
    }
    let boundary_valid = match (pin.platform, evidence.tree_boundary) {
        (Platform::Windows, Some(TreeBoundary::WindowsJob))
        | (Platform::Linux, Some(TreeBoundary::LinuxCgroup))
        | (Platform::Macos, Some(TreeBoundary::MacUidSweep)) => true,
        (
            Platform::Windows,
            Some(
                TreeBoundary::ProcessGroup | TreeBoundary::LinuxCgroup | TreeBoundary::MacUidSweep,
            )
            | None,
        )
        | (
            Platform::Linux,
            Some(TreeBoundary::ProcessGroup | TreeBoundary::WindowsJob | TreeBoundary::MacUidSweep)
            | None,
        )
        | (
            Platform::Macos,
            Some(TreeBoundary::ProcessGroup | TreeBoundary::WindowsJob | TreeBoundary::LinuxCgroup)
            | None,
        ) => false,
    };
    let stop_valid = match evidence.stop {
        Some(StopReason::Exited) => evidence.exit_code.is_some(),
        Some(
            StopReason::Timeout
            | StopReason::Cancelled
            | StopReason::OutputLimit
            | StopReason::Shutdown,
        ) => evidence.exit_code.is_none(),
        Some(StopReason::Failed) | None => false,
    };
    m.reason = if evidence.cleanup_confirmed != Some(true) || !boundary_valid {
        m.cleanup_confirmed = Some(false);
        Some(Reason::CleanupUnconfirmed)
    } else if m.identity_matches_pin == Some(false) {
        Some(Reason::IdentityMismatch)
    } else if m.identity_matches_pin.is_none() || !stop_valid || discovery_valid == Some(false) {
        Some(Reason::ExecutionFailed)
    } else {
        None
    };
    let quarantine = m.reason.is_some();
    let outcome = if !quarantine
        && evidence.stop == Some(StopReason::Exited)
        && evidence.exit_code == Some(0)
    {
        Outcome::Succeeded
    } else {
        Outcome::Failed
    };
    (outcome, m, quarantine)
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

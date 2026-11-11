//! Explicit owner-authorized development execution. This is not owner isolation.
mod types;
pub use types::{Cancellation, CapturedOutput, RunSpec};
#[cfg(windows)]
mod windows;
use crate::store::{IdentityMode, Metadata, Outcome, Reason, StopReason};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[cfg(all(test, windows))]
pub(crate) use windows::tests::{CHILD, fixture_settings};
pub const MODE: &str = "development_current_user";
pub const CLAIM: &str = "development_only_not_dedicated_account";
pub const TOOLS: &[&str] = &[
    "cargo",
    "rustc",
    "rustup",
    "git",
    "python3",
    "python",
    "node",
    "qemu-system-x86_64",
    "qemu-img",
];
const ENVIRONMENT: &[&str] = &[
    "PATH",
    "SystemRoot",
    "TEMP",
    "TMP",
    "CARGO_HOME",
    "RUSTUP_HOME",
];
#[derive(Clone, Deserialize)]
pub enum Mode {
    #[serde(rename = "development_current_user")]
    CurrentUser,
}
#[derive(Clone, Deserialize)]
pub enum Acknowledge {
    #[serde(rename = "shares_owner_user_files_and_credentials")]
    Shared,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub memory_bytes: usize,
    pub pids: u32,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub mode: Mode,
    pub acknowledge: Acknowledge,
    pub workspace: PathBuf,
    pub environment: BTreeMap<String, String>,
    pub limits: Limits,
}
impl Settings {
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::config::checked_path(&self.workspace)?;
        if !self.workspace.is_dir()
            || self.workspace.parent().is_none()
            || !(16777216..=68719476736).contains(&self.limits.memory_bytes)
            || !(1..=1024).contains(&self.limits.pids)
        {
            anyhow::bail!("invalid development configuration");
        }
        if self.environment.len() > 16
            || !self.environment.contains_key("PATH")
            || self.environment.iter().any(|(k, v)| {
                !ENVIRONMENT.contains(&k.as_str()) || v.len() > 4096 || v.contains('\0')
            })
        {
            anyhow::bail!("invalid development environment");
        }
        for dir in self.environment["PATH"].split(';') {
            crate::config::checked_path(Path::new(dir))?;
            if !Path::new(dir).is_dir() {
                anyhow::bail!("invalid tool directory");
            }
        }
        Ok(())
    }
    pub fn disjoint(&self, path: &Path) -> anyhow::Result<()> {
        let a = self
            .workspace
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase();
        let b = path.to_string_lossy().replace('/', "\\").to_lowercase();
        if a == b || a.starts_with(&(b.clone() + "\\")) || b.starts_with(&(a + "\\")) {
            anyhow::bail!("workspace overlaps private state");
        }
        Ok(())
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub tool: String,
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
        TOOLS.contains(&self.tool.as_str())
            && self.args.len() <= 128
            && self
                .args
                .iter()
                .all(|s| s.len() <= 8192 && !s.contains('\0'))
            && self.args.iter().map(String::len).sum::<usize>() <= 65536
            && (1..=3600).contains(&self.timeout_s)
            && (2..=8192).contains(&self.output_bytes)
            && self.cwd.len() <= 4096
            && !self.cwd.contains([':', '\0', '<', '>', '"', '|', '?', '*'])
            && !self.cwd.starts_with(['/', '\\'])
            && self
                .cwd
                .split(['/', '\\'])
                .all(|c| c != ".." && c != "." && !c.ends_with([' ', '.']))
    }
}
#[derive(Serialize)]
pub struct ResultData {
    pub execution_mode: &'static str,
    pub claim: &'static str,
    pub dedicated: bool,
    pub stop_reason: StopReason,
    pub exit_code: Option<i32>,
    pub cleanup_confirmed: bool,
    pub matches_current_user: bool,
    pub stdout: String,
    pub stderr: String,
    /// Bytes read from the pipe but not retained (output past `output_bytes`).
    pub stdout_dropped_bytes: u64,
    pub stderr_dropped_bytes: u64,
}
pub struct Execution {
    #[cfg(all(test, windows))]
    policy_fixture: bool,
    /// Test-only fault: perturbs the in-memory daemon TokenUser copy so the
    /// pre-resume child comparison must fail.
    #[cfg(all(test, windows))]
    tamper_identity: AtomicBool,
    settings: Settings,
    busy: AtomicBool,
    quarantined: AtomicBool,
    shutdown: Cancellation,
}
struct CancelOnDrop(Cancellation);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
type Work = (serde_json::Value, Outcome, Metadata);
impl Execution {
    pub fn new(settings: Settings) -> anyhow::Result<Arc<Self>> {
        settings.validate()?;
        #[cfg(windows)]
        windows::preflight().map_err(|_| anyhow::anyhow!("development preflight refused"))?;
        #[cfg(not(windows))]
        anyhow::bail!("development execution requires Windows");
        #[cfg(windows)]
        {
            Ok(Arc::new(Self::build(settings, false)))
        }
    }
    #[cfg(any(windows, test))]
    fn build(settings: Settings, _fixture: bool) -> Self {
        Self {
            #[cfg(all(test, windows))]
            policy_fixture: _fixture,
            #[cfg(all(test, windows))]
            tamper_identity: AtomicBool::new(false),
            settings,
            busy: AtomicBool::new(false),
            quarantined: AtomicBool::new(false),
            shutdown: Cancellation::default(),
        }
    }
    /// Unit-test-only constructor: skips the token *policy* (hosted runners are
    /// elevated) but keeps the child TokenUser comparison, Job and all bounds.
    /// It does not exist in any non-test build, so no binary or config reaches it.
    #[cfg(test)]
    pub(crate) fn policy_fixture(settings: Settings) -> Self {
        Self::build(settings, true)
    }
    #[cfg(all(test, windows))]
    pub(crate) fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    #[cfg(all(test, windows))]
    pub(crate) fn tamper_identity(&self, on: bool) {
        self.tamper_identity.store(on, Ordering::Release);
    }
    pub async fn drain(&self) {
        for _ in 0..500 {
            if !self.busy.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }
    pub fn capability(&self) -> serde_json::Value {
        let reason = if self.quarantined.load(Ordering::Acquire) {
            Some("quarantined")
        } else if self.shutdown.is_cancelled() {
            Some("shutdown")
        } else {
            None
        };
        serde_json::json!({"available":reason.is_none(),"reason":reason,"mode":MODE,"claim":CLAIM,"dedicated":false,"isolation":"none_shares_owner_user","rights":null,"matches_current_user":true,"integrity":"medium","elevated":false})
    }

    /// The blocking worker is spawned before this returns and owns audit
    /// completion, so dropping the returned future (SDK cancellation, even before
    /// its first poll) cancels the child but still completes the row exactly once.
    pub fn dispatch(
        self: &Arc<Self>,
        request: Option<Request>,
        activity: Arc<crate::store::Activity>,
        id: i64,
    ) -> impl std::future::Future<Output = std::result::Result<serde_json::Value, Reason>> + Send + 'static
    {
        let cancellation = Cancellation::default();
        let guard = CancelOnDrop(cancellation.clone());
        let this = self.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let result = this.work(request.as_ref(), &cancellation);
            match result {
                Ok((value, outcome, meta)) => {
                    activity
                        .finish_typed(id, outcome, &meta)
                        .map_err(|_| Reason::StorageFailed)?;
                    Ok(value)
                }
                Err(reason) => {
                    activity
                        .finish_typed(
                            id,
                            Outcome::Refused,
                            &metadata(request.as_ref(), None, Some(reason)),
                        )
                        .map_err(|_| Reason::StorageFailed)?;
                    Err(reason)
                }
            }
        });
        async move {
            let _guard = guard;
            worker.await.map_err(|_| Reason::ExecutionFailed)?
        }
    }
    fn work(
        &self,
        request: Option<&Request>,
        cancellation: &Cancellation,
    ) -> std::result::Result<Work, Reason> {
        if self.quarantined.load(Ordering::Acquire) {
            return Err(Reason::Quarantined);
        }
        if self.shutdown.is_cancelled() {
            return Err(Reason::Shutdown);
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(Reason::Busy);
        }
        let _busy = Busy(&self.busy);
        let cancelled = || {
            let mut m = metadata(request, None, None);
            m.stop_reason = Some(StopReason::Cancelled);
            Ok((serde_json::Value::Null, Outcome::Failed, m))
        };
        // A request cancelled before its worker ran starts no process.
        if cancellation.is_cancelled() {
            return cancelled();
        }
        if let Some(r) = request {
            let output = self.run(r, cancellation)?;
            let value = serde_json::to_value(&output).map_err(|_| Reason::ExecutionFailed)?;
            return Ok((
                value,
                outcome(&output),
                metadata(Some(r), Some(&output), None),
            ));
        }
        let mut tools = Vec::new();
        for name in TOOLS {
            if cancellation.is_cancelled() {
                return cancelled();
            }
            let probe = Request {
                tool: (*name).into(),
                args: vec!["--version".into()],
                cwd: String::new(),
                timeout_s: 5,
                output_bytes: 4096,
            };
            let entry = match self.run(&probe, cancellation) {
                Ok(r) if r.stop_reason == StopReason::Exited => {
                    serde_json::json!({"name":name,"found":true,"version":r.stdout.chars().take(256).collect::<String>()})
                }
                Ok(_) => serde_json::json!({"name":name,"found":true,"version":null}),
                Err(Reason::Unavailable) => {
                    serde_json::json!({"name":name,"found":false,"version":null})
                }
                Err(reason) => return Err(reason),
            };
            tools.push(entry);
        }
        Ok((
            serde_json::json!({"execution_mode":MODE,"claim":CLAIM,"tools":tools}),
            Outcome::Succeeded,
            metadata(None, None, None),
        ))
    }
    #[cfg_attr(not(windows), allow(unused_variables))]
    fn run(
        &self,
        r: &Request,
        cancellation: &Cancellation,
    ) -> std::result::Result<ResultData, Reason> {
        #[cfg(windows)]
        {
            windows::run(self, r, cancellation)
        }
        #[cfg(not(windows))]
        {
            let _ = &self.settings;
            Err(Reason::Unavailable)
        }
    }
}
pub fn metadata(
    request: Option<&Request>,
    result: Option<&ResultData>,
    reason: Option<Reason>,
) -> Metadata {
    let mut m = Metadata {
        identity_mode: Some(IdentityMode::DevelopmentCurrentUser),
        reason,
        ..Default::default()
    };
    if let Some(r) = request {
        m.argc = Some(r.args.len().min(128) as u16);
        m.program = serde_json::from_value(serde_json::json!(r.tool)).ok();
    }
    if let Some(r) = result {
        m.stop_reason = Some(r.stop_reason);
        m.exit_code = r.exit_code;
        m.cleanup_confirmed = Some(r.cleanup_confirmed);
        m.identity_matches_pin = Some(r.matches_current_user);
        m.integrity_rid = Some(8192);
        m.elevation = Some(false);
        m.observation_point = Some(crate::store::ObservationPoint::PreExec);
        m.tree_boundary = Some(crate::store::TreeBoundary::WindowsJob);
    }
    m
}
pub fn outcome(r: &ResultData) -> Outcome {
    if r.stop_reason == StopReason::Exited && r.exit_code == Some(0) {
        Outcome::Succeeded
    } else {
        Outcome::Failed
    }
}
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in s.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n('\\', slashes));
        }
        out.push(c);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}
/// CreateProcessW requires the block sorted case-insensitively by name.
pub fn environment(settings: &Settings) -> Vec<u16> {
    let mut pairs = settings.environment.iter().collect::<Vec<_>>();
    pairs.sort_by_key(|(k, _)| k.to_uppercase());
    let mut v = Vec::new();
    for (k, x) in pairs {
        v.extend(format!("{k}={x}").encode_utf16());
        v.push(0);
    }
    v.push(0);
    v
}
#[cfg(windows)]
fn redact(text: String, settings: &Settings) -> String {
    let mut pairs = vec![(
        settings.workspace.to_string_lossy().into_owned(),
        "<workspace>",
    )];
    for (k, label) in [
        ("CARGO_HOME", "<cargo_home>"),
        ("RUSTUP_HOME", "<rustup_home>"),
        ("TEMP", "<temp>"),
        ("TMP", "<temp>"),
    ] {
        if let Some(v) = settings.environment.get(k) {
            pairs.push((v.clone(), label));
        }
    }
    // Best effort only: these values are never inserted into the child environment.
    #[cfg(windows)]
    if let Some(profile) = windows::profile() {
        pairs.push((profile, "<profile>"));
    }
    for (k, label) in [("USERPROFILE", "<profile>"), ("USERNAME", "<user>")] {
        if let Ok(v) = std::env::var(k) {
            pairs.push((v, label));
        }
    }
    redact_with(&text, pairs)
}
/// Longest needle first; ASCII case-insensitive with either slash. A single
/// forward scan per needle, so a label that contains its needle (a user named
/// `user`) can never be matched again or loop.
#[cfg(any(windows, test))]
fn redact_with(text: &str, mut pairs: Vec<(String, &str)>) -> String {
    let normal = |s: &str| s.replace('/', "\\").to_ascii_lowercase();
    pairs.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
    let mut text = text.to_owned();
    for (needle, label) in pairs {
        let needle = normal(&needle);
        if needle.is_empty() {
            continue;
        }
        // Normalization only maps ASCII bytes to ASCII bytes, so offsets in the
        // normalized haystack are char boundaries of the original text.
        let hay = normal(&text);
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for (i, _) in hay.match_indices(&needle) {
            out.push_str(&text[last..i]);
            out.push_str(label);
            last = i + needle.len();
        }
        out.push_str(&text[last..]);
        text = out;
    }
    redact_sids(&text)
}
/// Replace every SID-shaped run (`S-1-` followed by digits and dashes),
/// anywhere in the text, keeping the surrounding characters.
#[cfg(any(windows, test))]
fn redact_sids(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let b = lower.as_bytes();
    let (mut out, mut last, mut i) = (String::with_capacity(text.len()), 0, 0);
    while i + 4 < b.len() {
        if &b[i..i + 4] == b"s-1-" && b[i + 4].is_ascii_digit() {
            let mut j = i + 4;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'-') {
                j += 1;
            }
            out.push_str(&text[last..i]);
            out.push_str("<sid>");
            last = j;
            i = j;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[last..]);
    out
}
#[cfg(test)]
mod tests;

// Budget the inner JSON string, so the SDK's outer text JSON still fits 64 KiB.
#[cfg(any(windows, test))]
fn bounded_text(text: String, budget: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            let n = match c {
                '"' | '\\' | '\n' | '\r' | '\t' => 2,
                c if (*c as u32) < 32 => 6,
                c => c.len_utf8(),
            };
            used += n;
            used <= budget
        })
        .collect()
}

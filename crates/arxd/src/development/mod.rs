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
                ![
                    "PATH",
                    "SystemRoot",
                    "TEMP",
                    "TMP",
                    "CARGO_HOME",
                    "RUSTUP_HOME",
                ]
                .contains(&k.as_str())
                    || v.len() > 4096
                    || v.contains('\0')
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
}
pub struct Execution {
    #[cfg(test)]
    policy_fixture: bool,
    settings: Settings,
    busy: AtomicBool,
    quarantined: AtomicBool,
    shutdown: Cancellation,
}
impl Execution {
    pub fn new(settings: Settings) -> anyhow::Result<Arc<Self>> {
        settings.validate()?;
        #[cfg(windows)]
        windows::preflight().map_err(|_| anyhow::anyhow!("development preflight refused"))?;
        #[cfg(not(windows))]
        anyhow::bail!("development execution requires Windows");
        #[cfg(windows)]
        {
            Ok(Arc::new(Self {
                #[cfg(test)]
                policy_fixture: false,
                settings,
                busy: AtomicBool::new(false),
                quarantined: AtomicBool::new(false),
                shutdown: Cancellation::default(),
            }))
        }
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
        serde_json::json!({"available":!self.quarantined.load(Ordering::Acquire) && !self.shutdown.is_cancelled(),"reason":if self.quarantined.load(Ordering::Acquire) {Some("quarantined")} else {None},"mode":MODE,"claim":CLAIM,"dedicated":false,"isolation":"none_shares_owner_user","rights":null,"matches_current_user":true,"integrity":"medium","elevated":false})
    }

    /// The blocking worker owns audit completion, even if the SDK drops its future.
    pub async fn dispatch(
        self: &Arc<Self>,
        request: Option<Request>,
        activity: Arc<crate::store::Activity>,
        id: i64,
    ) -> std::result::Result<serde_json::Value, Reason> {
        let cancellation = Cancellation::default();
        struct Cancel(Cancellation);
        impl Drop for Cancel {
            fn drop(&mut self) {
                self.0.cancel();
            }
        }
        let _cancel = Cancel(cancellation.clone());
        let this = self.clone();
        tokio::task::spawn_blocking(move||{
            let result=(||{
                if this.quarantined.load(Ordering::Acquire){return Err(Reason::Quarantined);}
                if this.shutdown.is_cancelled(){return Err(Reason::Shutdown);}
                if this.busy.swap(true,Ordering::AcqRel){return Err(Reason::Busy);}
                struct Busy<'a>(&'a AtomicBool);impl Drop for Busy<'_>{fn drop(&mut self){self.0.store(false,Ordering::Release);}}
                let _busy=Busy(&this.busy);
                let run=|r:&Request| {
                    #[cfg(windows)] {windows::run(&this,r,&cancellation)}
                    #[cfg(not(windows))] {let _=(&this.settings,r,&cancellation);Err::<ResultData,_>(Reason::Unavailable)}
                };
                if let Some(r)=&request {
                    let output=run(r)?;
                    let value=serde_json::to_value(&output).map_err(|_|Reason::ExecutionFailed)?;
                    return Ok((value,outcome(&output),metadata(Some(r),Some(&output),None)));
                }
                let mut tools=Vec::new();
                for name in TOOLS {
                    if cancellation.is_cancelled(){return Ok((serde_json::Value::Null,Outcome::Failed,Metadata{stop_reason:Some(StopReason::Cancelled),..metadata(None,None,None)}));}
                    match run(&Request{tool:(*name).into(),args:vec!["--version".into()],cwd:String::new(),timeout_s:5,output_bytes:4096}) {
                        Ok(r)=>{
                            if r.stop_reason!=StopReason::Exited {return Ok((serde_json::Value::Null,Outcome::Failed,metadata(None,Some(&r),None)));}
                            tools.push(serde_json::json!({"name":name,"found":true,"version":r.stdout.chars().take(256).collect::<String>()}));
                        }
                        Err(Reason::Unavailable)=>tools.push(serde_json::json!({"name":name,"found":false,"version":null})),
                        Err(reason)=>return Err(reason),
                    }
                }
                Ok((serde_json::json!({"execution_mode":MODE,"claim":CLAIM,"tools":tools}),Outcome::Succeeded,metadata(None,None,None)))
            })();
            match result {
                Ok((value,outcome,meta))=>{activity.finish_typed(id,outcome,&meta).map_err(|_|Reason::StorageFailed)?;Ok(value)}
                Err(reason)=>{activity.finish_typed(id,Outcome::Refused,&metadata(request.as_ref(),None,Some(reason))).map_err(|_|Reason::StorageFailed)?;Err(reason)}
            }
        }).await.map_err(|_|Reason::ExecutionFailed)?
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
pub fn environment(settings: &Settings) -> Vec<u16> {
    let mut v = Vec::new();
    for (k, x) in &settings.environment {
        v.extend(format!("{k}={x}").encode_utf16());
        v.push(0);
    }
    v.push(0);
    v
}
fn redact(mut text: String, settings: &Settings) -> String {
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
    for (k, label) in [("USERPROFILE", "<profile>"), ("USERNAME", "<user>")] {
        if let Ok(v) = std::env::var(k) {
            pairs.push((v, label));
        }
    }
    pairs.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
    for (needle, label) in pairs {
        if needle.is_empty() {
            continue;
        }
        loop {
            let hay = text.replace('/', "\\").to_ascii_lowercase();
            let needle = needle.replace('/', "\\").to_ascii_lowercase();
            let Some(i) = hay.find(&needle) else {
                break;
            };
            text.replace_range(i..i + needle.len(), label);
        }
    }
    // Redact SID-shaped words even if emitted by the command itself.
    text.split_inclusive(char::is_whitespace)
        .map(|word| {
            if word.to_ascii_lowercase().starts_with("s-1-") {
                "<sid> ".to_string()
            } else {
                word.to_string()
            }
        })
        .collect()
}
#[cfg(test)]
mod tests;

// Budget the inner JSON string, so the SDK's outer text JSON still fits 64 KiB.
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

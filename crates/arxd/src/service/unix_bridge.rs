//! Inactive ADR0019 adapter. Construction and injection exist only in tests.
#![allow(dead_code)]
use super::executor::{Creation, Evidence, Executor, Observation, Report, Request, ToolVersion};
use super::{
    config::{Rights, ServiceConfig},
    layout::{Layout, Platform},
    preflight::ExecutionUnavailable as Unavailable,
};
use crate::{
    development::{Cancellation, CapturedOutput, TOOLS},
    exec,
    store::{ObservationPoint, StopReason, TreeBoundary},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    time::Duration,
};

#[cfg(test)]
use crate::exec::hierarchy::locate;

trait RunnerPort: Send + Sync {
    fn run(
        &self,
        spec: &exec::RunSpec,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Result<exec::RunResult, exec::RunError>;
}
impl RunnerPort for exec::Runner {
    fn run(
        &self,
        spec: &exec::RunSpec,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Result<exec::RunResult, exec::RunError> {
        self.run_phased(spec, cancel, shutdown)
    }
}
struct UnixBridge {
    binary: PathBuf,
    runner: Result<Box<dyn RunnerPort>, Unavailable>,
    check: Box<dyn Fn() -> Result<(), Unavailable> + Send + Sync>,
    ready: bool,
    #[cfg(test)]
    seam: u8,
}
impl UnixBridge {
    #[cfg(test)]
    fn new(
        layout: &Layout,
        config: &ServiceConfig,
        parent: PathBuf,
        factory: impl FnOnce(exec::OwnerConfig) -> Result<Box<dyn RunnerPort>, Unavailable>,
        check: Box<dyn Fn() -> Result<(), Unavailable> + Send + Sync>,
        ready: bool,
    ) -> Self {
        let runner = derive(layout, config, parent).and_then(factory);
        Self {
            binary: layout.binary().into(),
            runner,
            check,
            ready,
            seam: 0,
        }
    }
    fn body(
        &self,
        request: Option<&Request>,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Report {
        let spec = spec(&self.binary, request);
        let runner = match &self.runner {
            Ok(r) => r,
            Err(r) => return Report::Unavailable(*r),
        };
        match runner.run(&spec, cancel, shutdown) {
            Err(error) => failure(error),
            Ok(result) => {
                #[cfg(test)]
                if self.seam == 1 {
                    panic!("ARX-T10-MAPPING-PANIC");
                }
                let evidence = evidence(&result);
                if request.is_none() {
                    #[cfg(test)]
                    if self.seam == 2 {
                        panic!("ARX-T10-DISCOVERY-PANIC");
                    }
                    Report::Discovered {
                        evidence,
                        tools: parse_tools(&result).unwrap_or_default(),
                    }
                } else {
                    Report::Completed {
                        evidence,
                        stdout: capture(result.stdout),
                        stderr: capture(result.stderr),
                    }
                }
            }
        }
    }
}
impl Executor for UnixBridge {
    fn preflight(&self) -> Result<(), Unavailable> {
        (self.check)()?;
        self.runner.as_ref().map_err(|r| *r)?;
        if !self.ready {
            return Err(Unavailable::MechanismUnavailable);
        }
        Ok(())
    }
    fn run(
        &self,
        request: Option<&Request>,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Report {
        match catch_unwind(AssertUnwindSafe(|| self.body(request, cancel, shutdown))) {
            Ok(report) => report,
            Err(_payload) => Report::Failed {
                creation: Creation::Unknown,
            },
        }
    }
}
// No filesystem or environment observation: constraints match Runner's owner policy.
fn derive(
    layout: &Layout,
    config: &ServiceConfig,
    parent: PathBuf,
) -> Result<exec::OwnerConfig, Unavailable> {
    if layout.platform() != Platform::Linux || config.rights() != Rights::Standard {
        return Err(Unavailable::MechanismUnavailable);
    }
    let uid = config
        .build_identity()
        .parse::<u32>()
        .map_err(|_| Unavailable::PreflightFailed)?;
    let gid = config.build_gid().ok_or(Unavailable::PreflightFailed)?;
    let (_, memory_bytes, pids) = config.limits();
    let account = config.build_name();
    let workspace = config.workspace();
    if uid == 0
        || gid == 0
        || account.is_empty()
        || account.len() > 128
        || !account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || !clean_absolute(workspace)
        || workspace == "/"
        || !(16 * 1024 * 1024..=64 * 1024 * 1024 * 1024).contains(&memory_bytes)
        || !(1..=1024).contains(&pids)
    {
        return Err(Unavailable::PreflightFailed);
    }
    Ok(exec::OwnerConfig {
        account: account.into(),
        uid,
        gid,
        workspace: workspace.into(),
        protected_directories: layout.private_dirs().to_vec(),
        environment: config.environment().clone(),
        admin: false,
        cgroup_parent: Some(parent),
        memory_bytes,
        pids,
    })
}
fn clean_absolute(text: &str) -> bool {
    text.len() <= 4096
        && text.starts_with('/')
        && !text.starts_with("//")
        && !text.contains(['\0', '\\'])
        && text.split('/').all(|s| s != "." && s != "..")
}
fn spec(binary: &std::path::Path, request: Option<&Request>) -> exec::RunSpec {
    let (arguments, timeout, output_bytes) = match request {
        None => (vec!["tool-probe".into()], 60, 4096),
        Some(r) => {
            let (mode, target) = match (&r.tool, &r.executable) {
                (Some(t), None) => ("tool", t.as_str()),
                (None, Some(p)) => ("path", p.as_str()),
                _ => ("invalid", ""),
            };
            let mut args = vec![
                "exec-child".into(),
                mode.into(),
                target.into(),
                r.cwd.clone(),
            ];
            args.extend(r.args.iter().cloned());
            (args, r.timeout_s, r.output_bytes)
        }
    };
    exec::RunSpec {
        executable: binary.into(),
        arguments,
        cwd: PathBuf::new(),
        timeout: Duration::from_secs(timeout),
        output_bytes,
    }
}
fn failure(error: exec::RunError) -> Report {
    use exec::{NotStarted as N, RunError as E};
    match error {
        E::NotStarted(N::Invalid | N::Resource) => Report::Failed {
            creation: Creation::NotInvoked,
        },
        E::NotStarted(N::Cancelled) => Report::CancelledBeforeSpawn,
        E::NotStarted(N::Shutdown) => Report::Unavailable(Unavailable::Stopping),
        E::NotStarted(N::Unsupported) => Report::Unavailable(Unavailable::PlatformUnsupported),
        E::NotStarted(N::ServiceIdentity | N::Containment) => {
            Report::Unavailable(Unavailable::MechanismUnavailable)
        }
        E::NotStarted(N::AccountChanged) => {
            Report::Unavailable(Unavailable::AccountIdentityChanged)
        }
        E::NotStarted(N::AccountPolicy | N::PrivateRoots) => {
            Report::Unavailable(Unavailable::PreflightFailed)
        }
        E::SpawnFailed => Report::Failed {
            creation: Creation::Unknown,
        },
        E::AfterSpawn => Report::Failed {
            creation: Creation::Created,
        },
    }
}
fn evidence(r: &exec::RunResult) -> Evidence {
    Evidence {
        observation: Some(Observation::Unix {
            uid: r.identity.uid,
            gid: r.identity.gid,
        }),
        point: Some(ObservationPoint::PreExec),
        stop: Some(match r.outcome {
            exec::Outcome::Exited => StopReason::Exited,
            exec::Outcome::TimedOut => StopReason::Timeout,
            exec::Outcome::Cancelled => StopReason::Cancelled,
            exec::Outcome::Shutdown => StopReason::Shutdown,
        }),
        exit_code: if r.outcome == exec::Outcome::Exited {
            r.exit_code
        } else {
            None
        },
        tree_boundary: Some(match r.tree_boundary {
            exec::TreeBoundary::LinuxCgroupV2 => TreeBoundary::LinuxCgroup,
            exec::TreeBoundary::MacProcessGroupBestEffort => TreeBoundary::ProcessGroup,
        }),
        cleanup_confirmed: Some(r.tree_cleanup_confirmed),
    }
}
fn capture(c: exec::CapturedOutput) -> CapturedOutput {
    CapturedOutput {
        head: c.head,
        tail: c.tail,
        dropped_bytes: c.dropped_bytes,
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeRow {
    name: String,
    found: bool,
    version: Option<String>,
}
fn parse_tools(result: &exec::RunResult) -> Option<Vec<ToolVersion>> {
    if result.outcome != exec::Outcome::Exited
        || result.exit_code != Some(0)
        || result.stdout.dropped_bytes != 0
    {
        return None;
    }
    let mut bytes = result.stdout.head.clone();
    bytes.extend_from_slice(&result.stdout.tail);
    let text = std::str::from_utf8(&bytes).ok()?;
    let lines: Vec<_> = text.lines().collect();
    if lines.len() != TOOLS.len() {
        return None;
    }
    let mut tools = Vec::new();
    for (line, expected) in lines.into_iter().zip(TOOLS) {
        // Value's key set makes the nullable version field mandatory too.
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        let keys = value.as_object()?;
        if keys.len() != 3
            || !["name", "found", "version"]
                .iter()
                .all(|k| keys.contains_key(*k))
        {
            return None;
        }
        let row: ProbeRow = serde_json::from_str(line).ok()?;
        if row.name != *expected
            || (!row.found && row.version.is_some())
            || row.version.as_ref().is_some_and(|v| {
                v.is_empty()
                    || v.len() > 128
                    || !v
                        .bytes()
                        .all(|b| (32..=126).contains(&b) && b != b'/' && b != b'\\')
            })
        {
            return None;
        }
        tools.push(ToolVersion {
            name: row.name,
            found: row.found,
            version: row.version,
        });
    }
    Some(tools)
}
#[cfg(test)]
mod tests;

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

mod readiness;
use crate::exec::hierarchy::parse::{Hardening, RightsFact, classify_rights, rights_line};
use readiness::{Cause, Readiness};

trait RunnerPort: Send + Sync {
    fn cleanup_residue(&self) -> bool;
    fn run(
        &self,
        spec: &exec::RunSpec,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Result<exec::RunResult, exec::RunError>;
}
impl RunnerPort for exec::Runner {
    fn cleanup_residue(&self) -> bool {
        self.cleanup_residue()
    }
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
    readiness: Readiness,
    #[cfg(test)]
    seam: u8,
}
impl UnixBridge {
    #[cfg(test)]
    fn new(
        layout: &Layout,
        config: &ServiceConfig,
        parent: Option<PathBuf>,
        factory: impl FnOnce(exec::OwnerConfig) -> Result<Box<dyn RunnerPort>, Unavailable>,
        check: Box<dyn Fn() -> Result<(), Unavailable> + Send + Sync>,
        readiness: Readiness,
    ) -> Self {
        let runner = parent
            .ok_or(Unavailable::MechanismUnavailable)
            .and_then(|parent| derive(layout, config, parent))
            .and_then(factory);
        Self {
            binary: layout.binary().into(),
            runner,
            check,
            readiness,
            seam: 0,
        }
    }
    fn body(
        &self,
        request: Option<&Request>,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Report {
        let runner = match &self.runner {
            Ok(r) => r,
            Err(r) => return Report::Unavailable(*r),
        };
        if runner.cleanup_residue() {
            self.readiness.latch(Cause::Containment);
        }
        if let Err(token) = self.readiness.check() {
            return Report::Unavailable(token);
        }
        let spec = spec(&self.binary, request);
        let outcome = runner.run(&spec, cancel, shutdown);
        if runner.cleanup_residue()
            || matches!(
                outcome,
                Err(exec::RunError::NotStarted(
                    exec::NotStarted::Containment | exec::NotStarted::Resource
                ))
            )
        {
            self.readiness.latch(Cause::Containment);
        }
        match outcome {
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
                    let tools = match parse_tools(&result) {
                        Some((tools, fact)) => {
                            if classify_rights(Rights::Standard, Hardening::Standard, fact).is_err()
                            {
                                self.readiness.latch(Cause::Rights);
                            }
                            tools
                        }
                        None => Vec::new(),
                    };
                    Report::Discovered { evidence, tools }
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
        self.readiness.check()
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
fn parse_tools(result: &exec::RunResult) -> Option<(Vec<ToolVersion>, RightsFact)> {
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
    if lines.len() != TOOLS.len() + 1 {
        return None;
    }
    let fact = rights_line(lines.last()?)?;
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
    Some((tools, fact))
}
#[cfg(test)]
mod tests;

// Inactive owned-subtree wiring. Readiness is allocated here, never supplied by
// another domain. The setup closure and runner factory are each consumed once.
#[cfg(test)]
struct StartupDomain {
    domain: AdmissionDomain,
    setup: std::sync::Mutex<Option<StartupSetup>>,
    readiness: Readiness,
}
#[cfg(test)]
type StartupSetup = Box<dyn FnOnce(Cancellation) -> bool + Send>;
#[cfg(test)]
impl StartupDomain {
    fn new(
        layout: &Layout,
        config: &ServiceConfig,
        location: Option<exec::hierarchy::parse::Location>,
        check: impl Fn() -> Result<(), Unavailable> + Send + Sync + 'static,
        factory: impl FnOnce(exec::OwnerConfig) -> Result<Box<dyn RunnerPort>, Unavailable>,
        setup: impl FnOnce(
            &exec::hierarchy::parse::Location,
            &Cancellation,
        ) -> exec::hierarchy::setup::SetupOutcome
        + Send
        + 'static,
    ) -> Self {
        let readiness = Readiness::pending();
        if location.is_none() {
            readiness.fail(Cause::Setup);
        }
        let state = readiness.clone();
        let constructed = std::cell::Cell::new(false);
        let domain = AdmissionDomain::new(
            layout,
            config,
            location.as_ref().map(|location| location.runs()),
            |owner| {
                let runner = factory(owner);
                constructed.set(runner.is_ok());
                runner
            },
            Box::new(check),
            readiness,
        );
        let valid = constructed.get();
        Self {
            domain,
            readiness: state.clone(),
            setup: std::sync::Mutex::new(location.map(|location| {
                Box::new(move |stop| {
                    if !valid {
                        state.fail(Cause::Setup);
                        return false;
                    }
                    match setup(&location, &stop) {
                        exec::hierarchy::setup::SetupOutcome::Ready(_) if !stop.is_cancelled() => {
                            state.set_ready()
                        }
                        exec::hierarchy::setup::SetupOutcome::Ready(_) => {
                            state.fail(Cause::Stopped)
                        }
                        exec::hierarchy::setup::SetupOutcome::Unavailable(f) => {
                            state.fail(f.into())
                        }
                    }
                    state.check().is_ok()
                }) as StartupSetup
            })),
        }
    }
}

// The inactive wiring owner retains the sole domain. Handles clone Execution,
// including its backend, readiness and slot counter, without another factory call.
#[cfg(test)]
struct AdmissionDomain {
    execution: std::sync::Arc<super::executor::Execution>,
}
#[cfg(test)]
impl AdmissionDomain {
    fn new(
        layout: &Layout,
        config: &ServiceConfig,
        parent: Option<PathBuf>,
        factory: impl FnOnce(exec::OwnerConfig) -> Result<Box<dyn RunnerPort>, Unavailable>,
        check: Box<dyn Fn() -> Result<(), Unavailable> + Send + Sync>,
        readiness: Readiness,
    ) -> Self {
        let bridge = UnixBridge::new(layout, config, parent, factory, check, readiness);
        Self {
            execution: super::executor::Execution::unix_bridge_for_test(
                std::sync::Arc::new(bridge),
                config,
            ),
        }
    }
    fn handle(&self) -> std::sync::Arc<super::executor::Execution> {
        self.execution.clone()
    }
}

//! Standard Linux execution adapter for the private build broker.
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
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(target_os = "linux")]
use super::cluster_build_executor::{BrokerRefusal, PolicyCheck};
#[cfg(test)]
use crate::exec::hierarchy::locate;

mod readiness;
use crate::exec::hierarchy::parse::{Hardening, RightsFact, classify_rights, rights_line};
use readiness::{Cause, Readiness};

#[cfg(target_os = "linux")]
pub(super) fn broker(
    paths: (PathBuf, PathBuf),
    config: &ServiceConfig,
    check: impl Fn() -> PolicyCheck + Send + Sync + 'static,
) -> Result<UnixBridge, BrokerRefusal> {
    use crate::exec::hierarchy::{
        linux::LinuxFs,
        parse,
        setup::{self, CgroupFs, PROC_MAGIC, SetupOutcome},
    };
    struct Monotonic(std::time::Instant);
    impl setup::Clock for Monotonic {
        fn now(&self) -> Duration {
            self.0.elapsed()
        }
        fn sleep(&self, duration: Duration) {
            std::thread::sleep(duration);
        }
    }
    let fs = LinuxFs::system();
    let proc = fs.proc_cgroup().map_err(|_| BrokerRefusal::Membership)?;
    let location = parse::locate(&proc.bytes)
        .filter(|_| proc.magic == PROC_MAGIC)
        .ok_or(BrokerRefusal::Membership)?;
    let readiness = Readiness::pending();
    let layout = Layout::system().map_err(|_| BrokerRefusal::Layout)?;
    let mut bridge = UnixBridge::with_paths(
        (paths.0, vec![paths.1]),
        &layout,
        config,
        Some(location.runs()),
        |owner| {
            exec::Runner::new(owner)
                .map(|runner| Box::new(runner) as Box<dyn RunnerPort>)
                .map_err(|_| Unavailable::PreflightFailed)
        },
        Box::new(|| Err(Unavailable::PreflightFailed)),
        readiness.clone(),
    );
    bridge
        .runner
        .as_ref()
        .map_err(|_| BrokerRefusal::RunnerConstruction)?;
    check().map_err(|(category, _)| category)?;
    // Refuse pre-existing residue before setup's janitor can change it.
    let runs = location.runs();
    if !runs.parent().is_some_and(cgroup_mounts) {
        return Err(BrokerRefusal::HierarchyWritableView);
    }
    match runs.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(BrokerRefusal::PreExistingRuns),
    }
    match setup::setup(
        &fs,
        &location,
        &Monotonic(std::time::Instant::now()),
        &Cancellation::default(),
    ) {
        SetupOutcome::Ready(_) => readiness.set_ready(),
        SetupOutcome::Unavailable(failure) => {
            readiness.fail(failure.into());
            return Err(BrokerRefusal::HierarchySetup);
        }
    }
    if !cgroup_mounts(&runs) {
        return Err(BrokerRefusal::HierarchyWritableView);
    }
    let chain = cgroup_chain(&runs).ok_or(BrokerRefusal::HierarchyPin)?;
    bridge.check = Box::new(move || {
        check().map_err(|(_, unavailable)| unavailable)?;
        if !cgroup_mounts(&runs) || cgroup_chain(&runs).as_ref() != Some(&chain) {
            return Err(Unavailable::MechanismUnavailable);
        }
        Ok(())
    });
    Ok(bridge)
}

#[cfg(target_os = "linux")]
fn cgroup_mounts(runs: &std::path::Path) -> bool {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let read_only = |path: &std::path::Path| {
        let name = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: statvfs initializes the pointed-to record on success.
        unsafe {
            (libc::statvfs(name.as_ptr(), stat.as_mut_ptr()) == 0)
                .then(|| stat.assume_init().f_flag & libc::ST_RDONLY != 0)
        }
    };
    read_only(std::path::Path::new("/sys/fs/cgroup")) == Some(true)
        && read_only(runs) == Some(false)
}

#[cfg(target_os = "linux")]
fn cgroup_chain(path: &std::path::Path) -> Option<Vec<(u64, u64, u32, u32)>> {
    use std::os::unix::fs::MetadataExt;
    let mut ancestors = Vec::new();
    for path in path.ancestors() {
        let metadata = path.symlink_metadata().ok()?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return None;
        }
        ancestors.push((
            metadata.dev(),
            metadata.ino(),
            metadata.uid(),
            metadata.mode(),
        ));
    }
    Some(ancestors)
}

#[cfg(target_os = "linux")]
pub(super) fn drained(bridge: &UnixBridge) -> bool {
    use crate::exec::hierarchy::{
        linux::LinuxFs,
        parse::{self, Entry},
        setup::CgroupFs,
    };
    let fs = LinuxFs::system();
    let inspect = || {
        let location = parse::locate(&fs.proc_cgroup().ok()?.bytes)?;
        let mut directory = fs.open_root().ok()?;
        for component in location.unit_components() {
            directory = fs.open_child(&directory, component.as_bytes()).ok()?;
        }
        let runs = fs.open_child(&directory, b"runs").ok()?;
        let events = fs.read(&runs, "cgroup.events", 4096).ok()?;
        Some(
            parse::events(std::str::from_utf8(&events).ok()?) == Some(false)
                && fs.list(&runs, 512).ok()?.iter().all(|entry| {
                    matches!(
                        parse::classify_entry(&entry.name, entry.d_type, entry.fstatat_mode),
                        Entry::Dot | Entry::Control
                    )
                }),
        )
    };
    bridge
        .runner
        .as_ref()
        .is_ok_and(|runner| !runner.cleanup_residue())
        && inspect() == Some(true)
}

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
pub(super) struct UnixBridge {
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
        Self::with_paths(
            (layout.binary().into(), layout.private_dirs().to_vec()),
            layout,
            config,
            parent,
            factory,
            check,
            readiness,
        )
    }

    #[cfg(any(test, target_os = "linux"))]
    fn with_paths(
        paths: (PathBuf, Vec<PathBuf>),
        layout: &Layout,
        config: &ServiceConfig,
        parent: Option<PathBuf>,
        factory: impl FnOnce(exec::OwnerConfig) -> Result<Box<dyn RunnerPort>, Unavailable>,
        check: Box<dyn Fn() -> Result<(), Unavailable> + Send + Sync>,
        readiness: Readiness,
    ) -> Self {
        let (binary, protected_directories) = paths;
        let runner = parent
            .ok_or(Unavailable::MechanismUnavailable)
            .and_then(|parent| derive_with_roots(layout, config, parent, protected_directories))
            .and_then(factory);
        Self {
            binary,
            runner,
            check,
            readiness,
            #[cfg(test)]
            seam: 0,
        }
    }
    fn body(
        &self,
        request: Option<&Request>,
        cancel: &Cancellation,
        shutdown: &Cancellation,
        slot: Option<&Path>,
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
        let mut spec = spec(&self.binary, request);
        if let Some(slot) = slot {
            spec.cwd = slot.into();
            spec.arguments[3].clear();
        }
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
    #[cfg(target_os = "linux")]
    fn confirm_cleanup(&self) -> bool {
        (self.check)().is_ok() && drained(self)
    }

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
        self.run_at(request, cancel, shutdown, None)
    }
}

impl UnixBridge {
    #[cfg(target_os = "linux")]
    pub(super) fn run_build(
        &self,
        request: &Request,
        slot: &Path,
        cancel: &Cancellation,
        shutdown: &Cancellation,
    ) -> Report {
        self.run_at(Some(request), cancel, shutdown, Some(slot))
    }

    fn run_at(
        &self,
        request: Option<&Request>,
        cancel: &Cancellation,
        shutdown: &Cancellation,
        slot: Option<&Path>,
    ) -> Report {
        match catch_unwind(AssertUnwindSafe(|| {
            self.body(request, cancel, shutdown, slot)
        })) {
            Ok(report) => report,
            Err(_payload) => {
                if self
                    .runner
                    .as_ref()
                    .is_ok_and(|runner| runner.cleanup_residue())
                {
                    self.readiness.latch(Cause::Containment);
                }
                Report::Failed {
                    creation: Creation::Unknown,
                }
            }
        }
    }
}
// No filesystem or environment observation: constraints match Runner's owner policy.
#[cfg(test)]
fn derive(
    layout: &Layout,
    config: &ServiceConfig,
    parent: PathBuf,
) -> Result<exec::OwnerConfig, Unavailable> {
    derive_with_roots(layout, config, parent, layout.private_dirs().to_vec())
}

fn derive_with_roots(
    layout: &Layout,
    config: &ServiceConfig,
    parent: PathBuf,
    protected_directories: Vec<PathBuf>,
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
        protected_directories,
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

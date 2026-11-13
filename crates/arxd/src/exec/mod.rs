//! Library-only execution. No MCP route or owner configuration loader is exposed.
//!
//! INTEGRATION CONTRACT: commit the authenticated caller's `started` activity
//! before calling `Runner::run`. If that transaction fails, do not call this API.
//! Persist the returned outcome; after restart mark unfinished activity interrupted.
//! Configuration is owner authority, never deserialize it from agent arguments.
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(target_os = "linux")]
mod cgroup;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod output;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;

#[derive(Clone)]
pub struct OwnerConfig {
    pub account: String,
    pub uid: u32,
    pub gid: u32,
    pub workspace: PathBuf,
    /// Must include every service state/configuration/key directory.
    pub protected_directories: Vec<PathBuf>,
    /// Exact environment names and values supplied by the owner; never inherited.
    pub environment: BTreeMap<String, String>,
    pub admin: bool,
    /// Linux: existing root-owned delegated cgroup v2 subtree with memory+pids
    /// enabled. The build account must have no write access to the hierarchy.
    pub cgroup_parent: Option<PathBuf>,
    pub memory_bytes: u64,
    pub pids: u32,
}

#[derive(Clone, Debug)]
pub struct RunSpec {
    /// Absolute executable, passed directly to exec. No implicit shell or PATH lookup.
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    /// Total retained bytes per stream, split between head and tail.
    pub output_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Exited,
    TimedOut,
    Cancelled,
    Shutdown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeBoundary {
    LinuxCgroupV2,
    MacProcessGroupBestEffort,
}
#[derive(Debug)]
pub struct RunResult {
    pub outcome: Outcome,
    pub exit_code: Option<i32>,
    /// Observed in the child after dropping real/effective/saved IDs, before exec.
    pub identity: ObservedIdentity,
    pub tree_boundary: TreeBoundary,
    pub tree_cleanup_confirmed: bool,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
}
#[derive(Debug, PartialEq, Eq)]
pub struct ObservedIdentity {
    pub uid: u32,
    pub gid: u32,
    pub daemon_was_root: bool,
}
/// Bytes are untrusted command output. Never render as HTML or instructions.
#[derive(Debug, PartialEq, Eq)]
pub struct CapturedOutput {
    pub head: Vec<u8>,
    pub tail: Vec<u8>,
    pub dropped_bytes: u64,
}
#[derive(Debug, PartialEq, Eq)]
pub enum ExecError {
    Invalid(&'static str),
    Unavailable(&'static str),
    OperatingSystem(i32),
}
impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ExecError {}
impl From<std::io::Error> for ExecError {
    fn from(e: std::io::Error) -> Self {
        Self::OperatingSystem(e.raw_os_error().unwrap_or(-1))
    }
}

// ADR 0019 C1: additive shared type identity; public API is preserved.
pub use crate::development::Cancellation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotStarted {
    Invalid,
    Unsupported,
    ServiceIdentity,
    AccountPolicy,
    AccountChanged,
    PrivateRoots,
    Containment,
    Resource,
    Shutdown,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError {
    NotStarted(NotStarted),
    SpawnFailed,
    AfterSpawn,
}
// Never format this value or reconstruct legacy errors from the phase.
struct RunFailure {
    phase: RunError,
    legacy: ExecError,
}
impl RunFailure {
    fn new(phase: RunError, legacy: ExecError) -> Self {
        Self { phase, legacy }
    }
    #[cfg(any(target_os = "linux", target_os = "macos", test))]
    fn os(phase: RunError, error: std::io::Error) -> Self {
        Self::new(phase, ExecError::from(error))
    }
}
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn shutdown_requested(runner: bool, caller: bool) -> bool {
    runner || caller
}
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn recheck(cancel: bool, shutdown: bool) -> Option<RunFailure> {
    let class = if shutdown {
        NotStarted::Shutdown
    } else if cancel {
        NotStarted::Cancelled
    } else {
        return None;
    };
    Some(RunFailure::new(
        RunError::NotStarted(class),
        ExecError::Unavailable("run already cancelled or shutting down"),
    ))
}

pub struct Runner {
    config: OwnerConfig,
    shutdown: Cancellation,
    residue: Arc<AtomicBool>,
}
impl Runner {
    pub fn new(config: OwnerConfig) -> Result<Self, ExecError> {
        validate_config(&config)?;
        Ok(Self {
            config,
            shutdown: Cancellation::default(),
            residue: Arc::new(AtomicBool::new(false)),
        })
    }
    /// Integration must signal this token during daemon shutdown and join run threads.
    /// Every active run polls it; callers must not forcibly abort their run threads.
    pub fn shutdown_handle(&self) -> Cancellation {
        self.shutdown.clone()
    }
    /// Monotone cleanup uncertainty for this runner's admission domain.
    pub fn cleanup_residue(&self) -> bool {
        self.residue.load(Ordering::Acquire)
    }
    pub fn run(&self, spec: &RunSpec, cancellation: &Cancellation) -> Result<RunResult, ExecError> {
        validate_spec(spec)?;
        self.run_core(spec, cancellation, None)
            .map_err(|f| f.legacy)
    }
    pub fn run_phased(
        &self,
        spec: &RunSpec,
        cancellation: &Cancellation,
        shutdown: &Cancellation,
    ) -> Result<RunResult, RunError> {
        validate_phased_spec(spec).map_err(|_| RunError::NotStarted(NotStarted::Invalid))?;
        self.run_core(spec, cancellation, Some(shutdown))
            .map_err(|f| f.phase)
    }
    fn run_core(
        &self,
        spec: &RunSpec,
        cancellation: &Cancellation,
        caller_shutdown: Option<&Cancellation>,
    ) -> Result<RunResult, RunFailure> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            unix::run(
                &self.config,
                spec,
                cancellation,
                &self.shutdown,
                caller_shutdown,
                #[cfg(target_os = "linux")]
                self.residue.clone(),
            )
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (&self.config, spec, cancellation, caller_shutdown);
            Err(RunFailure::new(
                RunError::NotStarted(NotStarted::Unsupported),
                ExecError::Unavailable("execution backend unavailable on this OS"),
            ))
        }
    }
}
impl Drop for Runner {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

fn clean_path(path: &Path, absolute: bool) -> bool {
    !path.as_os_str().is_empty()
        && path.is_absolute() == absolute
        && path.components().all(|c| {
            matches!(c, Component::Normal(_) | Component::RootDir)
                || (cfg!(windows) && absolute && matches!(c, Component::Prefix(_)))
        })
        && path.to_str().is_some_and(|s| {
            s.len() <= 4096 && !s.contains('\0') && !s.split('/').any(|p| p == "." || p == "..")
        })
}
fn validate_config(c: &OwnerConfig) -> Result<(), ExecError> {
    if c.admin {
        return Err(ExecError::Unavailable(
            "admin execution needs owner policy implementation",
        ));
    }
    if c.uid == 0
        || c.gid == 0
        || c.account.is_empty()
        || c.account.len() > 128
        || !c
            .account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(ExecError::Invalid("non-root pinned account required"));
    }
    if !clean_path(&c.workspace, true)
        || c.workspace.parent().is_none()
        || c.protected_directories.is_empty()
        || c.protected_directories.len() > 32
    {
        return Err(ExecError::Invalid("workspace and protected roots required"));
    }
    for p in &c.protected_directories {
        if !clean_path(p, true) || p.starts_with(&c.workspace) || c.workspace.starts_with(p) {
            return Err(ExecError::Invalid("workspace overlaps private state"));
        }
    }
    if c.environment.len() > 16
        || c.environment.iter().any(|(k, v)| {
            !matches!(
                k.as_str(),
                "PATH"
                    | "HOME"
                    | "TMPDIR"
                    | "LANG"
                    | "LC_ALL"
                    | "TZ"
                    | "CARGO_HOME"
                    | "RUSTUP_HOME"
            ) || v.len() > 4096
                || v.contains('\0')
        })
    {
        return Err(ExecError::Invalid("environment is outside owner whitelist"));
    }
    if !(16 * 1024 * 1024..=64 * 1024 * 1024 * 1024).contains(&c.memory_bytes)
        || !(1..=1024).contains(&c.pids)
    {
        return Err(ExecError::Invalid("resource bounds"));
    }
    Ok(())
}
fn validate_spec(s: &RunSpec) -> Result<(), ExecError> {
    if !clean_path(&s.executable, true)
        || (!s.cwd.as_os_str().is_empty() && !clean_path(&s.cwd, false))
    {
        return Err(ExecError::Invalid(
            "absolute executable and workspace-relative cwd required",
        ));
    }
    if s.arguments.len() > 128
        || s.arguments
            .iter()
            .any(|a| a.len() > 8192 || a.contains('\0'))
        || s.arguments.iter().map(String::len).sum::<usize>() > 65536
    {
        return Err(ExecError::Invalid("argv bounds"));
    }
    if s.timeout < Duration::from_millis(10)
        || s.timeout > Duration::from_secs(3600)
        || !(2..=1024 * 1024).contains(&s.output_bytes)
    {
        return Err(ExecError::Invalid("timeout/output bounds"));
    }
    Ok(())
}
// Only the exact shim prefix earns overhead; the agent tail retains legacy bounds.
fn validate_phased_spec(s: &RunSpec) -> Result<(), ExecError> {
    if s.arguments.first().is_none_or(|a| a != "exec-child") {
        return validate_spec(s);
    }
    let a = &s.arguments;
    if a.len() < 4
        || !matches!(a[1].as_str(), "path" | "tool")
        || a[2].is_empty()
        || a[2].len() > 4096
        || a[3].len() > 4096
        || a[..4].iter().any(|v| v.contains('\0'))
    {
        return Err(ExecError::Invalid("argv bounds"));
    }
    let tail = RunSpec {
        arguments: a[4..].to_vec(),
        ..s.clone()
    };
    validate_spec(&tail)
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn stop_reason(
    elapsed: Duration,
    timeout: Duration,
    cancel: bool,
    shutdown: bool,
) -> Option<Outcome> {
    if shutdown {
        Some(Outcome::Shutdown)
    } else if cancel {
        Some(Outcome::Cancelled)
    } else if elapsed >= timeout {
        Some(Outcome::TimedOut)
    } else {
        None
    }
}

#[cfg(all(test, target_os = "linux"))]
mod disposable;
#[cfg(test)]
mod stage_b_tests;

#[cfg(test)]
mod tests {
    include!("tests.rs");

    #[test]
    fn independent_owned_domains_keep_distinct_monotone_runner_flags() {
        // These runners represent distinct test domains, never replacements
        // for one service subtree. No execution or account lookup occurs.
        let domains = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
        let runners = domains.each_ref().map(|domain| {
            let mut config = super::tests::config();
            config.workspace = domain.path().join("workspace");
            config.protected_directories = vec![domain.path().join("private")];
            config.cgroup_parent = Some(domain.path().join("runs"));
            for path in [
                &config.workspace,
                &config.protected_directories[0],
                config.cgroup_parent.as_ref().unwrap(),
            ] {
                std::fs::create_dir(path).unwrap();
            }
            Runner::new(config).unwrap()
        });
        assert_ne!(
            runners[0].config.cgroup_parent,
            runners[1].config.cgroup_parent
        );
        assert!(!Arc::ptr_eq(&runners[0].residue, &runners[1].residue));
        assert!(runners.iter().all(|r| !r.cleanup_residue()));
        let group_flag = runners[0].residue.clone();
        std::thread::spawn(move || group_flag.store(true, Ordering::Release))
            .join()
            .unwrap();
        assert!(runners[0].cleanup_residue());
        assert!(!runners[1].cleanup_residue());
        runners[0].shutdown_handle().cancel();
        assert!(runners[0].cleanup_residue());
    }

    #[test]
    fn cleanup_source_pins_release_sites_and_legacy_errno_order() {
        // A portable source guard, not evidence of Linux syscall behavior.
        let source = include_str!("cgroup.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let helper = source
            .split("fn release_leaf(")
            .nth(1)
            .unwrap()
            .split("fn text(")
            .next()
            .unwrap();
        let create = source
            .split("pub fn create(")
            .nth(1)
            .unwrap()
            .split("pub fn kill_and_confirm(")
            .next()
            .unwrap();
        let kill = source
            .split("pub fn kill_and_confirm(")
            .nth(1)
            .unwrap()
            .split("impl Drop for Group")
            .next()
            .unwrap();
        assert_eq!(source.matches("libc::unlinkat(").count(), 2);
        assert_eq!(helper.matches("libc::unlinkat(").count(), 1);
        assert_eq!(kill.matches("libc::unlinkat(").count(), 1);
        assert!(!create.contains("libc::unlinkat("));
        assert_eq!(
            create.matches("release_leaf(fd, &name, &residue);").count(),
            2
        );
        assert!(create.contains("if raw < 0 {\n            release_leaf(fd, &name, &residue);\n            return Err(std::io::Error::last_os_error().into());"));
        assert!(create.contains("Err(e) => {\n                release_leaf(fd, &name, &residue);\n                Err(e.into())"));
        assert_eq!(helper.matches("libc::").count(), 2); // unlinkat + AT_REMOVEDIR
        assert!(helper.contains("residue.store(true, Ordering::Release)"));
        assert!(source.contains("if !self.kill_and_confirm() {\n            self.residue.store(true, Ordering::Release);"));
        let unix = include_str!("unix.rs");
        assert!(unix.contains("#[cfg(target_os = \"linux\")] residue: Arc<AtomicBool>"));
        assert!(unix.contains("Group::create(c, residue)"));
    }
}

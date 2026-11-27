//! Local owner routing. Parsing completes before native authority is acquired.
#[cfg(any(unix, test))]
use super::owner_plan::Plan;
use super::{
    cluster_install::{self, InstallError, Role},
    config::{ClusterRequest, ConfigureRequest, Resolver, Rights, ServiceConfig},
    layout::{Error, Layout},
    owner_plan::{self, OwnerContext},
    owner_transactions::{Failure, RightReport},
};
use crate::client_config::ClientError;
use std::{ffi::OsString, io::Write};

#[derive(PartialEq)]
pub(super) struct ConfigureArgs {
    build: String,
    client: String,
    rights: Rights,
    workspace: String,
    environment: Vec<(String, String)>,
    agents: Vec<(String, String)>,
    /// Linux `cluster` launch setting: installation file and intent.
    cluster: Option<(String, String)>,
}
impl ConfigureArgs {
    fn request(&self) -> ConfigureRequest<'_> {
        ConfigureRequest {
            build_account: &self.build,
            client_account: &self.client,
            rights: self.rights,
            workspace: &self.workspace,
            environment: &self.environment,
            agents: &self.agents,
            cluster: self
                .cluster
                .as_ref()
                .map(|(config, intent)| ClusterRequest { config, intent }),
        }
    }
}
pub(super) struct ClientArgs {
    principal: String,
    out: String,
}
enum Command {
    Install,
    Configure(ConfigureArgs),
    Start,
    Stop,
    Status,
    Uninstall,
    Client(ClientArgs),
    ClusterConfig(cluster_install::Request),
}
struct Usage;
fn parse(args: &[OsString]) -> Result<Command, Usage> {
    if args.len() > 170
        || args.iter().any(|a| a.len() > 4200)
        || args.iter().map(|a| a.len()).sum::<usize>() > 32768
    {
        return Err(Usage);
    }
    let a = args
        .iter()
        .map(|a| a.to_str().ok_or(Usage))
        .collect::<Result<Vec<_>, _>>()?;
    match a.as_slice() {
        ["service", "install"] => return Ok(Command::Install),
        ["service", "start"] => return Ok(Command::Start),
        ["service", "stop"] => return Ok(Command::Stop),
        ["service", "status"] => return Ok(Command::Status),
        ["service", "uninstall"] => return Ok(Command::Uninstall),
        _ => {}
    }
    if a.starts_with(&["service", "cluster-config"]) {
        return parse_cluster_config(&a[2..]).map(Command::ClusterConfig);
    }
    let client = a.starts_with(&["client-config", "new"]);
    if !client && !a.starts_with(&["service", "configure"]) {
        return Err(Usage);
    }
    if (a.len() - 2) % 2 != 0 {
        return Err(Usage);
    }
    let (mut build, mut account, mut rights, mut workspace, mut principal, mut out) =
        (None, None, None, None, None, None);
    let (mut cluster_config, mut cluster_intent) = (None, None);
    let (mut environment, mut agents) = (Vec::new(), Vec::new());
    for pair in a[2..].chunks_exact(2) {
        let (flag, value) = (pair[0], pair[1]);
        if value.is_empty() || value.starts_with("--") {
            return Err(Usage);
        }
        let slot = match (client, flag) {
            (true, "--principal") => &mut principal,
            (true, "--out") => &mut out,
            (false, "--build-account") => &mut build,
            (false, "--client-account") => &mut account,
            (false, "--rights") => &mut rights,
            (false, "--workspace") => &mut workspace,
            (false, "--cluster-config") => &mut cluster_config,
            (false, "--cluster-intent") => &mut cluster_intent,
            (false, "--env" | "--agent") => {
                let (key, val) = value.split_once('=').ok_or(Usage)?;
                if key.is_empty() || (flag == "--agent" && val.is_empty()) {
                    return Err(Usage);
                }
                let (list, max) = if flag == "--env" {
                    (&mut environment, 16)
                } else {
                    (&mut agents, 64)
                };
                if list.len() == max {
                    return Err(Usage);
                }
                list.push((key.to_owned(), val.to_owned()));
                continue;
            }
            _ => return Err(Usage),
        };
        if slot.replace(value.to_owned()).is_some() {
            return Err(Usage);
        }
    }
    if client {
        return Ok(Command::Client(ClientArgs {
            principal: principal.ok_or(Usage)?,
            out: out.ok_or(Usage)?,
        }));
    }
    Ok(Command::Configure(ConfigureArgs {
        build: build.ok_or(Usage)?,
        client: account.ok_or(Usage)?,
        rights: match rights.as_deref() {
            Some("standard") => Rights::Standard,
            Some("admin") => Rights::Admin,
            _ => return Err(Usage),
        },
        workspace: workspace.ok_or(Usage)?,
        environment,
        agents,
        cluster: match (cluster_config, cluster_intent) {
            (Some(config), Some(intent)) => Some((config, intent)),
            (None, None) => None,
            _ => return Err(Usage),
        },
    }))
}

/// `service cluster-config`: every flag appears exactly once, except the
/// joiner-only `--issuer-peer`. Values are checked by `cluster_install::check`.
fn parse_cluster_config(a: &[&str]) -> Result<cluster_install::Request, Usage> {
    const FLAGS: [&str; 16] = [
        "--out",
        "--role",
        "--workspace",
        "--state",
        "--credentials",
        "--wrap",
        "--custody",
        "--history",
        "--owner-socket",
        "--peer",
        "--enrollment",
        "--issuer-peer",
        "--operator-account",
        "--control-group",
        "--agent-account",
        "--build-account",
    ];
    if !a.len().is_multiple_of(2) {
        return Err(Usage);
    }
    let mut values: [Option<String>; 16] = Default::default();
    for pair in a.chunks_exact(2) {
        let (flag, value) = (pair[0], pair[1]);
        let index = FLAGS.iter().position(|f| *f == flag).ok_or(Usage)?;
        if value.is_empty() || value.starts_with("--") {
            return Err(Usage);
        }
        if values[index].replace(value.to_owned()).is_some() {
            return Err(Usage);
        }
    }
    let [
        out,
        role,
        workspace,
        state,
        credentials,
        wrap,
        custody,
        history,
        owner_socket,
        peer,
        enrollment,
        issuer_peer,
        operator_account,
        control_group,
        agent_account,
        build_account,
    ] = values;
    Ok(cluster_install::Request {
        out: out.ok_or(Usage)?,
        role: match role.as_deref() {
            Some("founder") => Role::Founder,
            Some("joiner") => Role::Joiner,
            _ => return Err(Usage),
        },
        workspace: workspace.ok_or(Usage)?,
        state: state.ok_or(Usage)?,
        credentials: credentials.ok_or(Usage)?,
        wrap: wrap.ok_or(Usage)?,
        custody: custody.ok_or(Usage)?,
        history: history.ok_or(Usage)?,
        owner_socket: owner_socket.ok_or(Usage)?,
        peer: peer.ok_or(Usage)?,
        enrollment: enrollment.ok_or(Usage)?,
        issuer_peer,
        operator_account: operator_account.ok_or(Usage)?,
        control_group: control_group.ok_or(Usage)?,
        agent_account: agent_account.ok_or(Usage)?,
        build_account: build_account.ok_or(Usage)?,
    })
}

pub(super) enum CliError {
    Service(Error),
    Owner(Failure),
    Client(ClientError),
    Cluster(InstallError),
}
impl From<Error> for CliError {
    fn from(e: Error) -> Self {
        Self::Service(e)
    }
}
impl From<Failure> for CliError {
    fn from(e: Failure) -> Self {
        Self::Owner(e)
    }
}
#[cfg(windows)]
impl From<super::owner_windows::OpenError> for CliError {
    fn from(e: super::owner_windows::OpenError) -> Self {
        match e {
            super::owner_windows::OpenError::Layout(e) => e.into(),
            super::owner_windows::OpenError::Store(e) => e.into(),
        }
    }
}
fn error(e: CliError) -> (&'static str, i32) {
    match e {
        CliError::Service(e) => match e {
            Error::Config => ("service-config", 3),
            Error::Identity => ("service-identity", 3),
            Error::AccountMissing => ("service-account-missing", 3),
            Error::Rights => ("service-rights", 3),
            Error::Workspace => ("service-workspace", 3),
            Error::Permissions => ("service-permissions", 4),
            Error::OwnerRequired => ("service-owner-required", 4),
            Error::Layout => ("service-layout", 7),
            Error::NativeAdapterMissing => ("service-native-adapter-missing", 7),
            Error::Manager => ("service-manager", 1),
            Error::NotInstalled => ("service-not-installed", 1),
            Error::Foreign => ("service-foreign-registration", 1),
            Error::Active => ("service-active", 1),
        },
        CliError::Owner(e) => (
            match e {
                Failure::InvalidPlan => "service-owner-plan",
                Failure::Io => "service-owner-io",
                Failure::Conflict => "service-owner-conflict",
                Failure::Busy => "service-owner-busy",
                Failure::RecoveryRequired => "service-owner-recovery-required",
                Failure::RightUnproven => "service-owner-right-unproven",
                Failure::Unsupported => "service-owner-unsupported",
            },
            1,
        ),
        CliError::Client(e) => match e {
            ClientError::Argument => ("client-config-argument", 3),
            ClientError::Elevated => ("client-config-elevated", 4),
            ClientError::Parent => ("client-config-parent", 4),
            ClientError::Exists => ("client-config-exists", 1),
            ClientError::Io => ("client-config-io", 1),
            ClientError::Partial => ("client-config-partial", 1),
        },
        CliError::Cluster(e) => (e.token(), e.exit_code()),
    }
}
pub(super) trait OwnerOps {
    fn install(&mut self) -> Result<(), CliError>;
    fn configure(&mut self, args: &ConfigureArgs) -> Result<(), CliError>;
    fn start(&mut self) -> Result<(), CliError>;
    fn stop(&mut self) -> Result<(), CliError>;
    fn status(&mut self) -> Result<&'static str, CliError>;
    fn uninstall(&mut self) -> Result<Option<RightReport>, CliError>;
    fn client(&mut self, args: &ClientArgs) -> Result<String, CliError>;
    /// Writes the private installation file; returns the `arxd-cluster` UID.
    fn cluster_config(&mut self, args: &cluster_install::Request) -> Result<u32, CliError>;
}
// Shared by real dispatch and fake dispatch; tokens belong to library Display.
fn right_output(report: RightReport) -> String {
    report.to_string()
}
fn dispatch(
    cmd: Command,
    ops: &mut impl OwnerOps,
    out: &mut impl Write,
    err: &mut impl Write,
) -> i32 {
    let result = match cmd {
        Command::Install => ops.install(),
        Command::Configure(a) => ops.configure(&a),
        Command::Start => ops.start(),
        Command::Stop => ops.stop(),
        Command::Status => ops.status().map(|s| {
            let _ = writeln!(out, "{s}");
        }),
        Command::Uninstall => ops.uninstall().map(|s| {
            if let Some(s) = s {
                let _ = writeln!(out, "{}", right_output(s));
            }
        }),
        Command::Client(a) => ops.client(&a).map(|hash| {
            let _ = writeln!(out, "{}={hash}", a.principal);
        }),
        // Pure argument checks run on every platform before native authority.
        Command::ClusterConfig(a) => cluster_install::check(&a)
            .map_err(CliError::Cluster)
            .and_then(|()| ops.cluster_config(&a))
            .map(|uid| {
                let _ = writeln!(out, "daemon-uid={uid}");
            }),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            let (token, code) = error(e);
            let _ = writeln!(err, "arxd: {token}");
            code
        }
    }
}

/// Called only for reserved prefixes, before the foreground Tokio runtime.
pub fn main(args: &[OsString]) -> i32 {
    #[cfg(target_os = "linux")]
    if args.first().is_some_and(|value| value == "service")
        && args.get(1).is_some_and(|value| value == "cluster-control")
    {
        return cluster_control::run(args);
    }
    let cmd = match parse(args) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("arxd: usage");
            return 2;
        }
    };
    #[cfg(not(test))]
    {
        dispatch(
            cmd,
            &mut NativeOps,
            &mut std::io::stdout().lock(),
            &mut std::io::stderr().lock(),
        )
    }
    #[cfg(test)]
    {
        let _ = cmd;
        panic!("valid owner commands require fake dispatch in tests")
    }
}

#[cfg(windows)]
fn windows_configure(
    session: &mut impl super::owner_windows::ConfigureSession,
    l: &Layout,
    ctx: &OwnerContext<'_>,
    new: &ServiceConfig,
    res: &impl Resolver,
) -> Result<(), CliError> {
    let prev = session.read_config()?;
    let old = prev
        .as_ref()
        .map(|b| ServiceConfig::parse(b, l, res))
        .transpose()?;
    let batch = session.observe_batch(new.build_identity())?;
    let plan = owner_plan::configure_plan(l, ctx, new, old.as_ref(), Some(batch), res)?;
    let req = super::owner_transactions::Configure::from_plan(l, &plan, res, batch)?;
    session.configure(&req)?;
    Ok(())
}
#[cfg(windows)]
pub(super) fn windows_start(
    preflight: impl FnOnce() -> Result<(), Error>,
    start: impl FnOnce() -> Result<(), Error>,
) -> Result<(), Error> {
    preflight()?;
    start()
}
#[cfg(any(unix, test))]
fn unix_verb(
    action: owner_plan::Action,
    l: &Layout,
    ctx: &OwnerContext<'_>,
    execute: impl FnOnce(&Plan) -> Result<(), Error>,
) -> Result<(), Error> {
    execute(&owner_plan::owner_plan(l, ctx, action)?)
}
#[cfg(any(unix, test))]
fn unix_configure(
    read_previous: impl FnOnce() -> Result<Option<Vec<u8>>, Error>,
    res: &impl Resolver,
    l: &Layout,
    ctx: &OwnerContext<'_>,
    args: &ConfigureArgs,
    cluster: impl FnOnce(&ServiceConfig) -> Result<(), Error>,
    execute: impl FnOnce(&Plan) -> Result<(), Error>,
) -> Result<(), Error> {
    let new = ServiceConfig::from_request(&args.request(), l, res)?;
    // Linux vets a requested cluster file and layout before any mutation.
    cluster(&new)?;
    let prev = read_previous()?;
    let old = prev
        .as_ref()
        .map(|b| ServiceConfig::parse(b, l, res))
        .transpose()?;
    execute(&owner_plan::configure_plan(
        l,
        ctx,
        &new,
        old.as_ref(),
        None,
        res,
    )?)
}

#[cfg(not(test))]
struct NativeOps;
#[cfg(all(not(test), windows))]
impl OwnerOps for NativeOps {
    fn install(&mut self) -> Result<(), CliError> {
        let guard = super::owner_windows::OwnerGuard::acquire()?;
        super::owner_install_windows::install(guard)?;
        Ok(())
    }
    fn configure(&mut self, a: &ConfigureArgs) -> Result<(), CliError> {
        let guard = super::owner_windows::OwnerGuard::acquire()?;
        let l = guard.layout().clone();
        let exe = std::env::current_exe().map_err(|_| Error::Layout)?;
        let ctx = OwnerContext {
            admin: true,
            running_binary: &exe,
        };
        let new = ServiceConfig::from_request(&a.request(), &l, &super::native::NativeResolver)?;
        let mut session = super::owner_windows::WindowsOwnerSession::open(guard)?;
        windows_configure(&mut session, &l, &ctx, &new, &super::native::NativeResolver)
    }
    fn start(&mut self) -> Result<(), CliError> {
        let guard = super::owner_windows::OwnerGuard::acquire()?;
        let l = guard.layout().clone();
        // Native preflight returns with all its pins released. Release image pin before SCM load.
        windows_start(
            || {
                super::native::owner_start_preflight(&l)?;
                drop(guard);
                Ok(())
            },
            || super::owner_scm::WindowsScm::system(&l)?.start(),
        )?;
        Ok(())
    }
    fn stop(&mut self) -> Result<(), CliError> {
        let guard = super::owner_windows::OwnerGuard::acquire()?;
        super::owner_scm::WindowsScm::system(guard.layout())?.stop()?;
        Ok(())
    }
    fn status(&mut self) -> Result<&'static str, CliError> {
        let guard = super::owner_windows::OwnerGuard::acquire()?;
        Ok(super::owner_scm::WindowsScm::system(guard.layout())?
            .status()?
            .token())
    }
    fn uninstall(&mut self) -> Result<Option<RightReport>, CliError> {
        let guard = super::owner_windows::OwnerGuard::acquire()?;
        let mut scm = super::owner_scm::WindowsScm::system(guard.layout())?;
        let report = super::owner_scm::uninstall_flow(&mut scm, |p| {
            super::owner_windows::WindowsOwnerSession::uninstall(guard, p)
        })??;
        Ok(Some(report))
    }
    fn client(&mut self, a: &ClientArgs) -> Result<String, CliError> {
        crate::client_config::create(&a.principal, &a.out).map_err(CliError::Client)
    }
    fn cluster_config(&mut self, _: &cluster_install::Request) -> Result<u32, CliError> {
        Err(CliError::Cluster(InstallError::Unsupported))
    }
}
#[cfg(all(not(test), unix))]
fn unix_context<T>(
    f: impl FnOnce(&Layout, &OwnerContext<'_>) -> Result<T, CliError>,
) -> Result<T, CliError> {
    super::owner_unix::require_owner()?;
    let l = Layout::system()?;
    let exe = std::env::current_exe().map_err(|_| Error::Layout)?;
    f(
        &l,
        &OwnerContext {
            admin: true,
            running_binary: &exe,
        },
    )
}
#[cfg(all(not(test), unix))]
impl OwnerOps for NativeOps {
    fn install(&mut self) -> Result<(), CliError> {
        unix_context(|l, c| {
            Ok(unix_verb(
                owner_plan::Action::Install,
                l,
                c,
                super::owner_unix::execute,
            )?)
        })
    }
    fn configure(&mut self, a: &ConfigureArgs) -> Result<(), CliError> {
        unix_context(|l, c| {
            Ok(unix_configure(
                super::owner_unix::read_config,
                &super::native::NativeResolver,
                l,
                c,
                a,
                |new| linux_cluster_preflight(l, new),
                super::owner_unix::execute,
            )?)
        })
    }
    fn start(&mut self) -> Result<(), CliError> {
        unix_context(|l, c| {
            Ok(unix_verb(
                owner_plan::Action::Start,
                l,
                c,
                super::owner_unix::execute,
            )?)
        })
    }
    fn stop(&mut self) -> Result<(), CliError> {
        unix_context(|l, c| {
            Ok(unix_verb(
                owner_plan::Action::Stop,
                l,
                c,
                super::owner_unix::execute,
            )?)
        })
    }
    fn status(&mut self) -> Result<&'static str, CliError> {
        unix_context(|_, _| Ok(super::owner_unix::status()?.token()))
    }
    fn uninstall(&mut self) -> Result<Option<RightReport>, CliError> {
        unix_context(|l, c| {
            unix_verb(
                owner_plan::Action::Uninstall,
                l,
                c,
                super::owner_unix::execute,
            )?;
            Ok(None)
        })
    }
    fn client(&mut self, a: &ClientArgs) -> Result<String, CliError> {
        crate::client_config::create(&a.principal, &a.out).map_err(CliError::Client)
    }
    fn cluster_config(&mut self, a: &cluster_install::Request) -> Result<u32, CliError> {
        #[cfg(target_os = "linux")]
        {
            let layout = Layout::system()?;
            super::cluster_runtime::install::publish(a, &layout).map_err(CliError::Cluster)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = a;
            Err(CliError::Cluster(InstallError::Unsupported))
        }
    }
}
/// Linux vets the cluster file and layout as `Child::spawn` would at start.
#[cfg(all(not(test), unix))]
fn linux_cluster_preflight(l: &Layout, new: &ServiceConfig) -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    if let Some(launch) = new.cluster() {
        return super::cluster_runtime::Child::preflight(launch, l, new).map_err(|_| Error::Config);
    }
    let _ = (l, new);
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cluster_enrollment;

#[cfg(target_os = "linux")]
mod cluster_control;

//! Local owner routing. Parsing completes before native authority is acquired.
#[cfg(any(unix, test))]
use super::owner_plan::Plan;
use super::{
    config::{ConfigureRequest, Resolver, Rights, ServiceConfig},
    layout::{Error, Layout},
    owner_plan::{self, OwnerContext},
    owner_transactions::Failure,
};
use crate::client_config::ClientError;
use std::{ffi::OsString, io::Write};

pub(super) struct ConfigureArgs {
    build: String,
    client: String,
    rights: Rights,
    workspace: String,
    environment: Vec<(String, String)>,
    agents: Vec<(String, String)>,
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
    let client = a.starts_with(&["client-config", "new"]);
    if !client && !a.starts_with(&["service", "configure"]) {
        return Err(Usage);
    }
    if (a.len() - 2) % 2 != 0 {
        return Err(Usage);
    }
    let (mut build, mut account, mut rights, mut workspace, mut principal, mut out) =
        (None, None, None, None, None, None);
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
    }))
}

pub(super) enum CliError {
    Service(Error),
    Owner(Failure),
    Client(ClientError),
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
    }
}
pub(super) trait OwnerOps {
    fn install(&mut self) -> Result<(), CliError>;
    fn configure(&mut self, args: &ConfigureArgs) -> Result<(), CliError>;
    fn start(&mut self) -> Result<(), CliError>;
    fn stop(&mut self) -> Result<(), CliError>;
    fn status(&mut self) -> Result<&'static str, CliError>;
    fn uninstall(&mut self) -> Result<Option<&'static str>, CliError>;
    fn client(&mut self, args: &ClientArgs) -> Result<String, CliError>;
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
                let _ = writeln!(out, "{s}");
            }
        }),
        Command::Client(a) => ops.client(&a).map(|hash| {
            let _ = writeln!(out, "{}={hash}", a.principal);
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
    execute: impl FnOnce(&Plan) -> Result<(), Error>,
) -> Result<(), Error> {
    let new = ServiceConfig::from_request(&args.request(), l, res)?;
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
    fn uninstall(&mut self) -> Result<Option<&'static str>, CliError> {
        let guard = super::owner_windows::OwnerGuard::acquire()?;
        let mut scm = super::owner_scm::WindowsScm::system(guard.layout())?;
        let report = super::owner_scm::uninstall_flow(&mut scm, |p| {
            super::owner_windows::WindowsOwnerSession::uninstall(guard, p)
        })??;
        Ok(Some(match report {
            super::owner_transactions::RightReport::Removed => "batch-right-removed",
            super::owner_transactions::RightReport::AlreadyAbsent => "batch-right-already-absent",
            super::owner_transactions::RightReport::RetainedUnproven => {
                "batch-right-retained-unproven"
            }
            super::owner_transactions::RightReport::RetainedNotArxcovo => {
                "batch-right-retained-not-arxcovo"
            }
            super::owner_transactions::RightReport::NotRecorded => "batch-right-not-recorded",
        }))
    }
    fn client(&mut self, a: &ClientArgs) -> Result<String, CliError> {
        crate::client_config::create(&a.principal, &a.out).map_err(CliError::Client)
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
    fn uninstall(&mut self) -> Result<Option<&'static str>, CliError> {
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
}

#[cfg(test)]
mod tests;

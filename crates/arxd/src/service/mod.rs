//! ADR0009 lifecycle plumbing. Production application integration is deliberately
//! unavailable until fixed config, inventory and durable daemon audit are wired.
pub mod config;
pub mod layout;
pub mod native;
pub mod owner_plan;
use std::{future::Future, time::Duration};
use tokio::sync::watch;

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ExitCode {
    Clean = 0,
    Usage = 2,
    Config = 3,
    Permissions = 4,
    Store = 5,
    Endpoint = 6,
    Layout = 7,
    NotServiceManager = 10,
}
impl ExitCode {
    pub fn category(self) -> &'static str {
        match self {
            Self::Clean => "service-stopped",
            Self::Usage => "usage: arxd service run",
            Self::Config => "service-config",
            Self::Permissions => "service-permissions",
            Self::Store => "service-store-or-drain",
            Self::Endpoint => "service-endpoint",
            Self::Layout => "service-layout-or-integration-unavailable",
            Self::NotServiceManager => "service-manager-required",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Starting,
    Running,
    Stopping,
    Stopped,
}

/// Level-triggered, cloneable cancellation, including subscribers created late.
#[derive(Clone, Debug)]
pub struct Shutdown(watch::Sender<bool>);
impl Default for Shutdown {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}
impl Shutdown {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    pub async fn cancelled(&self) {
        let mut rx = self.0.subscribe();
        let _ = rx.wait_for(|value| *value).await;
    }
}

/// Hooks must be cancellation safe. Cleanup hooks must tolerate partial startup.
/// `open_store` includes restart reconciliation. Audit hooks must durably commit
/// daemon rows. `start_discovery` spawns tracked background work after readiness;
/// `drain` joins all runs/discovery and persists terminal outcomes with a separate
/// shutdown stop reason before return. This contract defines no storage enum.
/// Preflight failure records an unavailable execution reason inside the app; it
/// must not fail inventory startup. No implementation here grants execution.
/// `verify_layout` must enforce ADR0009's root/SYSTEM owner and private config
/// policy, including a protected Windows DACL. The foreground config permission
/// helper alone is insufficient: its DACL check is not a service owner check.
/// Blocking work must run off the host task so control/checkpoint timers can run.
#[allow(async_fn_in_trait)]
pub trait Application {
    async fn load_config(&mut self) -> Result<(), ExitCode>;
    async fn verify_layout(&mut self) -> Result<(), ExitCode>;
    async fn open_store(&mut self) -> Result<(), ExitCode>;
    async fn service_start(&mut self) -> Result<(), ExitCode>;
    async fn bind(&mut self, cancellation: Shutdown) -> Result<(), ExitCode>;
    async fn preflight(&mut self);
    fn start_discovery(&mut self);
    async fn serve(&mut self) -> Result<(), ExitCode>;
    fn stop_accepting(&mut self) -> Result<(), ExitCode>;
    async fn drain(&mut self) -> Result<(), ExitCode>;
    async fn service_stop(&mut self) -> Result<(), ExitCode>;
    fn release(&mut self) -> Result<(), ExitCode>;
}
/// Repeated pending reports are heartbeats; adapters increment checkpoints.
pub trait Reporter {
    fn report(&mut self, state: State, code: ExitCode) -> Result<(), ExitCode>;
}

pub struct Host;
impl Host {
    pub async fn run<A: Application, R: Reporter>(
        app: &mut A,
        request: Shutdown,
        reporter: &mut R,
    ) -> ExitCode {
        let cancellation = Shutdown::default();
        let mut audit_started = false;
        let mut result = reporter.report(State::Starting, ExitCode::Clean);
        if result.is_ok() {
            result = pending(
                async {
                    app.load_config().await?;
                    app.verify_layout().await?;
                    app.open_store().await?;
                    app.service_start().await?;
                    audit_started = true;
                    app.bind(cancellation.clone()).await?;
                    app.preflight().await;
                    Ok(())
                },
                &request,
                reporter,
                State::Starting,
            )
            .await;
        }
        if result.is_ok() && !request.is_cancelled() {
            result = reporter.report(State::Running, ExitCode::Clean);
            if result.is_ok() && !request.is_cancelled() {
                app.start_discovery();
                result = tokio::select! {
                    biased;
                    _ = request.cancelled() => Ok(()),
                    result = app.serve() => result,
                };
            }
        }
        // Preserve the first failure; cleanup failures must never turn it clean.
        retain(
            &mut result,
            reporter.report(State::Stopping, ExitCode::Clean),
        );
        retain(&mut result, app.stop_accepting());
        cancellation.cancel();
        let cleanup = async {
            let mut cleanup = app.drain().await;
            if audit_started {
                retain(&mut cleanup, app.service_stop().await);
            }
            cleanup
        };
        let deadline = Shutdown::default(); // cleanup is bounded, never cancelled by a second stop
        let drained = tokio::time::timeout(
            Duration::from_secs(10),
            pending(cleanup, &deadline, reporter, State::Stopping),
        )
        .await;
        retain(&mut result, drained.unwrap_or(Err(ExitCode::Store)));
        retain(&mut result, app.release());
        let code = result.err().unwrap_or(ExitCode::Clean);
        retain(&mut result, reporter.report(State::Stopped, code));
        result.err().unwrap_or(ExitCode::Clean)
    }
}
fn retain(result: &mut Result<(), ExitCode>, next: Result<(), ExitCode>) {
    if result.is_ok() {
        *result = next;
    }
}
async fn pending<F: Future<Output = Result<(), ExitCode>>, R: Reporter>(
    future: F,
    request: &Shutdown,
    reporter: &mut R,
    state: State,
) -> Result<(), ExitCode> {
    tokio::pin!(future);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.tick().await;
    let mut reporting = Ok(());
    loop {
        tokio::select! {
            biased;
            _ = request.cancelled() => return reporting,
            result = &mut future => { retain(&mut reporting, result); return reporting; },
            // A reporting error must not discard drain/audit futures. Finish
            // cleanup within its deadline, then return the reporting failure.
            _ = tick.tick() => retain(&mut reporting, reporter.report(state, ExitCode::Clean)),
        }
    }
}

/// No fixed-layout config/store/inventory adapter exists in slices 1–2. In
/// particular, do not open foreground state or claim ready with a fake adapter.
pub fn run() -> ExitCode {
    #[cfg(windows)]
    {
        windows::run()
    }
    #[cfg(unix)]
    {
        unix::run()
    }
}

#[cfg(test)]
mod tests;

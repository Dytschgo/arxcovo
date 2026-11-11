//! Production composition. There is no CLI/test adapter selector.
use super::{
    Application, ExitCode, Shutdown,
    config::ServiceConfig,
    layout::{Error, Layout},
    native,
    preflight::ExecutionUnavailable,
};
use crate::{
    local::Listener,
    store::{Activity, Lifecycle},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(crate) trait Hooks: Send + Sync + 'static {
    fn load(&self) -> Result<(Layout, ServiceConfig), ExitCode>;
    fn layout(&self, layout: &Layout) -> Result<Vec<native::Observed>, ExitCode>;
    fn store(&self, layout: &Layout) -> Result<Activity, ExitCode>;
    fn bind(&self, layout: &Layout, client: String) -> Result<Listener, ExitCode>;
    fn preflight(&self, layout: &Layout, config: &ServiceConfig) -> ExecutionUnavailable;
}
pub(crate) struct Native;
impl Hooks for Native {
    fn load(&self) -> Result<(Layout, ServiceConfig), ExitCode> {
        native::require_service_identity().map_err(category)?;
        let layout = Layout::system().map_err(category)?;
        let config = native::read_config(&layout).map_err(category)?;
        Ok((layout, config))
    }
    fn layout(&self, layout: &Layout) -> Result<Vec<native::Observed>, ExitCode> {
        if std::env::current_exe().map_err(|_| ExitCode::Layout)? != layout.binary() {
            return Err(ExitCode::Layout);
        }
        native::observe_layout(layout).map_err(category)
    }
    fn store(&self, layout: &Layout) -> Result<Activity, ExitCode> {
        Activity::open_service(layout).map_err(|_| ExitCode::Store)
    }
    fn bind(&self, layout: &Layout, client: String) -> Result<Listener, ExitCode> {
        Listener::bind_service(layout, client).map_err(|_| ExitCode::Endpoint)
    }
    fn preflight(&self, layout: &Layout, config: &ServiceConfig) -> ExecutionUnavailable {
        match config.execution_preflight(layout, &native::NativeResolver) {
            Ok(()) => ExecutionUnavailable::MechanismUnavailable,
            Err(reason) => reason,
        }
    }
}
fn category(error: Error) -> ExitCode {
    match error {
        Error::Config
        | Error::Identity
        | Error::AccountMissing
        | Error::Rights
        | Error::Workspace => ExitCode::Config,
        Error::Permissions | Error::OwnerRequired => ExitCode::Permissions,
        _ => ExitCode::Layout,
    }
}
#[derive(Default)]
struct Blocking {
    count: AtomicUsize,
    done: tokio::sync::Notify,
}
struct Active(Arc<Blocking>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.count.fetch_sub(1, Ordering::SeqCst);
        self.0.done.notify_one();
    }
}
impl Blocking {
    async fn run<T: Send + 'static>(
        self: &Arc<Self>,
        f: impl FnOnce() -> Result<T, ExitCode> + Send + 'static,
    ) -> Result<T, ExitCode> {
        self.count.fetch_add(1, Ordering::SeqCst);
        let active = Active(self.clone());
        tokio::task::spawn_blocking(move || {
            let _active = active;
            f()
        })
        .await
        .map_err(|_| ExitCode::Layout)?
    }
    async fn drain(&self) {
        loop {
            let done = self.done.notified();
            if self.count.load(Ordering::SeqCst) == 0 {
                break;
            }
            done.await;
        }
    }
}
pub(crate) struct ServiceApplication<H = Native> {
    hooks: Arc<H>,
    blocking: Arc<Blocking>,
    layout: Option<Layout>,
    config: Option<ServiceConfig>,
    pins: Vec<native::Observed>,
    activity: Option<Arc<Activity>>,
    listener: Option<Listener>,
    serving: Option<tokio::task::JoinHandle<Result<(), ExitCode>>>,
    stop: Shutdown,
}
impl ServiceApplication<Native> {
    pub(crate) fn native() -> Self {
        Self::new(Native)
    }
}
impl<H: Hooks> ServiceApplication<H> {
    fn new(hooks: H) -> Self {
        Self {
            hooks: Arc::new(hooks),
            blocking: Arc::default(),
            layout: None,
            config: None,
            pins: Vec::new(),
            activity: None,
            listener: None,
            serving: None,
            stop: Shutdown::default(),
        }
    }
    async fn audit(&self, event: Lifecycle) -> Result<(), ExitCode> {
        let Some(activity) = self.activity.clone() else {
            return Err(ExitCode::Store);
        };
        self.blocking
            .run(move || {
                let id = activity
                    .start_lifecycle(event)
                    .map_err(|_| ExitCode::Store)?;
                activity
                    .finish(id, "succeeded")
                    .map_err(|_| ExitCode::Store)
            })
            .await
    }
}
impl<H: Hooks> Application for ServiceApplication<H> {
    async fn load_config(&mut self) -> Result<(), ExitCode> {
        let hooks = self.hooks.clone();
        let (layout, config) = self.blocking.run(move || hooks.load()).await?;
        self.layout = Some(layout);
        self.config = Some(config);
        Ok(())
    }
    async fn verify_layout(&mut self) -> Result<(), ExitCode> {
        let hooks = self.hooks.clone();
        let layout = self.layout.clone().ok_or(ExitCode::Layout)?;
        self.pins = self.blocking.run(move || hooks.layout(&layout)).await?;
        Ok(())
    }
    async fn open_store(&mut self) -> Result<(), ExitCode> {
        let hooks = self.hooks.clone();
        let layout = self.layout.clone().ok_or(ExitCode::Layout)?;
        self.activity = Some(Arc::new(
            self.blocking.run(move || hooks.store(&layout)).await?,
        ));
        Ok(())
    }
    async fn service_start(&mut self) -> Result<(), ExitCode> {
        self.audit(Lifecycle::Start).await
    }
    async fn bind(&mut self, _: Shutdown) -> Result<(), ExitCode> {
        let hooks = self.hooks.clone();
        let layout = self.layout.clone().ok_or(ExitCode::Layout)?;
        let client = self
            .config
            .as_ref()
            .ok_or(ExitCode::Config)?
            .client_identity()
            .to_owned();
        self.listener = Some(
            self.blocking
                .run(move || hooks.bind(&layout, client))
                .await?,
        );
        Ok(())
    }
    async fn preflight(&mut self) {
        let hooks = self.hooks.clone();
        if let (Some(layout), Some(config), Some(activity)) = (
            self.layout.clone(),
            self.config.clone(),
            self.activity.clone(),
        ) {
            let reason = self
                .blocking
                .run(move || Ok(hooks.preflight(&layout, &config)))
                .await
                .unwrap_or(ExecutionUnavailable::PreflightFailed);
            activity.set_service_reason(reason);
        }
    }
    fn start_discovery(&mut self) {
        // No build-account backend: never probe the privileged service's PATH.
    }
    async fn serve(&mut self) -> Result<(), ExitCode> {
        let config = self.config.as_ref().ok_or(ExitCode::Config)?;
        let transport = Arc::new(crate::config::Config {
            execution: None,
            development_execution: None,
            listen: None,
            state_dir: self.layout.as_ref().ok_or(ExitCode::Layout)?.state().into(),
            local: None,
            agents: config
                .agents()
                .map(|(p, t)| crate::config::Agent {
                    principal: p.into(),
                    token_sha256: t.into(),
                })
                .collect(),
        });
        let listener = self.listener.take().ok_or(ExitCode::Endpoint)?;
        let activity = self.activity.clone().ok_or(ExitCode::Store)?;
        let stop = self.stop.clone();
        self.serving = Some(tokio::spawn(async move {
            listener
                .run_service(transport, activity, stop)
                .await
                .map_err(|_| ExitCode::Endpoint)
        }));
        let returned = self.serving.as_mut().unwrap().await;
        self.serving.take();
        returned.map_err(|_| ExitCode::Endpoint)?
    }
    fn stop_accepting(&mut self) -> Result<(), ExitCode> {
        self.stop.cancel();
        self.listener.take();
        Ok(())
    }
    async fn drain(&mut self) -> Result<(), ExitCode> {
        if let Some(task) = self.serving.as_mut() {
            task.await.map_err(|_| ExitCode::Endpoint)??;
        }
        self.serving.take();
        self.blocking.drain().await;
        if let Some(activity) = self.activity.clone() {
            self.blocking
                .run(move || activity.shutdown_pending().map_err(|_| ExitCode::Store))
                .await?;
        }
        Ok(())
    }
    async fn service_stop(&mut self) -> Result<(), ExitCode> {
        self.audit(Lifecycle::Stop).await
    }
    fn release(&mut self) -> Result<(), ExitCode> {
        if let Some(task) = self.serving.take() {
            task.abort();
        }
        self.listener.take();
        self.activity.take();
        self.pins.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests;

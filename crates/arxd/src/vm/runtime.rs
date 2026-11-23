use super::reaper::CleanupWork;
use crate::config::VmRuntimeProfile;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRefusal {
    Disabled,
    UnsupportedPlatform,
    QualificationMissing,
    IdentityUnverified,
    DistributionUnverified,
    PathCustodyUnverified,
    JobUnverified,
    WhpxUnverified,
    QmpUnverified,
    GuestEndpointUnverified,
    DiskCeilingUnverified,
    OwnerActivityUnverified,
    ReservesUnverified,
    NetworkFenceUnverified,
    /// Compatibility refusal for the unwired private template adapter.
    InstallerPhaseUnavailable,
    InstallerPhaseMismatch,
    InstallerDeadlineExpired,
}

pub enum NativeCapability {
    Unavailable(NativeRefusal),
    Qualified(LaunchAuthority),
}

/// Intentionally uninhabited until a reviewed issuer can bind the full
/// qualification record and live evidence to a particular owned operation.
pub struct LaunchAuthority {
    _closed: std::convert::Infallible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMutation {
    CopyMedia,
    Boot,
    CreateImage,
}

pub struct CandidateRuntime<'a> {
    profile: Option<&'a VmRuntimeProfile>,
}

impl<'a> CandidateRuntime<'a> {
    pub fn new(profile: Option<&'a VmRuntimeProfile>) -> Self {
        Self { profile }
    }

    pub fn capability(&self) -> NativeCapability {
        let reason = if !self.profile.is_some_and(VmRuntimeProfile::enabled) {
            NativeRefusal::Disabled
        } else if !cfg!(all(windows, target_arch = "x86_64")) {
            NativeRefusal::UnsupportedPlatform
        } else {
            NativeRefusal::QualificationMissing
        };
        NativeCapability::Unavailable(reason)
    }

    /// Refuse before the private lifecycle can stage anything. Native launch
    /// independently requires an operation-bound LaunchAuthority.
    #[allow(
        dead_code,
        reason = "The private lifecycle has no production entry point yet"
    )]
    pub(super) fn template_installer(&self) -> Result<std::convert::Infallible, NativeRefusal> {
        Err(NativeRefusal::InstallerPhaseUnavailable)
    }

    /// The callback is the first boundary for external effects. Even a valid
    /// portable admission is insufficient to reach it.
    pub fn dispatch<T>(
        &self,
        _operation: NativeMutation,
        effect: impl FnOnce(LaunchAuthority) -> T,
    ) -> Result<T, NativeRefusal> {
        match self.capability() {
            NativeCapability::Unavailable(reason) => Err(reason),
            NativeCapability::Qualified(authority) => Ok(effect(authority)),
        }
    }
}

/// Diagnostic checks can describe absent evidence but cannot issue authority.
#[derive(Default)]
pub struct QualificationEvidence {
    pub identity: bool,
    pub distribution: bool,
    pub path_custody: bool,
    pub job: bool,
    pub whpx: bool,
    pub qmp: bool,
    pub guest_endpoint: bool,
    pub disk_ceiling: bool,
    pub owner_activity: bool,
    pub reserves: bool,
    pub network_fence: bool,
}

impl QualificationEvidence {
    pub fn check(&self) -> Result<(), NativeRefusal> {
        use NativeRefusal::*;
        [
            (self.identity, IdentityUnverified),
            (self.distribution, DistributionUnverified),
            (self.path_custody, PathCustodyUnverified),
            (self.job, JobUnverified),
            (self.whpx, WhpxUnverified),
            (self.qmp, QmpUnverified),
            (self.guest_endpoint, GuestEndpointUnverified),
            (self.disk_ceiling, DiskCeilingUnverified),
            (self.owner_activity, OwnerActivityUnverified),
            (self.reserves, ReservesUnverified),
            (self.network_fence, NetworkFenceUnverified),
        ]
        .into_iter()
        .find_map(|(proved, reason)| (!proved).then_some(reason))
        .map_or(Ok(()), Err)
    }
}

/// Maximum age of a host observation used to admit or start work.
pub const FRESHNESS: Duration = Duration::from_secs(5);

pub fn fresh_observation(
    captured: Instant,
    now: Instant,
    missing: NativeRefusal,
) -> Result<(), NativeRefusal> {
    now.checked_duration_since(captured)
        .filter(|age| *age <= FRESHNESS)
        .map(|_| ())
        .ok_or(missing)
}

/// Remaining headroom on the fixed volume and its host backing are independent.
pub fn capacity(
    allocated: u64,
    inner_free: u64,
    outer_free: u64,
    total: u64,
    ceiling: u64,
    floor: u64,
) -> Result<super::Capacity, NativeRefusal> {
    if total == 0 || total > ceiling || allocated.checked_add(inner_free).is_none_or(|n| n > total)
    {
        return Err(NativeRefusal::DiskCeilingUnverified);
    }
    let free_bytes = outer_free.min(
        inner_free
            .checked_add(floor)
            .ok_or(NativeRefusal::DiskCeilingUnverified)?,
    );
    Ok(super::Capacity {
        free_bytes,
        managed_bytes: allocated,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopDisposition {
    CleanRelease,
    DirtyRelease,
    ChargedQuarantine,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerRecipe {
    Ubuntu,
    Windows,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerPhase {
    UbuntuInstall,
    WindowsSetup,
    WindowsFinalize,
}

impl InstallerPhase {
    pub fn recipe(self) -> InstallerRecipe {
        match self {
            Self::UbuntuInstall => InstallerRecipe::Ubuntu,
            Self::WindowsSetup | Self::WindowsFinalize => InstallerRecipe::Windows,
        }
    }
}

/// Create once before the first launch and keep through detached verification.
/// Per-stage limits can shorten this budget only.
pub struct InstallerBudget {
    recipe: InstallerRecipe,
    deadline: tokio::time::Instant,
}

impl InstallerBudget {
    pub fn start(recipe: InstallerRecipe) -> Self {
        let maximum = match recipe {
            InstallerRecipe::Ubuntu => Duration::from_secs(60 * 60),
            InstallerRecipe::Windows => Duration::from_secs(3 * 60 * 60),
        };
        Self {
            recipe,
            deadline: tokio::time::Instant::now() + maximum,
        }
    }

    /// Also used to clip installed-mode health and verification operations.
    pub fn clip(&self, limit: Duration) -> tokio::time::Instant {
        let now = tokio::time::Instant::now();
        if now >= self.deadline {
            self.deadline
        } else {
            now + limit.min(self.deadline - now)
        }
    }

    pub fn stage(&self, phase: InstallerPhase, limit: Duration) -> Option<InstallerStage> {
        (self.recipe == phase.recipe()).then(|| InstallerStage {
            phase,
            deadline: self.clip(limit),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InstallerStage {
    phase: InstallerPhase,
    deadline: tokio::time::Instant,
}

impl InstallerStage {
    pub fn phase(self) -> InstallerPhase {
        self.phase
    }

    pub fn deadline(self) -> tokio::time::Instant {
        self.deadline
    }

    /// Resolve a phase only after teardown has produced its release evidence.
    pub fn complete(
        self,
        result: Result<(), InstallerFailureReason>,
        observed: StopDisposition,
        cancelled: bool,
    ) -> Result<(), InstallerFailure> {
        use InstallerFailureReason as Failure;
        let reason = match result {
            Err(reason) => reason,
            Ok(()) if cancelled => Failure::Cancelled,
            Ok(()) if tokio::time::Instant::now() >= self.deadline => Failure::Timeout,
            Ok(()) if observed == StopDisposition::CleanRelease => return Ok(()),
            Ok(()) => Failure::ReleaseUnverified,
        };
        let recovery = if observed == StopDisposition::ChargedQuarantine {
            StopDisposition::ChargedQuarantine
        } else {
            StopDisposition::DirtyRelease
        };
        Err(InstallerFailure {
            phase: self.phase,
            reason,
            recovery,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerFailureReason {
    Timeout,
    Cancelled,
    Native(NativeRefusal),
    Protocol(super::qmp::ChannelError),
    ReleaseUnverified,
    ProcessFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstallerFailure {
    pub phase: InstallerPhase,
    pub reason: InstallerFailureReason,
    pub recovery: StopDisposition,
}

#[derive(Default)]
pub struct StopEvidence {
    pub guest_shutdown: bool,
    pub clean_process_exit: bool,
    pub process_exited: bool,
    pub job_empty: bool,
    pub workers_joined: bool,
    pub artifacts_reopened: bool,
}

impl StopEvidence {
    /// The release check may acquire exclusive writer custody. It may only
    /// run once the child, its whole Job and its workers have stopped.
    pub fn verify_release(&mut self, verify: impl FnOnce() -> bool) {
        self.artifacts_reopened = self.ready_to_verify_release() && verify();
    }

    pub(super) fn ready_to_verify_release(&self) -> bool {
        self.process_exited && self.job_empty && self.workers_joined
    }

    pub fn disposition(&self) -> StopDisposition {
        if !(self.process_exited
            && self.job_empty
            && self.workers_joined
            && self.artifacts_reopened)
        {
            return StopDisposition::ChargedQuarantine;
        }
        if self.guest_shutdown && self.clean_process_exit {
            StopDisposition::CleanRelease
        } else {
            StopDisposition::DirtyRelease
        }
    }
}

/// Release must be bound to the old generation and pin the slot against reuse.
/// An observed process exit or fixture receipt cannot produce this lease.
pub struct OfflineLease {
    _closed: std::convert::Infallible,
}

/// One teardown authority per work item. Implementations must bind it to the
/// recorded subject and generation and serialize it against boot and stop.
/// Every method must return by its monotonic deadline.
pub(crate) trait ReaperRuntime {
    /// Signal the outstanding worker and join it or return Unconfirmed.
    /// Offline requires independent process, worker and artifact release proof,
    /// including on restart. A missing PID is insufficient.
    fn quiesce(&self, work: &CleanupWork, deadline: Instant) -> Quiesced;

    fn graceful_stop(&self, work: &CleanupWork, deadline: Instant) -> StopDisposition;

    /// Invoked only after quiesce returned an owned TEST Job for a lease
    /// expiry. The adapter must revalidate the held Job before using it.
    fn force_reap(&self, work: &CleanupWork, deadline: Instant) -> StopDisposition;

    /// Non-deleting release proof for a committed static generation after an
    /// interrupted boot. True requires the same independent process, worker
    /// and Job release proof as [`ReaperRuntime::quiesce`] for this exact VM
    /// and generation. It grants no artifact authority. Closed by default.
    fn static_released(&self, vm: super::Id, generation: super::Id, deadline: Instant) -> bool {
        let _ = (vm, generation, deadline);
        false
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code, reason = "native release authority is an activation gate")]
pub(crate) enum Quiesced {
    Offline,
    OwnedTestJob,
    Unconfirmed,
}

impl ReaperRuntime for CandidateRuntime<'_> {
    fn quiesce(&self, _: &CleanupWork, _: Instant) -> Quiesced {
        Quiesced::Unconfirmed
    }

    fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }

    fn force_reap(&self, _: &CleanupWork, _: Instant) -> StopDisposition {
        StopDisposition::ChargedQuarantine
    }
}

impl CandidateRuntime<'_> {
    pub(crate) fn publication_origin(&self, _vm: super::Id) -> Option<publication::Origin> {
        // Native static boot is still closed by A2. A provisional port on its
        // own never qualifies as a current-boot publication witness.
        None
    }
}

pub(crate) mod publication {
    #![cfg_attr(
        not(any(test, all(windows, target_arch = "x86_64"))),
        allow(
            dead_code,
            unused_variables,
            reason = "native publication is Windows x64 only"
        )
    )]
    use anyhow::{Result, ensure};
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use std::{
        num::NonZeroU16,
        path::{Path, PathBuf},
    };
    use tokio::sync::{Mutex, OwnedMutexGuard};

    #[cfg(all(windows, target_arch = "x86_64"))]
    use crate::vm::publish::{
        cloudflare::{CloudflareDns, DnsBearer},
        connector::{Connector, ConnectorError, Plan, TunnelCredential},
        windows::{public_roots, read_secret},
    };
    use crate::{
        service::Shutdown,
        store::vm::publish::{Intent, Phase, Update},
        vm::{
            Clock, FailureReason as Failure, Id, OperationRef, Refusal, VmController,
            paths::{ArtifactPin, LocalPath},
            publish::{
                Hostname, TunnelId,
                cloudflare::{DnsApi, DnsError, Reconciled, RecordId, Removal, Route},
                hex_id,
            },
            static_web::{ORIGIN_PORT_FIRST, ORIGIN_PORT_LAST, StaticName, StaticOriginIdentity},
        },
    };
    use std::sync::Arc;

    /// All values come from the owner-private foreground profile.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct Profile {
        hostnames: [String; 1],
        static_name: String,
        host_id: String,
        tunnel_id: String,
        account_id: String,
        zone_id: String,
        private_root: PathBuf,
        dns_ciphertext: PathBuf,
        tunnel_ciphertext: PathBuf,
        connector: PathBuf,
        connector_length: u64,
        connector_sha256: String,
    }

    impl Profile {
        fn path(path: &Path) -> Result<LocalPath> {
            let path = LocalPath::parse(path.to_str().unwrap_or_default())
                .map_err(|_| anyhow::anyhow!("invalid publication profile"))?;
            ensure!(!path.is_drive_root(), "invalid publication profile");
            Ok(path)
        }

        pub(crate) fn validate(&self, state: &LocalPath) -> Result<()> {
            let private = Self::path(&self.private_root)?;
            let dns = Self::path(&self.dns_ciphertext)?;
            let tunnel = Self::path(&self.tunnel_ciphertext)?;
            let connector = Self::path(&self.connector)?;
            ensure!(
                cfg!(all(windows, target_arch = "x86_64"))
                    && self
                        .hostnames
                        .iter()
                        .all(|name| Hostname::parse(name).is_ok())
                    && TunnelId::parse(&self.tunnel_id).is_ok()
                    && hex_id(&self.account_id)
                    && hex_id(&self.zone_id)
                    && hex_id(&self.host_id)
                    && StaticName::parse(&self.static_name)
                        .is_ok_and(|name| name.as_str() == self.static_name)
                    && Path::new(dns.as_str()).parent() == Some(Path::new(private.as_str()))
                    && Path::new(tunnel.as_str()).parent() == Some(Path::new(private.as_str()))
                    && !dns.overlaps(&tunnel)
                    && !state.overlaps(&private)
                    && !state.overlaps(&connector)
                    && !private.overlaps(&connector)
                    && (1..=128 * 1024 * 1024).contains(&self.connector_length)
                    && self.connector_sha256.len() == 64
                    && self
                        .connector_sha256
                        .as_bytes()
                        .chunks_exact(32)
                        .all(|slice| { std::str::from_utf8(slice).is_ok_and(hex_id) }),
                "invalid publication profile"
            );
            Ok(())
        }

        pub(crate) fn target(&self, host: Id, name: &StaticName) -> bool {
            self.host_id == hex::encode(host) && self.static_name == name.as_str()
        }

        pub(crate) fn intent(&self, vm: Id, origin: &Origin) -> Result<Intent> {
            let mut hash = Sha256::new();
            for bytes in [
                self.hostnames[0].as_bytes(),
                self.static_name.as_bytes(),
                self.host_id.as_bytes(),
                self.tunnel_id.as_bytes(),
                self.account_id.as_bytes(),
                self.zone_id.as_bytes(),
                self.private_root.as_os_str().as_encoded_bytes(),
                self.dns_ciphertext.as_os_str().as_encoded_bytes(),
                self.tunnel_ciphertext.as_os_str().as_encoded_bytes(),
                self.connector.as_os_str().as_encoded_bytes(),
                self.connector_sha256.as_bytes(),
                &self.connector_length.to_be_bytes(),
            ] {
                hash.update((bytes.len() as u64).to_be_bytes());
                hash.update(bytes);
            }
            Ok(Intent {
                vm,
                generation: origin.generation,
                boot: origin.boot,
                revision: hash.finalize().into(),
                hostname: self.hostnames[0].clone(),
                tunnel: self.tunnel_id.clone(),
                zone: self.zone_id.clone(),
                credential_ref: Self::path(&self.dns_ciphertext)?.as_str().to_owned(),
                nonce: Route::new_nonce()
                    .map_err(|_| anyhow::anyhow!("publication nonce unavailable"))?,
                phase: Phase::Publishing,
                post_possible: false,
                record: None,
                reason: None,
            })
        }
    }

    /// A custodied origin witness belongs to one active static generation and
    /// one held boot. The runtime retires it before releasing the origin.
    #[derive(Clone)]
    pub(crate) struct Origin {
        pub(crate) generation: Id,
        pub(crate) boot: Id,
        pub(crate) port: NonZeroU16,
        pub(crate) ca_path: PathBuf,
        pub(crate) ca_receipt: ArtifactPin,
        pub(crate) retired: Shutdown,
    }

    impl Origin {
        fn name(&self) -> Result<Hostname, Failure> {
            let plan = StaticOriginIdentity::for_generation(self.generation).provisioning();
            Hostname::parse(plan.server_name()).map_err(|_| Failure::OriginUnavailable)
        }

        fn current(&self) -> bool {
            !self.retired.is_cancelled()
                && (ORIGIN_PORT_FIRST..=ORIGIN_PORT_LAST).contains(&self.port.get())
        }
    }

    pub(crate) struct Prepared {
        dns: Dns,
        start: Start,
    }

    enum Start {
        #[cfg(all(windows, target_arch = "x86_64"))]
        Native(Arc<Profile>, TunnelCredential),
        #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
        Closed(std::convert::Infallible),
        #[cfg(test)]
        Fixture(Arc<Fake>),
    }

    #[allow(clippy::large_enum_variant)]
    enum Dns {
        #[cfg(all(windows, target_arch = "x86_64"))]
        Native(CloudflareDns),
        #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
        Closed(std::convert::Infallible),
        #[cfg(test)]
        Fixture(Arc<Fake>),
    }

    impl Profile {
        fn prepare(self: &Arc<Self>) -> Result<Prepared, Refusal> {
            #[cfg(all(windows, target_arch = "x86_64"))]
            {
                let dns =
                    native_dns(&self.dns_ciphertext).ok_or(Refusal::PublishCredentialMissing)?;
                let tunnel = TunnelId::parse(&self.tunnel_id).map_err(|_| Refusal::Invalid)?;
                let credential =
                    TunnelCredential::load(&self.tunnel_ciphertext, &self.account_id, &tunnel)
                        .map_err(|_| Refusal::PublishCredentialMissing)?;
                Ok(Prepared {
                    dns,
                    start: Start::Native(self.clone(), credential),
                })
            }
            #[cfg(not(all(windows, target_arch = "x86_64")))]
            Err(Refusal::QualificationMissing)
        }
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    fn native_dns(path: &Path) -> Option<Dns> {
        let local = Profile::path(path).ok()?;
        let path = Path::new(local.as_str());
        let raw = read_secret(path, 512).ok()?;
        let bearer = DnsBearer::new(std::str::from_utf8(&raw).ok()?.to_owned()).ok()?;
        CloudflareDns::new(bearer, public_roots().ok()?)
            .ok()
            .map(Dns::Native)
    }

    fn route(intent: &Intent) -> Result<Route, Failure> {
        let invalid = |_| Failure::DnsUnavailable;
        Route::new(
            &intent.zone,
            Hostname::parse(&intent.hostname).map_err(invalid)?,
            &TunnelId::parse(&intent.tunnel).map_err(invalid)?,
            &intent.nonce,
        )
        .map_err(invalid)
    }

    impl DnsApi for Dns {
        async fn preflight(&self, route: &Route) -> Result<(), DnsError> {
            match *self {
                #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
                Self::Closed(never) => match never {},
                #[cfg(all(windows, target_arch = "x86_64"))]
                Self::Native(ref dns) => dns.preflight(route).await,
                #[cfg(test)]
                Self::Fixture(ref fake) => fake.preflight(route).await,
            }
        }

        async fn create(&self, route: &Route) -> Result<RecordId, DnsError> {
            match *self {
                #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
                Self::Closed(never) => match never {},
                #[cfg(all(windows, target_arch = "x86_64"))]
                Self::Native(ref dns) => dns.create(route).await,
                #[cfg(test)]
                Self::Fixture(ref fake) => fake.create(route).await,
            }
        }

        async fn reconcile(&self, route: &Route) -> Result<Reconciled, DnsError> {
            match *self {
                #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
                Self::Closed(never) => match never {},
                #[cfg(all(windows, target_arch = "x86_64"))]
                Self::Native(ref dns) => dns.reconcile(route).await,
                #[cfg(test)]
                Self::Fixture(ref fake) => fake.reconcile(route).await,
            }
        }

        async fn remove(&self, route: &Route, id: &RecordId) -> Result<Removal, DnsError> {
            match *self {
                #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
                Self::Closed(never) => match never {},
                #[cfg(all(windows, target_arch = "x86_64"))]
                Self::Native(ref dns) => dns.remove(route, id).await,
                #[cfg(test)]
                Self::Fixture(ref fake) => fake.remove(route, id).await,
            }
        }
    }

    enum LocalProcess {
        #[cfg(all(windows, target_arch = "x86_64"))]
        Native(Box<Connector>),
        #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
        Closed(std::convert::Infallible),
        #[cfg(test)]
        Fixture(Arc<Fake>),
    }

    impl LocalProcess {
        async fn ready(&self) -> Result<(), Failure> {
            match *self {
                #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
                Self::Closed(never) => match never {},
                #[cfg(all(windows, target_arch = "x86_64"))]
                Self::Native(ref connector) => {
                    if !connector.running() {
                        return Err(Failure::ConnectorExited);
                    }
                    if !connector.ready().await {
                        return Err(Failure::ConnectorUnready);
                    }
                    connector
                        .origin_ready()
                        .await
                        .map_err(|_| Failure::OriginUnavailable)
                }
                #[cfg(test)]
                Self::Fixture(ref fake) => fake.ready(),
            }
        }

        async fn stop(&mut self) -> bool {
            match *self {
                #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
                Self::Closed(never) => match never {},
                #[cfg(all(windows, target_arch = "x86_64"))]
                Self::Native(ref mut connector) => connector.stop().await.is_ok(),
                #[cfg(test)]
                Self::Fixture(ref fake) => {
                    fake.local.store(false, std::sync::atomic::Ordering::SeqCst);
                    true
                }
            }
        }
    }

    impl Start {
        async fn connect(
            self,
            intent: &Intent,
            origin: &Origin,
            shutdown: &Shutdown,
            unsettled: &mut Option<LocalProcess>,
        ) -> Result<LocalProcess, Failure> {
            if !origin.current() || shutdown.is_cancelled() {
                return Err(Failure::OriginUnavailable);
            }
            let hostname =
                Hostname::parse(&intent.hostname).map_err(|_| Failure::ConnectorFailed)?;
            let tunnel = TunnelId::parse(&intent.tunnel).map_err(|_| Failure::ConnectorFailed)?;
            let origin_name = origin.name()?;
            match self {
                #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
                Self::Closed(never) => match never {},
                #[cfg(all(windows, target_arch = "x86_64"))]
                Self::Native(profile, credential) => {
                    let mut sha256 = [0; 32];
                    hex::decode_to_slice(&profile.connector_sha256, &mut sha256)
                        .map_err(|_| Failure::ConnectorFailed)?;
                    let plan = Plan {
                        executable: &profile.connector,
                        binary_receipt: ArtifactPin {
                            size: profile.connector_length,
                            sha256,
                        },
                        private_root: &profile.private_root,
                        hostname: &hostname,
                        tunnel: &tunnel,
                        credential: &credential,
                        origin_port: origin.port,
                        origin_name: &origin_name,
                        ca_path: &origin.ca_path,
                        ca_receipt: origin.ca_receipt,
                    };
                    let mut held = None;
                    let result =
                        Connector::start_cancellable(&plan, &origin.retired, shutdown, &mut held)
                            .await;
                    *unsettled = held.map(Box::new).map(LocalProcess::Native);
                    result
                        .map(Box::new)
                        .map(LocalProcess::Native)
                        .map_err(|reason| match reason {
                            ConnectorError::Origin => Failure::OriginUnavailable,
                            ConnectorError::Crashed => Failure::ConnectorExited,
                            ConnectorError::Readiness => Failure::ConnectorUnready,
                            ConnectorError::CleanupPending => Failure::CleanupUnconfirmed,
                            _ => Failure::ConnectorFailed,
                        })
                }
                #[cfg(test)]
                Self::Fixture(fake) => {
                    *fake.ingress.lock().unwrap() = Some(crate::vm::publish::ingress(
                        &tunnel,
                        std::array::from_ref(&hostname),
                        origin.port,
                        &origin_name,
                        origin.ca_path.to_str().unwrap(),
                    ));
                    fake.ready()?;
                    fake.local.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(LocalProcess::Fixture(fake))
                }
            }
        }
    }

    struct Live {
        vm: Id,
        origin: Origin,
        connector: LocalProcess,
    }

    #[derive(Default)]
    pub(crate) struct Publication {
        effects: Arc<Mutex<()>>,
        local: Mutex<Option<Live>>,
        #[cfg(test)]
        pub(crate) fake: std::sync::Mutex<Option<Arc<Fake>>>,
    }

    impl Publication {
        pub(crate) fn prepare(&self, profile: &Arc<Profile>) -> Result<Prepared, Refusal> {
            #[cfg(test)]
            if let Some(fake) = self.fake.lock().unwrap().as_ref() {
                use std::sync::atomic::Ordering;
                if !fake.credential.load(Ordering::SeqCst) {
                    return Err(Refusal::PublishCredentialMissing);
                }
                return Ok(Prepared {
                    dns: Dns::Fixture(fake.clone()),
                    start: Start::Fixture(fake.clone()),
                });
            }
            profile.prepare()
        }

        pub(crate) fn claim(&self) -> Option<OwnedMutexGuard<()>> {
            self.effects.clone().try_lock_owned().ok()
        }

        /// Called before VM release. Local Job termination is independent of
        /// Cloudflare credentials, SQLite writes and outstanding DNS requests.
        pub(crate) async fn stop_local(&self, vm: Option<Id>) -> bool {
            let mut local = self.local.lock().await;
            let Some(live) = local
                .as_mut()
                .filter(|live| vm.is_none_or(|id| live.vm == id))
            else {
                return true;
            };
            if !live.connector.stop().await {
                return false;
            }
            *local = None;
            true
        }

        async fn ready(&self, intent: &Intent, shutdown: &Shutdown) -> Result<(), Failure> {
            let local = self.local.lock().await;
            let live = local
                .as_ref()
                .filter(|live| {
                    live.vm == intent.vm
                        && live.origin.generation == intent.generation
                        && live.origin.boot == intent.boot
                        && live.origin.current()
                        && !shutdown.is_cancelled()
                })
                .ok_or(Failure::OriginUnavailable)?;
            live.connector.ready().await
        }

        pub(crate) async fn publish<C: Clock>(
            &self,
            controller: &VmController<C>,
            operation: OperationRef,
            intent: Intent,
            prepared: Prepared,
            origin: Origin,
            shutdown: &Shutdown,
        ) -> Result<()> {
            let result = self
                .activate(controller, &intent, prepared, origin, shutdown)
                .await;
            if result.is_ok() {
                // Commit while holding the local slot. A concurrently retiring
                // origin cannot be used by a successor boot.
                let local = self.local.lock().await;
                if local.as_ref().is_some_and(|live| live.origin.current())
                    && !shutdown.is_cancelled()
                {
                    let written = controller.finish_publication(
                        operation,
                        Some((&intent.nonce, Update::Phase(Phase::Published, None))),
                        None,
                        None,
                    );
                    drop(local);
                    if written.is_ok() {
                        return Ok(());
                    }
                    self.stop_local(Some(intent.vm)).await;
                    return written;
                }
            }

            let failure = result.err().unwrap_or(Failure::OriginUnavailable);
            let clean = self
                .cleanup(controller, intent.vm, Some(failure), Some(shutdown))
                .await?;
            controller.finish_publication(operation, None, Some(failure), Some(clean))
        }

        async fn activate<C: Clock>(
            &self,
            controller: &VmController<C>,
            intent: &Intent,
            prepared: Prepared,
            origin: Origin,
            shutdown: &Shutdown,
        ) -> Result<(), Failure> {
            let route = route(intent)?;
            let map = |error| match error {
                DnsError::Conflict => Failure::DnsConflict,
                _ => Failure::DnsUnavailable,
            };
            if !origin.current() || shutdown.is_cancelled() {
                return Err(Failure::OriginUnavailable);
            }
            tokio::select! {
                biased;
                _ = origin.retired.cancelled() => return Err(Failure::OriginUnavailable),
                _ = shutdown.cancelled() => return Err(Failure::Cancelled),
                result = prepared.dns.preflight(&route) => result.map_err(map)?,
            }
            {
                let mut local = self.local.lock().await;
                if local.is_some() {
                    return Err(Failure::CleanupUnconfirmed);
                }
                let mut unsettled = None;
                let started = prepared
                    .start
                    .connect(intent, &origin, shutdown, &mut unsettled)
                    .await;
                if let Some(connector) = unsettled {
                    *local = Some(Live {
                        vm: intent.vm,
                        origin: origin.clone(),
                        connector,
                    });
                }
                let connector = started?;
                *local = Some(Live {
                    vm: intent.vm,
                    origin: origin.clone(),
                    connector,
                });
            }
            self.ready(intent, shutdown).await?;
            controller
                .update_publication(&intent.nonce, Update::PostPossible)
                .map_err(|_| Failure::CleanupUnconfirmed)?;
            // Check cancellation before submitting DNS. From the durable
            // marker onwards an interrupted POST remains possible.
            if !origin.current() || shutdown.is_cancelled() {
                return Err(Failure::OriginUnavailable);
            }
            let id = tokio::select! {
                biased;
                _ = origin.retired.cancelled() => return Err(Failure::OriginUnavailable),
                _ = shutdown.cancelled() => return Err(Failure::Cancelled),
                result = prepared.dns.create(&route) => result.map_err(map)?,
            };
            controller
                .update_publication(&intent.nonce, Update::Record(id.as_str()))
                .map_err(|_| Failure::CleanupUnconfirmed)?;
            self.ready(intent, shutdown).await
        }

        pub(crate) async fn unpublish<C: Clock>(
            &self,
            controller: &VmController<C>,
            operation: OperationRef,
            vm: Id,
            shutdown: &Shutdown,
        ) -> Result<()> {
            let clean = self.cleanup(controller, vm, None, Some(shutdown)).await?;
            controller.finish_publication(
                operation,
                None,
                (!clean).then_some(Failure::CleanupUnconfirmed),
                Some(clean),
            )
        }

        pub(crate) async fn turn<C: Clock>(
            &self,
            controller: &VmController<C>,
            shutdown: &Shutdown,
        ) -> Result<()> {
            let Some(_slot) = self.claim() else {
                return Ok(());
            };
            let Some(intent) = controller.publication_intent()? else {
                return Ok(());
            };
            let reason = if intent.phase == Phase::Published {
                match self.ready(&intent, shutdown).await {
                    Ok(()) => return Ok(()),
                    Err(reason) => Some(reason),
                }
            } else {
                intent.reason
            };
            self.cleanup(controller, intent.vm, reason, None).await?;
            Ok(())
        }

        async fn cleanup<C: Clock>(
            &self,
            controller: &VmController<C>,
            vm: Id,
            reason: Option<Failure>,
            shutdown: Option<&Shutdown>,
        ) -> Result<bool> {
            let local = self.stop_local(Some(vm)).await;
            let Some(intent) = controller
                .publication_intent()?
                .filter(|intent| intent.vm == vm)
            else {
                return Ok(local);
            };
            controller.update_publication(&intent.nonce, Update::Phase(Phase::Revoking, reason))?;
            let dns = if let Some(shutdown) = shutdown {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => Err(Failure::Cancelled),
                    result = self.remove_dns(controller, &intent) => result,
                }
            } else {
                self.remove_dns(controller, &intent).await
            };
            let update = match (&dns, local) {
                (Ok(()), true) => Update::Released(reason),
                _ => Update::Phase(
                    Phase::CleanupPending,
                    reason
                        .or_else(|| dns.err())
                        .or(Some(Failure::CleanupUnconfirmed)),
                ),
            };
            let released = matches!(update, Update::Released(_));
            if shutdown.is_none() {
                controller.reconcile_publication(&intent, update, reason, released)?;
            } else {
                controller.update_publication(&intent.nonce, update)?;
            }
            Ok(released)
        }

        async fn remove_dns<C: Clock>(
            &self,
            controller: &VmController<C>,
            intent: &Intent,
        ) -> Result<(), Failure> {
            if !intent.post_possible {
                return Ok(());
            }
            let dns = self.cleanup_dns(intent).ok_or(Failure::DnsUnavailable)?;
            let route = route(intent)?;
            let unavailable = |_| Failure::DnsUnavailable;
            let id = match &intent.record {
                Some(id) => RecordId::parse(id.clone()).map_err(|_| Failure::DnsUnavailable)?,
                None => match dns.reconcile(&route).await.map_err(unavailable)? {
                    Reconciled::Owned(id) | Reconciled::OwnedWithConflict(id) => {
                        controller
                            .update_publication(&intent.nonce, Update::Record(id.as_str()))
                            .map_err(|_| Failure::CleanupUnconfirmed)?;
                        id
                    }
                    Reconciled::Absent => return Err(Failure::CleanupUnconfirmed),
                    Reconciled::ForeignOrAmbiguous => return Err(Failure::DnsConflict),
                },
            };
            match dns.remove(&route, &id).await.map_err(unavailable)? {
                Removal::Absent => Ok(()),
                Removal::ForeignOrAmbiguous => Err(Failure::DnsConflict),
            }
        }

        fn cleanup_dns(&self, intent: &Intent) -> Option<Dns> {
            #[cfg(test)]
            if let Some(fake) = self.fake.lock().unwrap().as_ref() {
                return fake
                    .credential
                    .load(std::sync::atomic::Ordering::SeqCst)
                    .then(|| Dns::Fixture(fake.clone()));
            }
            #[cfg(all(windows, target_arch = "x86_64"))]
            {
                native_dns(Path::new(&intent.credential_ref))
            }
            #[cfg(not(all(windows, target_arch = "x86_64")))]
            {
                let _ = intent;
                None
            }
        }
    }

    #[cfg(test)]
    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) credential: std::sync::atomic::AtomicBool,
        pub(crate) foreign: std::sync::atomic::AtomicBool,
        pub(crate) health: std::sync::atomic::AtomicU8,
        pub(crate) local: std::sync::atomic::AtomicBool,
        pub(crate) posts: std::sync::atomic::AtomicUsize,
        pub(crate) deletes: std::sync::atomic::AtomicUsize,
        pub(crate) ingress: std::sync::Mutex<Option<String>>,
        pub(crate) record: std::sync::atomic::AtomicBool,
        pub(crate) post_gate: std::sync::Mutex<
            Option<(
                tokio::sync::oneshot::Sender<()>,
                tokio::sync::oneshot::Receiver<()>,
            )>,
        >,
    }

    #[cfg(test)]
    impl Profile {
        pub(crate) fn synthetic(host: Id, name: &str) -> Self {
            Self {
                hostnames: ["publish.example.invalid".into()],
                static_name: name.into(),
                host_id: hex::encode(host),
                tunnel_id: "12345678-1234-4321-8234-123456789abc".into(),
                account_id: "a".repeat(32),
                zone_id: "b".repeat(32),
                private_root: "D:/synthetic".into(),
                dns_ciphertext: "D:/synthetic/dns.dpapi".into(),
                tunnel_ciphertext: "D:/synthetic/tunnel.dpapi".into(),
                connector: "D:/synthetic-dist/cloudflared.exe".into(),
                connector_length: 1,
                connector_sha256: "c".repeat(64),
            }
        }
    }

    #[cfg(test)]
    impl Fake {
        fn ready(&self) -> Result<(), Failure> {
            use std::sync::atomic::Ordering::SeqCst;
            match self.health.load(SeqCst) {
                0 => Ok(()),
                1 => Err(Failure::ConnectorExited),
                _ => Err(Failure::ConnectorUnready),
            }
        }

        fn id() -> RecordId {
            RecordId::parse("e".repeat(32)).unwrap()
        }
    }

    #[cfg(test)]
    impl DnsApi for Fake {
        async fn preflight(&self, route: &Route) -> Result<(), DnsError> {
            match self.reconcile(route).await? {
                Reconciled::Absent => Ok(()),
                _ => Err(DnsError::Conflict),
            }
        }

        async fn create(&self, _: &Route) -> Result<RecordId, DnsError> {
            use std::sync::atomic::Ordering::SeqCst;
            assert!(
                self.local.load(SeqCst),
                "DNS activated before connector readiness"
            );
            assert!(!self.record.swap(true, SeqCst), "repeated POST");
            self.posts.fetch_add(1, SeqCst);
            let gate = self.post_gate.lock().unwrap().take();
            if let Some((started, release)) = gate {
                let _ = started.send(());
                release.await.map_err(|_| DnsError::Unavailable)?;
            }
            Ok(Self::id())
        }

        async fn reconcile(&self, _: &Route) -> Result<Reconciled, DnsError> {
            use std::sync::atomic::Ordering::SeqCst;
            Ok(
                match (self.record.load(SeqCst), self.foreign.load(SeqCst)) {
                    (true, false) => Reconciled::Owned(Self::id()),
                    (true, true) => Reconciled::OwnedWithConflict(Self::id()),
                    (false, true) => Reconciled::ForeignOrAmbiguous,
                    (false, false) => Reconciled::Absent,
                },
            )
        }

        async fn remove(&self, _: &Route, id: &RecordId) -> Result<Removal, DnsError> {
            use std::sync::atomic::Ordering::SeqCst;
            assert!(
                !self.local.load(SeqCst),
                "DNS removed before local connector stopped"
            );
            assert_eq!(*id, Self::id());
            if self.record.swap(false, SeqCst) {
                self.deletes.fetch_add(1, SeqCst);
            }
            if self.foreign.load(SeqCst) {
                Ok(Removal::ForeignOrAmbiguous)
            } else {
                Ok(Removal::Absent)
            }
        }
    }
}

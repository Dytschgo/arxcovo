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

/// One checked process creation's authority, carrying the binding it was
/// issued for. Off Windows x64 it is uninhabited. On Windows x64 only a
/// consumed TEST launch permit mints it, and no production path installs
/// that issuer before 7c 16. Static and public-egress dispatch never
/// receive one. Staged writes and guest scripts have their own types.
pub struct LaunchAuthority {
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    _closed: std::convert::Infallible,
    #[cfg(all(windows, target_arch = "x86_64"))]
    _issued: super::test_issuer::Minted,
    #[cfg(all(windows, target_arch = "x86_64"))]
    binding: Box<super::qualification::LaunchPermitBinding>,
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl LaunchAuthority {
    pub(super) fn new(
        issued: super::test_issuer::Minted,
        binding: super::qualification::LaunchPermitBinding,
    ) -> Self {
        Self {
            _issued: issued,
            binding: Box::new(binding),
        }
    }

    pub(crate) fn binding(&self) -> super::qualification::LaunchPermitBinding {
        *self.binding
    }
}

/// One staged write's authority, for exactly the target it names.
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) struct StageAuthority {
    _issued: super::test_issuer::Minted,
    target: super::qualification::StageTarget,
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl StageAuthority {
    pub(super) fn new(
        issued: super::test_issuer::Minted,
        target: super::qualification::StageTarget,
    ) -> Self {
        Self {
            _issued: issued,
            target,
        }
    }

    pub(crate) fn target(&self) -> &super::qualification::StageTarget {
        &self.target
    }
}

/// One guest script's authority, for exactly the admitted bytes, timeout,
/// generation and writer epoch.
#[cfg(all(windows, target_arch = "x86_64"))]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the script channel consumes it in 7c 14")
)]
pub(crate) struct ScriptAuthority {
    _issued: super::test_issuer::Minted,
    script: super::qualification_record::Hash,
    timeout_s: u32,
    generation: super::Id,
    writer_epoch: super::Id,
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the script channel consumes it in 7c 14")
)]
impl ScriptAuthority {
    pub(super) fn new(
        issued: super::test_issuer::Minted,
        script: super::qualification_record::Hash,
        timeout_s: u32,
        generation: super::Id,
        writer_epoch: super::Id,
    ) -> Self {
        Self {
            _issued: issued,
            script,
            timeout_s,
            generation,
            writer_epoch,
        }
    }

    /// True only for the exact script this authority was issued for.
    pub(crate) fn names(
        &self,
        generation: super::Id,
        writer_epoch: super::Id,
        script: &str,
        timeout_s: u32,
    ) -> bool {
        (self.generation, self.writer_epoch, self.timeout_s)
            == (generation, writer_epoch, timeout_s)
            && self.script == super::qualification_record::digest(script.as_bytes())
    }
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

    /// A fixture copy receipt has no offline launch or native file authority.
    pub fn snapshot_lease(&self) -> Result<OfflineLease, NativeRefusal> {
        Err(NativeRefusal::QualificationMissing)
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
/// On Windows x64 only a held, ledger-identified helper target produces one
/// (7c 11), and the checked launch accepts it only for that exact target. It
/// is a binding, not effect authority: creation still needs a consumed
/// `LaunchAuthority`.
pub struct OfflineLease {
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    _closed: std::convert::Infallible,
    #[cfg(all(windows, target_arch = "x86_64"))]
    target: (
        super::qualification::WorkspaceBinding,
        super::paths::FileIdentity,
    ),
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl OfflineLease {
    /// The only constructor: a held, ledger-identified helper target, which
    /// only the checked custody code can produce.
    pub(in crate::vm) fn for_helper(target: &super::qualification::HelperTarget<'_>) -> Self {
        Self {
            target: (target.binding(), target.identity()),
        }
    }

    pub(in crate::vm) fn binds(&self, target: &super::qualification::HelperTarget<'_>) -> bool {
        self.target == (target.binding(), target.identity())
    }
}

pub const SNAPSHOT_COPY_MAXIMUM: Duration = Duration::from_secs(15 * 60);

/// A copy may shorten the controller's admission deadline. Each blocking
/// custody call receives this same deadline, including during inspection.
pub(super) fn snapshot_deadline(worker: &super::PendingOperation, start: Instant) -> Instant {
    start
        .checked_add(SNAPSHOT_COPY_MAXIMUM)
        .unwrap_or(worker.deadline())
        .min(worker.deadline())
}

/// One teardown authority per work item. Implementations must bind it to the
/// recorded subject and generation and serialize it against boot and stop.
/// Every method must return by its monotonic deadline.
pub(crate) trait ReaperRuntime {
    /// Signal the outstanding worker and join it or return Unconfirmed.
    /// Offline requires independent process, worker and artifact release proof,
    /// including on restart. A missing PID is insufficient.
    fn quiesce(&self, work: &CleanupWork, deadline: Instant) -> Quiesced;

    /// Recheck absence and hold the subject's ownership fence across proof
    /// and commit. Implementations leave the foreground gate before calling
    /// the observer and keep the scoped reservation until it returns.
    fn with_orphan_fence(&self, _work: &CleanupWork, _recover: &mut dyn FnMut() -> bool) -> bool {
        false
    }

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
    /// No current owner and reacquisition is fenced for the exact claim.
    Orphaned,
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
        store::vm::publish::{HostIntent, HostState, Intent, Phase, Update},
        vm::{
            Clock, FailureReason as Failure, Id, OperationRef, Refusal, VmController,
            paths::{ArtifactPin, LocalPath},
            publish::{
                Hostname, MAX_HOSTNAMES, RouteSlot, TunnelId,
                cloudflare::{
                    DnsApi, DnsError, Reconciled, RecordId, Removal, Route, SendAuthority, Sent,
                    Submission, arbitrate,
                },
                hex_id, validate_hostname_set,
            },
            static_web::{ORIGIN_PORT_FIRST, ORIGIN_PORT_LAST, StaticName, StaticOriginIdentity},
        },
    };
    use std::sync::Arc;

    /// One enrolled public hostname bound to a fixed Compose project/service/route slot.
    #[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct RouteBinding {
        #[serde(deserialize_with = "single_hostname")]
        pub(crate) hostname: Hostname,
        pub(crate) project_id: String,
        pub(crate) service: String,
        pub(crate) project_slot: u8,
        pub(crate) service_index: u8,
        pub(crate) route_slot: u8,
    }

    impl RouteBinding {
        fn slot(&self) -> RouteSlot {
            RouteSlot {
                project_id: self.project_id.clone(),
                service: self.service.clone(),
                project_slot: self.project_slot,
                service_index: self.service_index,
                route_slot: self.route_slot,
            }
        }
    }

    /// All values come from the owner-private foreground profile.
    #[cfg_attr(test, derive(Clone))]
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct Profile {
        /// Sorted, unique list of 1..=8 exact hostnames in the same enrolled
        /// zone (ADR 0064 §4).
        #[serde(deserialize_with = "hostnames")]
        hostnames: Vec<Hostname>,
        /// Optional explicit enrolled DNS zone name (e.g. `example.invalid`).
        #[serde(default, deserialize_with = "opt_hostname")]
        zone_name: Option<Hostname>,
        /// Optional fixed project/service/route slots matching `hostnames` 1-to-1.
        #[serde(default)]
        routes: Vec<RouteBinding>,
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

    fn single_hostname<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Hostname, D::Error> {
        let name = String::deserialize(input)?;
        Hostname::parse(&name).map_err(|_| serde::de::Error::custom("invalid publication hostname"))
    }

    fn opt_hostname<'de, D: serde::Deserializer<'de>>(
        input: D,
    ) -> Result<Option<Hostname>, D::Error> {
        let value = Option::<String>::deserialize(input)?;
        value
            .map(|name| {
                Hostname::parse(&name)
                    .map_err(|_| serde::de::Error::custom("invalid publication zone name"))
            })
            .transpose()
    }

    fn hostnames<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Vec<Hostname>, D::Error> {
        let raw = Vec::<String>::deserialize(input)?;
        if raw.is_empty() || raw.len() > MAX_HOSTNAMES {
            return Err(serde::de::Error::custom(
                "publication hostname count out of bounds",
            ));
        }
        let parsed: Vec<Hostname> = raw
            .into_iter()
            .map(|name| {
                Hostname::parse(&name)
                    .map_err(|_| serde::de::Error::custom("invalid publication hostname"))
            })
            .collect::<Result<_, _>>()?;
        validate_hostname_set(&parsed, None).map_err(|_| {
            serde::de::Error::custom(
                "publication hostnames must be sorted, unique, and in one zone",
            )
        })?;
        Ok(parsed)
    }

    impl Profile {
        fn path(path: &Path) -> Result<LocalPath> {
            let path = LocalPath::parse(path.to_str().unwrap_or_default())
                .map_err(|_| anyhow::anyhow!("invalid publication profile"))?;
            ensure!(!path.is_drive_root(), "invalid publication profile");
            Ok(path)
        }

        /// Platform-independent validation of the hostname list, zone, and
        /// project/service/route slot bindings.
        pub(crate) fn validate_binding(&self) -> Result<()> {
            ensure!(
                validate_hostname_set(&self.hostnames, self.zone_name.as_ref()).is_ok(),
                "invalid publication hostnames or zone"
            );
            ensure!(
                self.hostnames.len() == 1 || !self.routes.is_empty(),
                "multi-host binding requires route slots"
            );
            ensure!(
                self.routes.is_empty() || self.zone_name.is_some(),
                "routed binding requires an enrolled zone"
            );
            if !self.routes.is_empty() {
                ensure!(
                    self.routes.len() == self.hostnames.len(),
                    "publication route count mismatch"
                );
                let mut routes = Vec::with_capacity(self.routes.len());
                for (expected_host, binding) in self.hostnames.iter().zip(&self.routes) {
                    ensure!(
                        binding.hostname == *expected_host,
                        "publication route hostname order mismatch"
                    );
                    routes.push(binding.slot().target(expected_host));
                }
                arx_guest::compose::routing::validate_routes(
                    self.zone_name.as_ref().map(Hostname::as_str),
                    &routes,
                )
                .map_err(|_| anyhow::anyhow!("invalid publication route bindings"))?;
            }
            Ok(())
        }

        pub(crate) fn validate(&self, state: &LocalPath) -> Result<()> {
            self.validate_binding()?;
            let private = Self::path(&self.private_root)?;
            let dns = Self::path(&self.dns_ciphertext)?;
            let tunnel = Self::path(&self.tunnel_ciphertext)?;
            let connector = Self::path(&self.connector)?;
            ensure!(
                cfg!(all(windows, target_arch = "x86_64"))
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
            self.validate_binding()?;
            ensure!(
                !self.hostnames.contains(
                    &origin
                        .name()
                        .map_err(|_| anyhow::anyhow!("invalid readiness name"))?
                ),
                "reserved readiness hostname"
            );
            let mut hash = Sha256::new();
            let feed = |hash: &mut Sha256, bytes: &[u8]| {
                hash.update((bytes.len() as u64).to_be_bytes());
                hash.update(bytes);
            };
            feed(&mut hash, &(self.hostnames.len() as u64).to_be_bytes());
            for hostname in &self.hostnames {
                feed(&mut hash, hostname.as_str().as_bytes());
            }
            if let Some(zone_name) = &self.zone_name {
                feed(&mut hash, zone_name.as_str().as_bytes());
            }
            feed(&mut hash, &(self.routes.len() as u64).to_be_bytes());
            for route in &self.routes {
                feed(&mut hash, route.hostname.as_str().as_bytes());
                feed(&mut hash, route.project_id.as_bytes());
                feed(&mut hash, route.service.as_bytes());
                feed(
                    &mut hash,
                    &[route.project_slot, route.service_index, route.route_slot],
                );
            }
            for bytes in [
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
                feed(&mut hash, bytes);
            }
            let nonce =
                Route::new_nonce().map_err(|_| anyhow::anyhow!("publication nonce unavailable"))?;
            let mut seen_markers = std::collections::BTreeSet::new();
            seen_markers.insert(nonce.clone());
            let mut hosts = Vec::with_capacity(self.hostnames.len());
            for (index, hostname) in self.hostnames.iter().enumerate() {
                let marker = loop {
                    let candidate = Route::new_nonce()
                        .map_err(|_| anyhow::anyhow!("publication marker unavailable"))?;
                    if seen_markers.insert(candidate.clone()) {
                        break candidate;
                    }
                };
                let route = self.routes.get(index).map(RouteBinding::slot);
                hosts.push(HostIntent {
                    position: u8::try_from(index)
                        .map_err(|_| anyhow::anyhow!("publication host index overflow"))?,
                    hostname: hostname.clone(),
                    route,
                    marker,
                    post_possible: false,
                    record: None,
                    ambiguous: false,
                    state: HostState::Pending,
                    reason: None,
                });
            }
            Ok(Intent {
                vm,
                generation: origin.generation,
                boot: origin.boot,
                revision: hash.finalize().into(),
                tunnel: self.tunnel_id.clone(),
                zone: self.zone_id.clone(),
                credential_ref: Self::path(&self.dns_ciphertext)?.as_str().to_owned(),
                nonce,
                phase: Phase::Publishing,
                reason: None,
                hosts,
            })
        }
    }

    /// A custodied origin witness belongs to one active static generation and
    /// one held boot. The runtime retires it before releasing the origin.
    #[derive(Clone)]
    pub(crate) struct Origin {
        pub(crate) generation: Id,
        pub(crate) origin_generation: Id,
        pub(crate) boot: Id,
        pub(crate) port: NonZeroU16,
        pub(crate) ca_path: PathBuf,
        #[cfg_attr(
            not(all(windows, target_arch = "x86_64")),
            allow(dead_code, reason = "read by the Windows connector plan")
        )]
        pub(crate) ca_receipt: ArtifactPin,
        pub(crate) retired: Retirement,
    }

    /// A custodied origin's retirement. Retiring takes the fence a first DNS
    /// transport write holds, so a retirement either precedes that write and
    /// withholds it, or follows it and finds the POST possibly sent.
    #[derive(Clone, Default)]
    pub(crate) struct Retirement {
        token: Shutdown,
        fence: Arc<std::sync::Mutex<()>>,
    }

    impl Retirement {
        #[cfg_attr(
            not(test),
            allow(
                dead_code,
                reason = "the closed native origin seam retires through this"
            )
        )]
        pub(crate) fn retire(&self) {
            let _fence = self.fence();
            self.token.cancel();
        }

        pub(crate) fn is_retired(&self) -> bool {
            self.token.is_cancelled()
        }

        pub(crate) fn token(&self) -> &Shutdown {
            &self.token
        }

        fn fence(&self) -> std::sync::MutexGuard<'_, ()> {
            self.fence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    impl Origin {
        fn name(&self) -> Result<Hostname, Failure> {
            let plan = StaticOriginIdentity::for_generation(self.origin_generation).provisioning();
            Hostname::parse(plan.server_name()).map_err(|_| Failure::OriginUnavailable)
        }

        fn current(&self) -> bool {
            !self.retired.is_retired()
                && (ORIGIN_PORT_FIRST..=ORIGIN_PORT_LAST).contains(&self.port.get())
        }
    }

    struct GuestRoutes {
        session: Option<Arc<crate::vm::compose::Session>>,
        zone: Hostname,
    }

    pub(crate) struct Prepared {
        routes: Option<GuestRoutes>,
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
        fn prepare(self: &Arc<Self>, routes: Option<GuestRoutes>) -> Result<Prepared, Refusal> {
            #[cfg(all(windows, target_arch = "x86_64"))]
            {
                let dns =
                    native_dns(&self.dns_ciphertext).ok_or(Refusal::PublishCredentialMissing)?;
                let tunnel = TunnelId::parse(&self.tunnel_id).map_err(|_| Refusal::Invalid)?;
                let credential =
                    TunnelCredential::load(&self.tunnel_ciphertext, &self.account_id, &tunnel)
                        .map_err(|_| Refusal::PublishCredentialMissing)?;
                Ok(Prepared {
                    routes,
                    dns,
                    start: Start::Native(self.clone(), credential),
                })
            }
            #[cfg(not(all(windows, target_arch = "x86_64")))]
            {
                let _ = routes;
                Err(Refusal::QualificationMissing)
            }
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

    fn host_route(intent: &Intent, host: &HostIntent) -> Result<Route, Failure> {
        let invalid = |_| Failure::DnsUnavailable;
        Route::new(
            &intent.zone,
            host.hostname.clone(),
            &TunnelId::parse(&intent.tunnel).map_err(invalid)?,
            &host.marker,
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

        async fn create(
            &self,
            route: &Route,
            submission: &Submission<'_>,
        ) -> Result<RecordId, DnsError> {
            match *self {
                #[cfg(not(any(test, all(windows, target_arch = "x86_64"))))]
                Self::Closed(never) => match never {},
                #[cfg(all(windows, target_arch = "x86_64"))]
                Self::Native(ref dns) => dns.create(route, submission).await,
                #[cfg(test)]
                Self::Fixture(ref fake) => fake.create(route, submission).await,
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
        async fn ready(&self, _probe_apps: bool) -> Result<(), Failure> {
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
                        .origin_ready(_probe_apps)
                        .await
                        .map_err(|_| Failure::OriginUnavailable)
                }
                #[cfg(test)]
                Self::Fixture(ref fake) => {
                    if _probe_apps
                        && fake
                            .upstream_unavailable
                            .load(std::sync::atomic::Ordering::SeqCst)
                    {
                        return Err(Failure::OriginUnavailable);
                    }
                    fake.ready()
                }
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
                    use std::sync::atomic::Ordering::SeqCst;
                    if fake.stop_panic.swap(false, SeqCst) {
                        panic!("synthetic connector stop panic");
                    }
                    if fake
                        .stop_failures
                        .fetch_update(SeqCst, SeqCst, |left| left.checked_sub(1))
                        .is_ok()
                    {
                        return false;
                    }
                    fake.local.store(false, SeqCst);
                    true
                }
            }
        }
    }

    /// What a failed connector start left behind locally.
    #[derive(Default)]
    enum Unsettled {
        #[default]
        Nothing,
        /// A spawned process whose termination is not yet confirmed. It stays
        /// in the local slot, so stops retry it and quiescence waits for it.
        Process(LocalProcess),
        /// A failed ingress-file write whose removal could not be confirmed.
        /// The residue keeps its private custody and is never handed to a
        /// connector, but its cleanup is not confirmed.
        Residue,
    }

    impl Unsettled {
        /// The start boundary's custody decision. A held process always wins:
        /// a failed file disposition never stands in for unconfirmed process
        /// termination.
        fn from_start(held: Option<LocalProcess>, residue: bool) -> Self {
            match (held, residue) {
                (Some(process), _) => Self::Process(process),
                (None, true) => Self::Residue,
                (None, false) => Self::Nothing,
            }
        }
    }

    impl Start {
        async fn connect(
            self,
            intent: &Intent,
            origin: &Origin,
            shutdown: &Shutdown,
            unsettled: &mut Unsettled,
        ) -> Result<LocalProcess, Failure> {
            if !origin.current() || shutdown.is_cancelled() {
                return Err(Failure::OriginUnavailable);
            }
            let hostnames: Vec<Hostname> = intent
                .hosts
                .iter()
                .map(|host| host.hostname.clone())
                .collect();
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
                    let probes: Vec<_> = profile
                        .routes
                        .iter()
                        .map(|binding| {
                            let target = binding.slot().target(&binding.hostname);
                            (binding.hostname.clone(), target.probe_path())
                        })
                        .collect();
                    let plan = Plan {
                        probes: &probes,
                        executable: &profile.connector,
                        binary_receipt: ArtifactPin {
                            size: profile.connector_length,
                            sha256,
                        },
                        private_root: &profile.private_root,
                        hostnames: &hostnames,
                        tunnel: &tunnel,
                        credential: &credential,
                        origin_port: origin.port,
                        origin_name: &origin_name,
                        ca_path: &origin.ca_path,
                        ca_receipt: origin.ca_receipt,
                    };
                    let mut held = None;
                    let result = Connector::start_cancellable(
                        &plan,
                        origin.retired.token(),
                        shutdown,
                        &mut held,
                    )
                    .await;
                    let residue = matches!(result, Err(ConnectorError::ResidueUnremoved));
                    *unsettled = Unsettled::from_start(
                        held.map(|held| LocalProcess::Native(Box::new(held))),
                        residue,
                    );
                    result
                        .map(Box::new)
                        .map(LocalProcess::Native)
                        .map_err(|reason| match reason {
                            ConnectorError::Origin => Failure::OriginUnavailable,
                            ConnectorError::Crashed => Failure::ConnectorExited,
                            ConnectorError::Readiness => Failure::ConnectorUnready,
                            ConnectorError::CleanupPending | ConnectorError::ResidueUnremoved => {
                                Failure::CleanupUnconfirmed
                            }
                            _ => Failure::ConnectorFailed,
                        })
                }
                #[cfg(test)]
                Self::Fixture(fake) => {
                    use std::sync::atomic::Ordering::SeqCst;
                    // Probe each requested Host against the origin before starting the connector.
                    fake.probe_origin_hosts(&hostnames)?;
                    fake.starts.fetch_add(1, SeqCst);
                    if fake.residue.swap(false, SeqCst) {
                        // As the native connector reports a failed ingress
                        // write whose removal it could not confirm.
                        *unsettled = Unsettled::from_start(None, true);
                        return Err(Failure::CleanupUnconfirmed);
                    }
                    if fake.start_held.swap(false, SeqCst) {
                        // As a native start that failed after spawn and whose
                        // stop could not confirm termination: the process is
                        // still running and handed back.
                        fake.local.store(true, SeqCst);
                        *unsettled =
                            Unsettled::from_start(Some(LocalProcess::Fixture(fake)), false);
                        return Err(Failure::CleanupUnconfirmed);
                    }
                    *fake.ingress.lock().unwrap() = Some(
                        crate::vm::publish::ingress(
                            &tunnel,
                            &hostnames,
                            origin.port,
                            &origin_name,
                            origin.ca_path.to_str().unwrap(),
                        )
                        .map_err(|_| Failure::ConnectorFailed)?,
                    );
                    // As the native start, revocation or shutdown aborts it.
                    tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => return Err(Failure::OriginUnavailable),
                        held = hold(&fake.start_gate) => held.map_err(|_| Failure::ConnectorFailed)?,
                    }
                    fake.ready()?;
                    fake.local.store(true, SeqCst);
                    Ok(LocalProcess::Fixture(fake))
                }
            }
        }
    }

    struct Live {
        vm: Id,
        /// The activation that started this connector. Deferred stops are
        /// bound to it, so they can never stop a later activation of `vm`.
        generation: u64,
        origin: Origin,
        connector: LocalProcess,
    }

    /// An activation's owner, intent and revocation latch. Kept in memory so
    /// an authenticated owner unpublish revokes it without SQLite, the
    /// dispatch gate or the effects slot.
    struct Activation {
        generation: u64,
        vm: Id,
        owner: String,
        /// The intent this activation admits; owner causes are bound to it.
        nonce: String,
        /// Notified only after `revoked`, the owner cause and the stop
        /// obligation are recorded under the registry lock.
        token: Shutdown,
        /// Latched by owner revocation under the registry lock. The first DNS
        /// transport write checks it under the same lock.
        revoked: bool,
    }

    /// What admission hands a publish worker for its activation.
    pub(crate) struct Ticket {
        pub(crate) generation: u64,
        pub(crate) revoked: Shutdown,
    }

    /// One activation generation's local stop obligation.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Stop {
        /// Recorded by a revocation; no runner has claimed it yet.
        Requested,
        /// Claimed by exactly one runner. Further requests coalesce.
        Running,
        /// The last attempt left the connector live. A later revocation
        /// requests it again; a confirmed stop of that connector settles it.
        Failed,
    }

    /// The publication transition authority. Synchronous and short, never held
    /// across an await or SQLite, so an authenticated unpublish wins without
    /// the effects slot, SQLite or DNS.
    #[derive(Default)]
    struct Activations {
        /// The latest admitted activation.
        current: Option<Activation>,
        /// An activation registered before its admission transaction, so an
        /// owner unpublish racing that transaction still revokes it.
        pending: Option<Activation>,
        /// The last generation issued. Generations only increase.
        issued: u64,
        /// The generation whose live connector failed its last stop attempt.
        /// Cleared once that connector is confirmed stopped. This alone closes
        /// publish admission.
        unconfirmed: Option<u64>,
        /// `(intent_nonce, host_marker)` pairs whose worker exited before
        /// submitting its marked POST. Kept until the intent is released, so
        /// a failed withdrawal and a failed release in one outage still leave
        /// the proof to a later cleanup.
        unsubmitted: std::collections::HashSet<(String, String)>,
        /// Intent nonces an owner revocation cancelled. The owner cause wins
        /// over every other reason in the phase, release, audit and receipt
        /// writes of that intent, and is kept until the intent is gone.
        cancelled: Vec<String>,
        /// Local stop obligations, at most one per activation generation.
        stops: std::collections::BTreeMap<u64, Stop>,
        /// Shutdown containment has begun. No activation registers and no
        /// revocation records a stop obligation afterwards, and no first DNS
        /// transport write passes.
        sealed: bool,
        /// A worker or stop completion was lost to a panic or cancellation.
        /// Sticky: the final drain stays unconfirmed.
        lost: bool,
        /// The last activation whose failed ingress-file write left a residue
        /// it could not remove. Sticky: the final drain stays unconfirmed.
        residue: Option<u64>,
        /// Publish receipts whose cleanup an unremoved residue left
        /// unconfirmed, kept until their terminal write is durable, so orphan
        /// settlement records them as unconfirmed as well.
        unconfirmed_receipts: std::collections::HashSet<i64>,
    }

    impl Activations {
        fn find(&self, generation: u64) -> Option<&Activation> {
            [self.current.as_ref(), self.pending.as_ref()]
                .into_iter()
                .flatten()
                .find(|activation| activation.generation == generation)
        }

        /// The owner revocation transition. For every current or registered
        /// activation of `vm` that `owner` started (any owner for an admitted
        /// unpublish), latches the revocation and its owner cause and records
        /// one generation-scoped stop obligation, then notifies the token.
        /// Returns the obligations it newly requested. A request for one that
        /// is already requested or running coalesces; a failed one is
        /// requested again. After the shutdown seal it has no authority.
        fn revoke(&mut self, vm: Id, owner: Option<&str>) -> Vec<u64> {
            if self.sealed {
                return Vec::new();
            }
            let Self {
                current,
                pending,
                cancelled,
                stops,
                ..
            } = self;
            let mut requested = Vec::new();
            for activation in [current.as_mut(), pending.as_mut()].into_iter().flatten() {
                if activation.vm != vm || owner.is_some_and(|owner| activation.owner != owner) {
                    continue;
                }
                activation.revoked = true;
                if !cancelled.contains(&activation.nonce) {
                    cancelled.push(activation.nonce.clone());
                }
                if !matches!(
                    stops.get(&activation.generation),
                    Some(Stop::Requested | Stop::Running)
                ) {
                    stops.insert(activation.generation, Stop::Requested);
                    requested.push(activation.generation);
                }
                activation.token.cancel();
            }
            requested
        }

        /// Keeps owner causes only for `keep` and for live activations.
        fn prune_causes(&mut self, keep: Option<&str>) {
            let Self {
                current,
                pending,
                cancelled,
                ..
            } = self;
            cancelled.retain(|nonce| {
                keep == Some(nonce.as_str())
                    || [current.as_ref(), pending.as_ref()]
                        .into_iter()
                        .flatten()
                        .any(|activation| activation.nonce == *nonce)
            });
        }
    }

    /// One publish worker's first-write authority. Owner revocation and the
    /// shutdown seal take the registry lock, and origin retirement takes the
    /// origin's retirement fence; the first write holds both. Each of them
    /// therefore either precedes the first write and withholds it, or follows
    /// it and finds the POST possibly sent.
    struct Permit<'a> {
        publication: &'a Publication,
        generation: u64,
        origin: &'a Origin,
        shutdown: &'a Shutdown,
        /// Written only under the registry lock.
        possibly: std::sync::atomic::AtomicBool,
    }

    impl SendAuthority for Permit<'_> {
        fn first_write(
            &self,
            write: &mut dyn FnMut() -> std::task::Poll<std::io::Result<usize>>,
        ) -> std::task::Poll<std::io::Result<usize>> {
            use std::sync::atomic::Ordering::SeqCst;
            let activations = self.publication.activations();
            let _retirement = self.origin.retired.fence();
            let mut possibly = self.possibly.load(SeqCst);
            let result = arbitrate(
                &mut possibly,
                || {
                    !activations.sealed
                        && !self.shutdown.is_cancelled()
                        && !self.origin.retired.is_retired()
                        && activations
                            .find(self.generation)
                            .is_some_and(|activation| !activation.revoked)
                },
                write,
            );
            self.possibly.store(possibly, SeqCst);
            result
        }

        fn sent(&self) -> Sent {
            if self.possibly.load(std::sync::atomic::Ordering::SeqCst) {
                Sent::Possibly
            } else {
                Sent::Never
            }
        }
    }

    /// A claimed stop obligation. Dropping it records the outcome: settled,
    /// failed and retryable, or lost when the runner never finished.
    struct StopClaim<'a> {
        publication: &'a Publication,
        generation: u64,
        stopped: Option<bool>,
    }

    impl Drop for StopClaim<'_> {
        fn drop(&mut self) {
            let mut activations = self.publication.activations();
            match self.stopped {
                Some(true) => {
                    activations.stops.remove(&self.generation);
                }
                Some(false) => {
                    activations.stops.insert(self.generation, Stop::Failed);
                }
                None => {
                    activations.stops.insert(self.generation, Stop::Failed);
                    activations.lost = true;
                }
            }
            drop(activations);
            self.publication.stops_changed.notify_waiters();
        }
    }

    /// What a publish worker knows about its own in-flight DNS POST.
    enum Evidence {
        Unknown,
        /// A validated, owned create response for `position` whose ID may not
        /// be durable yet.
        Acknowledged {
            position: u8,
            id: RecordId,
        },
        /// The worker exited before submitting the POST it had marked for
        /// `position`.
        NotSubmitted {
            position: u8,
        },
    }

    /// The outcome of one publication cleanup.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Cleanup {
        /// The local connector is confirmed stopped and no intent remains.
        Released,
        /// The intent stays durable as pending, for this reason.
        Pending(Failure),
    }

    /// The outcome of one reaper or final reconciliation turn.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Turn {
        /// Another effect holds the slot; nothing was decided.
        Busy,
        /// No publication intent remains.
        Idle,
        /// A published intent whose connector is ready.
        Healthy,
        /// The intent stays durable as pending, for this reason.
        Pending(Failure),
    }

    /// Why a final drain cannot claim a clean publication settlement.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Unconfirmed {
        /// A durable intent or a pending publication receipt remains.
        Publication,
        /// A worker or stop completion was lost to a panic or cancellation.
        LostCompletion,
        /// A failed ingress-file write left a residue this process could not
        /// remove.
        LocalResidue,
        /// The final reconciliation failed or did not finish in time.
        Reconciliation,
    }

    /// The publication part of a quiescent drain.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Drained {
        /// Every effect joined, local ingress is confirmed down, no intent and
        /// no pending publication receipt remain.
        Clean,
        Unconfirmed(Unconfirmed),
    }

    /// A token cancelled by either input, for one bounded await.
    struct Linked(Shutdown, tokio::task::AbortHandle);

    impl Linked {
        fn new(first: &Shutdown, second: &Shutdown) -> Self {
            let token = Shutdown::default();
            if first.is_cancelled() || second.is_cancelled() {
                token.cancel();
            }
            let (first, second, linked) = (first.clone(), second.clone(), token.clone());
            let task = tokio::spawn(async move {
                tokio::select! {
                    _ = first.cancelled() => {}
                    _ = second.cancelled() => {}
                }
                linked.cancel();
            });
            Self(token, task.abort_handle())
        }
    }

    impl Drop for Linked {
        fn drop(&mut self) {
            self.1.abort();
        }
    }

    #[derive(Default)]
    pub(crate) struct Publication {
        effects: Arc<Mutex<()>>,
        /// Never held across a SQLite call, so a local stop never waits on
        /// the store.
        local: Mutex<Option<Live>>,
        /// The transition authority; see [`Activations`].
        activations: std::sync::Mutex<Activations>,
        /// Signalled whenever a stop obligation settles or fails.
        stops_changed: tokio::sync::Notify,
        #[cfg(test)]
        pub(crate) fake: std::sync::Mutex<Option<Arc<Fake>>>,
        /// Parks a publish worker after its final token check, before its
        /// commit transaction.
        #[cfg(test)]
        pub(crate) commit_gate: std::sync::Mutex<Option<FakeGate>>,
        /// Parks the next deferred revocation stop.
        #[cfg(test)]
        pub(crate) stop_gate: std::sync::Mutex<Option<FakeGate>>,
        /// Parks the next unpublish worker before it waits for the slot.
        #[cfg(test)]
        pub(crate) unpublish_gate: std::sync::Mutex<Option<FakeGate>>,
        /// Parks the next publish worker, uncancellably, before any effect.
        #[cfg(test)]
        pub(crate) activate_gate: std::sync::Mutex<Option<FakeGate>>,
        /// Deferred stop completions and registry clears, in order.
        #[cfg(test)]
        pub(crate) events: std::sync::Mutex<Vec<&'static str>>,
        /// The registry size each cleanup observed on entry.
        #[cfg(test)]
        pub(crate) cleanup_registry: std::sync::Mutex<Vec<usize>>,
        /// Parks a publish worker after its pre-submission cancellation check
        /// and before its DNS POST future is first polled.
        #[cfg(test)]
        pub(crate) submit_gate: std::sync::Mutex<Option<FakeGate>>,
        /// Parks a publish worker after its durable possible-POST marker and
        /// before its pre-submission cancellation check.
        #[cfg(test)]
        pub(crate) marker_gate: std::sync::Mutex<Option<FakeGate>>,
        /// Parks a publish worker inside its commit transaction, after the
        /// final authorization and before COMMIT. The transaction holds the
        /// store, so the release side is synchronous.
        #[cfg(test)]
        pub(crate) commit_hook: std::sync::Mutex<Option<CommitHook>>,
        /// Receipts whose worker is alive. A pending publication receipt
        /// outside this set lost its terminal write, and the reaper settles it.
        workers: std::sync::Mutex<std::collections::HashSet<i64>>,
    }

    /// Held from the runtime route check through the snapshot commit or abort.
    /// The dispatch gate precedes the effects slot, and registry/local guards
    /// are released before any SQLite operation. Local-origin release for the
    /// exact active generation and writer epoch is bound independently before
    /// admission and commit.
    pub(crate) struct SnapshotRoute {
        vm: Id,
        #[cfg(test)]
        origin_release: Option<(Id, Id)>,
        _slot: OwnedMutexGuard<()>,
    }

    impl SnapshotRoute {
        pub(crate) fn vm(&self) -> Id {
            self.vm
        }

        #[cfg(test)]
        pub(crate) fn bind_origin_release(&mut self, generation: Id, epoch: Id) {
            self.origin_release = Some((generation, epoch));
        }

        #[cfg(test)]
        pub(crate) fn origin_release(&self) -> Option<(Id, Id)> {
            self.origin_release
        }
    }

    impl Publication {
        pub(crate) fn snapshot_route(&self, vm: Id) -> Option<SnapshotRoute> {
            let slot = self.claim()?;
            let activations = self.activations();
            let registered = [activations.current.as_ref(), activations.pending.as_ref()]
                .into_iter()
                .flatten()
                .any(|activation| activation.vm == vm);
            if registered
                || activations.lost
                || activations.unconfirmed.is_some()
                || !activations.stops.is_empty()
            {
                return None;
            }
            drop(activations);
            if self
                .local
                .try_lock()
                .ok()?
                .as_ref()
                .is_some_and(|live| live.vm == vm)
            {
                return None;
            }
            Some(SnapshotRoute {
                vm,
                #[cfg(test)]
                origin_release: None,
                _slot: slot,
            })
        }

        pub(crate) fn prepare(
            &self,
            profile: &Arc<Profile>,
            session: Option<Arc<crate::vm::compose::Session>>,
        ) -> Result<Prepared, Refusal> {
            let routes = if profile.routes.is_empty() {
                None
            } else {
                Some(GuestRoutes {
                    session,
                    zone: profile.zone_name.clone().ok_or(Refusal::Invalid)?,
                })
            };
            #[cfg(test)]
            if let Some(fake) = self.fake.lock().unwrap().as_ref() {
                use std::sync::atomic::Ordering;
                if !fake.credential.load(Ordering::SeqCst) {
                    return Err(Refusal::PublishCredentialMissing);
                }
                return Ok(Prepared {
                    routes,
                    dns: Dns::Fixture(fake.clone()),
                    start: Start::Fixture(fake.clone()),
                });
            }
            profile.prepare(routes)
        }

        pub(crate) fn claim(&self) -> Option<OwnedMutexGuard<()>> {
            self.effects.clone().try_lock_owned().ok()
        }

        /// Waits for in-flight effects, for example a held DNS POST, to settle.
        pub(crate) async fn slot(&self) -> OwnedMutexGuard<()> {
            self.effects.clone().lock_owned().await
        }

        fn activations(&self) -> std::sync::MutexGuard<'_, Activations> {
            self.activations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        /// Registers the activation about to be admitted, before its admission
        /// transaction. The caller holds the dispatch gate and the effects slot,
        /// and then calls [`Self::admitted`] or [`Self::abandon`]. After the
        /// shutdown seal nothing registers.
        pub(crate) fn register(&self, vm: Id, owner: &str, nonce: &str) -> Option<Ticket> {
            let mut activations = self.activations();
            if activations.sealed {
                return None;
            }
            activations.issued += 1;
            let generation = activations.issued;
            let token = Shutdown::default();
            activations.pending = Some(Activation {
                generation,
                vm,
                owner: owner.to_owned(),
                nonce: nonce.to_owned(),
                token: token.clone(),
                revoked: false,
            });
            Some(Ticket {
                generation,
                revoked: token,
            })
        }

        /// The registered activation was admitted and is now current. The
        /// previous intent was released before admission, so its owner cause
        /// is spent.
        pub(crate) fn admitted(&self, generation: u64) {
            let mut activations = self.activations();
            if let Some(pending) = activations
                .pending
                .take_if(|pending| pending.generation == generation)
            {
                activations.current = Some(pending);
                activations.prune_causes(None);
            }
        }

        /// The publication of `vm` ended with its connector confirmed stopped.
        /// Called under the effects slot, where only the durable intent's
        /// activation can be current for `vm`.
        fn retire(&self, vm: Id) {
            self.activations()
                .current
                .take_if(|current| current.vm == vm);
        }

        /// The shutdown seal, in the transition that requests shutdown
        /// containment: no activation registers, no revocation records a stop
        /// obligation and no first DNS transport write passes afterwards.
        /// `shutdown` is notified after the seal is recorded.
        pub(crate) fn seal(&self, shutdown: &Shutdown) {
            let mut activations = self.activations();
            activations.sealed = true;
            shutdown.cancel();
        }

        /// Daemon shutdown after every worker and stop joined and local
        /// ingress is confirmed down. Owner causes and no-submission proof are
        /// cleared only once the publication settled.
        pub(crate) fn clear(&self, settled: bool) {
            #[cfg(test)]
            self.events.lock().unwrap().push("cleared");
            let mut activations = self.activations();
            activations.current = None;
            activations.pending = None;
            activations.stops.clear();
            if settled {
                activations.cancelled.clear();
                activations.unsubmitted.clear();
            }
        }

        /// A live connector failed its last stop attempt. Only this closes
        /// publish admission.
        pub(crate) fn stop_unconfirmed(&self) -> bool {
            self.activations().unconfirmed.is_some()
        }

        /// A worker or stop completion was lost to a panic or cancellation.
        #[cfg(test)]
        pub(crate) fn lost(&self) -> bool {
            self.activations().lost
        }

        #[cfg(test)]
        pub(crate) fn registry_len(&self) -> usize {
            let activations = self.activations();
            usize::from(activations.current.is_some()) + usize::from(activations.pending.is_some())
        }

        /// Recorded stop obligations, in any state.
        #[cfg(test)]
        pub(crate) fn stop_obligations(&self) -> usize {
            self.activations().stops.len()
        }

        /// Latched owner causes.
        #[cfg(test)]
        pub(crate) fn owner_causes(&self) -> usize {
            self.activations().cancelled.len()
        }

        fn unsubmitted(&self, nonce: &str, marker: &str) -> bool {
            self.activations()
                .unsubmitted
                .contains(&(nonce.to_owned(), marker.to_owned()))
        }

        fn mark_unsubmitted(&self, nonce: &str, marker: &str) {
            self.activations()
                .unsubmitted
                .insert((nonce.to_owned(), marker.to_owned()));
        }

        fn clear_unsubmitted(&self, nonce: &str) {
            self.activations()
                .unsubmitted
                .retain(|(entry_nonce, _)| entry_nonce != nonce);
        }

        /// An owner revocation cancelled the publication of this intent.
        fn owner_cancelled(&self, nonce: &str) -> bool {
            self.activations()
                .cancelled
                .iter()
                .any(|cancelled| cancelled == nonce)
        }

        /// `reason` after owner-cause precedence for this intent.
        fn cause(&self, nonce: &str, reason: Option<Failure>) -> Option<Failure> {
            if self.owner_cancelled(nonce) {
                Some(Failure::Cancelled)
            } else {
                reason
            }
        }

        /// The registered activation was refused and never ran.
        pub(crate) fn abandon(&self, generation: u64) {
            let mut activations = self.activations();
            if let Some(pending) = activations
                .pending
                .take_if(|pending| pending.generation == generation)
            {
                activations
                    .cancelled
                    .retain(|nonce| *nonce != pending.nonce);
            }
        }

        /// Owner revocation before any store, gate, slot or DNS dependency;
        /// see [`Activations::revoke`]. Whatever the request key: the replay
        /// ledger decides only the response and the durable bookkeeping. The
        /// returned obligations are already recorded; a runner only executes
        /// them, and the final drain services any whose runner never started.
        #[must_use]
        pub(crate) fn revoke_owned(&self, owner: &str, vm: Id) -> Vec<u64> {
            self.activations().revoke(vm, Some(owner))
        }

        /// Revokes every activation of `vm` after an admitted owner unpublish
        /// committed `revoking`.
        #[must_use]
        pub(crate) fn revoke(&self, vm: Id) -> Vec<u64> {
            self.activations().revoke(vm, None)
        }

        /// Records a joined worker or stop task. A panic or cancellation is a
        /// lost completion, whoever harvests it.
        pub(crate) fn record_join<T>(&self, joined: &Result<T, tokio::task::JoinError>) {
            if joined.is_err() {
                self.activations().lost = true;
            }
        }

        /// Held across a publication admission transaction, then records its
        /// receipt, so the reaper never treats a just-admitted receipt as lost.
        pub(crate) fn workers(&self) -> std::sync::MutexGuard<'_, std::collections::HashSet<i64>> {
            self.workers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        /// The worker for `receipt` exited.
        pub(crate) fn untrack(&self, receipt: i64) {
            self.workers().remove(&receipt);
        }

        /// Runs one recorded stop obligation if it is still requested. A
        /// coalesced or stale runner finds it claimed or settled and returns.
        pub(crate) async fn run_stop(&self, generation: u64) {
            let claimed = {
                let mut activations = self.activations();
                match activations.stops.get_mut(&generation) {
                    Some(stop @ Stop::Requested) => {
                        *stop = Stop::Running;
                        true
                    }
                    _ => false,
                }
            };
            if claimed {
                self.service_stop(generation).await;
            }
        }

        async fn service_stop(&self, generation: u64) {
            let mut claim = StopClaim {
                publication: self,
                generation,
                stopped: None,
            };
            claim.stopped = Some(self.stop_activation(generation).await);
        }

        /// The drain's stop settlement, after the seal: runs every requested
        /// obligation whose runner never started and waits for every running
        /// one. No obligation can be recorded meanwhile.
        pub(crate) async fn settle_stops(&self) {
            loop {
                let changed = self.stops_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                let (claimed, running) = {
                    let mut activations = self.activations();
                    let claimed: Vec<u64> = activations
                        .stops
                        .iter_mut()
                        .filter(|(_, stop)| **stop == Stop::Requested)
                        .map(|(generation, stop)| {
                            *stop = Stop::Running;
                            *generation
                        })
                        .collect();
                    let running = activations
                        .stops
                        .values()
                        .any(|stop| *stop == Stop::Running);
                    (claimed, running)
                };
                if !claimed.is_empty() {
                    for generation in claimed {
                        self.service_stop(generation).await;
                    }
                    continue;
                }
                if !running {
                    return;
                }
                changed.await;
            }
        }

        /// A deferred revocation stop, bound to one activation generation.
        async fn stop_activation(&self, generation: u64) -> bool {
            #[cfg(test)]
            let _ = hold(&self.stop_gate).await;
            let stopped = self.stop_where(|live| live.generation == generation).await;
            #[cfg(test)]
            self.events.lock().unwrap().push("stopped");
            stopped
        }

        /// Called before VM release. Local Job termination is independent of
        /// Cloudflare credentials, SQLite writes and outstanding DNS requests.
        pub(crate) async fn stop_local(&self, vm: Option<Id>) -> bool {
            self.stop_where(|live| vm.is_none_or(|id| live.vm == id))
                .await
        }

        async fn stop_where(&self, matches: impl Fn(&Live) -> bool) -> bool {
            let mut local = self.local.lock().await;
            let Some(live) = local.as_mut().filter(|live| matches(live)) else {
                if local.is_none() {
                    // No connector is live, so no stop can be outstanding and
                    // no failed stop obligation has anything left to stop.
                    let mut activations = self.activations();
                    activations.unconfirmed = None;
                    activations.stops.retain(|_, stop| *stop != Stop::Failed);
                }
                return true;
            };
            let generation = live.generation;
            if !live.connector.stop().await {
                self.activations().unconfirmed = Some(generation);
                return false;
            }
            *local = None;
            let mut activations = self.activations();
            if activations.unconfirmed == Some(generation) {
                activations.unconfirmed = None;
            }
            if activations.stops.get(&generation) == Some(&Stop::Failed) {
                activations.stops.remove(&generation);
            }
            // That activation's connector is confirmed stopped and a worker
            // never restarts it, so its publication's ingress has ended. A
            // registered generation with no connector yet is kept: its
            // cancelled worker still settles it.
            activations
                .current
                .take_if(|current| current.generation == generation);
            true
        }

        async fn ready(&self, intent: &Intent, shutdown: &Shutdown) -> Result<(), Failure> {
            if self.owner_cancelled(&intent.nonce) {
                return Err(Failure::Cancelled);
            }
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
            live.connector.ready(intent.phase != Phase::Published).await
        }

        #[allow(clippy::too_many_arguments)] // Explicit admission evidence and both cancellations.
        pub(crate) async fn publish<C: Clock>(
            &self,
            controller: &VmController<C>,
            operation: OperationRef,
            intent: Intent,
            prepared: Prepared,
            origin: Origin,
            ticket: &Ticket,
            shutdown: &Shutdown,
        ) -> Result<()> {
            let (generation, revoked) = (ticket.generation, &ticket.revoked);
            let result = self
                .activate(controller, &intent, prepared, &origin, ticket, shutdown)
                .await;
            // An unremoved ingress residue is never confirmed cleanup, even
            // though no process or DNS effect remains. The receipt is recorded
            // as unconfirmed before any later step can fail, so whichever path
            // settles it (this worker, the reaper or the final drain as an
            // orphan) records it as unconfirmed. It is dropped only once this
            // worker's own terminal write is durable.
            let residue = self.activations().residue == Some(generation);
            let receipt = operation.receipt.activity_id;
            if residue {
                self.activations().unconfirmed_receipts.insert(receipt);
            }
            let authorized =
                || origin.current() && !shutdown.is_cancelled() && !revoked.is_cancelled();
            let commit_authorized = || {
                let authorized = authorized();
                #[cfg(test)]
                if let Some((entered, release)) = self.commit_hook.lock().unwrap().take() {
                    let _ = entered.send(());
                    let _ = release.recv();
                }
                authorized
            };
            let live = self
                .local
                .lock()
                .await
                .as_ref()
                .is_some_and(|live| live.generation == generation);
            if result.is_ok() && live && authorized() {
                #[cfg(test)]
                let _ = hold(&self.commit_gate).await;
                // The final authorization runs inside the commit transaction,
                // and the durable transition requires a still-`publishing`
                // intent. An owner revocation, an origin retirement or a
                // committed `revoking` before it always wins. The local slot is
                // not held here, so a revocation stop never waits on SQLite.
                match controller.commit_publication(operation, &intent.nonce, commit_authorized) {
                    Ok(true) if authorized() => return Ok(()),
                    Ok(true) => {
                        // A revocation, origin retirement or shutdown landed
                        // inside the transaction, after its authorization and
                        // before COMMIT. The receipt stands, and this worker
                        // drives the activation back to unpublished itself.
                        let reason = if origin.current() {
                            Failure::Cancelled
                        } else {
                            Failure::OriginUnavailable
                        };
                        let bound = shutdown.is_cancelled().then_some(shutdown);
                        self.cleanup(
                            controller,
                            intent.vm,
                            Some(reason),
                            Evidence::Unknown,
                            bound,
                        )
                        .await?;
                        return Ok(());
                    }
                    Ok(false) => {}
                    Err(error) => {
                        self.stop_activation(generation).await;
                        return Err(error);
                    }
                }
            }

            let (failure, evidence) = match result {
                Err((failure, evidence)) => (failure, evidence),
                Ok(()) if !origin.current() => (Failure::OriginUnavailable, Evidence::Unknown),
                Ok(()) => (Failure::Cancelled, Evidence::Unknown),
            };
            let cleanup = self
                .cleanup(
                    controller,
                    intent.vm,
                    Some(failure),
                    evidence,
                    Some(shutdown),
                )
                .await?;
            let clean = cleanup == Cleanup::Released && !residue;
            // A latched owner cause wins over this worker's local failure.
            let failure = self.cause(&intent.nonce, Some(failure));
            controller.finish_publication(operation, None, failure, Some(clean))?;
            self.activations().unconfirmed_receipts.remove(&receipt);
            Ok(())
        }

        /// Errors carry this worker's POST evidence: an acknowledged record ID
        /// that could not be made durable, or proof that it never submitted.
        #[allow(clippy::too_many_arguments)] // The activation identity and both cancellations.
        async fn activate<C: Clock>(
            &self,
            controller: &VmController<C>,
            intent: &Intent,
            prepared: Prepared,
            origin: &Origin,
            ticket: &Ticket,
            shutdown: &Shutdown,
        ) -> Result<(), (Failure, Evidence)> {
            let (generation, revoked) = (ticket.generation, &ticket.revoked);
            #[cfg(test)]
            let _ = hold(&self.activate_gate).await;
            if intent.hosts.is_empty() {
                return Err((Failure::OriginUnavailable, Evidence::Unknown));
            }
            let routes: Vec<(&HostIntent, Route)> = intent
                .hosts
                .iter()
                .map(|host| Ok((host, host_route(intent, host)?)))
                .collect::<Result<_, Failure>>()
                .map_err(|failure| (failure, Evidence::Unknown))?;
            let map = |error| match error {
                DnsError::Conflict => (Failure::DnsConflict, Evidence::Unknown),
                _ => (Failure::DnsUnavailable, Evidence::Unknown),
            };
            if !origin.current() || shutdown.is_cancelled() {
                return Err((Failure::OriginUnavailable, Evidence::Unknown));
            }
            // Preflight every hostname in stable position order before any
            // connector start or DNS mutation.
            for (_, route) in &routes {
                tokio::select! {
                    biased;
                    _ = revoked.cancelled() => return Err((Failure::Cancelled, Evidence::Unknown)),
                    _ = origin.retired.token().cancelled() => return Err((Failure::OriginUnavailable, Evidence::Unknown)),
                    _ = shutdown.cancelled() => return Err((Failure::Cancelled, Evidence::Unknown)),
                    result = prepared.dns.preflight(route) => result.map_err(map)?,
                }
            }
            {
                let mut local = self.local.lock().await;
                if local.is_some() {
                    return Err((Failure::CleanupUnconfirmed, Evidence::Unknown));
                }
                let stop = Linked::new(revoked, shutdown);
                if let Some(routes) = prepared.routes {
                    let unavailable = (Failure::OriginUnavailable, Evidence::Unknown);
                    let session = routes.session.ok_or(unavailable)?;
                    let install = session.install_routes(
                        controller.compose_store(),
                        intent,
                        origin.name().map_err(|e| (e, Evidence::Unknown))?,
                        routes.zone,
                        &stop.0,
                    );
                    tokio::select! {
                        biased;
                        _ = origin.retired.token().cancelled() => return Err((Failure::OriginUnavailable, Evidence::Unknown)),
                        _ = stop.0.cancelled() => return Err((Failure::Cancelled, Evidence::Unknown)),
                        result = install => result.map_err(|_| (Failure::OriginUnavailable,Evidence::Unknown))?,
                    }
                }
                let mut unsettled = Unsettled::default();
                let started = prepared
                    .start
                    .connect(intent, origin, &stop.0, &mut unsettled)
                    .await;
                match unsettled {
                    Unsettled::Nothing => {}
                    Unsettled::Process(connector) => {
                        *local = Some(Live {
                            vm: intent.vm,
                            generation,
                            origin: origin.clone(),
                            connector,
                        });
                    }
                    // Recorded for this publication's receipt and for the
                    // final drain, which cannot claim a clean settlement.
                    Unsettled::Residue => self.activations().residue = Some(generation),
                }
                let connector = started.map_err(|failure| (failure, Evidence::Unknown))?;
                *local = Some(Live {
                    vm: intent.vm,
                    generation,
                    origin: origin.clone(),
                    connector,
                });
            }
            self.ready(intent, shutdown)
                .await
                .map_err(|failure| (failure, Evidence::Unknown))?;
            // Create and verify every hostname in stable position order with
            // independent per-row ownership markers, custody and cancellation
            // evidence.
            for (host, route) in &routes {
                controller
                    .update_publication(
                        &intent.nonce,
                        Update::HostPostPossible {
                            position: host.position,
                        },
                    )
                    .map_err(|_| (Failure::CleanupUnconfirmed, Evidence::Unknown))?;
                #[cfg(test)]
                let _ = hold(&self.marker_gate).await;
                if !origin.current() || shutdown.is_cancelled() || revoked.is_cancelled() {
                    self.mark_unsubmitted(&intent.nonce, &host.marker);
                    let failure = if origin.current() {
                        Failure::Cancelled
                    } else {
                        Failure::OriginUnavailable
                    };
                    let _withdrawn = controller
                        .update_publication(
                            &intent.nonce,
                            Update::HostPostWithdrawn {
                                position: host.position,
                            },
                        )
                        .is_ok();
                    return Err((
                        failure,
                        Evidence::NotSubmitted {
                            position: host.position,
                        },
                    ));
                }
                #[cfg(test)]
                let _ = hold(&self.submit_gate).await;
                let cause = || {
                    if origin.retired.is_retired() {
                        Some(Failure::OriginUnavailable)
                    } else if revoked.is_cancelled() || shutdown.is_cancelled() {
                        Some(Failure::Cancelled)
                    } else {
                        None
                    }
                };
                let permit = Permit {
                    publication: self,
                    generation,
                    origin,
                    shutdown,
                    possibly: Default::default(),
                };
                let submission = Submission::with(&permit);
                let id = tokio::select! {
                    biased;
                    _ = origin.retired.token().cancelled() => Err(None),
                    _ = shutdown.cancelled() => Err(None),
                    // Owner revocation abandons the POST only while it is unsent.
                    _ = async {
                        revoked.cancelled().await;
                        if submission.sent() == Sent::Possibly {
                            std::future::pending::<()>().await;
                        }
                    } => Err(None),
                    result = prepared.dns.create(route, &submission) => result.map_err(Some),
                };
                let id = match id {
                    Ok(id) => id,
                    Err(error) if submission.sent() == Sent::Possibly => {
                        return Err(match error {
                            Some(error) => map(error),
                            None => (cause().unwrap_or(Failure::Cancelled), Evidence::Unknown),
                        });
                    }
                    Err(error) => {
                        let cause = cause()
                            .or(error.map(|error| map(error).0))
                            .unwrap_or(Failure::Cancelled);
                        self.mark_unsubmitted(&intent.nonce, &host.marker);
                        let _withdrawn = controller
                            .update_publication(
                                &intent.nonce,
                                Update::HostPostWithdrawn {
                                    position: host.position,
                                },
                            )
                            .is_ok();
                        return Err((
                            cause,
                            Evidence::NotSubmitted {
                                position: host.position,
                            },
                        ));
                    }
                };
                // The acknowledged ID is durable before verification, so a
                // failed exact lookup can never discard it.
                if controller
                    .update_publication(
                        &intent.nonce,
                        Update::HostRecord {
                            position: host.position,
                            record: id.as_str(),
                        },
                    )
                    .is_err()
                {
                    return Err((
                        Failure::CleanupUnconfirmed,
                        Evidence::Acknowledged {
                            position: host.position,
                            id,
                        },
                    ));
                }
                tokio::select! {
                    biased;
                    _ = origin.retired.token().cancelled() => return Err((Failure::OriginUnavailable, Evidence::Unknown)),
                    _ = shutdown.cancelled() => return Err((Failure::Cancelled, Evidence::Unknown)),
                    result = prepared.dns.verify(route, &id) => result.map_err(map)?,
                }
                controller
                    .update_publication(
                        &intent.nonce,
                        Update::HostVerified {
                            position: host.position,
                        },
                    )
                    .map_err(|_| (Failure::CleanupUnconfirmed, Evidence::Unknown))?;
            }
            self.ready(intent, shutdown)
                .await
                .map_err(|failure| (failure, Evidence::Unknown))
        }

        /// Waits for the effects slot. Its pending receipt keeps publish
        /// admission for `vm` closed, so no later activation can start before
        /// this worker settles.
        pub(crate) async fn unpublish<C: Clock>(
            &self,
            controller: &VmController<C>,
            operation: OperationRef,
            vm: Id,
            shutdown: &Shutdown,
        ) -> Result<()> {
            #[cfg(test)]
            let _ = hold(&self.unpublish_gate).await;
            let _slot = self.slot().await;
            let released = self
                .cleanup(controller, vm, None, Evidence::Unknown, Some(shutdown))
                .await?
                == Cleanup::Released;
            controller.finish_publication(
                operation,
                None,
                (!released).then_some(Failure::CleanupUnconfirmed),
                Some(released),
            )
        }

        /// One reaper turn. Skips as busy while another effect holds the slot.
        pub(crate) async fn turn<C: Clock>(
            &self,
            controller: &VmController<C>,
            shutdown: &Shutdown,
        ) -> Result<Turn> {
            let Some(_slot) = self.claim() else {
                return Ok(Turn::Busy);
            };
            self.reconcile(controller, shutdown).await
        }

        /// The final drain's reconciliation, after every worker and stop
        /// joined and local ingress is confirmed down. It waits for the
        /// exclusive effects slot within `deadline`. Clean requires no intent,
        /// no pending publication receipt, no lost completion and no residue;
        /// a timeout or failure is unconfirmed and leaves the durable intent,
        /// cause and evidence in place. Settled entries are cleared, and owner
        /// causes and no-submission proof only on a clean settlement.
        pub(crate) async fn finish<C: Clock>(
            &self,
            controller: &VmController<C>,
            shutdown: &Shutdown,
            deadline: tokio::time::Instant,
        ) -> Drained {
            let turn = tokio::time::timeout_at(deadline, async {
                let _slot = self.slot().await;
                self.reconcile(controller, shutdown).await
            })
            .await;
            let outcome = match turn {
                Ok(Ok(Turn::Idle)) => match controller.publication_receipts_pending() {
                    Ok(0) => None,
                    _ => Some(Unconfirmed::Publication),
                },
                Ok(Ok(_)) => Some(Unconfirmed::Publication),
                Ok(Err(_)) | Err(_) => Some(Unconfirmed::Reconciliation),
            };
            let (lost, residue) = {
                let activations = self.activations();
                (activations.lost, activations.residue.is_some())
            };
            let outcome = if lost {
                Some(Unconfirmed::LostCompletion)
            } else if residue {
                Some(Unconfirmed::LocalResidue)
            } else {
                outcome
            };
            self.clear(outcome.is_none());
            outcome.map_or(Drained::Clean, Drained::Unconfirmed)
        }

        async fn reconcile<C: Clock>(
            &self,
            controller: &VmController<C>,
            shutdown: &Shutdown,
        ) -> Result<Turn> {
            let intent = controller.publication_intent()?;
            {
                // No-submission proof and owner causes are kept only for the
                // durable intent and live activations.
                let mut activations = self.activations();
                if let Some(intent) = &intent {
                    activations
                        .unsubmitted
                        .retain(|(nonce, _)| nonce == &intent.nonce);
                } else {
                    activations.unsubmitted.clear();
                }
                activations.prune_causes(intent.as_ref().map(|intent| intent.nonce.as_str()));
            }
            let turn = match intent {
                None => Turn::Idle,
                Some(intent) => {
                    let stale = if intent.phase == Phase::Published {
                        self.ready(&intent, shutdown).await.err().map(Some)
                    } else {
                        // A latched owner cause wins over a durable generic
                        // reason, including when its own checkpoint never
                        // became durable.
                        Some(self.cause(&intent.nonce, intent.reason))
                    };
                    match stale {
                        None => Turn::Healthy,
                        Some(reason) => match self
                            .cleanup(controller, intent.vm, reason, Evidence::Unknown, None)
                            .await?
                        {
                            Cleanup::Released => Turn::Idle,
                            Cleanup::Pending(why) => Turn::Pending(why),
                        },
                    }
                }
            };
            // Under the slot no worker settles concurrently. A receipt whose
            // worker exited without its terminal write is settled here once
            // its VM has no publication intent left.
            let unconfirmed = self.activations().unconfirmed_receipts.clone();
            let settled = {
                let workers = self.workers();
                controller.settle_orphaned_publications(&workers, &unconfirmed)?
            };
            let mut activations = self.activations();
            for receipt in settled {
                activations.unconfirmed_receipts.remove(&receipt);
            }
            Ok(turn)
        }

        async fn cleanup<C: Clock>(
            &self,
            controller: &VmController<C>,
            vm: Id,
            reason: Option<Failure>,
            evidence: Evidence,
            shutdown: Option<&Shutdown>,
        ) -> Result<Cleanup> {
            #[cfg(test)]
            self.cleanup_registry
                .lock()
                .unwrap()
                .push(self.registry_len());
            // Under the effects slot only this VM's current or leftover
            // connector can be live, so a VM-scoped stop is exact here.
            let local = self.stop_local(Some(vm)).await;
            if local {
                self.retire(vm);
            }
            let Some(intent) = controller
                .publication_intent()?
                .filter(|intent| intent.vm == vm)
            else {
                return Ok(if local {
                    Cleanup::Released
                } else {
                    Cleanup::Pending(Failure::CleanupUnconfirmed)
                });
            };
            // A latched owner cause wins over the reason this cleanup supplies.
            let reason = self.cause(&intent.nonce, reason);
            controller.update_publication(&intent.nonce, Update::Phase(Phase::Revoking, reason))?;
            // Clean EVERY child row independently without short-circuiting:
            // owned DNS records are removed even when a different row is
            // blocked or ambiguous.
            let mut first_cleanup_error: Option<Failure> = None;
            let (mut acknowledged, not_submitted_pos) = match evidence {
                Evidence::Acknowledged { position, id } => (Some((position, id)), None),
                Evidence::NotSubmitted { position } => (None, Some(position)),
                Evidence::Unknown => (None, None),
            };
            for host in &intent.hosts {
                if host.state == HostState::Cleaned {
                    continue;
                }
                // A proven unsubmitted POST needs no remote work, so it is
                // decided before DNS and before cancellation can win.
                let unsubmitted = host.record.is_none()
                    && (not_submitted_pos == Some(host.position)
                        || self.unsubmitted(&intent.nonce, &host.marker));
                let row_ack = acknowledged
                    .as_ref()
                    .is_some_and(|(pos, _)| *pos == host.position)
                    .then(|| acknowledged.take().map(|(_, id)| id))
                    .flatten();
                let row_result = if !host.post_possible || unsubmitted {
                    Ok(())
                } else if let Some(shutdown) = shutdown {
                    tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => Err(Failure::Cancelled),
                        result = self.remove_host_dns(controller, &intent, host, row_ack) => result,
                    }
                } else {
                    self.remove_host_dns(controller, &intent, host, row_ack)
                        .await
                };
                match row_result {
                    Ok(()) => {
                        if controller
                            .update_publication(
                                &intent.nonce,
                                Update::HostCleaned {
                                    position: host.position,
                                },
                            )
                            .is_err()
                        {
                            first_cleanup_error.get_or_insert(Failure::CleanupUnconfirmed);
                        }
                    }
                    Err(err) => {
                        let still_ambiguous =
                            host.record.is_none() && err == Failure::CleanupUnconfirmed;
                        let _ = controller.update_publication(
                            &intent.nonce,
                            Update::HostCleanupPending {
                                position: host.position,
                                ambiguous: still_ambiguous,
                                reason: Some(err),
                            },
                        );
                        first_cleanup_error.get_or_insert(err);
                    }
                }
            }
            let outcome = match (first_cleanup_error, local) {
                (None, true) => Cleanup::Released,
                (dns_err, _) => {
                    Cleanup::Pending(reason.or(dns_err).unwrap_or(Failure::CleanupUnconfirmed))
                }
            };
            let update = match outcome {
                Cleanup::Released => Update::Released(reason),
                Cleanup::Pending(why) => Update::Phase(Phase::CleanupPending, Some(why)),
            };
            let released = outcome == Cleanup::Released;
            if shutdown.is_none() {
                controller.reconcile_publication(&intent, update, reason, released)?;
            } else {
                controller.update_publication(&intent.nonce, update)?;
            }
            if released {
                self.clear_unsubmitted(&intent.nonce);
            }
            Ok(outcome)
        }

        async fn remove_host_dns<C: Clock>(
            &self,
            controller: &VmController<C>,
            intent: &Intent,
            host: &HostIntent,
            acknowledged: Option<RecordId>,
        ) -> Result<(), Failure> {
            if !host.post_possible {
                return Ok(());
            }
            let dns = self.cleanup_dns(intent).ok_or(Failure::DnsUnavailable)?;
            let route = host_route(intent, host)?;
            let unavailable = |_| Failure::DnsUnavailable;
            let id = match (&host.record, acknowledged) {
                (Some(id), _) => {
                    RecordId::parse(id.clone()).map_err(|_| Failure::DnsUnavailable)?
                }
                // An acknowledged ID whose custody write failed: retry custody
                // and target it either way.
                (None, Some(id)) => {
                    let _ = controller.update_publication(
                        &intent.nonce,
                        Update::HostRecord {
                            position: host.position,
                            record: id.as_str(),
                        },
                    );
                    id
                }
                (None, None) => match dns.reconcile(&route).await.map_err(unavailable)? {
                    Reconciled::Owned(id) | Reconciled::OwnedWithConflict(id) => {
                        controller
                            .update_publication(
                                &intent.nonce,
                                Update::HostRecord {
                                    position: host.position,
                                    record: id.as_str(),
                                },
                            )
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
        /// Connector starts, including failed ones.
        pub(crate) starts: std::sync::atomic::AtomicUsize,
        /// The next N connector stops fail and leave the connector live.
        pub(crate) stop_failures: std::sync::atomic::AtomicUsize,
        /// The next connector start fails with an unremoved ingress residue.
        pub(crate) residue: std::sync::atomic::AtomicBool,
        /// The next connector start fails after spawn with its process still
        /// running and unconfirmed.
        pub(crate) start_held: std::sync::atomic::AtomicBool,
        /// Parks the next connector start before readiness, abortably.
        pub(crate) start_gate: std::sync::Mutex<Option<FakeGate>>,
        /// The next connector stop panics before stopping anything.
        pub(crate) stop_panic: std::sync::atomic::AtomicBool,
        /// The next POST panics its worker after its first byte was accepted.
        pub(crate) panic_after_post: std::sync::atomic::AtomicBool,
        /// Parks the next record removal.
        pub(crate) remove_gate: std::sync::Mutex<Option<FakeGate>>,
        pub(crate) posts: std::sync::atomic::AtomicUsize,
        pub(crate) deletes: std::sync::atomic::AtomicUsize,
        pub(crate) ingress: std::sync::Mutex<Option<String>>,
        pub(crate) record: Arc<std::sync::atomic::AtomicBool>,
        pub(crate) post_gate: std::sync::Mutex<Option<FakeGate>>,
        /// Parks the next create during connection and TLS setup, before its
        /// first write.
        pub(crate) tls_gate: std::sync::Mutex<Option<FakeGate>>,
        /// The next first write fails with zero bytes accepted.
        pub(crate) first_write_error: std::sync::atomic::AtomicBool,
        pub(crate) preflight_gate: std::sync::Mutex<Option<FakeGate>>,
        /// The next POST commits only after release, in a task that outlives
        /// the abandoned request, and never acknowledges.
        pub(crate) late_commit: std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
        /// The next exact lookup fails after the acknowledged record vanished.
        pub(crate) lose_after_post: std::sync::atomic::AtomicBool,
        /// Every route tuple used for lookup or removal.
        pub(crate) routes: std::sync::Mutex<Vec<String>>,
        /// Multi-host mode: tracks per-hostname ownership, conflicts, failures
        /// and call ordering independently.
        pub(crate) multi_host: std::sync::atomic::AtomicBool,
        /// Owned DNS records per hostname: hostname -> (route_witness, record_id).
        pub(crate) host_records:
            Arc<std::sync::Mutex<std::collections::BTreeMap<String, (String, String)>>>,
        /// Hostnames currently carrying a foreign/conflicting DNS record.
        pub(crate) host_foreign: std::sync::Mutex<std::collections::BTreeSet<String>>,
        /// Hostnames whose next POST fails after the first write.
        pub(crate) host_fail_create: std::sync::Mutex<std::collections::BTreeSet<String>>,
        /// Hostnames whose POST commits only after release and never acknowledges.
        pub(crate) host_late_commit: std::sync::Mutex<
            std::collections::BTreeMap<String, tokio::sync::oneshot::Receiver<()>>,
        >,
        /// Hostnames whose DNS removal currently fails (transient error).
        pub(crate) host_fail_remove: std::sync::Mutex<std::collections::BTreeSet<String>>,
        /// Hostnames that fail the origin TLS/Host probe before connector start.
        pub(crate) host_origin_mismatch: std::sync::Mutex<std::collections::BTreeSet<String>>,
        /// Observed origin Host probes, preflight order, create order and remove order.
        pub(crate) starts_at_route_install: std::sync::Mutex<Vec<usize>>,
        pub(crate) upstream_unavailable: std::sync::atomic::AtomicBool,
        pub(crate) origin_probed: std::sync::Mutex<Vec<String>>,
        pub(crate) preflight_order: std::sync::Mutex<Vec<String>>,
        pub(crate) create_order: std::sync::Mutex<Vec<String>>,
        pub(crate) remove_order: std::sync::Mutex<Vec<String>>,
    }

    #[cfg(test)]
    pub(crate) type CommitHook = (
        tokio::sync::oneshot::Sender<()>,
        std::sync::mpsc::Receiver<()>,
    );

    #[cfg(test)]
    pub(crate) type FakeGate = (
        tokio::sync::oneshot::Sender<()>,
        tokio::sync::oneshot::Receiver<()>,
    );

    #[cfg(test)]
    async fn hold(gate: &std::sync::Mutex<Option<FakeGate>>) -> Result<(), DnsError> {
        let gate = gate.lock().unwrap().take();
        if let Some((started, release)) = gate {
            let _ = started.send(());
            release.await.map_err(|_| DnsError::Unavailable)?;
        }
        Ok(())
    }

    #[cfg(test)]
    impl Profile {
        pub(crate) fn synthetic(host: Id, name: &str) -> Self {
            Self::drifted(host, name, "publish.example.invalid", &"b".repeat(32))
        }

        /// A replaced owner binding, for drift witnesses.
        pub(crate) fn drifted(host: Id, name: &str, hostname: &str, zone: &str) -> Self {
            Self::synthetic_multi(host, name, &[hostname], None, zone)
        }

        pub(crate) fn synthetic_multi(
            host: Id,
            name: &str,
            hostnames: &[&str],
            zone_name: Option<&str>,
            zone: &str,
        ) -> Self {
            Self {
                hostnames: hostnames
                    .iter()
                    .map(|value| Hostname::parse(value).unwrap())
                    .collect(),
                zone_name: zone_name.map(|value| Hostname::parse(value).unwrap()),
                routes: if hostnames.len() <= 1 {
                    Vec::new()
                } else {
                    hostnames
                        .iter()
                        .enumerate()
                        .map(|(index, value)| RouteBinding {
                            hostname: Hostname::parse(value).unwrap(),
                            project_id: format!("project-{}", index / 4),
                            service: format!("svc-{}", index % 4),
                            project_slot: (index / 4) as u8,
                            service_index: (index % 4) as u8,
                            route_slot: index as u8,
                        })
                        .collect()
                },
                static_name: name.into(),
                host_id: hex::encode(host),
                tunnel_id: "12345678-1234-4321-8234-123456789abc".into(),
                account_id: "a".repeat(32),
                zone_id: zone.into(),
                private_root: "D:/synthetic".into(),
                dns_ciphertext: "D:/synthetic/dns.dpapi".into(),
                tunnel_ciphertext: "D:/synthetic/tunnel.dpapi".into(),
                connector: "D:/synthetic-dist/cloudflared.exe".into(),
                connector_length: 1,
                connector_sha256: "c".repeat(64),
            }
        }

        pub(crate) fn with_routes(mut self, routes: Vec<RouteBinding>) -> Self {
            self.routes = routes;
            self
        }

        pub(crate) fn static_name(&self) -> &str {
            &self.static_name
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

        fn probe_origin_hosts(&self, hostnames: &[Hostname]) -> Result<(), Failure> {
            let mut probed = self.origin_probed.lock().unwrap();
            let mismatch = self.host_origin_mismatch.lock().unwrap();
            for host in hostnames {
                probed.push(host.as_str().to_owned());
                if mismatch.contains(host.as_str()) {
                    return Err(Failure::OriginUnavailable);
                }
            }
            Ok(())
        }

        fn id() -> RecordId {
            RecordId::parse("e".repeat(32)).unwrap()
        }

        fn multi_id(seq: usize) -> RecordId {
            RecordId::parse(format!("{seq:032x}")).unwrap()
        }
    }

    #[cfg(test)]
    impl DnsApi for Fake {
        async fn preflight(&self, route: &Route) -> Result<(), DnsError> {
            use std::sync::atomic::Ordering::SeqCst;
            hold(&self.preflight_gate).await?;
            if self.multi_host.load(SeqCst) {
                self.preflight_order
                    .lock()
                    .unwrap()
                    .push(route.name().as_str().to_owned());
            }
            match self.reconcile(route).await? {
                Reconciled::Absent => Ok(()),
                _ => Err(DnsError::Conflict),
            }
        }

        async fn create(
            &self,
            route: &Route,
            submission: &Submission<'_>,
        ) -> Result<RecordId, DnsError> {
            use std::sync::atomic::Ordering::SeqCst;
            // Connection and TLS setup: nothing is written yet.
            hold(&self.tls_gate).await?;
            // One attempt of the first underlying write.
            let failing = self.first_write_error.swap(false, SeqCst);
            let written = submission.first_write(|| {
                std::task::Poll::Ready(if failing {
                    Err(std::io::Error::other("zero-byte first write failure"))
                } else {
                    Ok(1)
                })
            });
            if !matches!(written, std::task::Poll::Ready(Ok(_))) {
                return Err(DnsError::Unavailable);
            }
            assert!(
                self.local.load(SeqCst),
                "DNS activated before connector readiness"
            );
            if self.multi_host.load(SeqCst) {
                let name = route.name().as_str().to_owned();
                let witness = route.witness();
                let seq = self.posts.fetch_add(1, SeqCst) + 1;
                self.create_order.lock().unwrap().push(name.clone());
                let id = Self::multi_id(seq);
                let id_str = id.as_str().to_owned();
                let late = self.host_late_commit.lock().unwrap().remove(&name);
                if let Some(release) = late {
                    let records = self.host_records.clone();
                    tokio::spawn(async move {
                        if release.await.is_ok() {
                            records.lock().unwrap().insert(name, (witness, id_str));
                        }
                    });
                    return std::future::pending().await;
                }
                if self.host_fail_create.lock().unwrap().remove(&name) {
                    return Err(DnsError::Unavailable);
                }
                {
                    let mut records = self.host_records.lock().unwrap();
                    assert!(!records.contains_key(&name), "repeated POST for {name}");
                    records.insert(name, (witness, id_str));
                }
                if self.panic_after_post.swap(false, SeqCst) {
                    panic!("synthetic worker panic after a possible POST");
                }
                hold(&self.post_gate).await?;
                return Ok(id);
            }
            assert!(!self.record.load(SeqCst), "repeated POST");
            self.posts.fetch_add(1, SeqCst);
            let late = self.late_commit.lock().unwrap().take();
            if let Some(release) = late {
                let record = self.record.clone();
                tokio::spawn(async move {
                    if release.await.is_ok() {
                        record.store(true, SeqCst);
                    }
                });
                return std::future::pending().await;
            }
            self.record.store(true, SeqCst);
            if self.panic_after_post.swap(false, SeqCst) {
                panic!("synthetic worker panic after a possible POST");
            }
            hold(&self.post_gate).await?;
            Ok(Self::id())
        }

        async fn reconcile(&self, route: &Route) -> Result<Reconciled, DnsError> {
            use std::sync::atomic::Ordering::SeqCst;
            let witness = route.witness();
            self.routes.lock().unwrap().push(witness.clone());
            if self.multi_host.load(SeqCst) {
                let name = route.name().as_str();
                let owned =
                    self.host_records
                        .lock()
                        .unwrap()
                        .get(name)
                        .and_then(|(stored_witness, id)| {
                            (stored_witness == &witness)
                                .then(|| RecordId::parse(id.clone()).ok())
                                .flatten()
                        });
                let foreign = self.host_foreign.lock().unwrap().contains(name);
                return Ok(match (owned, foreign) {
                    (Some(id), false) => Reconciled::Owned(id),
                    (Some(id), true) => Reconciled::OwnedWithConflict(id),
                    (None, true) => Reconciled::ForeignOrAmbiguous,
                    (None, false) => Reconciled::Absent,
                });
            }
            if self.record.load(SeqCst) && self.lose_after_post.swap(false, SeqCst) {
                self.record.store(false, SeqCst);
                return Err(DnsError::Unavailable);
            }
            Ok(
                match (self.record.load(SeqCst), self.foreign.load(SeqCst)) {
                    (true, false) => Reconciled::Owned(Self::id()),
                    (true, true) => Reconciled::OwnedWithConflict(Self::id()),
                    (false, true) => Reconciled::ForeignOrAmbiguous,
                    (false, false) => Reconciled::Absent,
                },
            )
        }

        async fn remove(&self, route: &Route, id: &RecordId) -> Result<Removal, DnsError> {
            use std::sync::atomic::Ordering::SeqCst;
            hold(&self.remove_gate).await?;
            let witness = route.witness();
            self.routes.lock().unwrap().push(witness.clone());
            assert!(
                !self.local.load(SeqCst),
                "DNS removed before local connector stopped"
            );
            if self.multi_host.load(SeqCst) {
                let name = route.name().as_str();
                self.remove_order.lock().unwrap().push(name.to_owned());
                if self.host_fail_remove.lock().unwrap().contains(name) {
                    return Err(DnsError::Unavailable);
                }
                let removed = {
                    let mut records = self.host_records.lock().unwrap();
                    match records.get(name) {
                        Some((stored_witness, stored_id))
                            if stored_witness == &witness && stored_id == id.as_str() =>
                        {
                            records.remove(name);
                            true
                        }
                        _ => false,
                    }
                };
                if removed {
                    self.deletes.fetch_add(1, SeqCst);
                }
                if self.host_foreign.lock().unwrap().contains(name) {
                    return Ok(Removal::ForeignOrAmbiguous);
                }
                return Ok(Removal::Absent);
            }
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

    /// The first DNS transport write runs under one activation's permit. Each
    /// cancellation source either precedes it and withholds it, or waits for
    /// an in-flight first write and then finds the POST possibly sent.
    #[cfg(test)]
    mod authority {
        use super::*;
        use std::{sync::mpsc::RecvTimeoutError, task::Poll, time::Duration};

        const VM: Id = [7; 16];

        fn origin() -> Origin {
            Origin {
                origin_generation: [1; 16],
                generation: [1; 16],
                boot: [2; 16],
                port: NonZeroU16::new(ORIGIN_PORT_FIRST).unwrap(),
                ca_path: PathBuf::new(),
                ca_receipt: ArtifactPin {
                    size: 1,
                    sha256: [1; 32],
                },
                retired: Retirement::default(),
            }
        }

        fn admitted(publication: &Publication) -> u64 {
            let ticket = publication.register(VM, "alpha", "nonce").unwrap();
            publication.admitted(ticket.generation);
            ticket.generation
        }

        fn permit<'a>(
            publication: &'a Publication,
            generation: u64,
            origin: &'a Origin,
            shutdown: &'a Shutdown,
        ) -> Permit<'a> {
            Permit {
                publication,
                generation,
                origin,
                shutdown,
                possibly: Default::default(),
            }
        }

        type Transition = fn(&Publication, &Origin, &Shutdown);

        fn owner_revocation(publication: &Publication, _: &Origin, _: &Shutdown) {
            let _ = publication.revoke_owned("alpha", VM);
        }

        fn shutdown_seal(publication: &Publication, _: &Origin, shutdown: &Shutdown) {
            publication.seal(shutdown);
        }

        fn origin_retirement(_: &Publication, origin: &Origin, _: &Shutdown) {
            origin.retired.retire();
        }

        const TRANSITIONS: [(&str, Transition); 3] = [
            ("owner revocation", owner_revocation),
            ("shutdown seal", shutdown_seal),
            ("origin retirement", origin_retirement),
        ];

        #[test]
        fn a_preceding_transition_withholds_the_first_write() {
            for (name, transition) in TRANSITIONS {
                let (publication, origin, shutdown) =
                    (Publication::default(), origin(), Shutdown::default());
                let generation = admitted(&publication);
                transition(&publication, &origin, &shutdown);
                let permit = permit(&publication, generation, &origin, &shutdown);
                let mut written = false;
                let result = Submission::with(&permit).first_write(|| {
                    written = true;
                    Poll::Ready(Ok(1))
                });
                assert!(
                    matches!(result, Poll::Ready(Err(_))),
                    "{name}: a first write passed"
                );
                assert!(!written, "{name}: the transport was written");
                assert_eq!(permit.sent(), Sent::Never, "{name}");
            }
        }

        #[test]
        fn a_transition_waits_for_an_in_flight_first_write_and_finds_it_sent() {
            for (name, transition) in TRANSITIONS {
                let (publication, origin, shutdown) =
                    (Publication::default(), origin(), Shutdown::default());
                let generation = admitted(&publication);
                let permit = permit(&publication, generation, &origin, &shutdown);
                std::thread::scope(|scope| {
                    let (done, finished) = std::sync::mpsc::channel();
                    let mut transitioning = None;
                    let result = Submission::with(&permit).first_write(|| {
                        let (publication, origin, shutdown) = (&publication, &origin, &shutdown);
                        transitioning = Some(scope.spawn(move || {
                            transition(publication, origin, shutdown);
                            done.send(()).unwrap();
                        }));
                        // The write is in flight: the transition must wait.
                        assert!(
                            matches!(
                                finished.recv_timeout(Duration::from_millis(100)),
                                Err(RecvTimeoutError::Timeout)
                            ),
                            "{name} passed an in-flight first write"
                        );
                        Poll::Ready(Ok(1))
                    });
                    assert!(matches!(result, Poll::Ready(Ok(1))), "{name}");
                    finished
                        .recv_timeout(Duration::from_secs(5))
                        .expect("the transition never completed");
                    transitioning.unwrap().join().unwrap();
                });
                // The accepted byte is never reclassified, and nothing new passes.
                assert_eq!(permit.sent(), Sent::Possibly, "{name}");
                let later = self::permit(&publication, generation, &origin, &shutdown);
                let result = Submission::with(&later).first_write(|| Poll::Ready(Ok(1)));
                assert!(
                    matches!(result, Poll::Ready(Err(_))),
                    "{name}: a later first write passed"
                );
            }
        }

        #[test]
        fn revocation_records_one_coalesced_stop_obligation_per_generation() {
            let publication = Publication::default();
            let generation = admitted(&publication);
            // Another principal never matches.
            assert!(publication.revoke_owned("beta", VM).is_empty());
            assert_eq!(publication.revoke_owned("alpha", VM), vec![generation]);
            assert!(publication.owner_cancelled("nonce"));
            // Repeated requests coalesce while it is requested.
            assert!(publication.revoke_owned("alpha", VM).is_empty());
            assert!(publication.revoke(VM).is_empty());
            assert_eq!(publication.stop_obligations(), 1);
            // A failed attempt is requested again by the next revocation.
            publication
                .activations()
                .stops
                .insert(generation, Stop::Failed);
            assert_eq!(publication.revoke_owned("alpha", VM), vec![generation]);
            assert_eq!(publication.stop_obligations(), 1);
            // After the seal no revocation records an obligation and nothing
            // registers.
            publication.activations().stops.clear();
            publication.seal(&Shutdown::default());
            assert!(publication.revoke_owned("alpha", VM).is_empty());
            assert_eq!(publication.stop_obligations(), 0);
            assert!(publication.register(VM, "alpha", "later").is_none());
        }

        #[test]
        fn profile_hostname_and_route_slot_bounds_are_enforced() {
            let host = [9; 16];
            let zone = "b".repeat(32);

            // Empty hostnames list is rejected.
            let empty = Profile::synthetic_multi(host, "site", &[], Some("example.invalid"), &zone);
            assert!(empty.validate_binding().is_err());

            // 8 sorted unique in-zone hostnames succeed.
            let eight = [
                "a.example.invalid",
                "b.example.invalid",
                "c.example.invalid",
                "d.example.invalid",
                "e.example.invalid",
                "f.example.invalid",
                "g.example.invalid",
                "h.example.invalid",
            ];
            let valid8 =
                Profile::synthetic_multi(host, "site", &eight, Some("example.invalid"), &zone);
            assert!(valid8.validate_binding().is_ok());

            // 9 hostnames exceed MAX_HOSTNAMES and are rejected.
            let nine = [
                "a.example.invalid",
                "b.example.invalid",
                "c.example.invalid",
                "d.example.invalid",
                "e.example.invalid",
                "f.example.invalid",
                "g.example.invalid",
                "h.example.invalid",
                "i.example.invalid",
            ];
            let invalid9 =
                Profile::synthetic_multi(host, "site", &nine, Some("example.invalid"), &zone);
            assert!(invalid9.validate_binding().is_err());

            // Unsorted and duplicate lists are rejected.
            let unsorted = Profile::synthetic_multi(
                host,
                "site",
                &["b.example.invalid", "a.example.invalid"],
                Some("example.invalid"),
                &zone,
            );
            assert!(unsorted.validate_binding().is_err());
            let dup = Profile::synthetic_multi(
                host,
                "site",
                &["a.example.invalid", "a.example.invalid"],
                Some("example.invalid"),
                &zone,
            );
            assert!(dup.validate_binding().is_err());

            // Cross-zone hostname is rejected.
            let cross_zone = Profile::synthetic_multi(
                host,
                "site",
                &["a.example.invalid", "b.other.invalid"],
                Some("example.invalid"),
                &zone,
            );
            assert!(cross_zone.validate_binding().is_err());

            // Route bindings must match hostnames 1:1 with unique slots 0..32.
            let with_routes = Profile::synthetic_multi(
                host,
                "site",
                &["api.example.invalid", "app.example.invalid"],
                Some("example.invalid"),
                &zone,
            )
            .with_routes(vec![
                RouteBinding {
                    hostname: Hostname::parse("api.example.invalid").unwrap(),
                    project_id: "proj-a".into(),
                    service: "api".into(),
                    project_slot: 0,
                    service_index: 0,
                    route_slot: 0,
                },
                RouteBinding {
                    hostname: Hostname::parse("app.example.invalid").unwrap(),
                    project_id: "proj-a".into(),
                    service: "web".into(),
                    project_slot: 0,
                    service_index: 1,
                    route_slot: 1,
                },
            ]);
            assert!(with_routes.validate_binding().is_ok());

            // Duplicate route_slot is rejected.
            let slot_collision = Profile::synthetic_multi(
                host,
                "site",
                &["api.example.invalid", "app.example.invalid"],
                Some("example.invalid"),
                &zone,
            )
            .with_routes(vec![
                RouteBinding {
                    hostname: Hostname::parse("api.example.invalid").unwrap(),
                    project_id: "proj-a".into(),
                    service: "api".into(),
                    project_slot: 0,
                    service_index: 0,
                    route_slot: 3,
                },
                RouteBinding {
                    hostname: Hostname::parse("app.example.invalid").unwrap(),
                    project_id: "proj-b".into(),
                    service: "web".into(),
                    project_slot: 1,
                    service_index: 0,
                    route_slot: 3,
                },
            ]);
            assert!(slot_collision.validate_binding().is_err());

            // Out-of-range route_slot (>= 32) is rejected.
            let bad_slot = Profile::synthetic_multi(
                host,
                "site",
                &["api.example.invalid"],
                Some("example.invalid"),
                &zone,
            )
            .with_routes(vec![RouteBinding {
                hostname: Hostname::parse("api.example.invalid").unwrap(),
                project_id: "proj-a".into(),
                service: "api".into(),
                project_slot: 0,
                service_index: 0,
                route_slot: 32,
            }]);
            assert!(bad_slot.validate_binding().is_err());
        }
    }
}

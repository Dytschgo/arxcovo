//! Portable evidence framing: the closed stage graph, the one file contract
//! shared by the writer and the verifier, canonical bounded records, the
//! invalidation dependency projection and the native verifier.
//!
//! Nothing here can mint authority. The verifier is the only reader, and it
//! accepts a bundle only against an independently pinned selection and a
//! freshly measured, complete dependency projection.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::Read;

mod docker;
pub use docker::{
    DockerDependencies, DockerFresh, DockerRecipe, DockerSelection, VerifiedDockerEvidence,
    read_native_docker,
};

pub type Hash = [u8; 32];

pub const CONTRACT: u16 = 4;
pub const COLLECTOR: u16 = 1;
pub const RECORD_LIMIT: usize = 16 * 1024;
/// Seven days, exclusive at the upper boundary.
pub const MAX_VALIDITY: u64 = 7 * 24 * 60 * 60;
pub const MAX_TRIAL_MS: u64 = 7 * 60 * 60 * 1000;
/// UTC has one-second resolution; this also absorbs ordinary clock slew.
pub const CLOCK_SKEW_MS: u64 = 3_000;
/// Live observations used to dispatch an effect, independent of validity.
pub const LIVE_FRESHNESS_MS: u64 = 5_000;

/// Reviewed stock QEMU 11.1.3 distribution pins (ADR 0054).
pub const STOCK_MANIFEST_SHA256: &str =
    "f1cf73c8ecae163e31467a3985fc13749aee1f12a8747756eb136108a1015759";
pub const STOCK_RECEIPT_BYTES: u64 = 7_438;
pub const STOCK_MANIFEST_BYTES: u64 = 11_915;
pub const STOCK_PAYLOAD_FILES: u16 = 136;
/// Enrollment, trial root, trial volume root, outer root and backing file.
pub const HELD_ROOTS: u8 = 5;

pub const GIB: u64 = 1 << 30;
pub const DISK_CEILING: u64 = 200 * GIB;
pub const OUTER_FLOOR: u64 = 50 * GIB;
pub const RAM_RESERVE: u64 = 16 * GIB;
pub const JOB_MEMORY_BYTES: u64 = 2 * GIB;
pub const JOB_CPU_RATE: u32 = 2500;

pub fn digest(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}

pub fn stock_manifest() -> Hash {
    let mut bytes = [0; 32];
    hex::decode_to_slice(STOCK_MANIFEST_SHA256, &mut bytes).expect("fixed manifest pin");
    bytes
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Test,
    StaticOrigin,
    Docker,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Inputs,
    HostBaseline,
    Distribution,
    CustodyJobCapacityBaseline,
    ActivityReservesBaseline,
    WhpxCapability,
    /// Dedicated, private, exclusively owned disposable store disjoint from
    /// production and protected data. Required before any effect.
    DisposableStore,
    /// OS-suspended child: token, Job membership and stdio before resume.
    SuspendedChild,
    /// QEMU `-S` prelaunch pause, diskless and networkless.
    QemuPrelaunchPause,
    /// The protected bundle inventory rechecked after QMP prelaunch.
    HeldBundleInventory,
    /// WHPX initialization and the first guest instruction, still diskless.
    FirstGuestExecution,
    SupervisorLossRelease,
    CapacityTrials,
    ReserveActivityTrials,
    QmpChannel,
    RestrictedTestNetwork,
    UbuntuRecipe,
    WindowsRecipe,
    UbuntuInstalledEndpoint,
    WindowsInstalledEndpoint,
    RestrictedOriginNetwork,
    StaticWebRecipe,
    PublicIpv4Network,
    DockerRecipe,
    /// After the applicable class's installed boot.
    InstalledGuestEndpoint,
    Cleanup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageKind {
    /// Read-only observation; no effect.
    Baseline,
    /// The disposable store proof that every effect consumes.
    Store,
    /// A native effect: it needs the store proof, satisfied prerequisites and
    /// a fresh live sample at dispatch.
    Effect,
    Cleanup,
}

// Every class shares the first fifteen stages; each class's installed
// endpoint check follows its own network probe and recipe boots.
const TEST: [Stage; 21] = [
    Stage::Inputs,
    Stage::HostBaseline,
    Stage::Distribution,
    Stage::CustodyJobCapacityBaseline,
    Stage::ActivityReservesBaseline,
    Stage::WhpxCapability,
    Stage::DisposableStore,
    Stage::SuspendedChild,
    Stage::QemuPrelaunchPause,
    Stage::HeldBundleInventory,
    Stage::FirstGuestExecution,
    Stage::SupervisorLossRelease,
    Stage::CapacityTrials,
    Stage::ReserveActivityTrials,
    Stage::QmpChannel,
    Stage::RestrictedTestNetwork,
    Stage::UbuntuRecipe,
    Stage::UbuntuInstalledEndpoint,
    Stage::WindowsRecipe,
    Stage::WindowsInstalledEndpoint,
    Stage::Cleanup,
];
const STATIC_ORIGIN: [Stage; 19] = [
    Stage::Inputs,
    Stage::HostBaseline,
    Stage::Distribution,
    Stage::CustodyJobCapacityBaseline,
    Stage::ActivityReservesBaseline,
    Stage::WhpxCapability,
    Stage::DisposableStore,
    Stage::SuspendedChild,
    Stage::QemuPrelaunchPause,
    Stage::HeldBundleInventory,
    Stage::FirstGuestExecution,
    Stage::SupervisorLossRelease,
    Stage::CapacityTrials,
    Stage::ReserveActivityTrials,
    Stage::QmpChannel,
    Stage::RestrictedOriginNetwork,
    Stage::StaticWebRecipe,
    Stage::InstalledGuestEndpoint,
    Stage::Cleanup,
];
const DOCKER: [Stage; 19] = [
    Stage::Inputs,
    Stage::HostBaseline,
    Stage::Distribution,
    Stage::CustodyJobCapacityBaseline,
    Stage::ActivityReservesBaseline,
    Stage::WhpxCapability,
    Stage::DisposableStore,
    Stage::SuspendedChild,
    Stage::QemuPrelaunchPause,
    Stage::HeldBundleInventory,
    Stage::FirstGuestExecution,
    Stage::SupervisorLossRelease,
    Stage::CapacityTrials,
    Stage::ReserveActivityTrials,
    Stage::QmpChannel,
    Stage::PublicIpv4Network,
    Stage::DockerRecipe,
    Stage::InstalledGuestEndpoint,
    Stage::Cleanup,
];

/// The one closed order for each network class.
pub fn stages(class: Class) -> &'static [Stage] {
    match class {
        Class::Test => &TEST,
        Class::StaticOrigin => &STATIC_ORIGIN,
        Class::Docker => &DOCKER,
    }
}

pub fn position(class: Class, stage: Stage) -> Option<usize> {
    stages(class).iter().position(|s| *s == stage)
}

impl Stage {
    pub fn kind(self) -> StageKind {
        match self {
            Self::Inputs
            | Self::HostBaseline
            | Self::Distribution
            | Self::CustodyJobCapacityBaseline
            | Self::ActivityReservesBaseline
            | Self::WhpxCapability => StageKind::Baseline,
            Self::DisposableStore => StageKind::Store,
            Self::Cleanup => StageKind::Cleanup,
            _ => StageKind::Effect,
        }
    }

    /// Explicit confinement bounds for host and network effect stages.
    pub fn confinement(self) -> Option<EffectPlan> {
        match self {
            Self::SuspendedChild
            | Self::QemuPrelaunchPause
            | Self::HeldBundleInventory
            | Self::CapacityTrials
            | Self::ReserveActivityTrials
            | Self::QmpChannel => Some(EffectPlan::CONFINED),
            Self::FirstGuestExecution | Self::SupervisorLossRelease => Some(EffectPlan {
                paused: false,
                ..EffectPlan::CONFINED
            }),
            Self::RestrictedTestNetwork => Some(EffectPlan {
                paused: false,
                disks: 0,
                network: true,
            }),
            Self::UbuntuRecipe | Self::WindowsRecipe => Some(EffectPlan {
                paused: false,
                disks: 2,
                network: true,
            }),
            _ => None,
        }
    }

    pub fn file(self) -> FileKind {
        match self {
            Self::Inputs => FileKind::Inputs,
            Self::HostBaseline => FileKind::HostBaseline,
            Self::Distribution => FileKind::Distribution,
            Self::CustodyJobCapacityBaseline
            | Self::DisposableStore
            | Self::SuspendedChild
            | Self::HeldBundleInventory
            | Self::SupervisorLossRelease
            | Self::CapacityTrials => FileKind::CustodyJobCapacity,
            Self::ActivityReservesBaseline | Self::ReserveActivityTrials => {
                FileKind::ActivityReserves
            }
            Self::WhpxCapability
            | Self::QemuPrelaunchPause
            | Self::FirstGuestExecution
            | Self::QmpChannel
            | Self::UbuntuInstalledEndpoint
            | Self::WindowsInstalledEndpoint
            | Self::InstalledGuestEndpoint => FileKind::WhpxQmpEndpoint,
            Self::RestrictedTestNetwork => FileKind::NetworkRestrictedTest,
            Self::UbuntuRecipe => FileKind::RecipeUbuntu,
            Self::WindowsRecipe => FileKind::RecipeWindows,
            Self::RestrictedOriginNetwork => FileKind::NetworkRestrictedOrigin,
            Self::StaticWebRecipe => FileKind::RecipeStaticWeb,
            Self::PublicIpv4Network => FileKind::NetworkPublicIpv4,
            Self::DockerRecipe => FileKind::RecipeDocker,
            Self::Cleanup => FileKind::Cleanup,
        }
    }
}

/// What an effect stage is about to dispatch. Checked before any effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectPlan {
    pub paused: bool,
    pub disks: u8,
    pub network: bool,
}

impl EffectPlan {
    pub const CONFINED: Self = Self {
        paused: true,
        disks: 0,
        network: false,
    };

    pub fn within(self, limit: Self) -> bool {
        (self.paused || !limit.paused)
            && self.disks <= limit.disks
            && (!self.network || limit.network)
    }
}

/// The fixed record files, in manifest order. The writer and the verifier
/// both derive file names and membership from this single contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FileKind {
    #[serde(rename = "inputs.json")]
    Inputs,
    #[serde(rename = "host-baseline.json")]
    HostBaseline,
    #[serde(rename = "distribution.json")]
    Distribution,
    #[serde(rename = "custody-job-capacity.json")]
    CustodyJobCapacity,
    #[serde(rename = "activity-reserves.json")]
    ActivityReserves,
    #[serde(rename = "whpx-qmp-endpoint.json")]
    WhpxQmpEndpoint,
    #[serde(rename = "network-restricted-test.json")]
    NetworkRestrictedTest,
    #[serde(rename = "recipe-ubuntu.json")]
    RecipeUbuntu,
    #[serde(rename = "recipe-windows.json")]
    RecipeWindows,
    #[serde(rename = "network-restricted-origin.json")]
    NetworkRestrictedOrigin,
    #[serde(rename = "recipe-static-web.json")]
    RecipeStaticWeb,
    #[serde(rename = "network-public-ipv4.json")]
    NetworkPublicIpv4,
    #[serde(rename = "recipe-docker.json")]
    RecipeDocker,
    #[serde(rename = "cleanup.json")]
    Cleanup,
}

impl FileKind {
    pub const ALL: [Self; 14] = [
        Self::Inputs,
        Self::HostBaseline,
        Self::Distribution,
        Self::CustodyJobCapacity,
        Self::ActivityReserves,
        Self::WhpxQmpEndpoint,
        Self::NetworkRestrictedTest,
        Self::RecipeUbuntu,
        Self::RecipeWindows,
        Self::NetworkRestrictedOrigin,
        Self::RecipeStaticWeb,
        Self::NetworkPublicIpv4,
        Self::RecipeDocker,
        Self::Cleanup,
    ];

    pub fn required(self, class: Class) -> bool {
        stages(class).iter().any(|stage| stage.file() == self)
    }

    /// The serialized name is the file name.
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .expect("unit variants serialize as their file name")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Native,
    Fixture,
}

/// Typed, path-free diagnostic codes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Diagnostic {
    OwnerActive,
    ReservesShort,
    WhpxAbsent,
    ObservationInvalid,
    Expired,
    ClockInconsistent,
    Cancelled,
    StaleLiveSample,
    PrerequisiteMissing,
    EffectUnconfined,
    TeardownUncertain,
    /// A preflight observation refused after the claim, settled as a typed
    /// diagnostic instead of ending the run.
    UnsupportedPlatform,
    DistributionUnverified,
    PathCustodyUnverified,
    JobUnverified,
    WhpxUnverified,
    DiskCeilingUnverified,
    OwnerActivityUnverified,
    ReservesUnverified,
    OtherRefusal,
}

/// Where the paused launcher evaluated a reserve sample.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReserveStep {
    Admission,
    PostClaim,
    PreCreate,
    PostCreate,
    PreResume,
    PostResume,
    Monitoring,
    Dispatch,
}

/// Closed diagnostic payload with only the measurements used by the failed check.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReserveCheck {
    ActiveCpusUnavailable,
    MemoryStatusUnavailable,
    JobMembershipUnavailable,
    QualifierInJob,
    ProcessorGroupsUnavailable,
    ProcessorGroups {
        measured_count: u16,
        required_count: u16,
    },
    AffinityUnavailable,
    EffectiveCpus {
        measured_count: u32,
        minimum_count: u32,
        active_count: u32,
    },
    CpuRate {
        active_count: u32,
        effective_count: u32,
        requested_basis_points: u64,
        allowed_basis_points: u64,
    },
    AvailableRam {
        available_bytes: u64,
        required_bytes: u64,
    },
    SampleStale {
        age_ms: u64,
        limit_ms: u64,
    },
    SampleFuture {
        ahead_ms: u64,
    },
    EnvelopeOverflow {
        owner_bytes: u64,
        job_bytes: u64,
        outside_bytes: u64,
    },
    DispatchArmRefused,
    DispatchWindowExpired {
        overdue_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveFailure {
    pub step: ReserveStep,
    pub failure: ReserveCheck,
}

/// Fixed QMP operations shared by the paused evidence collector and its reader.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PausedQmpStep {
    Greeting,
    Capabilities,
    QueryStatus,
    QueryAccelerators,
    ReleaseStartup,
    Quit,
    Powerdown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PausedDispatchState {
    Idle,
    Armed,
    Writing,
    WrittenInTime,
    Expired,
    Ambiguous,
}

/// State of the already-held child before requesting teardown.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PausedChildState {
    Absent,
    Live,
    Exited { status: Option<u32> },
    Unavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PausedSnapshot {
    pub child: PausedChildState,
    pub dispatch: Option<PausedDispatchState>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PausedFailure {
    pub failure: PausedCheck,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<PausedSnapshot>,
}

/// Non-reserve subchecks that previously collapsed to an opaque refusal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "check", rename_all = "snake_case", deny_unknown_fields)]
pub enum PausedCheck {
    PrepareStdio,
    StdioMissing,
    StartStdioWorkers,
    QmpChannel {
        step: PausedQmpStep,
        error: super::qmp::ChannelError,
    },
    QmpGreetingVersion {
        measured: [u16; 3],
        required: [u16; 3],
    },
    QmpGreetingPackageBytes {
        measured_bytes: u32,
        maximum_bytes: u32,
    },
    QmpGreetingCapabilityCount {
        measured_count: u32,
        maximum_count: u32,
    },
    QmpGreetingCapabilityBytes {
        measured_bytes: u32,
        minimum_bytes: u32,
        maximum_bytes: u32,
    },
    QmpStatus {
        measured: super::qmp::RunStatus,
        required: super::qmp::RunStatus,
    },
    DispatchUnconfirmed {
        step: PausedQmpStep,
        state: PausedDispatchState,
        wait_limit_ms: u64,
        wait_finished: bool,
    },
    DispatchInterrupted {
        step: PausedQmpStep,
        error: super::qmp::ChannelError,
    },
    DispatchNotWritable {
        step: PausedQmpStep,
        state: PausedDispatchState,
    },
    WhpxChannel {
        error: super::qmp::ChannelError,
    },
    WhpxNotEnabled,
    WhpxAcceleratorCount {
        measured_count: u32,
        maximum_count: u32,
    },
    WhpxPresenceCount {
        measured_count: u32,
        required_count: u32,
    },
    WhpxUnknownAccelerator,
    Disabled,
    QualificationMissing,
    IdentityUnverified,
    QmpUnverified,
    GuestEndpointUnverified,
    NetworkFenceUnverified,
    InstallerPhaseUnavailable,
    InstallerPhaseMismatch,
    InstallerDeadlineExpired,
}

/// Controlled mutations apply only inside the owner-local trial. The
/// record describes the fresh sample before the fault and its settlement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlledFault {
    WtsAmbiguity,
    CpuReserve,
    RamReserve,
    DiskReserve,
}

impl ControlledFault {
    pub const ALL: [Self; 4] = [
        Self::WtsAmbiguity,
        Self::CpuReserve,
        Self::RamReserve,
        Self::DiskReserve,
    ];
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FaultRecord {
    pub fault: ControlledFault,
    pub sampled_ms: u64,
    pub settled_ms: u64,
    pub console_idle: bool,
    pub effective_cpus: u32,
    pub available_ram_bytes: u64,
    pub inner_free_bytes: u64,
    pub outer_free_bytes: u64,
}

/// Catalog identity and exact asset/recipe digest qualified by the run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeIdentity {
    pub recipe_id: [u8; 16],
    pub recipe_sha256: Hash,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TestRecipes {
    pub ubuntu: RecipeIdentity,
    pub windows: RecipeIdentity,
}

impl TestRecipes {
    pub fn key(self) -> Hash {
        digest(&serde_json::to_vec(&("arx-vm-test-recipes-v1", self)).expect("fixed recipe pair"))
    }

    fn valid(self) -> bool {
        self.ubuntu.recipe_id == self.ubuntu.recipe_sha256[..16]
            && self.ubuntu.recipe_id != self.windows.recipe_id
            && self.windows.recipe_id == super::template::windows_recipe_id()
            && self.ubuntu.recipe_sha256 != [0; 32]
            && self.windows.recipe_sha256 != [0; 32]
    }
}

/// Passed predicates for baseline and paused-trial stages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observation {
    InputPins {
        held_roots: u8,
        enrollment_private: bool,
        evidence_private: bool,
        trial_root_private: bool,
        evidence_claimed: bool,
    },
    Host {
        windows_build: u32,
        windows_revision: u32,
    },
    Distribution {
        receipt: Hash,
        receipt_bytes: u64,
        manifest: Hash,
        manifest_bytes: u64,
        payload_files: u16,
        payload_bytes: u64,
    },
    Capacity {
        inner_caller_free_bytes: u64,
        inner_total_free_bytes: u64,
        inner_total_bytes: u64,
        inner_extent_bytes: u64,
        vhd_virtual_bytes: u64,
        outer_caller_free_bytes: u64,
        outer_total_free_bytes: u64,
        backing_allocated_bytes: u64,
        backing_length_bytes: u64,
        disk_ceiling_bytes: u64,
        outer_floor_bytes: u64,
        disk_budget_bytes: u64,
        empty_job_memory_limit_bytes: u64,
        empty_job_cpu_rate: u32,
    },
    Activity {
        console_idle: bool,
        active_cpus: u32,
        available_ram_bytes: u64,
        owner_ram_reserve_bytes: u64,
        owner_cpu_reserve: u32,
    },
    Whpx {
        present: bool,
    },
    PausedStore {
        validated: bool,
    },
    FirmwareSec {
        sec_marker_verified: bool,
        prelaunch_quiet: bool,
        job_memory_limit_bytes: u64,
        job_cpu_rate: u32,
        clean_release: bool,
    },
    QmpFirmware {
        greeting_verified: bool,
        whpx_enabled: bool,
        quit_clean_eof: bool,
        clean_release: bool,
    },
    RestrictedNetworkLive {
        job_memory_limit_bytes: u64,
        job_cpu_rate: u32,
        host_controls_pre: bool,
        host_controls_post: bool,
        nic_setup_verified: bool,
        denied_probes: u8,
        zero_hostfwd: bool,
        clean_guest_shutdown: bool,
        seed_released: bool,
    },
    RestrictedNetworkWitness {
        ipv4_and_ipv6_setup: bool,
        reachable_gateway_control: bool,
        validated_attempts: u8,
        witness_receipts: u8,
        witnesses_settled: bool,
        clean_guest_shutdown: bool,
        seed_charge_released: bool,
    },
    SupervisorLoss {
        job_memory_limit_bytes: u64,
        job_cpu_rate: u32,
        active_process_limit: u32,
        child_exited: bool,
        job_empty_notified: bool,
        dirty_release: bool,
    },
    CapacityExercised {
        trial_allocated_bytes: u64,
        recipe_allocated_bytes: u64,
        denial_checks: u8,
        overlap_excluded: bool,
        aggregate_cap_checked: bool,
        trial_released: bool,
        recipe_released: bool,
    },
    ReserveActivity {
        controlled_faults: Box<[FaultRecord; 4]>,
        console_transitions_ms: [u64; 5],
    },
    UbuntuRecipeVerified {
        recipe: RecipeIdentity,
        offline_installer: bool,
        pinned_seed: bool,
        detached_health: bool,
        pair_unchanged: bool,
        installed_isolation: bool,
    },
    WindowsRecipeVerified {
        recipe: RecipeIdentity,
        offline_setup_and_boot_next: bool,
        signed_drivers: bool,
        installed_readiness: bool,
        disk_only_finalize: bool,
        registry_generalized: bool,
        state_ini_generalized: bool,
        media_free_health: bool,
        distinct_clone_identities: bool,
        pair_unchanged: bool,
        installed_isolation: bool,
    },
    TestInstalledEndpoint {
        recipe: RecipeIdentity,
        boot_identity: [u8; 16],
        live_job_and_same_peer: bool,
        nonce_bound_script: bool,
        script_contained: bool,
        clean_stop: bool,
    },
    DockerDistribution {
        manifest: Hash,
        executable: Hash,
        receipt: Hash,
        loaded_artifacts: Hash,
    },
    DockerRecipeVerified {
        recipe: DockerRecipe,
        restricted_install: bool,
        detached_health: bool,
        independent_pair: bool,
    },
    DockerPublicIpv4 {
        policy: Hash,
        topology: Hash,
        /// Run-scoped digest of the private receiver and control observations.
        receiver_witness: Hash,
        receiver_controls: bool,
        public_tcp_udp: bool,
        nonpublic_denied: bool,
        ipv6_icmp_denied: bool,
        route_drift_denied: bool,
    },
    DockerInstalledEndpoint {
        recipe: RecipeIdentity,
        compose_protocol: u32,
        exact_pipe_peer: bool,
        pinned_origin_tls: bool,
        clean_stop: bool,
    },
    PausedTrial {
        verified: bool,
    },
    Cleanup {
        dispatched_effects: u16,
        released: bool,
        charge_retained: bool,
        /// The run completed after cancellation, the trial budget, expiry
        /// or a clock inconsistency. Teardown still ran; nothing may seal.
        late: bool,
    },
    /// Fixture wire evidence. This variant never validates under native origin.
    FixtureEffect {
        stage: Stage,
    },
}

impl Observation {
    pub fn validates(&self, stage: Stage, origin: Origin) -> bool {
        use Observation as O;
        match (stage, self) {
            (
                Stage::Distribution,
                O::DockerDistribution {
                    manifest,
                    executable,
                    receipt,
                    loaded_artifacts,
                },
            ) => {
                origin == Origin::Native
                    && [manifest, executable, receipt, loaded_artifacts]
                        .iter()
                        .all(|hash| **hash != [0; 32])
            }
            (
                Stage::DockerRecipe,
                O::DockerRecipeVerified {
                    recipe,
                    restricted_install,
                    detached_health,
                    independent_pair,
                },
            ) => {
                origin == Origin::Native
                    && recipe.valid()
                    && *restricted_install
                    && *detached_health
                    && *independent_pair
            }
            (
                Stage::PublicIpv4Network,
                O::DockerPublicIpv4 {
                    policy,
                    topology,
                    receiver_witness,
                    receiver_controls,
                    public_tcp_udp,
                    nonpublic_denied,
                    ipv6_icmp_denied,
                    route_drift_denied,
                },
            ) => {
                origin == Origin::Native
                    && *policy != [0; 32]
                    && *topology != [0; 32]
                    && *receiver_witness != [0; 32]
                    && *receiver_controls
                    && *public_tcp_udp
                    && *nonpublic_denied
                    && *ipv6_icmp_denied
                    && *route_drift_denied
            }
            (
                Stage::InstalledGuestEndpoint,
                O::DockerInstalledEndpoint {
                    recipe,
                    compose_protocol,
                    exact_pipe_peer,
                    pinned_origin_tls,
                    clean_stop,
                },
            ) => {
                origin == Origin::Native
                    && docker::recipe_id_valid(*recipe)
                    && *compose_protocol == arx_guest::compose::COMPOSE_PROTOCOL_VERSION
                    && *exact_pipe_peer
                    && *pinned_origin_tls
                    && *clean_stop
            }
            (
                Stage::Inputs,
                O::InputPins {
                    held_roots,
                    enrollment_private,
                    evidence_private,
                    trial_root_private,
                    evidence_claimed,
                },
            ) => {
                *held_roots == HELD_ROOTS
                    && *enrollment_private
                    && *evidence_private
                    && *trial_root_private
                    && *evidence_claimed
            }
            (Stage::HostBaseline, O::Host { windows_build, .. }) => *windows_build >= 22_000,
            (
                Stage::Distribution,
                O::Distribution {
                    receipt,
                    receipt_bytes,
                    manifest,
                    manifest_bytes,
                    payload_files,
                    payload_bytes,
                },
            ) => {
                *receipt == crate::vm::public_egress::STOCK_RECEIPT_1113_SHA256
                    && *receipt_bytes == STOCK_RECEIPT_BYTES
                    && *manifest == stock_manifest()
                    && *manifest_bytes == STOCK_MANIFEST_BYTES
                    && *payload_files == STOCK_PAYLOAD_FILES
                    && *payload_bytes > 0
            }
            (
                Stage::CustodyJobCapacityBaseline,
                O::Capacity {
                    inner_caller_free_bytes,
                    inner_total_free_bytes,
                    inner_total_bytes,
                    inner_extent_bytes,
                    vhd_virtual_bytes,
                    outer_caller_free_bytes,
                    outer_total_free_bytes,
                    backing_allocated_bytes,
                    backing_length_bytes,
                    disk_ceiling_bytes,
                    outer_floor_bytes,
                    disk_budget_bytes,
                    empty_job_memory_limit_bytes,
                    empty_job_cpu_rate,
                },
            ) => {
                *inner_total_bytes > 0
                    && inner_caller_free_bytes <= inner_total_free_bytes
                    && inner_total_free_bytes <= inner_total_bytes
                    && inner_total_bytes <= inner_extent_bytes
                    && inner_extent_bytes <= vhd_virtual_bytes
                    && *disk_ceiling_bytes == DISK_CEILING
                    && vhd_virtual_bytes <= disk_ceiling_bytes
                    && *outer_floor_bytes == OUTER_FLOOR
                    && outer_caller_free_bytes <= outer_total_free_bytes
                    && outer_caller_free_bytes >= outer_floor_bytes
                    && backing_length_bytes >= vhd_virtual_bytes
                    && backing_allocated_bytes >= backing_length_bytes
                    && *disk_budget_bytes == (*disk_ceiling_bytes).min(*inner_extent_bytes)
                    && *empty_job_memory_limit_bytes == JOB_MEMORY_BYTES
                    && *empty_job_cpu_rate == JOB_CPU_RATE
            }
            (
                Stage::ActivityReservesBaseline,
                O::Activity {
                    console_idle,
                    active_cpus,
                    available_ram_bytes,
                    owner_ram_reserve_bytes,
                    owner_cpu_reserve,
                },
            ) => {
                *console_idle
                    && *active_cpus > 1
                    && *owner_ram_reserve_bytes == RAM_RESERVE
                    && available_ram_bytes > owner_ram_reserve_bytes
                    && *owner_cpu_reserve == active_cpus.div_ceil(2)
            }
            (
                Stage::FirstGuestExecution,
                O::FirmwareSec {
                    sec_marker_verified,
                    prelaunch_quiet,
                    job_memory_limit_bytes,
                    job_cpu_rate,
                    clean_release,
                },
            ) => {
                origin == Origin::Native
                    && *sec_marker_verified
                    && *prelaunch_quiet
                    && *job_memory_limit_bytes == JOB_MEMORY_BYTES
                    && *job_cpu_rate == JOB_CPU_RATE
                    && *clean_release
            }
            (
                Stage::QmpChannel,
                O::QmpFirmware {
                    greeting_verified,
                    whpx_enabled,
                    quit_clean_eof,
                    clean_release,
                },
            ) => {
                origin == Origin::Native
                    && *greeting_verified
                    && *whpx_enabled
                    && *quit_clean_eof
                    && *clean_release
            }
            // Older diagnostics can be decoded, but their socket-error counts
            // carry no evidence that the restricted backend denied a packet.
            (Stage::RestrictedTestNetwork, O::RestrictedNetworkLive { .. }) => false,
            (
                Stage::RestrictedTestNetwork,
                O::RestrictedNetworkWitness {
                    ipv4_and_ipv6_setup,
                    reachable_gateway_control,
                    validated_attempts,
                    witness_receipts,
                    witnesses_settled,
                    clean_guest_shutdown,
                    seed_charge_released,
                },
            ) => {
                origin == Origin::Native
                    && *ipv4_and_ipv6_setup
                    && *reachable_gateway_control
                    && *validated_attempts == 8
                    && *witness_receipts == 0
                    && *witnesses_settled
                    && *clean_guest_shutdown
                    && *seed_charge_released
            }
            (
                Stage::SupervisorLossRelease,
                O::SupervisorLoss {
                    job_memory_limit_bytes,
                    job_cpu_rate,
                    active_process_limit,
                    child_exited,
                    job_empty_notified,
                    dirty_release,
                },
            ) => {
                origin == Origin::Native
                    && *job_memory_limit_bytes == JOB_MEMORY_BYTES
                    && *job_cpu_rate == JOB_CPU_RATE
                    && *active_process_limit == 1
                    && *child_exited
                    && *job_empty_notified
                    && *dirty_release
            }
            (
                Stage::CapacityTrials,
                O::CapacityExercised {
                    trial_allocated_bytes,
                    recipe_allocated_bytes,
                    denial_checks,
                    overlap_excluded,
                    aggregate_cap_checked,
                    trial_released,
                    recipe_released,
                },
            ) => {
                origin == Origin::Native
                    && *trial_allocated_bytes >= (1 << 20)
                    && *recipe_allocated_bytes >= (1 << 20)
                    && *denial_checks == 5
                    && *overlap_excluded
                    && *aggregate_cap_checked
                    && *trial_released
                    && *recipe_released
            }
            (
                Stage::ReserveActivityTrials,
                O::ReserveActivity {
                    controlled_faults,
                    console_transitions_ms,
                },
            ) => {
                origin == Origin::Native
                    && controlled_faults
                        .iter()
                        .zip(ControlledFault::ALL)
                        .all(|(row, expected)| {
                            row.fault == expected
                                && row.console_idle
                                && row.effective_cpus >= 2
                                && row.available_ram_bytes > RAM_RESERVE
                                && row.outer_free_bytes >= OUTER_FLOOR
                                && row.inner_free_bytes > 0
                                && row.sampled_ms < row.settled_ms
                        })
                    && controlled_faults
                        .windows(2)
                        .all(|pair| pair[0].settled_ms <= pair[1].sampled_ms)
                    && controlled_faults[3].settled_ms <= console_transitions_ms[0]
                    && console_transitions_ms
                        .windows(2)
                        .all(|pair| pair[0] < pair[1])
                    && console_transitions_ms[4] <= MAX_TRIAL_MS
            }
            (Stage::WhpxCapability, O::Whpx { present }) => *present,
            (
                Stage::UbuntuRecipe,
                O::UbuntuRecipeVerified {
                    recipe,
                    offline_installer,
                    pinned_seed,
                    detached_health,
                    pair_unchanged,
                    installed_isolation,
                },
            ) => {
                origin == Origin::Native
                    && recipe.recipe_id == recipe.recipe_sha256[..16]
                    && recipe.recipe_sha256 != [0; 32]
                    && *offline_installer
                    && *pinned_seed
                    && *detached_health
                    && *pair_unchanged
                    && *installed_isolation
            }
            (
                Stage::WindowsRecipe,
                O::WindowsRecipeVerified {
                    recipe,
                    offline_setup_and_boot_next,
                    signed_drivers,
                    installed_readiness,
                    disk_only_finalize,
                    registry_generalized,
                    state_ini_generalized,
                    media_free_health,
                    distinct_clone_identities,
                    pair_unchanged,
                    installed_isolation,
                },
            ) => {
                origin == Origin::Native
                    && recipe.recipe_id == super::template::windows_recipe_id()
                    && recipe.recipe_sha256 != [0; 32]
                    && *offline_setup_and_boot_next
                    && *signed_drivers
                    && *installed_readiness
                    && *disk_only_finalize
                    && *registry_generalized
                    && *state_ini_generalized
                    && *media_free_health
                    && *distinct_clone_identities
                    && *pair_unchanged
                    && *installed_isolation
            }
            (
                Stage::UbuntuInstalledEndpoint | Stage::WindowsInstalledEndpoint,
                O::TestInstalledEndpoint {
                    recipe,
                    boot_identity,
                    live_job_and_same_peer,
                    nonce_bound_script,
                    script_contained,
                    clean_stop,
                },
            ) => {
                origin == Origin::Native
                    && recipe.recipe_id != [0; 16]
                    && recipe.recipe_sha256 != [0; 32]
                    && *boot_identity != [0; 16]
                    && *live_job_and_same_peer
                    && *nonce_bound_script
                    && *script_contained
                    && *clean_stop
            }
            (Stage::DisposableStore, O::PausedStore { validated }) => {
                origin == Origin::Native && *validated
            }
            (
                Stage::SuspendedChild | Stage::QemuPrelaunchPause | Stage::HeldBundleInventory,
                O::PausedTrial { verified },
            ) => origin == Origin::Native && *verified,
            (
                Stage::Cleanup,
                O::Cleanup {
                    released,
                    charge_retained,
                    late,
                    ..
                },
            ) => *released && !*charge_retained && !*late,
            (_, O::FixtureEffect { stage: witnessed }) => {
                origin == Origin::Fixture
                    && *witnessed == stage
                    && matches!(stage.kind(), StageKind::Store | StageKind::Effect)
            }
            _ => false,
        }
    }
}

/// Socket observations from the held live guest channel. A socket error can
/// also result from guest setup or routing, so it carries no denial claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcome {
    Reachable,
    MalformedReply,
    SocketError,
    NotAttempted,
    DatagramSent,
    Ipv6PathAbsent,
    TimedOut,
    RefusedOrReset,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkProbeReport {
    pub nic_ready: bool,
    pub ipv4_lease: bool,
    pub default_route: bool,
    pub no_ipv6_egress_route: bool,
    /// IPv4, IPv6, host, slirp host alias, management, public, DNS and slirp
    /// DNS alias, in the order the pinned guest script runs them.
    pub outcomes: [ProbeOutcome; 8],
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestrictedNetworkDiagnostic {
    pub guest: NetworkProbeReport,
    pub host_controls_post: bool,
    pub clean_guest_shutdown: bool,
    pub seed_charge_released: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StageResult {
    Passed(Observation),
    Failed(Diagnostic),
    Inconclusive(Diagnostic),
    /// A live TEST probe report without independent packet attribution.
    /// The graph treats this as non-passing.
    NetworkDiagnostic(RestrictedNetworkDiagnostic),
    /// Additive, versioned incomplete evidence. Contract-4 passing rows and
    /// the legacy generic reserve refusal retain their canonical encodings.
    ReserveRefusalV1(ReserveFailure),
    /// Additive incomplete evidence for paused protocol and native checks.
    PausedRefusalV1(PausedFailure),
    /// The native collector for this stage is not implemented in this
    /// contract version. Never completes a dossier.
    Unimplemented,
}

impl StageResult {
    pub fn passes(&self, stage: Stage, origin: Origin) -> bool {
        matches!(self, Self::Passed(observation) if observation.validates(stage, origin))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub class: Class,
    /// Opaque random scope generated by the runner, never caller supplied.
    pub scope: Hash,
    /// Derived from the reviewed contract and distribution pins.
    pub release: Hash,
    pub run: [u8; 16],
    /// Keyed digest of the complete dependency projection.
    pub dependencies: Hash,
    pub boot: BootAnchor,
    /// Elapsed time from the qualifier's stage clock origin.
    pub issued_mono_ms: u64,
    pub issued_utc: u64,
    pub expires_utc: u64,
}

impl Binding {
    pub fn issued_on_boot(self, marker: Hash) -> Option<Instant2> {
        if marker == [0; 32] || self.boot.marker != marker || self.issued_mono_ms > MAX_TRIAL_MS {
            return None;
        }
        Some(Instant2 {
            utc: self.issued_utc,
            mono_ms: self.boot.origin_mono_ms.checked_add(self.issued_mono_ms)?,
        })
    }

    pub fn live_at(self, now_utc: u64, dependencies: Hash) -> bool {
        self.scope != [0; 32]
            && self.release == release()
            && self.run != [0; 16]
            && self.dependencies == dependencies
            && self.issued_utc > 0
            && self.issued_utc <= now_utc
            && now_utc < self.expires_utc
            && self.expires_utc - self.issued_utc <= MAX_VALIDITY
    }
}

/// The boot-scoped marker and the qualifier stage clock's origin on that boot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BootAnchor {
    pub marker: Hash,
    pub origin_mono_ms: u64,
}

impl BootAnchor {
    pub fn on_boot(self, instant: Instant2) -> Option<Instant2> {
        Some(Instant2 {
            utc: instant.utc,
            mono_ms: self.origin_mono_ms.checked_add(instant.mono_ms)?,
        })
    }
}

/// The release names the evidence contract: the contract and collector
/// versions, this package version and the stock distribution pins. Two
/// builds sharing those values share a release, so the exact reviewed
/// artifact is a separate `artifact` dependency, which stays unmeasured
/// until 7c binds the owner-selected reviewed digest.
pub fn release() -> Hash {
    digest(
        &serde_json::to_vec(&(
            "arx-vm-qualify",
            CONTRACT,
            COLLECTOR,
            env!("CARGO_PKG_VERSION"),
            crate::vm::public_egress::STOCK_RECEIPT_1113_SHA256,
            stock_manifest(),
        ))
        .expect("fixed tuple"),
    )
}

/// One monotonic and one UTC observation taken together.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Instant2 {
    pub utc: u64,
    pub mono_ms: u64,
}

/// Elapsed UTC must track elapsed monotonic time, so a rollback or jump
/// inside an otherwise valid interval closes the run.
pub fn clocks_agree(start: Instant2, now: Instant2) -> bool {
    let (Some(utc), Some(mono)) = (
        now.utc
            .checked_sub(start.utc)
            .and_then(|s| s.checked_mul(1000)),
        now.mono_ms.checked_sub(start.mono_ms),
    ) else {
        return false;
    };
    utc.abs_diff(mono) <= CLOCK_SKEW_MS
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Measured<T> {
    Measured(T),
    /// No reviewed collector measures this dependency yet.
    Unmeasured,
}

impl<T> Measured<T> {
    fn is_measured(&self) -> bool {
        matches!(self, Self::Measured(_))
    }
}

/// The OS build and revision and the installation's stable machine
/// identity. Raw values stay local, inside the scope-keyed digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostDependency {
    pub windows_build: u32,
    pub windows_revision: u32,
    pub machine: [u8; 16],
}

/// Raw identities stay local: the projection is only ever published as a
/// digest keyed by the random scope. Volumes are bound by serial, root
/// object and mount-manager GUID, so a reformatted or swapped volume with a
/// colliding serial still drifts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDependency {
    pub trial_volume: u64,
    pub trial_volume_root: [u8; 16],
    pub trial_volume_guid: [u8; 16],
    pub trial_root: [u8; 16],
    pub outer_volume: u64,
    pub outer_volume_root: [u8; 16],
    pub outer_volume_guid: [u8; 16],
    pub backing_file: [u8; 16],
    pub vhd_virtual_bytes: u64,
    pub inner_extent_bytes: u64,
    pub test_scope: Option<Hash>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DistributionDependency {
    pub receipt: Hash,
    pub manifest: Hash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PolicyDependency {
    pub disk_ceiling_bytes: u64,
    pub outer_floor_bytes: u64,
    pub ram_reserve_bytes: u64,
    pub cpu_reserve_half: bool,
    pub job_memory_bytes: u64,
    pub job_cpu_rate: u32,
    pub validity_secs: u64,
    pub live_freshness_ms: u64,
}

impl PolicyDependency {
    pub const CURRENT: Self = Self {
        disk_ceiling_bytes: DISK_CEILING,
        outer_floor_bytes: OUTER_FLOOR,
        ram_reserve_bytes: RAM_RESERVE,
        cpu_reserve_half: true,
        job_memory_bytes: JOB_MEMORY_BYTES,
        job_cpu_rate: JOB_CPU_RATE,
        validity_secs: MAX_VALIDITY,
        live_freshness_ms: LIVE_FRESHNESS_MS,
    };
}

/// Every stable input a qualification depends on. Drift in any field changes
/// the key. Unmeasured fields keep the projection incomplete, and an
/// incomplete projection can never seal or verify.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Dependencies {
    pub release: Hash,
    /// The exact reviewed artifact digest. No reviewed pin exists in 7b1, so
    /// native runs mark it explicitly absent and stay incomplete.
    pub artifact: Measured<Hash>,
    pub host: Measured<HostDependency>,
    pub store: Measured<StoreDependency>,
    pub owner_profile: Measured<Hash>,
    pub distribution: Measured<DistributionDependency>,
    pub install_media: Measured<Hash>,
    pub recipes: Measured<Hash>,
    pub firmware: Measured<Hash>,
    pub guest_protocol: Measured<u32>,
    pub network_topology: Measured<Hash>,
    pub policy: PolicyDependency,
}

impl Dependencies {
    pub fn key(&self, scope: Hash) -> Hash {
        digest(&serde_json::to_vec(&(scope, self)).expect("plain data serializes"))
    }

    /// Names of the dependencies that no reviewed collector measures.
    pub fn unmeasured(&self) -> Vec<&'static str> {
        [
            ("artifact", self.artifact.is_measured()),
            ("host", self.host.is_measured()),
            ("store", self.store.is_measured()),
            ("owner_profile", self.owner_profile.is_measured()),
            ("distribution", self.distribution.is_measured()),
            ("install_media", self.install_media.is_measured()),
            ("recipes", self.recipes.is_measured()),
            ("firmware", self.firmware.is_measured()),
            ("guest_protocol", self.guest_protocol.is_measured()),
            ("network_topology", self.network_topology.is_measured()),
        ]
        .into_iter()
        .filter_map(|(name, measured)| (!measured).then_some(name))
        .collect()
    }

    pub fn complete(&self) -> bool {
        self.release == release()
            && self.policy == PolicyDependency::CURRENT
            && self.unmeasured().is_empty()
            && matches!(&self.store, Measured::Measured(store) if store.test_scope.is_some())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub contract: u16,
    pub collector: u16,
    pub origin: Origin,
    pub binding: Binding,
    pub stage: Stage,
    pub started_ms: u64,
    pub finished_ms: u64,
    pub prior: Hash,
    pub result: StageResult,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub file: FileKind,
    pub length: u64,
    pub sha256: Hash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Seal {
    pub contract: u16,
    pub collector: u16,
    pub origin: Origin,
    pub binding: Binding,
    pub finished_utc: u64,
    pub manifest: Vec<ManifestEntry>,
}

/// Canonical files produced from a settled ordered chain. The native sink
/// independently reopens and verifies these files before committing.
pub struct SealBundle {
    pub seal: Vec<u8>,
    pub files: Vec<(FileKind, Vec<u8>)>,
}

pub fn produce_bundle(
    origin: Origin,
    binding: Binding,
    records: &[Record],
    finished_utc: u64,
) -> Result<SealBundle, RunError> {
    let files: Vec<_> = FileKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let group: Vec<_> = records
                .iter()
                .filter(|row| row.stage.file() == kind)
                .collect();
            (!group.is_empty()).then(|| canonical(&group).map(|bytes| (kind, bytes)))
        })
        .collect::<Result<_, _>>()?;
    let seal = Seal {
        contract: CONTRACT,
        collector: COLLECTOR,
        origin,
        binding,
        finished_utc,
        manifest: files
            .iter()
            .map(|(file, bytes)| ManifestEntry {
                file: *file,
                length: bytes.len() as u64,
                sha256: digest(bytes),
            })
            .collect(),
    };
    check_bundle(&seal, &files, origin)?;
    Ok(SealBundle {
        seal: canonical(&seal)?,
        files,
    })
}

#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
pub enum RunError {
    Input,
    OutOfOrder,
    Timing,
    Incomplete,
    Encoding,
    Output,
    Untrusted,
    #[cfg(all(windows, target_arch = "x86_64"))]
    Native(crate::vm::runtime::NativeRefusal),
}

pub fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, RunError> {
    let bytes = serde_json::to_vec(value).map_err(|_| RunError::Encoding)?;
    if bytes.len() > RECORD_LIMIT {
        return Err(RunError::Encoding);
    }
    Ok(bytes)
}

/// Strict decode: bounded, schema-checked and byte-identical to the one
/// canonical encoding, so duplicate, reordered or respelled fields fail.
pub fn decode<T: Serialize + DeserializeOwned>(bytes: &[u8]) -> Result<T, RunError> {
    if bytes.len() > RECORD_LIMIT {
        return Err(RunError::Encoding);
    }
    let parsed = serde_json::from_slice(bytes).map_err(|_| RunError::Encoding)?;
    if canonical(&parsed)? != bytes {
        return Err(RunError::Encoding);
    }
    Ok(parsed)
}

/// Read one evidence file. The declared length is checked before any
/// allocation, and the reader may not yield more than it declared.
pub fn read_bounded(mut file: impl Read, declared: u64) -> Result<Vec<u8>, RunError> {
    if declared == 0 || declared > RECORD_LIMIT as u64 {
        return Err(RunError::Encoding);
    }
    let mut bytes = Vec::with_capacity(declared as usize);
    (&mut file)
        .take(declared + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RunError::Encoding)?;
    if bytes.len() as u64 != declared {
        return Err(RunError::Encoding);
    }
    Ok(bytes)
}

/// The owner-local selection, pinned independently of the bundle.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub seal_sha256: Hash,
    pub scope: Hash,
}

/// Independently re-measured state at verification time.
///
/// The reader compares an independent boot marker and boot uptime with the
/// pinned issuance instant. The daemon's persistent UTC high-water mark is
/// an additional admission check outside this reader.
pub struct Fresh<'a> {
    /// Monotonic milliseconds since host boot.
    pub now: Instant2,
    pub boot_marker: Hash,
    pub dependencies: &'a Dependencies,
}

/// Selection evidence returned only after the native manifest, dependency
/// projection and matching installed endpoint rows have been verified.
#[derive(Debug, Eq, PartialEq)]
pub struct VerifiedEvidence {
    pub seal: Seal,
    pub recipes: TestRecipes,
}

/// Read through a caller-owned evidence directory. The opener must use the
/// held directory and refuse reparse points. Each name and declared size is
/// checked before a row can be opened or allocated.
pub fn read_native<R: Read>(
    seal_file: impl Read,
    seal_length: u64,
    mut open_row: impl FnMut(FileKind) -> Result<R, RunError>,
    selection: &Selection,
    fresh: &Fresh<'_>,
) -> Result<VerifiedEvidence, RunError> {
    let seal_bytes = read_bounded(seal_file, seal_length)?;
    let seal = selected_seal(&seal_bytes, selection, fresh)?;
    let mut files = Vec::with_capacity(seal.manifest.len());
    for entry in &seal.manifest {
        files.push((
            entry.file,
            read_bounded(open_row(entry.file)?, entry.length)?,
        ));
    }
    let recipes = verify_native_rows(&seal, &files, fresh)?;
    Ok(VerifiedEvidence { seal, recipes })
}

/// Verify a complete native bundle already in memory.
pub fn verify_native(
    seal_bytes: &[u8],
    files: &[(FileKind, Vec<u8>)],
    selection: &Selection,
    fresh: &Fresh<'_>,
) -> Result<(), RunError> {
    let seal = selected_seal(seal_bytes, selection, fresh)?;
    verify_native_rows(&seal, files, fresh).map(|_| ())
}

fn verify_native_rows(
    seal: &Seal,
    files: &[(FileKind, Vec<u8>)],
    fresh: &Fresh<'_>,
) -> Result<TestRecipes, RunError> {
    let checked = bundle_rows(seal, files, Origin::Native)?;
    let recipes = checked.recipes.ok_or(RunError::Incomplete)?;
    let finished = seal
        .binding
        .boot
        .origin_mono_ms
        .checked_add(checked.last_finished_ms)
        .ok_or(RunError::Untrusted)?;
    if fresh.dependencies.recipes != Measured::Measured(recipes.key())
        || fresh.now.mono_ms < finished
    {
        return Err(RunError::Untrusted);
    }
    Ok(recipes)
}

fn selected_seal(
    seal_bytes: &[u8],
    selection: &Selection,
    fresh: &Fresh<'_>,
) -> Result<Seal, RunError> {
    selected_native_seal(
        seal_bytes,
        selection.seal_sha256,
        selection.scope,
        Class::Test,
        fresh.now,
        fresh.boot_marker,
        fresh
            .dependencies
            .complete()
            .then(|| fresh.dependencies.key(selection.scope)),
    )
}

fn selected_native_seal(
    seal_bytes: &[u8],
    seal_sha256: Hash,
    scope: Hash,
    class: Class,
    now: Instant2,
    boot_marker: Hash,
    dependency_key: Option<Hash>,
) -> Result<Seal, RunError> {
    if seal_bytes.len() > RECORD_LIMIT || digest(seal_bytes) != seal_sha256 {
        return Err(RunError::Untrusted);
    }
    let seal: Seal = decode(seal_bytes)?;
    let binding = seal.binding;
    let issued = binding
        .issued_on_boot(boot_marker)
        .ok_or(RunError::Untrusted)?;
    if seal.origin != Origin::Native
        || binding.class != class
        || binding.scope != scope
        || !dependency_key.is_some_and(|key| binding.live_at(now.utc, key))
        || !clocks_agree(issued, now)
        || seal.finished_utc < binding.issued_utc
        || seal.finished_utc > now.utc
    {
        return Err(RunError::Untrusted);
    }
    check_manifest(&seal, Origin::Native)?;
    Ok(seal)
}

fn check_manifest(seal: &Seal, origin: Origin) -> Result<(), RunError> {
    let required = FileKind::ALL
        .into_iter()
        .filter(|kind| kind.required(seal.binding.class));
    if seal.contract != CONTRACT
        || seal.collector != COLLECTOR
        || seal.origin != origin
        || seal.manifest.len() != required.clone().count()
    {
        return Err(RunError::Incomplete);
    }
    if seal.manifest.iter().zip(required).any(|(entry, expected)| {
        entry.file != expected || !(1..=RECORD_LIMIT as u64).contains(&entry.length)
    }) {
        return Err(RunError::Encoding);
    }
    Ok(())
}

/// Structural checks shared by every origin: contract, manifest, canonical
/// group order and the linked chain. Provenance is checked by the caller.
pub fn check_bundle(
    seal: &Seal,
    files: &[(FileKind, Vec<u8>)],
    origin: Origin,
) -> Result<(), RunError> {
    bundle_rows(seal, files, origin).map(|_| ())
}

struct BundleRows {
    recipes: Option<TestRecipes>,
    docker: docker::Rows,
    last_finished_ms: u64,
}

fn bundle_rows(
    seal: &Seal,
    files: &[(FileKind, Vec<u8>)],
    origin: Origin,
) -> Result<BundleRows, RunError> {
    let class = seal.binding.class;
    check_manifest(seal, origin)?;
    if files.len() != seal.manifest.len() {
        return Err(RunError::Incomplete);
    }
    let sequence = stages(class);
    let mut by_position: Vec<Option<Record>> = vec![None; sequence.len()];
    for (entry, (file, bytes)) in seal.manifest.iter().zip(files) {
        if *file != entry.file
            || entry.length != bytes.len() as u64
            || entry.sha256 != digest(bytes)
        {
            return Err(RunError::Encoding);
        }
        let records: Vec<Record> = decode(bytes)?;
        let mut last = None;
        for record in records {
            let at = position(class, record.stage).ok_or(RunError::Incomplete)?;
            // One canonical group order: ascending stage position, no sort.
            if record.stage.file() != entry.file
                || last.is_some_and(|prior| at <= prior)
                || by_position[at].is_some()
            {
                return Err(RunError::Incomplete);
            }
            last = Some(at);
            by_position[at] = Some(record);
        }
    }
    let mut previous = [0; 32];
    let mut when = 0;
    let mut ubuntu = None;
    let mut windows = None;
    let mut ubuntu_endpoint = None;
    let mut windows_endpoint = None;
    let mut docker = docker::Rows::default();
    for (slot, stage) in by_position.into_iter().zip(sequence) {
        let record = slot.ok_or(RunError::Incomplete)?;
        if record.contract != CONTRACT
            || record.collector != COLLECTOR
            || record.origin != origin
            || record.stage != *stage
            || record.binding != seal.binding
            || record.prior != previous
            || record.started_ms < when
            || record.finished_ms < record.started_ms
            || record.finished_ms > MAX_TRIAL_MS
            || !record.result.passes(*stage, origin)
            || !observations_within(&record)
        {
            return Err(RunError::Incomplete);
        }
        when = record.finished_ms;
        previous = digest(&canonical(&record)?);
        if origin == Origin::Native {
            docker.observe(class, &record.result)?;
        }
        match (record.stage, record.result) {
            (
                Stage::UbuntuRecipe,
                StageResult::Passed(Observation::UbuntuRecipeVerified { recipe, .. }),
            ) => {
                ubuntu = Some(recipe);
            }
            (
                Stage::WindowsRecipe,
                StageResult::Passed(Observation::WindowsRecipeVerified { recipe, .. }),
            ) => {
                windows = Some(recipe);
            }
            (
                Stage::UbuntuInstalledEndpoint,
                StageResult::Passed(Observation::TestInstalledEndpoint {
                    recipe,
                    boot_identity,
                    ..
                }),
            ) => {
                ubuntu_endpoint = Some((recipe, boot_identity));
            }
            (
                Stage::WindowsInstalledEndpoint,
                StageResult::Passed(Observation::TestInstalledEndpoint {
                    recipe,
                    boot_identity,
                    ..
                }),
            ) => {
                windows_endpoint = Some((recipe, boot_identity));
            }
            _ => {}
        }
    }
    let recipes = if origin == Origin::Native && class == Class::Test {
        let (Some(ubuntu), Some(windows), Some((u, u_boot)), Some((w, w_boot))) =
            (ubuntu, windows, ubuntu_endpoint, windows_endpoint)
        else {
            return Err(RunError::Incomplete);
        };
        let pair = TestRecipes { ubuntu, windows };
        if (ubuntu, windows) != (u, w) || u_boot == w_boot || !pair.valid() {
            return Err(RunError::Untrusted);
        }
        Some(pair)
    } else {
        None
    };
    Ok(BundleRows {
        recipes,
        docker,
        last_finished_ms: when,
    })
}

fn observations_within(record: &Record) -> bool {
    if let StageResult::Passed(Observation::ReserveActivity {
        controlled_faults,
        console_transitions_ms,
    }) = &record.result
    {
        controlled_faults[0].sampled_ms >= record.started_ms
            && console_transitions_ms[4] <= record.finished_ms
    } else {
        true
    }
}

/// The final diagnostic written by an incomplete run, after every record.
/// It lists the unattempted stages and the unmeasured dependencies.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Incomplete {
    pub contract: u16,
    pub origin: Origin,
    pub binding: Binding,
    pub blocked_at: Option<Stage>,
    pub unattempted: Vec<Stage>,
    pub unmeasured: Vec<String>,
    pub dispatched_effects: u16,
    pub charge_retained: bool,
    /// Why a completion was latched late, if it was.
    pub late: Option<Diagnostic>,
    /// Record files the sink refused; the summary is still written last.
    pub unwritten: Vec<FileKind>,
}

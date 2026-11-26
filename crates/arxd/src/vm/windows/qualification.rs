//! Owner-local qualification mechanisms. The separate runner owns its
//! private trial authority. The held baseline supports a fixed paused
//! trial and publishes diagnostic evidence only.
//!
//! Native observations here are **unverified on real hardware**.

mod activity;
mod boot;
mod capacity;
#[allow(
    dead_code,
    reason = "checked native creation and live witness peers remain gated"
)]
pub(super) mod docker_trial;
mod foreground;
pub(crate) use foreground::ForegroundQualification;
pub(super) mod inventory;
mod launch;
pub(super) use launch::WorkspaceCustody;
pub use launch::{RecipeProbe, RecipeProjection};
mod network;
pub(in crate::vm) use network::Targets as WitnessTargets;
mod paused;
mod production;
#[cfg(test)]
pub(crate) use production::consume_checked;
#[allow(unused_imports, reason = "consumed by the 7c foreground issuer")]
pub(crate) use production::{
    FinalizedPair, GenerationGuards, HeldStartRefusal, HelperSettlement, HelperTarget,
    InstallerSettlement, LaunchFailure, LaunchPermit, LaunchPermitBinding, LaunchRequest,
    LaunchTarget, NativeLaunch, NativeRecovery, ProductionWorkspace, ReleasedGeneration,
    StartCreation, StartPair, StartRefusal, WorkspaceBinding,
};
mod recipe;
#[allow(unused_imports, reason = "consumed by the 7c foreground issuer")]
pub(crate) use recipe::{
    BootReservation, InstallerReservation, QualifiedBoot, QualifiedBootPlan, StageTarget,
    StagedInstaller, recipe_selected, stage_supported,
};
mod schema;
mod sink;
mod static_compose;
mod test;

pub use paused::{
    CapacityProbe, CapacityProbePermit, FirmwareProbe, FirmwareProbePermit, NetworkProbe,
    NetworkProbePermit, PausedCompleted, PausedPhase, PausedTrial, PausedTrialPermit,
    PausedVerdict, ReserveActivityPermit, ReserveActivityProbe, SupervisorLossPermit,
    SupervisorLossProbe,
};
pub use schema::{RECORD_COLLECTOR, RECORD_CONTRACT};
pub use sink::{DiagnosticFile, Entry, SinkError};

use super::*;
use serde::{Deserialize, Serialize};
use std::{io::Read, time::Instant};
use windows_sys::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::System::{
        LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        SystemServices::VER_NT_WORKSTATION,
    },
};

const GIB: u64 = 1 << 30;
const DISK_CEILING: u64 = 200 * GIB;
const OUTER_FLOOR: u64 = 50 * GIB;
const RAM_RESERVE: u64 = 16 * GIB;
const JOB_MEMORY_BYTES: usize = 2 * (1 << 30);
const JOB_CPU_RATE: u32 = 2500;
const MAX_ENROLLMENT_BYTES: u64 = 16_384;
/// Reviewed stock distribution manifest (ADR 0054).
pub use crate::vm::qualification_record::STOCK_MANIFEST_SHA256 as STOCK_MANIFEST_1113_SHA256;
/// Five seconds in WTS/FILETIME 100 ns units.
const LIVE_TICKS: i64 = 50_000_000;
const IDLE_TICKS: i64 = 6_000_000_000;
/// Bound between the LSA logon record and the WTS session logon time.
const LOGON_SKEW_TICKS: i64 = 1_200_000_000;
/// Matches the interactive-only rule in `token_identity`.
const LOGON_INTERACTIVE: u32 = 2;

/// Raw local enrollment. It never leaves this module: no field is copied
/// into an observation, error or output.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    owner_sid: String,
    /// Early diagnostic enrollments predate explicit mode and release pins.
    execution_profile: Option<ExecutionProfile>,
    release: Option<ReleaseEnrollment>,
    storage: Option<test::StorageEnrollment>,
    network: Option<test::NetworkEnrollment>,
    windows_setup_vars: Option<ReleaseImage>,
    windows_recipe: Option<launch::WindowsEnrollment>,
    ubuntu_guest: Option<ReleaseImage>,
    /// Enrolled witness account for the full TEST console-switch prompt loop.
    witness_session: Option<WitnessEnrollment>,
    stock_bundle: String,
    trial_root: String,
    trial_volume: VolumePin,
    /// The mount manager's `\\?\Volume{GUID}\` name of the trial volume.
    trial_volume_guid: String,
    trial_identity: VolumePin,
    outer_root: String,
    backing_file: String,
    backing_identity: VolumePin,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WitnessEnrollment {
    user: String,
    domain: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExecutionProfile {
    CurrentUser,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseEnrollment {
    daemon: ReleaseImage,
    qualifier: ReleaseImage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseImage {
    path: String,
    size: u64,
    sha256: String,
}

const MAX_RELEASE_IMAGE_BYTES: u64 = 128 * (1 << 20);

fn hold_release(
    release: &ReleaseEnrollment,
    current_image: &LocalPath,
    evidence: &LocalPath,
    daemon_running: bool,
) -> Result<([u8; 32], [PathPin; 2]), NativeRefusal> {
    let reason = NativeRefusal::QualificationMissing;
    let hold = |image: &ReleaseImage, basename: &str| {
        let path = local(&image.path)?;
        if path.overlaps(evidence)
            || !path
                .as_str()
                .rsplit('\\')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(basename))
            || !image
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(reason);
        }
        let mut sha256 = [0; 32];
        hex::decode_to_slice(&image.sha256, &mut sha256).map_err(|_| reason)?;
        let expected = ArtifactPin {
            size: image.size,
            sha256,
        };
        let mut pin = PathPin::open(&path, false, false)?;
        pin.verify_content(expected, MAX_RELEASE_IMAGE_BYTES)?;
        Ok((expected, pin))
    };
    let (daemon, daemon_pin) = hold(&release.daemon, "arxd.exe")?;
    let (qualifier, qualifier_pin) = hold(&release.qualifier, "arx-vm-qualify.exe")?;
    let running = if daemon_running {
        &daemon_pin
    } else {
        &qualifier_pin
    };
    if !running
        .path
        .as_str()
        .eq_ignore_ascii_case(current_image.as_str())
        || daemon_pin.identity == qualifier_pin.identity
        || daemon.sha256 == qualifier.sha256
    {
        return Err(reason);
    }
    let key = crate::vm::qualification_record::digest(
        &serde_json::to_vec(&(
            "arx-vm-reviewed-release-v1",
            release_digest(),
            (daemon.size, daemon.sha256),
            (qualifier.size, qualifier.sha256),
        ))
        .map_err(|_| reason)?,
    );
    Ok((key, [daemon_pin, qualifier_pin]))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VolumePin {
    volume: u64,
    file: [u8; 16],
}

impl VolumePin {
    fn matches(&self, id: FileIdentity) -> bool {
        self.volume == id.volume && self.file == id.file
    }
}

/// A result with monotonic milliseconds from `Baseline::origin`.
pub struct Observation<T> {
    pub started_ms: u64,
    pub finished_ms: u64,
    pub result: T,
}

/// A post-claim observation. A refusal here never ends the run early: the
/// runner settles it as a typed diagnostic and finishes the directory.
pub enum Taken<T> {
    Observed(Observation<T>),
    Refused { at_ms: u64, reason: NativeRefusal },
}

pub struct Inputs {
    pub held_roots: u8,
    pub enrollment_private: bool,
    pub evidence_private: bool,
    pub trial_root_private: bool,
    pub evidence_claimed: bool,
}

pub struct Host {
    pub windows_build: u32,
    pub windows_revision: u32,
    /// The installation's `MachineGuid`. Local only: the runner binds it into
    /// the keyed invalidation digest and never publishes it.
    pub machine: [u8; 16],
}

pub struct Distribution {
    pub receipt: [u8; 32],
    pub receipt_bytes: u64,
    pub manifest: [u8; 32],
    pub manifest_bytes: u64,
    pub payload_files: u16,
    pub payload_bytes: u64,
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub inner_caller_free_bytes: u64,
    pub inner_total_free_bytes: u64,
    pub inner_total_bytes: u64,
    pub inner_extent_bytes: u64,
    pub vhd_virtual_bytes: u64,
    pub outer_caller_free_bytes: u64,
    pub outer_total_free_bytes: u64,
    pub backing_allocated_bytes: u64,
    pub backing_length_bytes: u64,
    pub disk_ceiling_bytes: u64,
    pub outer_floor_bytes: u64,
    pub disk_budget_bytes: u64,
    pub empty_job_memory_limit_bytes: u64,
    pub empty_job_cpu_rate: u32,
}

pub struct Activity {
    pub console_idle: bool,
    pub active_cpus: u32,
    pub available_ram_bytes: u64,
    pub owner_ram_reserve_bytes: u64,
    pub owner_cpu_reserve: u32,
}

/// Stable identities of the held store objects, taken before the claim from
/// the GUID-anchored walks. They are published only inside a digest keyed
/// by the runner's random scope.
pub struct StoreIdentity {
    pub trial_volume: u64,
    pub trial_volume_root: [u8; 16],
    pub trial_volume_guid: [u8; 16],
    pub trial_root: [u8; 16],
    pub outer_volume: u64,
    pub outer_volume_root: [u8; 16],
    pub outer_volume_guid: [u8; 16],
    pub backing_file: [u8; 16],
}

pub struct Preflight {
    pub inputs: Observation<Inputs>,
    pub host: Taken<Host>,
    pub distribution: Taken<Distribution>,
    pub limits: Taken<Limits>,
    pub activity: Taken<Activity>,
    pub whpx_present: Taken<bool>,
    pub store: StoreIdentity,
    pub owner_profile: Option<[u8; 32]>,
    pub reviewed_release: Option<[u8; 32]>,
    pub test_scope: Option<[u8; 32]>,
}

/// Held custody for the whole run. No public constructor, no Clone, and no
/// authority: it carries only observations, pins and the diagnostic sink.
///
/// The runner's trial authority is not part of this library:
///
/// ```compile_fail,E0432
/// use arxd::vm::qualification::TrialAuthority;
/// ```
///
/// A baseline cannot be assembled outside the preflight. Every field is
/// supplied, so field privacy is the only refusal:
///
/// ```compile_fail,E0451
/// fn forge(p: arxd::vm::qualification::Preflight) -> arxd::vm::qualification::Baseline {
///     arxd::vm::qualification::Baseline {
///         observations: p,
///         origin: std::time::Instant::now(),
///         evidence: todo!(),
///         _held: todo!(),
///     }
/// }
/// ```
///
/// Nor converted into the daemon's launch authority, by `From`:
///
/// ```compile_fail,E0277
/// fn launch(b: arxd::vm::qualification::Baseline) -> arxd::vm::runtime::LaunchAuthority {
///     b.into()
/// }
/// ```
///
/// or by `TryFrom`:
///
/// ```compile_fail,E0277
/// use arxd::vm::runtime::LaunchAuthority;
/// fn launch(b: arxd::vm::qualification::Baseline) -> Option<LaunchAuthority> {
///     LaunchAuthority::try_from(b).ok()
/// }
/// ```
///
/// Nor can a completed qualification be written through the sink:
///
/// ```compile_fail,E0599
/// let _ = arxd::vm::qualification::DiagnosticFile::Qualification;
/// ```
///
/// There is no byte-taking writer, so caller bytes never reach an opaque
/// field:
///
/// ```compile_fail,E0599
/// fn write(b: &arxd::vm::qualification::Baseline) {
///     let _ = b.write(arxd::vm::qualification::DiagnosticFile::Inputs, b"[]");
/// }
/// ```
///
/// and a published entry has no field for the chain digest the writer
/// derives:
///
/// ```compile_fail,E0560
/// fn entry<'a>(
///     stage: &'a &'static str,
///     result: &'a (),
/// ) -> arxd::vm::qualification::Entry<'a, &'static str, ()> {
///     arxd::vm::qualification::Entry { stage, started_ms: 0, finished_ms: 0, result, prior: [0; 32] }
/// }
/// ```
///
/// Positive control: the authority-free surface these checks rely on
/// compiles, so each refusal above is the fence and not a missing path.
///
/// ```no_run
/// use arxd::vm::qualification::{Baseline, DiagnosticFile, Entry, Preflight, SinkError};
/// fn surface<'a>(b: &'a Baseline) -> Result<&'a Preflight, SinkError> {
///     let entry = Entry { stage: &"inputs", started_ms: 0, finished_ms: 1, result: &"unimplemented" };
///     let unwritten: Vec<DiagnosticFile> = b.publish(&"test", &(), 0, &[entry], &())?;
///     let _ = (unwritten, b.scope(), b.run());
///     Ok(b.observations())
/// }
/// fn permit(b: &arxd::vm::qualification::Baseline) {
///     let _ = b.prepare_paused_trial();
/// }
/// let _ = surface;
/// let _ = arxd::vm::qualification::preflight;
/// let _: Option<arxd::vm::runtime::LaunchAuthority> = None;
/// ```
pub struct Baseline {
    observations: Preflight,
    origin: Instant,
    evidence: sink::EvidenceDir,
    _held: Held,
}

struct Held {
    bundle: Option<inventory::Bundle>,
    _source: PathPin,
    _release: Option<[PathPin; 2]>,
    scope: Option<test::Scope>,
    network: Option<test::NetworkInputs>,
    windows_setup_vars: Result<launch::HeldAsset, NativeRefusal>,
    windows_recipe: Result<launch::WindowsInputs, NativeRefusal>,
    ubuntu_guest: Result<launch::HeldAsset, NativeRefusal>,
    witness_session: Option<WtsAccount>,
    trial: PathPin,
    _backing: PathPin,
    volumes: [capacity::AnchoredVolume; 2],
    token: std::sync::Arc<VmToken>,
    activity: activity::Collector,
    _capacity: Option<(OwnedHandle, capacity::HeldDevice)>,
    paused: bool,
    full_test: bool,
}

impl Baseline {
    pub fn dependencies(&self) -> Dependencies {
        self.observations.dependencies()
    }

    pub fn boot_anchor(&self) -> crate::vm::qualification_record::BootAnchor {
        self.evidence.boot_anchor()
    }

    pub fn recipe_projection(&self) -> Result<RecipeProjection, NativeRefusal> {
        if !self._held.full_test {
            return Err(NativeRefusal::QualificationMissing);
        }
        launch::project_recipes(&self._held, self.run(), self.scope())
    }

    /// A settled full-TEST chain stays borrowed against its exact held
    /// baseline, release and recipe projection through the marker rename.
    pub fn publish_test_completion(
        &self,
        bundle: &crate::vm::qualification_record::SealBundle,
        issued: crate::vm::qualification_record::Instant2,
        cancel: &Shutdown,
        clock: impl Fn() -> crate::vm::qualification_record::Instant2,
        dependencies: &crate::vm::qualification_record::Dependencies,
        projection: &RecipeProjection,
    ) -> Result<(), SinkError> {
        use crate::vm::qualification_record::Measured;
        let held = &self._held;
        if !held.full_test
            || projection.run != self.run()
            || projection.scope != self.scope()
            || dependencies.install_media != Measured::Measured(projection.install_media)
            || dependencies.recipes != Measured::Measured(projection.recipes.key())
            || dependencies.firmware != Measured::Measured(projection.firmware)
            || dependencies.network_topology != Measured::Measured(projection.network_topology)
            || dependencies.guest_protocol != Measured::Measured(projection.guest_protocol)
            || self
                .observations
                .reviewed_release
                .map(Measured::Measured)
                .as_ref()
                != Some(&dependencies.artifact)
        {
            return Err(SinkError::Invalid);
        }
        self.evidence
            .publish_complete(bundle, issued, cancel, &clock, || {
                let captured = self.paused_live().map_err(|_| SinkError::Invalid)?;
                if self.boot_anchor().on_boot(clock()).is_none_or(|time| {
                    time.mono_ms.abs_diff(boot::uptime())
                        > crate::vm::qualification_record::CLOCK_SKEW_MS
                }) {
                    return Err(SinkError::Invalid);
                }
                let scope = held.scope.as_ref().ok_or(SinkError::Invalid)?;
                scope.validate_live().map_err(|_| SinkError::Invalid)?;
                for volume in &held.volumes {
                    if !capacity::still_bound(&capacity::MountManager, volume) {
                        return Err(SinkError::Invalid);
                    }
                }
                let (cpus, available) = host_resources().map_err(|_| SinkError::Invalid)?;
                if cpus < 2
                    || available < RAM_RESERVE + 7 * GIB
                    || !hypervisor_present().map_err(|_| SinkError::Invalid)?
                    || !matches!(&dependencies.host, Measured::Measured(host)
                    if Ok(host.windows_build) == platform()
                    && Ok(host.windows_revision) == windows_revision()
                    && Ok(host.machine) == machine_guid())
                    || !matches!(&dependencies.owner_profile, Measured::Measured(profile)
                    if held.token.profile_key().is_ok_and(|live| &live == profile))
                {
                    return Err(SinkError::Invalid);
                }
                launch::revalidate_assets(held).map_err(|_| SinkError::Invalid)?;
                let no_charge = |root: &PathPin, forbidden: &[&str]| {
                    listing_bounded(
                        &root.leaf,
                        4096,
                        Some(Instant::now() + Duration::from_secs(1)),
                    )
                    .is_ok_and(|entries| {
                        entries.iter().all(|(entry, _)| {
                            forbidden
                                .iter()
                                .all(|name| !entry.eq_ignore_ascii_case(name))
                        })
                    })
                };
                Ok(sink::PublishSample {
                    dependencies,
                    live_captured_ms: elapsed(self.origin).min(
                        u64::try_from(captured.duration_since(self.origin).as_millis())
                            .unwrap_or(u64::MAX),
                    ),
                    settled: no_charge(
                        &scope.recipe.root,
                        &[".arx-recipes-charge", ".arx-recipe-capacity-charge"],
                    ) && no_charge(
                        &held.trial,
                        &[".arx-paused-trial-charge", ".arx-network-seed-charge"],
                    ),
                })
            })
    }

    pub async fn run_ubuntu_recipe(&self, cancel: &Shutdown) -> Result<RecipeProbe, NativeRefusal> {
        launch::run_ubuntu(self, cancel).await
    }

    pub fn ubuntu_guest_ready(&self) -> Result<(), NativeRefusal> {
        self._held
            .ubuntu_guest
            .as_ref()
            .map(|_| ())
            .map_err(|reason| *reason)
    }

    pub async fn run_windows_recipe(
        &self,
        cancel: &Shutdown,
    ) -> Result<RecipeProbe, NativeRefusal> {
        launch::run_windows(self, cancel).await
    }

    pub fn windows_recipe_ready(&self) -> Result<(), NativeRefusal> {
        self._held
            .windows_setup_vars
            .as_ref()
            .map_err(|reason| *reason)?;
        self._held
            .windows_recipe
            .as_ref()
            .map(|_| ())
            .map_err(|reason| *reason)
    }

    /// Prepare the fixed owner-local trial from held, explicitly enrolled
    /// prerequisites. The caller must consume its permit to dispatch.
    ///
    /// The permit cannot be constructed by a caller:
    ///
    /// ```compile_fail,E0423
    /// let permit = arxd::vm::qualification::PausedTrialPermit(());
    /// ```
    pub fn prepare_paused_trial(&self) -> Result<PausedTrialPermit<'_>, NativeRefusal> {
        PausedTrialPermit::prepare(self)
    }

    /// Check live owner activity before dispatching the paused operation.
    pub fn paused_live(&self) -> Result<Instant, NativeRefusal> {
        require_paused(self._held.paused || self._held.full_test)?;
        let captured = Instant::now();
        if !self._held.current_console_idle()? {
            return Err(NativeRefusal::OwnerActivityUnverified);
        }
        Ok(captured)
    }

    pub fn prepare_firmware_probe(&self) -> Result<FirmwareProbePermit<'_>, NativeRefusal> {
        FirmwareProbePermit::prepare(self)
    }

    pub fn prepare_supervisor_loss(&self) -> Result<SupervisorLossPermit<'_>, NativeRefusal> {
        SupervisorLossPermit::prepare(self)
    }

    pub fn prepare_capacity_probe(&self) -> Result<CapacityProbePermit<'_>, NativeRefusal> {
        CapacityProbePermit::prepare(self)
    }

    pub fn prepare_reserve_activity(&self) -> Result<ReserveActivityPermit<'_>, NativeRefusal> {
        ReserveActivityPermit::prepare(self)
    }

    pub fn prepare_network_probe(&self) -> Result<NetworkProbePermit<'_>, NativeRefusal> {
        NetworkProbePermit::prepare(self)
    }

    pub fn observations(&self) -> &Preflight {
        &self.observations
    }

    /// The monotonic origin of every observation's milliseconds.
    pub fn origin(&self) -> Instant {
        self.origin
    }

    /// The run's opaque scope, drawn by the evidence writer's own random
    /// generator before the claim.
    pub fn scope(&self) -> [u8; 32] {
        self.evidence.scope()
    }

    /// The run's opaque identifier, drawn like the scope.
    pub fn run(&self) -> [u8; 16] {
        self.evidence.run()
    }

    /// Publish the run's diagnostics once: each entry's stage, times and
    /// result, in chain order, then the summary fields. The writer derives
    /// the binding (scope, run, release and the scope-keyed digest of
    /// `dependencies`) and the record chain itself, commits every file
    /// through a temporary entry, and writes the summary last. It returns the
    /// record files it could not commit, which the summary lists.
    pub fn publish<C, D, St, R, B>(
        &self,
        class: &C,
        dependencies: &D,
        issued_utc: u64,
        entries: &[Entry<'_, St, R>],
        summary: &B,
    ) -> Result<Vec<DiagnosticFile>, SinkError>
    where
        C: Serialize,
        D: Serialize,
        St: Serialize,
        R: Serialize,
        B: Serialize,
    {
        self.evidence
            .publish(class, dependencies, issued_utc, entries, summary)
    }
}

impl Preflight {
    pub fn dependencies(&self) -> crate::vm::qualification_record::Dependencies {
        dependencies(self)
    }
}

fn observed<T>(value: &Taken<T>) -> Option<&T> {
    match value {
        Taken::Observed(observation) => Some(&observation.result),
        Taken::Refused { .. } => None,
    }
}

use crate::vm::qualification_record::{Dependencies, Measured};

fn measured<T>(value: Option<T>) -> Measured<T> {
    value.map_or(Measured::Unmeasured, Measured::Measured)
}

/// The local invalidation projection. A dependency whose observation was
/// refused, or that no reviewed collector measures yet, stays unmeasured,
/// which keeps every native trial unsealable.
fn dependencies(p: &Preflight) -> Dependencies {
    use crate::vm::qualification_record::*;
    let s = &p.store;
    Dependencies {
        release: release(),
        artifact: measured(p.reviewed_release),
        host: measured(observed(&p.host).map(|host| HostDependency {
            windows_build: host.windows_build,
            windows_revision: host.windows_revision,
            machine: host.machine,
        })),
        store: measured(observed(&p.limits).map(|limits| StoreDependency {
            trial_volume: s.trial_volume,
            trial_volume_root: s.trial_volume_root,
            trial_volume_guid: s.trial_volume_guid,
            trial_root: s.trial_root,
            outer_volume: s.outer_volume,
            outer_volume_root: s.outer_volume_root,
            outer_volume_guid: s.outer_volume_guid,
            backing_file: s.backing_file,
            vhd_virtual_bytes: limits.vhd_virtual_bytes,
            inner_extent_bytes: limits.inner_extent_bytes,
            test_scope: p.test_scope,
        })),
        owner_profile: measured(p.owner_profile),
        distribution: measured(observed(&p.distribution).map(|distribution| {
            DistributionDependency {
                receipt: distribution.receipt,
                manifest: distribution.manifest,
            }
        })),
        install_media: Measured::Unmeasured,
        recipes: Measured::Unmeasured,
        firmware: Measured::Unmeasured,
        guest_protocol: Measured::Unmeasured,
        network_topology: Measured::Unmeasured,
        policy: PolicyDependency::CURRENT,
    }
}

/// The release digest the writer binds, as the runner derives it. Pure and
/// authority-free; the runner's tests check the two derivations agree.
pub fn release_digest() -> [u8; 32] {
    schema::release()
}

/// The closed-schema check the held writer applies to `file`. Pure and
/// authority-free; the runner's tests keep every encoding inside it.
pub fn schema_accepts(file: DiagnosticFile, bytes: &[u8]) -> bool {
    schema::check(file, bytes)
}

fn elapsed(origin: Instant) -> u64 {
    u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Take one post-claim observation and whatever it must keep held.
fn take_held<T, H>(
    origin: Instant,
    run: impl FnOnce() -> Result<(T, H), NativeRefusal>,
) -> (Taken<T>, Option<H>) {
    let started_ms = elapsed(origin);
    match run() {
        Ok((result, held)) => (
            Taken::Observed(Observation {
                started_ms,
                finished_ms: elapsed(origin),
                result,
            }),
            Some(held),
        ),
        Err(reason) => (
            Taken::Refused {
                at_ms: elapsed(origin),
                reason,
            },
            None,
        ),
    }
}

fn take<T>(origin: Instant, run: impl FnOnce() -> Result<T, NativeRefusal>) -> Taken<T> {
    take_held(origin, || run().map(|result| (result, ()))).0
}

/// Probe the empty Job only after measuring the held volume and verifying
/// the protected stock distribution.
#[doc(hidden)]
pub fn stock_capacity<H>(
    origin: Instant,
    measure: impl FnOnce() -> Result<(Limits, H), NativeRefusal>,
    verify_distribution: impl FnOnce() -> Result<(), NativeRefusal>,
    probe_job: impl FnOnce() -> Result<(), NativeRefusal>,
) -> (Taken<Limits>, Option<H>) {
    take_held(origin, || {
        let capacity = measure()?;
        verify_distribution()?;
        probe_job()?;
        Ok(capacity)
    })
}

/// Inspect only the explicitly enrolled roots, the current token and console,
/// aggregate counters and the exact stock payload. Behavioral trials are
/// later stages with their own collectors.
pub fn preflight(enrollment_file: &str, evidence_dir: &str) -> Result<Baseline, NativeRefusal> {
    preflight_with(
        &capacity::MountManager,
        enrollment_file,
        evidence_dir,
        false,
        false,
    )
}

fn require_paused(paused: bool) -> Result<(), NativeRefusal> {
    paused
        .then_some(())
        .ok_or(NativeRefusal::DistributionUnverified)
}

/// Preflight for the explicit paused-trial entry point in the owner runner.
pub fn preflight_paused(enrollment: &str, evidence: &str) -> Result<Baseline, NativeRefusal> {
    preflight_with(&capacity::MountManager, enrollment, evidence, true, false)
}

/// Separate, fail-closed entry point for explicit Docker qualification.
pub fn preflight_docker(
    enrollment: &str,
    evidence: &str,
) -> Result<std::convert::Infallible, NativeRefusal> {
    docker_trial::preflight(enrollment, evidence)
}

pub fn preflight_full_test(enrollment: &str, evidence: &str) -> Result<Baseline, NativeRefusal> {
    preflight_with(&capacity::MountManager, enrollment, evidence, false, true)
}

#[derive(Clone, Copy)]
enum CollectMode<'a> {
    /// Standalone qualifier claiming a fresh writable evidence directory and
    /// reading the stopped production store without a daemon lock.
    Qualifier,
    /// Daemon foreground reader borrowing its already-locked `VmStore` and
    /// opening an existing evidence dossier read-only.
    Foreground(&'a crate::store::vm::VmStore),
}

impl CollectMode<'_> {
    fn is_foreground(self) -> bool {
        matches!(self, Self::Foreground(_))
    }

    fn is_qualifier(self) -> bool {
        matches!(self, Self::Qualifier)
    }
}

/// Every refusal before the claim leaves the evidence directory untouched,
/// so a corrected invocation can retry in it. After the claim nothing
/// returns early: each observation is a typed result the runner settles.
fn preflight_with(
    names: &impl capacity::VolumeNames,
    enrollment_file: &str,
    evidence_dir: &str,
    paused: bool,
    full_test: bool,
) -> Result<Baseline, NativeRefusal> {
    let (observations, origin, _held, evidence) = collect_preflight(
        names,
        enrollment_file,
        evidence_dir,
        paused,
        full_test,
        CollectMode::Qualifier,
        sink::EvidenceDir::claim,
    )?;
    Ok(Baseline {
        observations,
        origin,
        _held,
        evidence,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "one shared native collector with separate custody modes"
)]
fn collect_preflight<E>(
    names: &impl capacity::VolumeNames,
    enrollment_file: &str,
    evidence_dir: &str,
    paused: bool,
    full_test: bool,
    mode: CollectMode<'_>,
    open_evidence: impl FnOnce(
        PathPin,
        PSID,
        &[&str],
        crate::vm::qualification_record::BootAnchor,
    ) -> Result<E, NativeRefusal>,
) -> Result<(Preflight, Instant, Held, E), NativeRefusal> {
    let origin_boot_ms = boot::uptime();
    let origin = Instant::now();
    let boot = boot::capture(origin_boot_ms)?;
    let custody = NativeRefusal::PathCustodyUnverified;
    let user = current_user().map_err(|_| NativeRefusal::IdentityUnverified)?;
    let user_sid = user_sid(&user);
    let path = local(enrollment_file)?;
    let source = PathPin::open(&path, false, false)?;
    if !sink::held_private(&source.leaf, user_sid) {
        return Err(custody);
    }
    let bytes = read_bounded(&source.leaf, MAX_ENROLLMENT_BYTES)
        .map_err(|_| NativeRefusal::QualificationMissing)?;
    let enrollment: Enrollment =
        serde_json::from_slice(&bytes).map_err(|_| NativeRefusal::QualificationMissing)?;
    // Identity first, before any root is pinned or output claimed: the
    // enrolled owner must be this process's interactive, non-elevated,
    // non-impersonating console token. An elevated or mismatched invocation
    // is refused here and writes nothing.
    let owner = canonical_sid(&enrollment.owner_sid)?;
    let token = std::sync::Arc::new(
        VmToken::derive_current_user(owner).map_err(|_| NativeRefusal::IdentityUnverified)?,
    );

    let owner_profile = token
        .profile_key()
        .map_err(|_| NativeRefusal::IdentityUnverified)?;

    let root = local(&enrollment.trial_root)?;
    let outer = local(&enrollment.outer_root)?;
    let backing = local(&enrollment.backing_file)?;
    let bundle = local(&enrollment.stock_bundle)?;
    let evidence_path = local(evidence_dir)?;
    let (reviewed_release, release_pins) =
        match (&enrollment.execution_profile, &enrollment.release) {
            (Some(ExecutionProfile::CurrentUser), Some(release)) => {
                let image =
                    std::env::current_exe().map_err(|_| NativeRefusal::QualificationMissing)?;
                let image = local(image.to_str().ok_or(NativeRefusal::QualificationMissing)?)?;
                let (key, pins) =
                    hold_release(release, &image, &evidence_path, mode.is_foreground())?;
                (Some(key), Some(pins))
            }
            (_, None) => (None, None),
            (None, Some(_)) => return Err(NativeRefusal::IdentityUnverified),
        };
    if full_test && reviewed_release.is_none() {
        return Err(NativeRefusal::QualificationMissing);
    }
    if !roots_disjoint(&path, &root, &outer, &backing, &bundle, &evidence_path) {
        return Err(custody);
    }
    // Each drive letter is resolved once, to a GUID root held for the run.
    // Every leaf is walked from its held root, so all observations of a
    // volume come from the same proven objects.
    let outer_volume = capacity::anchor_volume(names, outer.as_str())?;
    let trial_volume = capacity::anchor_volume(names, &root.as_str()[..3])?;
    // The evidence directory shares write in qualifier mode so the writer can
    // rename its committed entries relative to this held handle.
    let evidence_pin = capacity::walk(
        names,
        &outer_volume,
        &evidence_path,
        true,
        mode.is_qualifier(),
    )?;
    let trial_pin = capacity::walk(names, &trial_volume, &root, true, false)?;
    let backing_pin = capacity::walk(names, &outer_volume, &backing, false, true)?;
    if !sink::held_private(&trial_pin.leaf, user_sid)
        || !enrollment.trial_volume.matches(trial_volume.identity)
        || !trial_volume
            .guid
            .eq_ignore_ascii_case(&enrollment.trial_volume_guid)
        || !enrollment.trial_identity.matches(trial_pin.identity)
        || !enrollment.backing_identity.matches(backing_pin.identity)
        || trial_volume.guid_bytes == outer_volume.guid_bytes
        || trial_volume.identity.volume == outer_volume.identity.volume
    {
        return Err(custody);
    }
    let test_scope = if full_test {
        Some(test::Scope::hold(
            names,
            enrollment
                .storage
                .as_ref()
                .ok_or(NativeRefusal::QualificationMissing)?,
            &trial_pin,
            &backing_pin,
            [&trial_volume, &outer_volume],
            &evidence_path,
            (user_sid, mode),
        )?)
    } else {
        None
    };
    let network = if full_test {
        Some(test::NetworkInputs::hold(
            enrollment
                .network
                .ok_or(NativeRefusal::QualificationMissing)?,
            &evidence_path,
        )?)
    } else {
        None
    };
    // A missing recipe asset blocks its own stage, after earlier evidence
    // can be committed. The unmodified stock VARS is never a Setup fallback.
    let recipe_asset = |input, cap| {
        if !full_test {
            return Err(NativeRefusal::InstallerPhaseUnavailable);
        }
        let held = launch::HeldAsset::open(input, &evidence_path, cap)?;
        if test_scope
            .as_ref()
            .is_some_and(|scope| held.overlaps(&scope.recipe.root.path))
        {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        Ok(held)
    };
    let windows_setup_vars = recipe_asset(
        enrollment.windows_setup_vars.as_ref(),
        super::MAX_SETUP_VARS_BYTES,
    );
    let ubuntu_guest = recipe_asset(enrollment.ubuntu_guest.as_ref(), 32 * (1 << 20));
    let windows_recipe = if full_test {
        launch::WindowsInputs::open(
            enrollment.windows_recipe,
            &evidence_path,
            test_scope
                .as_ref()
                .ok_or(NativeRefusal::QualificationMissing)?,
        )
    } else {
        Err(NativeRefusal::InstallerPhaseUnavailable)
    };
    let witness_session = match &enrollment.witness_session {
        Some(witness) => {
            let plain = |s: &str| !s.is_empty() && !s.contains('\\') && s.len() <= 128;
            if !plain(&witness.user) || !plain(&witness.domain) {
                return Err(NativeRefusal::IdentityUnverified);
            }
            Some(WtsAccount {
                user: witness.user.clone(),
                domain: witness.domain.clone(),
            })
        }
        None if full_test => return Err(NativeRefusal::QualificationMissing),
        None => None,
    };
    let mut private_values = vec![
        enrollment.owner_sid.as_str(),
        enrollment.stock_bundle.as_str(),
        enrollment.trial_root.as_str(),
        enrollment.trial_volume_guid.as_str(),
        enrollment.outer_root.as_str(),
        enrollment.backing_file.as_str(),
        enrollment_file,
        evidence_dir,
    ];
    if let Some(witness) = &enrollment.witness_session {
        private_values.push(witness.user.as_str());
        private_values.push(witness.domain.as_str());
    }
    // Claim once identity and every input root are held and checked. A
    // concurrent runner loses here, before any observation is taken.
    let evidence = open_evidence(evidence_pin, user_sid, &private_values, boot)?;

    let inputs = Observation {
        started_ms: 0,
        finished_ms: elapsed(origin),
        result: Inputs {
            held_roots: 5,
            enrollment_private: true,
            evidence_private: true,
            trial_root_private: true,
            evidence_claimed: mode.is_qualifier(),
        },
    };
    let host = take(origin, || {
        Ok(Host {
            windows_build: platform()?,
            windows_revision: windows_revision()?,
            machine: machine_guid()?,
        })
    });
    let (distribution, bundle_pins) = take_held(origin, || inventory::collect(&bundle));
    let measure = || {
        let reason = NativeRefusal::DiskCeilingUnverified;
        let (vhd, facts) = capacity::fixed_vhd(names, &outer_volume, &backing_pin)?;
        let device = capacity::volume_extent(&trial_volume, facts.disk)?;
        let sample = capacity::CapacitySample {
            provider_subtype: facts.provider_subtype,
            vhd_virtual: facts.virtual_size,
            vhd_physical: facts.physical_size,
            sector: facts.sector,
            extent_start: device.extent_start,
            extent_length: device.extent_length,
            inner: capacity::held_free(&trial_pin.leaf)?,
            outer: capacity::held_free(&outer_volume.root)?,
            backing_length: backing_pin.leaf.metadata().map_err(|_| reason)?.len(),
            backing_allocated: backing_pin.allocated,
        };
        let limits = capacity::check_capacity(&sample)?;
        if test_scope
            .as_ref()
            .is_some_and(|scope| !scope.within_shared_cap(&limits))
        {
            return Err(reason);
        }
        // A drive letter rebound while the observation ran closes it.
        if !capacity::still_bound(names, &trial_volume)
            || !capacity::still_bound(names, &outer_volume)
        {
            return Err(reason);
        }
        Ok((limits, (vhd, device)))
    };
    let (limits, capacity_handles) = stock_capacity(
        origin,
        measure,
        || {
            let bundle = bundle_pins
                .as_ref()
                .ok_or(NativeRefusal::DistributionUnverified)?;
            bundle.protected(token.account.0, Instant::now() + Duration::from_secs(2))
        },
        empty_job,
    );
    let activity_collector = activity::Collector::default();
    let activity = take(origin, || {
        let console_idle = activity_collector.baseline(&token, mode.is_qualifier())?;
        let (active_cpus, available_ram_bytes) = host_resources()?;
        Ok(Activity {
            console_idle,
            active_cpus,
            available_ram_bytes,
            owner_ram_reserve_bytes: RAM_RESERVE,
            owner_cpu_reserve: active_cpus.div_ceil(2),
        })
    });
    let whpx_present = take(origin, hypervisor_present);
    let store = StoreIdentity {
        trial_volume: trial_volume.identity.volume,
        trial_volume_root: trial_volume.identity.file,
        trial_volume_guid: trial_volume.guid_bytes,
        trial_root: trial_pin.identity.file,
        outer_volume: outer_volume.identity.volume,
        outer_volume_root: outer_volume.identity.file,
        outer_volume_guid: outer_volume.guid_bytes,
        backing_file: backing_pin.identity.file,
    };
    Ok((
        Preflight {
            inputs,
            host,
            distribution,
            limits,
            activity,
            whpx_present,
            store,
            owner_profile: Some(owner_profile),
            reviewed_release,
            test_scope: test_scope.as_ref().map(|scope| scope.key),
        },
        origin,
        Held {
            bundle: bundle_pins,
            _source: source,
            _release: release_pins,
            scope: test_scope,
            network,
            windows_setup_vars,
            windows_recipe,
            ubuntu_guest,
            witness_session,
            trial: trial_pin,
            _backing: backing_pin,
            volumes: [trial_volume, outer_volume],
            token,
            activity: activity_collector,
            _capacity: capacity_handles,
            paused,
            full_test,
        },
        evidence,
    ))
}

/// Containment rules for the enrolled roots and the evidence directory.
fn roots_disjoint(
    enrollment: &LocalPath,
    root: &LocalPath,
    outer: &LocalPath,
    backing: &LocalPath,
    bundle: &LocalPath,
    evidence: &LocalPath,
) -> bool {
    !(evidence.is_drive_root()
        || !outer.contains(evidence)
        || evidence.overlaps(root)
        || evidence.overlaps(bundle)
        || evidence.contains(enrollment)
        || evidence.overlaps(backing)
        || root.is_drive_root()
        || bundle.is_drive_root()
        || !outer.as_str().eq_ignore_ascii_case(r"D:\")
        || !outer.contains(backing)
        || backing.is_drive_root()
        || root.overlaps(outer)
        || bundle.overlaps(root)
        || bundle.overlaps(backing)
        || root.contains(enrollment))
}

fn empty_job() -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::JobUnverified;
    let job = Job::new(JOB_MEMORY_BYTES, Some(JOB_CPU_RATE)).map_err(|_| reason)?;
    let mut flags = 0;
    // Query only our own empty, unnamed Job. Assignment, nesting and
    // supervisor loss are later stages and cannot be inferred here.
    if unsafe { GetHandleInformation(job.0.as_raw_handle(), &mut flags) } == 0
        || flags & HANDLE_FLAG_INHERIT != 0
        || !matches!(job.active(), Ok(0))
    {
        return Err(reason);
    }
    Ok(())
}

fn current_user() -> io::Result<Vec<usize>> {
    let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY)?;
    token_data(&token, TokenUser)
}

pub(crate) fn require_held_private(file: &File) -> io::Result<()> {
    let user = current_user()?;
    if sink::held_private(file, user_sid(&user)) {
        Ok(())
    } else {
        Err(io::Error::other("private owner-only permissions required"))
    }
}

fn user_sid(data: &[usize]) -> PSID {
    // TokenUser data begins with a TOKEN_USER that points into the buffer.
    unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

fn local(raw: &str) -> Result<LocalPath, NativeRefusal> {
    LocalPath::parse(raw).map_err(|_| NativeRefusal::PathCustodyUnverified)
}

fn canonical_sid(raw: &str) -> Result<LocalSid, NativeRefusal> {
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    let reason = NativeRefusal::IdentityUnverified;
    if raw.len() > 184 || !raw.starts_with("S-1-") {
        return Err(reason);
    }
    let sid = LocalSid::parse(raw).map_err(|_| reason)?;
    let mut output = null_mut();
    // The OS round trip rejects aliases and noncanonical SID spellings.
    if unsafe { ConvertSidToStringSidW(sid.0, &mut output) } == 0 || output.is_null() {
        return Err(reason);
    }
    let expected = wide(raw);
    let mut matches = true;
    for (i, &unit) in expected.iter().enumerate() {
        // ConvertSidToStringSidW returned a terminated allocation. Stop at
        // the first mismatch, including an early terminator.
        if unsafe { *output.add(i) } != unit {
            matches = false;
            break;
        }
    }
    unsafe { LocalFree(output.cast()) };
    matches.then_some(sid).ok_or(reason)
}

/// The 16 bytes of an `8-4-4-4-12` hexadecimal GUID spelling.
fn parse_guid(text: &str) -> Option<[u8; 16]> {
    let bytes = text.as_bytes();
    if bytes.len() != 36 || [8, 13, 18, 23].iter().any(|&i| bytes[i] != b'-') {
        return None;
    }
    let digits: String = text.chars().filter(|&c| c != '-').collect();
    if digits.len() != 32 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0; 16];
    hex::decode_to_slice(&digits, &mut out).ok()?;
    Some(out)
}

fn platform() -> Result<u32, NativeRefusal> {
    let reason = NativeRefusal::UnsupportedPlatform;
    let (mut process, mut machine) = (0, 0);
    let mut version = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    // The extended buffer is selected by its size. Check the actual machine
    // separately so an emulated process does not qualify an ARM64 host.
    if unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut machine) } == 0
        || process != IMAGE_FILE_MACHINE_UNKNOWN
        || machine != IMAGE_FILE_MACHINE_AMD64
        || unsafe { RtlGetVersion(ptr_of(&mut version).cast()) } != 0
        || version.dwMajorVersion != 10
        || version.dwMinorVersion != 0
        || version.dwBuildNumber < 22_000
        || u32::from(version.wProductType) != VER_NT_WORKSTATION
    {
        return Err(reason);
    }
    Ok(version.dwBuildNumber)
}

/// The update build revision, which build numbers alone omit.
fn windows_revision() -> Result<u32, NativeRefusal> {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
    };
    let mut value = 0u32;
    let mut size = size_of_val(&value) as u32;
    // A typed DWORD read of one fixed value into a fixed buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").as_ptr(),
            wide("UBR").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            ptr_of(&mut value),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS || size != 4 {
        return Err(NativeRefusal::UnsupportedPlatform);
    }
    Ok(value)
}

/// The installation's stable `MachineGuid`, for the local invalidation key.
fn machine_guid() -> Result<[u8; 16], NativeRefusal> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let reason = NativeRefusal::UnsupportedPlatform;
    let mut value = [0u16; 40];
    let mut size = size_of_val(&value) as u32;
    // A typed, terminated string read of one fixed value into a fixed buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SOFTWARE\Microsoft\Cryptography").as_ptr(),
            wide("MachineGuid").as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            value.as_mut_ptr().cast(),
            &mut size,
        )
    };
    // Thirty-six UTF-16 units and the terminator.
    if status != ERROR_SUCCESS || size != 74 {
        return Err(reason);
    }
    String::from_utf16(&value[..36])
        .ok()
        .and_then(|text| parse_guid(&text))
        .ok_or(reason)
}

fn hypervisor_present() -> Result<bool, NativeRefusal> {
    let reason = NativeRefusal::WhpxUnverified;
    // Avoid adding a daemon load-time dependency on this optional platform DLL.
    let lib = unsafe {
        LoadLibraryExW(
            wide("WinHvPlatform.dll").as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if lib.is_null() {
        return Err(reason);
    }
    // The function pointer is used only before this DLL reference is released.
    let entry = unsafe { GetProcAddress(lib, c"WHvGetCapability".as_ptr().cast()) };
    let result = entry.ok_or(reason).and_then(|entry| {
        type GetCapability =
            unsafe extern "system" fn(i32, *mut core::ffi::c_void, u32, *mut u32) -> i32;
        let query: GetCapability = unsafe { std::mem::transmute(entry) };
        let mut value = 0u64;
        let mut written = 0;
        // WHvCapabilityCodeHypervisorPresent is code zero and returns a BOOL.
        let status = unsafe {
            query(
                0,
                ptr_of(&mut value),
                size_of_val(&value) as u32,
                &mut written,
            )
        };
        match (status, written, value) {
            (0, 4, 0) => Ok(false),
            (0, 4, 1) => Ok(true),
            _ => Err(reason),
        }
    });
    unsafe { FreeLibrary(lib) };
    result
}

/// One read-only WTS sample of the token's session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct WtsSample {
    session: u32,
    state: i32,
    flags: i32,
    logon: i64,
    input: i64,
    current: i64,
}

/// The account names a WTS sample reports for the session.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WtsAccount {
    user: String,
    domain: String,
}

/// The LSA record for the token's logon LUID, read from the local logon
/// session itself. Its names are the record's own, never looked up by name.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LsaLogon {
    logon_type: u32,
    session: u32,
    logon_time: i64,
    owner: bool,
    user: String,
    domain: String,
}

#[derive(Clone, Debug)]
struct ConsoleEvidence {
    token_session: u32,
    active_console: u32,
    /// The account names of both WTS samples; `None` when unreadable.
    accounts: [Option<WtsAccount>; 2],
    wts: [WtsSample; 2],
    lsa: [LsaLogon; 2],
    /// The token's session, LUID and logon SID did not change.
    token_stable: bool,
    now: i64,
    local_input: Option<activity::LocalSample>,
}

impl ConsoleEvidence {
    fn owner_console_active(&self) -> bool {
        self.active_console == self.token_session
            && self.wts.iter().all(|sample| {
                sample.state == WTSActive && sample.flags == WTS_SESSIONSTATE_UNLOCK as i32
            })
    }
}

impl Held {
    fn current_console_idle(&self) -> Result<bool, NativeRefusal> {
        self.activity
            .capture(&self.token, activity::View::Admission)
            .map(|(_, input)| input.idle)
    }
}

/// Offline binding of the WTS session account to the LSA logon record that
/// is already pinned by the token's LUID and the enrolled SID: both must
/// name the same account. Nothing is resolved by name, so no lookup can
/// leave the host. Empty, qualified or unequal names are ambiguous and
/// close; WTS truncates long names, so a truncated name never matches.
fn account_bound(account: Option<&WtsAccount>, lsa: &LsaLogon) -> bool {
    let plain = |name: &str| !name.is_empty() && !name.contains('\\');
    account.is_some_and(|wts| {
        plain(&wts.user)
            && plain(&wts.domain)
            && plain(&lsa.user)
            && plain(&lsa.domain)
            && wts.user.eq_ignore_ascii_case(&lsa.user)
            && wts.domain.eq_ignore_ascii_case(&lsa.domain)
    })
}

/// Prove the WTS/LSA identity and chronology before selecting an input source.
fn console_source(
    e: &ConsoleEvidence,
    view: activity::View,
) -> Result<activity::Source, NativeRefusal> {
    let reason = NativeRefusal::OwnerActivityUnverified;
    let [first, second] = e.wts;
    let session = e.token_session;
    let lsa_ok = e.lsa.iter().all(|lsa| {
        lsa.owner
            && lsa.session == session
            && lsa.logon_type == LOGON_INTERACTIVE
            && lsa.logon_time > 0
            && lsa.logon_time.abs_diff(first.logon) <= LOGON_SKEW_TICKS as u64
    }) && e.lsa[0] == e.lsa[1];
    if e.accounts[0] != e.accounts[1]
        || !account_bound(e.accounts[0].as_ref(), &e.lsa[0])
        || !e.token_stable
        || !lsa_ok
        || (view == activity::View::Admission && !e.owner_console_active())
        || first.session != session
        || second.session != session
        || first.logon <= 0
        || first.logon != second.logon
        || second.current < first.current
        || !(0..=LIVE_TICKS).contains(&e.now.saturating_sub(second.current))
    {
        return Err(reason);
    }
    match (first.input, second.input) {
        // Both logon times match and current time cannot decrease.
        (0, 0) if first.current >= first.logon => Ok(activity::Source::Local),
        (0, _) | (_, 0) => Err(reason),
        _ if first.input >= first.logon
            && second.input >= first.input
            && second.current >= second.input =>
        {
            Ok(activity::Source::Wts)
        }
        _ => Err(reason),
    }
}

fn wts_sample(session: u32) -> Option<(WtsSample, Option<WtsAccount>)> {
    let raw = wts_session(session)?;
    let account = wts_text(&raw.UserName)
        .zip(wts_text(&raw.DomainName))
        .map(|(user, domain)| WtsAccount { user, domain });
    Some((
        WtsSample {
            session: raw.SessionId,
            state: raw.SessionState,
            flags: raw.SessionFlags,
            logon: raw.LogonTime,
            input: raw.LastInputTime,
            current: raw.CurrentTime,
        },
        account,
    ))
}

/// A counted LSA string; anything malformed reads as empty, which closes.
fn lsa_text(text: &LSA_UNICODE_STRING) -> String {
    if text.Buffer.is_null() || !text.Length.is_multiple_of(2) || text.Length > text.MaximumLength {
        return String::new();
    }
    // The LSA record owns this counted buffer until it is released.
    let units = unsafe { std::slice::from_raw_parts(text.Buffer, usize::from(text.Length / 2)) };
    String::from_utf16(units).unwrap_or_default()
}

fn lsa_logon(luid: &LUID, owner: PSID) -> Option<LsaLogon> {
    let mut data = null_mut();
    // The LSA allocates the record; it is released below.
    if unsafe { LsaGetLogonSessionData(luid, &mut data) } < 0 || data.is_null() {
        return None;
    }
    // A successful call returned one complete record.
    let record = unsafe { &*data };
    let logon = LsaLogon {
        logon_type: record.LogonType,
        session: record.Session,
        logon_time: record.LogonTime,
        owner: !record.Sid.is_null() && unsafe { EqualSid(record.Sid, owner) } != 0,
        user: lsa_text(&record.UserName),
        domain: lsa_text(&record.LogonDomain),
    };
    unsafe { LsaFreeReturnBuffer(data.cast()) };
    Some(logon)
}

fn console_evidence(
    token: &VmToken,
    view: activity::View,
) -> Result<ConsoleEvidence, NativeRefusal> {
    let reason = NativeRefusal::OwnerActivityUnverified;
    require_unimpersonated().map_err(|_| reason)?;
    let session = token.identity.session;
    let luid = token.identity.authentication;
    let lsa_first = lsa_logon(&luid, token.account.0).ok_or(reason)?;
    let (first, account) = wts_sample(session).ok_or(reason)?;
    // Bracket the session-local query with independent WTS observations.
    let local_input = if first.input == 0
        && first.session == session
        && first.state == WTSActive
        && first.flags == WTS_SESSIONSTATE_UNLOCK as i32
    {
        activity::local_sample(session)
    } else {
        None
    };
    let (second, account_again) = wts_sample(session).ok_or(reason)?;
    let lsa_second = lsa_logon(&luid, token.account.0).ok_or(reason)?;
    let actual = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).map_err(|_| reason)?;
    let now_identity = inspect_token_identity(
        &actual,
        &token.account,
        false,
        TokenMode::CurrentUser,
        view == activity::View::Admission,
    )
    .map_err(|_| reason)?;
    let token_stable = now_identity.session == session
        && now_identity.authentication.LowPart == luid.LowPart
        && now_identity.authentication.HighPart == luid.HighPart
        && unsafe { EqualSid(now_identity.logon_sid(), token.identity.logon_sid()) } != 0;
    let mut time = FILETIME::default();
    unsafe { GetSystemTimeAsFileTime(&mut time) };
    let now = ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) as i64;
    Ok(ConsoleEvidence {
        token_session: session,
        active_console: unsafe { WTSGetActiveConsoleSessionId() },
        accounts: [account, account_again],
        wts: [first, second],
        lsa: [lsa_first, lsa_second],
        token_stable,
        now,
        local_input: local_input.filter(|sample| sample.fresh_at(boot::uptime())),
    })
}

fn read_bounded(file: &File, max: u64) -> io::Result<Vec<u8>> {
    let declared = file.metadata()?.len();
    // Refuse an oversized declared length before allocating.
    if declared > max {
        return Err(io::Error::other("input byte limit exceeded"));
    }
    let mut bytes = Vec::with_capacity(declared as usize);
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::other("input byte limit exceeded"));
    }
    Ok(bytes)
}

/// Every entry of a held directory with its attributes, read through the
/// handle itself; the directory is never enumerated by name.
fn listing(dir: &File) -> io::Result<Vec<(String, u32)>> {
    listing_bounded(dir, 4096, None)
}

enum ListingError {
    /// Directory contains more entries than the qualified bound allows.
    LimitExceeded,
    /// Enumeration hit its wall-clock deadline before finishing.
    TimedOut,
    /// OS / Win32 I/O or malformed directory-record failure.
    Io(io::Error),
}

impl ListingError {
    fn into_io(self) -> io::Error {
        match self {
            Self::LimitExceeded => io::Error::other("directory entry limit exceeded"),
            Self::TimedOut => io::ErrorKind::TimedOut.into(),
            Self::Io(error) => error,
        }
    }
}

fn listing_bounded(
    dir: &File,
    max_entries: usize,
    deadline: Option<Instant>,
) -> io::Result<Vec<(String, u32)>> {
    listing_bounded_typed(dir, max_entries, deadline).map_err(ListingError::into_io)
}

fn listing_bounded_typed(
    dir: &File,
    max_entries: usize,
    deadline: Option<Instant>,
) -> Result<Vec<(String, u32)>, ListingError> {
    listing_bounded_with(dir, max_entries, deadline, &mut Instant::now)
}

fn listing_bounded_with(
    dir: &File,
    max_entries: usize,
    deadline: Option<Instant>,
    now: &mut impl FnMut() -> Instant,
) -> Result<Vec<(String, u32)>, ListingError> {
    let mut buffer = vec![0u64; 8192];
    let capacity = buffer.len() * size_of::<u64>();
    let header = size_of::<FILE_FULL_DIR_INFO>();
    let name_offset = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName);
    let mut found = Vec::new();
    let mut class = FileFullDirectoryRestartInfo;
    loop {
        if deadline.is_some_and(|until| now() >= until) {
            return Err(ListingError::TimedOut);
        }
        // The aligned buffer receives a chain of variable-length records.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle(),
                class,
                buffer.as_mut_ptr().cast(),
                capacity as u32,
            )
        };
        let os_error = (ok == 0).then(io::Error::last_os_error);
        // Check the deadline after capturing the Win32 status so `now()`
        // cannot clobber `GetLastError` and a read that crosses the cutoff
        // cannot resolve as a terminal inventory result.
        if deadline.is_some_and(|until| now() >= until) {
            return Err(ListingError::TimedOut);
        }
        if let Some(error) = os_error {
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(found);
            }
            return Err(ListingError::Io(error));
        }
        class = FileFullDirectoryInfo;
        let bytes = buffer.as_ptr().cast::<u8>();
        let mut offset = 0;
        loop {
            if offset + header > capacity {
                return Err(ListingError::Io(io::Error::other(
                    "directory record out of bounds",
                )));
            }
            // The record starts inside the buffer at an 8-byte boundary.
            let entry = unsafe { &*bytes.add(offset).cast::<FILE_FULL_DIR_INFO>() };
            let length = entry.FileNameLength as usize;
            if offset + name_offset + length > capacity || !length.is_multiple_of(2) {
                return Err(ListingError::Io(io::Error::other(
                    "directory name out of bounds",
                )));
            }
            // The name lies inside the buffer, as checked above.
            let units = unsafe {
                std::slice::from_raw_parts(
                    bytes.add(offset + name_offset).cast::<u16>(),
                    length / 2,
                )
            };
            let name = String::from_utf16(units)
                .map_err(|error| ListingError::Io(io::Error::other(error)))?;
            if name != "." && name != ".." {
                if found.len() == max_entries {
                    return Err(ListingError::LimitExceeded);
                }
                found.push((name, entry.FileAttributes));
            }
            if entry.NextEntryOffset == 0 {
                break;
            }
            let next = entry.NextEntryOffset as usize;
            if next < name_offset + length || !next.is_multiple_of(8) {
                return Err(ListingError::Io(io::Error::other(
                    "invalid directory record offset",
                )));
            }
            offset += next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_mode_cannot_enter_paused_preparation_or_live_sampling() {
        assert_eq!(
            require_paused(false),
            Err(NativeRefusal::DistributionUnverified)
        );
        assert_eq!(require_paused(true), Ok(()));
    }

    #[test]
    fn exact_release_binds_two_held_images_and_the_expected_current_image() {
        let scratch = tempfile::tempdir().unwrap();
        let image = |name, data: &[u8]| {
            let path = scratch.path().join(name);
            std::fs::write(&path, data).unwrap();
            ReleaseImage {
                path: path.to_str().unwrap().to_owned(),
                size: data.len() as u64,
                sha256: hex::encode(crate::vm::qualification_record::digest(data)),
            }
        };
        let mut release = ReleaseEnrollment {
            daemon: image("arxd.exe", b"reviewed daemon fixture"),
            qualifier: image("arx-vm-qualify.exe", b"reviewed qualifier fixture"),
        };
        let qualifier = local(&release.qualifier.path).unwrap();
        let evidence = LocalPath::parse(scratch.path().join("evidence").to_str().unwrap()).unwrap();
        let (qualified, pins) = hold_release(&release, &qualifier, &evidence, false).unwrap();
        let daemon = local(&release.daemon.path).unwrap();
        let (deployed, foreground_pins) = hold_release(&release, &daemon, &evidence, true).unwrap();
        assert_eq!(qualified, deployed);
        assert!(hold_release(&release, &qualifier, &evidence, true).is_err());
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&release.daemon.path)
                .is_err(),
            "release pin excludes writers"
        );
        assert!(
            hold_release(
                &release,
                &local(&release.daemon.path).unwrap(),
                &evidence,
                false
            )
            .is_err()
        );
        drop(pins);
        drop(foreground_pins);
        release.daemon.size += 1;
        assert!(hold_release(&release, &qualifier, &evidence, false).is_err());
    }

    #[test]
    fn canonical_sid_round_trips_only_exact_spellings() {
        assert!(canonical_sid("S-1-5-18").is_ok());
        assert!(canonical_sid("S-1-12-1-1111111111-2222222222-3333333333-4044444444").is_ok());
        for bad in ["s-1-5-18", "S-1-5-018", "S-1-0x5-18", "SY", "S-1-5-18 ", ""] {
            assert_eq!(
                canonical_sid(bad).err(),
                Some(NativeRefusal::IdentityUnverified),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn enrolled_roots_must_be_disjoint_and_contained() {
        let p = |raw: &str| LocalPath::parse(raw).unwrap();
        let (enrollment, root, outer, backing, bundle, evidence) = (
            p(r"C:\Owner\enrollment.json"),
            p(r"T:\trial"),
            p(r"D:\"),
            p(r"D:\Arx\trial.vhdx"),
            p(r"C:\Arx\qemu"),
            p(r"D:\Arx\evidence"),
        );
        assert!(roots_disjoint(
            &enrollment,
            &root,
            &outer,
            &backing,
            &bundle,
            &evidence
        ));
        for (name, enrollment, root, outer, backing, bundle, evidence) in [
            (
                "evidence is a drive root",
                &enrollment,
                &root,
                &outer,
                &backing,
                &bundle,
                &p(r"D:\"),
            ),
            (
                "evidence outside outer",
                &enrollment,
                &root,
                &outer,
                &backing,
                &bundle,
                &p(r"C:\e"),
            ),
            (
                "evidence holds backing",
                &enrollment,
                &root,
                &outer,
                &backing,
                &bundle,
                &p(r"D:\Arx"),
            ),
            (
                "trial root is a drive",
                &enrollment,
                &p(r"T:\"),
                &outer,
                &backing,
                &bundle,
                &evidence,
            ),
            (
                "outer is not D",
                &enrollment,
                &root,
                &p(r"E:\"),
                &backing,
                &bundle,
                &evidence,
            ),
            (
                "backing outside outer",
                &enrollment,
                &root,
                &outer,
                &p(r"C:\b.vhdx"),
                &bundle,
                &evidence,
            ),
            (
                "bundle in trial root",
                &enrollment,
                &root,
                &outer,
                &backing,
                &p(r"T:\trial\q"),
                &evidence,
            ),
            (
                "enrollment in trial root",
                &p(r"T:\trial\e.json"),
                &root,
                &outer,
                &backing,
                &bundle,
                &evidence,
            ),
            (
                "enrollment in evidence",
                &p(r"D:\Arx\evidence\e.json"),
                &root,
                &outer,
                &backing,
                &bundle,
                &evidence,
            ),
        ] {
            assert!(
                !roots_disjoint(enrollment, root, outer, backing, bundle, evidence),
                "{name}"
            );
        }
    }

    type Mutation = (&'static str, fn(&mut ConsoleEvidence));

    fn account(user: &str, domain: &str) -> Option<WtsAccount> {
        Some(WtsAccount {
            user: user.to_owned(),
            domain: domain.to_owned(),
        })
    }

    pub(super) fn console() -> ConsoleEvidence {
        let wts = WtsSample {
            session: 2,
            state: WTSActive,
            flags: WTS_SESSIONSTATE_UNLOCK as i32,
            logon: 100,
            input: 100,
            current: 100 + IDLE_TICKS,
        };
        let lsa = LsaLogon {
            logon_type: LOGON_INTERACTIVE,
            session: 2,
            logon_time: 150,
            owner: true,
            user: "Owner".to_owned(),
            domain: "AzureAD".to_owned(),
        };
        ConsoleEvidence {
            token_session: 2,
            active_console: 2,
            accounts: [account("Owner", "AzureAD"), account("Owner", "AzureAD")],
            wts: [
                wts,
                WtsSample {
                    current: wts.current + 10,
                    ..wts
                },
            ],
            lsa: [lsa.clone(), lsa],
            token_stable: true,
            now: wts.current + 20,
            local_input: None,
        }
    }

    fn console_decision(e: &ConsoleEvidence) -> Result<bool, NativeRefusal> {
        match console_source(e, activity::View::Admission)? {
            activity::Source::Wts => Ok(activity::Input::wts(e.wts[1]).idle),
            activity::Source::Local => Err(NativeRefusal::OwnerActivityUnverified),
        }
    }

    #[test]
    fn console_idle_requires_a_stable_correlated_owner_session() {
        assert_eq!(console_decision(&console()), Ok(true));
        let mut active = console();
        active.wts[1].input = active.wts[1].current - 1;
        assert_eq!(
            console_decision(&active),
            Ok(false),
            "recent input is owner activity"
        );
        // Account names compare case-insensitively on both sides.
        let mut cased = console();
        cased.accounts = [account("OWNER", "azuread"), account("OWNER", "azuread")];
        assert_eq!(console_decision(&cased), Ok(true));

        let cases: [Mutation; 25] = [
            ("unreadable WTS account", |e| e.accounts = [None, None]),
            ("LSA record names another account", |e| {
                e.lsa[0].user = "Other".to_owned();
                e.lsa[1].user = "Other".to_owned();
            }),
            ("LSA record names another domain", |e| {
                e.lsa[0].domain = "WORKGROUP".to_owned();
                e.lsa[1].domain = "WORKGROUP".to_owned();
            }),
            ("empty names on both sides", |e| {
                e.lsa[0].user = String::new();
                e.lsa[1].user = String::new();
                e.accounts = [account("", "AzureAD"), account("", "AzureAD")];
            }),
            ("qualified names on both sides", |e| {
                e.accounts = [
                    account(r"AzureAD\Owner", "AzureAD"),
                    account(r"AzureAD\Owner", "AzureAD"),
                ];
                e.lsa[0].user = r"AzureAD\Owner".to_owned();
                e.lsa[1].user = r"AzureAD\Owner".to_owned();
            }),
            ("truncated WTS name", |e| {
                e.accounts = [account("Own", "AzureAD"), account("Own", "AzureAD")]
            }),
            ("account name changed", |e| {
                e.accounts[1] = account("Owner2", "AzureAD")
            }),
            ("token incarnation changed", |e| e.token_stable = false),
            ("switched console", |e| e.active_console = 3),
            ("session mismatch", |e| e.wts[1].session = 3),
            ("disconnected", |e| e.wts[1].state = WTSDisconnected),
            ("locked", |e| e.wts[0].flags = WTS_SESSIONSTATE_LOCK as i32),
            ("logon incarnation changed", |e| e.wts[1].logon = 101),
            ("input before logon", |e| {
                e.wts[0].input = 99;
                e.wts[1].input = 99;
            }),
            ("WTS clock backwards", |e| {
                e.wts[1].current = e.wts[0].current - 1
            }),
            ("stale sample", |e| {
                e.now = e.wts[1].current + LIVE_TICKS + 1
            }),
            ("sample from the future", |e| e.now = e.wts[1].current - 1),
            ("remote LSA logon", |e| e.lsa[0].logon_type = 10),
            ("cached LSA logon", |e| e.lsa[1].logon_type = 11),
            ("LSA session differs", |e| e.lsa[1].session = 3),
            ("LSA SID differs", |e| e.lsa[0].owner = false),
            ("LSA record changed", |e| e.lsa[1].logon_time = 151),
            ("LSA names changed between samples", |e| {
                e.lsa[1].user = "Other".to_owned()
            }),
            ("LSA logon far from WTS logon", |e| {
                e.lsa[0].logon_time = 100 + LOGON_SKEW_TICKS + 1;
                e.lsa[1].logon_time = 100 + LOGON_SKEW_TICKS + 1;
            }),
            ("zero LSA logon time", |e| {
                e.lsa[0].logon_time = 0;
                e.lsa[1].logon_time = 0;
            }),
        ];
        for (name, mutate) in cases {
            let mut evidence = console();
            mutate(&mut evidence);
            assert_eq!(
                console_decision(&evidence),
                Err(NativeRefusal::OwnerActivityUnverified),
                "{name}"
            );
        }
    }

    #[test]
    fn guid_spellings_parse_to_exact_bytes() {
        assert_eq!(
            parse_guid("0123abcd-4567-89AB-cdef-0123456789ab"),
            Some([
                0x01, 0x23, 0xab, 0xcd, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
                0x89, 0xab
            ])
        );
        for bad in [
            "0123abcd-4567-89ab-cdef-0123456789a",
            "0123abcd-4567-89ab-cdef-0123456789abc",
            "0123abcd_4567-89ab-cdef-0123456789ab",
            "0123abcd-4567-89ab-cdef-0123456789ag",
            "0123abcd-4567-89ab-cdef--123456789ab",
            "{123abcd-4567-89ab-cdef-0123456789ab}",
        ] {
            assert_eq!(parse_guid(bad), None, "{bad}");
        }
    }

    /// A private file owned by the current user, as the owner enrolls it.
    fn private_file(dir: &Path, name: &str, bytes: &[u8]) -> String {
        use std::io::Write;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        };
        let user = current_user().unwrap();
        let mut text = null_mut();
        assert_ne!(
            unsafe { ConvertSidToStringSidW(user_sid(&user), &mut text) },
            0
        );
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        let me = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) }).unwrap();
        unsafe { LocalFree(text.cast()) };
        let mut descriptor = null_mut();
        let sddl = wide(format!("O:{me}D:P(A;;FA;;;{me})"));
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                )
            },
            0
        );
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let path = dir.join(name);
        let handle = unsafe {
            CreateFileW(
                wide(&path).as_ptr(),
                FILE_GENERIC_WRITE,
                0,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        unsafe { LocalFree(descriptor) };
        let mut file = File::from(unsafe { owned(handle) }.unwrap());
        file.write_all(bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn identity_is_refused_before_any_root_is_pinned_or_output_claimed() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let evidence = root.path().join("evidence");
        std::fs::create_dir(&evidence).unwrap();
        let pin = r#"{"volume":1,"file":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}"#;
        // Every root is unusable, so only an identity check that runs first
        // can produce the identity refusal.
        for (index, owner) in ["S-1-5-21-1-2-3-1001", "S-1-5-018"].iter().enumerate() {
            let enrollment = format!(
                r#"{{"owner_sid":"{owner}","stock_bundle":"Q:\\missing\\qemu","trial_root":"T:\\missing","trial_volume":{pin},"trial_volume_guid":"none","trial_identity":{pin},"outer_root":"D:\\","backing_file":"D:\\missing\\trial.vhdx","backing_identity":{pin}}}"#
            );
            let file = private_file(
                root.path(),
                &format!("enrollment-{index}.json"),
                enrollment.as_bytes(),
            );
            assert_eq!(
                preflight(&file, evidence.to_str().unwrap()).err(),
                Some(NativeRefusal::IdentityUnverified),
                "{owner}"
            );
            assert_eq!(std::fs::read_dir(&evidence).unwrap().count(), 0);
        }
    }
}

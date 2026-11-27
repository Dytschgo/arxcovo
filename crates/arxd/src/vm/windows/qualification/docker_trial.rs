//! Disposable Docker qualification. The native witness adapter is a separate
//! boundary. Only the deterministic tests can currently supply its peer.
use super::*;
use crate::vm::{
    Id,
    paths::Artifact,
    qemu_args::BootRecipe,
    qualification_record::{
        COLLECTOR, CONTRACT, Class, Diagnostic, DockerDependencies, Hash, Instant2, MAX_TRIAL_MS,
        Observation, Origin, Record, RunError, Stage, StageResult, canonical, clocks_agree, digest,
        produce_bundle, stages,
    },
};
use arx_guest::compose::{COMPOSE_PROTOCOL_VERSION, ComposeResponse};
use arx_guest::docker_trial::{BootMode, Challenge, TrialRequest, TrialResponse};
use std::io::Write;

mod origin;
pub(super) use origin::profile_pin as origin_profile_pin;
pub(in crate::vm::windows) mod witness;
use witness::{Listener, ObservedRoute, ReceiverEnrollment, RouteSample, Target};

const REFUSED: NativeRefusal = NativeRefusal::PathCustodyUnverified;
// Simultaneous worst-case allocation of the fixture, original and both
// independent verification pairs, including qcow2 metadata, VARS and seed.
const PEAK_BYTES: u64 = 4 * (33 << 30) + (1 << 30);
const CHARGE_FILE: &str = "docker-trial.charge";
const RELEASE_BUDGET_MS: u64 = 30_000;

#[derive(Deserialize)]
struct EnrollmentClass {
    class: Class,
}

pub(super) fn require_enrollment(bytes: &[u8]) -> Result<(), NativeRefusal> {
    match serde_json::from_slice::<EnrollmentClass>(bytes) {
        Ok(EnrollmentClass {
            class: Class::Docker,
        }) => Ok(()),
        _ => Err(NativeRefusal::QualificationMissing),
    }
}

pub(super) fn preflight(
    enrollment: &str,
    evidence: &str,
) -> Result<std::convert::Infallible, NativeRefusal> {
    let user = current_user().map_err(|_| NativeRefusal::IdentityUnverified)?;
    let path = local(enrollment)?;
    let evidence = local(evidence)?;
    let source = PathPin::open(&path, false, false)?;
    if path.overlaps(&evidence) || !sink::held_private(&source.leaf, user_sid(&user)) {
        return Err(REFUSED);
    }
    let bytes = read_bounded(&source.leaf, MAX_ENROLLMENT_BYTES)
        .map_err(|_| NativeRefusal::QualificationMissing)?;
    let enrollment = static_compose::enrollment::Enrollment::parse(&bytes)?;
    let profile = enrollment.runtime_profile.file();
    let profile = static_compose::hold_file(profile.path, profile.pin, MAX_ENROLLMENT_BYTES)?;
    if profile.path.overlaps(&evidence) || !sink::held_private(&profile.leaf, user_sid(&user)) {
        return Err(REFUSED);
    }
    let config = crate::config::VmRuntimeProfile::parse(&static_compose::held_bytes(
        &profile,
        MAX_ENROLLMENT_BYTES,
    )?)
    .map_err(|_| NativeRefusal::QualificationMissing)?;
    let selected = config
        .test_qualification()
        .filter(|_| config.enabled())
        .ok_or(NativeRefusal::QualificationMissing)?;
    if [selected.dossier(), selected.enrollment()]
        .into_iter()
        .any(|path| path.overlaps(&evidence))
    {
        return Err(REFUSED);
    }
    let mode = CollectMode::QualifierReview;
    let m4 = foreground::QualifierTestInputs::open(&selected)?;
    let scope = m4.held.scope.as_ref().ok_or(REFUSED)?;
    if [
        &scope.production.root.path,
        &scope.recipe.root.path,
        &m4.held.trial.path,
    ]
    .into_iter()
    .any(|path| path.overlaps(&evidence))
    {
        return Err(REFUSED);
    }
    let recipe = enrollment.recipe.hold(&m4.held, &m4.recipes, [1; 16])?;
    let inputs = enrollment.hold_inputs(&evidence, mode, &m4.held, &recipe)?;
    let targets = inputs
        .bound_targets(selected.selection().seal_sha256)?
        .addresses();
    let (_dependencies, watch) = inputs.measure_live(&targets)?;
    m4.require_current()?;
    // A complete input check grants no native effects or evidence claim.
    // Require the independent receiver and boot-bound HTTPS adapters first.
    watch.require_current()?;
    Err(NativeRefusal::NetworkFenceUnverified)
}

#[cfg(test)]
#[test]
fn only_docker_enrollment_reaches_the_docker_gate() {
    for bad in [
        br#"{"owner_sid":"example"}"#.as_slice(),
        br#"{"class":"test"}"#,
        br#"{"class":"static_origin"}"#,
        br#"{"class":"docker","class":"test"}"#,
    ] {
        assert_eq!(
            require_enrollment(bad),
            Err(NativeRefusal::QualificationMissing)
        );
        assert!(serde_json::from_slice::<Enrollment>(bad).is_err());
    }
    let docker = br#"{"class":"docker","owner_sid":"example"}"#;
    assert!(require_enrollment(docker).is_ok());
    assert!(static_compose::enrollment::Enrollment::parse(docker).is_err());
    assert!(serde_json::from_slice::<Enrollment>(docker).is_err());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(super) enum Phase {
    PublicFixture,
    Install,
    Detached,
    Endpoint,
}

impl Phase {
    pub(super) fn plan<'a>(
        self,
        recipe: &'a BootRecipe<'a>,
        medium: Option<&'a Artifact>,
    ) -> Result<CommandPlan<'a>, NativeRefusal> {
        let plan = match (self, medium) {
            (Self::Install, Some(medium)) => recipe.static_compose_installer(medium),
            (Self::Detached, None) => recipe.docker_trial_boot(BootMode::Detached),
            (Self::Endpoint, None) => recipe.docker_trial_boot(BootMode::Endpoint),
            (Self::PublicFixture, None) => recipe.plan(),
            _ => return Err(REFUSED),
        }
        .map_err(|_| REFUSED)?;
        let expected = match self {
            Self::PublicFixture | Self::Endpoint => PlanKind::PublicEgressBoot,
            Self::Install => PlanKind::StaticComposeInstaller,
            Self::Detached => PlanKind::StaticComposeDetached,
        };
        (plan.kind() == expected).then_some(plan).ok_or(REFUSED)
    }

    pub(super) fn leaves(self) -> [&'static str; 2] {
        match self {
            Self::PublicFixture => ["fixture.qcow2", "fixture.fd"],
            Self::Install => ["install.qcow2", "install.fd"],
            Self::Detached => ["detached.qcow2", "detached.fd"],
            Self::Endpoint => ["endpoint.qcow2", "endpoint.fd"],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Scope {
    pub(super) run: Id,
    evidence: Hash,
    workspace: FileIdentity,
}

pub(super) struct Custody {
    scope: Scope,
    root: PathPin,
    journal: File,
    pairs: Vec<FileIdentity>,
    dispatched: u16,
    charged: bool,
    release_uncertain: bool,
}

impl Custody {
    pub(super) fn claim(
        root: PathPin,
        evidence: &sink::EvidenceDir,
        owner: PSID,
    ) -> Result<Self, NativeRefusal> {
        if root.path.is_drive_root()
            || evidence.path().overlaps(&root.path)
            || !root.mutable
            || !sink::held_private(&root.leaf, owner)
            || !sink::empty(&root.leaf)?
        {
            return Err(REFUSED);
        }
        let scope = Scope {
            run: evidence.run(),
            evidence: evidence.scope(),
            workspace: root.identity,
        };
        let mut journal =
            sink::create_trial_journal(&root.leaf, CHARGE_FILE).map_err(|_| REFUSED)?;
        journal.write_all(b"ARXDOCKER1").map_err(|_| REFUSED)?;
        journal.write_all(&scope.run).map_err(|_| REFUSED)?;
        journal.write_all(&scope.evidence).map_err(|_| REFUSED)?;
        journal
            .write_all(&PEAK_BYTES.to_le_bytes())
            .map_err(|_| REFUSED)?;
        journal.sync_all().map_err(|_| REFUSED)?;
        Ok(Self {
            scope,
            root,
            journal,
            pairs: Vec::new(),
            dispatched: 0,
            charged: true,
            release_uncertain: false,
        })
    }

    fn begin(&mut self, phase: Phase, peak_available: u64) -> Result<Ticket, NativeRefusal> {
        if !self.charged || peak_available < PEAK_BYTES || phase as u16 != self.dispatched {
            return Err(REFUSED);
        }
        inventory::binding(&self.root)?;
        let generation = u64::from(self.dispatched) + 1;
        self.journal
            .write_all(&[phase as u8])
            .map_err(|_| REFUSED)?;
        self.journal.sync_all().map_err(|_| REFUSED)?;
        self.dispatched += 1;
        self.release_uncertain = true;
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| REFUSED)?;
        Ok(Ticket {
            scope: self.scope,
            phase,
            challenge: Challenge { generation, nonce },
        })
    }

    fn bind(
        &mut self,
        ticket: Ticket,
        staged_scope: Scope,
        plan: &CommandPlan<'_>,
    ) -> Result<(), NativeRefusal> {
        if staged_scope != ticket.scope || ticket.scope != self.scope {
            return Err(REFUSED);
        }
        let mut binding = Vec::new();
        for (role, artifact) in plan.inputs() {
            let path = artifact.path_for(*role).map_err(|_| REFUSED)?;
            let leaf = match role {
                ArtifactRole::StaticDisk => Some(ticket.phase.leaves()[0]),
                ArtifactRole::StaticVars => Some(ticket.phase.leaves()[1]),
                ArtifactRole::UbuntuCidata => Some("install-seed.iso"),
                _ => None,
            };
            let id = artifact.identity().ok_or(REFUSED)?;
            if let Some(leaf) = leaf {
                if self.root.path.join(leaf).ok().as_ref() != Some(path)
                    || id.volume != self.scope.workspace.volume
                    || self.pairs.contains(&id)
                    || binding.contains(&id)
                {
                    return Err(REFUSED);
                }
                binding.push(id);
            } else if self.root.path.overlaps(path) {
                return Err(REFUSED);
            }
        }
        self.pairs.extend(binding);
        Ok(())
    }

    fn settle(
        &mut self,
        release: WorkspaceRelease,
        fresh: impl FnOnce() -> bool,
    ) -> Result<(), NativeRefusal> {
        if release.scope != self.scope || !release.owned_leaves_absent || !release.helpers_joined {
            return Err(REFUSED);
        }
        inventory::binding(&self.root)?;
        let entries = listing(&self.root.leaf).map_err(|_| REFUSED)?;
        if !matches!(entries.as_slice(), [(name, attributes)]
            if name == CHARGE_FILE
                && attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0)
        {
            return Err(REFUSED);
        }
        if !fresh() {
            return Err(REFUSED);
        }
        self.journal.write_all(b"released").map_err(|_| REFUSED)?;
        self.journal.sync_all().map_err(|_| REFUSED)?;
        self.charged = false;
        Ok(())
    }
}

/// A create intent in one disposable root. A peer can only consume it via
/// the collector after the sealed argv has been checked against that root.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Ticket {
    pub(super) scope: Scope,
    pub(super) phase: Phase,
    pub(super) challenge: Challenge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PeerId {
    pub(super) pid: u32,
    pub(super) boot: Id,
}

pub(super) struct Release {
    pub(super) peer: PeerId,
    pub(super) process_exited: bool,
    pub(super) empty_job: bool,
    pub(super) workers_joined: bool,
    pub(super) writers_excluded: bool,
    pub(super) clean_guest_shutdown: bool,
}

impl Release {
    fn clean(&self, peer: PeerId) -> bool {
        self.peer == peer
            && self.process_exited
            && self.empty_job
            && self.workers_joined
            && self.writers_excluded
            && self.clean_guest_shutdown
    }
}

pub(super) struct WorkspaceRelease {
    pub(super) scope: Scope,
    pub(super) owned_leaves_absent: bool,
    pub(super) helpers_joined: bool,
}

/// Metadata observed via the held guest transport, including the actual
/// server process ID. A wire reply alone cannot establish that identity.
pub(super) struct Reply {
    pub(super) peer: PeerId,
    pub(super) frame: Vec<u8>,
}

/// A stage returns the materials it actually prepared. The collector builds
/// argv from those materials while the prepared object still owns the holds.
pub(super) trait Prepared {
    fn scope(&self) -> Scope;
    fn recipe(&self) -> BootRecipe<'_>;
    fn dependency_medium(&self) -> Option<&Artifact>;
}

pub(super) struct CheckedBoot<'a> {
    ticket: Ticket,
    plan: CommandPlan<'a>,
}

impl<'a> CheckedBoot<'a> {
    pub(super) fn ticket(&self) -> Ticket {
        self.ticket
    }

    pub(super) fn argv(&self) -> &CommandPlan<'a> {
        &self.plan
    }
}

pub(super) trait Host {
    type Prepared: Prepared;
    fn clock(&self) -> Instant2;
    fn on_boot(&self) -> Result<(Instant2, Hash), NativeRefusal>;
    fn measure(&mut self) -> Result<DockerDependencies, NativeRefusal>;
    fn baseline(&mut self, stage: Stage) -> Result<Observation, NativeRefusal>;
    fn headroom(&mut self) -> Result<u64, NativeRefusal>;

    // Begin staging only after receiving the ticket. Implementations own
    // staged leaves and the child/job even when start or monitoring fails.
    fn stage(&mut self, ticket: Ticket, cutoff: u64) -> Result<Self::Prepared, NativeRefusal>;
    fn route(
        &mut self,
        boot: &CheckedBoot<'_>,
        peer: Option<PeerId>,
        cutoff: u64,
    ) -> Result<RouteSample, NativeRefusal>;
    fn receiver_targets(&mut self, ticket: Ticket) -> Result<Vec<Target>, NativeRefusal>;
    // Checked create through QMP prelaunch, with guest execution still paused.
    fn prelaunch(
        &mut self,
        boot: &CheckedBoot<'_>,
        cutoff: u64,
        cancel: &Shutdown,
    ) -> Result<PeerId, NativeRefusal>;
    fn cont(&mut self, peer: PeerId, cutoff: u64, cancel: &Shutdown) -> Result<(), NativeRefusal>;
    fn listener(&mut self, peer: PeerId, port: u16, cutoff: u64)
    -> Result<Listener, NativeRefusal>;
    fn drain(&mut self, ticket: Ticket, cutoff: u64) -> Result<Release, NativeRefusal>;
    fn dispose(&mut self, scope: Scope, cutoff: u64) -> Result<WorkspaceRelease, NativeRefusal>;
    fn copies_independent_and_original_unchanged(&mut self) -> bool;
}

pub(super) trait Guest {
    fn exchange(
        &mut self,
        peer: PeerId,
        frame: Vec<u8>,
        cutoff: u64,
    ) -> Result<Reply, NativeRefusal>;
}

pub(super) trait Capture {
    fn fixture(
        &mut self,
        peer: PeerId,
        ticket: Ticket,
        targets: &[Target],
        window: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, NativeRefusal>;
}

pub(super) struct Run<'a, H, G, C> {
    pub(super) host: H,
    pub(super) guest: G,
    pub(super) capture: C,
    pub(super) dependencies: DockerDependencies,
    pub(super) stock: static_compose::StockBootInputs<'a>,
    pub(super) receiver_enrollment: ReceiverEnrollment,
    pub(super) packages: Hash,
    pub(super) custody: Custody,
    pub(super) evidence: &'a sink::EvidenceDir,
    pub(super) cancel: &'a Shutdown,
    pub(super) runtime: &'a tokio::runtime::Runtime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Outcome {
    Complete,
    Incomplete,
}

struct RunningBoot<'a, 'plan> {
    peer: PeerId,
    ticket: Ticket,
    targets: &'a [Target],
    plan: &'a CommandPlan<'plan>,
    started_ms: u64,
    route: Option<ObservedRoute>,
}

impl<H: Host, G: Guest, C: Capture> Run<'_, H, G, C> {
    fn run_boot<T>(
        &mut self,
        phase: Phase,
        cutoff: u64,
        verify: impl FnOnce(&mut H, &mut G, &mut C, RunningBoot<'_, '_>) -> Result<T, NativeRefusal>,
    ) -> Result<(T, PeerId), NativeRefusal> {
        let available = self.host.headroom()?;
        if self.cancel.is_cancelled() || self.host.clock().mono_ms >= cutoff {
            return Err(NativeRefusal::QualificationMissing);
        }
        let ticket = self.custody.begin(phase, available)?;
        let prepared = self.host.stage(ticket, cutoff);
        let mut spawned = None;
        let result = (|| {
            let prepared = prepared.as_ref().map_err(|reason| *reason)?;
            let recipe = prepared.recipe();
            let plan = phase.plan(&recipe, prepared.dependency_medium())?;
            self.custody.bind(ticket, prepared.scope(), &plan)?;
            if matches!(phase, Phase::Install | Phase::Detached) {
                self.stock.require_plan(&plan)?;
            }
            for (role, observed) in plan.inputs() {
                let expected = match role {
                    ArtifactRole::PublicEgressQemu => self.dependencies.executable,
                    ArtifactRole::PublicEgressPolicy => self.dependencies.policy,
                    ArtifactRole::UbuntuInstallIso => self.dependencies.installer,
                    ArtifactRole::ComposeDependencyMedium => self.dependencies.recipe.medium,
                    _ => continue,
                };
                if observed.pin().is_none_or(|pin| pin.sha256 != expected) {
                    return Err(REFUSED);
                }
            }
            if self.cancel.is_cancelled() || self.host.clock().mono_ms >= cutoff {
                return Err(NativeRefusal::QualificationMissing);
            }
            let public = plan.kind() == PlanKind::PublicEgressBoot;
            let targets = if phase == Phase::PublicFixture {
                self.receiver_enrollment
                    .check_targets(self.host.receiver_targets(ticket)?)?
            } else {
                Vec::new()
            };
            let boot = CheckedBoot { ticket, plan };
            if public {
                witness::route(
                    self.host.route(&boot, None, cutoff)?,
                    &boot,
                    None,
                    &self.dependencies,
                )?;
            }
            if self.cancel.is_cancelled() || self.host.clock().mono_ms >= cutoff {
                return Err(NativeRefusal::QualificationMissing);
            }
            let peer = self.host.prelaunch(&boot, cutoff, self.cancel)?;
            spawned = Some(peer);
            if peer.pid == 0 || peer.boot == [0; 16] {
                return Err(NativeRefusal::GuestEndpointUnverified);
            }
            let route = if public {
                Some(witness::route(
                    self.host.route(&boot, Some(peer), cutoff)?,
                    &boot,
                    Some(peer),
                    &self.dependencies,
                )?)
            } else {
                None
            };
            let started_ms = self.host.clock().mono_ms;
            if self.cancel.is_cancelled() || started_ms >= cutoff {
                return Err(NativeRefusal::QualificationMissing);
            }
            self.host.cont(peer, cutoff, self.cancel)?;
            let proof = verify(
                &mut self.host,
                &mut self.guest,
                &mut self.capture,
                RunningBoot {
                    peer,
                    ticket,
                    targets: &targets,
                    plan: boot.argv(),
                    started_ms,
                    route,
                },
            )?;
            // The fixture deliberately exercises drift. The installed endpoint
            // must still match the enrolled route after its HTTPS exchange.
            if phase == Phase::Endpoint {
                witness::route(
                    self.host.route(&boot, Some(peer), cutoff)?,
                    &boot,
                    Some(peer),
                    &self.dependencies,
                )?;
            }
            Ok(proof)
        })();
        let release_start = self.host.clock();
        let release_cutoff = release_start.mono_ms.saturating_add(RELEASE_BUDGET_MS);
        let release = self.host.drain(ticket, release_cutoff);
        let peer = spawned.ok_or(REFUSED)?;
        if !release?.clean(peer)
            || !release_in_time(
                self.cancel,
                release_start,
                cutoff,
                release_cutoff,
                self.host.clock(),
            )
        {
            return Err(REFUSED);
        }
        drop(prepared);
        self.custody.release_uncertain = false;
        result.map(|proof| (proof, peer))
    }

    fn collect(
        &mut self,
        stage: Stage,
        cutoff: u64,
        fixture: &mut Option<PeerId>,
    ) -> Result<Observation, NativeRefusal> {
        let dependencies = self.dependencies.clone();
        let packages = self.packages;
        match stage {
            Stage::Distribution => Ok(Observation::DockerDistribution {
                manifest: dependencies.manifest,
                executable: dependencies.executable,
                receipt: dependencies.receipt,
                loaded_artifacts: dependencies.loaded_artifacts,
            }),
            Stage::PublicIpv4Network => {
                let (receiver_witness, peer) =
                    self.run_boot(Phase::PublicFixture, cutoff, |host, _, capture, boot| {
                        let raw = capture.fixture(
                            boot.peer,
                            boot.ticket,
                            boot.targets,
                            boot.started_ms..cutoff,
                        )?;
                        witness::receivers(&raw, &boot, host.clock().mono_ms)
                    })?;
                *fixture = Some(peer);
                Ok(Observation::DockerPublicIpv4 {
                    policy: dependencies.policy,
                    topology: dependencies.topology,
                    receiver_witness,
                    receiver_controls: true,
                    public_tcp_udp: true,
                    nonpublic_denied: true,
                    ipv6_icmp_denied: true,
                    route_drift_denied: true,
                })
            }
            Stage::DockerRecipe => {
                self.run_boot(Phase::Install, cutoff, |_, _, _, _| Ok(()))?;
                self.run_boot(Phase::Detached, cutoff, |_, guest, _, boot| {
                    health(guest, boot.peer, boot.ticket.challenge, packages, cutoff)
                })?;
                if !self.host.copies_independent_and_original_unchanged() {
                    return Err(REFUSED);
                }
                Ok(Observation::DockerRecipeVerified {
                    recipe: dependencies.recipe,
                    restricted_install: true,
                    detached_health: true,
                    independent_pair: true,
                })
            }
            Stage::InstalledGuestEndpoint => {
                let runtime = self.runtime;
                let cancel = self.cancel;
                self.run_boot(Phase::Endpoint, cutoff, |host, guest, _, boot| {
                    let peer = boot.peer;
                    let challenge = boot.ticket.challenge;
                    if !fixture.is_some_and(|first| first.boot != peer.boot) {
                        return Err(NativeRefusal::GuestEndpointUnverified);
                    }
                    health(guest, peer, challenge, packages, cutoff)?;
                    let provision: TrialResponse = exchange(
                        guest,
                        peer,
                        &TrialRequest::DockerTrialProvision { challenge },
                        cutoff,
                    )?;
                    let ack: ComposeResponse = exchange(
                        guest,
                        peer,
                        &arx_guest::docker_trial::compose_hello(challenge),
                        cutoff,
                    )?;
                    let TrialResponse::DockerTrialProvisioned {
                        challenge: echoed,
                        certificate_der,
                    } = provision
                    else {
                        return Err(NativeRefusal::GuestEndpointUnverified);
                    };
                    if echoed != challenge
                        || !arx_guest::docker_trial::matches_compose_ack(challenge, &ack)
                        || dependencies.origin_profile != origin::profile_pin()
                    {
                        return Err(NativeRefusal::GuestEndpointUnverified);
                    }
                    let port = boot.plan.static_origin_lease().ok_or(REFUSED)?.port();
                    host.listener(peer, port, cutoff)?.check(port, peer.pid)?;
                    let remaining = cutoff.saturating_sub(host.clock().mono_ms);
                    runtime.block_on(origin::probe(
                        port,
                        challenge,
                        &certificate_der,
                        Duration::from_millis(remaining),
                        cancel,
                    ))?;
                    host.listener(peer, port, cutoff)?.check(port, peer.pid)?;
                    Ok(())
                })?;
                if !self.host.copies_independent_and_original_unchanged() {
                    return Err(REFUSED);
                }
                Ok(Observation::DockerInstalledEndpoint {
                    recipe: dependencies.recipe.identity,
                    compose_protocol: COMPOSE_PROTOCOL_VERSION,
                    exact_pipe_peer: true,
                    pinned_origin_tls: true,
                    clean_stop: true,
                })
            }
            _ => self.host.baseline(stage),
        }
    }

    pub(super) fn run(mut self) -> Result<Outcome, RunError> {
        if self.custody.scope.run != self.evidence.run()
            || self.custody.scope.evidence != self.evidence.scope()
            || self.stock.inventory != self.dependencies.stock_inventory
            || self.receiver_enrollment.fixture() != self.dependencies.network_fixture
            || self
                .host
                .measure()
                .map_err(RunError::Native)?
                .key(self.evidence.scope())
                != self.dependencies.key(self.evidence.scope())
        {
            return Err(RunError::Incomplete);
        }
        let issued = self.host.clock();
        let cutoff = MAX_TRIAL_MS;
        let binding = self
            .evidence
            .docker_binding(&self.dependencies, issued)
            .map_err(|_| RunError::Incomplete)?;
        let mut prior = [0; 32];
        let mut rows = Vec::new();
        let mut fixture = None;
        let mut passed = true;
        let mut late = false;
        let mut finished = issued;
        for &stage in stages(Class::Docker) {
            let start = self.host.clock();
            let result = if stage == Stage::Cleanup {
                // Settlement may release the charge only while both the
                // release deadline and the whole trial are still live.
                if !self.custody.release_uncertain
                    && !late
                    && in_time(self.cancel, issued, cutoff, start)
                {
                    let release_cutoff =
                        start.mono_ms.saturating_add(RELEASE_BUDGET_MS).min(cutoff);
                    if let Ok(proof) = self.host.dispose(self.custody.scope, release_cutoff) {
                        let _ = self.custody.settle(proof, || {
                            let now = self.host.clock();
                            let fresh =
                                release_in_time(self.cancel, start, cutoff, release_cutoff, now)
                                    && clocks_agree(issued, now);
                            late |= !fresh;
                            fresh
                        });
                    }
                }
                late |= !in_time(self.cancel, issued, cutoff, self.host.clock());
                StageResult::Passed(Observation::Cleanup {
                    dispatched_effects: self.custody.dispatched,
                    released: !self.custody.charged,
                    charge_retained: self.custody.charged,
                    late,
                })
            } else if passed && in_time(self.cancel, issued, cutoff, start) {
                match self.collect(stage, cutoff, &mut fixture) {
                    Ok(observation) => StageResult::Passed(observation),
                    Err(_) if self.custody.release_uncertain => {
                        StageResult::Failed(Diagnostic::TeardownUncertain)
                    }
                    Err(_) => StageResult::Failed(Diagnostic::ObservationInvalid),
                }
            } else {
                StageResult::Inconclusive(Diagnostic::PrerequisiteMissing)
            };
            finished = self.host.clock();
            late |= !in_time(self.cancel, issued, cutoff, finished);
            passed &= result.passes(stage, Origin::Native) && !late;
            let row = Record {
                contract: CONTRACT,
                collector: COLLECTOR,
                origin: Origin::Native,
                binding,
                stage,
                started_ms: start.mono_ms.min(MAX_TRIAL_MS),
                finished_ms: finished.mono_ms.max(start.mono_ms).min(MAX_TRIAL_MS),
                prior,
                result,
            };
            prior = digest(&canonical(&row)?);
            rows.push(row);
        }
        if passed {
            let bundle = produce_bundle(Origin::Native, binding, &rows, finished.utc)?;
            let host = std::cell::RefCell::new(self.host);
            self.evidence
                .publish_docker_with_clock(
                    &bundle,
                    issued,
                    self.cancel,
                    || {
                        let mut host = host.borrow_mut();
                        let (now, _) = host.on_boot().map_err(|_| SinkError::Invalid)?;
                        if host
                            .measure()
                            .map_err(|_| SinkError::Invalid)?
                            .key(self.evidence.scope())
                            != Some(binding.dependencies)
                        {
                            return Err(SinkError::Invalid);
                        }
                        Ok(sink::DockerPublishSample {
                            dependencies: &self.dependencies,
                            captured_uptime_ms: now.mono_ms,
                            settled: !self.custody.charged,
                        })
                    },
                    || host.borrow().on_boot().map_err(|_| SinkError::Invalid),
                )
                .map_err(|_| RunError::Output)?;
            return Ok(Outcome::Complete);
        }

        let entries: Vec<_> = rows
            .iter()
            .map(|row| Entry {
                stage: &row.stage,
                result: &row.result,
                started_ms: row.started_ms,
                finished_ms: row.finished_ms,
            })
            .collect();
        #[derive(Serialize)]
        struct Summary {
            blocked_at: Option<Stage>,
            unattempted: Vec<Stage>,
            unmeasured: [&'static str; 0],
            dispatched_effects: u16,
            charge_retained: bool,
            late: Option<Diagnostic>,
        }
        let summary = Summary {
            blocked_at: rows
                .iter()
                .find(|row| !row.result.passes(row.stage, Origin::Native))
                .map(|row| row.stage),
            unattempted: rows
                .iter()
                .filter_map(|row| {
                    matches!(
                        row.result,
                        StageResult::Inconclusive(Diagnostic::PrerequisiteMissing)
                    )
                    .then_some(row.stage)
                })
                .collect(),
            unmeasured: [],
            dispatched_effects: self.custody.dispatched,
            charge_retained: self.custody.charged,
            late: late.then_some(if self.cancel.is_cancelled() {
                Diagnostic::Cancelled
            } else {
                Diagnostic::Expired
            }),
        };
        let unwritten = self
            .evidence
            .publish_docker_diagnostics(&self.dependencies, issued, &entries, &summary)
            .map_err(|_| RunError::Output)?;
        if !unwritten.is_empty() {
            return Err(RunError::Output);
        }
        Ok(Outcome::Incomplete)
    }
}

fn in_time(cancel: &Shutdown, start: Instant2, cutoff: u64, now: Instant2) -> bool {
    !cancel.is_cancelled() && now.mono_ms < cutoff && clocks_agree(start, now)
}

fn release_in_time(
    cancel: &Shutdown,
    start: Instant2,
    trial_cutoff: u64,
    release_cutoff: u64,
    now: Instant2,
) -> bool {
    in_time(cancel, start, trial_cutoff, now) && now.mono_ms < release_cutoff
}

fn health(
    guest: &mut impl Guest,
    peer: PeerId,
    challenge: Challenge,
    packages: Hash,
    cutoff: u64,
) -> Result<(), NativeRefusal> {
    let reply: TrialResponse = exchange(
        guest,
        peer,
        &TrialRequest::DockerTrialHealth { challenge },
        cutoff,
    )?;
    if reply
        != (TrialResponse::DockerTrialHealthy {
            challenge,
            packages,
        })
    {
        return Err(NativeRefusal::GuestEndpointUnverified);
    }
    Ok(())
}

fn exchange<Req: arx_guest::protocol::Message, Rep: arx_guest::protocol::Message>(
    guest: &mut impl Guest,
    peer: PeerId,
    request: &Req,
    cutoff: u64,
) -> Result<Rep, NativeRefusal> {
    let refused = NativeRefusal::GuestEndpointUnverified;
    let mut frame = Vec::new();
    arx_guest::write_frame(&mut frame, request).map_err(|_| refused)?;
    let reply = guest.exchange(peer, frame, cutoff)?;
    let mut bytes = reply.frame.as_slice();
    if reply.peer != peer || bytes.len() > arx_guest::MAX_FRAME + 4 {
        return Err(refused);
    }
    let response = arx_guest::read_frame(&mut bytes)
        .map_err(|_| refused)?
        .ok_or(refused)?;
    if !bytes.is_empty() {
        return Err(refused);
    }
    Ok(response)
}

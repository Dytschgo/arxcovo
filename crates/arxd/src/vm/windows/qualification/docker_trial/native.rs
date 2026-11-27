//! Windows x64 execution for the disposable, journaled Docker trial.
use super::*;
use crate::vm::{
    paths::ArtifactPin,
    qemu_args::{BootPurpose, StaticNetwork, qualifier_create},
    qemu_img::inspect_header,
    qualification_record::BootAnchor,
    runtime::{InstallerBudget, InstallerPhase, InstallerRecipe},
    static_web::OriginPortLease,
};
use std::io::{Read, Seek, SeekFrom};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::NamedPipeClient,
    time::Instant as Deadline,
};

const DISK_BYTES: u64 = 32 << 30;
const DISK_FILE_MAX: u64 = DISK_BYTES + GIB;
const FIRMWARE_MAX: u64 = 16 << 20;
const NETWORK: NativeRefusal = NativeRefusal::NetworkFenceUnverified;
const ENDPOINT: NativeRefusal = NativeRefusal::GuestEndpointUnverified;
const JOB: NativeRefusal = NativeRefusal::JobUnverified;
const QMP: NativeRefusal = NativeRefusal::QmpUnverified;

pub(super) fn hold_workspace(
    m4: &foreground::QualifierTestInputs,
    enrollment: &static_compose::enrollment::TrialWorkspace,
) -> Result<(PathPin, PathPin), NativeRefusal> {
    let storage = &m4.held.scope.as_ref().ok_or(REFUSED)?.recipe;
    if enrollment.path == storage.root.path || !storage.root.path.contains(&enrollment.path) {
        return Err(REFUSED);
    }
    let hold = || {
        let root = capacity::walk(
            &capacity::MountManager,
            &storage.volume,
            &enrollment.path,
            true,
            true,
        )?;
        if !enrollment.identity.matches(root.identity)
            || !sink::held_private(&root.leaf, m4.held.token.account.0)
            || !sink::empty(&root.leaf)?
        {
            return Err(REFUSED);
        }
        Ok(root)
    };
    Ok((hold()?, hold()?))
}

pub(super) fn preclaim(
    m4: &foreground::QualifierTestInputs,
    root: &PathPin,
    cwd: &LocalPath,
    inputs: &static_compose::HeldDockerInputs<'_>,
    owner: PSID,
) -> Result<(), NativeRefusal> {
    let cwd = PathPin::open(cwd, true, false)?;
    for bundle in [&inputs.stock, &inputs.patched.bundle] {
        if bundle.root.path.overlaps(&root.path) {
            return Err(REFUSED);
        }
        inventory::validate_launch_namespace(bundle, &cwd, owner, inventory::safe_dll_search())?;
    }
    reserves(m4, root, 0)?;
    Ok(())
}

fn reserves(
    m4: &foreground::QualifierTestInputs,
    root: &PathPin,
    allocated: u64,
) -> Result<Instant, NativeRefusal> {
    let captured = Instant::now();
    let held = &m4.held;
    let scope = held.scope.as_ref().ok_or(REFUSED)?;
    scope.validate_live()?;
    inventory::binding(root)?;
    if !sink::held_private(&root.leaf, held.token.account.0) {
        return Err(REFUSED);
    }
    let free = capacity::held_free(&root.leaf)?;
    let outer = capacity::held_free(&held.volumes[1].root)?;
    if free.total != scope.recipe.limits.inner_total_bytes
        || PEAK_BYTES > scope.recipe.limits.disk_budget_bytes
        || allocated > PEAK_BYTES
        || free.caller.min(free.total_free) < PEAK_BYTES - allocated
        || outer.caller.min(outer.total_free) < OUTER_FLOOR
        || !capacity::still_bound(&capacity::MountManager, &held.volumes[1])
        || !scope.aggregate_reservation_within_cap(&m4.limits, 0)
    {
        return Err(NativeRefusal::DiskCeilingUnverified);
    }
    let (cores, free_ram) = host_resources()?;
    let effective_cores = super::super::paused::qualifier_cpu_scope()
        .map_err(|_| NativeRefusal::ReservesUnverified)?;
    if cores != effective_cores
        || cores < 2
        || free_ram < RAM_RESERVE + 7 * GIB
        || !hypervisor_present()?
    {
        return Err(NativeRefusal::ReservesUnverified);
    }
    if !held.current_console_idle()? {
        return Err(NativeRefusal::OwnerActivityUnverified);
    }
    Ok(captured)
}

pub(super) struct Assets<'a> {
    stock: [(ArtifactRole, &'a PathPin); 3],
    patched: [PathPin; 3],
    patched_vars: PathPin,
    helper: PathPin,
}

impl<'a> Assets<'a> {
    pub(super) fn hold(
        inputs: &static_compose::HeldDockerInputs<'_>,
        stock: &'a static_compose::StockBootInputs<'_>,
    ) -> Result<Self, NativeRefusal> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let patched = &inputs.patched;
        let image = |leaf, expected: ArtifactPin| {
            let held = launch::stock_asset(&patched.bundle, leaf, expected.size, deadline)?;
            (held.verified_content == Some(expected))
                .then_some(held)
                .ok_or(NativeRefusal::DistributionUnverified)
        };
        let data = patched
            .bundle
            .root
            .path
            .join("share")
            .map_err(|_| REFUSED)?;
        let data = PathPin::open(&data, true, false)?;
        let code = image("share/edk2-x86_64-code.fd", patched.review.firmware_code)?;
        let vars = image("share/edk2-i386-vars.fd", patched.review.firmware_vars)?;
        // Endpoint boots retain the installer's VARS. Their paired firmware
        // must be byte-identical in both reviewed bundles.
        let [stock_code, stock_vars] = &inputs.recipe.firmware;
        if code.verified_content != stock_code.verified_content
            || vars.verified_content != stock_vars.verified_content
        {
            return Err(NativeRefusal::DistributionUnverified);
        }
        let helper = launch::stock_asset(&inputs.stock, "qemu-img.exe", 32 << 20, deadline)?;
        Ok(Self {
            stock: stock.roles(),
            patched: [
                image("qemu-system-x86_64.exe", patched.review.qemu)?,
                data,
                code,
            ],
            patched_vars: vars,
            helper,
        })
    }

    fn loader(&self, phase: Phase) -> [(ArtifactRole, &PathPin); 3] {
        use ArtifactRole::*;
        if matches!(phase, Phase::PublicFixture | Phase::Endpoint) {
            let [image, data, firmware] = &self.patched;
            [
                (PublicEgressQemu, image),
                (Data, data),
                (FirmwareCode, firmware),
            ]
        } else {
            self.stock
        }
    }
}

pub(super) struct HostInputs<'a> {
    pub m4: &'a foreground::QualifierTestInputs,
    pub source: &'a PathPin,
    pub inputs: &'a static_compose::HeldDockerInputs<'a>,
    pub assets: &'a Assets<'a>,
    pub root: &'a PathPin,
    pub journal: &'a File,
    pub scope: Scope,
    pub cwd: LocalPath,
    pub targets: [std::net::Ipv4Addr; 2],
    pub receivers: &'a ReceiverEnrollment,
    pub watch: static_compose::route_watch::RouteWatch,
    pub topology: Hash,
    pub anchor: BootAnchor,
    pub runtime: &'a tokio::runtime::Runtime,
    pub cancel: &'a Shutdown,
}

impl<'a> HostInputs<'a> {
    fn by(&self, cutoff: u64) -> Deadline {
        let remaining = self
            .anchor
            .origin_mono_ms
            .saturating_add(cutoff)
            .saturating_sub(boot::uptime());
        Deadline::now() + Duration::from_millis(remaining)
    }

    fn sample_route(
        &self,
    ) -> Result<(crate::vm::public_egress::RouteObservation, Hash), NativeRefusal> {
        let sample = self
            .watch
            .observe(&self.inputs.parsed_policy, &self.targets)?;
        if sample.1 != self.topology {
            self.watch.retire();
            return Err(NETWORK);
        }
        Ok(sample)
    }

    fn live(&self, cutoff: u64, require_route: bool) -> Result<Instant, NativeRefusal> {
        if self.cancel.is_cancelled()
            || boot::uptime() >= self.anchor.origin_mono_ms.saturating_add(cutoff)
        {
            return Err(NativeRefusal::QualificationMissing);
        }
        inventory::binding(self.source)?;
        let allocated = allocated_under(&self.root.path, self.root.identity.volume, 10)?;
        let captured = reserves(self.m4, self.root, allocated)?;
        if require_route {
            self.sample_route()?;
        }
        Ok(captured)
    }

    fn launch(
        &self,
        ticket: Ticket,
        plan: CommandPlan<'_>,
        pins: Vec<PathPin>,
        installer: Option<InstallerStage>,
        cutoff: u64,
    ) -> Result<(CheckedLaunchCore<'a>, BootIo), NativeRefusal> {
        if ticket.scope != self.scope {
            return Err(REFUSED);
        }
        let bundle = if plan.kind() == PlanKind::PublicEgressBoot {
            &self.inputs.patched.bundle
        } else {
            &self.inputs.stock
        };
        let workspace = WorkspaceCustody::new(
            ticket.scope.run,
            ticket.challenge.generation,
            self.root,
            self.journal,
        )?;
        let launch = LaunchInputs {
            plan,
            cwd: &self.cwd,
            workspace,
            bundle,
            memory: recipe::TEST_JOB_MEMORY,
            cpu_rate: JOB_CPU_RATE,
            installer,
            finalization_slots: None,
            pins,
        };
        let (core, io, ()) = CheckedLaunchCore::hold_checked_for(
            &self.m4.held.token,
            launch,
            |_| Ok((self.live(cutoff, true)?, ())),
            || {
                (!self.cancel.is_cancelled()
                    && boot::uptime() < self.anchor.origin_mono_ms.saturating_add(cutoff))
                .then_some(())
                .ok_or(JOB)
            },
            true,
        )
        .map_err(|failure| failure.reason)?;
        Ok((core, io))
    }
}

pub(super) struct PhaseHost<'a> {
    env: HostInputs<'a>,
    active: Option<Active<'a>>,
    leaves: Vec<(&'static str, FileIdentity)>,
    staged: Option<Staged>,
    installed: Option<[PathPin; 2]>,
}

struct Staged {
    ticket: Ticket,
    serial: SerialName,
    guards: Vec<File>,
    seed: Option<PathPin>,
}

struct Active<'a> {
    ticket: Ticket,
    peer: PeerId,
    core: CheckedLaunchCore<'a>,
    io: Option<BootIo>,
    qmp: Option<PipeQmp>,
    pipe: Option<NamedPipeClient>,
    serial: SerialName,
    origin_port: Option<u16>,
    installer: Option<InstallerStage>,
    guards: Vec<File>,
    continued: bool,
    installed: bool,
}

pub(super) struct Media {
    scope: Scope,
    loader: [Artifact; 3],
    vars: Artifact,
    disk: Artifact,
    header: Vec<u8>,
    serial: SerialName,
    origin: Option<(OriginPortLease, Artifact)>,
    install: Option<[Artifact; 5]>,
}

impl Prepared for Media {
    fn scope(&self) -> Scope {
        self.scope
    }

    fn recipe(&self) -> BootRecipe<'_> {
        let [image, data, firmware] = &self.loader;
        let boot = match (&self.install, &self.origin) {
            (Some([iso, cidata, kernel, initrd, _]), _) => BootPurpose::UbuntuInstall {
                iso,
                cidata,
                kernel,
                initrd,
            },
            (_, Some((lease, policy))) => {
                BootPurpose::InstalledStaticWeb(lease, StaticNetwork::PublicEgress(policy))
            }
            _ => BootPurpose::DetachedStaticCompose,
        };
        BootRecipe {
            image,
            data,
            firmware,
            vars: &self.vars,
            disk: &self.disk,
            disk_header: &self.header,
            backing: None,
            backing_header: None,
            serial: &self.serial,
            boot,
            ram_mib: 4096,
            vcpus: 2,
        }
    }

    fn dependency_medium(&self) -> Option<&Artifact> {
        self.install.as_ref().map(|media| &media[4])
    }
}

impl<'a> PhaseHost<'a> {
    pub(super) fn new(env: HostInputs<'a>) -> Self {
        Self {
            env,
            active: None,
            leaves: Vec::new(),
            staged: None,
            installed: None,
        }
    }

    fn create_leaf(&mut self, name: &'static str) -> Result<PathPin, NativeRefusal> {
        let path = self.env.root.path.join(name).map_err(|_| REFUSED)?;
        let pin = PathPin::create(&path)?;
        self.leaves.push((name, pin.identity));
        if pin.identity.volume != self.env.root.identity.volume {
            return Err(REFUSED);
        }
        Ok(pin)
    }

    fn copy(
        &mut self,
        name: &'static str,
        source: &PathPin,
        limit: u64,
        cutoff: u64,
    ) -> Result<(PathPin, File), NativeRefusal> {
        inventory::binding(source)?;
        let expected = source.verified_content.ok_or(REFUSED)?;
        if !(1..=limit).contains(&expected.size) {
            return Err(REFUSED);
        }
        let staged = self.create_leaf(name)?;
        let mut input = &source.leaf;
        input.seek(SeekFrom::Start(0)).map_err(|_| REFUSED)?;
        let deadline = self.env.by(cutoff).into_std();
        let cancel = || self.env.cancel.is_cancelled();
        copy_exact(input, &staged.leaf, expected.size, deadline, &cancel)?;
        staged.leaf.sync_all().map_err(|_| REFUSED)?;
        let (pin, guard) = guarded(reopen(staged, true)?)?;
        if launch::hash_held(&guard, limit, deadline, &cancel)? != expected {
            return Err(REFUSED);
        }
        Ok((pin, guard))
    }

    fn blank_disk(
        &mut self,
        ticket: Ticket,
        cutoff: u64,
    ) -> Result<(PathPin, File), NativeRefusal> {
        let staged = reopen(self.create_leaf(ticket.phase.leaves()[0])?, true)?;
        let image = launch::artifact(ArtifactRole::QemuImg, &self.env.assets.helper)?;
        let disk = launch::artifact(ArtifactRole::Staging, &staged)?;
        let plan =
            qualifier_create(&image, &disk, None, DISK_BYTES, PEAK_BYTES).map_err(|_| REFUSED)?;
        let pins = vec![duplicate(&self.env.assets.helper)?, duplicate(&staged)?];
        let env = &self.env;
        let (mut launch, io) = env.launch(ticket, plan, pins, None, cutoff)?;
        let (result, (release, exclusion)) = env.runtime.block_on(
            launch.helper_held(
                io,
                env.cancel,
                env.by(cutoff)
                    .min(Deadline::now() + Duration::from_secs(30)),
                || env.live(cutoff, true),
                || true,
            ),
        );
        if release != StopDisposition::CleanRelease {
            return Err(JOB);
        }
        result?;
        // The helper returns the held disk read guard acquired after its Job
        // and workers settled. Keep it through the installer resume edge.
        let (_, mut guards) = exclusion.ok_or(REFUSED)?;
        let guard = guards.pop().filter(|_| guards.is_empty()).ok_or(REFUSED)?;
        Ok((staged, guard))
    }

    fn installed_pair(&self) -> Result<[PathPin; 2], NativeRefusal> {
        let [disk, vars] = Phase::Install.leaves().map(|name| {
            let (_, id) = self
                .leaves
                .iter()
                .find(|(leaf, _)| *leaf == name)
                .ok_or(REFUSED)?;
            let path = self.env.root.path.join(name).map_err(|_| REFUSED)?;
            let pin = PathPin::open(&path, false, false)?;
            (pin.identity == *id).then_some(pin).ok_or(REFUSED)
        });
        let pair = [disk?, vars?];
        if inspect_header(&launch::header(&pair[0])?, None) != Ok(DISK_BYTES) {
            return Err(REFUSED);
        }
        Ok(pair)
    }
}

fn copy_exact(
    mut input: impl Read,
    mut output: impl Write,
    size: u64,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), NativeRefusal> {
    let mut work = vec![0; 256 << 10];
    let mut remaining = size;
    while remaining != 0 {
        launch::check_preparation(deadline, cancelled)?;
        let length = remaining.min(work.len() as u64) as usize;
        input.read_exact(&mut work[..length]).map_err(|_| REFUSED)?;
        output.write_all(&work[..length]).map_err(|_| REFUSED)?;
        remaining -= length as u64;
    }
    launch::check_preparation(deadline, cancelled)?;
    if input.read(&mut [0]).map_err(|_| REFUSED)? != 0 {
        return Err(REFUSED);
    }
    Ok(())
}

fn guarded(pin: PathPin) -> Result<(PathPin, File), NativeRefusal> {
    let guard = pin.reopen_without_writer().map_err(|_| REFUSED)?;
    Ok((pin, guard))
}

fn reopen(created: PathPin, mutable: bool) -> Result<PathPin, NativeRefusal> {
    let id = created.identity;
    let path = created.path.clone();
    drop(created);
    let held = PathPin::open(&path, false, mutable)?;
    (held.identity == id).then_some(held).ok_or(REFUSED)
}

fn duplicate(pin: &PathPin) -> Result<PathPin, NativeRefusal> {
    let duplicate = |file: &File| file.try_clone().map_err(|_| REFUSED);
    Ok(PathPin {
        _ancestors: pin
            ._ancestors
            .iter()
            .map(duplicate)
            .collect::<Result<_, _>>()?,
        leaf: duplicate(&pin.leaf)?,
        identity: pin.identity,
        allocated: pin.allocated,
        path: pin.path.clone(),
        mutable: pin.mutable,
        verified_content: pin.verified_content,
    })
}

fn prove_before_guest(
    active: &Active<'_>,
    env: &HostInputs<'_>,
    cutoff: u64,
) -> Result<(), NativeRefusal> {
    active
        .core
        .check_running(|| env.live(cutoff, true), false)?;
    if let Some(port) = active.origin_port {
        witness::listener(&active.core.child, port)?.check(port, active.peer.pid)?;
    }
    Ok(())
}

impl<'a> Host for PhaseHost<'a> {
    type Prepared = Media;

    fn clock(&self) -> Instant2 {
        let now = boot::now();
        Instant2 {
            mono_ms: now.mono_ms.saturating_sub(self.env.anchor.origin_mono_ms),
            ..now
        }
    }

    fn on_boot(&self) -> Result<(Instant2, Hash), NativeRefusal> {
        self.env.watch.require_current()?;
        Ok((boot::now(), boot::marker()?))
    }

    fn measure(&mut self) -> Result<DockerDependencies, NativeRefusal> {
        self.env.m4.require_current()?;
        inventory::binding(self.env.source)?;
        let (route, topology) = self.env.sample_route()?;
        let measured = self.env.inputs.measure(&route, topology)?;
        self.env.watch.require_current()?;
        Ok(measured)
    }

    fn baseline(&mut self, stage: Stage) -> Result<Observation, NativeRefusal> {
        self.env.live(MAX_TRIAL_MS, true)?;
        match stage {
            Stage::Inputs => Ok(Observation::InputPins {
                held_roots: crate::vm::qualification_record::HELD_ROOTS,
                enrollment_private: true,
                evidence_private: true,
                trial_root_private: true,
                evidence_claimed: true,
            }),
            Stage::HostBaseline => Ok(Observation::Host {
                windows_build: platform()?,
                windows_revision: windows_revision()?,
            }),
            stage if stage.docker_inherits_test() => Ok(Observation::DockerTestBaseline {
                test_seal: self.env.inputs.m4_seal(),
            }),
            _ => Err(NativeRefusal::QualificationMissing),
        }
    }

    fn headroom(&mut self) -> Result<u64, NativeRefusal> {
        self.env.live(MAX_TRIAL_MS, true)?;
        Ok(PEAK_BYTES)
    }

    fn stage(&mut self, ticket: Ticket, cutoff: u64) -> Result<Media, NativeRefusal> {
        use ArtifactRole::*;
        if self.active.is_some() || self.staged.is_some() || self.env.scope != ticket.scope {
            return Err(REFUSED);
        }
        self.env.live(cutoff, true)?;
        self.env.m4.require_current()?;
        let phase = ticket.phase;
        let [disk_name, vars_name] = phase.leaves();
        let ((disk, disk_guard), (vars, vars_guard)) = match phase {
            Phase::PublicFixture => (
                self.copy(
                    disk_name,
                    &duplicate(&self.env.inputs.fixture[0])?,
                    DISK_FILE_MAX,
                    cutoff,
                )?,
                self.copy(
                    vars_name,
                    &duplicate(&self.env.assets.patched_vars)?,
                    FIRMWARE_MAX,
                    cutoff,
                )?,
            ),
            Phase::Install => (
                self.blank_disk(ticket, cutoff)?,
                self.copy(
                    vars_name,
                    &duplicate(&self.env.inputs.recipe.firmware[1])?,
                    FIRMWARE_MAX,
                    cutoff,
                )?,
            ),
            Phase::Detached | Phase::Endpoint => {
                let pair = self.installed.as_mut().ok_or(REFUSED)?;
                if phase == Phase::Detached {
                    let deadline = self.env.by(cutoff).into_std();
                    for (pin, max) in pair.iter_mut().zip([DISK_FILE_MAX, FIRMWARE_MAX]) {
                        launch::hash_at_rest_until(pin, max, deadline, &|| {
                            self.env.cancel.is_cancelled()
                        })?;
                    }
                }
                let (disk, vars) = (duplicate(&pair[0])?, duplicate(&pair[1])?);
                (
                    self.copy(disk_name, &disk, DISK_FILE_MAX, cutoff)?,
                    self.copy(vars_name, &vars, FIRMWARE_MAX, cutoff)?,
                )
            }
        };
        let guards = vec![disk_guard, vars_guard];
        let header = launch::header(&disk)?;
        if inspect_header(&header, None) != Ok(DISK_BYTES) {
            return Err(REFUSED);
        }
        let recipe = self.env.inputs.recipe;
        let mut seed = None;
        let install = if phase == Phase::Install {
            let staged = self.create_leaf("install-seed.iso")?;
            (&staged.leaf)
                .write_all(&recipe.seed)
                .map_err(|_| REFUSED)?;
            staged.leaf.sync_all().map_err(|_| REFUSED)?;
            let mut held = reopen(staged, false)?;
            held.verify_content(
                ArtifactPin {
                    size: recipe.seed.len() as u64,
                    sha256: digest(&recipe.seed),
                },
                40 << 20,
            )?;
            let media = [
                launch::artifact(UbuntuInstallIso, &recipe.installer)?,
                launch::artifact(UbuntuCidata, &held)?,
                launch::artifact(UbuntuKernel, &recipe.kernel)?,
                launch::artifact(UbuntuInitrd, &recipe.initrd)?,
                recipe.dependency_artifact()?,
            ];
            seed = Some(held);
            Some(media)
        } else {
            None
        };
        let origin = if matches!(phase, Phase::PublicFixture | Phase::Endpoint) {
            Some((
                OriginPortLease::acquire().map_err(|_| ENDPOINT)?,
                launch::artifact(PublicEgressPolicy, &self.env.inputs.policy)?,
            ))
        } else {
            None
        };
        let [image, data, firmware] = self
            .env
            .assets
            .loader(phase)
            .map(|(role, pin)| launch::artifact(role, pin));
        let serial = SerialName::generate().map_err(|_| ENDPOINT)?;
        self.staged = Some(Staged {
            ticket,
            serial: serial.clone(),
            guards,
            seed,
        });
        Ok(Media {
            scope: ticket.scope,
            loader: [image?, data?, firmware?],
            vars: launch::artifact(StaticVars, &vars)?,
            disk: launch::artifact(StaticDisk, &disk)?,
            header,
            serial,
            install,
            origin,
        })
    }

    fn route(
        &mut self,
        boot: &CheckedBoot<'_>,
        peer: Option<PeerId>,
        cutoff: u64,
    ) -> Result<RouteSample, NativeRefusal> {
        self.env.live(cutoff, true)?;
        if let Some(peer) = peer {
            let active = self
                .active
                .as_ref()
                .filter(|active| active.peer == peer)
                .ok_or(ENDPOINT)?;
            if active.ticket != boot.ticket() {
                return Err(ENDPOINT);
            }
            prove_before_guest(active, &self.env, cutoff)?;
        }
        let (route, topology) = self.env.sample_route()?;
        let policy = &self.env.inputs.policy;
        Ok(RouteSample {
            ticket: boot.ticket(),
            peer,
            topology,
            route,
            policy_identity: policy.identity,
            policy_bytes: static_compose::held_bytes(
                policy,
                crate::vm::public_egress::MAX_POLICY_BYTES,
            )?,
        })
    }

    fn receiver_targets(&mut self, ticket: Ticket) -> Result<Vec<Target>, NativeRefusal> {
        if ticket.scope != self.env.scope || ticket.phase != Phase::PublicFixture {
            return Err(NETWORK);
        }
        Ok(self.env.receivers.targets().to_vec())
    }

    fn create_suspended(
        &mut self,
        boot: &CheckedBoot<'_>,
        cutoff: u64,
        cancel: &Shutdown,
    ) -> Result<PeerId, NativeRefusal> {
        let ticket = boot.ticket();
        if self.active.is_some() || cancel.is_cancelled() {
            return Err(JOB);
        }
        let staged = self
            .staged
            .take()
            .filter(|staged| staged.ticket == ticket)
            .ok_or(REFUSED)?;
        let env = &self.env;
        let plan = boot.argv();
        let recipe = env.inputs.recipe;
        let mut assets: Vec<_> = env.assets.loader(ticket.phase).map(|(_, pin)| pin).into();
        assets.extend([
            &env.inputs.policy,
            &recipe.installer,
            &recipe.kernel,
            &recipe.initrd,
            &recipe.medium,
        ]);
        assets.extend(staged.seed.as_ref());
        let mut pins = Vec::new();
        for (role, artifact) in plan.inputs() {
            let path = artifact.path_for(*role).map_err(|_| REFUSED)?;
            let pin = if role.writable() {
                PathPin::open(path, false, true)?
            } else {
                duplicate(assets.iter().find(|pin| pin.path == *path).ok_or(REFUSED)?)?
            };
            if artifact.identity() != Some(pin.identity) || artifact.pin() != pin.verified_content {
                return Err(REFUSED);
            }
            pins.push(pin);
        }
        let installer = if ticket.phase == Phase::Install {
            let remaining = env.by(cutoff).saturating_duration_since(Deadline::now());
            Some(
                InstallerBudget::start(InstallerRecipe::Ubuntu)
                    .stage(InstallerPhase::UbuntuInstall, remaining)
                    .ok_or(REFUSED)?,
            )
        } else {
            None
        };
        let mut id = [0; 16];
        getrandom::fill(&mut id).map_err(|_| ENDPOINT)?;
        let origin_port = plan.static_origin_lease().map(OriginPortLease::port);
        let (core, io) = env.launch(ticket, plan.clone(), pins, installer, cutoff)?;
        let peer = PeerId {
            pid: core.child.verify_live().map_err(|_| JOB)?,
            boot: id,
        };
        self.active = Some(Active {
            ticket,
            peer,
            core,
            io: Some(io),
            qmp: None,
            pipe: None,
            serial: staged.serial,
            origin_port,
            installer,
            guards: staged.guards,
            continued: false,
            installed: false,
        });
        Ok(peer)
    }

    fn prelaunch(
        &mut self,
        peer: PeerId,
        origin: Option<std::net::TcpListener>,
        cutoff: u64,
        cancel: &Shutdown,
    ) -> Result<(), NativeRefusal> {
        let env = &self.env;
        let active = self
            .active
            .as_mut()
            .filter(|active| active.peer == peer)
            .ok_or(JOB)?;
        let actual = origin
            .as_ref()
            .map(|socket| socket.local_addr().map_err(|_| ENDPOINT))
            .transpose()?;
        if actual
            != active
                .origin_port
                .map(|port| (std::net::Ipv4Addr::LOCALHOST, port).into())
        {
            return Err(ENDPOINT);
        }
        let (read, write) = active.io.take().ok_or(QMP)?;
        let setup = env
            .by(cutoff)
            .min(Deadline::now() + Duration::from_secs(60));
        active
            .core
            .check_running(|| env.live(cutoff, true), false)?;
        active.core.child.resume_checked_with_handoff(
            cancel,
            Some(setup),
            &mut || env.live(cutoff, true),
            || {
                active.guards.clear();
                drop(origin);
            },
        )?;
        let token = &env.m4.held.token;
        let (qmp, pipe) = env.runtime.block_on(monitor_running(
            async {
                let pipe = connect_serial(
                    &active.serial,
                    &active.core.child,
                    token.account.0,
                    &token.entries(),
                    cancel,
                    setup.min(Deadline::now() + Duration::from_secs(10)),
                )
                .await?;
                let mut qmp = match active.installer {
                    Some(stage) => Qmp::installer_handshake(read, write, cancel, stage).await,
                    None => Qmp::handshake(read, write, cancel).await,
                }
                .map_err(|_| QMP)?;
                qmp.verify_whpx(cancel)
                    .await
                    .map_err(|_| NativeRefusal::WhpxUnverified)?;
                if qmp.status(cancel).await != Ok(crate::vm::qmp::RunStatus::Prelaunch) {
                    return Err(QMP);
                }
                Ok((qmp, pipe))
            },
            || active.core.check_running(|| env.live(cutoff, true), false),
            cancel,
            setup,
            |_| QMP,
        ))?;
        prove_before_guest(active, env, cutoff)?;
        active.qmp = Some(qmp);
        active.pipe = Some(pipe);
        Ok(())
    }

    fn cont(&mut self, peer: PeerId, cutoff: u64, cancel: &Shutdown) -> Result<(), NativeRefusal> {
        let env = &self.env;
        let active = self
            .active
            .as_mut()
            .filter(|active| active.peer == peer && !active.continued)
            .ok_or(QMP)?;
        prove_before_guest(active, env, cutoff)?;
        let qmp = active.qmp.as_mut().ok_or(QMP)?;
        let core = &active.core;
        let port = active.origin_port;
        let deadline = env.by(cutoff);
        let dispatch_by = deadline.min(Deadline::now() + Duration::from_secs(60));
        env.runtime.block_on(monitor_running(
            async {
                core.dispatch_cont(
                    qmp,
                    cancel,
                    &mut || {
                        if let Some(port) = port {
                            witness::listener(&core.child, port)?.check(port, peer.pid)?;
                        }
                        env.live(cutoff, true)
                    },
                    Some(dispatch_by),
                )
                .await
                .map_err(|error| match error {
                    InstallerFailureReason::Native(reason) => reason,
                    _ => QMP,
                })
            },
            || core.check_running(|| env.live(cutoff, true), active.installer.is_some()),
            cancel,
            dispatch_by,
            |_| QMP,
        ))?;
        active.continued = true;
        if let Some(installer) = active.installer {
            env.runtime.block_on(monitor_running(
                async {
                    qmp.observe_installer_shutdown(cancel)
                        .await
                        .map_err(|_| QMP)?;
                    core.child
                        .reap(installer.deadline())
                        .await
                        .map_err(|_| JOB)?;
                    if core.child.exit_code() != Ok(0) {
                        return Err(JOB);
                    }
                    Ok(())
                },
                || core.check_running(|| env.live(cutoff, true), true),
                cancel,
                installer.deadline(),
                |_| QMP,
            ))?;
            active.installed = true;
        } else if active.ticket.phase != Phase::PublicFixture {
            use crate::vm::qmp::PortWatch;
            env.runtime.block_on(monitor_running(
                async {
                    let mut watch = PortWatch::default();
                    while !qmp
                        .port_change(crate::vm::qemu_args::GUEST_PORT_ID, &mut watch)
                        .await
                        .map_err(|_| ENDPOINT)?
                    {}
                    Ok(())
                },
                || core.check_running(|| env.live(cutoff, true), false),
                cancel,
                deadline.min(Deadline::now() + crate::vm::guest::GUEST_READY_BUDGET),
                |_| ENDPOINT,
            ))?;
        }
        Ok(())
    }

    fn listener(
        &mut self,
        peer: PeerId,
        port: u16,
        cutoff: u64,
    ) -> Result<Listener, NativeRefusal> {
        let active = self
            .active
            .as_ref()
            .filter(|active| active.peer == peer && active.origin_port == Some(port))
            .ok_or(ENDPOINT)?;
        self.env.live(cutoff, true)?;
        witness::listener(&active.core.child, port)
    }

    fn fixture_current(&mut self, peer: PeerId, cutoff: u64) -> Result<(), NativeRefusal> {
        let active = self
            .active
            .as_ref()
            .filter(|active| {
                active.peer == peer
                    && active.ticket.phase == Phase::PublicFixture
                    && active.continued
            })
            .ok_or(NETWORK)?;
        active
            .core
            .check_running(|| self.env.live(cutoff, false), false)
    }

    fn after_fixture(&mut self, cutoff: u64) -> Result<(), NativeRefusal> {
        if self.active.is_some() || self.env.watch.require_current().is_ok() {
            return Err(NETWORK);
        }
        self.env.live(cutoff, false)?;
        // The sole handoff follows verified signed drift and fixture release.
        self.env.watch =
            static_compose::route_watch::RouteWatch::arm(&self.env.inputs.parsed_policy)?;
        self.env.sample_route()?;
        Ok(())
    }

    fn drain(&mut self, ticket: Ticket, cutoff: u64) -> Result<Release, NativeRefusal> {
        let mut active = self
            .active
            .take()
            .filter(|active| active.ticket == ticket)
            .ok_or(JOB)?;
        let deadline = self.env.by(cutoff);
        let env = &self.env;
        let release = env.runtime.block_on(async {
            let shutdown = if active.installer.is_some() {
                active.installed
            } else if active.continued {
                match active.qmp.take() {
                    Some(qmp) => tokio::time::timeout_at(
                        deadline - Duration::from_secs(3),
                        qmp.powerdown(env.cancel),
                    )
                    .await
                    .is_ok_and(|result| result.is_ok()),
                    None => false,
                }
            } else {
                false
            };
            drop(active.pipe.take());
            let reap_by = deadline - Duration::from_secs(2);
            let exited = shutdown && active.core.child.reap(reap_by).await.is_ok();
            if !exited {
                active.core.workers.stop();
                let _ = active.core.child.job.terminate();
            }
            let clean = exited && active.core.child.exit_code() == Ok(0);
            active
                .core
                .release_evidence_held(shutdown, clean, deadline, || true)
                .await
                .0
        });
        let peer = active.peer;
        drop(active);
        if release != StopDisposition::CleanRelease {
            return Err(JOB);
        }
        if ticket.phase == Phase::Install {
            self.installed = Some(self.installed_pair()?);
        }
        Ok(Release {
            peer,
            process_exited: true,
            empty_job: true,
            workers_joined: true,
            writers_excluded: true,
            clean_guest_shutdown: true,
        })
    }

    fn dispose(&mut self, scope: Scope, cutoff: u64) -> Result<WorkspaceRelease, NativeRefusal> {
        if self.active.is_some() || self.staged.is_some() || scope != self.env.scope {
            return Err(REFUSED);
        }
        inventory::binding(self.env.root)?;
        self.installed = None;
        for &(name, id) in &self.leaves {
            if Deadline::now() >= self.env.by(cutoff) {
                return Err(REFUSED);
            }
            sink::discard_owned(&self.env.root.leaf, name, id).map_err(|_| REFUSED)?;
        }
        Ok(WorkspaceRelease {
            scope,
            owned_leaves_absent: true,
            helpers_joined: true,
        })
    }

    fn copies_independent_and_original_unchanged(&mut self) -> bool {
        let Some(pair) = self.installed.as_ref() else {
            return false;
        };
        pair.iter()
            .zip([DISK_FILE_MAX, FIRMWARE_MAX])
            .all(|(pin, max)| {
                inventory::binding(pin).is_ok()
                    && pin.verified_content.is_some_and(|expected| {
                        launch::hash_held(
                            &pin.leaf,
                            max,
                            self.env.by(MAX_TRIAL_MS).into_std(),
                            &|| self.env.cancel.is_cancelled(),
                        )
                        .ok()
                            == Some(expected)
                    })
            })
    }
}

pub(super) struct Serial;

impl Guest<PhaseHost<'_>> for Serial {
    fn exchange(
        &mut self,
        peer: PeerId,
        frame: Vec<u8>,
        cutoff: u64,
        host: &mut PhaseHost<'_>,
    ) -> Result<Reply, NativeRefusal> {
        let env = &host.env;
        let active = host
            .active
            .as_mut()
            .filter(|a| a.peer == peer && a.continued)
            .ok_or(ENDPOINT)?;
        let pipe = active.pipe.as_mut().ok_or(ENDPOINT)?;
        let token = &env.m4.held.token;
        let core = &active.core;
        let deadline = env
            .by(cutoff)
            .min(Deadline::now() + Duration::from_secs(90));
        let frame = env.runtime.block_on(monitor_running(
            async {
                prove_serial_peer(pipe, &core.child, token.account.0, &token.entries())?;
                pipe.write_all(&frame).await.map_err(|_| ENDPOINT)?;
                pipe.flush().await.map_err(|_| ENDPOINT)?;
                let len = pipe.read_u32().await.map_err(|_| ENDPOINT)? as usize;
                if !(1..=arx_guest::MAX_FRAME).contains(&len) {
                    return Err(ENDPOINT);
                }
                let mut received = vec![0; len + 4];
                received[..4].copy_from_slice(&(len as u32).to_be_bytes());
                tokio::time::timeout(
                    crate::vm::guest::FRAME_BOUND,
                    pipe.read_exact(&mut received[4..]),
                )
                .await
                .map_err(|_| ENDPOINT)?
                .map_err(|_| ENDPOINT)?;
                prove_serial_peer(pipe, &core.child, token.account.0, &token.entries())?;
                Ok(received)
            },
            || core.check_running(|| env.live(cutoff, true), false),
            env.cancel,
            deadline,
            |_| ENDPOINT,
        ))?;
        Ok(Reply { peer, frame })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_copy_requires_exact_source_length_and_respects_cancellation() {
        let bytes = vec![0xAB; (256 << 10) + 11];
        let by = Instant::now() + Duration::from_secs(5);
        let size = bytes.len() as u64;
        let mut destination = Vec::new();
        assert_eq!(
            copy_exact(&bytes[..], &mut destination, size, by, &|| false),
            Ok(())
        );
        assert_eq!(destination, bytes);
        for incorrect in [size - 1, size + 1] {
            assert_eq!(
                copy_exact(&bytes[..], io::sink(), incorrect, by, &|| false),
                Err(REFUSED)
            );
        }
        destination.clear();
        let cancelled = std::cell::Cell::new(false);
        assert_eq!(
            copy_exact(&bytes[..], &mut destination, size, by, &|| cancelled
                .replace(true)),
            Err(REFUSED)
        );
        assert_eq!(destination.len(), 256 << 10);
    }
}

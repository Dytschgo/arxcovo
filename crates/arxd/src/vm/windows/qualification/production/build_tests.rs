//! Run the build against real NTFS and SQLite, consuming the real issuer
//! permits. Child launch and guest responses are injected at the runtime edge.
use super::super::disposal::tests::{
    FakeClock, Fault as DisposalFault, FaultDisposal, NOW, RestartReaper, SweepFixture, exists,
};
use super::super::recovery::{NativeRecovery, StorePins};
use super::super::tests::custody::{Fault as CustodyFault, Recorder};
use super::super::tests::unbacked_qcow2_header;
use super::*;
use crate::vm::paths::ArtifactPin;
use crate::vm::qemu_args::{BootPurpose, BootRecipe, SerialName};
use crate::vm::test_issuer::tests::{FakeSource, assert_admission_closed, config};
use crate::vm::test_issuer::{Installation, Intent, NativeTarget, SelectedSource, TestIssuer};
use crate::vm::{BuildAdmission, Capacity, DiskClaim, HostObservation, RequestId, Resources};
use std::cell::{Cell, RefCell};

const SIZE: u64 = 64 << 20;
const IMAGES: u64 = 2 * GIB;
/// A retained image claim that fits the initial pair but not its growth.
const TIGHT: u64 = 256 << 20;
const VARS: &[u8] = b"fixture VARS";
const SEED: &[u8] = b"fixture installer seed";
const META: u64 = 8192;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fault {
    None,
    Identity,
    SeedWrite,
    SeedSync,
    VarsWrite,
    Setup,
    SetupPhase,
    SetupSlot,
    WindowsHealth,
    Finalization,
    OverlayChild,
    OverlayLineage,
    VerificationBoot,
    VerificationHealth,
    VerificationLineage,
    VerificationTeardown,
    CancelDuringStage,
    CancelAfterRelease,
    Dispose(DisposalFault),
    SealRollback,
    SealCutoff,
    /// The final primary boot grows the pair one byte past the claim.
    OverRetained,
    /// A real SQLite failure inside the seal transaction.
    SealSql,
    /// A genuine permit for different content.
    CopyContent,
    CancelAtCopy,
    /// Cancellation arrives while capture waits for the store.
    CancelAtCapture,
    /// The pre-seal observation ages past freshness before the seal gate.
    StaleSample,
    /// The installer stage reaches its own deadline.
    InstallerTimeout,
    CancelBeforeStage,
    /// Something tries to change the verification VARS copy in place
    /// between the copy and its launch.
    VarsMutation,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    UbuntuInstall,
    WindowsSetup,
    WindowsHealth,
    WindowsFinalize,
    Verification,
}

struct Peer<'a> {
    fx: &'a SweepFixture,
    slots: BuildSlots,
    intent: &'a Intent<'a, &'a FakeSource, FakeClock>,
    issuer: &'a TestIssuer<'a, &'a FakeSource, FakeClock>,
    cancel: &'a Shutdown,
    cwd: LocalPath,
    fault: Fault,
    steps: RefCell<Vec<Step>>,
    seal_checks: Cell<u32>,
    verification_settled: Cell<bool>,
    origin: native::recovery::Origin,
    mutation_refused: Cell<Option<bool>>,
}

impl Peer<'_> {
    fn artifact(&self, role: ArtifactRole, name: &str) -> Artifact {
        let path = self.cwd.join(name).unwrap();
        std::fs::write(path.as_str(), b"fixture input").unwrap();
        let pin = PathPin::open(&path, false, false).unwrap();
        launch::artifact(role, &pin)
            .unwrap()
            .with_pin(pin_bytes(b"fixture input"))
    }

    fn record_job(&self, workspace: &ProductionWorkspace) {
        let scope = custody::scope(workspace.binding());
        let origin = self.origin;
        self.fx
            .store()
            .transact(|db| native::recovery::record_job(db, scope, scope.generation, origin))
            .unwrap();
    }

    fn checked_boot(&self, request: &Boot<'_>, stage: Option<InstallerStage>) -> Result<(), Q> {
        use ArtifactRole::*;
        let Boot {
            workspace,
            pair,
            recipe,
            backing,
            staged,
            ..
        } = *request;
        let image = self.artifact(Qemu, "qemu-system-x86_64.exe");
        let cwd = &self.cwd;
        let share = cwd.join("share").unwrap();
        std::fs::create_dir_all(share.as_str()).unwrap();
        let data = launch::artifact(Data, &PathPin::open(&share, true, false)?)?;
        let firmware = self.artifact(FirmwareCode, "firmware.fd");
        let [disk, vars] = pair.evidence;
        let disk = workspace.pin_slot(pair.slots.disk, disk.identity, true)?;
        let vars = workspace.pin_slot(pair.slots.vars, vars.identity, true)?;
        let phase = stage.map(InstallerStage::phase);
        let vars_role = if phase == Some(InstallerPhase::WindowsSetup) {
            WindowsSetupVars
        } else {
            Vars
        };
        let vars = launch::artifact(vars_role, &vars)?.with_pin(pair.evidence[1].pin);
        let role = if phase.is_some() {
            TestInstallDisk
        } else {
            GuestDisk
        };
        let disk_arg = launch::artifact(role, &disk)?;
        let header = launch::header(&disk)?;
        let backing_header = backing.map(|_| unbacked_qcow2_header(SIZE));
        let iso = self.artifact(
            if vars_role == WindowsSetupVars {
                WindowsInstallIso
            } else {
                UbuntuInstallIso
            },
            "install.iso",
        );
        let drivers = self.artifact(WindowsDriverMedium, "drivers.iso");
        let kernel = self.artifact(UbuntuKernel, "kernel");
        let initrd = self.artifact(UbuntuInitrd, "initrd");
        let seed = staged.map(|staged| {
            let (slot, evidence) = staged.artifacts()[0];
            let pin = workspace.pin_slot(slot, evidence.identity, false).unwrap();
            launch::artifact(
                if vars_role == WindowsSetupVars {
                    WindowsAnswerSeed
                } else {
                    UbuntuCidata
                },
                &pin,
            )
            .unwrap()
            .with_pin(evidence.pin)
        });
        let boot = match phase {
            Some(InstallerPhase::UbuntuInstall) => BootPurpose::UbuntuInstall {
                iso: &iso,
                cidata: seed.as_ref().unwrap(),
                kernel: &kernel,
                initrd: &initrd,
            },
            Some(InstallerPhase::WindowsSetup) => BootPurpose::WindowsSetup {
                iso: &iso,
                answer: seed.as_ref().unwrap(),
                drivers: &drivers,
            },
            Some(InstallerPhase::WindowsFinalize) => BootPurpose::WindowsFinalize,
            None => BootPurpose::Installed,
        };
        let serial = SerialName::generate().unwrap();
        let boot = BootRecipe {
            image: &image,
            data: &data,
            firmware: &firmware,
            vars: &vars,
            disk: &disk_arg,
            disk_header: &header,
            backing,
            backing_header: backing_header.as_deref(),
            serial: &serial,
            boot,
            ram_mib: 4096,
            vcpus: 2,
        };
        let plan = boot.plan().expect("fixture uses the production qemu plan");
        let binding = workspace.binding();
        let target = || LaunchTarget {
            plan: &plan,
            writer_epoch: binding.generation,
            cwd,
            pair: Some(pair.slots),
            installer: stage,
            recipe: Some(recipe),
            helper: None,
            memory: TEST_JOB_MEMORY,
            cpu_rate: JOB_CPU_RATE,
        };
        let permit = self.intent.launch(binding, target())?;
        consume_checked(permit, target(), binding, config().selection().seal_sha256)?;
        Ok(())
    }
}

impl Effects for Peer<'_> {
    fn fresh(&self) -> Result<Instant, Q> {
        if self.fault == Fault::StaleSample {
            // Still fresh before the seal transaction, stale inside it.
            return Ok(Instant::now() - Duration::from_secs(4));
        }
        self.issuer.refresh()
    }

    fn crossing(&self, point: &'static str) {
        match (point, self.fault) {
            ("capture", Fault::CancelAtCapture) => self.cancel.cancel(),
            ("copy", Fault::VarsMutation) => {
                use std::io::Write;
                let path = self.fx.slot_file(self.slots.verification.vars);
                match std::fs::OpenOptions::new().write(true).open(path) {
                    Ok(mut file) => {
                        // Same object, same length, one changed byte.
                        file.write_all(b"X").unwrap();
                        self.mutation_refused.set(Some(false));
                    }
                    Err(_) => self.mutation_refused.set(Some(true)),
                }
            }
            _ => {}
        }
    }

    fn stage<'w>(
        &self,
        reservation: super::super::super::recipe::InstallerReservation<'w>,
        ledger: &dyn CustodyCheckpoints,
    ) -> Result<super::super::super::recipe::StagedInstaller<'w>, Q> {
        let result = super::super::super::recipe::stage_prepared(
            reservation,
            ledger,
            config().selection().seal_sha256,
            Vec::new(),
            [SEED, VARS],
            &|| self.cancel.is_cancelled(),
            |target| {
                match (self.fault, target.slot.role) {
                    (Fault::SeedWrite, "seed") | (Fault::VarsWrite, "vars") => {
                        WRITE_FAULT.with(|f| f.set(Some(WriteFault::Partial)))
                    }
                    (Fault::SeedSync, "seed") => {
                        WRITE_FAULT.with(|f| f.set(Some(WriteFault::Sync)))
                    }
                    _ => {}
                }
                let authority = self.intent.stage(target)?.consume(target)?;
                if self.fault == Fault::CancelDuringStage {
                    self.cancel.cancel();
                }
                Ok(authority)
            },
        );
        WRITE_FAULT.set(None);
        result
    }

    fn image(&self) -> Result<Artifact, Q> {
        Ok(self.artifact(ArtifactRole::QemuImg, "qemu-img.exe"))
    }

    async fn helper<'w>(
        &'w self,
        prepared: &'w PreparedHelper<'w, '_>,
        image: &'w Artifact,
        backing: Option<(&Artifact, &[u8])>,
        size: u64,
        ceiling: u64,
    ) -> HelperSettlement<'w> {
        let slot = prepared.slot();
        let binding = prepared.workspace().binding();
        let stage = prepared.staging().unwrap();
        let plan =
            crate::vm::qemu_img::create(prepared.lease(), image, &stage, backing, size, ceiling)
                .unwrap();
        let target = || prepared.launch_target(&plan, &self.cwd);
        let permit = self.intent.launch(binding, target()).unwrap();
        consume_checked(permit, target(), binding, config().selection().seal_sha256).unwrap();
        self.record_job(prepared.workspace());
        let cluster = match backing {
            Some((backing, _)) => {
                let path = backing.path_for(ArtifactRole::Golden).unwrap().as_str();
                let name = if self.fault == Fault::OverlayLineage {
                    format!("{path}.foreign")
                } else {
                    path.to_owned()
                };
                backed_header(&name)
            }
            None => unbacked_qcow2_header(size),
        };
        std::fs::write(self.fx.slot_file(slot), cluster).unwrap();
        if slot.role == "verification_overlay" && self.fault == Fault::OverlayChild {
            return HelperSettlement::quarantined(Err(PATH));
        }
        let pin = prepared
            .workspace()
            .pin_slot(slot, prepared.identity(), true)
            .unwrap();
        let held = pin.reopen_without_writer().unwrap();
        let inputs = plan
            .inputs()
            .iter()
            .map(|(role, input)| (*role, input.identity().unwrap()))
            .collect();
        let released = ReleasedGeneration::checked(
            binding,
            ReleaseTargets::Helper {
                kind: PlanKind::Mutate,
                inputs,
            },
            WorkspaceCustody::production(&prepared.workspace().root, &prepared.workspace().lease)
                .unwrap(),
            (vec![pin].into(), vec![held]),
        )
        .unwrap();
        HelperSettlement {
            output: Ok(Vec::new()),
            disposition: StopDisposition::CleanRelease,
            released: Some(released),
        }
    }

    fn authorize_copy(&self, pair: PairSlots, content: ArtifactPin) -> Result<CopyAuthority, Q> {
        let content = if self.fault == Fault::CopyContent {
            pin_bytes(b"other VARS")
        } else {
            content
        };
        let authority = self
            .intent
            .copy_verification(pair, content)?
            .consume(pair.vars, content)?;
        if self.fault == Fault::CancelAtCopy {
            self.cancel.cancel();
        }
        Ok(authority)
    }

    async fn boot<'w>(
        &self,
        mut request: Boot<'_>,
        workspace: &'w ProductionWorkspace,
    ) -> Result<BootRelease<'w>, Q> {
        let handoff = request.vars.take();
        let pair = request.pair;
        let installer = match request.phase {
            Phase::Install(stage) => Some(stage),
            _ => None,
        };
        let phase = installer.map(InstallerStage::phase);
        let step = match (phase, request.phase) {
            (Some(InstallerPhase::UbuntuInstall), _) => Step::UbuntuInstall,
            (Some(InstallerPhase::WindowsSetup), _) => Step::WindowsSetup,
            (Some(InstallerPhase::WindowsFinalize), _) => Step::WindowsFinalize,
            (None, Phase::Health(HealthPhase::Verification)) => Step::Verification,
            (None, _) => Step::WindowsHealth,
        };
        self.checked_boot(&request, installer)?;
        // The child opens its writable VARS after resume.
        drop(handoff);
        self.record_job(workspace);
        self.steps.borrow_mut().push(step);
        let verify = step == Step::Verification;
        if matches!(self.fault, Fault::Setup) && installer.is_some()
            || verify && self.fault == Fault::VerificationBoot
        {
            return Err(HEALTH);
        }
        if self.fault == Fault::InstallerTimeout && installer.is_some() {
            return Err(NativeRefusal::InstallerDeadlineExpired);
        }
        // Each successful launch writes to the staged pair. The final ledger
        // pins must describe these bytes and not the pre-launch contents.
        use std::io::Write;
        let mut vars = std::fs::OpenOptions::new()
            .append(true)
            .open(self.fx.slot_file(pair.slots.vars))
            .unwrap();
        vars.write_all(b"+guest").unwrap();
        drop(vars);
        if verify && self.fault == Fault::VerificationLineage {
            std::fs::write(
                self.fx.slot_file(pair.slots.disk),
                backed_header("D:\\foreign"),
            )
            .unwrap();
        }
        if verify && self.fault == Fault::VerificationTeardown {
            return Err(PATH);
        }
        let last_primary = matches!(step, Step::UbuntuInstall | Step::WindowsFinalize);
        if last_primary && self.fault == Fault::OverRetained {
            let vars = std::fs::metadata(self.fx.slot_file(pair.slots.vars))
                .unwrap()
                .len();
            std::fs::OpenOptions::new()
                .write(true)
                .open(self.fx.slot_file(pair.slots.disk))
                .unwrap()
                .set_len(TIGHT - vars + 1)
                .unwrap();
        }
        let [disk, vars] = [
            workspace.pin_slot(pair.slots.disk, pair.evidence[0].identity, true)?,
            workspace.pin_slot(pair.slots.vars, pair.evidence[1].identity, true)?,
        ];
        let guards = [&disk, &vars]
            .into_iter()
            .map(|pin| pin.reopen_without_writer().unwrap())
            .collect();
        let mut slots = pair.slots;
        if self.fault == Fault::SetupSlot && installer.is_some() {
            slots.vars.key = [0x7f; 16];
        }
        let mut release = ReleasedGeneration::checked(
            workspace.binding(),
            ReleaseTargets::Pair {
                pair: slots,
                ids: pair.evidence.map(|file| file.identity),
            },
            WorkspaceCustody::production(&workspace.root, &workspace.lease)?,
            (vec![disk, vars].into(), guards),
        )
        .unwrap();
        release.stop_disposition = Some(StopDisposition::CleanRelease);
        if verify {
            self.verification_settled.set(true);
            if self.fault == Fault::CancelAfterRelease {
                self.cancel.cancel();
            }
        }
        if verify && self.fault == Fault::VerificationHealth
            || step == Step::WindowsHealth && self.fault == Fault::WindowsHealth
        {
            return Err(HEALTH);
        }
        let finalized = (step == Step::WindowsFinalize)
            .then_some(Finalized::Fixture(self.fault != Fault::Finalization));
        let phase = if self.fault == Fault::SetupPhase && installer.is_some() {
            Some(InstallerPhase::WindowsFinalize)
        } else {
            phase
        };
        Ok(BootRelease {
            release,
            phase,
            finalized,
        })
    }

    fn inside_seal(&self) {
        assert!(!exists(&self.fx.slot_file(self.slots.seed)));
        assert!(!exists(
            &self.fx.gen_dir(self.slots.verification.disk.generation)
        ));
        // The primary lease is still held exclusively by the build.
        if let Ok(lease) = std::fs::File::open(self.fx.lease_file(self.slots.pair.disk.generation))
        {
            assert!(
                lease.try_lock().is_err(),
                "the primary lease was released before the seal transaction"
            );
        }
        for slot in [self.slots.pair.disk, self.slots.pair.vars] {
            assert!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(self.fx.slot_file(slot))
                    .is_err(),
                "a writer acquired {} inside the seal transaction",
                slot.role
            );
        }
        let check = self.seal_checks.get() + 1;
        self.seal_checks.set(check);
        if check == 1 && self.fault == Fault::StaleSample {
            std::thread::sleep(Duration::from_millis(1_500));
        }
        if check == 2 {
            match self.fault {
                Fault::SealRollback => self.cancel.cancel(),
                Fault::SealCutoff => self.fx.clock.advance(0, 14_000),
                _ => {}
            }
        }
    }
}

fn pin_bytes(bytes: &[u8]) -> ArtifactPin {
    ArtifactPin {
        size: bytes.len() as u64,
        sha256: crate::vm::qualification_record::digest(bytes),
    }
}

fn backed_header(backing: &str) -> Vec<u8> {
    let mut cluster = unbacked_qcow2_header(SIZE);
    cluster[104..108].copy_from_slice(&0xe279_2acau32.to_be_bytes());
    cluster[108..112].copy_from_slice(&5u32.to_be_bytes());
    cluster[112..117].copy_from_slice(b"qcow2");
    cluster[8..16].copy_from_slice(&128u64.to_be_bytes());
    cluster[16..20].copy_from_slice(&(backing.len() as u32).to_be_bytes());
    cluster[128..128 + backing.len()].copy_from_slice(backing.as_bytes());
    cluster
}

struct Outcome {
    slots: BuildSlots,
    claim: i64,
    status: anyhow::Result<Status>,
    steps: Vec<Step>,
    seal_checks: u32,
    verification_settled: bool,
    /// The recorded failure cause, or 0 for none.
    cause: i64,
    mutation_refused: Option<bool>,
}

fn request(recipe: &str, digest: Hash, nonce: u32) -> BuildRequest<'_> {
    let key = RequestId::parse(&format!("{NOW:016x}{nonce:016x}")).unwrap();
    BuildRequest {
        principal: "alice",
        key,
        recipe,
        digest,
        retained: DiskClaim {
            images: IMAGES,
            metadata: META,
            staging: 0,
        },
        transient: DiskClaim {
            images: IMAGES,
            metadata: 0,
            staging: 4096,
        },
    }
}

fn host() -> HostObservation {
    HostObservation {
        owner_idle: Some(true),
        logical_cpus: Some(8),
        available_ram: Some(32 * GIB),
        capacity: Some(Capacity {
            free_bytes: 400 * GIB,
            managed_bytes: 0,
        }),
    }
}

fn resources() -> Resources {
    Resources {
        vcpus: 2,
        guest_ram: 4 * GIB,
        overhead_ram: GIB,
    }
}

async fn run(fx: &SweepFixture, windows: bool, fault: Fault, nonce: u32) -> Outcome {
    let source = FakeSource::new(fx.clock.now().monotonic);
    let recipes = (&source).recipes();
    let recipe = if windows {
        recipes.windows
    } else {
        recipes.ubuntu
    };
    let recipe_name = hex::encode(recipe.recipe_id);
    let mut request = request(&recipe_name, recipe.recipe_sha256, nonce);
    if fault == Fault::OverRetained {
        request.retained.images = TIGHT;
    }
    let BuildAdmission::Fresh(slots, worker, _) = fx
        .ctl()
        .admit_build(request, host(), resources(), |_, _| None)
        .unwrap()
        .unwrap()
    else {
        panic!("fresh build")
    };
    let slots = *slots;
    let claim = (request.retained.total().unwrap() + request.transient.total().unwrap()) as i64;
    assert_eq!(
        fx.count("SELECT sum(staging_bytes) FROM vm_build_intents"),
        claim
    );
    let cancel = Shutdown::default();
    let issuer = TestIssuer::open(&Installation::fixture(), fx.ctl(), &source, &config()).unwrap();
    let intent = issuer
        .intent(
            &worker,
            NativeTarget {
                generation: slots.pair.disk.generation,
                writer_epoch: None,
            },
            &cancel,
        )
        .unwrap();
    let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let cwd = LocalPath::parse(dir.path().to_str().unwrap()).unwrap();
    // A store held in the production root records the real writer origin
    // that restart recovery can prove. Otherwise the origin stays foreign.
    let origin = match StorePins::open_with_owner(&fx.production, fx.owner_sid()) {
        Ok(pins) => pins.origin(fx.store(), boot::marker().unwrap()).unwrap(),
        Err(_) => native::recovery::Origin {
            boot: [0x43; 32],
            database: FileIdentity {
                volume: fx.production.identity.volume,
                file: [0x44; 16],
            },
            lock: FileIdentity {
                volume: fx.production.identity.volume,
                file: [0x45; 16],
            },
        },
    };
    let peer = Peer {
        fx,
        slots,
        intent: &intent,
        issuer: &issuer,
        cancel: &cancel,
        cwd,
        fault,
        steps: RefCell::new(Vec::new()),
        seal_checks: Cell::new(0),
        verification_settled: Cell::new(false),
        origin,
        mutation_refused: Cell::new(None),
    };
    let ledger = Recorder::new(fx.store(), &fx.production.path, slots.pair.disk.generation);
    let ledger_disposal = FaultDisposal::new(
        fx.store(),
        match fault {
            Fault::Dispose(f) => Some(f),
            _ => None,
        },
    );
    let guards = GenerationGuards::default();
    let build = Build {
        controller: fx.ctl(),
        worker: &worker,
        request,
        slots,
        recipe,
        ledger: &ledger,
        disposal: fx.disposal(&ledger_disposal, &guards),
        cancel: &cancel,
        owner: fx.owner_sid().0,
        size: SIZE,
    };
    let reserve =
        |generation| reserve_workspace_under(&fx.production, fx.owner_sid().0, generation, &ledger);
    let primary = reserve(slots.pair.disk.generation).unwrap();
    if fault == Fault::Identity {
        ledger
            .fault
            .set(Some(CustodyFault::Before("slot_identified")));
    }
    if fault == Fault::CancelBeforeStage {
        cancel.cancel();
    }
    if fault == Fault::SealSql {
        fx.store()
            .transact(|db| {
                db.execute_batch(
                    "CREATE TEMP TRIGGER fail_seal BEFORE INSERT ON vm_templates
                     BEGIN SELECT RAISE(ABORT,'injected seal failure'); END;",
                )?;
                Ok(())
            })
            .unwrap();
    }
    let status = build
        .run(&peer, primary, || {
            reserve(slots.verification.disk.generation)
        })
        .await;
    if fault == Fault::SealSql {
        fx.store()
            .transact(|db| {
                db.execute_batch("DROP TRIGGER temp.fail_seal;")?;
                Ok(())
            })
            .unwrap();
    }
    let cause = fx.count(&format!(
        "SELECT coalesce(failure_reason,0) FROM vm_operations WHERE activity_id={}",
        worker.activity_id()
    ));
    let at_identity = ledger
        .log
        .borrow()
        .iter()
        .filter_map(|(name, len)| (*name == "slot_identified").then_some(*len))
        .collect::<Vec<_>>();
    assert!(at_identity.len() <= 5);
    assert!(
        at_identity.iter().flatten().all(|len| *len == 0),
        "installer wrote before its identity checkpoint: {at_identity:?}"
    );
    Outcome {
        slots,
        claim,
        status,
        steps: peer.steps.into_inner(),
        seal_checks: peer.seal_checks.get(),
        verification_settled: peer.verification_settled.get(),
        cause,
        mutation_refused: peer.mutation_refused.get(),
    }
}

#[tokio::test]
async fn build_seals_both_recipes_with_observed_charge_and_live_writer_exclusions() {
    for (windows, expected) in [
        (false, vec![Step::UbuntuInstall, Step::Verification]),
        (
            true,
            vec![
                Step::WindowsSetup,
                Step::WindowsHealth,
                Step::WindowsFinalize,
                Step::Verification,
            ],
        ),
    ] {
        assert_admission_closed();
        let fx = SweepFixture::new();
        let Outcome {
            slots,
            status,
            steps,
            seal_checks,
            ..
        } = run(&fx, windows, Fault::None, 11).await;
        assert_eq!(steps, expected);
        assert_eq!(status.unwrap(), Status::Succeeded);
        assert_eq!(seal_checks, 2);
        assert_eq!(fx.count("SELECT count(*) FROM vm_templates"), 1);
        assert_eq!(fx.count("SELECT count(*) FROM vm_artifacts"), 2);
        assert_eq!(fx.count("SELECT count(*) FROM vm_native_generations"), 1);
        assert_eq!(
            fx.count(
                "SELECT count(*) FROM vm_build_intents WHERE staging_bytes=0 AND health_verified=1"
            ),
            1
        );
        let retained = [slots.pair.disk, slots.pair.vars]
            .into_iter()
            .map(|slot| std::fs::metadata(fx.slot_file(slot)).unwrap().len())
            .sum::<u64>()
            + META;
        assert_eq!(
            fx.count("SELECT sum(bytes) FROM vm_templates"),
            retained as i64
        );
        assert!(exists(&fx.lease_file(slots.pair.disk.generation)));
        assert!(!exists(&fx.slot_file(slots.seed)));
        assert!(!exists(&fx.gen_dir(slots.verification.disk.generation)));
    }
}

/// I06/I15: the authorized verification VARS copy stays writer-excluded
/// from its content check until the launch, so an in-place change between
/// copy and launch is refused and the boot uses exactly the copied bytes.
#[tokio::test]
async fn build_keeps_the_verification_vars_copy_writer_excluded_until_launch() {
    for windows in [false, true] {
        assert_admission_closed();
        let fx = SweepFixture::new();
        let outcome = run(&fx, windows, Fault::VarsMutation, 66).await;
        assert_eq!(outcome.mutation_refused, Some(true), "{windows}");
        assert_eq!(outcome.status.unwrap(), Status::Succeeded);
        assert!(outcome.steps.contains(&Step::Verification));
    }
}

/// The launch-side content recheck refuses a same-identity in-place change
/// if writer exclusion were ever lost, and the handed-off pin itself
/// refuses a writer.
#[test]
fn vars_handoff_refuses_an_in_place_change_of_the_same_object() {
    use std::io::Write;
    let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let path = dir.path().join("vars");
    std::fs::write(&path, VARS).unwrap();
    let local = LocalPath::parse(path.to_str().unwrap()).unwrap();
    // A write-sharing pin stands in for a handoff whose exclusion was lost.
    let pin = PathPin::open(&local, false, true).unwrap();
    let file = ArtifactEvidence {
        identity: pin.identity,
        directory: [0; 16],
        pin: pin_bytes(VARS),
    };
    let handoff = VarsHandoff {
        pin,
        content: pin_bytes(VARS),
    };
    let by = Instant::now() + Duration::from_secs(5);
    assert_eq!(handoff.confirm(file, by, &|| false), Ok(()));
    let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    writer.write_all(b"X").unwrap();
    drop(writer);
    assert_eq!(handoff.confirm(file, by, &|| false), Err(PATH));
    drop(handoff);
    std::fs::write(&path, VARS).unwrap();
    let held = PathPin::open(&local, false, false).unwrap();
    assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
    drop(held);
}

#[tokio::test]
async fn build_faults_keep_work_attributed_and_prevent_seal() {
    for fault in [
        Fault::Identity,
        Fault::SeedWrite,
        Fault::SeedSync,
        Fault::VarsWrite,
        Fault::Setup,
        Fault::SetupPhase,
        Fault::SetupSlot,
        Fault::WindowsHealth,
        Fault::Finalization,
        Fault::OverlayChild,
        Fault::OverlayLineage,
        Fault::VerificationBoot,
        Fault::VerificationHealth,
        Fault::VerificationLineage,
        Fault::VerificationTeardown,
        Fault::CancelDuringStage,
        Fault::CancelAfterRelease,
        Fault::SealRollback,
        Fault::SealCutoff,
        Fault::OverRetained,
        Fault::SealSql,
        Fault::CopyContent,
        Fault::CancelAtCopy,
        Fault::CancelAtCapture,
        Fault::StaleSample,
        Fault::InstallerTimeout,
        Fault::CancelBeforeStage,
    ] {
        assert_admission_closed();
        let fx = SweepFixture::new();
        let Outcome {
            slots,
            claim,
            status,
            steps,
            seal_checks,
            verification_settled,
            cause,
            ..
        } = run(&fx, true, fault, 22).await;
        assert!(status.is_err(), "{fault:?}");
        // Staging, installer, health and seal failures keep their proven
        // cause. Cancellation has none and records unconfirmed cleanup.
        let expected = match fault {
            Fault::Identity
            | Fault::SeedWrite
            | Fault::SeedSync
            | Fault::VarsWrite
            | Fault::OverlayChild
            | Fault::OverlayLineage
            | Fault::CopyContent => Some(FailureReason::StagingFailed),
            Fault::Setup | Fault::SetupPhase | Fault::SetupSlot => {
                Some(FailureReason::InstallFailed)
            }
            Fault::WindowsHealth
            | Fault::VerificationBoot
            | Fault::VerificationHealth
            | Fault::VerificationLineage
            | Fault::VerificationTeardown => Some(FailureReason::HealthFailed),
            Fault::Finalization | Fault::OverRetained | Fault::SealSql | Fault::StaleSample => {
                Some(FailureReason::SealFailed)
            }
            Fault::InstallerTimeout => Some(FailureReason::InstallTimedOut),
            // Cancellation, the controller's worker deadline (SealCutoff) and
            // a cancelled preparation are interruptions without a cause.
            Fault::CancelDuringStage
            | Fault::CancelAfterRelease
            | Fault::CancelAtCopy
            | Fault::CancelAtCapture
            | Fault::CancelBeforeStage
            | Fault::SealRollback
            | Fault::SealCutoff => Some(FailureReason::CleanupUnconfirmed),
            _ => None,
        };
        if let Some(expected) = expected {
            assert_eq!(cause, expected as i64, "{fault:?}");
        }
        if matches!(fault, Fault::CopyContent | Fault::CancelAtCopy) {
            assert!(!exists(&fx.slot_file(slots.verification.vars)), "{fault:?}");
            assert!(exists(&fx.slot_file(slots.verification.disk)), "{fault:?}");
        }
        if matches!(
            fault,
            Fault::OverRetained | Fault::Finalization | Fault::CancelAtCapture
        ) {
            // The refused pair never received a durable digest.
            assert_eq!(
                fx.count(
                    "SELECT count(*) FROM vm_native_slots WHERE content_digest IS NOT NULL
                     AND role IN ('disk','vars','verification_overlay')"
                ),
                0,
                "{fault:?}"
            );
            assert!(!steps.contains(&Step::Verification), "{fault:?}");
        }
        if matches!(fault, Fault::SealSql | Fault::StaleSample) {
            assert_eq!(seal_checks, 1, "{fault:?}");
        }
        if fault == Fault::CancelBeforeStage {
            assert!(steps.is_empty());
        }
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_templates"),
            0,
            "{fault:?}"
        );
        assert_eq!(
            fx.count("SELECT sum(staging_bytes) FROM vm_build_intents"),
            claim,
            "{fault:?}"
        );
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_operations WHERE status=0"),
            0,
            "{fault:?}"
        );
        if matches!(fault, Fault::SealRollback | Fault::SealCutoff) {
            assert_eq!(seal_checks, 2);
            assert!(!exists(&fx.gen_dir(slots.verification.disk.generation)));
        }
        if fault == Fault::VerificationHealth {
            assert!(verification_settled);
        }
        if fault == Fault::VerificationTeardown {
            assert!(steps.contains(&Step::Verification));
            assert!(!verification_settled);
            assert_eq!(
                fx.count("SELECT count(*) FROM vm_native_writer WHERE boot IS NOT NULL"),
                2
            );
        }
        if fault == Fault::Identity {
            assert_eq!(
                std::fs::metadata(fx.slot_file(slots.seed)).unwrap().len(),
                0
            );
        }
    }
}

/// Interrupted transient disposal and a rolled-back seal transaction keep
/// the existing golden and the peak claim until ordinary sweep converges.
#[tokio::test]
async fn build_transient_disposal_restarts_without_losing_the_existing_golden() {
    for fault in [
        Fault::Dispose(DisposalFault::Before("slot_disposed", 1)),
        Fault::Dispose(DisposalFault::Before("slot_disposed", 2)),
        Fault::Dispose(DisposalFault::Before("slot_disposed", 3)),
        Fault::Dispose(DisposalFault::After("intend_lease_disposal", 1)),
        Fault::Dispose(DisposalFault::After("intend_directory_disposal", 1)),
        Fault::Dispose(DisposalFault::Before("generation_disposed", 1)),
        Fault::SealSql,
        Fault::SealRollback,
    ] {
        assert_admission_closed();
        let mut fx = SweepFixture::new();
        let first = run(&fx, false, Fault::None, 33).await;
        assert_eq!(first.status.unwrap(), Status::Succeeded);
        let golden = std::fs::read(fx.slot_file(first.slots.pair.disk)).unwrap();
        let interrupted = run(&fx, true, fault, 34).await;
        assert!(interrupted.status.is_err(), "{fault:?}");
        assert_eq!(fx.count("SELECT count(*) FROM vm_templates"), 1);
        assert_eq!(
            fx.count("SELECT sum(staging_bytes) FROM vm_build_intents"),
            interrupted.claim
        );
        fx.reopen();
        fx.clock.advance(60_000, 60);
        for _ in 0..4 {
            fx.sweep(None);
        }
        assert_eq!(fx.count("SELECT count(*) FROM vm_templates"), 1);
        assert_eq!(
            fx.count("SELECT sum(staging_bytes) FROM vm_build_intents"),
            0
        );
        assert_eq!(
            std::fs::read(fx.slot_file(first.slots.pair.disk)).unwrap(),
            golden
        );
        assert!(!exists(
            &fx.gen_dir(interrupted.slots.verification.disk.generation)
        ));
        assert!(!exists(&fx.gen_dir(interrupted.slots.pair.disk.generation)));
    }
}

/// T17/T30/T32: a restart after a mid-stage fault reconciles the identified
/// mutable slots and the recorded writer origins through restart recovery,
/// then releases the peak claim without a seal.
#[tokio::test]
async fn build_mid_stage_faults_restart_through_native_recovery() {
    for fault in [Fault::VarsWrite, Fault::Setup, Fault::VerificationTeardown] {
        assert_admission_closed();
        let mut fx = SweepFixture::new_with_store(true);
        let interrupted = run(&fx, true, fault, 55).await;
        assert!(interrupted.status.is_err(), "{fault:?}");
        assert_eq!(
            fx.count("SELECT sum(staging_bytes) FROM vm_build_intents"),
            interrupted.claim,
            "{fault:?}"
        );
        // Only a pair past its final writable release has a durable digest:
        // the primary pair once verification started, never the overlay pair.
        let primary_verified = i64::from(fault == Fault::VerificationTeardown);
        assert_eq!(
            fx.count(
                "SELECT count(*) FROM vm_native_slots WHERE content_digest IS NOT NULL
                 AND role IN ('disk','vars','verification_overlay')"
            ),
            2 * primary_verified,
            "{fault:?}"
        );
        assert_eq!(
            fx.count(
                "SELECT count(*) FROM vm_native_slots WHERE content_digest IS NOT NULL
                 AND role='disk'"
            ),
            primary_verified,
            "{fault:?}"
        );
        fx.reopen();
        let guards = GenerationGuards::default();
        let custody =
            NativeRecovery::open_with_owner(fx.store(), fx.state_root(), fx.owner_sid(), &guards)
                .unwrap();
        let runtime = RestartReaper {
            owned: false,
            can_fence: true,
            retries: Cell::new(0),
        };
        for _ in 0..6 {
            fx.clock.advance(60_000, 61);
            fx.ctl().sweep_with(&runtime, &custody).unwrap();
        }
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_templates"),
            0,
            "{fault:?}"
        );
        assert_eq!(
            fx.count("SELECT count(*) FROM vm_build_intents"),
            0,
            "{fault:?}"
        );
        for generation in [
            interrupted.slots.pair.disk.generation,
            interrupted.slots.verification.disk.generation,
        ] {
            assert!(!exists(&fx.gen_dir(generation)), "{fault:?}");
        }
    }
}

#[tokio::test]
async fn build_replay_and_new_request_reuse_the_exact_sealed_pair() {
    assert_admission_closed();
    let fx = SweepFixture::new();
    let first = run(&fx, false, Fault::None, 44).await;
    assert_eq!(first.status.unwrap(), Status::Succeeded);
    let source = FakeSource::new(fx.clock.now().monotonic);
    let recipe = (&source).recipes().ubuntu;
    let name = hex::encode(recipe.recipe_id);
    let replay = fx
        .ctl()
        .admit_build(
            request(&name, recipe.recipe_sha256, 44),
            host(),
            resources(),
            |_, _| panic!("request replay must not probe a golden"),
        )
        .unwrap()
        .unwrap();
    let BuildAdmission::Existing(replay) = replay else {
        panic!("expected original receipt")
    };
    let pair = first.slots.pair;
    let native::disposal::GenerationState::Bound { scope, .. } = fx
        .store()
        .transact(|db| native::disposal::generation_state(db, pair.disk.generation))
        .unwrap()
    else {
        panic!("sealed workspace remains bound")
    };
    let workspace = hold_workspace_under(
        &fx.production,
        fx.owner_sid().0,
        super::super::disposal::binding_from_scope(scope),
    )
    .unwrap();
    let pins = [pair.disk, pair.vars].map(|slot| {
        PathPin::open(
            &slot_path(&workspace.root.path, slot).unwrap(),
            false,
            false,
        )
        .unwrap()
    });
    let evidence = pins.each_ref().map(|pin| ArtifactEvidence {
        identity: pin.identity,
        directory: scope.workspace.file,
        pin: launch::hash_held(
            &pin.leaf,
            IMAGES,
            Instant::now() + Duration::from_secs(5),
            &|| false,
        )
        .unwrap(),
    });
    let reuse = fx
        .ctl()
        .admit_build(
            request(&name, recipe.recipe_sha256, 45),
            host(),
            resources(),
            |_, slots| {
                assert_eq!(slots, pair);
                Some(evidence)
            },
        )
        .unwrap()
        .unwrap();
    let BuildAdmission::Existing(reuse) = reuse else {
        panic!("expected verified reuse")
    };
    assert_ne!(reuse.id, replay.id);
    assert_eq!(reuse.receipt.status, Status::Succeeded);
    assert_eq!(fx.count("SELECT count(*) FROM vm_templates"), 1);
    assert_eq!(fx.count("SELECT count(*) FROM vm_generations"), 1);
}

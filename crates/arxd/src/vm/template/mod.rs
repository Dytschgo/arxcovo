mod media;
mod ubuntu;
#[allow(
    dead_code,
    reason = "5c supplies the Windows lifecycle caller; native admission is closed"
)]
mod windows;

pub use ubuntu::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};

#[allow(
    dead_code,
    reason = "4c supplies the durable lifecycle caller; native admission is closed"
)]
mod lifecycle {
    use super::{RecipeError, UbuntuAssets, UbuntuRecipe, ubuntu::DETACHED_HEALTH};
    use crate::vm::{
        Admission, Clock, Completion, DiskClaim, Guest, HostObservation, Id, Operation, Receipt,
        Refusal, Request, Resources, SealedTemplate, Status, VmController, VmPolicy,
        paths::{ArtifactPin, FileIdentity},
        runtime::{CandidateRuntime, NativeRefusal, StopDisposition},
    };
    use arx_guest::protocol::ScriptStatus;
    use sha2::{Digest, Sha256};
    use std::{
        sync::{Mutex, MutexGuard, TryLockError},
        time::Duration,
    };

    const HEALTH_TIMEOUT_S: u32 = 30;
    const INSTALL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

    struct Seed(Vec<u8>);

    impl Seed {
        fn prepare(
            recipe: &UbuntuRecipe,
            assets: UbuntuAssets<'_>,
            id: Id,
            guest: &[u8],
        ) -> Result<Self, RecipeError> {
            let mut bytes = Vec::new();
            recipe.write_seed(assets, id, guest, &mut bytes)?;
            Ok(Self(bytes))
        }

        fn pin(&self) -> ArtifactPin {
            ArtifactPin {
                size: self.0.len() as u64,
                sha256: Sha256::digest(&self.0).into(),
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct PinnedFile {
        identity: FileIdentity,
        bytes: ArtifactPin,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Pair {
        disk: PinnedFile,
        vars: PinnedFile,
    }

    impl Pair {
        fn fits(self, images: u64) -> bool {
            self.disk.bytes.size > 0
                && self.vars.bytes.size > 0
                && self.disk.identity != self.vars.identity
                && self
                    .disk
                    .bytes
                    .size
                    .checked_add(self.vars.bytes.size)
                    .is_some_and(|size| size <= images)
        }
    }

    #[derive(Debug)]
    struct Golden {
        id: Id,
        files: Pair,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum LinkedSlot {
        Verification(Id),
        Clone(Id),
    }

    #[derive(Debug)]
    struct Linked {
        slot: LinkedSlot,
        files: Pair,
        backing: PinnedFile,
    }

    impl Linked {
        fn matches(&self, golden: &Golden, slot: LinkedSlot, images: u64) -> bool {
            self.slot == slot
                && self.backing == golden.files.disk
                && self.files.fits(images)
                && self.files.vars.bytes == golden.files.vars.bytes
                && [self.files.disk.identity, self.files.vars.identity]
                    .iter()
                    .all(|id| {
                        *id != golden.files.disk.identity && *id != golden.files.vars.identity
                    })
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Boot(Id);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Readiness {
        ScriptChannel,
        PingOnly,
        Timeout,
        WrongPeer,
    }

    #[derive(Debug)]
    enum Failure {
        Native(NativeRefusal),
        Refused(Refusal),
        Store(anyhow::Error),
        Seed(RecipeError),
        Media,
        ChangedPair,
        ReusedBoot,
        Readiness(Readiness),
        Health,
        Stop(StopDisposition),
        CloneInterrupted,
    }

    /// Trust boundary for storage and process effects. Implementations must
    /// establish custody, observe fixed budgets, and retain resources if an
    /// operation fails without proving release. No adapter issues a native lease.
    trait TemplateRuntime {
        /// Create fresh backing-less disk, private VARS and read-only CIDATA
        /// in the reserved generation, under the supplied byte limits. Return
        /// the CIDATA pin from a bounded reread through its custody handle.
        fn stage(&mut self, spec: &BuildSpec, seed: &[u8]) -> Result<ArtifactPin, Failure>;
        /// Attach the installer and CIDATA, then observe poweroff and release.
        fn install(
            &mut self,
            id: Id,
            timeout: Duration,
        ) -> Result<(Boot, StopDisposition), Failure>;
        /// Reopen the backing-less candidate at its final generation paths and
        /// verify the image, role, identity, size and digest of both files.
        fn capture_golden(&mut self, id: Id, images: u64) -> Result<Pair, Failure>;
        /// Create a fresh overlay with explicit qcow2 backing at the pinned
        /// golden disk and copy VARS into a distinct writable file. Return
        /// the verified immediate backing, with no other chain or data file.
        fn link(
            &mut self,
            golden: &Golden,
            slot: LinkedSlot,
            images: u64,
        ) -> Result<Linked, Failure>;
        /// Fresh child, serial endpoint and installed boot with only this pair.
        fn boot_detached(&mut self, pair: &Linked) -> Result<Boot, Failure>;
        /// ScriptChannel requires verified peer custody and a boot-local v1
        /// Hello echo advertising script support within GUEST_READY_BUDGET.
        fn ready(&mut self, boot: Boot) -> Result<Readiness, Failure>;
        fn health(
            &mut self,
            boot: Boot,
            script: &str,
            seconds: u32,
        ) -> Result<(ScriptStatus, Option<u32>), Failure>;
        fn stop(&mut self, boot: Boot) -> StopDisposition;
        /// Remove only the attributable, released CIDATA and verification pair.
        /// Keep the captured golden pair protected at its final paths.
        fn discard_transients(&mut self, id: Id) -> Result<(), Failure>;
    }

    enum Installer<'a> {
        Candidate(&'a CandidateRuntime<'a>),
        #[cfg(test)]
        Fixture(&'a mut dyn TemplateRuntime),
    }

    impl<'a> Installer<'a> {
        fn admit(self) -> Result<&'a mut dyn TemplateRuntime, Failure> {
            match self {
                Self::Candidate(candidate) => candidate
                    .template_installer()
                    .map(|closed| match closed {})
                    .map_err(Failure::Native),
                #[cfg(test)]
                Self::Fixture(runtime) => Ok(runtime),
            }
        }
    }

    struct BuildSpec {
        id: Id,
        recipe_digest: [u8; 32],
        golden: DiskClaim,
        verification: DiskClaim,
    }

    impl BuildSpec {
        fn peak(&self) -> Result<u64, Failure> {
            if self.golden.images == 0 || self.golden.staging != 0 || self.verification.images == 0
            {
                return Err(Failure::Refused(Refusal::Invalid));
            }
            self.golden
                .total()
                .and_then(|golden| golden.checked_add(self.verification.total()?))
                .filter(|peak| i64::try_from(*peak).is_ok())
                .ok_or(Failure::Refused(Refusal::Disk))
        }
    }

    // Share this gate among all slots served by one lifecycle. Once writes
    // begin, keep the build charged on every incomplete outcome, including
    // unwinding. 4c must preserve and reconcile that charge after restart.
    #[derive(Default)]
    struct Writer(Mutex<Option<(Id, u64)>>);

    impl Writer {
        fn acquire(&self) -> Result<MutexGuard<'_, Option<(Id, u64)>>, Failure> {
            match self.0.try_lock() {
                Ok(guard) if guard.is_none() => Ok(guard),
                Ok(_) | Err(TryLockError::WouldBlock) => Err(Failure::Refused(Refusal::Busy)),
                Err(TryLockError::Poisoned(_)) => Err(Failure::Refused(Refusal::Busy)),
            }
        }
    }

    struct Lifecycle<'a, C> {
        controller: &'a VmController<C>,
        writer: &'a Writer,
        policy: VmPolicy,
        host: HostObservation,
        resources: Resources,
    }

    #[derive(Debug)]
    enum CloneResult {
        Created(Linked),
        Existing(Receipt),
    }

    impl<C: Clock> Lifecycle<'_, C> {
        fn build(
            &self,
            installer: Installer<'_>,
            spec: &BuildSpec,
            prepare: impl FnOnce() -> Result<Seed, RecipeError>,
        ) -> Result<Golden, Failure> {
            let runtime = installer.admit()?;
            let mut charge = self.writer.acquire()?;
            let peak = spec.peak()?;
            let capacity = self
                .policy
                .host(self.host, self.resources)
                .map_err(Failure::Refused)?;
            // The controller does not yet expose a ledger-inclusive build
            // admission. This is the local peak check against observed bytes.
            self.policy
                .disk(capacity, 0, peak)
                .map_err(Failure::Refused)?;
            *charge = Some((spec.id, peak));
            let seed = prepare().map_err(Failure::Seed).and_then(|seed| {
                if seed.0.len() as u64 > spec.verification.staging {
                    return Err(Failure::Refused(Refusal::Disk));
                }
                Ok(seed)
            });
            if seed.is_err() {
                *charge = None;
            }
            let seed = seed?;
            if runtime.stage(spec, &seed.0)? != seed.pin() {
                return Err(Failure::Media);
            }
            let (installed, stop) = runtime.install(spec.id, INSTALL_TIMEOUT)?;
            clean(stop)?;

            let files = runtime.capture_golden(spec.id, spec.golden.images)?;
            if !files.fits(spec.golden.images) {
                return Err(Failure::Media);
            }
            let golden = Golden { id: spec.id, files };
            let slot = LinkedSlot::Verification(spec.id);
            let linked = runtime.link(&golden, slot, spec.verification.images)?;
            if !linked.matches(&golden, slot, spec.verification.images) {
                return Err(Failure::Media);
            }
            verify_detached(runtime, &linked, installed)?;
            runtime.discard_transients(spec.id)?;
            unchanged(runtime, &golden, spec.golden.images)?;
            self.controller
                .seal_template(
                    SealedTemplate {
                        id: golden.id,
                        guest: Guest::UbuntuX86_64,
                        recipe_digest: spec.recipe_digest,
                        disk_digest: files.disk.bytes.sha256,
                        vars_digest: files.vars.bytes.sha256,
                        disk: spec.golden,
                    },
                    self.host,
                    self.resources,
                )
                .map_err(Failure::Store)?
                .map_err(Failure::Refused)?;
            *charge = None;
            Ok(golden)
        }

        fn linked_clone(
            &self,
            runtime: &mut dyn TemplateRuntime,
            golden: &Golden,
            request: Request<'_>,
        ) -> Result<CloneResult, Failure> {
            if !matches!(request.operation, Operation::Clone { template, .. } if template == golden.id)
            {
                return Err(Failure::Refused(Refusal::Invalid));
            }
            let _writer = self.writer.acquire()?;
            let slot = LinkedSlot::Clone(request.vm);
            let images = request.disk.images;
            let pending = match self
                .controller
                .admit(request, self.host, self.resources)
                .map_err(Failure::Store)?
            {
                Admission::Admitted(pending) => pending,
                Admission::Existing(receipt) => return Ok(CloneResult::Existing(receipt)),
                Admission::Refused(reason) => return Err(Failure::Refused(reason)),
            };
            let linked = (|| {
                let golden_bytes = golden.files.disk.bytes.size + golden.files.vars.bytes.size;
                unchanged(runtime, golden, golden_bytes)?;
                let pair = runtime.link(golden, slot, images)?;
                if !pair.matches(golden, slot, images) {
                    return Err(Failure::Media);
                }
                unchanged(runtime, golden, golden_bytes)?;
                Ok(pair)
            })();
            let completion = if linked.is_ok() {
                Completion::Succeeded
            } else {
                Completion::CleanupUnconfirmed
            };
            let status = self
                .controller
                .finish(&pending, completion)
                .map_err(Failure::Store)?;
            let pair = linked?;
            if status != Status::Succeeded {
                return Err(Failure::CloneInterrupted);
            }
            Ok(CloneResult::Created(pair))
        }
    }

    fn clean(stop: StopDisposition) -> Result<(), Failure> {
        if stop != StopDisposition::CleanRelease {
            return Err(Failure::Stop(stop));
        }
        Ok(())
    }

    fn unchanged(
        runtime: &mut dyn TemplateRuntime,
        golden: &Golden,
        images: u64,
    ) -> Result<(), Failure> {
        if runtime.capture_golden(golden.id, images)? != golden.files {
            return Err(Failure::ChangedPair);
        }
        Ok(())
    }

    fn verify_detached(
        runtime: &mut dyn TemplateRuntime,
        pair: &Linked,
        installed: Boot,
    ) -> Result<(), Failure> {
        let boot = runtime.boot_detached(pair)?;
        let checked = (|| {
            if boot == installed {
                return Err(Failure::ReusedBoot);
            }
            let ready = runtime.ready(boot)?;
            if ready != Readiness::ScriptChannel {
                return Err(Failure::Readiness(ready));
            }
            if runtime.health(boot, DETACHED_HEALTH, HEALTH_TIMEOUT_S)?
                != (ScriptStatus::Exited, Some(0))
            {
                return Err(Failure::Health);
            }
            Ok(())
        })();
        clean(runtime.stop(boot))?;
        checked
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{
            service::application::tests::private_temp,
            vm::{Capacity, ClockReading, RequestId},
        };
        use rusqlite::Connection;
        use std::{
            fs,
            io::Write,
            path::{Path, PathBuf},
            time::Instant,
        };

        const BUILD: Id = [1; 16];
        const INSTANCE: Id = [2; 16];
        const INSTALL: Boot = Boot([3; 16]);
        const DETACHED: Boot = Boot([4; 16]);
        const MIB: u64 = 1024 * 1024;
        const GIB: u64 = 1024 * MIB;
        const NOW: i64 = 1_800_000_000_000;

        struct TestClock(Instant);

        impl Clock for TestClock {
            fn now(&self) -> ClockReading {
                ClockReading {
                    utc_ms: NOW,
                    monotonic: self.0,
                }
            }
        }

        struct Fixture {
            controller: VmController<TestClock>,
            root: tempfile::TempDir,
            writer: Writer,
        }

        impl Fixture {
            fn new() -> Self {
                let root = private_temp();
                let controller =
                    VmController::open(root.path(), VmPolicy::default(), TestClock(Instant::now()))
                        .unwrap();
                Self {
                    controller,
                    root,
                    writer: Writer::default(),
                }
            }

            fn lifecycle(&self) -> Lifecycle<'_, TestClock> {
                Lifecycle {
                    controller: &self.controller,
                    writer: &self.writer,
                    policy: VmPolicy::default(),
                    host: HostObservation {
                        owner_idle: Some(true),
                        logical_cpus: Some(8),
                        available_ram: Some(32 * GIB),
                        capacity: Some(Capacity {
                            free_bytes: 400 * GIB,
                            managed_bytes: 0,
                        }),
                    },
                    resources: Resources {
                        vcpus: 2,
                        guest_ram: 4 * GIB,
                        overhead_ram: 256 * MIB,
                    },
                }
            }

            fn runtime(&self) -> Recording<'_> {
                Recording {
                    root: self.root.path(),
                    calls: Vec::new(),
                    live: None,
                    fault: None,
                    contend: None,
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Fault {
            PartialStage,
            SeedMismatch,
            Capture,
            InstallerStop(StopDisposition),
            ReuseChild,
            Readiness(Readiness),
            Health(ScriptStatus, Option<u32>),
            Stop(StopDisposition),
            GoldenWrite,
            WrongBacking,
            SharedVars,
            CrashAfterCloneWrite,
        }

        struct Recording<'a> {
            root: &'a Path,
            calls: Vec<&'static str>,
            live: Option<Boot>,
            fault: Option<Fault>,
            contend: Option<&'a Fixture>,
        }

        fn db(root: &Path) -> Connection {
            Connection::open(root.join("activity.sqlite3")).unwrap()
        }

        fn scalar(root: &Path, query: &str) -> i64 {
            db(root).query_row(query, [], |row| row.get(0)).unwrap()
        }

        fn create(path: &Path, bytes: &[u8]) {
            fs::File::create_new(path)
                .unwrap()
                .write_all(bytes)
                .unwrap();
        }

        fn file(path: &Path, id: u8) -> PinnedFile {
            let bytes = fs::read(path).unwrap();
            PinnedFile {
                identity: FileIdentity {
                    volume: 1,
                    file: [id; 16],
                },
                bytes: ArtifactPin {
                    size: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).into(),
                },
            }
        }

        impl Recording<'_> {
            fn path(&self, name: &str) -> PathBuf {
                self.root.join(name)
            }

            fn build(&mut self, f: &Fixture) -> Result<Golden, Failure> {
                f.lifecycle().build(Installer::Fixture(self), &spec(), seed)
            }
        }

        impl TemplateRuntime for Recording<'_> {
            fn stage(&mut self, spec: &BuildSpec, seed: &[u8]) -> Result<ArtifactPin, Failure> {
                self.calls.push("stage");
                assert_eq!(spec.id, BUILD);
                assert_eq!(&seed[16 * 2048 + 40..16 * 2048 + 46], b"CIDATA");
                create(&self.path("seed.iso"), seed);
                if self.fault == Some(Fault::PartialStage) {
                    return Err(Failure::Media);
                }
                create(&self.path("golden.disk"), b"empty fixture disk");
                create(&self.path("golden.vars"), b"firmware fixture");
                if self.fault == Some(Fault::SeedMismatch) {
                    fs::write(self.path("seed.iso"), b"truncated seed").unwrap();
                }
                Ok(file(&self.path("seed.iso"), 7).bytes)
            }

            fn install(
                &mut self,
                id: Id,
                timeout: Duration,
            ) -> Result<(Boot, StopDisposition), Failure> {
                assert_eq!(id, BUILD);
                assert_eq!(timeout.as_secs(), 3600);
                self.calls.push("installer boot");
                assert!(self.path("seed.iso").exists());
                assert!(self.live.replace(INSTALL).is_none());
                if let Some(f) = self.contend {
                    let mut second_slot = f.runtime();
                    let mut second_spec = spec();
                    second_spec.id = INSTANCE;
                    assert!(matches!(
                        f.lifecycle().build(
                            Installer::Fixture(&mut second_slot),
                            &second_spec,
                            || panic!("second writer prepared a seed")
                        ),
                        Err(Failure::Refused(Refusal::Busy))
                    ));
                    assert!(second_slot.calls.is_empty());
                }
                fs::write(self.path("golden.disk"), b"installed fixture disk").unwrap();
                self.calls.push("installer release");
                let stop = match self.fault {
                    Some(Fault::InstallerStop(stop)) => stop,
                    _ => StopDisposition::CleanRelease,
                };
                if stop != StopDisposition::ChargedQuarantine {
                    assert_eq!(self.live.take(), Some(INSTALL));
                }
                Ok((INSTALL, stop))
            }

            fn capture_golden(&mut self, id: Id, images: u64) -> Result<Pair, Failure> {
                assert_eq!(id, BUILD);
                assert!(self.live.is_none());
                self.calls.push("capture golden");
                if self.fault == Some(Fault::Capture) {
                    return Err(Failure::Media);
                }
                let pair = Pair {
                    disk: file(&self.path("golden.disk"), 1),
                    vars: file(&self.path("golden.vars"), 2),
                };
                assert!(pair.fits(images));
                Ok(pair)
            }

            fn link(
                &mut self,
                golden: &Golden,
                slot: LinkedSlot,
                _images: u64,
            ) -> Result<Linked, Failure> {
                assert_eq!(golden.id, BUILD);
                assert!(self.live.is_none());
                let (name, disk_id, vars_id) = match slot {
                    LinkedSlot::Verification(BUILD) => ("verify", 3, 4),
                    LinkedSlot::Clone(INSTANCE) => {
                        assert_eq!(scalar(self.root, "SELECT count(*) FROM vm_disk_claims"), 1);
                        ("clone", 5, 6)
                    }
                    _ => panic!("unexpected fixture generation"),
                };
                self.calls.push("link");
                let disk = self.path(&format!("{name}.disk"));
                let vars = self.path(&format!("{name}.vars"));
                let backing_path = self.path("golden.disk");
                // This miniature overlay records the explicit backing name.
                create(&disk, backing_path.to_str().unwrap().as_bytes());
                create(&vars, &fs::read(self.path("golden.vars")).unwrap());
                assert_ne!(
                    self.fault,
                    Some(Fault::CrashAfterCloneWrite),
                    "injected clone interruption"
                );
                let mut pair = Linked {
                    slot,
                    files: Pair {
                        disk: file(&disk, disk_id),
                        vars: file(&vars, vars_id),
                    },
                    backing: file(&backing_path, 1),
                };
                if self.fault == Some(Fault::WrongBacking) {
                    pair.backing = pair.files.disk;
                }
                if self.fault == Some(Fault::SharedVars) {
                    pair.files.vars = golden.files.vars;
                }
                Ok(pair)
            }

            fn boot_detached(&mut self, pair: &Linked) -> Result<Boot, Failure> {
                assert_eq!(pair.slot, LinkedSlot::Verification(BUILD));
                self.calls.push("detached boot");
                let boot = if self.fault == Some(Fault::ReuseChild) {
                    INSTALL
                } else {
                    DETACHED
                };
                assert!(self.live.replace(boot).is_none());
                fs::write(self.path("verify.vars"), b"verification firmware writes").unwrap();
                if self.fault == Some(Fault::GoldenWrite) {
                    fs::write(self.path("golden.vars"), b"unexpected golden write").unwrap();
                }
                Ok(boot)
            }

            fn ready(&mut self, boot: Boot) -> Result<Readiness, Failure> {
                self.calls.push("ready");
                assert_eq!(self.live, Some(DETACHED));
                assert_eq!(boot, DETACHED);
                Ok(match self.fault {
                    Some(Fault::Readiness(readiness)) => readiness,
                    _ => Readiness::ScriptChannel,
                })
            }

            fn health(
                &mut self,
                boot: Boot,
                script: &str,
                seconds: u32,
            ) -> Result<(ScriptStatus, Option<u32>), Failure> {
                self.calls.push("health");
                assert_eq!(boot, DETACHED);
                assert_eq!(self.live, Some(boot));
                assert_eq!(script, DETACHED_HEALTH);
                assert!(super::super::ubuntu::health_uses_public_paths(script));
                assert!(script.contains("verify_account_result /proc/sys/kernel/random/boot_id /run/arx-account-audit/result"));
                assert_eq!(seconds, 30);
                Ok(match self.fault {
                    Some(Fault::Health(status, exit_code)) => (status, exit_code),
                    _ => (ScriptStatus::Exited, Some(0)),
                })
            }

            fn stop(&mut self, boot: Boot) -> StopDisposition {
                self.calls.push("graceful stop");
                assert_eq!(scalar(self.root, "SELECT count(*) FROM vm_templates"), 0);
                assert_eq!(self.live, Some(boot));
                let stop = match self.fault {
                    Some(Fault::Stop(stop)) => stop,
                    _ => StopDisposition::CleanRelease,
                };
                if stop != StopDisposition::ChargedQuarantine {
                    self.live = None;
                }
                stop
            }

            fn discard_transients(&mut self, id: Id) -> Result<(), Failure> {
                assert_eq!(id, BUILD);
                assert!(self.live.is_none());
                assert_eq!(self.calls.last(), Some(&"graceful stop"));
                self.calls.push("discard transients");
                for name in ["seed.iso", "verify.disk", "verify.vars"] {
                    fs::remove_file(self.path(name)).unwrap();
                }
                Ok(())
            }
        }

        fn spec() -> BuildSpec {
            BuildSpec {
                id: BUILD,
                recipe_digest: [9; 32],
                golden: DiskClaim {
                    images: MIB,
                    metadata: MIB,
                    staging: 0,
                },
                verification: DiskClaim {
                    images: MIB,
                    metadata: MIB,
                    staging: MIB,
                },
            }
        }

        fn seed() -> Result<Seed, RecipeError> {
            use super::super::AssetPin;
            fn pin(bytes: &[u8]) -> AssetPin {
                AssetPin {
                    length: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).into(),
                }
            }
            let guest = b"fixture guest";
            let recipe = UbuntuRecipe {
                guest: Some(pin(guest)),
                service: Some(pin(UbuntuRecipe::SERVICE)),
                finalize: Some(pin(UbuntuRecipe::FINALIZE)),
                nocloud_schema: Some(UbuntuRecipe::NOCLOUD_PROFILE_PIN),
                ..UbuntuRecipe::default()
            };
            // The fixture uses the same CIDATA encoder without asserting
            // qualification of an owner ISO that the test does not have.
            recipe.render_seed(BUILD, guest).map(Seed)
        }

        fn clone_request() -> Request<'static> {
            Request {
                principal: "fixture",
                request_id: RequestId::parse(&format!("{NOW:016x}{:016x}", 1_u64)).unwrap(),
                vm: INSTANCE,
                operation: Operation::Clone {
                    template: BUILD,
                    lease_seconds: None,
                },
                disk: DiskClaim {
                    images: MIB,
                    metadata: 0,
                    staging: MIB,
                },
            }
        }

        #[test]
        fn detached_lifecycle_and_clone_preserve_the_sealed_pair() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            rt.contend = Some(&f);
            let golden = rt.build(&f).unwrap();
            assert_eq!(
                rt.calls,
                [
                    "stage",
                    "installer boot",
                    "installer release",
                    "capture golden",
                    "link",
                    "detached boot",
                    "ready",
                    "health",
                    "graceful stop",
                    "discard transients",
                    "capture golden"
                ]
            );
            let hashes: (Vec<u8>, Vec<u8>) = db(f.root.path())
                .query_row(
                    "SELECT disk_digest,vars_digest FROM vm_templates",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(hashes.0, golden.files.disk.bytes.sha256);
            assert_eq!(hashes.1, golden.files.vars.bytes.sha256);
            assert!(f.writer.0.lock().unwrap().is_none());
            assert!(!rt.path("seed.iso").exists());
            assert!(!rt.path("verify.disk").exists());
            assert!(!rt.path("verify.vars").exists());

            let CloneResult::Created(clone) = f
                .lifecycle()
                .linked_clone(&mut rt, &golden, clone_request())
                .unwrap()
            else {
                panic!("fresh clone was not created")
            };
            assert_eq!(clone.backing, golden.files.disk);
            assert_eq!(
                fs::read_to_string(rt.path("clone.disk")).unwrap(),
                rt.path("golden.disk").to_str().unwrap()
            );
            assert_ne!(clone.files.vars.identity, golden.files.vars.identity);
            assert_eq!(
                fs::read(rt.path("clone.vars")).unwrap(),
                fs::read(rt.path("golden.vars")).unwrap()
            );
            fs::write(rt.path("clone.disk"), b"clone disk writes").unwrap();
            fs::write(rt.path("clone.vars"), b"clone firmware writes").unwrap();
            assert_eq!(rt.capture_golden(BUILD, MIB).unwrap(), golden.files);
            let calls = rt.calls.len();
            assert!(matches!(
                f.lifecycle()
                    .linked_clone(&mut rt, &golden, clone_request()),
                Ok(CloneResult::Existing(Receipt {
                    status: Status::Succeeded,
                    ..
                }))
            ));
            assert_eq!(rt.calls.len(), calls);
        }

        #[test]
        fn candidate_refuses_installer_with_zero_effects() {
            let f = Fixture::new();
            let candidate = CandidateRuntime::new(None);
            let lifecycle = f.lifecycle();
            let before = fs::read_dir(f.root.path()).unwrap().count();
            // Holding the writer also proves the refusal precedes reservation.
            let writer = f.writer.acquire().unwrap();
            assert!(matches!(
                lifecycle.build(Installer::Candidate(&candidate), &spec(), || panic!(
                    "prepared"
                )),
                Err(Failure::Native(NativeRefusal::InstallerPhaseUnavailable))
            ));
            assert!(writer.is_none());
            assert_eq!(fs::read_dir(f.root.path()).unwrap().count(), before);
            assert_eq!(scalar(f.root.path(), "SELECT count(*) FROM activity"), 0);
        }

        #[test]
        fn peak_refusal_and_failed_seed_preparation_allow_clean_retry() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            let mut lifecycle = f.lifecycle();
            lifecycle.host.capacity.as_mut().unwrap().free_bytes = 50 * GIB + 13 * MIB;
            assert!(matches!(
                lifecycle.build(Installer::Fixture(&mut rt), &spec(), || panic!("prepared")),
                Err(Failure::Refused(Refusal::Disk))
            ));
            assert!(matches!(
                f.lifecycle()
                    .build(Installer::Fixture(&mut rt), &spec(), || Seed::prepare(
                        &UbuntuRecipe::default(),
                        UbuntuAssets::default(),
                        BUILD,
                        b"guest"
                    )),
                Err(Failure::Seed(RecipeError::Unqualified))
            ));
            assert!(rt.calls.is_empty());
            assert!(f.writer.0.lock().unwrap().is_none());
            rt.build(&f).unwrap();
        }

        #[test]
        fn readiness_and_verification_fail_closed() {
            for fault in [
                Fault::ReuseChild,
                Fault::Readiness(Readiness::PingOnly),
                Fault::Readiness(Readiness::Timeout),
                Fault::Readiness(Readiness::WrongPeer),
                Fault::Health(ScriptStatus::Exited, Some(1)),
                Fault::Health(ScriptStatus::TimedOut, None),
                Fault::Stop(StopDisposition::DirtyRelease),
                Fault::Stop(StopDisposition::ChargedQuarantine),
                Fault::GoldenWrite,
                Fault::WrongBacking,
                Fault::SharedVars,
                Fault::InstallerStop(StopDisposition::DirtyRelease),
                Fault::InstallerStop(StopDisposition::ChargedQuarantine),
            ] {
                let f = Fixture::new();
                let mut rt = f.runtime();
                rt.fault = Some(fault);
                let failure = rt.build(&f).unwrap_err();
                match (fault, failure) {
                    (Fault::ReuseChild, Failure::ReusedBoot) => {
                        assert!(!rt.calls.contains(&"ready"))
                    }
                    (Fault::Readiness(expected), Failure::Readiness(actual)) => {
                        assert_eq!(expected, actual);
                        assert!(!rt.calls.contains(&"health"));
                        assert_eq!(rt.calls.last(), Some(&"graceful stop"));
                    }
                    (Fault::Health(..), Failure::Health) => {
                        assert_eq!(rt.calls.last(), Some(&"graceful stop"))
                    }
                    (Fault::Stop(expected), Failure::Stop(actual)) => {
                        assert_eq!(expected, actual);
                        assert!(!rt.calls.contains(&"discard transients"));
                    }
                    (Fault::InstallerStop(expected), Failure::Stop(actual)) => {
                        assert_eq!(expected, actual);
                        assert!(!rt.calls.contains(&"detached boot"));
                    }
                    (Fault::GoldenWrite, Failure::ChangedPair) => {}
                    (Fault::WrongBacking | Fault::SharedVars, Failure::Media) => {
                        assert!(!rt.calls.contains(&"detached boot"));
                    }
                    pair => panic!("unexpected disposition {pair:?}"),
                }
                assert_eq!(
                    scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                    0
                );
                assert_eq!(
                    *f.writer.0.lock().unwrap(),
                    Some((BUILD, spec().peak().unwrap()))
                );
            }
        }

        #[test]
        fn interrupted_build_and_seal_rollback_retain_charge_without_publishing() {
            for fault in [
                Some(Fault::PartialStage),
                Some(Fault::SeedMismatch),
                Some(Fault::Capture),
                None,
            ] {
                let f = Fixture::new();
                let mut rt = f.runtime();
                rt.fault = fault;
                if fault.is_none() {
                    // Fail after the template insert, when recording the seal activity.
                    db(f.root.path())
                        .execute_batch(
                            "CREATE TRIGGER abort_seal BEFORE INSERT ON activity
                        BEGIN SELECT RAISE(ABORT, 'fixture seal rollback'); END;",
                        )
                        .unwrap();
                }
                let failure = rt.build(&f).unwrap_err();
                match (fault, failure) {
                    (Some(Fault::PartialStage | Fault::SeedMismatch), Failure::Media) => {
                        assert!(rt.path("seed.iso").exists());
                        assert_eq!(rt.calls, ["stage"]);
                    }
                    (Some(Fault::Capture), Failure::Media) => {
                        assert!(rt.path("golden.disk").exists())
                    }
                    (None, Failure::Store(error)) => {
                        assert!(error.to_string().contains("fixture seal rollback"))
                    }
                    pair => panic!("unexpected interruption {pair:?}"),
                }
                assert_eq!(
                    scalar(f.root.path(), "SELECT count(*) FROM vm_templates"),
                    0
                );
                assert_eq!(
                    *f.writer.0.lock().unwrap(),
                    Some((BUILD, spec().peak().unwrap()))
                );
                assert!(matches!(rt.build(&f), Err(Failure::Refused(Refusal::Busy))));
            }
        }

        #[test]
        fn failed_clone_keeps_durable_claim_and_never_replays_writes() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            let golden = rt.build(&f).unwrap();
            rt.fault = Some(Fault::WrongBacking);
            assert!(matches!(
                f.lifecycle()
                    .linked_clone(&mut rt, &golden, clone_request()),
                Err(Failure::Media)
            ));
            assert_eq!(
                scalar(
                    f.root.path(),
                    "SELECT sum(persistent + staging) FROM vm_disk_claims"
                ),
                (2 * MIB) as i64
            );
            assert_eq!(scalar(f.root.path(), "SELECT state FROM vm_instances"), 4);
            let calls = rt.calls.len();
            assert!(matches!(
                f.lifecycle()
                    .linked_clone(&mut rt, &golden, clone_request()),
                Ok(CloneResult::Existing(Receipt {
                    status: Status::Interrupted,
                    ..
                }))
            ));
            assert_eq!(rt.calls.len(), calls);
        }

        #[test]
        fn clone_crash_after_writes_retains_claim_across_reopen() {
            let f = Fixture::new();
            let mut rt = f.runtime();
            let golden = rt.build(&f).unwrap();
            rt.fault = Some(Fault::CrashAfterCloneWrite);
            let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                f.lifecycle()
                    .linked_clone(&mut rt, &golden, clone_request())
            }));
            assert!(interrupted.is_err());
            assert!(rt.path("clone.disk").exists());
            assert!(rt.path("clone.vars").exists());
            let Fixture {
                controller, root, ..
            } = f;
            drop(controller);
            let reopened = Fixture {
                controller: VmController::open(
                    root.path(),
                    VmPolicy::default(),
                    TestClock(Instant::now()),
                )
                .unwrap(),
                root,
                writer: Writer::default(),
            };
            assert_eq!(
                scalar(
                    reopened.root.path(),
                    "SELECT sum(persistent + staging) FROM vm_disk_claims"
                ),
                (2 * MIB) as i64
            );
            assert_eq!(
                scalar(reopened.root.path(), "SELECT state FROM vm_instances"),
                4
            );
            let mut fresh = reopened.runtime();
            assert!(matches!(
                reopened
                    .lifecycle()
                    .linked_clone(&mut fresh, &golden, clone_request()),
                Ok(CloneResult::Existing(Receipt {
                    status: Status::Interrupted,
                    ..
                }))
            ));
            assert!(fresh.calls.is_empty());
        }
    }
}

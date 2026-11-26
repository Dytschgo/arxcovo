use super::*;
use crate::service::Shutdown;
use crate::vm::paths::Artifact;
use crate::vm::qemu_args::{BootPurpose, BootRecipe, NETWORK, StaticNetwork};
use crate::vm::static_web::OriginPortLease;
use crate::vm::windows::qualification::{docker_trial as trial, static_compose};
use arx_guest::docker_trial::{self as wire, Challenge, TrialRequest, TrialResponse};
use records::{DockerDependencies, Hash, Observation, Record, Stage, StageResult, decode};
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;

const PACKAGES: Hash = [31; 32];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fault {
    None,
    Stage,
    Scope,
    UnknownCreate,
    WorkspaceLeftover,
    Dirty,
    Late,
    LateDrain,
    LateDispose,
    LateDisposeTrial,
    WrongStockExecutable,
    WrongStockFirmware,
    WrongPackages,
    ForeignGuest,
    WrongCompose,
    WrongOrigin,
    WrongTrustProfile,
    WrongFixture,
    MissingControls,
    ReplayedBoot,
    Sink,
}

#[derive(Default)]
struct Trace {
    staged: Vec<trial::Phase>,
    launched: Vec<trial::Phase>,
    drained: Vec<trial::Phase>,
    disposed: bool,
}

struct Host<'a> {
    dependencies: DockerDependencies,
    rows: Vec<Record>,
    fault: Fault,
    time: Cell<u64>,
    trace: &'a RefCell<Trace>,
    journal: LocalPath,
    orphan: LocalPath,
    recipes: &'a [BootRecipe<'a>; 4],
    medium: &'a Artifact,
}

struct Prepared<'a> {
    scope: trial::Scope,
    recipe: &'a BootRecipe<'a>,
    medium: Option<&'a Artifact>,
}

impl trial::Prepared for Prepared<'_> {
    fn scope(&self) -> trial::Scope {
        self.scope
    }

    fn recipe(&self) -> BootRecipe<'_> {
        BootRecipe { ..*self.recipe }
    }

    fn dependency_medium(&self) -> Option<&Artifact> {
        self.medium
    }
}

impl<'a> trial::Host for Host<'a> {
    type Prepared = Prepared<'a>;
    fn clock(&self) -> Instant2 {
        let mono_ms = self.time.get();
        self.time.set(mono_ms + 20);
        Instant2 {
            mono_ms,
            utc: ISSUE.utc + mono_ms / 1000,
        }
    }

    fn on_boot(&self) -> Result<(Instant2, Hash), NativeRefusal> {
        Ok((TEST_BOOT.on_boot(self.clock()).unwrap(), TEST_BOOT.marker))
    }

    fn measure(&mut self) -> Result<DockerDependencies, NativeRefusal> {
        Ok(self.dependencies.clone())
    }

    fn baseline(&mut self, stage: Stage) -> Result<Observation, NativeRefusal> {
        let row = self.rows.iter().find(|row| row.stage == stage).unwrap();
        let StageResult::Passed(mut observation) = row.result.clone() else {
            unreachable!()
        };
        if let Observation::ReserveActivity {
            controlled_faults,
            console_transitions_ms,
        } = &mut observation
        {
            let start = self.time.get() - 20;
            for item in controlled_faults.iter_mut() {
                item.sampled_ms = start + item.sampled_ms % 100;
                item.settled_ms = start + item.settled_ms % 100;
            }
            for timestamp in console_transitions_ms.iter_mut() {
                *timestamp = start + *timestamp % 100;
            }
        }
        Ok(observation)
    }

    fn headroom(&mut self) -> Result<u64, NativeRefusal> {
        Ok(200 << 30)
    }

    fn stage(&mut self, ticket: trial::Ticket, _: u64) -> Result<Self::Prepared, NativeRefusal> {
        // Read the durable journal before the fake peer stages any leaf.
        let reader = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(self.journal.as_str())
            .unwrap();
        let mut charge = Vec::new();
        reader.take(256).read_to_end(&mut charge).unwrap();
        let mut trace = self.trace.borrow_mut();
        let expected: Vec<u8> = trace
            .staged
            .iter()
            .copied()
            .chain([ticket.phase])
            .map(|phase| phase as u8)
            .collect();
        let header_bytes = b"ARXDOCKER1".len() + 16 + 32 + 8;
        assert_eq!(
            &charge[header_bytes..],
            expected,
            "stage intent must already be present"
        );
        trace.staged.push(ticket.phase);
        let mut scope = ticket.scope;
        if ticket.phase == trial::Phase::Install {
            match self.fault {
                Fault::Stage => {
                    std::fs::File::create_new(self.orphan.as_str()).unwrap();
                    return Err(NativeRefusal::PathCustodyUnverified);
                }
                Fault::Scope => scope.run[0] ^= 1,
                _ => {}
            }
        }
        Ok(Prepared {
            scope,
            recipe: &self.recipes[ticket.phase as usize],
            medium: (ticket.phase == trial::Phase::Install).then_some(self.medium),
        })
    }

    fn boot(
        &mut self,
        boot: &trial::CheckedBoot<'_>,
        _: u64,
        _: &Shutdown,
    ) -> Result<trial::PeerId, NativeRefusal> {
        let ticket = boot.ticket();
        let plan = boot.argv();
        self.trace.borrow_mut().launched.push(ticket.phase);
        let options = |flag| {
            plan.args()
                .windows(2)
                .filter_map(move |pair| (pair[0] == flag).then_some(pair[1].as_str()))
        };
        assert_eq!(options("-smp").collect::<Vec<_>>(), ["2"]);
        assert_eq!(options("-m").collect::<Vec<_>>(), ["4096"]);
        assert_eq!(options("-accel").collect::<Vec<_>>(), ["whpx"]);
        assert_eq!(plan.args().iter().filter(|arg| *arg == "-S").count(), 1);
        let blocks = options("-blockdev")
            .map(|arg| serde_json::from_str::<serde_json::Value>(arg).unwrap())
            .collect::<Vec<_>>();
        let block = blocks
            .iter()
            .find(|value| value["node-name"] == "guestdisk")
            .unwrap();
        assert!(block["backing"].is_null());
        assert_eq!(block["file"]["read-only"], false);
        let network = options("-netdev").collect::<Vec<_>>();
        match ticket.phase {
            trial::Phase::PublicFixture | trial::Phase::Endpoint => {
                assert_eq!(plan.kind(), PlanKind::PublicEgressBoot);
                assert!(plan.image().as_str().ends_with("patched.exe"));
                let policy = plan
                    .public_egress_policy()
                    .unwrap()
                    .path_for(ArtifactRole::PublicEgressPolicy)
                    .unwrap();
                assert_eq!(
                    network,
                    [format!(
                        "{},arx-public-egress={},hostfwd=tcp:127.0.0.1:{}-10.0.2.15:443",
                        crate::vm::qemu_args::PUBLIC_EGRESS_NETWORK,
                        policy.as_str(),
                        plan.static_origin_lease().unwrap().port(),
                    )]
                );
                assert_eq!(blocks.len(), 3);
            }
            trial::Phase::Install | trial::Phase::Detached => {
                assert!(plan.image().as_str().ends_with("qemu-system-x86_64.exe"));
                assert_eq!(network, [NETWORK]);
                if ticket.phase == trial::Phase::Install {
                    assert_eq!(plan.kind(), PlanKind::StaticComposeInstaller);
                    let medium = blocks
                        .iter()
                        .find(|value| value["node-name"] == "arxcompose")
                        .unwrap();
                    assert_eq!(medium["read-only"], true);
                    assert_eq!(medium["file"]["read-only"], true);
                    assert!(options("-chardev").any(|arg| arg == "null,id=arxguest"));
                } else {
                    assert_eq!(plan.kind(), PlanKind::StaticComposeDetached);
                    assert_eq!(blocks.len(), 3);
                }
            }
        }
        let cfg = options("-fw_cfg").collect::<Vec<_>>();
        match ticket.phase {
            trial::Phase::Detached => assert_eq!(
                cfg,
                [format!("name={},string={}", wire::FW_CFG, wire::DETACHED)]
            ),
            trial::Phase::Endpoint => assert_eq!(
                cfg,
                [format!("name={},string={}", wire::FW_CFG, wire::ENDPOINT)]
            ),
            _ => assert!(cfg.is_empty()),
        }
        if self.fault == Fault::UnknownCreate && ticket.phase == trial::Phase::Install {
            std::fs::File::create_new(self.orphan.as_str()).unwrap();
            return Err(NativeRefusal::JobUnverified);
        }
        Ok(peer(ticket.phase, self.fault))
    }

    fn drain(
        &mut self,
        ticket: trial::Ticket,
        cutoff: u64,
    ) -> Result<trial::Release, NativeRefusal> {
        self.trace.borrow_mut().drained.push(ticket.phase);
        if ticket.phase == trial::Phase::Endpoint {
            match self.fault {
                Fault::Late => self.time.set(records::MAX_TRIAL_MS + 10),
                Fault::LateDrain => self.time.set(cutoff + 1),
                _ => {}
            }
        }
        Ok(trial::Release {
            peer: peer(ticket.phase, self.fault),
            process_exited: true,
            empty_job: true,
            workers_joined: true,
            writers_excluded: true,
            clean_guest_shutdown: self.fault != Fault::Dirty
                || ticket.phase != trial::Phase::Install,
        })
    }

    fn dispose(
        &mut self,
        scope: trial::Scope,
        cutoff: u64,
    ) -> Result<trial::WorkspaceRelease, NativeRefusal> {
        self.trace.borrow_mut().disposed = true;
        match self.fault {
            Fault::WorkspaceLeftover => {
                std::fs::File::create_new(self.orphan.as_str()).unwrap();
            }
            Fault::LateDispose => self.time.set(cutoff + 1),
            Fault::LateDisposeTrial => self.time.set(records::MAX_TRIAL_MS + 10),
            _ => {}
        }
        Ok(trial::WorkspaceRelease {
            scope,
            owned_leaves_absent: true,
            helpers_joined: true,
        })
    }

    fn copies_independent_and_original_unchanged(&mut self) -> bool {
        true
    }
}

fn peer(phase: trial::Phase, fault: Fault) -> trial::PeerId {
    let boot = if fault == Fault::ReplayedBoot && phase == trial::Phase::Endpoint {
        1
    } else {
        phase as u8 + 1
    };
    trial::PeerId {
        pid: 100 + phase as u32,
        boot: [boot; 16],
    }
}

struct Guest {
    fault: Fault,
    challenge: Option<Challenge>,
}

impl trial::Guest for Guest {
    fn exchange(
        &mut self,
        mut peer: trial::PeerId,
        frame: Vec<u8>,
        _: u64,
    ) -> Result<trial::Reply, NativeRefusal> {
        let mut reply = Vec::new();
        let input = &mut frame.as_slice();
        match arx_guest::read_frame::<TrialRequest>(input) {
            Ok(Some(TrialRequest::DockerTrialHealth { challenge })) => {
                self.challenge = Some(challenge);
                arx_guest::write_frame(
                    &mut reply,
                    &TrialResponse::DockerTrialHealthy {
                        challenge,
                        packages: if self.fault == Fault::WrongPackages {
                            [99; 32]
                        } else {
                            PACKAGES
                        },
                    },
                )
                .unwrap();
            }
            Ok(Some(TrialRequest::DockerTrialProvision { challenge })) => {
                assert_eq!(self.challenge, Some(challenge));
                arx_guest::write_frame(
                    &mut reply,
                    &TrialResponse::DockerTrialProvisioned { challenge },
                )
                .unwrap();
            }
            _ => {
                use arx_guest::compose::{ComposeRequest, ComposeResponse};
                let request: ComposeRequest = arx_guest::read_frame(&mut frame.as_slice())
                    .unwrap()
                    .unwrap();
                assert!(request == wire::compose_hello(self.challenge.unwrap()));
                let ComposeRequest::ComposeHello {
                    version,
                    challenge,
                    capabilities,
                    mut generation,
                } = request
                else {
                    unreachable!()
                };
                if self.fault == Fault::WrongCompose {
                    // A fully valid Compose frame for a different boot.
                    generation += 1;
                }
                arx_guest::write_frame(
                    &mut reply,
                    &ComposeResponse::ComposeHelloAck {
                        version,
                        challenge,
                        generation,
                        capabilities,
                        cleared_staging: 0,
                        ambiguous_operations: 0,
                    },
                )
                .unwrap();
            }
        }
        if self.fault == Fault::ForeignGuest {
            peer.pid += 1;
        }
        Ok(trial::Reply { peer, frame: reply })
    }
}

struct Capture {
    fault: Fault,
    profile: Hash,
    policy: Hash,
    topology: Hash,
}

impl trial::Capture for Capture {
    fn fixture(
        &mut self,
        peer: trial::PeerId,
        mut challenge: Challenge,
        _: u64,
    ) -> Result<trial::FixtureProof, NativeRefusal> {
        if self.fault == Fault::WrongFixture {
            challenge.nonce[0] ^= 1;
        }
        Ok(trial::FixtureProof {
            peer,
            challenge,
            receiver_digest: [33; 32],
            policy: self.policy,
            topology: self.topology,
            receiver_controls: self.fault != Fault::MissingControls,
            public_tcp_udp: true,
            nonpublic_denied: true,
            ipv6_icmp_denied: true,
            route_drift_denied: true,
        })
    }

    fn origin(
        &mut self,
        peer: trial::PeerId,
        challenge: Challenge,
        port: u16,
        _: u64,
    ) -> Result<trial::OriginProof, NativeRefusal> {
        Ok(trial::OriginProof {
            peer,
            challenge,
            port: if self.fault == Fault::WrongOrigin {
                port ^ 1
            } else {
                port
            },
            tls_profile: if self.fault == Fault::WrongTrustProfile {
                [99; 32]
            } else {
                self.profile
            },
        })
    }
}

fn collect(fault: Fault) {
    use ArtifactRole::*;
    use trial::Phase::*;
    let dir = tempfile::tempdir().unwrap();
    let (output, evidence) = claimed(dir.path(), &[]);
    let (owner, root) = private_dir(dir.path(), "trial");
    let pin = PathPin::open(&root, true, true).unwrap();
    let volume = pin.identity.volume;
    let custody = trial::Custody::claim(pin, &evidence, sid(&owner)).unwrap();
    let (mut dependencies, baseline, _) = docker_fixture(&evidence);
    let firmware_bytes = b"reviewed firmware fixture";
    let inventory = inventory::tests::Fixture::with_payloads(&[
        (
            "qemu-system-x86_64.exe",
            b"reviewed stock executable fixture",
        ),
        ("share/edk2-x86_64-code.fd", firmware_bytes),
    ]);
    let (distribution, stock_bundle) = inventory
        .collect_with_receipt(ArtifactPin {
            size: b"receipt".len() as u64,
            sha256: records::digest(b"receipt"),
        })
        .unwrap();
    let stock_inputs = static_compose::StockBootInputs::fixture(
        &stock_bundle,
        &distribution,
        ArtifactPin {
            size: firmware_bytes.len() as u64,
            sha256: records::digest(firmware_bytes),
        },
    );
    dependencies.stock_inventory = stock_inputs.inventory;
    let [stock, stock_data, stock_firmware] = stock_inputs.artifacts().unwrap();
    let wrong_pin = |asset: &Artifact| {
        let pin = asset.pin().unwrap();
        asset.clone().with_pin(ArtifactPin {
            sha256: [99; 32],
            ..pin
        })
    };
    let wrong_stock = wrong_pin(&stock);
    let wrong_firmware = wrong_pin(&stock_firmware);
    let rows = baseline
        .files
        .iter()
        .flat_map(|(_, bytes)| decode::<Vec<Record>>(bytes).unwrap())
        .collect();
    let path = LocalPath::parse(dir.path().to_str().unwrap()).unwrap();
    let artifact = |role, path: &LocalPath, name, id, sha256| {
        Artifact::new(role, path.join(name).unwrap())
            .unwrap()
            .with_identity(FileIdentity {
                volume,
                file: [id; 16],
            })
            .with_pin(ArtifactPin { size: 32, sha256 })
    };
    let patched = artifact(
        PublicEgressQemu,
        &path,
        "patched.exe",
        2,
        dependencies.executable,
    );
    let policy = artifact(
        PublicEgressPolicy,
        &path,
        "policy.json",
        3,
        dependencies.policy,
    );
    let data = artifact(Data, &path, "share", 4, [4; 32]);
    let firmware = artifact(FirmwareCode, &path, "code.fd", 5, [5; 32]);
    let iso = artifact(
        UbuntuInstallIso,
        &path,
        "installer.iso",
        6,
        dependencies.installer,
    );
    let seed = artifact(UbuntuCidata, &root, "install-seed.iso", 7, [7; 32]);
    let kernel = artifact(UbuntuKernel, &path, "vmlinuz", 8, [8; 32]);
    let initrd = artifact(UbuntuInitrd, &path, "initrd", 9, [9; 32]);
    let medium = artifact(
        ComposeDependencyMedium,
        &path,
        "closure.iso",
        10,
        dependencies.recipe.medium,
    );
    let phases = [PublicFixture, Install, Detached, Endpoint];
    let disks = phases.map(|phase| {
        artifact(
            StaticDisk,
            &root,
            phase.leaves()[0],
            20 + phase as u8,
            [20; 32],
        )
    });
    let vars = phases.map(|phase| {
        artifact(
            StaticVars,
            &root,
            phase.leaves()[1],
            30 + phase as u8,
            [30; 32],
        )
    });
    let ports = [
        OriginPortLease::acquire().unwrap(),
        OriginPortLease::acquire().unwrap(),
    ];
    let serial = SerialName::generate().unwrap();
    let mut header = vec![0; 65536];
    header[..4].copy_from_slice(b"QFI\xfb");
    for (at, val) in [(4, 3_u32), (20, 16), (96, 4), (100, 112)] {
        header[at..at + 4].copy_from_slice(&val.to_be_bytes());
    }
    header[24..32].copy_from_slice(&(32_u64 << 30).to_be_bytes());
    let recipe = phases.map(|phase| {
        let (image, boot) = match phase {
            PublicFixture => (
                &patched,
                BootPurpose::InstalledStaticWeb(&ports[0], StaticNetwork::PublicEgress(&policy)),
            ),
            Install => (
                if fault == Fault::WrongStockExecutable {
                    &wrong_stock
                } else {
                    &stock
                },
                BootPurpose::UbuntuInstall {
                    iso: &iso,
                    cidata: &seed,
                    kernel: &kernel,
                    initrd: &initrd,
                },
            ),
            Detached => (&stock, BootPurpose::DetachedStaticCompose),
            Endpoint => (
                &patched,
                BootPurpose::InstalledStaticWeb(&ports[1], StaticNetwork::PublicEgress(&policy)),
            ),
        };
        let i = phase as usize;
        BootRecipe {
            image,
            data: if matches!(phase, Install | Detached) {
                &stock_data
            } else {
                &data
            },
            firmware: match phase {
                Detached if fault == Fault::WrongStockFirmware => &wrong_firmware,
                Install | Detached => &stock_firmware,
                _ => &firmware,
            },
            vars: &vars[i],
            disk: &disks[i],
            disk_header: &header,
            backing: None,
            backing_header: None,
            serial: &serial,
            boot,
            ram_mib: 4096,
            vcpus: 2,
        }
    });

    assert!(Detached.plan(&recipe[Endpoint as usize], None).is_err());
    assert!(Endpoint.plan(&recipe[Detached as usize], None).is_err());
    assert!(
        PublicFixture
            .plan(&recipe[Install as usize], Some(&medium))
            .is_err()
    );
    assert!(Install.plan(&recipe[Install as usize], None).is_err());
    assert!(
        Detached
            .plan(&recipe[Detached as usize], Some(&medium))
            .is_err()
    );

    let trace = RefCell::new(Trace::default());
    let journal = root.join("docker-trial.charge").unwrap();
    let orphan = root.join("staged-partial.qcow2").unwrap();
    let outcome = trial::Run {
        host: Host {
            dependencies: dependencies.clone(),
            rows,
            fault,
            time: Cell::new(0),
            trace: &trace,
            journal: journal.clone(),
            orphan: orphan.clone(),
            recipes: &recipe,
            medium: &medium,
        },
        guest: Guest {
            fault,
            challenge: None,
        },
        capture: Capture {
            fault,
            profile: dependencies.origin_profile,
            policy: dependencies.policy,
            topology: dependencies.topology,
        },
        packages: PACKAGES,
        dependencies: dependencies.clone(),
        stock: stock_inputs,
        custody,
        evidence: &evidence,
        cancel: &Shutdown::default(),
    };
    if fault == Fault::Sink {
        FAULT.set(Some((COMPLETE, Step::Flush)));
    }
    let result = outcome.run();
    FAULT.set(None);
    let trace = trace.into_inner();
    assert_eq!(trace.staged, trace.drained);
    let unresolved = matches!(
        fault,
        Fault::Stage
            | Fault::Scope
            | Fault::UnknownCreate
            | Fault::Dirty
            | Fault::Late
            | Fault::LateDrain
            | Fault::WrongStockExecutable
            | Fault::WrongStockFirmware
    );
    let retained = unresolved
        || matches!(
            fault,
            Fault::WorkspaceLeftover | Fault::LateDispose | Fault::LateDisposeTrial
        );
    assert_eq!(trace.disposed, !unresolved);
    assert_eq!(
        std::fs::exists(orphan.as_str()).unwrap(),
        matches!(
            fault,
            Fault::Stage | Fault::UnknownCreate | Fault::WorkspaceLeftover
        ),
    );
    let charge = std::fs::read(journal.as_str()).unwrap();
    assert_eq!(charge.ends_with(b"released"), !retained);
    assert!(
        trial::Custody::claim(
            PathPin::open(&root, true, true).unwrap(),
            &evidence,
            sid(&owner)
        )
        .is_err()
    );
    if fault == Fault::None {
        assert_eq!(result, Ok(trial::Outcome::Complete));
        assert_eq!(trace.launched, phases);
        let seal = read(&output, COMPLETE);
        let selection = records::DockerSelection {
            seal_sha256: records::digest(&seal),
            scope: evidence.scope(),
        };
        let fresh = records::DockerFresh {
            now: TEST_BOOT.on_boot(NOW).unwrap(),
            boot_marker: TEST_BOOT.marker,
            dependencies: &dependencies,
        };
        records::read_native_docker(
            seal.as_slice(),
            seal.len() as u64,
            |file| Ok(std::io::Cursor::new(read(&output, &file.name()))),
            &selection,
            &fresh,
        )
        .unwrap();
        return;
    }
    assert!(!names(&output).contains(&COMPLETE.to_owned()));
    if fault == Fault::Sink {
        assert!(matches!(result, Err(records::RunError::Output)));
        assert!(names(&output).contains(&"cleanup.json".to_owned()));
        return;
    }
    assert_eq!(result, Ok(trial::Outcome::Incomplete), "{fault:?}");
    let incomplete = value(&output, "incomplete.json");
    assert_eq!(incomplete["charge_retained"], retained);
    assert_eq!(
        incomplete["late"].as_str(),
        matches!(
            fault,
            Fault::Late | Fault::LateDispose | Fault::LateDisposeTrial
        )
        .then_some("expired")
    );
    if matches!(
        fault,
        Fault::Stage | Fault::Scope | Fault::WrongStockExecutable
    ) {
        assert_eq!(trace.launched, [PublicFixture]);
    }
    if matches!(fault, Fault::Dirty | Fault::WrongStockFirmware) {
        assert_eq!(trace.launched, [PublicFixture, Install]);
    }
}

#[test]
fn docker_collector_completes_the_ordered_chain_and_writes_marker_last() {
    collect(Fault::None);
    collect(Fault::Sink);
}

#[test]
fn docker_collector_refuses_foreign_trial_scope_before_create() {
    collect(Fault::Scope);
}

#[test]
fn docker_collector_requires_clean_release() {
    collect(Fault::Dirty);
}

#[test]
fn docker_collector_retains_charge_when_drain_reply_is_late() {
    collect(Fault::LateDrain);
    collect(Fault::Late);
}

#[test]
fn docker_collector_retains_charge_when_dispose_reply_is_late() {
    collect(Fault::LateDispose);
    collect(Fault::LateDisposeTrial);
}

#[test]
fn docker_collector_refuses_wrong_stock_pins_before_dispatch() {
    collect(Fault::WrongStockExecutable);
    collect(Fault::WrongStockFirmware);
}

#[test]
fn docker_collector_retains_staged_crashes_and_refuses_bad_proofs() {
    for fault in [
        Fault::Stage,
        Fault::UnknownCreate,
        Fault::WorkspaceLeftover,
        Fault::WrongPackages,
        Fault::ForeignGuest,
        Fault::WrongCompose,
        Fault::WrongOrigin,
        Fault::WrongTrustProfile,
        Fault::WrongFixture,
        Fault::MissingControls,
        Fault::ReplayedBoot,
    ] {
        collect(fault);
    }
}

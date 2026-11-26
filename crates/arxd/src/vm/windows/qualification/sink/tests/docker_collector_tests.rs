use super::*;
use crate::service::Shutdown;
use crate::vm::paths::Artifact;
use crate::vm::qemu_args::{BootPurpose, BootRecipe, NETWORK, StaticNetwork};
use crate::vm::static_web::OriginPortLease;
use crate::vm::windows::qualification::docker_trial as trial;
use crate::vm::{public_egress::RouteObservation, qualification_record::digest};
use arx_guest::docker_trial::{self as wire, Challenge, TrialRequest, TrialResponse};
use records::{DockerDependencies, Hash, Observation, Record, Stage, StageResult, decode};
use std::cell::{Cell, RefCell};
use trial::witness::{self, Case, Counts, Listener, Receivers, RouteSample, Target, Transport};

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
    WrongPackages,
    ForeignGuest,
    WrongCompose,
    WrongOrigin,
    WrongTrustProfile,
    WrongFixture,
    WrongReceiverBoot,
    WrongReceiverGeneration,
    MissingReceiver,
    MissingReceiverTransport,
    MalformedReceiver,
    WrongReceiverTarget,
    MissingControls,
    ForbiddenArrival,
    NoAttempt,
    ReplayedReceiverWindow,
    MissingRoute,
    ForeignRoute,
    MalformedPolicy,
    ChangedRoute,
    ChangedTopology,
    ReplacedPolicy,
    ForeignListener,
    MissingListener,
    WrongOriginBoot,
    WrongOriginGeneration,
    ReplayedBoot,
    Sink,
}

#[derive(Default)]
struct Trace {
    staged: Vec<trial::Phase>,
    launched: Vec<trial::Phase>,
    continued: Vec<trial::Phase>,
    listener_checks: usize,
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
        // The journal is already open with exclusive write access. The
        // create intent must name the phase before the peer attempts creation.
        assert!(std::fs::exists(self.journal.as_str()).unwrap());
        self.trace.borrow_mut().staged.push(ticket.phase);
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

    fn route(
        &mut self,
        boot: &trial::CheckedBoot<'_>,
        peer: Option<trial::PeerId>,
        _: u64,
    ) -> Result<RouteSample, NativeRefusal> {
        let ticket = boot.ticket();
        let trace = self.trace.borrow();
        if peer.is_some() {
            assert_eq!(trace.launched.last(), Some(&ticket.phase));
        }
        assert!(!trace.continued.contains(&ticket.phase));
        let mut sample = RouteSample {
            ticket,
            peer,
            policy_bytes: policy_bytes(),
            policy_identity: boot
                .argv()
                .public_egress_policy()
                .unwrap()
                .identity()
                .unwrap(),
            topology: self.dependencies.topology,
            route: RouteObservation {
                interface_index: 1,
                interface_luid: 2,
                source: "192.0.2.2".parse().unwrap(),
                gateway: "192.0.2.1".parse().unwrap(),
                default_route: true,
            },
        };
        // The sample before create is valid. The live recheck must refuse.
        if peer.is_some() {
            match self.fault {
                Fault::MissingRoute => return Err(NativeRefusal::NetworkFenceUnverified),
                Fault::ForeignRoute => sample.peer.as_mut().unwrap().boot[0] ^= 1,
                Fault::MalformedPolicy => sample.policy_bytes.push(0),
                Fault::ChangedRoute => sample.route.interface_luid += 1,
                Fault::ChangedTopology => sample.topology[0] ^= 1,
                Fault::ReplacedPolicy => sample.policy_identity.file[0] ^= 1,
                _ => {}
            }
        }
        Ok(sample)
    }

    fn receiver_targets(&mut self, ticket: trial::Ticket) -> Result<Vec<Target>, NativeRefusal> {
        assert_eq!(ticket.phase, trial::Phase::PublicFixture);
        let mut targets = witness::fixture_targets();
        if self.fault == Fault::MissingReceiverTransport {
            targets.retain(|target| {
                target.case != Case::Private || target.transport != Transport::Udp
            });
        }
        Ok(targets)
    }

    fn prelaunch(
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
                assert!(plan.image().as_str().ends_with("stock.exe"));
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

    fn cont(&mut self, peer_id: trial::PeerId, _: u64, _: &Shutdown) -> Result<(), NativeRefusal> {
        let mut trace = self.trace.borrow_mut();
        let phase = *trace.launched.last().unwrap();
        assert_eq!(peer(phase, self.fault), peer_id);
        trace.continued.push(phase);
        Ok(())
    }

    fn listener(
        &mut self,
        peer: trial::PeerId,
        port: u16,
        _: u64,
    ) -> Result<Listener, NativeRefusal> {
        let mut trace = self.trace.borrow_mut();
        assert_eq!(trace.continued.last(), Some(&trial::Phase::Endpoint));
        trace.listener_checks += 1;
        // Model a foreign listener that replaces the original during HTTPS.
        match self.fault {
            Fault::ForeignListener if trace.listener_checks == 2 => {
                Ok(Listener::fixture(port, peer.pid + 1))
            }
            Fault::MissingListener => Err(NativeRefusal::GuestEndpointUnverified),
            _ => Ok(Listener::fixture(port, peer.pid)),
        }
    }

    fn drain(&mut self, ticket: trial::Ticket, _: u64) -> Result<trial::Release, NativeRefusal> {
        self.trace.borrow_mut().drained.push(ticket.phase);
        if self.fault == Fault::Late && ticket.phase == trial::Phase::Endpoint {
            self.time.set(records::MAX_TRIAL_MS + 10);
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
        _: u64,
    ) -> Result<trial::WorkspaceRelease, NativeRefusal> {
        self.trace.borrow_mut().disposed = true;
        if self.fault == Fault::WorkspaceLeftover {
            std::fs::File::create_new(self.orphan.as_str()).unwrap();
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

struct Capture<'a> {
    fault: Fault,
    profile: Hash,
    trace: &'a RefCell<Trace>,
}

impl trial::Capture for Capture<'_> {
    fn fixture(
        &mut self,
        peer: trial::PeerId,
        ticket: trial::Ticket,
        targets: &[Target],
        window: std::ops::Range<u64>,
    ) -> Result<Vec<u8>, NativeRefusal> {
        assert_eq!(self.trace.borrow().continued.last(), Some(&ticket.phase));
        let mut capture = Receivers {
            run: ticket.scope.run,
            boot: peer.boot,
            generation: ticket.challenge.generation,
            nonce: ticket.challenge.nonce,
            observations: targets
                .iter()
                .map(|target| Counts {
                    target: target.clone(),
                    before_ms: window.start + 1,
                    between_ms: [window.start + 2, window.start + 3],
                    after_ms: window.start + 4,
                    before: 1,
                    attempts: 1,
                    arrivals: target.case.expected_arrivals(),
                    after: 1,
                })
                .collect(),
        };
        let denied = capture
            .observations
            .iter_mut()
            .find(|counts| counts.target.case == Case::Host)
            .unwrap();
        match self.fault {
            Fault::WrongFixture => capture.nonce[0] ^= 1,
            Fault::WrongReceiverBoot => capture.boot[0] ^= 1,
            Fault::WrongReceiverGeneration => capture.generation += 1,
            Fault::MissingReceiver => {
                capture.observations.pop();
            }
            Fault::MalformedReceiver => return Ok(b"{".to_vec()),
            Fault::WrongReceiverTarget => denied.target.identity[0] ^= 1,
            Fault::MissingControls => {
                denied.before = 0;
                denied.after = 0;
            }
            Fault::ForbiddenArrival => denied.arrivals = 1,
            Fault::NoAttempt => denied.attempts = 0,
            Fault::ReplayedReceiverWindow => denied.before_ms = window.start - 1,
            _ => {}
        }
        Ok(serde_json::to_vec(&capture).unwrap())
    }

    fn origin(
        &mut self,
        peer: trial::PeerId,
        challenge: Challenge,
        port: u16,
        _: u64,
    ) -> Result<trial::OriginProof, NativeRefusal> {
        assert_eq!(self.trace.borrow().listener_checks, 1);
        let mut observed = peer;
        let mut binding = challenge;
        if self.fault == Fault::WrongOriginBoot {
            observed.boot[0] ^= 1;
        }
        if self.fault == Fault::WrongOriginGeneration {
            binding.generation += 1;
        }
        Ok(trial::OriginProof {
            peer: observed,
            challenge: binding,
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

fn policy_bytes() -> Vec<u8> {
    let body = concat!(
        "version=1\ninterface_index=1\ninterface_luid=2\n",
        "source=192.0.2.2\ngateway=192.0.2.1\n",
    );
    format!(
        "{body}fingerprint={}\n",
        hex::encode(digest(body.as_bytes()))
    )
    .into_bytes()
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
    dependencies.policy = digest(&policy_bytes());
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
    let stock = artifact(Qemu, &path, "stock.exe", 1, [1; 32]);
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
                &stock,
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
            data: &data,
            firmware: &firmware,
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
            trace: &trace,
        },
        packages: PACKAGES,
        dependencies: dependencies.clone(),
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
        Fault::Stage | Fault::Scope | Fault::UnknownCreate | Fault::Dirty
    );
    let retained = unresolved || fault == Fault::WorkspaceLeftover;
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
        assert_eq!(trace.continued, phases);
        assert_eq!(trace.listener_checks, 2);
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
        (fault == Fault::Late).then_some("expired")
    );
    if matches!(fault, Fault::Stage | Fault::Scope) {
        assert_eq!(trace.launched, [PublicFixture]);
    }
    if matches!(
        fault,
        Fault::MissingRoute
            | Fault::ForeignRoute
            | Fault::MalformedPolicy
            | Fault::ChangedRoute
            | Fault::ChangedTopology
            | Fault::ReplacedPolicy
    ) {
        assert_eq!(trace.launched, [PublicFixture]);
        assert!(trace.continued.is_empty());
    }
    if fault == Fault::Dirty {
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
fn docker_collector_retains_staged_crashes_and_refuses_bad_or_late_proofs() {
    for fault in [
        Fault::Stage,
        Fault::UnknownCreate,
        Fault::WorkspaceLeftover,
        Fault::Late,
        Fault::WrongPackages,
        Fault::ForeignGuest,
        Fault::WrongCompose,
        Fault::WrongOrigin,
        Fault::WrongTrustProfile,
        Fault::WrongFixture,
        Fault::ReplayedBoot,
    ] {
        collect(fault);
    }
}

#[test]
fn docker_collector_refuses_silence_without_positive_controls() {
    collect(Fault::MissingControls);
}

#[test]
fn docker_collector_rechecks_live_policy_route_before_cont() {
    collect(Fault::ChangedRoute);
}

#[test]
fn docker_collector_refuses_foreign_listener_after_https() {
    collect(Fault::ForeignListener);
}

#[test]
fn docker_collector_refuses_missing_foreign_and_malformed_witnesses() {
    for fault in [
        Fault::WrongReceiverBoot,
        Fault::WrongReceiverGeneration,
        Fault::MissingReceiver,
        Fault::MissingReceiverTransport,
        Fault::MalformedReceiver,
        Fault::WrongReceiverTarget,
        Fault::ForbiddenArrival,
        Fault::NoAttempt,
        Fault::ReplayedReceiverWindow,
        Fault::MissingRoute,
        Fault::ForeignRoute,
        Fault::MalformedPolicy,
        Fault::ChangedTopology,
        Fault::ReplacedPolicy,
        Fault::MissingListener,
        Fault::WrongOriginBoot,
        Fault::WrongOriginGeneration,
    ] {
        collect(fault);
    }
}

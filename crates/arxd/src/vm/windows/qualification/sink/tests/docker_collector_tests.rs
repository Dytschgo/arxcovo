use super::*;
use crate::service::Shutdown;
use crate::vm::paths::Artifact;
use crate::vm::qemu_args::{BootPurpose, BootRecipe, NETWORK, StaticNetwork};
use crate::vm::static_web::OriginPortLease;
use crate::vm::windows::qualification::{docker_trial as trial, static_compose};
use crate::vm::{public_egress::RouteObservation, qualification_record::digest};
use arx_guest::docker_trial::{self as wire, Challenge, TrialRequest, TrialResponse};
use records::{DockerDependencies, Hash, Observation, Record, Stage, StageResult, decode};
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use trial::witness::{
    self, Case, Counts, Drift, FlowEvent, Listener, ReceiverEnrollment, Receivers, RouteChange,
    RouteSample, Target, Transport,
};

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
    WrongStockData,
    WrongStockDataIdentity,
    WrongStockFirmware,
    WrongPackages,
    ForeignGuest,
    WrongCompose,
    WrongOrigin,
    WrongOriginCertificate,
    WrongFixture,
    WrongReceiverBoot,
    WrongReceiverGeneration,
    MissingReceiver,
    MissingReceiverTransport,
    MalformedReceiver,
    WrongReceiverTarget,
    OmittedEnrolledReceiver,
    ExtraEnrolledReceiver,
    SubstitutedEnrolledReceiver,
    NoRouteChange,
    MissingDriftEvents,
    ForeignClosedFlow,
    NoEstablishedArrivals,
    ForeignRefusedFlow,
    ReusedDriftFlow,
    WrongDriftBaseline,
    PublicDeliveryAfterDrift,
    DnsDeliveryAfterDrift,
    PublicDeliveryAtDrift,
    DeniedObservationAfterDrift,
    RouteDriftArrivalAfterDrift,
    FixtureBoundStockRow,
    MissingBeforeControl,
    MissingAfterControl,
    ForbiddenArrival,
    NoAttempt,
    ReplayedReceiverWindow,
    MissingRoute,
    ForeignRoute,
    MalformedPolicy,
    ChangedRoute,
    EndpointRouteDrift,
    ChangedTopology,
    ReplacedPolicy,
    ForeignListener,
    MissingListener,
    WrongOriginName,
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
        if trace.continued.contains(&ticket.phase) {
            assert_eq!(ticket.phase, trial::Phase::Endpoint);
            assert_eq!(trace.listener_checks, 2);
            if self.fault == Fault::EndpointRouteDrift {
                return Err(NativeRefusal::NetworkFenceUnverified);
            }
        }
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
            route: fixture_route(),
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
        match self.fault {
            Fault::MissingReceiverTransport => targets.retain(|target| {
                target.case != Case::Private || target.transport != Transport::Udp
            }),
            Fault::OmittedEnrolledReceiver => targets.retain(|target| target.identity != [100; 32]),
            Fault::ExtraEnrolledReceiver => targets.push(Target {
                case: Case::Private,
                transport: Transport::Tcp,
                identity: [101; 32],
            }),
            Fault::SubstitutedEnrolledReceiver => {
                targets
                    .iter_mut()
                    .find(|target| target.identity == [100; 32])
                    .unwrap()
                    .identity = [101; 32];
            }
            _ => {}
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
            clean_guest_shutdown: self.trace.borrow().continued.contains(&ticket.phase)
                && (self.fault != Fault::Dirty || ticket.phase != trial::Phase::Install),
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

struct Guest<'a> {
    fault: Fault,
    challenge: Option<Challenge>,
    runtime: &'a tokio::runtime::Runtime,
    port: &'a OriginPortLease,
}

impl trial::Guest for Guest<'_> {
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
                let certificate_der = origin_peer(self.runtime, self.port, challenge, self.fault);
                let mut echoed = challenge;
                if self.fault == Fault::WrongOriginGeneration {
                    echoed.generation += 1;
                }
                arx_guest::write_frame(
                    &mut reply,
                    &TrialResponse::DockerTrialProvisioned {
                        challenge: echoed,
                        certificate_der,
                    },
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
        let mut before = witness::ObservedRoute::from(fixture_route());
        let mut after = before;
        if self.fault != Fault::NoRouteChange {
            after.interface_luid += 1;
        }
        if self.fault == Fault::WrongDriftBaseline {
            before.interface_luid += 2;
        }
        let mut capture = Receivers {
            run: ticket.scope.run,
            boot: peer.boot,
            generation: ticket.challenge.generation,
            nonce: ticket.challenge.nonce,
            route_change: RouteChange {
                at_ms: window.start + 8,
                before,
                after,
            },
            observations: targets
                .iter()
                .enumerate()
                .map(|(index, target)| {
                    let is_drift = target.case == Case::RouteDrift;
                    let (before_ms, between_ms, after_ms) = if is_drift {
                        (
                            window.start + 6,
                            [window.start + 10, window.start + 12],
                            window.start + 13,
                        )
                    } else {
                        // Complete the ordinary matrix while the enrolled route is stable.
                        (
                            window.start + 1,
                            [window.start + 2, window.start + 4],
                            window.start + 5,
                        )
                    };
                    Counts {
                        target: target.clone(),
                        before_ms,
                        between_ms,
                        after_ms,
                        before: 1,
                        attempts: 1,
                        arrivals: target.case.expected_arrivals(),
                        after: 1,
                        drift: is_drift
                            .then(|| {
                                let flow = if self.fault == Fault::ReusedDriftFlow {
                                    [201; 16]
                                } else {
                                    [index as u8 + 1; 16]
                                };
                                let stamp = |offset| FlowEvent {
                                    flow,
                                    at_ms: window.start + offset,
                                };
                                let mut closed = stamp(9);
                                let mut refused = stamp(11);
                                if self.fault == Fault::ForeignClosedFlow {
                                    closed.flow[0] ^= 1;
                                }
                                if self.fault == Fault::ForeignRefusedFlow {
                                    refused.flow[0] ^= 1;
                                }
                                Drift {
                                    established: stamp(7),
                                    established_arrivals: u16::from(
                                        self.fault != Fault::NoEstablishedArrivals,
                                    ),
                                    closed,
                                    refused,
                                }
                            })
                            .filter(|_| self.fault != Fault::MissingDriftEvents),
                    }
                })
                .collect(),
        };
        match self.fault {
            Fault::PublicDeliveryAfterDrift | Fault::DnsDeliveryAfterDrift => {
                let target_case = if self.fault == Fault::PublicDeliveryAfterDrift {
                    Case::Public
                } else {
                    Case::RecursiveDns
                };
                let delivery = capture
                    .observations
                    .iter_mut()
                    .find(|counts| counts.target.case == target_case)
                    .unwrap();
                delivery.before_ms = window.start + 6;
                delivery.between_ms = [window.start + 10, window.start + 12];
                delivery.after_ms = window.start + 13;
            }
            Fault::PublicDeliveryAtDrift => {
                let delivery = capture
                    .observations
                    .iter_mut()
                    .find(|counts| counts.target.case == Case::Public)
                    .unwrap();
                delivery.before_ms = window.start + 1;
                delivery.between_ms = [window.start + 2, window.start + 7];
                delivery.after_ms = window.start + 8;
            }
            Fault::DeniedObservationAfterDrift => {
                let denied = capture
                    .observations
                    .iter_mut()
                    .find(|counts| counts.target.case == Case::Host)
                    .unwrap();
                denied.before_ms = window.start + 6;
                denied.between_ms = [window.start + 10, window.start + 12];
                denied.after_ms = window.start + 13;
            }
            Fault::RouteDriftArrivalAfterDrift => {
                let drift = capture
                    .observations
                    .iter_mut()
                    .find(|counts| counts.target.case == Case::RouteDrift)
                    .unwrap();
                drift.arrivals = 1;
            }
            _ => {}
        }
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
            Fault::MissingBeforeControl => denied.before = 0,
            Fault::MissingAfterControl => denied.after = 0,
            Fault::ForbiddenArrival => denied.arrivals = 1,
            Fault::NoAttempt => denied.attempts = 0,
            Fault::ReplayedReceiverWindow => denied.before_ms = window.start - 1,
            _ => {}
        }
        let json = serde_json::to_string(&capture).unwrap();
        if self.fault == Fault::FixtureBoundStockRow {
            assert!(json.contains(r#""case":"Host""#));
            return Ok(json
                .replacen(r#""case":"Host""#, r#""case":"StockTest""#, 1)
                .into_bytes());
        }
        Ok(json.into_bytes())
    }
}

// The fixture supplies a TLS server, not an origin verdict. The collector
// makes the same certificate, SNI and HTTP checks for the emitted dossier.
fn origin_peer(
    runtime: &tokio::runtime::Runtime,
    port: &OriginPortLease,
    challenge: Challenge,
    fault: Fault,
) -> Vec<u8> {
    let mut other = challenge;
    other.nonce[0] ^= 1;
    let name = if fault == Fault::WrongOriginName {
        other.origin_name()
    } else {
        challenge.origin_name()
    };
    let leaf = rcgen::generate_simple_self_signed(vec![name]).unwrap();
    let der = if fault == Fault::WrongOriginCertificate {
        rcgen::generate_simple_self_signed(vec![challenge.origin_name()])
            .unwrap()
            .cert
            .der()
            .to_vec()
    } else {
        leaf.cert.der().to_vec()
    };
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(leaf.signing_key.serialize_der()).into(),
    )
    .unwrap();
    let body = if fault == Fault::WrongOrigin {
        other.origin_health()
    } else {
        challenge.origin_health()
    };
    let listener = port.fixture_listener().unwrap();
    runtime.spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let tls = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let Ok(mut stream) = tls.accept(socket).await else {
            return;
        };
        let mut request = Vec::new();
        while request.len() < 1024 && !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        let expected = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            wire::ORIGIN_HEALTH_PATH,
            challenge.origin_name(),
        );
        assert_eq!(request, expected.as_bytes());
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    der
}

fn fixture_route() -> RouteObservation {
    RouteObservation {
        interface_index: 1,
        interface_luid: 2,
        source: "192.0.2.2".parse().unwrap(),
        gateway: "192.0.2.1".parse().unwrap(),
        default_route: true,
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
    dependencies.origin_profile = trial::origin_profile_pin();
    let manifest = serde_json::to_vec(&witness::fixture_targets()).unwrap();
    let receiver_enrollment =
        ReceiverEnrollment::from_manifest(&manifest, [[51; 32], [52; 32], [53; 32]]).unwrap();
    dependencies.network_fixture = receiver_enrollment.fixture();
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
    let alt_share_path = local(dir.path().join("substituted-share").to_str().unwrap()).unwrap();
    std::fs::create_dir(alt_share_path.as_str()).unwrap();
    let alt_share_pin = PathPin::open(&alt_share_path, true, false).unwrap();
    let wrong_stock_data = Artifact::new(Data, alt_share_pin.path.clone())
        .unwrap()
        .with_identity(alt_share_pin.identity);
    let wrong_stock_data_identity = Artifact::new(Data, stock_data.path_for(Data).unwrap().clone())
        .unwrap()
        .with_identity(alt_share_pin.identity);
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
            data: match phase {
                Install if fault == Fault::WrongStockData => &wrong_stock_data,
                Detached if fault == Fault::WrongStockDataIdentity => &wrong_stock_data_identity,
                Install | Detached => &stock_data,
                _ => &data,
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

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
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
            runtime: &runtime,
            port: &ports[1],
        },
        capture: Capture {
            fault,
            trace: &trace,
        },
        packages: PACKAGES,
        dependencies: dependencies.clone(),
        stock: stock_inputs,
        receiver_enrollment,
        custody,
        evidence: &evidence,
        cancel: &Shutdown::default(),
        runtime: &runtime,
    };
    if fault == Fault::Sink {
        FAULT.set(Some((COMPLETE, Step::Flush)));
    }
    let result = outcome.run();
    FAULT.set(None);
    let trace = trace.into_inner();
    assert_eq!(trace.staged, trace.drained);
    let pre_cont_refusal = matches!(
        fault,
        Fault::MissingRoute
            | Fault::ForeignRoute
            | Fault::MalformedPolicy
            | Fault::ChangedRoute
            | Fault::ChangedTopology
            | Fault::ReplacedPolicy
    );
    let inventory_refusal = matches!(
        fault,
        Fault::OmittedEnrolledReceiver
            | Fault::ExtraEnrolledReceiver
            | Fault::SubstitutedEnrolledReceiver
            | Fault::MissingReceiverTransport
    );
    let unresolved = matches!(
        fault,
        Fault::Stage
            | Fault::Scope
            | Fault::UnknownCreate
            | Fault::Dirty
            | Fault::Late
            | Fault::LateDrain
            | Fault::WrongStockExecutable
            | Fault::WrongStockData
            | Fault::WrongStockDataIdentity
            | Fault::WrongStockFirmware
    ) || pre_cont_refusal
        || inventory_refusal;
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
        crate::service::windows::set_current_token_owner().unwrap();
        let (_, enrolled) = private_dir(dir.path(), "docker-enrollment");
        let enrolled = enrolled.join("docker.json").unwrap();
        std::fs::write(enrolled.as_str(), br#"{"class":"docker"}"#).unwrap();
        let output_pin = PathPin::open(&output, true, false).unwrap();
        let expected = crate::config::fixture_directory_pin(&output_pin.leaf);
        let dossier =
            PreparedStaticDossier::hold(&output, &expected, &enrolled, selection).unwrap();
        let (selected, _rows) = dossier.read_measured(&fresh).unwrap();
        assert_eq!(selected.recipe(), dependencies.recipe);
        let mut other_boot = fresh.boot_marker;
        other_boot[0] ^= 1;
        assert!(
            dossier
                .read_measured(&records::DockerFresh {
                    boot_marker: other_boot,
                    ..fresh
                })
                .is_err()
        );
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
        Fault::Stage | Fault::Scope | Fault::WrongStockExecutable | Fault::WrongStockData
    ) {
        assert_eq!(trace.launched, [PublicFixture]);
    }
    if inventory_refusal {
        assert!(trace.launched.is_empty());
        assert!(trace.continued.is_empty());
    }
    if pre_cont_refusal {
        assert_eq!(trace.launched, [PublicFixture]);
        assert!(trace.continued.is_empty());
    }
    if matches!(
        fault,
        Fault::Dirty | Fault::WrongStockFirmware | Fault::WrongStockDataIdentity
    ) {
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
fn docker_collector_refuses_substituted_stock_data_directory_before_dispatch() {
    collect(Fault::WrongStockData);
    collect(Fault::WrongStockDataIdentity);
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
        Fault::WrongOriginCertificate,
        Fault::WrongFixture,
        Fault::ReplayedBoot,
    ] {
        collect(fault);
    }
}

#[test]
fn docker_collector_refuses_silence_without_before_control() {
    collect(Fault::MissingBeforeControl);
}

#[test]
fn docker_collector_refuses_silence_without_after_control() {
    collect(Fault::MissingAfterControl);
}

#[test]
fn docker_collector_refuses_omitted_or_extra_enrolled_receivers() {
    collect(Fault::OmittedEnrolledReceiver);
    collect(Fault::ExtraEnrolledReceiver);
}

#[test]
fn docker_collector_refuses_substituted_enrolled_receiver() {
    collect(Fault::SubstitutedEnrolledReceiver);
}

#[test]
fn docker_collector_refuses_route_drift_silence_without_change() {
    collect(Fault::NoRouteChange);
}

#[test]
fn docker_collector_refuses_unbound_route_drift_events() {
    for fault in [
        Fault::MissingDriftEvents,
        Fault::ForeignClosedFlow,
        Fault::NoEstablishedArrivals,
        Fault::ForeignRefusedFlow,
        Fault::ReusedDriftFlow,
        Fault::WrongDriftBaseline,
    ] {
        collect(fault);
    }
}

#[test]
fn docker_collector_refuses_public_and_dns_deliveries_after_route_drift() {
    for fault in [
        Fault::PublicDeliveryAfterDrift,
        Fault::DnsDeliveryAfterDrift,
        Fault::PublicDeliveryAtDrift,
        Fault::DeniedObservationAfterDrift,
        Fault::RouteDriftArrivalAfterDrift,
    ] {
        collect(fault);
    }
}

#[test]
fn docker_collector_refuses_stock_row_on_patched_boot() {
    collect(Fault::FixtureBoundStockRow);
}

#[test]
fn docker_collector_rechecks_live_policy_route_before_cont() {
    collect(Fault::ChangedRoute);
}

#[test]
fn docker_collector_refuses_endpoint_route_drift_after_https() {
    collect(Fault::EndpointRouteDrift);
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
        Fault::WrongOriginName,
        Fault::WrongOriginGeneration,
    ] {
        collect(fault);
    }
}

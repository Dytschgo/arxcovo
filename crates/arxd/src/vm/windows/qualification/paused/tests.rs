use super::*;
use std::{
    cell::RefCell,
    io::Cursor,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

const BUDGET: Duration = Duration::from_secs(1);
const JOB: NativeRefusal = NativeRefusal::JobUnverified;
const DISK: NativeRefusal = NativeRefusal::DiskCeilingUnverified;
const RESERVES: NativeRefusal = NativeRefusal::ReservesUnverified;
const OPENS: NativeRefusal = NativeRefusal::PathCustodyUnverified;
const DISTRIBUTION: NativeRefusal = NativeRefusal::DistributionUnverified;
const GREETING: &str = r#"{"QMP":{"version":{"qemu":{"major":11,"minor":1,"micro":3},"package":""},"capabilities":[]}}"#;
const CAPABILITIES: &str = r#"{"return":{},"id":1}"#;
const PRELAUNCH: &str = r#"{"return":{"running":false,"status":"prelaunch"},"id":2}"#;

fn live() -> Live {
    Live {
        captured: Tick::now(),
        idle: true,
        active_cpus: 8,
        effective_cpus: 8,
        available_ram_bytes: 32 * GIB,
        whpx: true,
        limits: Limits {
            inner_caller_free_bytes: 70 * GIB,
            inner_total_free_bytes: 71 * GIB,
            inner_total_bytes: 99 * GIB,
            inner_extent_bytes: 99 * GIB,
            vhd_virtual_bytes: 100 * GIB,
            outer_caller_free_bytes: 60 * GIB,
            outer_total_free_bytes: 61 * GIB,
            backing_allocated_bytes: 100 * GIB + 8192,
            backing_length_bytes: 100 * GIB + 8192,
            disk_ceiling_bytes: DISK_CEILING,
            outer_floor_bytes: OUTER_FLOOR,
            disk_budget_bytes: 99 * GIB,
            empty_job_memory_limit_bytes: JOB_MEMORY_BYTES as u64,
            empty_job_cpu_rate: JOB_CPU_RATE,
        },
        outstanding_inner_bytes: 2 * GIB,
        outstanding_outer_bytes: 2 * GIB,
    }
}

const PEAK: Peak = Peak {
    outside_ram_bytes: GIB,
    outside_cpu_rate: 250,
    inner_bytes: 1024 * 1024,
    outer_bytes: 1024 * 1024,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    ProveContract,
    Claim,
    Create,
    InspectChild,
    Resume,
    VerifyOpens,
    VerifyWhpx,
    Stop,
    Reopen,
    Settle,
}

#[derive(Clone, Copy, Default)]
enum Stop {
    #[default]
    Confirmed,
    ChildAlive,
    SupervisorLost,
    WorkerAlive,
    Late,
}

struct FakeWriter {
    written: Rc<RefCell<Vec<u8>>>,
    dispatch: Arc<DispatchGate>,
    late: bool,
}

impl AsyncWrite for FakeWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        // The fixture's async deadlines and pipe-worker dispatch share the
        // same clock, including when Tokio advances it in a paused test.
        let now = Tick::now().into_std();
        *self.dispatch.test.clock.lock().unwrap() = Some(now);
        if !self.dispatch.begin_write() {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        if self.late {
            // The peer may already have replied. Set the worker clock past
            // the tracked window just before the actual write settles.
            *self.dispatch.test.clock.lock().unwrap() = Some(now + FRESHNESS);
        }
        self.written.borrow_mut().extend_from_slice(bytes);
        self.dispatch.end_write(true);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

type LiveOverride = (usize, fn(&mut Live));

struct FakeSlot;
struct FakeOpens;

struct Fake {
    cancel: Shutdown,
    steps: Vec<Step>,
    written: Rc<RefCell<Vec<u8>>>,
    replies: Option<String>,
    peer_hold: Option<tokio::io::DuplexStream>,
    dispatch: Option<Arc<DispatchGate>>,
    fail: Option<(Step, NativeRefusal)>,
    live_override: Option<LiveOverride>,
    live_calls: usize,
    stop: Stop,
    reopen: bool,
    late_reopen: bool,
    late_write: bool,
    cancel_on_resume: bool,
    charge_retained: bool,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            cancel: Shutdown::default(),
            steps: Vec::new(),
            written: Rc::default(),
            replies: Some(format!("{GREETING}\n{CAPABILITIES}\n{PRELAUNCH}\n")),
            peer_hold: None,
            dispatch: None,
            fail: None,
            live_override: None,
            live_calls: 0,
            stop: Stop::Confirmed,
            reopen: true,
            late_reopen: false,
            late_write: false,
            cancel_on_resume: false,
            charge_retained: false,
        }
    }
}

impl Fake {
    fn step(&mut self, step: Step) -> Result<(), NativeRefusal> {
        self.steps.push(step);
        if let Some((when, failure)) = self.fail
            && when == step
        {
            return Err(failure);
        }
        Ok(())
    }

    async fn run(&mut self) -> TrialResult {
        let lat = |text| LocalPath::parse(text).unwrap();
        let image = lat(r"X:\fixture\qemu-system-x86_64.exe");
        let data = lat(r"X:\fixture\share");
        let bios = lat(r"X:\fixture\share\bios-256k.bin");
        let plan = PausedPlan {
            image: &image,
            data: &data,
            bios: &bios,
        };
        let cancel = self.cancel.clone();
        run(self, &plan, &cancel, Tick::now() + BUDGET, BUDGET).await
    }
}

impl Host for Fake {
    type Opens = FakeOpens;
    type Slot = FakeSlot;
    type Reader = Box<dyn AsyncRead + Unpin>;
    type Writer = FakeWriter;

    fn open_contract(&mut self, _: &PausedPlan<'_>) -> Result<(FakeOpens, Peak), NativeRefusal> {
        self.step(Step::ProveContract)?;
        Ok((FakeOpens, PEAK))
    }

    fn live(&mut self) -> Result<Live, NativeRefusal> {
        let mut observation = live();
        if let Some(dispatch) = &self.dispatch {
            *dispatch.test.clock.lock().unwrap() = Some(observation.captured.into_std());
        }
        if let Some((at, mutate)) = self.live_override
            && self.live_calls == at
        {
            mutate(&mut observation);
        }
        self.live_calls += 1;
        Ok(observation)
    }

    fn claim(&mut self, _: &Live, _: Peak) -> Result<FakeSlot, NativeRefusal> {
        self.step(Step::Claim)?;
        assert!(!self.charge_retained);
        self.charge_retained = true;
        Ok(FakeSlot)
    }

    fn create_suspended(
        &mut self,
        _: &FakeSlot,
        plan: &PausedPlan<'_>,
        _: &FakeOpens,
    ) -> Result<(), NativeRefusal> {
        assert!(self.charge_retained);
        assert_eq!(plan.image.as_str(), r"X:\fixture\qemu-system-x86_64.exe");
        assert_eq!(
            plan.args(),
            [
                "-no-user-config",
                "-nodefaults",
                "-display",
                "none",
                "-serial",
                "none",
                "-monitor",
                "none",
                "-nic",
                "none",
                "-accel",
                "whpx",
                "-machine",
                "pc-q35-11.1",
                "-smp",
                "1",
                "-m",
                "256M",
                "-L",
                r"X:\fixture\share",
                "-bios",
                r"X:\fixture\share\bios-256k.bin",
                "-S",
                "-chardev",
                "stdio,id=arxqmp,signal=off",
                "-mon",
                "chardev=arxqmp,mode=control",
            ]
        );
        self.step(Step::Create)
    }

    fn verify_child(
        &mut self,
        _: &FakeSlot,
    ) -> Result<Pipes<Self::Reader, Self::Writer>, NativeRefusal> {
        self.step(Step::InspectChild)?;
        let stdout: Self::Reader = match self.replies.take() {
            Some(replies) => Box::new(Cursor::new(replies.into_bytes())),
            None => {
                let (read, write) = tokio::io::duplex(64);
                self.peer_hold = Some(write);
                Box::new(read)
            }
        };
        let dispatch = Arc::new(DispatchGate::default());
        self.dispatch = Some(dispatch.clone());
        let stdin = FakeWriter {
            written: self.written.clone(),
            dispatch: dispatch.clone(),
            late: self.late_write,
        };
        Ok(Pipes {
            stdout,
            stdin,
            dispatch,
        })
    }

    fn resume(&mut self, _: &FakeSlot) -> Result<(), NativeRefusal> {
        self.step(Step::Resume)?;
        if self.cancel_on_resume {
            self.cancel.cancel();
        }
        Ok(())
    }

    fn verify_opens(&mut self, _: &FakeSlot, _: &FakeOpens) -> Result<(), NativeRefusal> {
        self.step(Step::VerifyOpens)
    }

    fn verify_whpx(&mut self, _: &FakeSlot) -> Result<(), NativeRefusal> {
        self.step(Step::VerifyWhpx)
    }

    fn request_stop(&mut self, _: &FakeSlot) {
        self.steps.push(Step::Stop);
    }

    async fn observe_stop(&mut self, _: &FakeSlot, deadline: Tick) -> StopEvidence {
        assert!(self.steps.ends_with(&[Step::Stop]));
        if matches!(self.stop, Stop::Late) {
            tokio::time::sleep_until(deadline).await;
        }
        StopEvidence {
            process_exited: !matches!(self.stop, Stop::ChildAlive),
            // Supervisor loss can provide a signaled wait-only process
            // handle. It cannot alone establish the whole Job was empty.
            job_empty: !matches!(self.stop, Stop::SupervisorLost),
            workers_joined: !matches!(self.stop, Stop::WorkerAlive),
            ..StopEvidence::default()
        }
    }

    async fn reopen(&mut self, _: &FakeSlot, deadline: Tick) -> bool {
        self.steps.push(Step::Reopen);
        if self.late_reopen {
            tokio::time::sleep_until(deadline).await;
        }
        self.reopen
    }

    fn settle(&mut self, _: FakeSlot, disposition: StopDisposition) {
        self.steps.push(Step::Settle);
        self.charge_retained = disposition == StopDisposition::ChargedQuarantine;
    }
}

#[tokio::test]
async fn exact_recipe_and_qmp_pause_require_custody_and_settle() {
    let mut host = Fake::default();
    let result = host.run().await;
    assert_eq!(
        result,
        TrialResult {
            paused: Ok(()),
            release: StopDisposition::DirtyRelease
        }
    );
    assert_eq!(host.live_calls, 6);
    assert_eq!(
        host.steps,
        [
            Step::ProveContract,
            Step::Claim,
            Step::Create,
            Step::InspectChild,
            Step::Resume,
            Step::VerifyOpens,
            Step::VerifyWhpx,
            Step::Stop,
            Step::Reopen,
            Step::Settle,
        ]
    );
    assert_eq!(host.written.borrow().as_slice(),
        b"{\"execute\":\"qmp_capabilities\",\"id\":1}\r\n{\"execute\":\"query-status\",\"id\":2}\r\n");
    assert!(!host.charge_retained);
}

#[tokio::test]
async fn missing_contract_and_claim_refuse_before_create() {
    for (step, reason) in [(Step::ProveContract, DISTRIBUTION), (Step::Claim, DISK)] {
        let mut host = Fake {
            fail: Some((step, reason)),
            ..Fake::default()
        };
        let result = host.run().await;
        assert_eq!(result.paused, Err(Failure::Refused(reason)));
        let expected = match step {
            Step::ProveContract => vec![Step::ProveContract],
            _ => vec![Step::ProveContract, Step::Claim],
        };
        assert_eq!(host.steps, expected);
        assert!(!host.charge_retained);
    }
}

#[tokio::test]
async fn failed_creation_assignment_or_resume_stops_without_qmp() {
    for step in [Step::Create, Step::InspectChild, Step::Resume] {
        let mut host = Fake {
            fail: Some((step, JOB)),
            ..Fake::default()
        };
        assert_eq!(host.run().await.paused, Err(Failure::Refused(JOB)));
        assert!(
            host.steps
                .ends_with(&[Step::Stop, Step::Reopen, Step::Settle])
        );
        assert_eq!(host.steps.contains(&Step::Resume), step == Step::Resume);
        assert!(host.written.borrow().is_empty());
        assert!(!host.charge_retained);
    }
}

#[tokio::test]
async fn qmp_status_is_no_substitute_for_actual_opens() {
    let mut host = Fake {
        fail: Some((Step::VerifyOpens, OPENS)),
        ..Fake::default()
    };
    assert_eq!(host.run().await.paused, Err(Failure::Refused(OPENS)));
    assert!(
        host.steps
            .ends_with(&[Step::VerifyOpens, Step::Stop, Step::Reopen, Step::Settle])
    );
}

#[tokio::test]
async fn preflight_capability_cannot_substitute_for_whpx_initialization() {
    let refusal = NativeRefusal::WhpxUnverified;
    let mut host = Fake {
        fail: Some((Step::VerifyWhpx, refusal)),
        ..Fake::default()
    };
    assert_eq!(host.run().await.paused, Err(Failure::Refused(refusal)));
    assert!(host.steps.ends_with(&[
        Step::VerifyOpens,
        Step::VerifyWhpx,
        Step::Stop,
        Step::Reopen,
        Step::Settle
    ]));
}

#[tokio::test]
async fn greeting_reply_correlation_and_paused_state_fail_closed() {
    let stream = format!("{GREETING}\n{CAPABILITIES}\n{PRELAUNCH}\n");
    let cases = [
        (
            stream.replace("\"micro\":3", "\"micro\":2"),
            ChannelError::Invalid,
        ),
        (
            stream.replace("\"id\":1", "\"id\":0"),
            ChannelError::Unexpected,
        ),
        (
            stream.replace("\"id\":2", "\"id\":1"),
            ChannelError::Unexpected,
        ),
        (
            stream.replace("prelaunch", "paused"),
            ChannelError::Unexpected,
        ),
        (
            stream.replace("\"running\":false", "\"running\":true"),
            ChannelError::Invalid,
        ),
        (format!("{}\n", "x".repeat(65_537)), ChannelError::Oversize),
    ];
    for (replies, reason) in cases {
        let mut host = Fake {
            replies: Some(replies),
            ..Fake::default()
        };
        assert_eq!(host.run().await.paused, Err(Failure::Qmp(reason)));
        assert!(
            host.steps
                .ends_with(&[Step::Stop, Step::Reopen, Step::Settle])
        );
        assert!(!host.steps.contains(&Step::VerifyOpens));
    }
}

#[tokio::test]
async fn acknowledgement_cannot_cure_late_stdio_write() {
    let mut host = Fake {
        late_write: true,
        ..Fake::default()
    };
    assert_eq!(
        host.run().await.paused,
        Err(Failure::Qmp(ChannelError::Refused))
    );
    assert_eq!(
        host.written.borrow().as_slice(),
        b"{\"execute\":\"qmp_capabilities\",\"id\":1}\r\n"
    );
    assert!(!host.steps.contains(&Step::VerifyOpens));
}

#[tokio::test(start_paused = true)]
async fn stalled_peer_is_bounded_and_pressure_is_sampled_while_waiting() {
    let mut stalled = Fake {
        replies: None,
        ..Fake::default()
    };
    assert_eq!(stalled.run().await.paused, Err(Failure::Timeout));
    assert!(stalled.live_calls > 4);
    assert!(!stalled.charge_retained);

    let mut pressured = Fake {
        replies: None,
        live_override: Some((4, |sample| sample.idle = false)),
        ..Fake::default()
    };
    assert_eq!(pressured.run().await.paused, Err(Failure::OwnerActive));
    assert!(
        pressured
            .steps
            .ends_with(&[Step::Stop, Step::Reopen, Step::Settle])
    );
}

#[tokio::test]
async fn cancellation_before_claim_and_after_resume_suppresses_qmp() {
    let mut cancelled = Fake::default();
    cancelled.cancel.cancel();
    assert_eq!(cancelled.run().await.paused, Err(Failure::Cancelled));
    assert!(cancelled.steps.is_empty());

    let mut after_resume = Fake {
        cancel_on_resume: true,
        ..Fake::default()
    };
    assert_eq!(after_resume.run().await.paused, Err(Failure::Cancelled));
    assert!(
        after_resume
            .steps
            .ends_with(&[Step::Stop, Step::Reopen, Step::Settle])
    );
    assert!(after_resume.written.borrow().is_empty());
}

#[tokio::test]
async fn live_pressure_and_freshness_gate_creation_resume_and_prelaunch() {
    let mut before_claim = Fake {
        live_override: Some((0, |sample| {
            sample.limits.outer_caller_free_bytes = OUTER_FLOOR
        })),
        ..Fake::default()
    };
    assert_eq!(before_claim.run().await.paused, Err(Failure::Refused(DISK)));
    assert_eq!(before_claim.steps, [Step::ProveContract]);

    let mut before_create = Fake {
        live_override: Some((1, |sample| sample.whpx = false)),
        ..Fake::default()
    };
    assert_eq!(
        before_create.run().await.paused,
        Err(Failure::Refused(NativeRefusal::WhpxUnverified))
    );
    assert!(!before_create.steps.contains(&Step::Create));

    let mut before_resume = Fake {
        live_override: Some((2, |sample| {
            sample.captured -= FRESHNESS + Duration::from_millis(1)
        })),
        ..Fake::default()
    };
    assert_eq!(
        before_resume.run().await.paused,
        Err(Failure::Refused(RESERVES))
    );
    assert!(!before_resume.steps.contains(&Step::Resume));

    let mut after_prelaunch = Fake {
        live_override: Some((5, |sample| sample.available_ram_bytes = RAM_RESERVE)),
        ..Fake::default()
    };
    assert_eq!(
        after_prelaunch.run().await.paused,
        Err(Failure::Refused(RESERVES))
    );
    assert!(!after_prelaunch.steps.contains(&Step::VerifyOpens));
    assert!(!after_prelaunch.charge_retained);
}

#[tokio::test(start_paused = true)]
async fn missing_or_late_release_proof_keeps_the_slot_charged() {
    for stop in [
        Stop::ChildAlive,
        Stop::SupervisorLost,
        Stop::WorkerAlive,
        Stop::Late,
    ] {
        let mut host = Fake {
            stop,
            ..Fake::default()
        };
        assert_eq!(host.run().await.release, StopDisposition::ChargedQuarantine);
        assert!(!host.steps.contains(&Step::Reopen));
        assert!(host.charge_retained);
    }
    for (reopen, late_reopen) in [(false, false), (true, true)] {
        let mut host = Fake {
            reopen,
            late_reopen,
            ..Fake::default()
        };
        assert_eq!(host.run().await.release, StopDisposition::ChargedQuarantine);
        assert!(host.steps.contains(&Step::Reopen));
        assert!(host.charge_retained);
    }
}

#[test]
fn reserve_check_includes_nested_cpu_capacity_and_outside_job_ram() {
    let mut sample = live();
    sample.active_cpus = 5;
    sample.effective_cpus = 3;
    assert_eq!(
        sample.check(PEAK, Tick::now()),
        Err(Failure::Refused(RESERVES))
    );
    sample.effective_cpus = 5;
    assert_eq!(sample.check(PEAK, Tick::now()), Ok(()));
    sample.available_ram_bytes = RAM_RESERVE + JOB_MEMORY_BYTES as u64 + GIB - 1;
    assert_eq!(
        sample.check(PEAK, Tick::now()),
        Err(Failure::Refused(RESERVES))
    );
}

#[test]
fn accounting_checks_both_free_bounds_cap_and_outstanding_charges() {
    let mut sample = live();
    assert_eq!(sample.check_disk(PEAK), Ok(()));
    sample.outstanding_inner_bytes = 70 * GIB;
    assert_eq!(sample.check_disk(PEAK), Err(DISK));
    sample.outstanding_inner_bytes = 0;
    sample.outstanding_outer_bytes = 10 * GIB;
    assert_eq!(sample.check_disk(PEAK), Err(DISK));
    sample.outstanding_outer_bytes = 0;
    sample.limits.outer_total_free_bytes = OUTER_FLOOR;
    assert_eq!(sample.check_disk(PEAK), Err(DISK));
    sample.limits.outer_total_free_bytes = 60 * GIB;
    sample.limits.disk_budget_bytes = 98 * GIB;
    assert_eq!(sample.check_disk(PEAK), Err(DISK));
    sample.limits.inner_total_bytes = DISK_CEILING + GIB;
    sample.limits.disk_budget_bytes = DISK_CEILING + GIB;
    assert_eq!(sample.check_disk(PEAK), Err(DISK));
}

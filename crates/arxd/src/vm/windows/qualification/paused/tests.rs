use super::*;
use std::{
    cell::{Cell, RefCell},
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

struct DelayedGreeting {
    delay: Pin<Box<tokio::time::Sleep>>,
    replies: Cursor<Vec<u8>>,
}

impl AsyncRead for DelayedGreeting {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.delay.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        Pin::new(&mut self.replies).poll_read(cx, buf)
    }
}

/// Deliver bytes in the same poll that crosses the sample deadline.
struct AdvancingGreeting {
    elapsed: Option<Duration>,
    replies: Cursor<Vec<u8>>,
}

impl AsyncRead for AdvancingGreeting {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Some(elapsed) = self.elapsed.take() {
            // Tokio 1.48 advances its paused clock before the first yield.
            let _ = std::pin::pin!(tokio::time::advance(elapsed)).poll(cx);
        }
        Pin::new(&mut self.replies).poll_read(cx, buffer)
    }
}

struct FakeWriter {
    written: Rc<RefCell<Vec<u8>>>,
    dispatch: Arc<DispatchGate>,
    late: bool,
    queued_lag: Option<Duration>,
}

impl AsyncWrite for FakeWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Some(lag) = self.queued_lag.take() {
            // The worker can be scheduled after the supervisor submits.
            let _ = std::pin::pin!(tokio::time::advance(lag)).poll(cx);
        }
        // The fixture's async deadlines and pipe-worker dispatch share the
        // same clock, including when Tokio advances it in a paused test.
        let now = Tick::now().into_std();
        *self.dispatch.test.clock.lock().unwrap() = Some(now);
        if !self.dispatch.begin_write() {
            return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
        }
        let written =
            self.dispatch
                .write_pipe(&mut *self.written.borrow_mut(), bytes, &Shutdown::default());
        if self.late {
            // The peer may already have replied. Set the worker clock past
            // the tracked window before recording the completion.
            *self.dispatch.test.clock.lock().unwrap() = Some(now + FRESHNESS);
        }
        self.dispatch.end_write(matches!(written, Ok(true)));
        Poll::Ready(written.and_then(|sent| {
            if sent {
                Ok(bytes.len())
            } else {
                Err(io::ErrorKind::TimedOut.into())
            }
        }))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

type LiveOverride = (usize, fn(&mut Live));
type FakeReader = Box<dyn AsyncRead + Unpin>;

struct ProofNotification {
    step: Step,
    receiver: RefCell<Option<tokio::sync::oneshot::Receiver<()>>>,
}

struct FakeSlot;
struct FakeOpens;

struct Fake {
    cancel: Shutdown,
    steps: RefCell<Vec<Step>>,
    written: Rc<RefCell<Vec<u8>>>,
    replies: Option<String>,
    reader_override: RefCell<Option<FakeReader>>,
    greeting_delay: Option<Duration>,
    peer_hold: RefCell<Option<tokio::io::DuplexStream>>,
    dispatch: RefCell<Option<Arc<DispatchGate>>>,
    fail: Option<(Step, NativeRefusal)>,
    live_override: Option<LiveOverride>,
    expiring_sample: Option<usize>,
    live_calls: Cell<usize>,
    stop: Stop,
    reopen: bool,
    late_reopen: bool,
    late_write: bool,
    queued_write_lag: Option<Duration>,
    pre_resume_lag: Option<Duration>,
    cancel_on_resume: bool,
    charge_retained: bool,
    stall_at: Option<Step>,
    slow_at: Option<Step>,
    proof_workspace_at: Option<Step>,
    proof_notification: Option<ProofNotification>,
    proof_workspace_held: Cell<bool>,
    owner_active_at: Option<Step>,
    owner_active: Cell<bool>,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            cancel: Shutdown::default(),
            steps: RefCell::default(),
            written: Rc::default(),
            replies: Some(format!("{GREETING}\n{CAPABILITIES}\n{PRELAUNCH}\n")),
            reader_override: RefCell::default(),
            greeting_delay: None,
            peer_hold: RefCell::default(),
            dispatch: RefCell::default(),
            fail: None,
            live_override: None,
            expiring_sample: None,
            live_calls: Cell::default(),
            stop: Stop::Confirmed,
            reopen: true,
            late_reopen: false,
            late_write: false,
            queued_write_lag: None,
            pre_resume_lag: None,
            cancel_on_resume: false,
            charge_retained: false,
            stall_at: None,
            slow_at: None,
            proof_workspace_at: None,
            proof_notification: None,
            proof_workspace_held: Cell::default(),
            owner_active_at: None,
            owner_active: Cell::default(),
        }
    }
}

impl Fake {
    fn step(&self, step: Step) -> Result<(), NativeRefusal> {
        self.steps.borrow_mut().push(step);
        if self.owner_active_at == Some(step) {
            self.owner_active.set(true);
        }
        if let Some((when, failure)) = self.fail
            && when == step
        {
            return Err(failure);
        }
        Ok(())
    }

    fn steps(&self) -> Vec<Step> {
        self.steps.borrow().clone()
    }

    async fn checkpoint(&self, step: Step) {
        if self.stall_at == Some(step) {
            std::future::pending::<()>().await;
        }
        if self.slow_at == Some(step) {
            tokio::time::sleep(BUDGET * 2).await;
        }
        if self.proof_workspace_at == Some(step) {
            self.proof_workspace_held.set(true);
            tokio::time::sleep(PRESSURE_INTERVAL).await;
            self.proof_workspace_held.set(false);
        }
        if let Some(notification) = &self.proof_notification
            && notification.step == step
        {
            self.proof_workspace_held.set(true);
            let receiver = notification.receiver.borrow_mut().take().unwrap();
            receiver.await.unwrap();
            self.proof_workspace_held.set(false);
        }
    }

    async fn run(&mut self) -> TrialResult {
        self.run_for(BUDGET).await
    }

    async fn run_for(&mut self, budget: Duration) -> TrialResult {
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
        run(self, &plan, &cancel, Tick::now() + budget, BUDGET).await
    }
}

impl Host for Fake {
    type Opens = FakeOpens;
    type Slot = FakeSlot;
    type Reader = FakeReader;
    type Writer = FakeWriter;

    fn open_contract(&mut self, _: &PausedPlan<'_>) -> Result<(FakeOpens, Peak), NativeRefusal> {
        self.step(Step::ProveContract)?;
        Ok((FakeOpens, PEAK))
    }

    fn live(&self) -> Result<Live, NativeRefusal> {
        let mut observation = live();
        if self.expiring_sample == Some(self.live_calls.get()) {
            observation.captured -= FRESHNESS - OBSERVATION_REMAINING;
        }
        if let Some(dispatch) = &*self.dispatch.borrow() {
            *dispatch.test.clock.lock().unwrap() = Some(observation.captured.into_std());
        }
        if let Some((at, mutate)) = self.live_override
            && self.live_calls.get() == at
        {
            mutate(&mut observation);
        }
        self.live_calls.set(self.live_calls.get() + 1);
        if self.owner_active.get() {
            observation.idle = false;
        }
        if self.proof_workspace_held.get() {
            observation.available_ram_bytes = RAM_RESERVE;
        }
        Ok(observation)
    }

    fn claim(&mut self, _: &Live, _: Peak) -> Result<FakeSlot, NativeRefusal> {
        self.step(Step::Claim)?;
        assert!(!self.charge_retained);
        self.charge_retained = true;
        Ok(FakeSlot)
    }

    async fn confirm_claim(&self, _: &FakeSlot) -> Result<(), NativeRefusal> {
        self.checkpoint(Step::Claim).await;
        Ok(())
    }

    async fn create_suspended(
        &self,
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
        self.step(Step::Create)?;
        self.checkpoint(Step::Create).await;
        Ok(())
    }

    async fn verify_child(
        &self,
        _: &FakeSlot,
    ) -> Result<Pipes<Self::Reader, Self::Writer>, NativeRefusal> {
        self.step(Step::InspectChild)?;
        self.checkpoint(Step::InspectChild).await;
        let stdout: Self::Reader = if let Some(reader) = self.reader_override.borrow_mut().take() {
            reader
        } else {
            match &self.replies {
                Some(replies) => {
                    let replies = Cursor::new(replies.as_bytes().to_vec());
                    if let Some(delay) = self.greeting_delay {
                        Box::new(DelayedGreeting {
                            delay: Box::pin(tokio::time::sleep(delay)),
                            replies,
                        })
                    } else {
                        Box::new(replies)
                    }
                }
                None => {
                    let (read, write) = tokio::io::duplex(64);
                    *self.peer_hold.borrow_mut() = Some(write);
                    Box::new(read)
                }
            }
        };
        let dispatch = Arc::new(DispatchGate::default());
        *self.dispatch.borrow_mut() = Some(dispatch.clone());
        let stdin = FakeWriter {
            written: self.written.clone(),
            dispatch: dispatch.clone(),
            late: self.late_write,
            queued_lag: self.queued_write_lag,
        };
        Ok(Pipes {
            stdout,
            stdin,
            dispatch,
        })
    }

    fn resume(&self, _: &FakeSlot) -> impl Future<Output = Result<(), NativeRefusal>> {
        if let Some(lag) = self.pre_resume_lag {
            // Simulate preemption after sampling and before the first resume
            // poll. The resume effect itself remains inside that future.
            let mut cx = Context::from_waker(std::task::Waker::noop());
            let _ = std::pin::pin!(tokio::time::advance(lag)).poll(&mut cx);
            self.owner_active.set(true);
        }
        async move {
            self.step(Step::Resume)?;
            self.checkpoint(Step::Resume).await;
            if self.cancel_on_resume {
                self.cancel.cancel();
            }
            Ok(())
        }
    }

    async fn verify_opens(&self, _: &FakeSlot, _: &FakeOpens) -> Result<(), NativeRefusal> {
        self.step(Step::VerifyOpens)?;
        self.checkpoint(Step::VerifyOpens).await;
        Ok(())
    }

    async fn verify_whpx(&self, _: &FakeSlot) -> Result<(), NativeRefusal> {
        self.step(Step::VerifyWhpx)?;
        self.checkpoint(Step::VerifyWhpx).await;
        Ok(())
    }

    fn request_stop(&mut self, _: &FakeSlot) {
        self.steps.borrow_mut().push(Step::Stop);
        self.proof_workspace_held.set(false);
    }

    async fn observe_stop(&mut self, _: &FakeSlot, deadline: Tick) -> StopEvidence {
        assert!(self.steps().ends_with(&[Step::Stop]));
        self.checkpoint(Step::Stop).await;
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
        self.steps.borrow_mut().push(Step::Reopen);
        self.checkpoint(Step::Reopen).await;
        if self.late_reopen {
            tokio::time::sleep_until(deadline).await;
        }
        self.reopen
    }

    fn settle(&mut self, _: FakeSlot, disposition: StopDisposition) {
        self.steps.borrow_mut().push(Step::Settle);
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
    assert_eq!(host.live_calls.get(), 9);
    assert_eq!(
        host.steps(),
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
        assert_eq!(host.steps(), expected);
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
            host.steps()
                .ends_with(&[Step::Stop, Step::Reopen, Step::Settle])
        );
        assert_eq!(host.steps().contains(&Step::Resume), step == Step::Resume);
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
        host.steps()
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
    assert!(host.steps().ends_with(&[
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
            host.steps()
                .ends_with(&[Step::Stop, Step::Reopen, Step::Settle])
        );
        assert!(!host.steps().contains(&Step::VerifyOpens));
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
    assert!(!host.steps().contains(&Step::VerifyOpens));
}

#[tokio::test(start_paused = true)]
async fn stalled_peer_is_bounded_and_pressure_is_sampled_while_waiting() {
    let mut stalled = Fake {
        replies: None,
        ..Fake::default()
    };
    assert_eq!(stalled.run().await.paused, Err(Failure::Timeout));
    assert!(stalled.live_calls.get() > 5);
    assert!(!stalled.charge_retained);

    let mut pressured = Fake {
        replies: None,
        live_override: Some((5, |sample| sample.idle = false)),
        ..Fake::default()
    };
    assert_eq!(pressured.run().await.paused, Err(Failure::OwnerActive));
    assert!(
        pressured
            .steps()
            .ends_with(&[Step::Stop, Step::Reopen, Step::Settle])
    );
}

#[tokio::test]
async fn cancellation_before_claim_and_after_resume_suppresses_qmp() {
    let mut cancelled = Fake::default();
    cancelled.cancel.cancel();
    assert_eq!(cancelled.run().await.paused, Err(Failure::Cancelled));
    assert!(cancelled.steps().is_empty());

    let mut after_resume = Fake {
        cancel_on_resume: true,
        ..Fake::default()
    };
    assert_eq!(after_resume.run().await.paused, Err(Failure::Cancelled));
    assert!(
        after_resume
            .steps()
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
    assert_eq!(before_claim.steps(), [Step::ProveContract]);

    let mut before_create = Fake {
        live_override: Some((1, |sample| sample.whpx = false)),
        ..Fake::default()
    };
    assert_eq!(
        before_create.run().await.paused,
        Err(Failure::Refused(NativeRefusal::WhpxUnverified))
    );
    assert!(!before_create.steps().contains(&Step::Create));

    let mut before_resume = Fake {
        live_override: Some((3, |sample| {
            sample.captured -= FRESHNESS + Duration::from_millis(1)
        })),
        ..Fake::default()
    };
    assert_eq!(
        before_resume.run().await.paused,
        Err(Failure::Refused(RESERVES))
    );
    assert!(!before_resume.steps().contains(&Step::Resume));

    let mut after_prelaunch = Fake {
        live_override: Some((6, |sample| sample.available_ram_bytes = RAM_RESERVE)),
        ..Fake::default()
    };
    assert_eq!(
        after_prelaunch.run().await.paused,
        Err(Failure::Refused(RESERVES))
    );
    assert!(!after_prelaunch.steps().contains(&Step::VerifyOpens));
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
        assert!(!host.steps().contains(&Step::Reopen));
        assert!(host.charge_retained);
    }
    for (reopen, late_reopen) in [(false, false), (true, true)] {
        let mut host = Fake {
            reopen,
            late_reopen,
            ..Fake::default()
        };
        assert_eq!(host.run().await.release, StopDisposition::ChargedQuarantine);
        assert!(host.steps().contains(&Step::Reopen));
        assert!(host.charge_retained);
    }
}

#[tokio::test(start_paused = true)]
async fn timely_settlement_releases_the_charge_after_live_deadlines() {
    for (lag, expiring, cutoff) in [
        (9, true, 1000),
        (10, true, 1000),
        (251, false, 1000),
        (21, false, 20),
    ] {
        let (release, proof) = tokio::sync::oneshot::channel();
        let mut host = Fake {
            expiring_sample: expiring.then_some(8),
            live_override: Some((9, |sample| sample.idle = false)),
            proof_notification: Some(ProofNotification {
                step: Step::Reopen,
                receiver: RefCell::new(Some(proof)),
            }),
            ..Fake::default()
        };
        // Start exclusive reopen before advancing time. The live deadline
        // can pass while the independent settlement deadline is still open.
        let (result, ()) = tokio::join!(
            biased;
            host.run_for(Duration::from_millis(cutoff)),
            async {
                tokio::time::advance(Duration::from_millis(lag)).await;
                release.send(()).unwrap();
            }
        );
        assert_eq!(result.paused, Ok(()));
        assert_eq!(result.release, StopDisposition::DirtyRelease);
        assert!(
            host.steps()
                .ends_with(&[Step::Stop, Step::Reopen, Step::Settle])
        );
        assert!(!host.charge_retained);
        assert_eq!(host.live_calls.get(), 9);
    }
}

#[tokio::test(start_paused = true)]
async fn dropping_pending_trial_stops_and_quarantines_every_charged_phase() {
    for (stall_at, last_before_stop) in [
        (Some(Step::Claim), Step::Claim),
        (Some(Step::Create), Step::Create),
        (Some(Step::InspectChild), Step::InspectChild),
        (Some(Step::Resume), Step::Resume),
        (None, Step::Resume), // No greeting arrives.
        (Some(Step::VerifyOpens), Step::VerifyOpens),
        (Some(Step::VerifyWhpx), Step::VerifyWhpx),
        (Some(Step::Stop), Step::Stop),
        (Some(Step::Reopen), Step::Reopen),
    ] {
        let mut host = Fake {
            stall_at,
            ..Fake::default()
        };
        if stall_at.is_none() {
            host.replies = None;
        }
        let mut trial = Box::pin(host.run());
        let mut ctx = Context::from_waker(std::task::Waker::noop());
        assert!(trial.as_mut().poll(&mut ctx).is_pending(), "{stall_at:?}");
        drop(trial);

        let steps = host.steps();
        let before_settle = steps
            .strip_suffix(&[Step::Settle])
            .expect("quarantine recorded");
        assert_eq!(steps.iter().filter(|&&step| step == Step::Stop).count(), 1);
        if matches!(last_before_stop, Step::Stop | Step::Reopen) {
            assert_eq!(before_settle.last(), Some(&last_before_stop));
        } else {
            assert!(before_settle.ends_with(&[last_before_stop, Step::Stop]));
        }
        assert!(host.charge_retained, "{stall_at:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn cancellation_during_a_pending_claim_or_proof_settles_release() {
    for step in [Step::Claim, Step::VerifyOpens] {
        let mut host = Fake {
            stall_at: Some(step),
            ..Fake::default()
        };
        let cancel = host.cancel.clone();
        let mut trial = Box::pin(host.run());
        let mut ctx = Context::from_waker(std::task::Waker::noop());
        assert!(trial.as_mut().poll(&mut ctx).is_pending());
        cancel.cancel();
        assert_eq!(
            trial.await,
            TrialResult {
                paused: Err(Failure::Cancelled),
                release: StopDisposition::DirtyRelease,
            }
        );
        assert!(
            host.steps()
                .ends_with(&[step, Step::Stop, Step::Reopen, Step::Settle])
        );
        assert!(!host.charge_retained);
    }
}

#[tokio::test(start_paused = true)]
async fn owner_activity_during_a_stalled_live_phase_refuses_and_stops() {
    for step in [
        Step::Create,
        Step::InspectChild,
        Step::Resume,
        Step::VerifyOpens,
        Step::VerifyWhpx,
    ] {
        let mut host = Fake {
            stall_at: Some(step),
            owner_active_at: Some(step),
            ..Fake::default()
        };
        let start = Tick::now();
        let result = host.run().await;
        assert_eq!(result.paused, Err(Failure::OwnerActive), "{step:?}");
        assert_eq!(result.release, StopDisposition::DirtyRelease);
        assert!(Tick::now() - start < BUDGET);
        assert!(
            host.steps()
                .ends_with(&[step, Step::Stop, Step::Reopen, Step::Settle])
        );
    }
}

#[tokio::test(start_paused = true)]
async fn slow_proof_is_cut_off_before_next_stage_and_before_success() {
    for proof in [Step::VerifyOpens, Step::VerifyWhpx] {
        let mut host = Fake {
            slow_at: Some(proof),
            ..Fake::default()
        };
        let started = Tick::now();
        assert_eq!(host.run().await.paused, Err(Failure::Timeout));
        assert!(Tick::now() - started < BUDGET * 2);
        assert!(
            host.steps()
                .ends_with(&[proof, Step::Stop, Step::Reopen, Step::Settle])
        );
    }
}

#[tokio::test]
async fn immediate_proof_completion_cannot_hide_new_owner_activity() {
    for proof in [Step::VerifyOpens, Step::VerifyWhpx] {
        let mut host = Fake {
            owner_active_at: Some(proof),
            ..Fake::default()
        };
        assert_eq!(host.run().await.paused, Err(Failure::OwnerActive));
        assert!(
            host.steps()
                .ends_with(&[proof, Step::Stop, Step::Reopen, Step::Settle])
        );
    }
}

async fn assert_due_pressure_precedes_proof_release(proof: Step) {
    let mut host = Fake {
        proof_workspace_at: Some(proof),
        ..Fake::default()
    };
    let mut trial = Box::pin(host.run());
    let mut ctx = Context::from_waker(std::task::Waker::noop());
    assert!(trial.as_mut().poll(&mut ctx).is_pending());

    // Both timers mature before the next poll, well inside the cutoff.
    // The proof would restore RAM on its completion poll.
    tokio::time::advance(PRESSURE_INTERVAL + Duration::from_millis(50)).await;
    assert_eq!(
        trial.await,
        TrialResult {
            paused: Err(Failure::Refused(RESERVES)),
            release: StopDisposition::DirtyRelease,
        }
    );
    assert!(
        host.steps()
            .ends_with(&[proof, Step::Stop, Step::Reopen, Step::Settle])
    );
}

#[tokio::test(start_paused = true)]
async fn due_pressure_precedes_consumer_open_workspace_release() {
    assert_due_pressure_precedes_proof_release(Step::VerifyOpens).await;
}

#[tokio::test(start_paused = true)]
async fn due_pressure_precedes_whpx_workspace_release() {
    assert_due_pressure_precedes_proof_release(Step::VerifyWhpx).await;
}

#[tokio::test(start_paused = true)]
async fn due_pressure_precedes_qmp_greeting_and_capabilities_write() {
    let mut host = Fake {
        greeting_delay: Some(PRESSURE_INTERVAL),
        // The first sample scheduled while awaiting the greeting sees input.
        live_override: Some((5, |sample| sample.idle = false)),
        ..Fake::default()
    };
    let mut trial = Box::pin(host.run());
    let mut ctx = Context::from_waker(std::task::Waker::noop());
    assert!(trial.as_mut().poll(&mut ctx).is_pending());

    tokio::time::advance(PRESSURE_INTERVAL + Duration::from_millis(50)).await;
    assert_eq!(
        trial.await,
        TrialResult {
            paused: Err(Failure::OwnerActive),
            release: StopDisposition::DirtyRelease,
        }
    );
    assert!(host.written.borrow().is_empty());
    assert!(
        host.steps()
            .ends_with(&[Step::Resume, Step::Stop, Step::Reopen, Step::Settle])
    );
}

const INDEPENDENT_READY_AT: Duration = Duration::from_micros(250_100);

async fn assert_independently_ready_proof_refuses(proof: Step) {
    // At the signal the monotonic deadline has passed, but Tokio's rounded
    // timer has not reached its millisecond tick yet.
    tokio::time::advance(Duration::from_micros(500)).await;
    let (signal, receiver) = tokio::sync::oneshot::channel();
    let mut host = Fake {
        proof_notification: Some(ProofNotification {
            step: proof,
            receiver: RefCell::new(Some(receiver)),
        }),
        ..Fake::default()
    };
    let (result, ()) = tokio::join!(biased; host.run(), async {
        tokio::time::advance(INDEPENDENT_READY_AT).await;
        let _ = signal.send(());
    });
    assert_eq!(
        result,
        TrialResult {
            paused: Err(Failure::Refused(RESERVES)),
            release: StopDisposition::DirtyRelease,
        }
    );
    assert!(
        host.steps()
            .ends_with(&[proof, Step::Stop, Step::Reopen, Step::Settle])
    );
}

#[tokio::test(start_paused = true)]
async fn independently_ready_consumer_open_refuses_before_releasing_ram() {
    assert_independently_ready_proof_refuses(Step::VerifyOpens).await;
}

#[tokio::test(start_paused = true)]
async fn independently_ready_whpx_refuses_before_releasing_ram() {
    assert_independently_ready_proof_refuses(Step::VerifyWhpx).await;
}

#[tokio::test(start_paused = true)]
async fn independently_ready_greeting_refuses_without_qmp_writes() {
    use tokio::io::AsyncWriteExt;

    tokio::time::advance(Duration::from_micros(500)).await;
    let (reader, mut peer) = tokio::io::duplex(4096);
    let mut host = Fake {
        reader_override: RefCell::new(Some(Box::new(reader))),
        live_override: Some((5, |sample| sample.idle = false)),
        ..Fake::default()
    };
    let (result, ()) = tokio::join!(biased; host.run(), async {
        tokio::time::advance(INDEPENDENT_READY_AT).await;
        let replies = format!("{GREETING}\n{CAPABILITIES}\n{PRELAUNCH}\n");
        let _ = peer.write_all(replies.as_bytes()).await;
    });
    assert_activity_refusal_without_qmp_writes(&host, result);
}

#[tokio::test(start_paused = true)]
async fn due_pressure_between_qmp_read_and_write_in_one_poll_refuses() {
    let replies = format!("{GREETING}\n{CAPABILITIES}\n{PRELAUNCH}\n");
    let mut host = Fake {
        reader_override: RefCell::new(Some(Box::new(AdvancingGreeting {
            elapsed: Some(INDEPENDENT_READY_AT),
            replies: Cursor::new(replies.into_bytes()),
        }))),
        live_override: Some((5, |sample| sample.idle = false)),
        ..Fake::default()
    };
    let result = host.run().await;
    assert_activity_refusal_without_qmp_writes(&host, result);
}

fn assert_activity_refusal_without_qmp_writes(host: &Fake, result: TrialResult) {
    assert_eq!(
        result,
        TrialResult {
            paused: Err(Failure::OwnerActive),
            release: StopDisposition::DirtyRelease,
        }
    );
    assert!(host.written.borrow().is_empty());
    assert!(
        host.steps()
            .ends_with(&[Step::Resume, Step::Stop, Step::Reopen, Step::Settle])
    );
}

const OBSERVATION_REMAINING: Duration = Duration::from_millis(10);
const EXPIRED_AFTER: Duration = Duration::from_millis(11);

#[tokio::test(start_paused = true)]
async fn expiring_observation_gates_the_first_resume_poll() {
    for (lag, resumed) in [
        (Duration::from_millis(9), true),
        (OBSERVATION_REMAINING, false),
        (EXPIRED_AFTER, false),
    ] {
        let mut host = Fake {
            expiring_sample: Some(3),
            pre_resume_lag: Some(lag),
            ..Fake::default()
        };
        assert_eq!(
            host.run().await,
            TrialResult {
                paused: Err(Failure::OwnerActive),
                release: StopDisposition::DirtyRelease,
            },
        );
        let steps = host.steps();
        assert_eq!(steps.contains(&Step::Resume), resumed, "{lag:?}");
        assert!(steps.ends_with(&[Step::Stop, Step::Reopen, Step::Settle]));
    }
}

#[tokio::test(start_paused = true)]
async fn expiring_observation_gates_proof_completion_and_pause_acceptance() {
    for (proof, sample_call) in [(Step::VerifyOpens, 6), (Step::VerifyWhpx, 7)] {
        let (signal, receiver) = tokio::sync::oneshot::channel();
        let mut host = Fake {
            expiring_sample: Some(sample_call),
            proof_notification: Some(ProofNotification {
                step: proof,
                receiver: RefCell::new(Some(receiver)),
            }),
            ..Fake::default()
        };
        let (result, ()) = tokio::join!(biased; host.run(), async {
            tokio::time::advance(EXPIRED_AFTER).await;
            let _ = signal.send(());
        });
        // The proof holds simulated RAM until its completion poll. A
        // replacement observation must see that RAM before it is released.
        assert_eq!(
            result,
            TrialResult {
                paused: Err(Failure::Refused(RESERVES)),
                release: StopDisposition::DirtyRelease,
            },
            "{proof:?}",
        );
        assert!(
            host.steps()
                .ends_with(&[proof, Step::Stop, Step::Reopen, Step::Settle])
        );
    }
}

#[tokio::test(start_paused = true)]
async fn expiring_observation_while_a_proof_returns_ready_requires_resampling() {
    let cancel = Shutdown::default();
    let now = Tick::now();
    let captured = now - (FRESHNESS - OBSERVATION_REMAINING);
    let gate = LivePoll::new(
        &cancel,
        &|| Err(Failure::OwnerActive),
        Freshness::new(captured, now, now + BUDGET),
    );
    let result = gate
        .watch(poll_fn(|cx| {
            // Start the poll with valid evidence, then cross expiry before
            // the result is available to the accepting caller.
            let _ = std::pin::pin!(tokio::time::advance(EXPIRED_AFTER)).poll(cx);
            Poll::Ready(Ok(()))
        }))
        .await;
    assert_eq!(result, Err(Failure::OwnerActive));
}

#[tokio::test(start_paused = true)]
async fn expiring_observation_in_the_final_sample_prevents_accepting_pause() {
    let mut host = Fake {
        live_override: Some((8, |sample| sample.captured -= FRESHNESS)),
        ..Fake::default()
    };
    assert_eq!(
        host.run().await,
        TrialResult {
            paused: Err(Failure::Refused(RESERVES)),
            release: StopDisposition::DirtyRelease,
        },
    );
    assert!(
        host.steps()
            .ends_with(&[Step::VerifyWhpx, Step::Stop, Step::Reopen, Step::Settle])
    );
}

#[tokio::test(start_paused = true)]
async fn expiring_observation_prevents_a_qmp_command_after_readiness() {
    let replies = format!("{GREETING}\n{CAPABILITIES}\n{PRELAUNCH}\n");
    let mut host = Fake {
        reader_override: RefCell::new(Some(Box::new(AdvancingGreeting {
            elapsed: Some(EXPIRED_AFTER),
            replies: Cursor::new(replies.into_bytes()),
        }))),
        expiring_sample: Some(4),
        live_override: Some((5, |sample| sample.idle = false)),
        ..Fake::default()
    };
    let result = host.run().await;
    assert_activity_refusal_without_qmp_writes(&host, result);
}

#[tokio::test(start_paused = true)]
async fn a_queued_qmp_write_cannot_outlive_its_observation_or_pressure_deadline() {
    for (expiring_sample, lag) in [
        (Some(4), EXPIRED_AFTER),
        (None, PRESSURE_INTERVAL + Duration::from_millis(1)),
    ] {
        let mut host = Fake {
            expiring_sample,
            queued_write_lag: Some(lag),
            ..Fake::default()
        };
        assert_eq!(
            host.run().await,
            TrialResult {
                paused: Err(Failure::Qmp(ChannelError::Io)),
                release: StopDisposition::DirtyRelease,
            }
        );
        assert!(host.written.borrow().is_empty());
        assert!(
            host.steps()
                .ends_with(&[Step::Resume, Step::Stop, Step::Reopen, Step::Settle])
        );
    }
}

#[test]
fn a_qmp_producer_waiting_for_dispatch_does_not_enqueue_past_cutoff() {
    use std::{sync::mpsc, thread};

    let dispatch = Arc::new(DispatchGate::default());
    let (locked, check_locked) = mpsc::channel();
    let (attempt, check_attempt) = mpsc::channel();
    let bound = Duration::from_secs(5);
    let worker_gate = dispatch.clone();
    let worker = thread::spawn(move || {
        worker_gate.state.send_modify(|_| {
            locked.send(()).unwrap();
            check_attempt.recv_timeout(bound).unwrap();
            thread::sleep(Duration::from_millis(90));
        });
    });
    check_locked.recv_timeout(bound).unwrap();
    let start = Tick::now();
    let cancel = Shutdown::default();
    let live = LivePoll::new(
        &cancel,
        &|| Ok(Tick::now()),
        Freshness::new(start, start, start + Duration::from_millis(40)),
    );
    let mut writer = LiveWriter {
        stream: LiveIo {
            gate: &live,
            inner: Vec::new(),
        },
        dispatch: &dispatch,
    };
    attempt.send(()).unwrap();
    let outcome = Pin::new(&mut writer).poll_write(
        &mut Context::from_waker(std::task::Waker::noop()),
        b"fixture",
    );
    worker.join().unwrap();
    assert!(
        matches!(outcome, Poll::Ready(Err(error)) if error.kind() == io::ErrorKind::PermissionDenied)
    );
    assert!(writer.stream.inner.is_empty());
    assert_eq!(live.refused.get(), Some(Failure::Timeout));
    assert_eq!(dispatch.current(), Dispatch::Idle);
}

#[tokio::test(start_paused = true)]
async fn a_healthy_delayed_greeting_arms_from_fresh_evidence_at_dispatch() {
    let mut host = Fake {
        greeting_delay: Some(PRESSURE_INTERVAL + Duration::from_millis(50)),
        ..Fake::default()
    };
    assert_eq!(host.run().await.paused, Ok(()));
    assert_eq!(
        host.written.borrow().as_slice(),
        b"{\"execute\":\"qmp_capabilities\",\"id\":1}\r\n{\"execute\":\"query-status\",\"id\":2}\r\n"
    );
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

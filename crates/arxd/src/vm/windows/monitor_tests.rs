use super::*;
use std::cell::Cell;
use tokio::time::{Instant, sleep};

#[tokio::test(start_paused = true)]
async fn late_readiness_cannot_outrun_owner_activity() {
    let completed = Cell::new(false);
    let samples = Cell::new(0);
    let start = Instant::now();
    let result = monitor_running(
        async {
            sleep(LIVE_POLL * 2).await;
            completed.set(true);
            Ok(())
        },
        || {
            samples.set(samples.get() + 1);
            if samples.get() < 3 {
                Ok(())
            } else {
                Err(NativeRefusal::OwnerActivityUnverified)
            }
        },
        &Shutdown::default(),
        Instant::now() + Duration::from_secs(10),
        |_| NativeRefusal::QmpUnverified,
    )
    .await;
    assert_eq!(result, Err(NativeRefusal::OwnerActivityUnverified));
    assert_eq!(start.elapsed(), LIVE_POLL * 2);
    assert!(!completed.get());
}

#[tokio::test(start_paused = true)]
async fn pending_script_is_dropped_on_live_job_loss() {
    struct Running<'a>(&'a Cell<bool>);

    impl Drop for Running<'_> {
        fn drop(&mut self) {
            self.0.set(false);
        }
    }

    let running = Cell::new(false);
    let result = monitor_running(
        async {
            running.set(true);
            let _guard = Running(&running);
            sleep(LIVE_POLL * 3).await;
            Ok(())
        },
        || {
            if running.get() {
                Err(NativeRefusal::JobUnverified)
            } else {
                Ok(())
            }
        },
        &Shutdown::default(),
        Instant::now() + Duration::from_secs(10),
        |_| NativeRefusal::QmpUnverified,
    )
    .await;
    assert_eq!(result, Err(NativeRefusal::JobUnverified));
    assert!(!running.get());
}

#[tokio::test(start_paused = true)]
async fn helper_output_is_withheld_on_completion_sample_failure() {
    let remaining_capacity = Cell::new(true);
    let result = monitor_running(
        async {
            remaining_capacity.set(false);
            Ok(b"helper output")
        },
        || {
            remaining_capacity
                .get()
                .then_some(())
                .ok_or(NativeRefusal::DiskCeilingUnverified)
        },
        &Shutdown::default(),
        Instant::now() + Duration::from_secs(10),
        |_| NativeRefusal::QmpUnverified,
    )
    .await;
    assert_eq!(result, Err(NativeRefusal::DiskCeilingUnverified));
}

#[tokio::test(start_paused = true)]
async fn a_long_successful_operation_is_sampled_until_the_result() {
    let samples = Cell::new(0);
    let result = monitor_running(
        async {
            sleep(LIVE_POLL * 2 + LIVE_POLL / 2).await;
            Ok::<_, NativeRefusal>(7)
        },
        || {
            samples.set(samples.get() + 1);
            Ok(())
        },
        &Shutdown::default(),
        Instant::now() + Duration::from_secs(10),
        |_| NativeRefusal::QmpUnverified,
    )
    .await;
    assert_eq!(result, Ok(7));
    assert_eq!(samples.get(), 4);
}

#[tokio::test(start_paused = true)]
async fn cancellation_during_final_observation_wins() {
    let cancel = Shutdown::default();
    let completed = Cell::new(false);
    let result = monitor_running(
        async {
            completed.set(true);
            Ok(7)
        },
        || {
            if completed.get() {
                cancel.cancel();
            }
            Ok(())
        },
        &cancel,
        Instant::now() + LIVE_POLL,
        |error| error,
    )
    .await;
    assert_eq!(result, Err(ChannelError::Cancelled));
}

#[tokio::test(start_paused = true)]
async fn observes_work_error_and_preserves_reason() {
    let samples = Cell::new(0);
    let result = monitor_running(
        async { Err::<(), _>(ChannelError::Remote) },
        || {
            samples.set(samples.get() + 1);
            Ok(())
        },
        &Shutdown::default(),
        Instant::now() + LIVE_POLL,
        |error| error,
    )
    .await;
    assert_eq!(result, Err(ChannelError::Remote));
    assert_eq!(samples.get(), 2);
}

struct DelayedExit(Cell<bool>);

impl InstallerChild for DelayedExit {
    fn observe(&self, _: std::time::Instant) -> Result<(), NativeRefusal> {
        Ok(())
    }

    async fn dispatch(
        &self,
        qmp: &mut PipeQmp,
        cancel: &Shutdown,
        _: InstallerStage,
        mut refresh: impl FnMut() -> Result<std::time::Instant, NativeRefusal>,
    ) -> Result<(), InstallerFailureReason> {
        Ok(qmp
            .release_startup_hold(cancel, || refresh().is_ok())
            .await?)
    }

    async fn exit(&self, _: Instant) -> Result<(), InstallerFailureReason> {
        self.0.set(true);
        sleep(LIVE_POLL + LIVE_POLL / 2).await;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn installer_monitors_silent_accelerator_before_dispatch() {
    use crate::vm::runtime::{InstallerBudget, InstallerPhase, InstallerRecipe};
    use tokio::io::AsyncWriteExt;
    let stage = InstallerBudget::start(InstallerRecipe::Ubuntu)
        .stage(InstallerPhase::UbuntuInstall, Duration::from_secs(60))
        .unwrap();
    let (read, mut output) = tokio::io::duplex(2048);
    let (write, _input) = tokio::io::duplex(2048);
    output.write_all(concat!(
        r#"{"QMP":{"version":{"qemu":{"major":11,"minor":1,"micro":3},"package":""},"capabilities":[]}}"#, "\n",
        r#"{"return":{},"id":1}"#, "\n"
    ).as_bytes()).await.unwrap();
    let start = Instant::now();
    let child = DelayedExit(Cell::new(false));
    let result = installer_session(&child, stage, (read, write), &Shutdown::default(), || {
        if Instant::now() > start {
            Err(NativeRefusal::ReservesUnverified)
        } else {
            Ok(std::time::Instant::now())
        }
    })
    .await;
    assert_eq!(
        result,
        Err(InstallerFailureReason::Native(
            NativeRefusal::ReservesUnverified
        ))
    );
    assert_eq!(start.elapsed(), LIVE_POLL);
    assert!(!child.0.get());
}

#[tokio::test(start_paused = true)]
async fn installer_owner_return_after_shutdown_refuses_clean_settlement() {
    use crate::vm::runtime::{InstallerBudget, InstallerPhase, InstallerRecipe};
    use tokio::io::AsyncWriteExt;
    let stage = InstallerBudget::start(InstallerRecipe::Ubuntu)
        .stage(InstallerPhase::UbuntuInstall, Duration::from_secs(60))
        .unwrap();
    let (read, mut output) = tokio::io::duplex(2048);
    let (write, _input) = tokio::io::duplex(2048);
    let replies = concat!(
        r#"{"QMP":{"version":{"qemu":{"major":11,"minor":1,"micro":3},"package":""},"capabilities":[]}}"#,
        "\n",
        r#"{"return":{},"id":1}"#,
        "\n",
        r#"{"return":{"enabled":"whpx","present":["whpx"]},"id":2}"#,
        "\n",
        r#"{"return":{"running":false,"singlestep":false,"status":"prelaunch"},"id":3}"#,
        "\n",
        r#"{"return":{},"id":4}"#,
        "\n",
        r#"{"event":"SHUTDOWN","data":{"guest":true,"reason":"guest-shutdown"}}"#,
        "\n",
    );
    output.write_all(replies.as_bytes()).await.unwrap();
    let child = DelayedExit(Cell::new(false));
    let start = Instant::now();
    let result = installer_session(&child, stage, (read, write), &Shutdown::default(), || {
        if child.0.get() {
            Err(NativeRefusal::OwnerActivityUnverified)
        } else {
            Ok(std::time::Instant::now())
        }
    })
    .await;
    assert_eq!(start.elapsed(), LIVE_POLL);
    let failure = stage
        .complete(result, StopDisposition::CleanRelease, false)
        .unwrap_err();
    assert_eq!(
        failure.reason,
        InstallerFailureReason::Native(NativeRefusal::OwnerActivityUnverified)
    );
    assert_eq!(failure.recovery, StopDisposition::DirtyRelease);
}

#[tokio::test(start_paused = true)]
async fn cancellation_during_observer_refusal_wins_after_terminal_sample() {
    let cancel = Shutdown::default();
    let samples = Cell::new(0);
    let result = monitor_running(
        async {
            sleep(LIVE_POLL * 3).await;
            Ok(7)
        },
        || {
            samples.set(samples.get() + 1);
            if samples.get() == 2 {
                cancel.cancel();
                Err(ChannelError::Remote)
            } else {
                Ok(())
            }
        },
        &cancel,
        Instant::now() + Duration::from_secs(10),
        |error| error,
    )
    .await;
    assert_eq!(result, Err(ChannelError::Cancelled));
    assert_eq!(samples.get(), 3);
}

#[tokio::test(start_paused = true)]
async fn initial_observer_refusal_still_takes_the_terminal_sample() {
    let samples = Cell::new(0);
    let result = monitor_running(
        async { Ok(7) },
        || {
            samples.set(samples.get() + 1);
            Err(NativeRefusal::OwnerActivityUnverified)
        },
        &Shutdown::default(),
        Instant::now() + Duration::from_secs(10),
        |_| NativeRefusal::QmpUnverified,
    )
    .await;
    assert_eq!(result, Err(NativeRefusal::OwnerActivityUnverified));
    assert_eq!(samples.get(), 2);
}

#[tokio::test(start_paused = true)]
async fn cancellation_during_helper_release_withholds_output_and_keeps_proof() {
    let cancel = Shutdown::default();
    let (result, held) = accept_after_release(
        Ok::<_, ChannelError>(b"helper output"),
        async {
            sleep(LIVE_POLL).await;
            cancel.cancel();
            "held proof"
        },
        &cancel,
        Instant::now() + Duration::from_secs(10),
        || async { Ok(()) },
        |error| error,
    )
    .await;
    assert_eq!(result, Err(ChannelError::Cancelled));
    assert_eq!(held, "held proof");
}

#[tokio::test(start_paused = true)]
async fn helper_release_past_the_cutoff_or_failing_host_withholds_output() {
    let deadline = Instant::now() + LIVE_POLL;
    let (expired, held) = accept_after_release(
        Ok::<_, ChannelError>(1),
        async {
            sleep(LIVE_POLL * 2).await;
            "held proof"
        },
        &Shutdown::default(),
        deadline,
        || async { Ok(()) },
        |error| error,
    )
    .await;
    assert_eq!(expired, Err(ChannelError::Timeout));
    assert_eq!(held, "held proof");
    let (host, _) = accept_after_release(
        Ok::<_, NativeRefusal>(1),
        async {},
        &Shutdown::default(),
        Instant::now() + LIVE_POLL,
        || async { Err(NativeRefusal::OwnerActivityUnverified) },
        |_| NativeRefusal::QmpUnverified,
    )
    .await;
    assert_eq!(host, Err(NativeRefusal::OwnerActivityUnverified));
    let (accepted, _) = accept_after_release(
        Ok::<_, NativeRefusal>(1),
        async {},
        &Shutdown::default(),
        Instant::now() + LIVE_POLL,
        || async { Ok(()) },
        |_| NativeRefusal::QmpUnverified,
    )
    .await;
    assert_eq!(accepted, Ok(1));
}

/// The held release a helper returns; it must come back unchanged when output
/// is refused so custody can still be confirmed.
#[derive(Debug, PartialEq, Eq)]
struct HeldProof(&'static str);

#[tokio::test(start_paused = true)]
async fn cancellation_inside_post_release_recheck_withholds_output_and_keeps_proof() {
    let cancel = Shutdown::default();
    let samples = Cell::new(0);
    let (cancel_ref, samples_ref) = (&cancel, &samples);
    let (result, held) = accept_after_release(
        Ok::<_, ChannelError>(b"helper output"),
        async { HeldProof("release") },
        &cancel,
        Instant::now() + Duration::from_secs(10),
        move || async move {
            // The sample itself is valid; cancellation lands while it runs.
            samples_ref.set(samples_ref.get() + 1);
            cancel_ref.cancel();
            Ok(())
        },
        |error| error,
    )
    .await;
    assert_eq!(result, Err(ChannelError::Cancelled));
    assert_eq!(held, HeldProof("release"));
    assert_eq!(samples.get(), 1);
}

#[tokio::test(start_paused = true)]
async fn cutoff_inside_post_release_recheck_withholds_output_and_keeps_proof() {
    let deadline = Instant::now() + LIVE_POLL;
    let samples = Cell::new(0);
    let samples_ref = &samples;
    let (result, held) = accept_after_release(
        Ok::<_, ChannelError>(b"helper output"),
        async { HeldProof("release") },
        &Shutdown::default(),
        deadline,
        move || async move {
            // The sample is valid but the clock reaches the cutoff exactly
            // while it runs.
            assert!(Instant::now() < deadline);
            tokio::time::advance(LIVE_POLL).await;
            assert_eq!(Instant::now(), deadline);
            samples_ref.set(samples_ref.get() + 1);
            Ok(())
        },
        |error| error,
    )
    .await;
    assert_eq!(result, Err(ChannelError::Timeout));
    assert_eq!(held, HeldProof("release"));
    assert_eq!(samples.get(), 1);
}

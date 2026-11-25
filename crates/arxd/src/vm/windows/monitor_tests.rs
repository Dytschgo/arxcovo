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
    )
    .await;
    assert_eq!(result, Ok(7));
    assert_eq!(samples.get(), 4);
}

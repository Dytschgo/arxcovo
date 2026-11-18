use super::*;

fn pair(wall_ms: u64, elapsed_ms: u64, bracket_ms: u64) -> Observation {
    Observation {
        wall_ms,
        elapsed_ms,
        bracket_ms,
    }
}
fn sample(clock: &HostClock, value: Observation) -> Result<u64, ClockError> {
    clock.observe_with(|| Ok(value))
}

#[test]
fn recovered_floor_equality_behind_and_invalid_latch() {
    let wall = 1_000_000;
    for floor in [0, wall, wall + SKEW_MS] {
        assert_eq!(sample(&HostClock::new(floor), pair(wall, 100, 0)), Ok(wall));
    }
    let behind = HostClock::new(wall + SKEW_MS + 1);
    assert_eq!(sample(&behind, pair(wall, 100, 0)), Err(ClockError::Behind));
    assert_eq!(
        sample(&behind, pair(wall + 1, 101, 0)),
        Err(ClockError::Behind)
    );
    assert_eq!(behind.state.lock().unwrap().baseline, None);
    for floor in [MAX_TIME_MS + 1, u64::MAX] {
        let invalid = HostClock::new(floor);
        assert_eq!(
            invalid.observe_with(|| panic!("invalid floor sampled")),
            Err(ClockError::InvalidTime)
        );
        assert_eq!(invalid.state.lock().unwrap().baseline, None);
    }
}

#[test]
fn accepted_wall_range_and_invalid_range_latch() {
    for wall in [0, MAX_TIME_MS] {
        assert_eq!(sample(&HostClock::new(wall), pair(wall, 0, 0)), Ok(wall));
    }
    for wall in [MAX_TIME_MS + 1, u64::MAX] {
        let clock = HostClock::new(0);
        assert_eq!(
            sample(&clock, pair(wall, 0, 0)),
            Err(ClockError::InvalidTime)
        );
        assert_eq!(sample(&clock, pair(0, 0, 0)), Err(ClockError::InvalidTime));
        assert_eq!(clock.state.lock().unwrap().baseline, None);
    }
}

#[test]
fn duration_conversion_checks_underflow_overflow_and_later_elapsed() {
    let ms = Duration::from_millis;
    assert_eq!(millis(ms(u64::MAX)), Ok(u64::MAX));
    assert_eq!(millis(Duration::MAX), Err(ClockError::InvalidTime));
    assert_eq!(
        observation(UNIX_EPOCH - ms(1), ms(0), ms(0)),
        Err(ClockError::InvalidTime)
    );
    assert_eq!(
        observation(UNIX_EPOCH, ms(2), ms(1)),
        Err(ClockError::Anomaly)
    );
    assert_eq!(
        observation(UNIX_EPOCH, Duration::MAX, Duration::MAX),
        Err(ClockError::InvalidTime)
    );
    assert_eq!(
        observation(UNIX_EPOCH + ms(MAX_TIME_MS + 1), ms(0), ms(0)),
        Err(ClockError::InvalidTime)
    );
    assert_eq!(
        observation(UNIX_EPOCH + ms(42), ms(3), ms(7)),
        Ok(pair(42, 7, 4))
    );
}

#[test]
fn bracket_rounds_actual_duration_up_and_enforces_ceiling() {
    let ms = Duration::from_millis;
    let ns = Duration::from_nanos;
    assert_eq!(
        observation(UNIX_EPOCH, ns(999_999), ns(1_000_001)),
        Ok(pair(0, 1, 1))
    );
    assert_eq!(observation(UNIX_EPOCH, ns(0), ns(1)), Ok(pair(0, 0, 1)));
    assert_eq!(
        observation(UNIX_EPOCH, ms(7), ms(7 + SKEW_MS)),
        Ok(pair(0, 7 + SKEW_MS, SKEW_MS))
    );
    assert_eq!(
        observation(UNIX_EPOCH, ms(7), ms(7 + SKEW_MS) + ns(1)),
        Err(ClockError::Anomaly)
    );
    let clock = HostClock::new(0);
    assert_eq!(
        sample(&clock, pair(0, 0, SKEW_MS + 1)),
        Err(ClockError::Anomaly)
    );
    assert_eq!(sample(&clock, pair(0, 0, 0)), Err(ClockError::Anomaly));
}

#[test]
fn healthy_progress_keeps_initial_baseline_over_many_samples() {
    let clock = HostClock::new(1_000_000);
    let base = pair(1_000_000, 10, 3);
    assert_eq!(sample(&clock, base), Ok(base.wall_ms));
    for step in 1..=100_000 {
        assert_eq!(
            sample(&clock, pair(base.wall_ms + step, base.elapsed_ms + step, 1)),
            Ok(base.wall_ms + step)
        );
    }
    let state = clock.state.lock().unwrap();
    assert_eq!(state.baseline, Some(base));
    assert_eq!(state.last, Some(pair(1_100_000, 100_010, 1)));
}

#[test]
fn trusted_utc_skew_can_move_between_both_endpoints() {
    let truth: u64 = 1_000_000;
    let progress = 100_000;
    for (initial_error, final_error) in [
        (-(SKEW_MS as i64), SKEW_MS as i64),
        (SKEW_MS as i64, -(SKEW_MS as i64)),
    ] {
        let base_wall = truth.checked_add_signed(initial_error).unwrap();
        let end_wall = (truth + progress).checked_add_signed(final_error).unwrap();
        let clock = HostClock::new(base_wall);
        assert_eq!(sample(&clock, pair(base_wall, 0, 0)), Ok(base_wall));
        assert_eq!(sample(&clock, pair(end_wall, progress, 0)), Ok(end_wall));
    }
}

#[test]
fn exact_envelope_is_allowed_and_one_more_millisecond_latches() {
    for (base_bracket, current_bracket) in [(0, 0), (3, 7), (SKEW_MS, SKEW_MS)] {
        for ahead in [false, true] {
            let clock = HostClock::new(0);
            let base = pair(1_000_000, 100, base_bracket);
            let envelope = 2 * SKEW_MS + base_bracket + current_bracket + 2;
            assert_eq!(sample(&clock, base), Ok(base.wall_ms));
            let (wall_delta, elapsed_delta) = if ahead { (envelope, 0) } else { (0, envelope) };
            let exact = pair(
                base.wall_ms + wall_delta,
                base.elapsed_ms + elapsed_delta,
                current_bracket,
            );
            assert_eq!(sample(&clock, exact), Ok(exact.wall_ms));
            let extra = pair(
                exact.wall_ms + u64::from(ahead),
                exact.elapsed_ms + u64::from(!ahead),
                current_bracket,
            );
            assert_eq!(sample(&clock, extra), Err(ClockError::Anomaly));
            assert_eq!(clock.state.lock().unwrap().last, Some(exact));
        }
    }
}

#[test]
fn frozen_wall_is_detected_across_many_small_samples() {
    let clock = HostClock::new(0);
    assert_eq!(sample(&clock, pair(1_000_000, 0, 0)), Ok(1_000_000));
    for step in 1..=2 * SKEW_MS + 2 {
        assert_eq!(sample(&clock, pair(1_000_000, step, 0)), Ok(1_000_000));
    }
    assert_eq!(
        sample(&clock, pair(1_000_000, 2 * SKEW_MS + 3, 0)),
        Err(ClockError::Anomaly)
    );
}

#[test]
fn forward_jump_and_elapsed_reversal_refuse() {
    for bad in [pair(1_060_003, 10, 0), pair(1_000_001, 9, 0)] {
        let clock = HostClock::new(0);
        assert_eq!(sample(&clock, pair(1_000_000, 10, 0)), Ok(1_000_000));
        assert_eq!(sample(&clock, bad), Err(ClockError::Anomaly));
        assert_eq!(
            sample(&clock, pair(1_000_001, 11, 0)),
            Err(ClockError::Anomaly)
        );
    }
}

#[test]
fn any_wall_rollback_is_latched_even_within_skew() {
    let clock = HostClock::new(0);
    assert_eq!(sample(&clock, pair(1_000_000, 0, 0)), Ok(1_000_000));
    assert_eq!(sample(&clock, pair(1_000_001, 1, 0)), Ok(1_000_001));
    assert_eq!(
        sample(&clock, pair(1_000_000, 2, 0)),
        Err(ClockError::Rollback)
    );
    assert_eq!(
        sample(&clock, pair(1_000_002, 3, 0)),
        Err(ClockError::Rollback)
    );
}

#[test]
fn every_first_raw_failure_latches_without_resampling_or_publication() {
    for error in [
        ClockError::Unavailable,
        ClockError::InvalidTime,
        ClockError::Anomaly,
        ClockError::Behind,
        ClockError::Rollback,
    ] {
        for initialized in [false, true] {
            let clock = HostClock::new(0);
            let baseline = pair(1_000_000, 0, 0);
            if initialized {
                assert_eq!(sample(&clock, baseline), Ok(baseline.wall_ms));
            }
            assert_eq!(clock.observe_with(|| Err(error)), Err(error));
            assert_eq!(
                clock.observe_with(|| panic!("latched clock resampled")),
                Err(error)
            );
            let state = clock.state.lock().unwrap();
            assert_eq!(state.baseline, initialized.then_some(baseline));
            assert_eq!(state.last, initialized.then_some(baseline));
        }
    }
}

#[test]
fn contention_precedes_raw_sampling_and_does_not_mutate_or_latch() {
    let clock = HostClock::new(0);
    for initialized in [false, true] {
        if initialized {
            assert_eq!(sample(&clock, pair(1_000_000, 0, 0)), Ok(1_000_000));
        }
        // Same-thread ownership deterministically forces contention; no helper,
        // timing assumption, or cleanup channel can be stranded on assertion.
        let held = clock.state.lock().unwrap();
        let baseline = held.baseline;
        let last = held.last;
        assert_eq!(
            clock.observe_with(|| panic!("contended clock sampled")),
            Err(ClockError::Unavailable)
        );
        assert_eq!(held.baseline, baseline);
        assert_eq!(held.last, last);
        assert_eq!(held.failure, None);
        drop(held);
        assert_eq!(sample(&clock, pair(1_000_000, 0, 0)), Ok(1_000_000));
    }
}

#[test]
fn mutex_poison_stays_unavailable_without_raw_sampling() {
    let clock = HostClock::new(0);
    assert_eq!(sample(&clock, pair(1_000_000, 0, 0)), Ok(1_000_000));
    let poison = std::panic::catch_unwind(|| {
        let _guard = clock.state.lock().unwrap();
        panic!("synthetic clock poison");
    });
    assert!(poison.is_err());
    for _ in 0..2 {
        assert_eq!(
            clock.observe_with(|| panic!("poisoned clock sampled")),
            Err(ClockError::Unavailable)
        );
    }
}

#[test]
fn error_labels_are_static_and_do_not_contain_observations() {
    for (error, label) in [
        (ClockError::Unavailable, "clock unavailable"),
        (ClockError::InvalidTime, "invalid clock time"),
        (ClockError::Anomaly, "clock anomaly"),
        (ClockError::Behind, "clock behind"),
        (ClockError::Rollback, "clock rollback"),
    ] {
        assert_eq!(error.to_string(), label);
    }
}

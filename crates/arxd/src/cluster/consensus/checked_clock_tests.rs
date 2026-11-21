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

/// Per-instance test source controls. Each case uses its own clock instances and
/// never changes the host wall clock or environment.
#[cfg(target_os = "linux")]
mod test_source {
    use super::*;
    use crate::cluster::enrollment::CODE_TTL_MS;

    // Real samples taken microseconds apart; a generous bound that is still far
    // below every offset asserted against it.
    const JITTER_MS: u64 = 5_000;

    #[test]
    fn coherent_advance_is_isolated_and_keeps_the_original_baseline() {
        let advanced = HostClock::new(0);
        let untouched = HostClock::new(0);
        let a0 = advanced.sample_ms().unwrap();
        let u0 = untouched.sample_ms().unwrap();
        let baseline = advanced.state.lock().unwrap().baseline;
        assert_eq!(
            advanced.advance_for_test(CODE_TTL_MS),
            Ok(TestShift {
                wall_ms: CODE_TTL_MS,
                elapsed_ms: CODE_TTL_MS
            })
        );
        let a1 = advanced.sample_ms().unwrap();
        let u1 = untouched.sample_ms().unwrap();
        assert!((a0 + CODE_TTL_MS..a0 + CODE_TTL_MS + JITTER_MS).contains(&a1));
        assert!((u0..u0 + JITTER_MS).contains(&u1));
        let state = advanced.state.lock().unwrap();
        assert_eq!(state.baseline, baseline);
        assert_eq!(state.failure, None);
        let last = state.last.unwrap();
        let base = baseline.unwrap();
        assert!(last.elapsed_ms >= base.elapsed_ms + CODE_TTL_MS);
        assert!(last.bracket_ms <= SKEW_MS);
        assert_eq!(untouched.state.lock().unwrap().failure, None);
    }

    #[test]
    fn wall_skew_within_envelope_passes_and_resume_visible_skew_latches() {
        let within = HostClock::new(0);
        let w0 = within.sample_ms().unwrap();
        within.shift_for_test(SKEW_MS, 0).unwrap();
        let w1 = within.sample_ms().unwrap();
        assert!((w0 + SKEW_MS..w0 + SKEW_MS + JITTER_MS).contains(&w1));

        // Wall advanced past 2*SKEW plus two maximal brackets while elapsed did not.
        let resumed = HostClock::new(0);
        resumed.sample_ms().unwrap();
        resumed.shift_for_test(4 * SKEW_MS + 10, 0).unwrap();
        assert_eq!(resumed.sample_ms(), Err(ClockError::Anomaly));
        assert_eq!(
            resumed.observe_with(|| panic!("latched clock resampled")),
            Err(ClockError::Anomaly)
        );

        // Elapsed advanced while the wall did not: a frozen wall is also refused.
        let frozen = HostClock::new(0);
        frozen.sample_ms().unwrap();
        frozen.shift_for_test(0, 4 * SKEW_MS + 10).unwrap();
        assert_eq!(frozen.sample_ms(), Err(ClockError::Anomaly));
    }

    #[test]
    fn first_sample_floor_and_range_checks_see_the_shifted_source() {
        let now = HostClock::new(0).sample_ms().unwrap();
        let floor = now + 10 * SKEW_MS;
        let behind = HostClock::new(floor);
        assert_eq!(behind.sample_ms(), Err(ClockError::Behind));

        let caught_up = HostClock::new(floor);
        caught_up.advance_for_test(10 * SKEW_MS).unwrap();
        assert!(caught_up.sample_ms().unwrap() >= floor);

        let out_of_range = HostClock::new(0);
        out_of_range.sample_ms().unwrap();
        out_of_range.advance_for_test(MAX_TIME_MS).unwrap();
        assert_eq!(out_of_range.sample_ms(), Err(ClockError::InvalidTime));
        assert_eq!(out_of_range.sample_ms(), Err(ClockError::InvalidTime));
    }

    #[test]
    fn a_latched_first_refusal_is_not_cleared_by_advancing() {
        for error in [
            ClockError::Rollback,
            ClockError::Anomaly,
            ClockError::Behind,
        ] {
            let clock = HostClock::new(0);
            clock.sample_ms().unwrap();
            assert_eq!(clock.observe_with(|| Err(error)), Err(error));
            clock.advance_for_test(SKEW_MS).unwrap();
            assert_eq!(clock.sample_ms(), Err(error));
        }
    }

    #[test]
    fn overflowing_or_poisoned_test_source_refuses_without_wrapping() {
        let clock = HostClock::new(0);
        clock.shift_for_test(u64::MAX, 0).unwrap();
        assert_eq!(
            clock.shift_for_test(1, 0),
            Err(ClockError::InvalidTime),
            "accumulated offset wrapped"
        );
        assert_eq!(
            *clock.test_source.lock().unwrap(),
            TestShift {
                wall_ms: u64::MAX,
                elapsed_ms: 0
            }
        );
        assert_eq!(clock.sample_ms(), Err(ClockError::InvalidTime));

        // After a baseline, an elapsed offset of u64::MAX ms either overflows the
        // millisecond conversion or leaves the envelope; both latch. (A first
        // sample has no baseline, so elapsed alone is not judged there.)
        let elapsed = HostClock::new(0);
        elapsed.sample_ms().unwrap();
        elapsed.shift_for_test(0, u64::MAX).unwrap();
        let refused = elapsed.sample_ms();
        assert!(
            matches!(refused, Err(ClockError::InvalidTime | ClockError::Anomaly)),
            "{refused:?}"
        );
        assert_eq!(elapsed.sample_ms(), refused);

        let poisoned = HostClock::new(0);
        poisoned.sample_ms().unwrap();
        let poison = std::panic::catch_unwind(|| {
            let _guard = poisoned.test_source.lock().unwrap();
            panic!("synthetic test source poison");
        });
        assert!(poison.is_err());
        assert_eq!(poisoned.sample_ms(), Err(ClockError::Unavailable));
        assert_eq!(poisoned.shift_for_test(1, 1), Err(ClockError::Unavailable));
        assert_eq!(
            poisoned.state.lock().unwrap().failure,
            Some(ClockError::Unavailable)
        );
    }

    #[test]
    fn state_contention_refuses_before_reading_the_test_source() {
        let clock = HostClock::new(0);
        clock.sample_ms().unwrap();
        let held = clock.state.lock().unwrap();
        clock.advance_for_test(SKEW_MS).unwrap();
        assert_eq!(clock.sample_ms(), Err(ClockError::Unavailable));
        assert_eq!(held.failure, None);
        drop(held);
        assert!(clock.sample_ms().is_ok());
    }
}

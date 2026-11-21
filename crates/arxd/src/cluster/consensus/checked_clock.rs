//! Conditional on trusted host UTC within SKEW_MS at every sensitive sample.
//! Local anomaly detection does not qualify UTC, restart, or suspend behavior.
use crate::cluster::enrollment::{MAX_TIME_MS, SKEW_MS};
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::{
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(in crate::cluster) trait CheckedClock: Send + Sync {
    fn sample_ms(&self) -> Result<u64, ClockError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cluster) enum ClockError {
    Unavailable,
    InvalidTime,
    Anomaly,
    Behind,
    Rollback,
}
impl std::fmt::Display for ClockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "clock unavailable",
            Self::InvalidTime => "invalid clock time",
            Self::Anomaly => "clock anomaly",
            Self::Behind => "clock behind",
            Self::Rollback => "clock rollback",
        })
    }
}
impl std::error::Error for ClockError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Observation {
    wall_ms: u64,
    elapsed_ms: u64,
    bracket_ms: u64,
}

fn millis(duration: Duration) -> Result<u64, ClockError> {
    u64::try_from(duration.as_millis()).map_err(|_| ClockError::InvalidTime)
}

fn observation(
    wall: SystemTime,
    before: Duration,
    after: Duration,
) -> Result<Observation, ClockError> {
    let bracket = after.checked_sub(before).ok_or(ClockError::Anomaly)?;
    // Round the actual bracket duration up, not a difference of rounded reads.
    let bracket_ms = millis(bracket)?
        .checked_add(u64::from(!bracket.subsec_nanos().is_multiple_of(1_000_000)))
        .ok_or(ClockError::InvalidTime)?;
    let value = Observation {
        wall_ms: millis(
            wall.duration_since(UNIX_EPOCH)
                .map_err(|_| ClockError::InvalidTime)?,
        )?,
        elapsed_ms: millis(after)?,
        bracket_ms,
    };
    validate(value)?;
    Ok(value)
}

fn validate(value: Observation) -> Result<(), ClockError> {
    if value.wall_ms > MAX_TIME_MS {
        return Err(ClockError::InvalidTime);
    }
    if value.bracket_ms > SKEW_MS {
        return Err(ClockError::Anomaly);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    baseline: Option<Observation>,
    last: Option<Observation>,
    failure: Option<ClockError>,
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct Checkpoint {
    generation: AtomicU64,
    published: Mutex<(u64, State)>,
}

/// No store ownership or startup wiring: construction requires the recovered
/// trusted local high-water timestamp. An invalid floor leaves a refused clock.
pub(super) struct HostClock {
    anchor: Instant,
    recovered_floor: u64,
    state: Mutex<State>,
    #[cfg(target_os = "linux")]
    checkpoint: Box<Checkpoint>,
    // Boxed so test builds keep the inline layout within existing size lints.
    #[cfg(all(test, target_os = "linux"))]
    test_source: Box<Mutex<TestShift>>,
}
impl HostClock {
    pub(super) fn new(recovered_floor: u64) -> Self {
        let state = State {
            failure: (recovered_floor > MAX_TIME_MS).then_some(ClockError::InvalidTime),
            ..State::default()
        };
        Self {
            anchor: Instant::now(),
            recovered_floor,
            state: Mutex::new(state),
            #[cfg(target_os = "linux")]
            checkpoint: Box::new(Checkpoint {
                published: Mutex::new((0, state)),
                ..Checkpoint::default()
            }),
            #[cfg(all(test, target_os = "linux"))]
            test_source: Box::default(),
        }
    }

    fn observe_with(
        &self,
        sample: impl FnOnce() -> Result<Observation, ClockError>,
    ) -> Result<u64, ClockError> {
        // Serialize before touching either raw clock. Contention is transient;
        // poison is permanently unavailable. Neither waits or collects a pair.
        let mut state = self.state.try_lock().map_err(|_| ClockError::Unavailable)?;
        if let Some(error) = state.failure {
            return Err(error);
        }
        #[cfg(target_os = "linux")]
        let generation = self
            .checkpoint
            .generation
            .fetch_add(1, SeqCst)
            .wrapping_add(1);
        let result = sample().and_then(|current| {
            self.validate_sample(&state, current)?;
            if state.baseline.is_none() {
                state.baseline = Some(current);
            }
            state.last = Some(current);
            Ok(current.wall_ms)
        });
        if let Err(error) = result {
            state.failure = Some(error);
        }
        // A busy publication slot only costs evidence. The old generation
        // will be rejected, including if sampling panics before publication.
        #[cfg(target_os = "linux")]
        if let Ok(mut published) = self.checkpoint.published.try_lock() {
            *published = (generation, *state);
        }
        result
    }

    fn validate_sample(&self, state: &State, current: Observation) -> Result<(), ClockError> {
        validate(current)?;
        if let Some(last) = state.last {
            if current.wall_ms < last.wall_ms {
                return Err(ClockError::Rollback);
            }
            if current.elapsed_ms < last.elapsed_ms {
                return Err(ClockError::Anomaly);
            }
        }
        if let Some(base) = state.baseline {
            let wall_delta = i128::from(current.wall_ms) - i128::from(base.wall_ms);
            let elapsed_delta = i128::from(current.elapsed_ms) - i128::from(base.elapsed_ms);
            let envelope = 2 * i128::from(SKEW_MS)
                + i128::from(base.bracket_ms)
                + i128::from(current.bracket_ms)
                + 2;
            if (wall_delta - elapsed_delta).abs() > envelope {
                return Err(ClockError::Anomaly);
            }
        } else if current
            .wall_ms
            .checked_add(SKEW_MS)
            .ok_or(ClockError::InvalidTime)?
            < self.recovered_floor
        {
            return Err(ClockError::Behind);
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub(super) fn sample_checkpoint(&self) -> Result<u64, ClockError> {
        self.checkpoint_with(|| {
            let (wall, before, after) = self.raw_pair()?;
            observation(wall, before, after)
        })
    }

    #[cfg(target_os = "linux")]
    fn checkpoint_with(
        &self,
        sample: impl FnOnce() -> Result<Observation, ClockError>,
    ) -> Result<u64, ClockError> {
        let (generation, state) = *self
            .checkpoint
            .published
            .try_lock()
            .map_err(|_| ClockError::Unavailable)?;
        if self.checkpoint.generation.load(SeqCst) != generation {
            return Err(ClockError::Unavailable);
        }
        if let Some(error) = state.failure {
            return Err(error);
        }
        let current = sample()?;
        self.validate_sample(&state, current)?;
        if self.checkpoint.generation.load(SeqCst) != generation {
            return Err(ClockError::Unavailable);
        }
        Ok(current.wall_ms)
    }

    fn raw_pair(&self) -> Result<(SystemTime, Duration, Duration), ClockError> {
        let before = Instant::now()
            .checked_duration_since(self.anchor)
            .ok_or(ClockError::Anomaly)?;
        let wall = SystemTime::now();
        let after = Instant::now()
            .checked_duration_since(self.anchor)
            .ok_or(ClockError::Anomaly)?;
        Ok((wall, before, after))
    }
}
impl CheckedClock for HostClock {
    fn sample_ms(&self) -> Result<u64, ClockError> {
        self.observe_with(|| {
            // One read per sample keeps the test pair coherent across the bracket.
            #[cfg(all(test, target_os = "linux"))]
            let shift = *self
                .test_source
                .lock()
                .map_err(|_| ClockError::Unavailable)?;
            let (wall, before, after) = self.raw_pair()?;
            #[cfg(all(test, target_os = "linux"))]
            let (wall, before, after) = shift.apply(wall, before, after)?;
            observation(wall, before, after)
        })
    }
}

/// Linux-test-only raw source offsets for one clock instance. They are applied
/// to the host readings before the unchanged checked arithmetic, so the floor,
/// range, bracket, rollback and wall/elapsed envelope still decide every sample.
/// Production builds have no field, switch or environment input for this.
#[cfg(all(test, target_os = "linux"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::cluster) struct TestShift {
    pub(in crate::cluster) wall_ms: u64,
    pub(in crate::cluster) elapsed_ms: u64,
}

#[cfg(all(test, target_os = "linux"))]
impl TestShift {
    fn apply(
        self,
        wall: SystemTime,
        before: Duration,
        after: Duration,
    ) -> Result<(SystemTime, Duration, Duration), ClockError> {
        let elapsed = Duration::from_millis(self.elapsed_ms);
        Ok((
            wall.checked_add(Duration::from_millis(self.wall_ms))
                .ok_or(ClockError::InvalidTime)?,
            before.checked_add(elapsed).ok_or(ClockError::InvalidTime)?,
            after.checked_add(elapsed).ok_or(ClockError::InvalidTime)?,
        ))
    }
}

#[cfg(all(test, target_os = "linux"))]
impl HostClock {
    /// Advance this instance's bracketed wall and elapsed sources together.
    pub(in crate::cluster) fn advance_for_test(&self, ms: u64) -> Result<TestShift, ClockError> {
        self.shift_for_test(ms, ms)
    }

    /// Forward-only per-instance offsets. Other instances and the host stay unchanged.
    pub(in crate::cluster) fn shift_for_test(
        &self,
        wall_ms: u64,
        elapsed_ms: u64,
    ) -> Result<TestShift, ClockError> {
        let mut shift = self
            .test_source
            .lock()
            .map_err(|_| ClockError::Unavailable)?;
        let next = TestShift {
            wall_ms: shift
                .wall_ms
                .checked_add(wall_ms)
                .ok_or(ClockError::InvalidTime)?,
            elapsed_ms: shift
                .elapsed_ms
                .checked_add(elapsed_ms)
                .ok_or(ClockError::InvalidTime)?,
        };
        *shift = next;
        Ok(next)
    }
}

#[cfg(test)]
#[path = "checked_clock_tests.rs"]
mod tests;

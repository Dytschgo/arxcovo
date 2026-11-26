//! Selection-epoch freshness for native TEST effects (ADR 0054).
//!
//! Every refusal is classified where it is observed. Contention or a
//! temporary live refusal refuses only the current effect, and the same
//! epoch can admit the next fresh call. Positive evidence of rollback,
//! reboot, expiry or material drift revokes the epoch's effect authority.
//! Poison, unknown failures and missing observations fail closed without
//! claiming drift. No outcome gates owned stop, release or recovery: the
//! epoch id stays available for settlement after revocation.
//!
//! The native TEST issuer that consumes this component lands in a later 7c
//! PR. Production TEST admission stays closed here.
#![allow(
    dead_code,
    reason = "The native TEST issuer consumes the epoch in a later 7c PR"
)]

use super::{
    Id,
    model::ClockReading,
    qualification_record::{Hash, Instant2, clocks_agree},
    runtime::{FRESHNESS, NativeRefusal},
};
use crate::store::vm as store;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const REASON: NativeRefusal = NativeRefusal::QualificationMissing;

/// Why a refresh refused the current effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Invalidation {
    /// Store contention, SQLite BUSY or LOCKED, a late sample or a temporary
    /// WTS or reserve refusal. The epoch survives.
    Transient(NativeRefusal),
    /// Poison, an unknown failure or a missing observation. The effect is
    /// refused, and no drift is claimed.
    Indeterminate(NativeRefusal),
    /// Positive evidence of rollback, reboot, expiry or material scope or
    /// dependency drift. The epoch loses effect authority.
    Confirmed(NativeRefusal),
}

impl Invalidation {
    pub(crate) fn refusal(self) -> NativeRefusal {
        match self {
            Self::Transient(reason) | Self::Indeterminate(reason) | Self::Confirmed(reason) => {
                reason
            }
        }
    }

    /// Classify a store or high-water error by its typed origin.
    pub(crate) fn from_store(error: &anyhow::Error, reason: NativeRefusal) -> Self {
        if error.is::<store::StoreIdentityMismatch>() {
            Self::Confirmed(reason)
        } else if store::contended(error) {
            Self::Transient(reason)
        } else {
            Self::Indeterminate(reason)
        }
    }

    /// A live owner-activity (WTS) or reserve refusal can clear on its own.
    pub(crate) fn from_live_sample(reason: NativeRefusal) -> Self {
        match reason {
            NativeRefusal::OwnerActivityUnverified | NativeRefusal::ReservesUnverified => {
                Self::Transient(reason)
            }
            reason => Self::Indeterminate(reason),
        }
    }
}

impl From<Invalidation> for NativeRefusal {
    fn from(invalidation: Invalidation) -> Self {
        invalidation.refusal()
    }
}

/// One bounded observation: the boot-scoped freshness read and the
/// committed high-water reading, both after `captured`.
#[derive(Clone, Copy)]
pub(crate) struct Sample {
    pub captured: Instant,
    pub now: Instant2,
    pub boot_marker: Hash,
    pub reading: ClockReading,
}

pub(crate) struct SelectionEpoch {
    id: Id,
    boot_marker: Hash,
    start: Instant2,
    monotonic: Instant,
    revoked: AtomicBool,
}

impl SelectionEpoch {
    pub(crate) fn open(id: Id, sample: Sample, finished: Instant) -> Result<Self, Invalidation> {
        let epoch = Self {
            id,
            boot_marker: sample.boot_marker,
            start: sample.now,
            monotonic: sample.reading.monotonic,
            revoked: AtomicBool::new(false),
        };
        epoch.check(&sample, finished)?;
        Ok(epoch)
    }

    /// Settlement keeps this id whether or not effects are still authorized.
    pub(crate) fn id(&self) -> Id {
        self.id
    }

    pub(crate) fn effects_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }

    /// Returns the capture instant that bounds the authorized effect.
    pub(crate) fn refresh(
        &self,
        sample: impl FnOnce() -> Result<Sample, Invalidation>,
        finished: impl FnOnce() -> Instant,
    ) -> Result<Instant, Invalidation> {
        if self.effects_revoked() {
            return Err(Invalidation::Confirmed(REASON));
        }
        let result = sample().and_then(|sample| {
            self.check(&sample, finished())?;
            Ok(sample.captured)
        });
        if matches!(result, Err(Invalidation::Confirmed(_))) {
            self.revoked.store(true, Ordering::Release);
        }
        result
    }

    fn check(&self, sample: &Sample, finished: Instant) -> Result<(), Invalidation> {
        if sample.boot_marker != self.boot_marker || !clocks_agree(self.start, sample.now) {
            return Err(Invalidation::Confirmed(REASON));
        }
        // The process and boot clocks, and both UTC reads, must describe one
        // instant. A disagreement means the reads were taken apart.
        let process = sample
            .reading
            .monotonic
            .checked_duration_since(self.monotonic);
        let boot = (sample.now.mono_ms.checked_sub(self.start.mono_ms)).map(Duration::from_millis);
        let together = process
            .zip(boot)
            .is_some_and(|(process, boot)| process.abs_diff(boot) <= FRESHNESS)
            && u64::try_from(sample.reading.utc_ms / 1_000)
                .is_ok_and(|utc| utc.abs_diff(sample.now.utc) <= 1);
        match finished.checked_duration_since(sample.captured) {
            None => Err(Invalidation::Indeterminate(REASON)),
            Some(age) if !together || age > FRESHNESS => Err(Invalidation::Transient(REASON)),
            Some(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        service::application::tests::private_temp,
        vm::{
            Clock, VmController, VmPolicy,
            runtime::{CandidateRuntime, NativeCapability, NativeMutation},
        },
    };
    use Invalidation::{Confirmed, Indeterminate, Transient};
    use std::sync::{Arc, Mutex};

    const NOW: i64 = 1_800_000_000_000;
    const BOOT_MS: u64 = 3_600_000;
    const MARKER: Hash = [7; 32];
    const EPOCH: Id = [9; 16];

    #[derive(Clone)]
    struct TestClock(Arc<Mutex<ClockReading>>);

    impl Clock for TestClock {
        fn now(&self) -> ClockReading {
            *self.0.lock().unwrap()
        }
    }

    struct Fixture {
        ctl: VmController<TestClock>,
        clock: TestClock,
        origin: Instant,
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = private_temp();
            let origin = Instant::now();
            let clock = TestClock(Arc::new(Mutex::new(ClockReading {
                utc_ms: NOW,
                monotonic: origin,
            })));
            let ctl = VmController::open(root.path(), VmPolicy::default(), clock.clone()).unwrap();
            Self {
                ctl,
                clock,
                origin,
                root,
            }
        }

        /// Both host clocks move together by whole seconds.
        fn advance(&self, seconds: u64) {
            let mut reading = self.clock.0.lock().unwrap();
            reading.utc_ms += i64::try_from(seconds * 1_000).unwrap();
            reading.monotonic += Duration::from_secs(seconds);
        }

        fn set_utc(&self, utc_ms: i64) {
            self.clock.0.lock().unwrap().utc_ms = utc_ms;
        }

        fn boot_now(&self, reading: ClockReading) -> Instant2 {
            let elapsed = reading.monotonic.duration_since(self.origin).as_millis();
            Instant2 {
                utc: u64::try_from(reading.utc_ms / 1_000).unwrap(),
                mono_ms: BOOT_MS + u64::try_from(elapsed).unwrap(),
            }
        }

        /// The fake freshness read plus the real high-water transaction.
        fn sample(&self) -> Result<Sample, Invalidation> {
            let captured = Instant::now();
            let reading = self.ctl.advance_native_clock()?;
            Ok(Sample {
                captured,
                now: self.boot_now(reading),
                boot_marker: MARKER,
                reading,
            })
        }

        fn epoch(&self) -> SelectionEpoch {
            SelectionEpoch::open(EPOCH, self.sample().unwrap(), Instant::now()).unwrap()
        }

        fn refresh(&self, epoch: &SelectionEpoch) -> Result<Instant, Invalidation> {
            epoch.refresh(|| self.sample(), Instant::now)
        }
    }

    /// A passing refresh issues no launch authority.
    fn assert_admission_closed() {
        let profile = crate::config::VmRuntimeProfile::parse(
            &serde_json::to_vec(&serde_json::json!({
                "enabled": true, "owner_sid": "S-1-5-21-1-2-3-1001",
                "supervisor_sid": "S-1-5-21-1-2-3-1002", "bundle": "E:/pinned bundle",
                "volume": "D:/", "state": "D:/test_vms/state",
                "disk_ceiling_bytes": 10_737_418_240_u64, "manifest": "qemu-11.1.3-windows-x64",
            }))
            .unwrap(),
        )
        .unwrap();
        let runtime = CandidateRuntime::new(Some(&profile));
        assert!(matches!(
            runtime.capability(),
            NativeCapability::Unavailable(_)
        ));
        assert!(runtime.dispatch(NativeMutation::Boot, |_| ()).is_err());
    }

    #[test]
    fn held_store_lock_refuses_then_the_same_epoch_passes() {
        let fixture = Fixture::new();
        let epoch = fixture.epoch();
        fixture.advance(1);
        let held = fixture
            .ctl
            .store_for_test()
            .read_backup_owner(|_| Ok(fixture.refresh(&epoch)))
            .unwrap();
        assert_eq!(held, Err(Transient(REASON)));
        assert!(!epoch.effects_revoked());
        fixture.advance(1);
        assert!(fixture.refresh(&epoch).is_ok());
        assert_eq!(epoch.id(), EPOCH);
        assert_admission_closed();
    }

    #[test]
    fn sqlite_busy_refuses_then_the_same_epoch_passes() {
        let fixture = Fixture::new();
        let epoch = fixture.epoch();
        let other =
            rusqlite::Connection::open(fixture.root.path().join("activity.sqlite3")).unwrap();
        other.execute_batch("BEGIN IMMEDIATE;").unwrap();
        fixture.advance(1);
        assert_eq!(fixture.refresh(&epoch), Err(Transient(REASON)));
        assert!(!epoch.effects_revoked());
        other.execute_batch("ROLLBACK;").unwrap();
        drop(other);
        assert!(fixture.refresh(&epoch).is_ok());
        assert_admission_closed();
    }

    #[test]
    fn high_water_rollback_revokes_effects_and_keeps_the_settlement_epoch() {
        let fixture = Fixture::new();
        let epoch = fixture.epoch();
        fixture.advance(2);
        assert!(fixture.refresh(&epoch).is_ok());
        // A reading equal to the committed high water is not a rollback.
        assert!(fixture.ctl.advance_native_clock().is_ok());
        assert!(fixture.refresh(&epoch).is_ok());
        let high_water = NOW + 2_000;
        fixture.set_utc(high_water - 1);
        assert_eq!(
            fixture.ctl.advance_native_clock().err(),
            Some(Confirmed(REASON))
        );
        assert_eq!(fixture.refresh(&epoch), Err(Confirmed(REASON)));
        assert!(epoch.effects_revoked());
        // Recovered clocks cannot restore effect authority to this epoch.
        fixture.set_utc(high_water);
        fixture.advance(1);
        assert!(fixture.ctl.advance_native_clock().is_ok());
        assert_eq!(fixture.refresh(&epoch), Err(Confirmed(REASON)));
        assert_eq!(epoch.id(), EPOCH);
        assert_admission_closed();
    }

    #[test]
    fn poisoned_store_fails_closed_without_revoking() {
        let fixture = Fixture::new();
        let epoch = fixture.epoch();
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fixture
                .ctl
                .store_for_test()
                .read_backup_owner(|_| -> anyhow::Result<()> { panic!("poison the connection") })
        }));
        assert!(poisoned.is_err());
        assert_eq!(fixture.refresh(&epoch), Err(Indeterminate(REASON)));
        assert!(!epoch.effects_revoked());
    }

    #[test]
    fn boot_marker_and_clock_disagreement_are_confirmed() {
        let fixture = Fixture::new();
        let rebooted = fixture.epoch();
        fixture.advance(1);
        let mut sample = fixture.sample().unwrap();
        sample.boot_marker = [8; 32];
        assert_eq!(
            rebooted.refresh(|| Ok(sample), Instant::now),
            Err(Confirmed(REASON))
        );
        assert!(rebooted.effects_revoked());

        let jumped = fixture.epoch();
        let mut sample = fixture.sample().unwrap();
        sample.now.utc += 4;
        assert_eq!(
            jumped.refresh(|| Ok(sample), Instant::now),
            Err(Confirmed(REASON))
        );
        assert!(jumped.effects_revoked());
    }

    #[test]
    fn late_split_or_missing_samples_refuse_without_revoking() {
        let fixture = Fixture::new();
        let epoch = fixture.epoch();
        fixture.advance(1);
        let sample = fixture.sample().unwrap();
        let at = |age| move || sample.captured + age;
        assert_eq!(
            epoch.refresh(|| Ok(sample), at(FRESHNESS)),
            Ok(sample.captured)
        );
        assert_eq!(
            epoch.refresh(|| Ok(sample), at(FRESHNESS + Duration::from_nanos(1))),
            Err(Transient(REASON))
        );
        // The freshness and high-water reads disagree by more than a second.
        let mut split = sample;
        split.reading.utc_ms += 2_000;
        assert_eq!(
            epoch.refresh(|| Ok(split), Instant::now),
            Err(Transient(REASON))
        );
        let mut early = sample;
        early.captured = Instant::now() + Duration::from_secs(60);
        assert_eq!(
            epoch.refresh(|| Ok(early), Instant::now),
            Err(Indeterminate(REASON))
        );
        assert_eq!(
            epoch.refresh(|| Err(Indeterminate(REASON)), Instant::now),
            Err(Indeterminate(REASON))
        );
        assert!(!epoch.effects_revoked());
        fixture.advance(1);
        assert!(fixture.refresh(&epoch).is_ok());
    }

    #[test]
    fn classification_follows_the_typed_origin() {
        let reason = NativeRefusal::PathCustodyUnverified;
        let mismatch = anyhow::Error::new(store::StoreIdentityMismatch);
        assert_eq!(
            Invalidation::from_store(&mismatch, reason),
            Confirmed(reason)
        );
        let contended = anyhow::Error::new(store::LiveAccess::Contended);
        assert_eq!(
            Invalidation::from_store(&contended, reason),
            Transient(reason)
        );
        let poisoned = anyhow::Error::new(store::LiveAccess::Poisoned);
        assert_eq!(
            Invalidation::from_store(&poisoned, reason),
            Indeterminate(reason)
        );
        // Matching text is not a typed origin.
        let unknown = anyhow::anyhow!("VM store identity mismatch");
        assert_eq!(
            Invalidation::from_store(&unknown, reason),
            Indeterminate(reason)
        );
        for live in [
            NativeRefusal::OwnerActivityUnverified,
            NativeRefusal::ReservesUnverified,
        ] {
            assert_eq!(Invalidation::from_live_sample(live), Transient(live));
        }
        assert_eq!(
            Invalidation::from_live_sample(NativeRefusal::JobUnverified),
            Indeterminate(NativeRefusal::JobUnverified)
        );
        assert_eq!(NativeRefusal::from(Confirmed(reason)), reason);
    }
}

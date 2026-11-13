//! Private, monotone admission state. No payload-bearing diagnostics.
use super::Unavailable;
use crate::exec::hierarchy::setup::SetupFailure;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Cause {
    Setup = 2,
    Hardening,
    Janitor,
    Containment,
    Rights,
    Stopped,
}
#[derive(Clone)]
pub(super) struct Readiness(Arc<AtomicU8>);
impl Readiness {
    pub(super) fn pending() -> Self {
        Self(Arc::new(AtomicU8::new(0)))
    }
    pub(super) fn set_ready(&self) {
        let _ = self
            .0
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(super) fn fail(&self, cause: Cause) {
        let _ = self
            .0
            .compare_exchange(0, cause as u8, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(super) fn latch(&self, cause: Cause) {
        if !matches!(cause, Cause::Containment | Cause::Rights) {
            return;
        }
        let _ = self
            .0
            .compare_exchange(1, cause as u8, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(super) fn check(&self) -> Result<(), Unavailable> {
        match self.0.load(Ordering::Acquire) {
            1 => Ok(()),
            4 => Err(Unavailable::Quarantined),
            6 => Err(Unavailable::RightsMismatch),
            7 => Err(Unavailable::Stopping),
            _ => Err(Unavailable::MechanismUnavailable),
        }
    }
}
impl From<SetupFailure> for Cause {
    fn from(f: SetupFailure) -> Self {
        use SetupFailure::*;
        match f {
            Hardening => Self::Hardening,
            KillTimeout | LeafRemoval | VerifyFailed | Panicked => Self::Janitor,
            Stopped => Self::Stopped,
            Identity | Locate | Root | Unit | Delegation | UnitState | Supervisor | EnableUnit
            | Runs | KillInterface | LeafStructure | EnableRuns | Postcondition => Self::Setup,
        }
    }
}

//! Fixed build health. The phase selects the probe, and a single-use
//! capability binds the exact probe bytes and timeout to the admitted recipe
//! and the launched workspace and pair. One runner owns readiness, the guest
//! exchange, terminal observation and teardown for both the checked launch
//! and its fake guest peer. Native launches are **unverified on real
//! hardware**.

use super::*;
use crate::vm::qualification_record::{Hash, digest};

/// Which installed health check a boot runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HealthPhase {
    /// Installed Windows after Setup and before Finalize. The Setup phase
    /// marker and the Finalize task must still be present.
    Readiness,
    /// The detached boot of the sealed candidate. On Windows, Finalize has
    /// removed the phase marker and the task.
    Verification,
}

impl HealthPhase {
    /// The same mapping as the template's installed targets: readiness
    /// validates the Setup phase, and verification requires its removal.
    pub(super) fn validation(self) -> bool {
        matches!(self, Self::Readiness)
    }
}

/// The probe for one recipe and phase. Ubuntu has no readiness phase.
pub(super) fn select_probe(
    recipe: RecipeIdentity,
    windows: RecipeIdentity,
    phase: HealthPhase,
    windows_probe: impl FnOnce(bool) -> Result<String, Q>,
) -> Result<String, Q> {
    if recipe == windows {
        windows_probe(phase.validation())
    } else if phase == HealthPhase::Verification {
        Ok(crate::vm::template::UBUNTU_DETACHED_HEALTH.to_owned())
    } else {
        Err(PHASE)
    }
}

/// The admitted recipe and phase, and the launched workspace and pair.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct HealthScope {
    pub recipe: RecipeIdentity,
    pub phase: HealthPhase,
    pub binding: WorkspaceBinding,
    pub pair: PairSlots,
}

/// One minted health exchange. It is not `Clone`, and dispatch consumes it.
#[must_use = "a minted health probe is dispatched at most once"]
pub(super) struct HealthProbe {
    scope: HealthScope,
    script: Hash,
    timeout_s: u32,
}

impl HealthProbe {
    pub(super) fn mint(scope: HealthScope, script: &str) -> Self {
        Self {
            scope,
            script: digest(script.as_bytes()),
            timeout_s: SCRIPT_TIMEOUT,
        }
    }

    /// At dispatch: the same scope and exactly the minted bytes and timeout.
    pub(super) fn admit(
        self,
        scope: HealthScope,
        script: String,
        timeout_s: u32,
    ) -> Result<AdmittedProbe, Q> {
        if self.scope != scope
            || self.timeout_s != timeout_s
            || digest(script.as_bytes()) != self.script
        {
            return Err(HEALTH);
        }
        Ok(AdmittedProbe { script, timeout_s })
    }
}

/// The only request a health session can transmit.
pub(super) struct AdmittedProbe {
    script: String,
    timeout_s: u32,
}

impl AdmittedProbe {
    pub(super) fn into_parts(self) -> (String, u32) {
        (self.script, self.timeout_s)
    }
}

/// A launched, held boot. A failed `boot` or `exchange` has already settled
/// the launch; otherwise the runner calls `stop` exactly once.
pub(super) trait HealthSession {
    type Release;
    fn scope(&self) -> (WorkspaceBinding, PairSlots);
    async fn boot(&mut self, ready_by: Instant) -> Result<(), Q>;
    async fn exchange(
        &mut self,
        probe: AdmittedProbe,
        by: Instant,
    ) -> Result<(ScriptStatus, Option<u32>), Q>;
    async fn stop(self) -> Option<Self::Release>;
}

pub(super) struct HealthRun<'a> {
    pub recipe: RecipeIdentity,
    pub windows: RecipeIdentity,
    pub phase: HealthPhase,
    pub ready_by: Instant,
    pub cutoff: Instant,
    pub cancelled: &'a dyn Fn() -> bool,
}

pub(super) async fn run_health<S: HealthSession>(
    session: S,
    run: HealthRun<'_>,
    windows_probe: impl FnOnce(bool) -> Result<String, Q>,
) -> Result<S::Release, Q> {
    run_health_with(session, run, windows_probe, |script| {
        (script, SCRIPT_TIMEOUT)
    })
    .await
}

/// `dispatched` is the identity in production. The fake guest tests use it
/// to substitute the request after the capability was minted.
async fn run_health_with<S: HealthSession>(
    mut session: S,
    run: HealthRun<'_>,
    windows_probe: impl FnOnce(bool) -> Result<String, Q>,
    dispatched: impl FnOnce(String) -> (String, u32),
) -> Result<S::Release, Q> {
    let check = |by: Instant| launch::check_preparation(by, run.cancelled);
    session.boot(run.ready_by).await?;
    let (binding, pair) = session.scope();
    let scope = HealthScope {
        recipe: run.recipe,
        phase: run.phase,
        binding,
        pair,
    };
    let selected = check(run.cutoff)
        .and_then(|()| select_probe(run.recipe, run.windows, run.phase, windows_probe));
    let health = match selected {
        Ok(script) => {
            let probe = HealthProbe::mint(scope, &script);
            let (script, timeout_s) = dispatched(script);
            let (binding, pair) = session.scope();
            let live = HealthScope {
                binding,
                pair,
                ..scope
            };
            match probe.admit(live, script, timeout_s) {
                Ok(admitted) => {
                    // The guest exchange allows its reply five seconds past
                    // the script timeout.
                    let health_by = run
                        .cutoff
                        .min(Instant::now() + Duration::from_secs(u64::from(SCRIPT_TIMEOUT) + 5));
                    let result = session.exchange(admitted, health_by).await?;
                    check(health_by).and_then(|()| {
                        (result == (ScriptStatus::Exited, Some(0)))
                            .then_some(())
                            .ok_or(HEALTH)
                    })
                }
                Err(reason) => Err(reason),
            }
        }
        Err(reason) => Err(reason),
    };
    let release = session.stop().await;
    health?;
    release.ok_or(PATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const WINDOWS: RecipeIdentity = RecipeIdentity {
        recipe_id: [1; 16],
        recipe_sha256: [1; 32],
    };
    const UBUNTU: RecipeIdentity = RecipeIdentity {
        recipe_id: [2; 16],
        recipe_sha256: [2; 32],
    };

    type Reply = Result<(ScriptStatus, Option<u32>), Q>;

    /// A guest peer that records every transmitted request. It never blocks,
    /// so a failed assertion cannot leave a pending exchange behind.
    struct Guest<'a> {
        scope: (WorkspaceBinding, PairSlots),
        boot: Result<(), Q>,
        reply: Reply,
        during: Option<&'a dyn Fn()>,
        sent: &'a RefCell<Vec<(String, u32)>>,
        stops: &'a Cell<u32>,
    }

    impl HealthSession for Guest<'_> {
        type Release = &'static str;

        fn scope(&self) -> (WorkspaceBinding, PairSlots) {
            self.scope
        }

        async fn boot(&mut self, _: Instant) -> Result<(), Q> {
            self.boot
        }

        async fn exchange(&mut self, probe: AdmittedProbe, _: Instant) -> Reply {
            self.sent.borrow_mut().push(probe.into_parts());
            if let Some(during) = self.during {
                during();
            }
            self.reply
        }

        async fn stop(self) -> Option<&'static str> {
            self.stops.set(self.stops.get() + 1);
            Some("released")
        }
    }

    fn identity(file: u8) -> FileIdentity {
        FileIdentity {
            volume: 7,
            file: [file; 16],
        }
    }

    fn launched(vars: u8) -> (WorkspaceBinding, PairSlots) {
        let generation = [3; 16];
        (
            WorkspaceBinding {
                generation,
                production: identity(1),
                workspace: identity(2),
                lease: identity(3),
            },
            PairSlots {
                disk: ArtifactSlot {
                    generation,
                    role: "disk",
                    key: [4; 16],
                },
                vars: ArtifactSlot {
                    generation,
                    role: "vars",
                    key: [vars; 16],
                },
            },
        )
    }

    /// Mirrors the installed image probe's first line.
    fn probe(validation: bool) -> Result<String, Q> {
        let validation = if validation { "$true" } else { "$false" };
        Ok(format!("$validation = {validation}\n# installed probe"))
    }

    fn guest<'a>(
        sent: &'a RefCell<Vec<(String, u32)>>,
        stops: &'a Cell<u32>,
        reply: Reply,
    ) -> Guest<'a> {
        Guest {
            scope: launched(5),
            boot: Ok(()),
            reply,
            during: None,
            sent,
            stops,
        }
    }

    async fn drive(
        guest: Guest<'_>,
        recipe: RecipeIdentity,
        phase: HealthPhase,
        cutoff: Instant,
        cancelled: &dyn Fn() -> bool,
        dispatched: impl FnOnce(String) -> (String, u32),
    ) -> Result<&'static str, Q> {
        let run = HealthRun {
            recipe,
            windows: WINDOWS,
            phase,
            ready_by: cutoff,
            cutoff,
            cancelled,
        };
        run_health_with(guest, run, probe, dispatched).await
    }

    fn exact(script: String) -> (String, u32) {
        (script, SCRIPT_TIMEOUT)
    }

    const PASSED: Reply = Ok((ScriptStatus::Exited, Some(0)));

    #[tokio::test]
    async fn each_health_phase_transmits_its_own_probe_and_stops_once() {
        let soon = Instant::now() + Duration::from_secs(600);
        assert!(HealthPhase::Readiness.validation());
        assert!(!HealthPhase::Verification.validation());
        for (recipe, phase, expected) in [
            (
                WINDOWS,
                HealthPhase::Readiness,
                "$validation = $true\n# installed probe".to_owned(),
            ),
            (
                WINDOWS,
                HealthPhase::Verification,
                "$validation = $false\n# installed probe".to_owned(),
            ),
            (
                UBUNTU,
                HealthPhase::Verification,
                crate::vm::template::UBUNTU_DETACHED_HEALTH.to_owned(),
            ),
        ] {
            let sent = RefCell::new(Vec::new());
            let stops = Cell::new(0);
            let released = drive(
                guest(&sent, &stops, PASSED),
                recipe,
                phase,
                soon,
                &|| false,
                exact,
            )
            .await;
            assert_eq!(released, Ok("released"), "{phase:?}");
            assert_eq!(sent.into_inner(), vec![(expected, SCRIPT_TIMEOUT)]);
            assert_eq!(stops.get(), 1, "{phase:?}");
        }
    }

    #[tokio::test]
    async fn refused_selection_terminal_status_and_cutoff_stop_without_release() {
        let soon = Instant::now() + Duration::from_secs(600);
        // Ubuntu has no readiness probe, and nothing is transmitted.
        let sent = RefCell::new(Vec::new());
        let stops = Cell::new(0);
        let refused = drive(
            guest(&sent, &stops, PASSED),
            UBUNTU,
            HealthPhase::Readiness,
            soon,
            &|| false,
            exact,
        )
        .await;
        assert_eq!(refused, Err(PHASE));
        assert!(sent.borrow().is_empty());
        assert_eq!(stops.get(), 1);
        // A terminal status other than a clean exit is a health failure.
        for reply in [
            Ok((ScriptStatus::Exited, Some(1))),
            Ok((ScriptStatus::TimedOut, None)),
            Ok((ScriptStatus::Terminated, None)),
        ] {
            let sent = RefCell::new(Vec::new());
            let stops = Cell::new(0);
            let failed = drive(
                guest(&sent, &stops, reply),
                WINDOWS,
                HealthPhase::Verification,
                soon,
                &|| false,
                exact,
            )
            .await;
            assert_eq!(failed, Err(HEALTH), "{reply:?}");
            assert_eq!(sent.borrow().len(), 1);
            assert_eq!(stops.get(), 1);
        }
        // An elapsed cutoff refuses before any script byte is sent.
        let sent = RefCell::new(Vec::new());
        let stops = Cell::new(0);
        let late = drive(
            guest(&sent, &stops, PASSED),
            WINDOWS,
            HealthPhase::Readiness,
            Instant::now(),
            &|| false,
            exact,
        )
        .await;
        assert_eq!(late, Err(PATH));
        assert!(sent.borrow().is_empty());
        assert_eq!(stops.get(), 1);
        // Cancellation during the exchange refuses its result.
        let cancelled = Cell::new(false);
        let sent = RefCell::new(Vec::new());
        let stops = Cell::new(0);
        let cancel = || cancelled.set(true);
        let during = Guest {
            during: Some(&cancel),
            ..guest(&sent, &stops, PASSED)
        };
        let interrupted = drive(
            during,
            WINDOWS,
            HealthPhase::Verification,
            soon,
            &|| cancelled.get(),
            exact,
        )
        .await;
        assert_eq!(interrupted, Err(PATH));
        assert_eq!(sent.borrow().len(), 1);
        assert_eq!(stops.get(), 1);
    }

    #[tokio::test]
    async fn a_failed_boot_or_transport_is_already_settled_by_the_session() {
        let soon = Instant::now() + Duration::from_secs(600);
        let sent = RefCell::new(Vec::new());
        let stops = Cell::new(0);
        let unbooted = Guest {
            boot: Err(NativeRefusal::QmpUnverified),
            ..guest(&sent, &stops, PASSED)
        };
        let failed = drive(
            unbooted,
            WINDOWS,
            HealthPhase::Readiness,
            soon,
            &|| false,
            exact,
        )
        .await;
        assert_eq!(failed, Err(NativeRefusal::QmpUnverified));
        assert!(sent.borrow().is_empty());
        assert_eq!(stops.get(), 0);
        let sent = RefCell::new(Vec::new());
        let stops = Cell::new(0);
        let lost = drive(
            guest(&sent, &stops, Err(HEALTH)),
            WINDOWS,
            HealthPhase::Readiness,
            soon,
            &|| false,
            exact,
        )
        .await;
        assert_eq!(lost, Err(HEALTH));
        assert_eq!(sent.borrow().len(), 1);
        assert_eq!(stops.get(), 0);
    }

    #[tokio::test]
    async fn substituted_probe_bytes_timeout_or_scope_are_never_transmitted() {
        let soon = Instant::now() + Duration::from_secs(600);
        let substitutions: [fn(String) -> (String, u32); 2] = [
            |script| (format!("{script}\nStop-Computer -Force"), SCRIPT_TIMEOUT),
            |script| (script, SCRIPT_TIMEOUT + 1),
        ];
        for substitute in substitutions {
            let sent = RefCell::new(Vec::new());
            let stops = Cell::new(0);
            let refused = drive(
                guest(&sent, &stops, PASSED),
                WINDOWS,
                HealthPhase::Readiness,
                soon,
                &|| false,
                substitute,
            )
            .await;
            assert_eq!(refused, Err(HEALTH));
            assert!(sent.borrow().is_empty());
            assert_eq!(stops.get(), 1);
        }
        let (binding, pair) = launched(5);
        let scope = HealthScope {
            recipe: WINDOWS,
            phase: HealthPhase::Readiness,
            binding,
            pair,
        };
        let script = probe(true).unwrap();
        let other_pair = HealthScope {
            pair: launched(6).1,
            ..scope
        };
        let other_phase = HealthScope {
            phase: HealthPhase::Verification,
            ..scope
        };
        let other_recipe = HealthScope {
            recipe: UBUNTU,
            ..scope
        };
        for other in [other_pair, other_phase, other_recipe] {
            assert!(
                HealthProbe::mint(scope, &script)
                    .admit(other, script.clone(), SCRIPT_TIMEOUT)
                    .is_err()
            );
        }
        let admitted = HealthProbe::mint(scope, &script)
            .admit(scope, script.clone(), SCRIPT_TIMEOUT)
            .unwrap();
        assert_eq!(admitted.into_parts(), (script, SCRIPT_TIMEOUT));
    }
}

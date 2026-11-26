//! Checked process adapter for installer phases, installed health, and the
//! offline image helper. Scope recording and teardown stay on the shared
//! native launch path.

use super::*;
use crate::vm::test_issuer::{Intent, NativeTarget, TestIssuer};
use crate::vm::windows::qualification::recipe::BootReservation;

struct Checked<'a, C> {
    qualification: &'a ForegroundQualification<'a>,
    issuer: &'a TestIssuer<'a, ForegroundQualification<'a>, C>,
    intent: &'a Intent<'a, ForegroundQualification<'a>, C>,
    cwd: LocalPath,
    cancel: &'a Shutdown,
    cancelled: &'a dyn Fn() -> bool,
    limits: BuildRequest<'a>,
}

impl<C: Clock> Effects for Checked<'_, C> {
    fn fresh(&self) -> Result<Instant, Q> {
        self.issuer.refresh()
    }

    fn stage<'w>(
        &self,
        reservation: InstallerReservation<'w>,
        ledger: &dyn CustodyCheckpoints,
    ) -> Result<StagedInstaller<'w>, Q> {
        self.qualification
            .stage_installer(reservation, ledger, self.cancelled, |target| {
                let bound = if target.slot.role == "seed" {
                    self.limits.transient.staging
                } else {
                    self.limits.retained.images
                };
                if target.content.size > bound {
                    return Err(Q::DiskCeilingUnverified);
                }
                self.intent.stage(target)?.consume(target)
            })
    }

    fn image(&self) -> Result<Artifact, Q> {
        self.qualification.helper_image()
    }

    async fn helper<'w>(
        &'w self,
        prepared: &'w PreparedHelper<'w, '_>,
        image: &'w Artifact,
        backing: Option<(&Artifact, &[u8])>,
        size: u64,
        ceiling: u64,
    ) -> HelperSettlement<'w> {
        let plan = prepared.staging().and_then(|target| {
            crate::vm::qemu_img::create(prepared.lease(), image, &target, backing, size, ceiling)
                .map_err(|_| Q::DiskCeilingUnverified)
        });
        let launch = plan.and_then(|plan| {
            let permit = self.intent.launch(
                prepared.workspace().binding(),
                prepared.launch_target(&plan, &self.cwd),
            )?;
            Ok((plan, permit))
        });
        match launch {
            Ok((plan, permit)) => {
                prepared
                    .run(
                        self.qualification,
                        plan,
                        &self.cwd,
                        permit,
                        self.cancel,
                        self.cancelled,
                        || self.fresh(),
                    )
                    .await
            }
            Err(reason) => HelperSettlement::quarantined(Err(reason)),
        }
    }

    fn authorize_copy(
        &self,
        pair: PairSlots,
        content: crate::vm::paths::ArtifactPin,
    ) -> Result<CopyAuthority, Q> {
        self.intent
            .copy_verification(pair, content)?
            .consume(pair.vars, content)
    }

    async fn boot<'w>(
        &self,
        request: Boot<'_>,
        workspace: &'w ProductionWorkspace,
    ) -> Result<BootRelease<'w>, Q> {
        let Boot {
            workspace: source,
            pair,
            recipe,
            phase,
            staged,
            backing,
            cutoff,
        } = request;
        if source.binding() != workspace.binding() {
            return Err(PATH);
        }
        let installer = match phase {
            Phase::Install(stage) => Some(stage),
            Phase::Health { .. } => None,
        };
        let qualified = self.qualification.prepare_boot(BootReservation {
            workspace,
            recipe,
            phase: installer.map(InstallerStage::phase),
            pair: pair.slots,
            identities: pair.evidence.map(|file| file.identity),
            backing,
            staged,
        })?;
        let planner = qualified.planner();
        let plan = planner.plan()?;
        let cancel = self.cancel;
        let cancelled = || cancel.is_cancelled();
        let check = |by| launch::check_preparation(by, &cancelled);
        let fresh = |by| {
            check(by)?;
            let captured = self.fresh()?;
            check(by)?;
            Ok(captured)
        };
        let ready_by = if installer.is_some() {
            cutoff
        } else {
            cutoff.min(Instant::now() + crate::vm::guest::GUEST_READY_BUDGET)
        };
        let permit = self.intent.launch(
            workspace.binding(),
            LaunchTarget {
                plan: &plan,
                writer_epoch: workspace.binding().generation,
                cwd: qualified.cwd(),
                pair: Some(pair.slots),
                installer,
                recipe: Some(recipe),
                helper: None,
                memory: TEST_JOB_MEMORY,
                cpu_rate: JOB_CPU_RATE,
            },
        )?;
        let launch = NativeLaunch::hold(
            self.qualification,
            LaunchRequest {
                plan,
                boot_inputs: Some(&qualified),
                cwd: qualified.cwd(),
                workspace,
                writer_epoch: workspace.binding().generation,
                pair: Some(pair),
                installer,
                helper_target: None,
                memory: TEST_JOB_MEMORY,
                cpu_rate: JOB_CPU_RATE,
                prepare_by: ready_by,
                cancelled: &cancelled,
            },
            permit,
            || fresh(ready_by),
        );
        let mut launch = match launch {
            Ok(launch) => launch,
            Err(mut refusal) => {
                if refusal.creation() != StartCreation::NeverCreated {
                    let _ = refusal
                        .settle_created(tokio::time::Instant::now() + Duration::from_secs(2))
                        .await;
                }
                return Err(refusal.reason());
            }
        };
        let (release, finalized) = match phase {
            Phase::Install(_) => match launch
                .run_installer(cancel, || fresh(cutoff), || true)
                .await
            {
                Ok(InstallerSettlement::Released {
                    generation,
                    finalization,
                }) => (generation, finalization.map(Finalized::Native)),
                Ok(InstallerSettlement::Failed { .. }) => return Err(PHASE),
                Err(reason) => {
                    let _ = launch.stop(cancel, || true).await;
                    return Err(reason);
                }
            },
            Phase::Health { verification } => {
                launch
                    .boot(cancel, || fresh(ready_by), || true)
                    .await
                    .map_err(|failed| failed.reason)?;
                // Boot readiness has its own budget. Script preparation and
                // the guest exchange are bounded by the phase cutoff.
                let checked = (|| {
                    check(cutoff)?;
                    let script = if recipe == self.qualification.evidence().recipes.windows {
                        self.qualification.windows_health(recipe, verification)?
                    } else {
                        crate::vm::template::UBUNTU_DETACHED_HEALTH.to_owned()
                    };
                    Ok(script)
                })();
                let health = match checked {
                    Ok(script) => {
                        // The guest exchange allows its reply five seconds
                        // past the script timeout.
                        let health_by = cutoff.min(
                            Instant::now() + Duration::from_secs(u64::from(SCRIPT_TIMEOUT) + 5),
                        );
                        let result = launch
                            .run_script(
                                script,
                                SCRIPT_TIMEOUT,
                                cancel,
                                || fresh(health_by),
                                || true,
                            )
                            .await
                            .map_err(|failed| failed.reason)?;
                        check(health_by).and_then(|()| {
                            (result == (ScriptStatus::Exited, Some(0)))
                                .then_some(())
                                .ok_or(HEALTH)
                        })
                    }
                    Err(reason) => Err(reason),
                };
                let (_, release) = launch.stop(cancel, || true).await;
                health?;
                (release.ok_or(PATH)?, None)
            }
        };
        fresh(cutoff)?;
        Ok(BootRelease {
            release: release.for_build(workspace, pair)?,
            phase: installer.map(InstallerStage::phase),
            finalized,
        })
    }
}

impl<'q> ForegroundQualification<'q> {
    /// Runs an already-admitted build. Installation of the issuer and the
    /// entry point that admits native production requests land in chunk 16.
    #[allow(clippy::too_many_arguments, reason = "one admitted build's inputs")]
    pub(crate) async fn build_test_golden<C: Clock>(
        &'q self,
        controller: &VmController<C>,
        issuer: &'q TestIssuer<'q, Self, C>,
        worker: &PendingOperation,
        slots: BuildSlots,
        request: BuildRequest<'_>,
        guards: &GenerationGuards,
        cancel: &Shutdown,
    ) -> anyhow::Result<Status> {
        let ledger = LedgerCheckpoints {
            store: controller.native_store(),
            origin: native::Origin::Build,
        };
        let disposal = LedgerDisposal {
            store: controller.native_store(),
        };
        let prepared = (|| {
            let refused = |error: Q| anyhow::anyhow!("TEST build refused: {error:?}");
            let intent = issuer
                .intent(
                    worker,
                    NativeTarget {
                        generation: slots.pair.disk.generation,
                        writer_epoch: None,
                    },
                    cancel,
                )
                .map_err(refused)?;
            let recipe = intent.build_recipe().ok_or_else(|| refused(PHASE))?;
            let cwd = self.command_cwd().map_err(refused)?;
            let primary = self
                .reserve_workspace(slots.pair.disk.generation, &ledger)
                .map_err(refused)?;
            Ok::<_, anyhow::Error>((intent, recipe, cwd, primary))
        })();
        let (intent, recipe, cwd, primary) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                controller.fail_build_with(
                    worker,
                    slots,
                    false,
                    Some(FailureReason::StagingFailed),
                )?;
                return Err(error);
            }
        };
        let cancelled = || cancel.is_cancelled();
        let effects = Checked {
            qualification: self,
            issuer,
            intent: &intent,
            cwd,
            cancel,
            cancelled: &cancelled,
            limits: request,
        };
        let build = Build {
            controller,
            worker,
            request,
            slots,
            recipe,
            ledger: &ledger,
            disposal: self.disposal(&disposal, guards),
            cancel,
            size: if request.recipe == hex::encode(VmController::<C>::windows_recipe_id()) {
                40 * GIB
            } else {
                24 * GIB
            },
            owner: self.held.token.account.0,
        };
        build
            .run(&effects, primary, || {
                self.reserve_workspace(slots.verification.disk.generation, &ledger)
            })
            .await
    }
}

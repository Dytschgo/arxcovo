use super::*;
use crate::store::vm::{native::Origin, snapshot::Work};
use crate::vm::{
    ClonePublication, FailureReason, HostObservation, PendingOperation, Resources, VmController,
    test_issuer::{NativeTarget, SelectedSource, TestIssuer, retry_contended},
};
use clone::{CheckedOverlay, CloneRequest, stage_linked_clone};
use snapshot::{
    CheckedSnapshotInspector, ControllerSnapshot, Holds, SnapshotRequest, copy_offline_snapshot,
};

pub(crate) const TEST_RESOURCES: Resources = Resources {
    vcpus: 2,
    guest_ram: 4 * GIB,
    overhead_ram: 3 * GIB,
};

impl ForegroundQualification {
    pub(crate) fn test_host(&self) -> Result<HostObservation, NativeRefusal> {
        self.observe_host(TEST_JOB_MEMORY).map(|(_, host)| host)
    }

    #[allow(clippy::too_many_arguments, reason = "one admitted clone's wiring")]
    pub(crate) async fn clone_test<S: SelectedSource, C: crate::vm::Clock>(
        &self,
        controller: &VmController<C>,
        issuer: &TestIssuer<'_, S, C>,
        worker: PendingOperation,
        slots: PairSlots,
        size: u64,
        cutoff: Instant,
        cancel: &Shutdown,
    ) -> anyhow::Result<()> {
        let cancelled = || cancel.is_cancelled();
        let finish = |result, gate: ClonePublication<'_>| {
            controller.finish_native_clone(&worker, slots, result, gate)
        };
        let prepared = (|| {
            let intent = issuer.intent(
                &worker,
                NativeTarget {
                    generation: slots.disk.generation,
                    writer_epoch: None,
                },
                cancel,
            )?;
            Ok::<_, NativeRefusal>((intent, self.helper_image()?, self.command_cwd()?))
        })();
        let Ok((intent, image, cwd)) = prepared else {
            finish(
                Err(FailureReason::CloneFailed),
                ClonePublication {
                    golden: None,
                    effect_cutoff: cutoff,
                    cancelled: &cancelled,
                },
            )?;
            return Ok(());
        };
        let ledger = LedgerCheckpoints {
            store: controller.native_store(),
            origin: Origin::Clone,
        };
        stage_linked_clone(
            || {
                controller
                    .admitted_sealed_golden(&worker)
                    .ok()
                    .flatten()
                    .ok_or(NativeRefusal::PathCustodyUnverified)
            },
            |golden| {
                let scope = retry_contended(
                    &cancelled,
                    &|| issuer.epoch().effects_revoked(),
                    || anyhow::anyhow!("TEST qualification missing"),
                    || controller.native_workspace_scope(golden.slots().disk.generation),
                )
                .map_err(|_| NativeRefusal::PathCustodyUnverified)?;
                self.hold_golden(
                    &ledger,
                    binding_from_scope(scope),
                    golden,
                    cutoff,
                    &cancelled,
                )
            },
            || self.reserve_workspace(slots.disk.generation, &ledger),
            &ledger,
            &CloneRequest {
                slots,
                size,
                worker_deadline: worker.deadline(),
                effect_cutoff: cutoff,
                owner: self.owner(),
                cancelled: &cancelled,
            },
            &image,
            CheckedOverlay {
                qualification: self,
                cwd: &cwd,
                cancel,
                cancelled: &cancelled,
                mint: |scope, target: LaunchTarget<'_, '_>| intent.launch(scope, target),
                refresh: || issuer.refresh(),
            },
            |slot, content| {
                self.test_host()?;
                intent.copy(slot, content)?.consume(slot, content)
            },
            finish,
        )
        .await?;
        Ok(())
    }

    /// Called by the tracked blocking worker. The inspector drives its helper
    /// via Handle::block_on, so the caller must not enter an async task here.
    #[allow(clippy::too_many_arguments, reason = "one admitted snapshot's wiring")]
    pub(crate) fn snapshot_test<S: SelectedSource, C: crate::vm::Clock>(
        &self,
        controller: &VmController<C>,
        issuer: &TestIssuer<'_, S, C>,
        worker: PendingOperation,
        work: Work,
        cutoff: Instant,
        cancel: &Shutdown,
        runtime: &tokio::runtime::Handle,
    ) -> anyhow::Result<()> {
        let host = || {
            self.test_host().unwrap_or(HostObservation {
                owner_idle: None,
                logical_cpus: None,
                available_ram: None,
                capacity: None,
            })
        };
        let ledger = ControllerSnapshot {
            controller,
            worker: &worker,
            work: &work,
            host: &host,
            resources: TEST_RESOURCES,
        };
        let prepared = (|| {
            let slots = work.candidate.ok_or(NativeRefusal::PathCustodyUnverified)?;
            let intent = issuer.intent(
                &worker,
                NativeTarget {
                    generation: slots.disk.generation,
                    writer_epoch: None,
                },
                cancel,
            )?;
            Ok::<_, NativeRefusal>((
                slots,
                intent,
                self.helper_image()?,
                self.command_cwd()?,
                self.snapshot_firmware()?,
            ))
        })();
        let Ok((slots, intent, image, cwd, firmware)) = prepared else {
            return controller.fail_snapshot_copy(
                &worker,
                &work,
                [crate::store::vm::snapshot::StagedLeaf::Untouched; 2],
            );
        };
        let store = controller.native_store();
        let custody = LedgerCheckpoints {
            store,
            origin: Origin::Restore,
        };
        let disposal = LedgerDisposal { store };
        let request = SnapshotRequest {
            work: &work,
            worker: &worker,
            production: self.production_root(),
            owner: self.owner(),
            firmware,
            effect_cutoff: cutoff,
            cancelled: &|| cancel.is_cancelled(),
        };
        copy_offline_snapshot(
            &request,
            || {
                Holds::hold(
                    request.production,
                    request.owner,
                    &custody,
                    &disposal,
                    &work,
                    cutoff.min(worker.deadline()),
                    request.cancelled,
                )
            },
            || self.reserve_workspace(slots.disk.generation, &custody),
            &custody,
            |slot, content| {
                self.test_host()?;
                intent.copy(slot, content)?.consume(slot, content)
            },
            &mut CheckedSnapshotInspector {
                qualification: self,
                image: &image,
                cwd: &cwd,
                cancel,
                runtime,
                mint: |scope, target: LaunchTarget<'_, '_>| intent.launch(scope, target),
                refresh: || issuer.refresh(),
            },
            &ledger,
        )
    }
}

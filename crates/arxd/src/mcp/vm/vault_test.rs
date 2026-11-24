//! The vault drives the same TEST VM admission and custody path as the local
//! profile. Native operations remain fenced until the VM profile can issue them.
use super::{
    Action, Gate, Guest, Id, Mutation, OperationStatus, Status, Supervisor, VmInventory, Work,
    parse_id,
};
use crate::vault::{self, ScriptOs, TestJob, Vault, VaultTester};
use arx_guest::ScriptOutcome;
use std::{
    path::Path,
    sync::{Arc, Weak, atomic::Ordering},
    time::{Duration, Instant},
};

const DAEMON: &str = "arxd"; // Reserved by the TEST VM profile loader.
const LEASE_SECONDS: u64 = 600;
const SCRIPT_SECONDS: u32 = 30;
const JOB_BUDGET: Duration = Duration::from_secs(480);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Default)]
pub(super) struct Jobs {
    tasks: tokio::task::JoinSet<anyhow::Result<()>>,
    failed: bool,
}

struct Tester(Weak<Supervisor>);

impl VaultTester for Tester {
    fn enqueue(&self, job: TestJob) {
        if let Some(supervisor) = self.0.upgrade()
            && let Ok(mut jobs) = supervisor.vault_jobs.lock()
        {
            // The dispatch gate remains held through this registration. Keep
            // failed joins for drain evidence and bound the healthy history.
            while let Some(result) = jobs.tasks.try_join_next() {
                jobs.failed |= !matches!(result, Ok(Ok(())));
            }
            let supervisor = supervisor.clone();
            jobs.tasks.spawn_blocking(move || {
                let mut run = Run {
                    inventory: VmInventory {
                        supervisor,
                        principal: DAEMON.into(),
                        closed: Default::default(),
                    },
                    deadline: Instant::now() + JOB_BUDGET,
                    clone_operation: None,
                    vm: None,
                };
                let outcome = run.exercise(&job).unwrap_or(ScriptOutcome::Unavailable);
                job.complete(outcome, run.teardown())
            });
        }
    }
}

impl Supervisor {
    pub(crate) fn open_vault(self: &Arc<Self>, state: &Path) -> anyhow::Result<()> {
        let vault = Vault::open(
            state,
            Some(Arc::new(Tester(Arc::downgrade(self)))),
            vault::utc_ms()?,
        )?;
        self.vault
            .set(vault)
            .map_err(|_| anyhow::anyhow!("VM vault already initialized"))
    }

    pub(super) fn vault_test_available(&self, gate: &Gate) -> bool {
        #[cfg(test)]
        if gate.fixture.is_some() {
            return true;
        }
        let _ = gate;
        matches!(
            self.candidate().capability(),
            crate::vm::runtime::NativeCapability::Qualified(_)
        )
    }

    pub(super) async fn drain_vault_jobs(
        &self,
        deadline: tokio::time::Instant,
    ) -> anyhow::Result<()> {
        let mut jobs = std::mem::take(
            &mut *self
                .vault_jobs
                .lock()
                .map_err(|_| anyhow::anyhow!("vault drain unconfirmed"))?,
        );
        while let Some(result) = tokio::time::timeout_at(deadline, jobs.tasks.join_next()).await? {
            jobs.failed |= !matches!(result, Ok(Ok(())));
        }
        anyhow::ensure!(!jobs.failed, "vault drain unconfirmed");
        Ok(())
    }
}

struct Run {
    inventory: VmInventory,
    deadline: Instant,
    clone_operation: Option<Id>,
    vm: Option<Id>,
}

impl Run {
    fn exercise(&mut self, job: &TestJob) -> Option<ScriptOutcome> {
        let vm = self.inventory.supervisor.clone();
        let (recipe_index, guest) = match job.os() {
            ScriptOs::Ubuntu => (0, Guest::UbuntuX86_64),
            ScriptOs::Windows => (2, Guest::WindowsX86_64),
        };
        let (recipe, digest) = vm.recipes[recipe_index]?;
        let template = vm
            .controller
            .vault_test_template(recipe, digest?, guest)
            .ok()??;
        let create = self.submit(Action::Clone, template, Work::Create(LEASE_SECONDS))?;
        self.clone_operation = Some(create);
        self.vm = self.status(create)?.vm;
        if self.wait(create)?.state != Status::Succeeded {
            return None;
        }
        let id = self.vm?;
        let start = self.submit(Action::Start, id, Work::Start)?;
        if self.wait(start)?.state != Status::Succeeded {
            return None;
        }
        let script = self.submit(
            Action::Script,
            id,
            Work::Script(job.content().into(), SCRIPT_SECONDS),
        )?;
        let result = self.wait(script)?;
        use crate::vm::ScriptResult;
        Some(match result.script? {
            ScriptResult::Exited(0) if result.state != Status::Succeeded => return None,
            ScriptResult::Exited(exit_code) => ScriptOutcome::Exited { exit_code },
            ScriptResult::Unavailable => ScriptOutcome::Unavailable,
            ScriptResult::TimedOut => ScriptOutcome::TimedOut,
            ScriptResult::LaunchFailed => ScriptOutcome::LaunchFailed,
            ScriptResult::Terminated => ScriptOutcome::Terminated,
        })
    }

    fn submit(&self, action: Action, id: Id, work: Work) -> Option<Id> {
        let supervisor = &self.inventory.supervisor;
        let mut gate = supervisor.lock().ok()?;
        if gate.closed
            || supervisor.fenced.load(Ordering::SeqCst)
            || Instant::now() >= self.deadline
        {
            return None;
        }
        let key = supervisor.controller.vault_test_key().ok()?;
        let value = self
            .inventory
            .mutate(&mut gate, action as i64, Mutation { key, id, work })
            .ok()?;
        parse_id(value.get("operation_id")?.as_str()?).ok()
    }

    fn status(&self, operation: Id) -> Option<OperationStatus> {
        self.inventory
            .supervisor
            .controller
            .operation_status(DAEMON, operation)
            .ok()?
            .ok()
    }

    fn wait(&self, operation: Id) -> Option<OperationStatus> {
        let supervisor = &self.inventory.supervisor;
        while !supervisor.fenced.load(Ordering::SeqCst) && Instant::now() < self.deadline {
            let status = self.status(operation)?;
            #[cfg(test)]
            let working = supervisor
                .lock()
                .ok()?
                .fixture
                .as_ref()
                .is_some_and(|fixture| status.vm.is_some_and(|vm| fixture.worker_active(vm)));
            #[cfg(not(test))]
            let working = false;
            if status.state != Status::Pending && !working {
                return Some(status);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        None
    }

    fn teardown(&self) -> bool {
        let Some(operation) = self.clone_operation else {
            // No confirmed reservation and no result that could pass.
            return false;
        };
        let Some(id) = self.vm.or_else(|| self.status(operation)?.vm) else {
            return false;
        };
        let supervisor = &self.inventory.supervisor;
        // The reaper stops the owned TEST job, verifies generation disposal,
        // reconciles the charge, and records the expiry proof. It retains any
        // uncertain custody for subsequent turns.
        supervisor
            .controller
            .expire_vault_test(id)
            .and_then(|()| supervisor.sweep())
            .and_then(|()| supervisor.controller.vault_test_released(id))
            .unwrap_or(false)
    }
}

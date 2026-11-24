use super::*;
use arxd::vm::paths::LocalPath;
use ledger::{Claim, Id, Ledger, Pair, PairProof, config, digest, id, open, pin_root, safe};
use rmcp::{RoleClient, model::CallToolRequestParams, service::RunningService};
use serde::Deserialize;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    process::{Child, Command},
    time::timeout,
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, GetLastError},
    Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE},
    System::{Console::*, Pipes::WaitNamedPipeW},
};

mod ledger;

const RPC: Duration = Duration::from_secs(15);
const TICK: Duration = Duration::from_secs(2);
const VM_WORK: Duration = Duration::from_secs(360);
const EXPIRY_GRACE: Duration = Duration::from_secs(180);
const UBUNTU_TTL: i64 = 1_800;
const WINDOWS_TTL: i64 = 2_400;
const SCRIPT_TIMEOUT: u64 = 30;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    daemon: PathBuf,
    daemon_sha256: String,
    daemon_config: PathBuf,
    client_config: PathBuf,
    qualification_sha256: Option<String>,
    evidence_dir: PathBuf,
    ubuntu: Recipe,
    windows: Recipe,
    #[serde(default)]
    idle_delay_s: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    recipe_id: String,
    recipe_sha256: String,
}

#[derive(Deserialize)]
struct Foreground {
    state_dir: PathBuf,
    local: arxd::local::Endpoint,
    agents: Vec<arxd::config::Agent>,
    test_vms: VmProfile,
}

#[derive(Deserialize)]
struct BridgeConfig {
    address: String,
}

#[derive(Deserialize)]
struct VmProfile {
    #[serde(default)]
    allow_agent_public_sites: bool,
    publication: Option<Value>,
    runtime: Option<RuntimeRoot>,
}

#[derive(Deserialize)]
struct RuntimeRoot {
    state: PathBuf,
}

#[derive(Serialize)]
struct Evidence {
    version: u8,
    run_id: String,
    client_sha256: String,
    daemon_sha256: String,
    qualification_sha256: Option<String>,
    started_at_ms: i64,
    finished_at_ms: i64,
    observations: Vec<Value>,
    drain: bool,
    failure: Option<Failure>,
    drain_failure: Option<Failure>,
}

impl Evidence {
    fn record(&mut self, step: &'static str, proof: Value) {
        self.observations.push(json!({"step":step,"proof":proof}));
    }

    fn write(&self, root: &Path) -> Result<()> {
        let directory = root.join(&self.run_id);
        safe(fs::create_dir(&directory), "evidence_directory")?;
        let _pin = open(&directory, true, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
        let body = safe(serde_json::to_vec_pretty(self), "evidence_encode")?;
        write_new(&directory.join("acceptance.json"), &body)?;
        let manifest = json!({
            "version":1,
            "files":[{"name":"acceptance.json","bytes":body.len(),
                "sha256":hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&body))}]
        });
        write_new(
            &directory.join("manifest.json"),
            &safe(serde_json::to_vec_pretty(&manifest), "manifest_encode")?,
        )
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = safe(
        OpenOptions::new().write(true).create_new(true).open(path),
        "evidence_open",
    )?;
    safe(file.write_all(bytes), "evidence_write")?;
    safe(file.sync_all(), "evidence_sync")
}

fn fresh() -> Result<Id> {
    let mut id = [0; 16];
    safe(getrandom::fill(&mut id), "random_id")?;
    check(id != [0; 16], "random_id")?;
    Ok(id)
}

fn now_ms() -> Result<i64> {
    let duration = safe(SystemTime::now().duration_since(UNIX_EPOCH), "utc_clock")?;
    safe(i64::try_from(duration.as_millis()), "utc_clock")
}

fn clock_sample() -> Result<ClockSample> {
    Ok(ClockSample {
        monotonic: Instant::now(),
        utc_ms: now_ms()?,
    })
}

type Client = RunningService<RoleClient, ()>;

async fn rpc(client: &Client, name: &'static str, args: Value) -> Result<Value> {
    let request = CallToolRequestParams::new(name).with_arguments(
        args.as_object()
            .cloned()
            .ok_or(Failure::Check("arguments"))?,
    );
    let result = timeout(RPC, client.call_tool(request))
        .await
        .map_err(|_| Failure::Timeout(name))?;
    let result = safe(result, name)?;
    let row = result.structured_content.ok_or(Failure::Check(name))?;
    if row["ok"] == false {
        return Err(reason(&row["reason"]).map_or(Failure::Check(name), Failure::AdmissionClosed));
    }
    check(row["ok"] == true && result.is_error != Some(true), name)?;
    Ok(row)
}

async fn terminal(client: &Client, operation: Id, budget: Duration) -> Result<Value> {
    poll("vm_operation", budget, TICK, || async {
        let row = rpc(
            client,
            "vm_operation",
            json!({"operation_id":hex::encode(operation)}),
        )
        .await?;
        check(id(&row, "operation_id")? == operation, "operation_binding")?;
        match row["state"].as_str() {
            Some("pending") => Ok(None),
            Some("succeeded" | "interrupted" | "failed") => Ok(Some(row)),
            _ => Err(Failure::Check("operation_state")),
        }
    })
    .await
}

struct Acceptance<'a> {
    client: &'a Client,
    ledger: Ledger,
    evidence: &'a mut Evidence,
    principal: &'a str,
    operations: Vec<Operation>,
}

struct Operation {
    request: Id,
    completed: Value,
    action: Action,
}

#[derive(Clone, Copy)]
enum Action {
    Build,
    Create,
    Start,
    Script,
    Stop,
    Snapshot,
    Restore,
}

impl Action {
    fn wire(self) -> (&'static str, &'static str, i64) {
        match self {
            Self::Build => ("vm_template_build", "template_build", 11),
            Self::Create => ("vm_create", "create", 1),
            Self::Start => ("vm_start", "start", 3),
            Self::Script => ("vm_run_script", "run_script", 2),
            Self::Stop => ("vm_stop", "stop", 4),
            Self::Snapshot => ("vm_snapshot_create", "snapshot", 5),
            Self::Restore => ("vm_snapshot_restore", "restore", 6),
        }
    }
}

fn succeeded(row: &Value) -> Result<()> {
    check(
        row["state"] == "succeeded" && row["cleanup"] == "confirmed",
        "operation_not_clean",
    )
}

impl Acceptance<'_> {
    async fn operation(
        &mut self,
        action: Action,
        mut args: Value,
        budget: Duration,
    ) -> Result<(Value, Value, Value)> {
        let request = fresh()?;
        args["request_id"] = json!(hex::encode(request));
        let (tool, label, _) = action.wire();
        let received = rpc(self.client, tool, args.clone()).await?;
        let operation = id(&received, "operation_id")?;
        self.evidence.record(
            "operation_admitted",
            json!({"action":label,"request_id":hex::encode(request),
            "receipt":operation_evidence(&received)}),
        );
        let result = terminal(self.client, operation, budget).await?;
        self.evidence
            .record("operation_terminal", operation_evidence(&result));
        check(
            result["action"] == label
                && (args["vm_id"].is_null() || result["vm_id"] == args["vm_id"]),
            "operation_subject",
        )?;
        self.operations.push(Operation {
            request,
            completed: result.clone(),
            action,
        });
        Ok((result, received, args))
    }

    async fn vm(&self, vm: Id) -> Result<Value> {
        let result = rpc(self.client, "vm_get", json!({"vm_id":hex::encode(vm)})).await?;
        check(id(&result, "vm_id")? == vm, "vm_binding")?;
        Ok(result)
    }

    async fn activity(&self, id: i64) -> Result<Value> {
        let read = rpc(self.client, "list_activity", json!({"activity_id":id})).await?;
        let rows = read["activities"]
            .as_array()
            .ok_or(Failure::Check("activity_list"))?;
        check(
            rows.len() == 1 && rows[0]["activity_id"] == id,
            "activity_id",
        )?;
        Ok(rows[0].clone())
    }

    async fn correlate(&mut self) -> Result<()> {
        for op in &self.operations {
            let operation = id(&op.completed, "operation_id")?;
            let (_, label, action) = op.action.wire();
            let activity = self
                .ledger
                .activity(operation, op.request, self.principal, action)?;
            let row = self.activity(activity).await?;
            check(
                row["identity_mode"] == "local_vm_principal"
                    && row["actor"] == "agent"
                    && row["action"] == label
                    && row["operation_id"] == op.completed["operation_id"]
                    && row["state"] == op.completed["state"]
                    && row["vm_id"] == op.completed["vm_id"]
                    && row["cleanup"] == op.completed["cleanup"],
                "mcp_activity_correlation",
            )?;
            self.evidence.record(
                "activity_correlated",
                json!({"activity_id":activity,
                "operation_id":hex::encode(operation),"caller":"local_vm_principal"}),
            );
        }
        Ok(())
    }

    async fn template(
        &mut self,
        entries: &[Value],
        recipe: &Recipe,
        guest: Guest,
    ) -> Result<Golden> {
        let paths = entries.iter().filter(|entry| {
            entry["guest"] == guest.label() && entry["recipe_id"] == recipe.recipe_id
        });
        let mut existing = paths.filter(|entry| entry["state"] == "sealed");
        let template = if let Some(sealed) = existing.next() {
            check(existing.next().is_none(), "ambiguous_sealed_template")?;
            id(sealed, "template_id")?
        } else {
            let (built, _, _) = self
                .operation(
                    Action::Build,
                    json!({"recipe_id":recipe.recipe_id}),
                    Duration::from_secs(guest.install_seconds()),
                )
                .await?;
            succeeded(&built)?;
            check(built["recipe_id"] == recipe.recipe_id, "build_recipe")?;
            id(&built, "template_id")?
        };
        let (pair, charge) = self.ledger.golden(
            template,
            &recipe.recipe_id,
            &recipe.recipe_sha256,
            guest.code(),
        )?;
        let proof = self.ledger.inspect(&pair, None, true)?;
        self.evidence.record("sealed_template", json!({"guest":guest.label(),"template_id":hex::encode(template),
            "recipe_id":recipe.recipe_id,"recipe_sha256":recipe.recipe_sha256,
            "generation":hex::encode(Ledger::generation(&pair)),"charge":charge,"pair":proof.evidence()}));
        Ok(Golden {
            template,
            pair,
            proof,
            charge,
        })
    }

    async fn clone(&mut self, golden: &Golden, guest: Guest, ttl: i64, replay: bool) -> Result<Vm> {
        let issued_at = clock_sample()?;
        let ttl_duration = Duration::from_secs(ttl as u64);
        let template = hex::encode(golden.template);
        let (completed, receipt, args) = self
            .operation(
                Action::Create,
                json!({"template_id":template,"ttl_s":ttl}),
                VM_WORK,
            )
            .await?;
        succeeded(&completed)?;
        check(completed["template_id"] == template, "clone_template")?;
        let vm = id(&completed, "vm_id")?;
        let status = self.vm(vm).await?;
        let deadline = status["expires_at_ms"]
            .as_i64()
            .ok_or(Failure::Check("clone_lease"))?;
        check(
            status["state"] == "stopped"
                && status["ready"] == false
                && status["guest"] == guest.label()
                && status["template_id"] == template
                && status["expires_at_ms"] == completed["expires_at_ms"],
            "clone_lease",
        )?;
        let lease = LeaseClock::new(issued_at, deadline, ttl_duration, RPC)?;
        lease.check_clock(clock_sample()?)?;
        let pair = self.ledger.vm_pair(vm)?;
        self.ledger.distinct(&golden.pair, &pair)?;
        let proof = self.ledger.inspect(&pair, Some(&golden.pair), false)?;
        check(
            proof.contents[1] == golden.proof.contents[1],
            "clone_vars_initial_pin",
        )?;
        let claim = self.ledger.charge(vm)?;
        check(
            claim.active > 0 && claim.snapshot == 0 && claim.staged == 0 && claim.generations == 1,
            "clone_charge",
        )?;
        self.evidence.record(
            "clone_created",
            json!({"vm_id":hex::encode(vm),"generation":hex::encode(Ledger::generation(&pair)),
            "deadline_ms":deadline,"pair":proof.evidence(),"charges":claim.evidence()}),
        );
        if replay {
            let before = self.ledger.effect(vm)?;
            let second = rpc(self.client, "vm_create", args).await?;
            let second_row = terminal(self.client, id(&second, "operation_id")?, VM_WORK).await?;
            retry_unchanged(&receipt, &second, &before, &self.ledger.effect(vm)?)?;
            check(
                second_row == completed
                    && self.vm(vm).await? == status
                    && self.ledger.vm_pair(vm)? == pair
                    && self
                        .ledger
                        .inspect(&pair, Some(&golden.pair), false)?
                        .contents
                        == proof.contents,
                "retry_clone_unchanged",
            )?;
            self.evidence.record(
                "same_key_retry",
                json!({"operation_id":receipt["operation_id"],"effect":before}),
            );
        }
        Ok(Vm {
            id: vm,
            deadline,
            lease,
            pair,
        })
    }

    async fn start(&mut self, vm: &Vm, guest: Guest) -> Result<Id> {
        let args = json!({"vm_id":hex::encode(vm.id)});
        let (started, _, _) = self.operation(Action::Start, args, VM_WORK).await?;
        succeeded(&started)?;
        let state = self.vm(vm.id).await?;
        check(
            state["state"] == "running"
                && state["ready"] == true
                && state["expires_at_ms"] == vm.deadline,
            "fresh_ready_boot",
        )?;
        let epoch = id(&started, "operation_id")?;
        self.ledger.release(&vm.pair, epoch, false)?;
        if matches!(guest, Guest::Windows) {
            let witness = windows_boot_witness(
                &state,
                &hex::encode(vm.id),
                &hex::encode(Ledger::generation(&vm.pair)),
                &hex::encode(epoch),
            )?;
            self.evidence.record(
                "windows_native_boot",
                json!({"vm_id":hex::encode(vm.id),
                "operation_id":hex::encode(epoch),"witness_id":witness}),
            );
        }
        self.evidence.record(
            "ready_boot",
            json!({"vm_id":hex::encode(vm.id),
            "generation":hex::encode(Ledger::generation(&vm.pair)),"epoch":hex::encode(epoch)}),
        );
        Ok(epoch)
    }

    async fn stop(&mut self, vm: &Vm, epoch: Id) -> Result<()> {
        let (stopped, _, _) = self
            .operation(Action::Stop, json!({"vm_id":hex::encode(vm.id)}), VM_WORK)
            .await?;
        succeeded(&stopped)?;
        self.stopped(vm).await?;
        self.ledger.release(&vm.pair, epoch, true)?;
        self.evidence.record(
            "clean_release",
            json!({"vm_id":hex::encode(vm.id),"epoch":hex::encode(epoch),
            "generation":hex::encode(Ledger::generation(&vm.pair))}),
        );
        Ok(())
    }

    async fn stopped(&self, vm: &Vm) -> Result<()> {
        let status = self.vm(vm.id).await?;
        check(
            status["state"] == "stopped"
                && status["ready"] == false
                && status["expires_at_ms"] == vm.deadline,
            "stopped_lease",
        )
    }

    async fn script(&mut self, vm: &Vm, script: String, exit: u64) -> Result<()> {
        let (row, _, _) = self
            .operation(
                Action::Script,
                json!({"vm_id":hex::encode(vm.id),"timeout_s":SCRIPT_TIMEOUT,"script":script}),
                Duration::from_secs(SCRIPT_TIMEOUT + 30),
            )
            .await?;
        check(
            row["cleanup"] == "confirmed"
                && row["script_status"] == "exited"
                && row["exit_code"] == exit
                && row["state"] == if exit == 0 { "succeeded" } else { "failed" },
            "checked_marker_exit",
        )
    }

    async fn expiry(&mut self, vm: &Vm, extra: Option<&Pair>, later: Option<&Vm>) -> Result<i64> {
        // Poll through the lease. Keep the two stopped VMs under observation
        // while waiting for Ubuntu so an early Windows expiry also fails.
        let cutoff = vm.lease.monotonic_deadline + EXPIRY_GRACE;
        let tombstone = timeout_at(cutoff, async {
            loop {
                vm.lease.check_clock(clock_sample()?)?;
                let status = self.vm(vm.id).await?;
                let expired = vm.lease.observe(&status, clock_sample()?)?;
                if let Some(later) = later {
                    later.lease.check_clock(clock_sample()?)?;
                    let later_status = self.vm(later.id).await?;
                    later.lease.observe(&later_status, clock_sample()?)?;
                    check(
                        later_status["state"] == "stopped",
                        "later_lease_not_stopped",
                    )?;
                }
                if expired {
                    return Ok(status);
                }
                sleep(TICK).await;
            }
        })
        .await
        .map_err(|_| Failure::Timeout("lease_sweep"))??;
        let activity = tombstone["cleanup_activity_id"]
            .as_i64()
            .filter(|n| *n > 0)
            .ok_or(Failure::Check("expiry_activity_id"))?;
        let expired = tombstone["expired_at_ms"]
            .as_i64()
            .ok_or(Failure::Check("expired_at"))?;
        let now = clock_sample()?;
        vm.lease.check_clock(now)?;
        check(
            expired >= vm.deadline
                && expired <= now.utc_ms
                && tombstone["cleanup"] == "confirmed"
                && tombstone["ready"] == false,
            "expiry_proof",
        )?;
        self.ledger
            .expiry(vm.id, self.principal, activity, vm.deadline, expired)?;
        check(self.ledger.gone(&vm.pair)?, "expired_pair_gone")?;
        if let Some(pair) = extra {
            check(self.ledger.gone(pair)?, "expired_snapshot_gone")?;
        }
        let charge = self.ledger.charge(vm.id)?;
        check(
            charge
                == (Claim {
                    active: 0,
                    snapshot: 0,
                    staged: 0,
                    generations: 0,
                }),
            "expired_charge_released",
        )?;
        self.evidence.record(
            "daemon_expiry",
            json!({"vm_id":hex::encode(vm.id),"activity_id":activity,
            "deadline_ms":vm.deadline,"expired_at_ms":expired,"charges":charge.evidence(),
            "tombstone_and_physical_cleanup_confirmed":true}),
        );
        Ok(activity)
    }
}

#[derive(Clone, Copy)]
enum Guest {
    Ubuntu,
    Windows,
}

impl Guest {
    fn label(self) -> &'static str {
        match self {
            Self::Ubuntu => "ubuntu",
            Self::Windows => "windows",
        }
    }
    fn code(self) -> i64 {
        match self {
            Self::Ubuntu => 1,
            Self::Windows => 2,
        }
    }
    fn install_seconds(self) -> u64 {
        match self {
            Self::Ubuntu => 3_660,
            Self::Windows => 10_860,
        }
    }
}

struct Golden {
    template: Id,
    pair: Pair,
    proof: PairProof,
    charge: i64,
}

struct Vm {
    id: Id,
    deadline: i64,
    lease: LeaseClock,
    pair: Pair,
}

fn selected_digest<'a>(reply: &'a Value, plan: &Plan) -> Result<&'a str> {
    assert_admitted(reply)?;
    qualification_digest(
        reply,
        plan.qualification_sha256.as_deref(),
        [
            ("ubuntu", &plan.ubuntu.recipe_id, &plan.ubuntu.recipe_sha256),
            (
                "windows",
                &plan.windows.recipe_id,
                &plan.windows.recipe_sha256,
            ),
        ],
    )
}

async fn catalog(client: &Client, plan: &Plan) -> Result<(Vec<Value>, String)> {
    let mut cursor = Value::Null;
    let mut entries = Vec::new();
    for _ in 0..4 {
        let args = if cursor.is_null() {
            json!({})
        } else {
            json!({"cursor":cursor})
        };
        let result = rpc(client, "vm_catalog", args).await?;
        let selected = selected_digest(&result, plan)?;
        check(result["arch"] == "x86_64", "catalog_arch")?;
        let page = result["entries"]
            .as_array()
            .ok_or(Failure::Check("catalog_page"))?;
        check(page.len() <= 20, "catalog_page_bound")?;
        entries.extend_from_slice(page);
        cursor = result["next_cursor"].clone();
        if cursor.is_null() {
            for (recipe, guest) in [
                (&plan.ubuntu, Guest::Ubuntu),
                (&plan.windows, Guest::Windows),
            ] {
                let matching = entries
                    .iter()
                    .filter(|entry| {
                        entry["recipe_id"] == recipe.recipe_id
                            && entry["guest"] == guest.label()
                            && entry["state"] == "buildable"
                    })
                    .count();
                if matching != 1 {
                    return Err(Failure::AdmissionClosed("recipe_unqualified"));
                }
            }
            return Ok((entries, selected.to_owned()));
        }
        check(
            cursor.as_str().is_some_and(|value| {
                !value.is_empty()
                    && value.len() <= 96
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            }),
            "catalog_cursor",
        )?;
    }
    Err(Failure::Check("catalog_page_budget"))
}

async fn journey(accept: &mut Acceptance<'_>, plan: &Plan, entries: &[Value]) -> Result<()> {
    accept.ledger.quiet()?;
    let initial = accept.ledger.goldens()?;
    let mut initial_goldens = Vec::new();
    for (recipe, guest) in [
        (&plan.ubuntu, Guest::Ubuntu),
        (&plan.windows, Guest::Windows),
    ] {
        for entry in entries.iter().filter(|entry| {
            entry["state"] == "sealed"
                && entry["recipe_id"] == recipe.recipe_id
                && entry["guest"] == guest.label()
        }) {
            let (pair, _) = accept.ledger.golden(
                id(entry, "template_id")?,
                &recipe.recipe_id,
                &recipe.recipe_sha256,
                guest.code(),
            )?;
            initial_goldens.push(pair);
        }
    }
    check(
        initial.len() == initial_goldens.len() && initial.len() <= 2,
        "isolated_golden_inventory",
    )?;
    accept
        .ledger
        .inventory(&initial_goldens.iter().collect::<Vec<_>>())?;
    accept.evidence.record(
        "initial_ledger",
        json!({"golden_templates":initial.iter().map(hex::encode).collect::<Vec<_>>(),
        "charged_bytes":accept.ledger.aggregate()?,"ledger_bytes":accept.ledger.metadata_bytes()?}),
    );
    let ubuntu = accept
        .template(entries, &plan.ubuntu, Guest::Ubuntu)
        .await?;
    let windows = accept
        .template(entries, &plan.windows, Guest::Windows)
        .await?;
    accept.ledger.distinct(&ubuntu.pair, &windows.pair)?;
    let mut u = accept
        .clone(&ubuntu, Guest::Ubuntu, UBUNTU_TTL, true)
        .await?;
    let w = accept
        .clone(&windows, Guest::Windows, WINDOWS_TTL, false)
        .await?;
    check(u.deadline < w.deadline, "lease_order")?;
    accept.ledger.distinct(&u.pair, &w.pair)?;
    let windows_charge = accept.ledger.charge(w.id)?;

    let u_marker = hex::encode(fresh()?);
    let changed = hex::encode(fresh()?);
    check(changed != u_marker, "unique_break_marker")?;
    let path = format!("/var/tmp/arxcovo-m4-{u_marker}");
    let u_first = accept.start(&u, Guest::Ubuntu).await?;
    accept.script(&u, format!(
        "set -euC\numask 077\nprintf '%s' '{u_marker}' > '{path}'\ncat /proc/sys/kernel/random/boot_id > '{path}.boot'\ntest \"$(cat '{path}')\" = '{u_marker}'"
    ), 0).await?;
    accept.stop(&u, u_first).await?;
    let baseline = accept.ledger.inspect(&u.pair, Some(&ubuntu.pair), false)?;

    let w_marker = hex::encode(fresh()?);
    let w_boot = accept.start(&w, Guest::Windows).await?;
    accept.script(&w, format!(
        "$ErrorActionPreference='Stop'\n$p=[IO.Path]::Combine($env:TEMP,'arxcovo-m4-{w_marker}')\nif ([IO.File]::Exists($p)) {{ exit 2 }}\n[IO.File]::WriteAllText($p,'{w_marker}')\nif ([IO.File]::ReadAllText($p) -cne '{w_marker}') {{ exit 3 }}\nexit 0"
    ),0).await?;
    accept.stop(&w, w_boot).await?;

    let vm = hex::encode(u.id);
    let (captured, _, _) = accept
        .operation(Action::Snapshot, json!({"vm_id":vm}), VM_WORK)
        .await?;
    succeeded(&captured)?;
    let point = id(&captured, "snapshot_id")?;
    let (snapshot, charge) =
        accept
            .ledger
            .point(u.id, point, ubuntu.template, Ledger::generation(&u.pair))?;
    let point_list = rpc(accept.client, "vm_snapshot_list", json!({"vm_id":vm})).await?;
    check(
        point_list["vm_id"] == vm
            && point_list["entries"].as_array().is_some_and(|entries| {
                entries.len() == 1
                    && entries[0]["snapshot_id"] == hex::encode(point)
                    && entries[0]["restore_eligible"] == true
                    && entries[0]["protected"] == false
                    && entries[0]["charged_bytes"] == charge
            }),
        "snapshot_list",
    )?;
    accept.ledger.distinct(&u.pair, &snapshot)?;
    let saved = accept.ledger.inspect(&snapshot, Some(&ubuntu.pair), true)?;
    check(
        saved.contents == baseline.contents && charge > 0,
        "snapshot_pair_copied",
    )?;
    accept.evidence.record("snapshot_pair", json!({"snapshot_id":hex::encode(point),
        "generation":hex::encode(Ledger::generation(&snapshot)),"charge":charge,"pair":saved.evidence()}));

    let broken_boot = accept.start(&u, Guest::Ubuntu).await?;
    check(broken_boot != u_first, "break_fresh_boot")?;
    accept.script(&u,format!(
        "set -eu\nold=$(cat '{path}.boot')\nnew=$(cat /proc/sys/kernel/random/boot_id)\ntest -n \"$old\" && test -n \"$new\" && test \"$old\" != \"$new\" || exit 2\ntest \"$(cat '{path}')\" = '{u_marker}' || exit 3\nprintf '%s' '{changed}' > '{path}'\ntest \"$(cat '{path}')\" = '{changed}'"
    ),0).await?;
    accept.script(&u,format!(
        "set -eu\nactual=$(cat '{path}')\ntest \"$actual\" = '{changed}' || exit 3\nif test \"$actual\" != '{u_marker}'; then exit 7; fi"
    ),7).await?;
    accept.stop(&u, broken_boot).await?;
    let (restored, _, _) = accept
        .operation(
            Action::Restore,
            json!({"vm_id":vm,"snapshot_id":hex::encode(point)}),
            VM_WORK,
        )
        .await?;
    check(
        restored["state"] == "succeeded"
            && restored["snapshot_id"] == hex::encode(point)
            && restored["cleanup"] == "cleanup_unconfirmed",
        "restore_commit",
    )?;
    accept.stopped(&u).await?;
    let old = std::mem::replace(&mut u.pair, accept.ledger.vm_pair(u.id)?);
    accept.ledger.distinct(&old, &u.pair)?;
    accept.ledger.distinct(&snapshot, &u.pair)?;
    let current = accept.ledger.inspect(&u.pair, Some(&ubuntu.pair), false)?;
    check(
        current.contents == saved.contents,
        "restored_pair_matches_snapshot",
    )?;
    let restore_op = id(&restored, "operation_id")?;
    let retired = poll("retired_generation_sweep", EXPIRY_GRACE, TICK, || async {
        if !accept.ledger.gone(&old)? {
            return Ok(None);
        }
        let claim = accept.ledger.charge(u.id)?;
        check(
            claim.active > 0
                && claim.snapshot == charge
                && claim.staged == 0
                && claim.generations == 2,
            "retired_charge_released",
        )?;
        Ok(Some(accept.ledger.reconciled(
            u.id,
            Ledger::generation(&old),
            "retired",
            Some(restore_op),
            None,
        )?))
    })
    .await?;
    accept.evidence.record(
        "restore_pair",
        json!({"old_generation":hex::encode(Ledger::generation(&old)),
        "new_generation":hex::encode(Ledger::generation(&u.pair)),"retirement_activity_id":retired,
        "snapshot_id":hex::encode(point),"pair":current.evidence()}),
    );

    let recovered_boot = accept.start(&u, Guest::Ubuntu).await?;
    check(
        recovered_boot != broken_boot && recovered_boot != u_first,
        "restore_fresh_boot",
    )?;
    accept.script(&u,format!(
        "set -eu\nold=$(cat '{path}.boot')\nnew=$(cat /proc/sys/kernel/random/boot_id)\ntest -n \"$old\" && test -n \"$new\" && test \"$old\" != \"$new\" || exit 2\ntest \"$(cat '{path}')\" = '{u_marker}'"
    ),0).await?;
    accept.stop(&u, recovered_boot).await?;

    accept.stopped(&w).await?;
    let u_expiry = accept.expiry(&u, Some(&snapshot), Some(&w)).await?;
    let snapshot_cleanup = accept.ledger.reconciled(
        u.id,
        Ledger::generation(&snapshot),
        "expiry_point",
        None,
        Some(u_expiry),
    )?;
    accept.evidence.record(
        "snapshot_expired",
        json!({"snapshot_id":hex::encode(point),
        "reconciliation_activity_id":snapshot_cleanup,"expiry_activity_id":u_expiry}),
    );
    accept.stopped(&w).await?;
    check(
        windows_charge == accept.ledger.charge(w.id)? && windows_charge.active > 0,
        "windows_separately_charged",
    )?;
    accept.ledger.inspect(&w.pair, Some(&windows.pair), false)?;
    accept
        .evidence
        .record("windows_still_charged", windows_charge.evidence());
    accept.expiry(&w, None, None).await?;

    for (golden, recipe, guest) in [
        (&ubuntu, &plan.ubuntu, Guest::Ubuntu),
        (&windows, &plan.windows, Guest::Windows),
    ] {
        let (after, charge) = accept.ledger.golden(
            golden.template,
            &recipe.recipe_id,
            &recipe.recipe_sha256,
            guest.code(),
        )?;
        check(
            after == golden.pair
                && charge == golden.charge
                && accept.ledger.inspect(&after, None, true)? == golden.proof,
            "golden_unchanged",
        )?;
    }
    accept.ledger.quiet()?;
    accept.ledger.inventory(&[&ubuntu.pair, &windows.pair])?;
    let remaining = accept.ledger.aggregate()?;
    check(
        remaining == ubuntu.charge + windows.charge,
        "remaining_charges",
    )?;
    accept.evidence.record("final_ledger",json!({"goldens_unchanged":true,"golden_charged_bytes":remaining,
        "golden_allocated_bytes":ubuntu.proof.allocated.iter().chain(windows.proof.allocated.iter()).sum::<u64>(),
        "non_golden_charged_bytes":0,"unattributed_media":0,"ledger_bytes":accept.ledger.metadata_bytes()?}));
    accept.correlate().await
}

/// A test-owned console lets Ctrl+C reach the foreground's public drain
/// handler without broadcasting it to the owner's shell or other processes.
struct Console;

unsafe extern "system" fn keep_client_alive(event: u32) -> i32 {
    i32::from(event == CTRL_C_EVENT)
}

impl Console {
    fn allocate() -> Result<Self> {
        // Detach only this test process from its inherited console.
        unsafe { FreeConsole() };
        check(unsafe { AllocConsole() } != 0, "private_console")?;
        let console = Self;
        check(
            unsafe { SetConsoleCtrlHandler(Some(keep_client_alive), 1) } != 0,
            "private_console_handler",
        )?;
        Ok(console)
    }

    async fn drain(&self, daemon: &mut Child) -> Result<()> {
        let daemon_id = daemon
            .id()
            .ok_or(Failure::Check("foreground_already_exited"))?;
        check(
            safe(daemon.try_wait(), "foreground_status")?.is_none(),
            "foreground_already_exited",
        )?;
        let mut ids = [0; 3];
        let count = unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) };
        check(
            count == 2 && ids[..2].contains(&daemon_id) && ids[..2].contains(&std::process::id()),
            "foreground_console_ownership",
        )?;
        check(
            unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) } != 0,
            "foreground_interrupt",
        )?;
        let waited = timeout(Duration::from_secs(40), daemon.wait())
            .await
            .map_err(|_| Failure::Timeout("foreground_drain"))?;
        check(
            safe(waited, "foreground_wait")?.success(),
            "foreground_drain_unconfirmed",
        )
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        unsafe { FreeConsole() };
    }
}

fn spawn_daemon(plan: &Plan) -> Result<Child> {
    safe(
        Command::new(&plan.daemon)
            .arg("--config")
            .arg(&plan.daemon_config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn(),
        "foreground_spawn",
    )
}

fn pipe_name(endpoint: &str) -> Vec<u16> {
    endpoint.encode_utf16().chain(Some(0)).collect()
}

fn require_unused(endpoint: &str) -> Result<()> {
    let name = pipe_name(endpoint);
    let absent =
        unsafe { WaitNamedPipeW(name.as_ptr(), 1) == 0 && GetLastError() == ERROR_FILE_NOT_FOUND };
    check(absent, "foreground_endpoint_already_in_use")
}

async fn ready(daemon: &mut Child, endpoint: &str) -> Result<()> {
    let name = pipe_name(endpoint);
    timeout(RPC, async {
        loop {
            check(
                safe(daemon.try_wait(), "foreground_status")?.is_none(),
                "foreground_startup",
            )?;
            if unsafe { WaitNamedPipeW(name.as_ptr(), 1) } != 0 {
                return Ok(());
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| Failure::Timeout("foreground_listener"))?
}

async fn open_bound_bridge(_plan: &Plan, expected_pid: u32) -> Result<(Child, Client)> {
    // The stdio bridge exposes no kernel peer PID from its own pipe handle.
    // A future adapter must check that handle BEFORE sending the credential
    // and retain the same connection through MCP initialization and calls.
    bind_session_process(expected_pid, None)?;
    Err(Failure::SessionProcessEvidenceUnavailable)
}

async fn with_client(
    plan: &Plan,
    foreground: &Foreground,
    expected_pid: u32,
    evidence: &mut Evidence,
) -> Result<()> {
    let (mut bridge, client) = open_bound_bridge(plan, expected_pid).await?;
    let observed = async {
        let discovered = timeout(RPC, client.list_tools(None))
            .await
            .map_err(|_| Failure::Discovery)?
            .map_err(|_| Failure::Discovery)
            .and_then(|reply| serde_json::to_value(reply).map_err(|_| Failure::Discovery));
        discover(discovered)?;
        // Preflight while idle delay is still running. Closed admission returns
        // its own reason immediately without touching media or waiting for WTS.
        let first = rpc(&client, "vm_catalog", json!({})).await?;
        selected_digest(&first, plan)?;
        sleep(Duration::from_secs(plan.idle_delay_s)).await;
        let (entries, selected) = catalog(&client, plan).await?;
        evidence.qualification_sha256 = Some(selected);
        let ledger = Ledger::open(&foreground.state_dir)?;
        let mut acceptance = Acceptance {
            client: &client,
            ledger,
            evidence,
            principal: &foreground.agents[0].principal,
            operations: Vec::new(),
        };
        journey(&mut acceptance, plan, &entries).await
    }
    .await;
    let cancelled = timeout(RPC, client.cancel())
        .await
        .map_err(|_| Failure::Timeout("bridge_cancel"))
        .and_then(|result| safe(result, "bridge_cancel"));
    // End any remaining bridge process before sending Ctrl+C to the console.
    // This process holds the client transport only.
    let joined = timeout(RPC, bridge.wait())
        .await
        .map_err(|_| Failure::Timeout("bridge_exit"))
        .and_then(|result| safe(result, "bridge_exit"))
        .and_then(|status| check(status.success(), "bridge_exit"));
    if joined.is_err() {
        let _ = timeout(RPC, bridge.kill()).await;
    }
    observed.and(cancelled.map(|_| ())).and(joined)
}

fn verify_hex(value: &str, length: usize, name: &'static str) -> Result<()> {
    check(opaque(&json!(value), length).is_some(), name)
}

fn local_path(path: &Path) -> Result<LocalPath> {
    let raw = path.to_str().ok_or(Failure::Check("local_path"))?;
    safe(LocalPath::parse(raw), "local_path")
}

fn d_path(path: &Path) -> Result<()> {
    let local = local_path(path)?;
    check(
        !local.is_drive_root() && local.as_str()[..2].eq_ignore_ascii_case("D:"),
        "d_drive_path",
    )
}

fn custody_root(state: &Path, runtime: Option<&Path>) -> Result<LocalPath> {
    let state = local_path(state)?;
    if let Some(root) = runtime {
        check(
            local_path(root)?
                .as_str()
                .eq_ignore_ascii_case(state.as_str()),
            "foreground_custody_root",
        )?;
    }
    Ok(state)
}

#[test]
fn refuses_runtime_custody_root_outside_foreground_store() {
    let state = Path::new("D:/acceptance/state");
    let same = Path::new("d:/acceptance/STATE");
    let other = Path::new("D:/acceptance/other");
    assert_eq!(custody_root(state, Some(same)), custody_root(state, None));
    assert_eq!(
        custody_root(state, Some(other)),
        Err(Failure::Check("foreground_custody_root"))
    );
}

fn pin_digest(path: &Path, expected: &str, max: u64) -> Result<File> {
    verify_hex(expected, 64, "digest_pin")?;
    let file = open(path, false, FILE_SHARE_READ)?;
    check(hex::encode(digest(&file, max)?.1) == expected, "digest_pin")?;
    Ok(file)
}

async fn run() -> Result<()> {
    let plan_path = PathBuf::from(
        env::var_os("ARX_M4_ACCEPTANCE_PLAN").ok_or(Failure::Check("acceptance_plan_required"))?,
    );
    d_path(&plan_path)?;
    let _plan_path = pin_root(plan_path.parent().ok_or(Failure::Check("plan_parent"))?)?;
    let plan: Plan = config(&open(&plan_path, false, FILE_SHARE_READ)?)?;
    check(plan.idle_delay_s <= 900, "idle_delay_bound")?;
    for recipe in [&plan.ubuntu, &plan.windows] {
        verify_hex(&recipe.recipe_id, 32, "recipe_id")?;
        verify_hex(&recipe.recipe_sha256, 64, "recipe_sha256")?;
    }
    check(
        plan.ubuntu.recipe_id != plan.windows.recipe_id
            && plan.ubuntu.recipe_id != "ffffffffffffffffffffffffffffffff"
            && plan.windows.recipe_id == "3c2fe2069942ce6c6cebe5c3e700e758",
        "test_recipe_ids",
    )?;
    let mut pins = Vec::new();
    for path in [&plan.daemon, &plan.daemon_config, &plan.client_config] {
        d_path(path)?;
        pins.extend(pin_root(
            path.parent().ok_or(Failure::Check("input_parent"))?,
        )?);
    }
    d_path(&plan.evidence_dir)?;
    pins.extend(pin_root(&plan.evidence_dir)?);
    let _daemon = pin_digest(&plan.daemon, &plan.daemon_sha256, 512 * 1024 * 1024)?;
    if let Some(pin) = &plan.qualification_sha256 {
        verify_hex(pin, 64, "qualification_pin")?;
    }
    let daemon_config = open(&plan.daemon_config, false, FILE_SHARE_READ)?;
    let client_config = open(&plan.client_config, false, FILE_SHARE_READ)?;
    let foreground: Foreground = config(&daemon_config)?;
    let bridge: BridgeConfig = config(&client_config)?;
    check(
        bridge.address == foreground.local.address,
        "bridge_endpoint_binding",
    )?;
    check(
        foreground.agents.len() == 1
            && !foreground.test_vms.allow_agent_public_sites
            && foreground.test_vms.publication.is_none(),
        "isolated_foreground_profile",
    )?;
    d_path(&foreground.state_dir)?;
    pins.extend(pin_root(&foreground.state_dir)?);
    let state = custody_root(
        &foreground.state_dir,
        foreground
            .test_vms
            .runtime
            .as_ref()
            .map(|runtime| runtime.state.as_path()),
    )?;
    let evidence_root = local_path(&plan.evidence_dir)?;
    check(!evidence_root.overlaps(&state), "isolated_evidence_root")?;
    safe(foreground.local.validate(), "foreground_endpoint")?;
    let executable = safe(env::current_exe(), "client_executable")?;
    let client_file = open(&executable, false, FILE_SHARE_READ)?;
    let run_id = hex::encode(fresh()?);
    let mut evidence = Evidence {
        version: 1,
        run_id: run_id.clone(),
        client_sha256: hex::encode(digest(&client_file, 512 * 1024 * 1024)?.1),
        daemon_sha256: plan.daemon_sha256.clone(),
        qualification_sha256: None,
        started_at_ms: now_ms()?,
        finished_at_ms: 0,
        observations: Vec::new(),
        drain: false,
        failure: None,
        drain_failure: None,
    };

    let result = async {
        require_unused(&foreground.local.address)?;
        Ledger::preflight(
            &foreground.state_dir,
            [
                (&plan.ubuntu.recipe_id, &plan.ubuntu.recipe_sha256),
                (&plan.windows.recipe_id, &plan.windows.recipe_sha256),
            ],
        )?;
        let console = Console::allocate()?;
        let mut daemon = spawn_daemon(&plan)?;
        let observed = async {
            ready(&mut daemon, &foreground.local.address).await?;
            let expected_pid = daemon
                .id()
                .ok_or(Failure::SessionProcessEvidenceUnavailable)?;
            with_client(&plan, &foreground, expected_pid, &mut evidence).await
        }
        .await;
        let drained = console.drain(&mut daemon).await;
        evidence.drain = drained.is_ok();
        evidence.drain_failure = drained.clone().err();
        observed.and(drained)
    }
    .await;

    evidence.finished_at_ms = now_ms()?;
    evidence.failure = result.clone().err();
    evidence.write(&plan.evidence_dir)?;
    println!("M4 acceptance run {run_id}");
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "owner-host acceptance requires independently reviewed native qualification"]
async fn owner_host_m4() {
    if let Err(error) = run().await {
        panic!("M4 acceptance {error:?}");
    }
}

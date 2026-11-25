use super::*;
use arxd::{
    config::{ExpectedVmStore, pinned_vm_config, with_enrolled_vm_store},
    local::{
        ClientConfig, ForegroundBridgeError, LoadedClient, RESPONSE_LIMIT, bind_foreground_bridge,
    },
    vm::paths::LocalPath,
};
use ledger::{Id, Ledger, LiveLedger, config, digest, id, open, pin_root, safe, store_identity};
use rmcp::{
    RoleClient, ServiceExt, model::CallToolRequestParams, service::RunningService,
    transport::async_rw::AsyncRwTransport,
};
use serde::Deserialize;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    process::{Child, Command},
    task::JoinSet,
    time::timeout,
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, GetLastError},
    Storage::FileSystem::{
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFinalPathNameByHandleW,
        GetVolumeNameForVolumeMountPointW, VOLUME_NAME_GUID,
    },
    System::{Console::*, Pipes::WaitNamedPipeW},
};

mod ledger;

#[cfg(test)]
#[path = "../vault_fixtures/private.rs"]
mod private_fixture;

const RPC: Duration = Duration::from_secs(15);
const TICK: Duration = Duration::from_secs(2);
const VM_WORK: Duration = Duration::from_secs(600);
const COMPOSE_WORK: Duration = Duration::from_secs(900);
const BACKUP_WORK: Duration = Duration::from_secs(660);
const SCRIPT_TIMEOUT: u64 = 100;

#[derive(Deserialize)]
struct Foreground {
    state_dir: PathBuf,
    local: arxd::local::Endpoint,
    agents: Vec<arxd::config::Agent>,
    test_vms: VmProfile,
}

#[derive(Deserialize)]
struct VmProfile {
    #[serde(default)]
    allow_agent_public_sites: bool,
    publication: Option<Value>,
    runtime: Option<RuntimeRoot>,
    compose_secret_root: Option<PathBuf>,
}

#[derive(Deserialize)]
struct RuntimeRoot {
    state: PathBuf,
    volume: PathBuf,
}

#[derive(Serialize)]
struct Evidence {
    version: u8,
    run_id: String,
    client_sha256: String,
    daemon_sha256: String,
    qualification_sha256: Option<String>,
    cloudflare_publishing_disabled: bool,
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
        safe(
            arxd::config::checked_owner_private_path(&directory),
            "evidence_permissions",
        )?;
        let body = safe(serde_json::to_vec_pretty(self), "evidence_encode")?;
        write_new(&directory.join("acceptance.json"), &body)?;
        let manifest = json!({
            "version": 1,
            "files": [{
                "name": "acceptance.json",
                "bytes": body.len(),
                "sha256": hex::encode(Sha256::digest(&body))
            }]
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
    let mut out = [0; 16];
    safe(getrandom::fill(&mut out), "random_id")?;
    check(out != [0; 16], "random_id")?;
    Ok(out)
}

fn request_key() -> Result<Id> {
    let mut key = fresh()?;
    key[..8].copy_from_slice(&now_ms()?.to_be_bytes());
    Ok(key)
}

fn now_ms() -> Result<i64> {
    let duration = safe(SystemTime::now().duration_since(UNIX_EPOCH), "utc_clock")?;
    safe(i64::try_from(duration.as_millis()), "utc_clock")
}

type Client = RunningService<RoleClient, ()>;

async fn rpc_raw(client: &Client, name: &'static str, args: Value) -> Result<Value> {
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
    check(
        row["ok"]
            .as_bool()
            .is_some_and(|ok| result.is_error == Some(!ok)),
        name,
    )?;
    Ok(row)
}

async fn rpc(client: &Client, name: &'static str, args: Value) -> Result<Value> {
    let row = rpc_raw(client, name, args).await?;
    if row["ok"] == false {
        return Err(reason(&row["reason"]).map_or(Failure::Check(name), Failure::AdmissionClosed));
    }
    check(row["ok"] == true, name)?;
    Ok(row)
}

async fn terminal(client: &Client, operation: Id, budget: Duration) -> Result<Value> {
    poll("vm_operation", budget, TICK, || async {
        let row = rpc(
            client,
            "vm_operation",
            json!({"operation_id": hex::encode(operation)}),
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

fn succeeded(row: &Value) -> Result<()> {
    check(
        row["state"] == "succeeded" && row["cleanup"] == "confirmed",
        "operation_not_clean",
    )
}

struct RecordedOp {
    request: Id,
    operation: Id,
    action_code: i64,
    action_label: &'static str,
    vm_id: Option<String>,
    state: String,
}

struct Acceptance<'a> {
    client: &'a Client,
    ledger: LiveLedger,
    evidence: &'a mut Evidence,
    principal: &'a str,
    operations: Vec<RecordedOp>,
}

impl Acceptance<'_> {
    async fn vm_op(
        &mut self,
        tool: &'static str,
        action_label: &'static str,
        action_code: i64,
        mut args: Value,
        budget: Duration,
    ) -> Result<(Value, Value, Value)> {
        let request = request_key()?;
        args["request_id"] = json!(hex::encode(request));
        let received = rpc(self.client, tool, args.clone()).await?;
        let operation = id(&received, "operation_id")?;
        self.evidence.record(
            "operation_admitted",
            json!({
                "action": action_label,
                "request_id": hex::encode(request),
                "receipt": operation_evidence(&received),
            }),
        );
        let result = terminal(self.client, operation, budget).await?;
        self.evidence
            .record("operation_terminal", operation_evidence(&result));
        check(
            result["action"] == action_label
                && (args["vm_id"].is_null() || result["vm_id"] == args["vm_id"]),
            "operation_subject",
        )?;
        self.operations.push(RecordedOp {
            request,
            operation,
            action_code,
            action_label,
            vm_id: result["vm_id"].as_str().map(str::to_owned),
            state: result["state"].as_str().unwrap_or_default().to_owned(),
        });
        Ok((result, received, args))
    }

    async fn deploy_and_wait(
        &mut self,
        vm_id: &str,
        project_id: &str,
        release_id: &str,
    ) -> Result<Value> {
        let request = request_key()?;
        let args = json!({
            "vm_id": vm_id,
            "project_id": project_id,
            "release_id": release_id,
            "request_id": hex::encode(request),
        });
        let admitted = rpc(self.client, "compose_deploy", args.clone()).await?;
        self.evidence
            .record("compose_deploy_admitted", operation_evidence(&admitted));
        // Same-key replay must return the same operation ID without creating a second op.
        id(&admitted, "operation_id")?;
        let replay = rpc(self.client, "compose_deploy", args).await?;
        check(
            replay["operation_id"] == admitted["operation_id"],
            "compose_deploy_replay",
        )?;
        let status = poll("compose_deploy", COMPOSE_WORK, TICK, || async {
            let status = rpc(
                self.client,
                "compose_status",
                json!({"vm_id": vm_id, "project_id": project_id}),
            )
            .await?;
            if status["operation_id"].is_null() {
                return Ok(Some(status));
            }
            Ok(None)
        })
        .await?;
        check(
            status["desired"] == "started"
                && status["current_boot"] == true
                && status["recovery"].is_null()
                && status["revision"].as_u64().is_some_and(|r| r > 0),
            "compose_deployed_ready",
        )?;
        self.evidence
            .record("compose_status", compose_status_evidence(&status));
        let logs = rpc(
            self.client,
            "compose_logs",
            json!({"vm_id": vm_id, "project_id": project_id, "limit": MAX_LOG_ENTRIES}),
        )
        .await?;
        let entries = logs["entries"]
            .as_array()
            .ok_or(Failure::Check("compose_logs_array"))?;
        check(
            entries.len() <= MAX_LOG_ENTRIES as usize,
            "compose_logs_bound",
        )?;
        self.evidence.record(
            "compose_logs_checked",
            json!({"entry_count": entries.len()}),
        );
        Ok(status)
    }

    async fn create_backup(&mut self, vm_id: &str, project_id: &str) -> Result<(Id, u64, String)> {
        let request = request_key()?;
        let args = json!({
            "vm_id": vm_id,
            "project_id": project_id,
            "request_id": hex::encode(request),
        });
        let settled = poll("compose_backup", BACKUP_WORK, TICK, || async {
            let reply = rpc(self.client, "compose_backup", args.clone()).await?;
            match reply["state"].as_str() {
                Some("pending") => Ok(None),
                Some("succeeded") => Ok(Some(reply)),
                _ => Err(Failure::Check("compose_backup_failed")),
            }
        })
        .await?;
        let backup_id = id(&settled, "backup_id")?;
        let size_bytes = settled["size_bytes"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or(Failure::Check("backup_size_bytes"))?;
        let sha256 = opaque(&settled["sha256"], 64)
            .ok_or(Failure::Check("backup_sha256"))?
            .to_owned();
        self.evidence
            .record("compose_backup_settled", operation_evidence(&settled));
        Ok((backup_id, size_bytes, sha256))
    }

    async fn verify_delete_refusals(&mut self, vm_id: &str, backup_id: Id) -> Result<()> {
        let probes: [(&'static str, Value); 11] = [
            ("static_vm_delete", json!({"vm_id": vm_id})),
            ("vm_delete", json!({"vm_id": vm_id})),
            (
                "backup_delete",
                json!({"backup_id": hex::encode(backup_id)}),
            ),
            (
                "vm_backup_delete",
                json!({"backup_id": hex::encode(backup_id)}),
            ),
            (
                "compose_backup_delete",
                json!({"backup_id": hex::encode(backup_id)}),
            ),
            ("compose_volume_delete", json!({"vm_id": vm_id})),
            ("compose_project_delete", json!({"vm_id": vm_id})),
            (
                "compose_backup_restore",
                json!({"backup_id": hex::encode(backup_id)}),
            ),
            (
                "vm_snapshot_delete",
                json!({
                    "vm_id": vm_id,
                    "snapshot_id": hex::encode(backup_id),
                    "request_id": hex::encode(request_key()?),
                }),
            ),
            ("run_command", json!({})),
            ("discover_tools", json!({})),
        ];
        for (tool, args) in probes {
            let expected = expected_delete_refusal(tool)?;
            let reply = rpc_raw(self.client, tool, args).await?;
            let actual = assert_refused(&reply, expected)?;
            self.evidence.record(
                "agent_refusal_verified",
                json!({"probe": tool, "reason": actual}),
            );
        }
        Ok(())
    }

    async fn correlate(&mut self) -> Result<()> {
        for op in &self.operations {
            let activity_id = self.ledger.read(|ledger| {
                ledger.activity_operation(op.operation, op.request, self.principal, op.action_code)
            })?;
            let read = rpc(
                self.client,
                "list_activity",
                json!({"activity_id": activity_id}),
            )
            .await?;
            let rows = read["activities"]
                .as_array()
                .ok_or(Failure::Check("activity_list"))?;
            check(
                rows.len() == 1 && rows[0]["activity_id"] == activity_id,
                "activity_id",
            )?;
            let row = &rows[0];
            check(
                row["identity_mode"] == "local_vm_principal"
                    && row["actor"] == "agent"
                    && row["action"] == op.action_label
                    && row["operation_id"] == hex::encode(op.operation)
                    && row["state"] == op.state
                    && op.vm_id.as_deref().is_none_or(|v| row["vm_id"] == v),
                "mcp_activity_correlation",
            )?;
            self.evidence.record(
                "activity_correlated",
                json!({
                    "activity_id": activity_id,
                    "operation_id": hex::encode(op.operation),
                    "caller": "local_vm_principal",
                }),
            );
        }
        // Confirm refused publish_site (action 15, reason 45 = public_sites_disabled) was audited.
        let pub_refusals = self
            .ledger
            .read(|ledger| ledger.activity_refusal_count(self.principal, 15, 45))?;
        check(pub_refusals >= 1, "publish_refusal_audited")?;
        Ok(())
    }
}

fn read_origin_ca_der(witness: &StaticOriginWitness, path: &Path) -> Result<Vec<u8>> {
    d_path(path)?;
    let _parents = pin_root(path.parent().ok_or(Failure::Check("origin_ca_parent"))?)?;
    let file = open(path, false, FILE_SHARE_READ)?;
    let mut bytes = Vec::new();
    safe(
        std::io::Read::read_to_end(&mut file.take(65_537), &mut bytes),
        "origin_ca_read",
    )?;
    check(
        !bytes.is_empty() && bytes.len() <= 65_536,
        "origin_ca_bound",
    )?;
    check(
        hex::encode(Sha256::digest(&bytes)) == witness.origin_ca_sha256,
        "origin_ca_sha256",
    )?;
    Ok(bytes)
}

fn read_admin_bearer(path: Option<&Path>) -> Result<Option<String>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let file = open(path, false, FILE_SHARE_READ)?;
    safe(
        arxd::config::checked_owner_private_path(path),
        "admin_permissions",
    )?;
    let mut raw = String::new();
    safe(
        std::io::Read::read_to_string(&mut file.take(4097), &mut raw),
        "admin_password_read",
    )?;
    let trimmed = raw.trim().to_owned();
    check(
        raw.len() <= 4096
            && !trimmed.is_empty()
            && trimmed.bytes().all(|byte| byte.is_ascii_graphic()),
        "admin_password_bound",
    )?;
    Ok(Some(trimmed))
}

fn build_egress_probe_script(spec: &PublicEgressSpec) -> String {
    let mut lines = vec![
        "set -eu".to_owned(),
        "test \"$(cat /proc/sys/net/ipv6/conf/all/disable_ipv6)\" = '1'".to_owned(),
    ];
    lines.push("test \"$(id -u)\" = 0".into());
    let tcp = parse_ipv4_endpoint(&spec.public_tcp_target).expect("validated public tcp");
    let dns = parse_ipv4_endpoint(&spec.public_dns_target).expect("validated public dns");
    lines.push(format!(
        "timeout 5 bash -c 'exec 3<>/dev/tcp/{}/{}' >/dev/null 2>&1",
        tcp.ip(),
        tcp.port()
    ));
    lines.push(format!(
        r#"dig +notcp +ignore +short +time=3 +tries=1 @{} -p {} example.org A | grep -Eq '^[0-9]+([.][0-9]+){{3}}$'"#,
        dns.ip(),
        dns.port()
    ));
    for raw in &spec.denied_targets {
        let ep = parse_ipv4_endpoint(raw).expect("validated denied target");
        lines.push(format!(
            "if timeout 2 bash -c 'exec 3<>/dev/tcp/{}/{}' >/dev/null 2>&1; then exit 41; fi",
            ep.ip(),
            ep.port()
        ));
    }
    lines.push("exit 0".to_owned());
    lines.join("\n")
}

async fn catalog(client: &Client, plan: &StaticPlan) -> Result<(Vec<Value>, String)> {
    let mut cursor = Value::Null;
    let mut entries = Vec::new();
    for _ in 0..4 {
        let args = if cursor.is_null() {
            json!({})
        } else {
            json!({"cursor": cursor})
        };
        let result = rpc(client, "vm_catalog", args).await?;
        assert_admitted(&result)?;
        let selected = static_qualification_digest(&result, plan)?;
        check(result["arch"] == "x86_64", "catalog_arch")?;
        let page = result["entries"]
            .as_array()
            .ok_or(Failure::Check("catalog_page"))?;
        check(page.len() <= 20, "catalog_page_bound")?;
        entries.extend_from_slice(page);
        cursor = result["next_cursor"].clone();
        if cursor.is_null() {
            let matching = entries
                .iter()
                .filter(|entry| {
                    entry["recipe_id"] == plan.static_vm.recipe_id
                        && entry["guest"] == "ubuntu"
                        && matches!(entry["state"].as_str(), Some("buildable" | "sealed"))
                })
                .count();
            if matching == 0 {
                return Err(Failure::AdmissionClosed("recipe_unqualified"));
            }
            return Ok((entries, selected.to_owned()));
        }
    }
    Err(Failure::Check("catalog_page_budget"))
}

async fn journey(accept: &mut Acceptance<'_>, plan: &StaticPlan, entries: &[Value]) -> Result<()> {
    // 1. Select or build the qualified Ubuntu Compose golden template.
    let template_id = if let Some(sealed) = entries.iter().find(|entry| {
        entry["guest"] == "ubuntu"
            && entry["recipe_id"] == plan.static_vm.recipe_id
            && entry["state"] == "sealed"
    }) {
        id(sealed, "template_id")?
    } else {
        let (built, _, _) = accept
            .vm_op(
                "vm_template_build",
                "template_build",
                11,
                json!({"recipe_id": plan.static_vm.recipe_id}),
                Duration::from_secs(3_660),
            )
            .await?;
        succeeded(&built)?;
        id(&built, "template_id")?
    };

    // 2. Create a fresh named static Ubuntu Compose VM and prove
    //    idempotent replay + non-expiring static provenance.
    let listed = rpc(accept.client, "static_vm_list", json!({})).await?;
    check(
        listed["entries"].as_array().is_some_and(Vec::is_empty) && listed["next_cursor"].is_null(),
        "isolated_static_catalog",
    )?;
    let static_vm_id = {
        let (created, first_receipt, create_args) = accept
            .vm_op(
                "static_vm_create",
                "static_create",
                12,
                json!({
                    "template_id": hex::encode(template_id),
                    "name": plan.static_vm.name,
                }),
                VM_WORK,
            )
            .await?;
        succeeded(&created)?;
        let replay = rpc(accept.client, "static_vm_create", create_args).await?;
        check(
            replay["operation_id"] == first_receipt["operation_id"],
            "static_create_replay",
        )?;
        id(&created, "vm_id")?
    };
    let vm_hex = hex::encode(static_vm_id);
    let vm_ledger = accept
        .ledger
        .read(|ledger| ledger.static_vm(static_vm_id, &plan.static_vm.name, accept.principal))?;
    let gen_hex = hex::encode(vm_ledger.generation);
    accept.evidence.record(
        "static_vm_ready_for_boot",
        json!({
            "vm_id": vm_hex,
            "generation": gen_hex,
            "persistent_charge": vm_ledger.persistent_charge,
        }),
    );

    // 3. Start static VM and verify static origin witness + public-egress policy pin.
    let (started, _, _) = accept
        .vm_op("vm_start", "start", 3, json!({"vm_id": vm_hex}), VM_WORK)
        .await?;
    succeeded(&started)?;
    let first_start_op = hex::encode(id(&started, "operation_id")?);
    let static_status = rpc(accept.client, "static_vm_get", json!({"vm_id": vm_hex})).await?;
    check(
        static_status["vm_id"] == vm_hex
            && static_status["provenance"]["template_id"] == hex::encode(template_id)
            && static_status["provenance"]["recipe_digest"] == plan.static_vm.recipe_sha256,
        "static_provenance",
    )?;
    check(
        static_status["state"] == "running"
            && static_status["ready"] == true
            && static_status["web_health"] == "healthy"
            && static_status["kind"] == "static",
        "static_vm_running",
    )?;
    let origin = static_origin_witness(
        &static_status,
        &vm_hex,
        &gen_hex,
        &first_start_op,
        &plan.public_egress.policy_sha256,
    )?;
    let ca_der = read_origin_ca_der(&origin, &plan.origin_ca_path)?;

    // 4. Verify public publishing stays OFF by default (ADR 0058 / ADR 0064).
    let pub_attempt = rpc_raw(
        accept.client,
        "publish_site",
        json!({"vm_id": vm_hex, "request_id": hex::encode(request_key()?)}),
    )
    .await?;
    assert_refused(&pub_attempt, "public_sites_disabled")?;
    let pub_status = rpc(accept.client, "publish_status", json!({"vm_id": vm_hex})).await?;
    check(
        pub_status["phase"] == "unpublished"
            && pub_status["host_count"] == 0
            && pub_status["hosts"].as_array().is_some_and(Vec::is_empty),
        "publish_status_unpublished",
    )?;
    accept.ledger.read(Ledger::publication_empty)?;
    accept.evidence.record(
        "public_publishing_default_off",
        publish_status_evidence(&pub_status),
    );

    // 5. Deploy primary (Calm Life) and secondary (synthetic second service).
    accept
        .deploy_and_wait(
            &vm_hex,
            &plan.compose.primary.project_id,
            &plan.compose.primary.release_id,
        )
        .await?;
    accept
        .deploy_and_wait(
            &vm_hex,
            &plan.compose.secondary.project_id,
            &plan.compose.secondary.release_id,
        )
        .await?;

    // 6. Verify direct TLS origin serves both configured names, rejects unknown Host,
    //    serves exact HTTP 206/416/If-Range byte ranges, and ingests a SQLite event.
    let primary_resp = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.primary.marker_path,
        &plan.compose.primary.hostname,
        &[],
        None,
    )
    .await?;
    let secondary_resp = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.secondary.marker_path,
        &plan.compose.secondary.hostname,
        &[],
        None,
    )
    .await?;
    let unknown_resp = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.primary.marker_path,
        "unknown-host.invalid",
        &[],
        None,
    )
    .await?;
    verify_two_host_responses(
        &primary_resp,
        &plan.compose.primary.expected_marker,
        &secondary_resp,
        &plan.compose.secondary.expected_marker,
        &unknown_resp,
    )?;
    accept.evidence.record(
        "two_configured_hosts_verified",
        json!({
            "primary_status": primary_resp.status,
            "secondary_status": secondary_resp.status,
            "unknown_host_status": unknown_resp.status,
            "distinct_bodies": true,
        }),
    );

    let full_media = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.primary.media_path,
        &plan.compose.primary.hostname,
        &[],
        None,
    )
    .await?;
    let range_hdr = format!(
        "bytes={}-{}",
        plan.compose.primary.range_start, plan.compose.primary.range_end
    );
    let partial_media = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.primary.media_path,
        &plan.compose.primary.hostname,
        &[("Range", &range_hdr)],
        None,
    )
    .await?;
    verify_byte_range_206(
        &partial_media,
        &full_media,
        plan.compose.primary.range_start,
        plan.compose.primary.range_end,
    )?;
    let bad_range_hdr = format!(
        "bytes={}-{}",
        full_media.body.len(),
        full_media.body.len() + 15
    );
    let unsatisfiable_media = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.primary.media_path,
        &plan.compose.primary.hostname,
        &[("Range", &bad_range_hdr)],
        None,
    )
    .await?;
    verify_byte_range_416(&unsatisfiable_media, full_media.body.len())?;
    if let Some(etag) = full_media.header("etag") {
        let hit = https_origin_request(
            origin.origin_port,
            &origin.origin_server_name,
            &ca_der,
            "GET",
            &plan.compose.primary.media_path,
            &plan.compose.primary.hostname,
            &[("Range", &range_hdr), ("If-Range", etag)],
            None,
        )
        .await?;
        let miss = https_origin_request(
            origin.origin_port,
            &origin.origin_server_name,
            &ca_der,
            "GET",
            &plan.compose.primary.media_path,
            &plan.compose.primary.hostname,
            &[
                ("Range", &range_hdr),
                ("If-Range", "\"arx-mismatched-etag\""),
            ],
            None,
        )
        .await?;
        verify_if_range(
            &hit,
            &miss,
            &full_media.body,
            plan.compose.primary.range_start,
            plan.compose.primary.range_end,
        )?;
    }
    accept.evidence.record(
        "byte_range_verified",
        json!({
            "media_bytes": full_media.body.len(),
            "partial_status": partial_media.status,
            "unsatisfiable_status": unsatisfiable_media.status,
            "media_sha256": hex::encode(Sha256::digest(&full_media.body)),
        }),
    );

    // Unauthenticated admin access must be refused; write a fresh synthetic SQLite event.
    let unauth_admin = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.primary.admin_auth_path,
        &plan.compose.primary.hostname,
        &[],
        None,
    )
    .await?;
    check(
        matches!(unauth_admin.status, 401 | 403),
        "admin_unauth_refused",
    )?;
    let event_nonce = hex::encode(fresh()?);
    let event_body =
        serde_json::to_vec(&json!({"type": "arxcovo_acceptance", "nonce": event_nonce}))
            .map_err(|_| Failure::Check("event_body_encode"))?;
    let event_post = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "POST",
        &plan.compose.primary.event_post_path,
        &plan.compose.primary.hostname,
        &[("Content-Type", "application/json")],
        Some(&event_body),
    )
    .await?;
    check(
        (200..300).contains(&event_post.status),
        "event_ingest_accepted",
    )?;
    let admin_secret = read_admin_bearer(plan.compose.primary.admin_password_file.as_deref())?;
    let auth_header = admin_secret.as_deref().map(|s| format!("Bearer {s}"));
    check(auth_header.is_some(), "admin_credential_required")?;
    let query_headers: Vec<(&str, &str)> = auth_header
        .as_deref()
        .map(|h| vec![("Authorization", h)])
        .unwrap_or_default();
    let authenticated_admin = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.primary.admin_auth_path,
        &plan.compose.primary.hostname,
        &query_headers,
        None,
    )
    .await?;
    check(authenticated_admin.status == 200, "admin_authentication")?;
    let before_restart = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        &ca_der,
        "GET",
        &plan.compose.primary.event_query_path,
        &plan.compose.primary.hostname,
        &query_headers,
        None,
    )
    .await?;
    check(
        before_restart.status == 200
            && std::str::from_utf8(&before_restart.body)
                .is_ok_and(|body| body.contains(&event_nonce)),
        "event_visible_before_restart",
    )?;

    // 7. Create a protected host SQLite backup and verify ledger & physical custody.
    let (backup_id, backup_size, backup_sha) = accept
        .create_backup(&vm_hex, &plan.compose.primary.project_id)
        .await?;
    let settled_backup = accept.ledger.read(|ledger| {
        ledger.compose_backup(
            static_vm_id,
            &plan.compose.primary.project_id,
            backup_id,
            backup_size,
            &backup_sha,
            &plan.backup_root,
        )
    })?;

    // 8. Verify agent delete refusal fence (static VM, volumes, backups, host exec)
    //    and confirm all protected artifacts survive untouched.
    let volumes = accept.ledger.read(|ledger| {
        ledger.compose_volumes_intact(static_vm_id, &plan.compose.primary.project_id)
    })?;
    accept.verify_delete_refusals(&vm_hex, backup_id).await?;
    accept.ledger.read(|ledger| {
        check(
            ledger.static_vm(static_vm_id, &plan.static_vm.name, accept.principal)? == vm_ledger,
            "static_vm_changed",
        )?;
        check(
            ledger.compose_volumes_intact(static_vm_id, &plan.compose.primary.project_id)?
                == volumes,
            "volume_count_changed",
        )?;
        ledger.compose_backup(
            static_vm_id,
            &plan.compose.primary.project_id,
            backup_id,
            backup_size,
            &backup_sha,
            &plan.backup_root,
        )?;
        Ok(())
    })?;

    // 9. Public-only egress probe inside the running static VM.
    let probe_script = build_egress_probe_script(&plan.public_egress);
    let (egress_row, _, _) = accept
        .vm_op(
            "vm_run_script",
            "run_script",
            2,
            json!({
                "vm_id": vm_hex,
                "timeout_s": SCRIPT_TIMEOUT,
                "script": probe_script,
            }),
            Duration::from_secs(SCRIPT_TIMEOUT + 30),
        )
        .await?;
    check(
        egress_row["cleanup"] == "confirmed"
            && egress_row["script_status"] == "exited"
            && egress_row["exit_code"] == 0
            && egress_row["state"] == "succeeded",
        "public_egress_probe",
    )?;
    accept.evidence.record(
        "public_egress_probe_passed",
        json!({
            "denied_target_count": plan.public_egress.denied_targets.len(),
            "ipv6_disabled": true,
        }),
    );

    // 10. Restart static VM and verify SQLite event survives restart.
    let (stopped, _, _) = accept
        .vm_op("vm_stop", "stop", 4, json!({"vm_id": vm_hex}), VM_WORK)
        .await?;
    succeeded(&stopped)?;
    let (restarted, _, _) = accept
        .vm_op("vm_start", "start", 3, json!({"vm_id": vm_hex}), VM_WORK)
        .await?;
    succeeded(&restarted)?;
    let second_start_op = hex::encode(id(&restarted, "operation_id")?);
    check(second_start_op != first_start_op, "restart_fresh_boot")?;
    let after_status = rpc(accept.client, "static_vm_get", json!({"vm_id": vm_hex})).await?;
    let after_origin = static_origin_witness(
        &after_status,
        &vm_hex,
        &gen_hex,
        &second_start_op,
        &plan.public_egress.policy_sha256,
    )?;
    let after_ca_der = read_origin_ca_der(&after_origin, &plan.origin_ca_path)?;
    let after_restart = https_origin_request(
        after_origin.origin_port,
        &after_origin.origin_server_name,
        &after_ca_der,
        "GET",
        &plan.compose.primary.event_query_path,
        &plan.compose.primary.hostname,
        &query_headers,
        None,
    )
    .await?;
    check(
        after_restart.status == 200
            && std::str::from_utf8(&after_restart.body)
                .is_ok_and(|body| body.contains(&event_nonce)),
        "sqlite_event_survived_restart",
    )?;
    accept.evidence.record(
        "restart_persistence_verified",
        json!({"survived_restart": true, "fresh_boot_op": second_start_op}),
    );

    // 11. Offline restore of the settled host backup into a disposable target
    //     directory, verifying the exact synthetic SQLite event and preserving
    //     the settled backup untouched.
    let run_restore_dir = plan.restore_dir.join(&accept.evidence.run_id);
    safe(fs::create_dir(&run_restore_dir), "restore_run_dir")?;
    let restore_evidence = verify_backup_restore_event(
        &settled_backup.settled_path,
        &backup_sha,
        backup_size,
        &run_restore_dir,
        &plan.compose.primary.sqlite_table_query,
        &event_nonce,
    )?;
    accept
        .evidence
        .record("offline_backup_restore_verified", restore_evidence);

    // 12. Stop static VM cleanly and correlate activity receipts.
    let (final_stop, _, _) = accept
        .vm_op("vm_stop", "stop", 4, json!({"vm_id": vm_hex}), VM_WORK)
        .await?;
    succeeded(&final_stop)?;
    accept.ledger.read(Ledger::publication_empty)?;
    accept.correlate().await
}

struct Console;

unsafe extern "system" fn keep_client_alive(event: u32) -> i32 {
    i32::from(event == CTRL_C_EVENT)
}

impl Console {
    fn allocate() -> Result<Self> {
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

fn spawn_daemon(plan: &StaticPlan, pinned_config: &Path) -> Result<Child> {
    safe(
        Command::new(&plan.daemon)
            .arg("--config")
            .arg(pinned_config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn(),
        "foreground_spawn",
    )
}

fn prepare_pinned_config(source: &Path, expected: &ExpectedVmStore) -> Result<tempfile::TempPath> {
    let bytes = safe(pinned_vm_config(source, expected), "foreground_config_pin")?;
    let parent = source.parent().ok_or(Failure::Check("input_parent"))?;
    let mut file = safe(
        tempfile::NamedTempFile::new_in(parent),
        "foreground_config_copy",
    )?;
    safe(file.write_all(&bytes), "foreground_config_copy")?;
    safe(file.as_file().sync_all(), "foreground_config_copy")?;
    Ok(file.into_temp_path())
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

fn bridge_failure(error: ForegroundBridgeError) -> Failure {
    match error {
        ForegroundBridgeError::EndpointMismatch => Failure::Check("bridge_endpoint_binding"),
        ForegroundBridgeError::ConnectTimeout => Failure::Timeout("bridge_connect"),
        ForegroundBridgeError::ConnectRejected => Failure::Check("bridge_connect"),
        ForegroundBridgeError::ProcessUnavailable => Failure::SessionProcessEvidenceUnavailable,
        ForegroundBridgeError::ProcessMismatch => Failure::SessionProcessMismatch,
    }
}

fn verify_hex(value: &str, length: usize, name: &'static str) -> Result<()> {
    check(opaque_hex(value, length), name)
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

fn custody_root(state: &Path, runtime: Option<&RuntimeRoot>) -> Result<LocalPath> {
    let state = local_path(state)?;
    check(!state.is_drive_root(), "foreground_custody_root")?;
    if let Some(runtime) = runtime {
        let volume = local_path(&runtime.volume)?;
        check(
            local_path(&runtime.state)?
                .as_str()
                .eq_ignore_ascii_case(state.as_str())
                && volume.is_drive_root()
                && volume.contains(&state),
            "foreground_custody_root",
        )?;
    }
    Ok(state)
}

fn validate_foreground_profile(foreground: &Foreground) -> Result<()> {
    check(foreground.agents.len() == 1, "isolated_foreground_profile")?;
    if foreground.test_vms.allow_agent_public_sites || foreground.test_vms.publication.is_some() {
        return Err(Failure::PublicPublishingEnabled);
    }
    // Current product constraint (`arxd::config::VmStoreComposeConflict`):
    // `expected_store` cannot yet be combined with `compose_secret_root`.
    if foreground.test_vms.compose_secret_root.is_some() {
        return Err(Failure::StoreComposeConflict);
    }
    Ok(())
}

fn require_distinct_roots(roots: &[(&LocalPath, &[File])]) -> Result<()> {
    let mut handle_sets = Vec::with_capacity(roots.len());
    for (_, pins) in roots {
        let ids: Vec<_> = pins
            .iter()
            .map(store_identity)
            .collect::<Result<Vec<_>>>()?;
        handle_sets.push(ids);
    }
    for i in 0..roots.len() {
        for j in (i + 1)..roots.len() {
            check(
                !roots[i].0.overlaps(roots[j].0),
                "overlapping_acceptance_roots",
            )?;
            let leaf_i = handle_sets[i]
                .last()
                .ok_or(Failure::Check("overlapping_acceptance_roots"))?;
            let leaf_j = handle_sets[j]
                .last()
                .ok_or(Failure::Check("overlapping_acceptance_roots"))?;
            check(
                !handle_sets[i].contains(leaf_j) && !handle_sets[j].contains(leaf_i),
                "overlapping_acceptance_roots",
            )?;
        }
    }
    Ok(())
}

fn mounted_volume(root: &str) -> Result<String> {
    let wide: Vec<_> = root.encode_utf16().chain(Some(0)).collect();
    let mut output = [0; 64];
    check(
        unsafe {
            GetVolumeNameForVolumeMountPointW(
                wide.as_ptr(),
                output.as_mut_ptr(),
                output.len() as u32,
            )
        } != 0,
        "store_volume_mount",
    )?;
    let end = output
        .iter()
        .position(|&unit| unit == 0)
        .ok_or(Failure::Check("store_volume_mount"))?;
    safe(String::from_utf16(&output[..end]), "store_volume_mount")
}

fn held_volume_path(file: &File) -> Result<String> {
    let mut output = [0; 512];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            output.as_mut_ptr(),
            output.len() as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    check(length > 0 && length < output.len(), "store_volume_path")?;
    safe(String::from_utf16(&output[..length]), "store_volume_path")
}

fn pin_store(
    state: &LocalPath,
    enrolled: &ExpectedVmStore,
    mount_name: impl Fn(&str) -> Result<String>,
) -> Result<Vec<File>> {
    let drive = &state.as_str()[..3];
    let guid = enrolled
        .volume_guid
        .strip_prefix(r"\\?\Volume{")
        .and_then(|value| value.strip_suffix(r"}\"))
        .ok_or(Failure::Check("store_volume_pin"))?;
    let mut parts = guid.split('-');
    for length in [8, 4, 4, 4, 12] {
        verify_hex(parts.next().unwrap_or_default(), length, "store_volume_pin")?;
    }
    check(parts.next().is_none(), "store_volume_pin")?;
    verify_hex(&enrolled.volume_serial, 16, "store_volume_pin")?;
    verify_hex(&enrolled.directory_file_id, 32, "store_directory_pin")?;
    let verify_mount = || {
        check(
            mount_name(drive)?.eq_ignore_ascii_case(&enrolled.volume_guid),
            "store_volume_mount",
        )
    };
    verify_mount()?;
    let path = Path::new(state.as_str());
    let pins = pin_root(path)?;
    for (part, file) in path.ancestors().zip(pins.iter().rev()) {
        let suffix = &part.to_str().ok_or(Failure::Check("store_volume_path"))?[3..];
        let expected = format!("{}{suffix}", enrolled.volume_guid);
        check(
            held_volume_path(file)?
                .trim_end_matches('\\')
                .eq_ignore_ascii_case(expected.trim_end_matches('\\')),
            "store_volume_path",
        )?;
    }
    let store = pins.last().ok_or(Failure::Check("store_directory_pin"))?;
    let actual = store_identity(store)?;
    check(
        format!("{:016x}", actual.volume) == enrolled.volume_serial
            && hex::encode(actual.file) == enrolled.directory_file_id,
        "store_directory_pin",
    )?;
    verify_mount()?;
    Ok(pins)
}

fn pin_digest(path: &Path, expected: &str, max: u64) -> Result<File> {
    verify_hex(expected, 64, "digest_pin")?;
    let file = open(path, false, FILE_SHARE_READ)?;
    check(hex::encode(digest(&file, max)?.1) == expected, "digest_pin")?;
    Ok(file)
}

async fn with_client(
    plan: &StaticPlan,
    foreground: &Foreground,
    store: &LocalPath,
    daemon: &Child,
    config: LoadedClient,
    evidence: &mut Evidence,
) -> Result<()> {
    let expected_pid = daemon
        .id()
        .ok_or(Failure::SessionProcessEvidenceUnavailable)?;
    let bound = bind_foreground_bridge(config, &foreground.local.address, expected_pid)
        .await
        .map_err(bridge_failure)?;
    let (sdk, io) = tokio::io::duplex(RESPONSE_LIMIT);
    let mut bridge = JoinSet::new();
    bridge.spawn(bound.forward(io));

    let observed = async {
        let (read, write) = tokio::io::split(sdk);
        let transport =
            AsyncRwTransport::new_client(read, write).with_max_line_length(RESPONSE_LIMIT);
        let initialized = timeout(RPC, ().serve(transport))
            .await
            .map_err(|_| Failure::Timeout("bridge_initialize"))?;
        let client = safe(initialized, "bridge_initialize")?;
        let journey_result = async {
            let discovered = timeout(RPC, client.list_tools(None))
                .await
                .map_err(|_| Failure::Discovery)?
                .map_err(|_| Failure::Discovery)
                .and_then(|reply| serde_json::to_value(reply).map_err(|_| Failure::Discovery));
            discover(discovered)?;
            // Closed admission returns its reason immediately before idle delay.
            let first = rpc(&client, "vm_catalog", json!({})).await?;
            assert_admitted(&first)?;
            static_qualification_digest(&first, plan)?;
            sleep(Duration::from_secs(plan.idle_delay_s)).await;
            let (entries, selected) = catalog(&client, plan).await?;
            evidence.qualification_sha256 = Some(selected);
            let _store = pin_store(store, &plan.store, mounted_volume)?;
            let ledger = LiveLedger::open(&foreground.state_dir, &plan.store)?;
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
        journey_result.and(cancelled.map(|_| ()))
    }
    .await;
    let joined = match timeout(RPC, bridge.join_next()).await {
        Ok(Some(result)) => {
            safe(result, "bridge_exit").and_then(|result| safe(result, "bridge_exit"))
        }
        Ok(None) => Err(Failure::Check("bridge_exit")),
        Err(_) => {
            bridge.abort_all();
            let _ = timeout(RPC, bridge.join_next()).await;
            Err(Failure::Timeout("bridge_exit"))
        }
    };
    observed.and(joined)
}

async fn run() -> Result<()> {
    let plan_path = PathBuf::from(
        env::var_os("ARX_STATIC_VM_ACCEPTANCE_PLAN")
            .ok_or(Failure::Check("acceptance_plan_required"))?,
    );
    d_path(&plan_path)?;
    let _plan_parent = pin_root(plan_path.parent().ok_or(Failure::Check("plan_parent"))?)?;
    let plan_file = open(&plan_path, false, FILE_SHARE_READ)?;
    safe(
        arxd::config::checked_owner_private_path(&plan_path),
        "plan_permissions",
    )?;
    let mut plan_bytes = Vec::new();
    safe(
        std::io::Read::read_to_end(&mut plan_file.take(65_537), &mut plan_bytes),
        "plan_read",
    )?;
    let plan = StaticPlan::parse_bytes(&plan_bytes)?;

    let mut input_pins = Vec::new();
    for path in [
        &plan.daemon,
        &plan.daemon_config,
        &plan.client_config,
        &plan.public_egress.policy_path,
    ] {
        d_path(path)?;
        input_pins.extend(pin_root(
            path.parent().ok_or(Failure::Check("input_parent"))?,
        )?);
    }
    if let Some(admin_pw) = &plan.compose.primary.admin_password_file {
        d_path(admin_pw)?;
        safe(
            arxd::config::checked_owner_private_path(admin_pw),
            "admin_permissions",
        )?;
        input_pins.extend(pin_root(
            admin_pw.parent().ok_or(Failure::Check("input_parent"))?,
        )?);
    }
    for dir in [&plan.evidence_dir, &plan.backup_root, &plan.restore_dir] {
        d_path(dir)?;
        safe(
            arxd::config::checked_owner_private_path(dir),
            "private_root_permissions",
        )?;
    }
    let evidence_pins = pin_root(&plan.evidence_dir)?;
    let backup_pins = pin_root(&plan.backup_root)?;
    let restore_pins = pin_root(&plan.restore_dir)?;

    let _daemon = pin_digest(&plan.daemon, &plan.daemon_sha256, 512 * 1024 * 1024)?;
    let policy_file = open(&plan.public_egress.policy_path, false, FILE_SHARE_READ)?;
    let mut policy_bytes = Vec::new();
    safe(
        std::io::Read::read_to_end(&mut policy_file.take(65_537), &mut policy_bytes),
        "policy_read",
    )?;
    verify_egress_policy_covers_plan(&policy_bytes, &plan.public_egress)?;

    let daemon_config = open(&plan.daemon_config, false, FILE_SHARE_READ)?;
    let _client_config = open(&plan.client_config, false, FILE_SHARE_READ)?;
    let foreground: Foreground = config(&daemon_config)?;
    let client = safe(ClientConfig::load(&plan.client_config), "bridge_config")?;
    check(
        client.address() == foreground.local.address,
        "bridge_endpoint_binding",
    )?;
    validate_foreground_profile(&foreground)?;

    d_path(&foreground.state_dir)?;
    let state = custody_root(&foreground.state_dir, foreground.test_vms.runtime.as_ref())?;
    let store_pins = pin_store(&state, &plan.store, mounted_volume)?;
    let evidence_root = local_path(&plan.evidence_dir)?;
    let backup_root = local_path(&plan.backup_root)?;
    let restore_root = local_path(&plan.restore_dir)?;
    require_distinct_roots(&[
        (&state, &store_pins),
        (&evidence_root, &evidence_pins),
        (&backup_root, &backup_pins),
        (&restore_root, &restore_pins),
    ])?;
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
        cloudflare_publishing_disabled: true,
        started_at_ms: now_ms()?,
        finished_at_ms: 0,
        observations: Vec::new(),
        drain: false,
        failure: None,
        drain_failure: None,
    };

    let result = async {
        require_unused(&foreground.local.address)?;
        safe(
            with_enrolled_vm_store(&foreground.state_dir, &plan.store, Ledger::preflight),
            "foreground_store_binding",
        )??;
        let launch_config = prepare_pinned_config(&plan.daemon_config, &plan.store)?;
        let _launch_config = open(&launch_config, false, FILE_SHARE_READ)?;
        let console = Console::allocate()?;
        let _startup_store = pin_store(&state, &plan.store, mounted_volume)?;
        let mut daemon = spawn_daemon(&plan, &launch_config)?;
        let observed = async {
            ready(&mut daemon, &foreground.local.address).await?;
            let _live_store = pin_store(&state, &plan.store, mounted_volume)?;
            with_client(&plan, &foreground, &state, &daemon, client, &mut evidence).await
        }
        .await;
        let drained = console.drain(&mut daemon).await;
        evidence.drain = drained.is_ok();
        evidence.drain_failure = drained.clone().err();
        observed
            .and(drained)
            .and_then(|()| pin_store(&state, &plan.store, mounted_volume).map(|_| ()))
    }
    .await;

    evidence.finished_at_ms = now_ms()?;
    evidence.failure = result.clone().err();
    let evidence_json =
        serde_json::to_value(&evidence).map_err(|_| Failure::Check("evidence_json"))?;
    assert_evidence_sanitized(
        &evidence_json,
        &[
            &plan.compose.primary.hostname,
            &plan.compose.secondary.hostname,
        ],
    )?;
    evidence.write(&plan.evidence_dir)?;
    println!("Static VM acceptance run {run_id}");
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "owner-host static-VM acceptance requires independently reviewed M4 and public-egress native qualification"]
async fn owner_host_static_vm() {
    if let Err(error) = run().await {
        panic!("Static VM acceptance {error:?}");
    }
}

#[cfg(test)]
mod owner_tests {
    use super::*;

    #[test]
    fn request_key_carries_current_wire_timestamp() {
        let before = now_ms().unwrap();
        let key = request_key().unwrap();
        assert!(arxd::vm::RequestId::parse(&hex::encode(key)).is_ok());
        let issued = i64::from_be_bytes(key[..8].try_into().unwrap());
        assert!((before..=now_ms().unwrap()).contains(&issued));
    }

    fn sample_foreground(
        allow_public: bool,
        has_publication: bool,
        has_secrets: bool,
    ) -> Foreground {
        serde_json::from_value(json!({
            "state_dir": "D:/acceptance/state",
            "local": {"address": r"\\.\pipe\arxcovo-static-fixture", "client_identity": "S-1-5-21-1-2-3-1001"},
            "agents": [{"principal": "accept", "token_sha256": "a".repeat(64)}],
            "test_vms": {
                "allow_agent_public_sites": allow_public,
                "publication": if has_publication { json!({"hostnames": ["calm.example.invalid"]}) } else { Value::Null },
                "compose_secret_root": if has_secrets { json!("D:/acceptance/secrets") } else { Value::Null }
            }
        }))
        .unwrap()
    }

    #[test]
    fn static_profile_refuses_publishing_enabled_or_publication_block() {
        assert_eq!(
            validate_foreground_profile(&sample_foreground(false, false, false)),
            Ok(())
        );
        assert_eq!(
            validate_foreground_profile(&sample_foreground(true, false, false)),
            Err(Failure::PublicPublishingEnabled)
        );
        assert_eq!(
            validate_foreground_profile(&sample_foreground(false, true, false)),
            Err(Failure::PublicPublishingEnabled)
        );
    }

    #[test]
    fn static_profile_detects_enrolled_store_compose_secret_conflict() {
        assert_eq!(
            validate_foreground_profile(&sample_foreground(false, false, true)),
            Err(Failure::StoreComposeConflict)
        );
    }

    #[test]
    fn static_plan_refuses_overlapping_store_backup_restore_and_evidence_roots() {
        let base = private_fixture::private_dir();
        let root = Path::new(base.path().to_str().unwrap().trim_start_matches(r"\\?\"));
        let dirs: Vec<_> = ["state", "evidence", "backups", "restore"]
            .into_iter()
            .map(|name| {
                let p = root.join(name);
                fs::create_dir(&p).unwrap();
                p
            })
            .collect();
        let locals: Vec<_> = dirs.iter().map(|p| local_path(p).unwrap()).collect();
        let pins: Vec<_> = dirs.iter().map(|p| pin_root(p).unwrap()).collect();
        let tuples: Vec<_> = locals
            .iter()
            .zip(pins.iter())
            .map(|(l, p)| (l, p.as_slice()))
            .collect();
        assert_eq!(require_distinct_roots(&tuples), Ok(()));

        // Nested child inside state_dir is refused.
        let nested = dirs[0].join("nested-restore");
        fs::create_dir(&nested).unwrap();
        let nested_local = local_path(&nested).unwrap();
        let nested_pins = pin_root(&nested).unwrap();
        assert_eq!(
            require_distinct_roots(&[(&locals[0], &pins[0]), (&nested_local, &nested_pins)]),
            Err(Failure::Check("overlapping_acceptance_roots"))
        );

        // Same directory used twice is refused.
        assert_eq!(
            require_distinct_roots(&[(&locals[1], &pins[1]), (&locals[1], &pins[1])]),
            Err(Failure::Check("overlapping_acceptance_roots"))
        );
    }
}

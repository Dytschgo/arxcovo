use arx_guest::{
    compose::{MAX_LOG_ENTRIES, is_project_id, routing},
    compose_backup::restore_sqlite_offline,
};
use arxd::{
    config::ExpectedVmStore,
    vm::{
        public_egress::{
            EGRESS_MANIFEST, FROZEN_DENY, PublicEgressPolicy, STOCK_QEMU_1113,
            STOCK_RECEIPT_1113_SHA256, SYNTHETIC_SUBNET, parse_ipv4,
        },
        static_web::{ORIGIN_PORT_FIRST, ORIGIN_PORT_LAST, StaticName},
    },
};
use rusqlite::{Connection, OpenFlags};
use rustls::{
    ClientConfig as TlsClientConfig, RootCertStore,
    pki_types::{CertificateDer, ServerName},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, sleep, timeout, timeout_at},
};
use tokio_rustls::TlsConnector;

#[cfg(all(windows, target_arch = "x86_64"))]
#[path = "static_vm_acceptance/owner.rs"]
mod owner;

type Result<T> = std::result::Result<T, Failure>;

const WINDOWS_TEST_RECIPE_ID: &str = "3c2fe2069942ce6c6cebe5c3e700e758";
const MAX_ORIGIN_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_RANGE_SPAN_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
enum Failure {
    Discovery,
    MissingTool(&'static str),
    ForbiddenToolExposed(&'static str),
    SchemaChanged(&'static str),
    AdmissionClosed(&'static str),
    DaemonQualificationEvidenceUnavailable,
    DaemonQualificationEvidenceMismatch,
    StaticOriginEvidenceUnavailable,
    StaticOriginEvidenceMismatch,
    #[cfg(all(windows, target_arch = "x86_64"))]
    SessionProcessEvidenceUnavailable,
    #[cfg(all(windows, target_arch = "x86_64"))]
    SessionProcessMismatch,
    #[cfg(all(windows, target_arch = "x86_64"))]
    PublicPublishingEnabled,
    RefusalMismatch(&'static str),
    OriginResponseMismatch(&'static str),
    ByteRangeMismatch(&'static str),
    BackupRestoreMismatch(&'static str),
    EgressFenceMismatch(&'static str),
    Timeout(&'static str),
    Check(&'static str),
}

fn check(condition: bool, name: &'static str) -> Result<()> {
    condition.then_some(()).ok_or(Failure::Check(name))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticPlan {
    daemon: PathBuf,
    daemon_sha256: String,
    origin_ca_path: PathBuf,
    daemon_config: PathBuf,
    client_config: PathBuf,
    store: ExpectedVmStore,
    qualification_sha256: String,
    m4_qualification_sha256: String,
    network_receipt: PathBuf,
    network_receipt_sha256: String,
    #[serde(default)]
    witness_dir: Option<PathBuf>,
    evidence_dir: PathBuf,
    backup_root: PathBuf,
    restore_dir: PathBuf,
    #[serde(default)]
    idle_delay_s: u64,
    static_vm: StaticVmSpec,
    public_egress: PublicEgressSpec,
    compose: ComposeSpec,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticVmSpec {
    vm_id: String,
    generation: String,
    name: String,
    recipe_id: String,
    recipe_sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicEgressSpec {
    manifest: String,
    qemu_sha256: String,
    receipt_sha256: String,
    policy_path: PathBuf,
    policy_sha256: String,
    public_tcp_target: String,
    public_dns_target: String,
    denied_targets: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComposeSpec {
    primary: PrimaryServiceSpec,
    secondary: SecondaryServiceSpec,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrimaryServiceSpec {
    project_id: String,
    release_id: String,
    hostname: String,
    marker_path: String,
    expected_marker: String,
    media_path: String,
    range_start: usize,
    range_end: usize,
    event_post_path: String,
    event_query_path: String,
    admin_auth_path: String,
    #[serde(default)]
    admin_password_file: Option<PathBuf>,
    sqlite_table_query: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecondaryServiceSpec {
    project_id: String,
    release_id: String,
    hostname: String,
    marker_path: String,
    expected_marker: String,
}

fn parse_ipv4_endpoint(raw: &str) -> Option<SocketAddrV4> {
    let (host, port) = raw.split_once(':')?;
    let ip = parse_ipv4(host)?;
    let port: u16 = port.parse().ok().filter(|p| *p > 0)?;
    Some(SocketAddrV4::new(ip, port))
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    !SYNTHETIC_SUBNET.contains(ip) && !FROZEN_DENY.iter().any(|cidr| cidr.contains(ip))
}

fn valid_http_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 256
        && path
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'\r' | b'\n'))
}

fn valid_readonly_sqlite_query(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed.contains(';') {
        return false;
    }
    let upper = trimmed.to_ascii_uppercase();
    if !upper.starts_with("SELECT ") {
        return false;
    }
    for forbidden in [
        "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "ATTACH", "DETACH", "PRAGMA",
        "REPLACE", "VACUUM",
    ] {
        if upper.contains(forbidden) {
            return false;
        }
    }
    true
}

impl StaticPlan {
    fn parse_bytes(bytes: &[u8]) -> Result<Self> {
        check(bytes.len() <= 65_536, "plan_size_bound")?;
        let plan: Self =
            serde_json::from_slice(bytes).map_err(|_| Failure::Check("plan_format"))?;
        plan.validate()?;
        Ok(plan)
    }

    fn validate(&self) -> Result<()> {
        for path in [
            &self.daemon,
            &self.origin_ca_path,
            &self.network_receipt,
            &self.daemon_config,
            &self.client_config,
            &self.evidence_dir,
            &self.backup_root,
            &self.restore_dir,
            &self.public_egress.policy_path,
        ]
        .into_iter()
        .chain(self.witness_dir.as_ref())
        .chain(self.compose.primary.admin_password_file.as_ref())
        {
            let raw = path.to_str().ok_or(Failure::Check("d_drive_path"))?;
            let local = arxd::vm::paths::LocalPath::parse(raw)
                .map_err(|_| Failure::Check("d_drive_path"))?;
            check(
                !local.is_drive_root() && local.as_str()[..2].eq_ignore_ascii_case("D:"),
                "d_drive_path",
            )?;
        }
        check(
            opaque_hex(&self.store.volume_serial, 16)
                && opaque_hex(&self.store.directory_file_id, 32),
            "store_identity",
        )?;
        check(
            opaque_nonzero_hex(&self.static_vm.vm_id, 32)
                && opaque_nonzero_hex(&self.static_vm.generation, 32),
            "enrolled_vm_pin",
        )?;
        check(
            opaque_nonzero_hex(&self.network_receipt_sha256, 64),
            "network_receipt_pin",
        )?;
        check(self.idle_delay_s <= 900, "idle_delay_bound")?;
        check(opaque_hex(&self.daemon_sha256, 64), "daemon_sha256")?;
        check(
            opaque_hex(&self.qualification_sha256, 64),
            "qualification_sha256",
        )?;

        check(
            opaque_nonzero_hex(&self.m4_qualification_sha256, 64),
            "m4_qualification_sha256",
        )?;
        StaticName::parse(&self.static_vm.name).map_err(|_| Failure::Check("static_vm_name"))?;
        check(
            opaque_nonzero_hex(&self.static_vm.recipe_id, 32)
                && self.static_vm.recipe_id != WINDOWS_TEST_RECIPE_ID,
            "compose_recipe_id",
        )?;
        check(
            opaque_hex(&self.static_vm.recipe_sha256, 64),
            "compose_recipe_sha256",
        )?;

        check(
            self.public_egress.manifest == EGRESS_MANIFEST,
            "public_egress_manifest",
        )?;
        check(
            opaque_hex(&self.public_egress.qemu_sha256, 64)
                && self.public_egress.qemu_sha256 != hex::encode(STOCK_QEMU_1113.sha256),
            "public_egress_qemu_sha256",
        )?;
        check(
            opaque_hex(&self.public_egress.receipt_sha256, 64)
                && self.public_egress.receipt_sha256 != hex::encode(STOCK_RECEIPT_1113_SHA256),
            "public_egress_receipt_sha256",
        )?;
        check(
            opaque_hex(&self.public_egress.policy_sha256, 64),
            "public_egress_policy_sha256",
        )?;

        parse_ipv4_endpoint(&self.public_egress.public_tcp_target)
            .filter(|addr| is_public_ipv4(*addr.ip()))
            .ok_or(Failure::Check("public_tcp_target"))?;
        parse_ipv4_endpoint(&self.public_egress.public_dns_target)
            .filter(|addr| is_public_ipv4(*addr.ip()) && addr.port() == 53)
            .ok_or(Failure::Check("public_dns_target"))?;

        check(
            (4..=32).contains(&self.public_egress.denied_targets.len()),
            "denied_targets_bound",
        )?;
        let mut saw_gateway = false;
        let mut saw_dns_alias = false;
        let mut saw_link_local = false;
        let mut saw_cgnat = false;
        let mut private_lan = false;
        let mut public_denied = std::collections::BTreeSet::new();
        for raw in &self.public_egress.denied_targets {
            let endpoint =
                parse_ipv4_endpoint(raw).ok_or(Failure::Check("denied_target_format"))?;
            let ip = *endpoint.ip();
            private_lan |= ip.is_private() && !SYNTHETIC_SUBNET.contains(ip);
            if is_public_ipv4(ip) {
                public_denied.insert(ip);
            }
            if ip == Ipv4Addr::new(10, 0, 2, 2) {
                saw_gateway = true;
            }
            if ip == Ipv4Addr::new(10, 0, 2, 3) {
                saw_dns_alias = true;
            }
            if ip.octets()[0] == 169 && ip.octets()[1] == 254 {
                saw_link_local = true;
            }
            if ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]) {
                saw_cgnat = true;
            }
        }
        check(
            saw_gateway
                && saw_dns_alias
                && saw_link_local
                && saw_cgnat
                && private_lan
                && public_denied.len() >= 2,
            "denied_targets_coverage",
        )?;

        let primary = &self.compose.primary;
        let secondary = &self.compose.secondary;
        check(
            primary.admin_password_file.is_some(),
            "admin_credential_required",
        )?;
        check(
            is_project_id(&primary.project_id)
                && is_project_id(&secondary.project_id)
                && primary.project_id != secondary.project_id,
            "compose_project_ids",
        )?;
        check(
            opaque_nonzero_hex(&primary.release_id, 32)
                && opaque_nonzero_hex(&secondary.release_id, 32)
                && primary.release_id != secondary.release_id,
            "compose_release_ids",
        )?;
        check(
            routing::is_exact_hostname(&primary.hostname)
                && routing::is_exact_hostname(&secondary.hostname)
                && primary.hostname != secondary.hostname
                && routing::share_enrolled_zone(&[&primary.hostname, &secondary.hostname], None),
            "compose_hostnames",
        )?;
        check(
            !primary.expected_marker.is_empty()
                && primary.expected_marker.len() <= 128
                && !secondary.expected_marker.is_empty()
                && secondary.expected_marker.len() <= 128
                && primary.expected_marker != secondary.expected_marker,
            "compose_distinct_markers",
        )?;
        for path in [
            &primary.marker_path,
            &primary.media_path,
            &primary.event_post_path,
            &primary.event_query_path,
            &primary.admin_auth_path,
            &secondary.marker_path,
        ] {
            check(valid_http_path(path), "compose_http_path")?;
        }
        check(
            primary.range_start <= primary.range_end
                && primary.range_end - primary.range_start < MAX_RANGE_SPAN_BYTES,
            "compose_byte_range",
        )?;
        check(
            valid_readonly_sqlite_query(&primary.sqlite_table_query),
            "compose_sqlite_query",
        )?;
        Ok(())
    }
}

fn opaque(value: &Value, len: usize) -> Option<&str> {
    value.as_str().filter(|id| opaque_hex(id, len))
}

fn opaque_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn opaque_nonzero_hex(value: &str, len: usize) -> bool {
    opaque_hex(value, len) && value.bytes().any(|b| b != b'0')
}

/// Independent reviewed wire contract for the static-VM, Compose, backup,
/// publishing-status, and activity tools in the foreground VM profile.
fn schemas() -> Vec<(&'static str, Value)> {
    let id = json!({"type":"string","pattern":"^[0-9a-f]{32}$"});
    let cursor = json!({"type":"string","maxLength":96,"pattern":"^[A-Za-z0-9_-]+$"});
    let project = json!({"type":"string","pattern":"^[a-z](?:[a-z0-9-]{0,22}[a-z0-9])?$"});
    let object = |properties: Value, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let vm_request = object(
        json!({"vm_id":id,"request_id":id}),
        &["vm_id", "request_id"],
    );
    let compose_request = object(
        json!({"vm_id":id,"project_id":project,"request_id":id}),
        &["vm_id", "project_id", "request_id"],
    );
    let page = object(
        json!({"limit":{"type":"integer","minimum":1,"maximum":20,"default":20},"cursor":cursor}),
        &[],
    );
    let mut activity = object(
        json!({
            "limit":{"type":"integer","minimum":1,"maximum":50,"default":20},
            "before_id":{"type":"integer","minimum":1,"maximum":i64::MAX},
            "activity_id":{"type":"integer","minimum":1,"maximum":i64::MAX},
            "include_reads":{"type":"boolean","default":false}
        }),
        &[],
    );
    activity["not"] = json!({"required":["activity_id","before_id"]});
    vec![
        ("list_activity", activity),
        ("vm_catalog", page.clone()),
        (
            "vm_template_build",
            object(
                json!({"recipe_id":id,"request_id":id}),
                &["recipe_id", "request_id"],
            ),
        ),
        (
            "vm_operation",
            object(json!({"operation_id":id}), &["operation_id"]),
        ),
        ("vm_start", vm_request.clone()),
        ("vm_stop", vm_request.clone()),
        (
            "vm_run_script",
            object(
                json!({
                    "vm_id":id,"request_id":id,
                    "script":{"type":"string","minLength":1,"maxLength":4096},
                    "timeout_s":{"type":"integer","minimum":1,"maximum":120,"default":30}
                }),
                &["vm_id", "request_id", "script"],
            ),
        ),
        (
            "static_vm_create",
            object(
                json!({
                    "template_id":id,"request_id":id,
                    "name":{"type":"string","minLength":1,"maxLength":63,
                        "pattern":"^[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?$"}
                }),
                &["template_id", "request_id", "name"],
            ),
        ),
        ("static_vm_get", object(json!({"vm_id":id}), &["vm_id"])),
        ("static_vm_list", page),
        ("publish_site", vm_request.clone()),
        ("unpublish_site", vm_request),
        ("publish_status", object(json!({"vm_id":id}), &["vm_id"])),
        (
            "vm_snapshot_delete",
            object(
                json!({"vm_id":id,"snapshot_id":id,"request_id":id}),
                &["vm_id", "snapshot_id", "request_id"],
            ),
        ),
        (
            "compose_deploy",
            object(
                json!({"vm_id":id,"project_id":project,"release_id":id,"request_id":id}),
                &["vm_id", "project_id", "release_id", "request_id"],
            ),
        ),
        ("compose_start", compose_request.clone()),
        ("compose_stop", compose_request.clone()),
        ("compose_down", compose_request.clone()),
        (
            "compose_status",
            object(
                json!({"vm_id":id,"project_id":project}),
                &["vm_id", "project_id"],
            ),
        ),
        (
            "compose_logs",
            object(
                json!({
                    "vm_id":id,"project_id":project,
                    "limit":{"type":"integer","minimum":1,"maximum":MAX_LOG_ENTRIES},
                    "cursor":{"type":"integer","minimum":0,"maximum":i64::MAX}
                }),
                &["vm_id", "project_id"],
            ),
        ),
        ("compose_backup", compose_request),
    ]
}

const FORBIDDEN_TOOLS: [&str; 10] = [
    "static_vm_delete",
    "vm_delete",
    "backup_delete",
    "vm_backup_delete",
    "compose_backup_delete",
    "compose_volume_delete",
    "compose_project_delete",
    "compose_backup_restore",
    "run_command",
    "discover_tools",
];

fn discover(page: Result<Value>) -> Result<()> {
    let page = page?;
    let tools = page["tools"].as_array().ok_or(Failure::Discovery)?;
    if !page["nextCursor"].is_null() || tools.len() > 64 {
        return Err(Failure::Discovery);
    }
    for forbidden in FORBIDDEN_TOOLS {
        if tools.iter().any(|tool| tool["name"] == forbidden) {
            return Err(Failure::ForbiddenToolExposed(forbidden));
        }
    }
    for (name, schema) in schemas() {
        let mut found = tools.iter().filter(|tool| tool["name"] == name);
        let tool = found.next().ok_or(Failure::MissingTool(name))?;
        if found.next().is_some() {
            return Err(Failure::Discovery);
        }
        if tool["inputSchema"] != schema {
            return Err(Failure::SchemaChanged(name));
        }
    }
    Ok(())
}

/// Closed refusal vocabulary matching `crates/arxd/src/mcp/vm.rs:refusal_wire`.
/// Any unrecognized string collapses to `"unrecognized_refusal"` so paths or
/// secrets can never leak into evidence.
fn reason(value: &Value) -> Option<&'static str> {
    [
        "invalid",
        "stale_key",
        "key_collision",
        "replay_full",
        "pending_full",
        "not_found",
        "busy",
        "expired",
        "template_limit",
        "instance_limit",
        "snapshot_limit",
        "disk",
        "state_headroom",
        "unknown_host",
        "owner_active",
        "cpu",
        "ram",
        "shutdown",
        "cleanup_unconfirmed",
        "disabled",
        "admission_closed",
        "draining",
        "resource_unavailable",
        "unsupported_platform",
        "qualification_missing",
        "identity_unverified",
        "distribution_unverified",
        "path_custody_unverified",
        "job_unverified",
        "whpx_unverified",
        "qmp_unverified",
        "guest_endpoint_unverified",
        "disk_ceiling_unverified",
        "owner_activity_unverified",
        "reserves_unverified",
        "network_fence_unverified",
        "installer_phase_unavailable",
        "installer_phase_mismatch",
        "installer_deadline_expired",
        "not_ready",
        "recipe_unqualified",
        "host_execution_disabled",
        "invalid_cursor",
        "state_unavailable",
        "name_unavailable",
        "static_delete_forbidden",
        "unknown_tool",
        "public_sites_disabled",
        "publish_credential_missing",
        "publish_target_mismatch",
        "publish_cleanup_pending",
        "snapshot_publish_not_quiesced",
        "static_backup_delete_forbidden",
        "snapshot_offline_required",
        "compose_recovery_required",
        "backup_limit",
    ]
    .into_iter()
    .find(|code| value == *code)
}

fn assert_admitted(catalog: &Value) -> Result<()> {
    let capability = &catalog["capability"];
    match capability["available"].as_bool() {
        Some(true) => Ok(()),
        Some(false) => Err(Failure::AdmissionClosed(
            reason(&capability["reason"]).unwrap_or("unrecognized_refusal"),
        )),
        None => Err(Failure::Check("catalog_capability")),
    }
}

fn assert_refused(reply: &Value, expected: &'static str) -> Result<&'static str> {
    if reply["ok"] != false {
        return Err(Failure::RefusalMismatch(expected));
    }
    let actual = reason(&reply["reason"]).unwrap_or("unrecognized_refusal");
    if actual != expected {
        return Err(Failure::RefusalMismatch(actual));
    }
    Ok(actual)
}

/// Exact refusal codes required by ADR 0058, ADR 0064, and ADR 0070 for
/// agent deletion, restore, and host execution probes.
fn expected_delete_refusal(tool: &str) -> Result<&'static str> {
    match tool {
        "static_vm_delete" | "vm_delete" | "backup_delete" | "vm_backup_delete" => {
            Ok("static_delete_forbidden")
        }
        "compose_backup_delete"
        | "compose_volume_delete"
        | "compose_project_delete"
        | "compose_backup_restore"
        | "vm_snapshot_delete" => Ok("static_backup_delete_forbidden"),
        "run_command" | "discover_tools" => Ok("host_execution_disabled"),
        _ => Err(Failure::Check("unknown_delete_probe")),
    }
}

/// Read the daemon's selected static/Compose qualification receipt from the
/// authenticated catalog projection. Plan pins are expectations only.
fn static_qualification_digest<'a>(catalog: &'a Value, plan: &StaticPlan) -> Result<&'a str> {
    let m4 = opaque(
        &catalog["capability"]["selected_test_qualification"]["sha256"],
        64,
    )
    .ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    if m4 != plan.m4_qualification_sha256 {
        return Err(Failure::DaemonQualificationEvidenceMismatch);
    }
    let selected = &catalog["capability"]["selected_static_qualification"];
    let digest =
        opaque(&selected["sha256"], 64).ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    let recipe = &selected["compose_recipe"];
    let egress = &selected["public_egress"];
    let recipe_id =
        opaque(&recipe["recipe_id"], 32).ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    let recipe_sha = opaque(&recipe["recipe_sha256"], 64)
        .ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    let qemu_sha = opaque(&egress["qemu_sha256"], 64)
        .ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    let receipt_sha = opaque(&egress["receipt_sha256"], 64)
        .ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    let policy_sha = opaque(&egress["policy_sha256"], 64)
        .ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    let stock_qemu = hex::encode(STOCK_QEMU_1113.sha256);
    let stock_receipt = hex::encode(STOCK_RECEIPT_1113_SHA256);
    if digest != plan.qualification_sha256
        || recipe_id != plan.static_vm.recipe_id
        || recipe_sha != plan.static_vm.recipe_sha256
        || egress["manifest"] != EGRESS_MANIFEST
        || qemu_sha != plan.public_egress.qemu_sha256
        || qemu_sha == stock_qemu
        || receipt_sha != plan.public_egress.receipt_sha256
        || receipt_sha == stock_receipt
        || policy_sha != plan.public_egress.policy_sha256
        || egress["network_qualified"] != true
    {
        return Err(Failure::DaemonQualificationEvidenceMismatch);
    }
    Ok(digest)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StaticOriginWitness {
    witness_id: String,
    origin_port: u16,
    origin_server_name: String,
    origin_ca_sha256: String,
}

fn static_origin_witness(
    status: &Value,
    vm_id: &str,
    generation: &str,
    start_op: &str,
    expected_policy_sha256: &str,
) -> Result<StaticOriginWitness> {
    let receipt = &status["static_origin_witness"];
    let witness_id =
        opaque(&receipt["witness_id"], 32).ok_or(Failure::StaticOriginEvidenceUnavailable)?;
    let origin_ca_sha256 =
        opaque(&receipt["origin_ca_sha256"], 64).ok_or(Failure::StaticOriginEvidenceUnavailable)?;
    let port = receipt["origin_port"]
        .as_u64()
        .and_then(|v| u16::try_from(v).ok())
        .ok_or(Failure::StaticOriginEvidenceUnavailable)?;
    let expected_server_name = format!("arx-{generation}.invalid");
    if receipt["source"] != "static_origin_tls"
        || receipt["vm_id"] != vm_id
        || receipt["generation"] != generation
        || receipt["operation_id"] != start_op
        || witness_id == start_op
        || !(ORIGIN_PORT_FIRST..=ORIGIN_PORT_LAST).contains(&port)
        || receipt["origin_server_name"] != expected_server_name
        || receipt["network_class"] != "public_egress"
        || receipt["policy_sha256"] != expected_policy_sha256
    {
        return Err(Failure::StaticOriginEvidenceMismatch);
    }
    Ok(StaticOriginWitness {
        witness_id: witness_id.to_owned(),
        origin_port: port,
        origin_server_name: expected_server_name,
        origin_ca_sha256: origin_ca_sha256.to_owned(),
    })
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// Connect directly to the static VM's localhost TLS forward (`127.0.0.1:<port>`)
/// verifying server TLS strictly against the per-generation CA and pinned
/// `originServerName` (`arx-<generation>.invalid`), without contacting Cloudflare.
#[allow(clippy::too_many_arguments)]
async fn https_origin_request(
    port: u16,
    origin_server_name: &str,
    ca_der: &[u8],
    method: &str,
    path: &str,
    host_header: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<HttpResponse> {
    if extra_headers
        .iter()
        .any(|(_, value)| !value.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
    {
        return Err(Failure::OriginResponseMismatch("unsafe_http_header"));
    }
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(ca_der.to_vec()))
        .map_err(|_| Failure::OriginResponseMismatch("origin_ca_invalid"))?;
    let tls_config =
        TlsClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| Failure::OriginResponseMismatch("tls_config"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(tls_config));
    let sni = ServerName::try_from(origin_server_name.to_owned())
        .map_err(|_| Failure::OriginResponseMismatch("origin_sni"))?;
    let tcp = timeout(
        Duration::from_secs(10),
        TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .map_err(|_| Failure::Timeout("origin_tcp_connect"))?
    .map_err(|_| Failure::OriginResponseMismatch("origin_tcp_connect"))?;
    let mut tls = timeout(Duration::from_secs(10), connector.connect(sni, tcp))
        .await
        .map_err(|_| Failure::Timeout("origin_tls_handshake"))?
        .map_err(|_| Failure::OriginResponseMismatch("origin_tls_handshake"))?;

    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\n");
    for (name, value) in extra_headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(payload) = body {
        request.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    request.push_str("\r\n");

    timeout(Duration::from_secs(10), async {
        tls.write_all(request.as_bytes()).await?;
        if let Some(payload) = body {
            tls.write_all(payload).await?;
        }
        tls.flush().await
    })
    .await
    .map_err(|_| Failure::Timeout("origin_write"))?
    .map_err(|_| Failure::OriginResponseMismatch("origin_write"))?;

    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    timeout(Duration::from_secs(15), async {
        loop {
            let read = tls.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            if raw.len().saturating_add(read) > MAX_ORIGIN_RESPONSE_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "response too large",
                ));
            }
            raw.extend_from_slice(&chunk[..read]);
        }
        Ok::<_, std::io::Error>(())
    })
    .await
    .map_err(|_| Failure::Timeout("origin_read"))?
    .map_err(|_| Failure::OriginResponseMismatch("origin_read"))?;

    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Result<HttpResponse> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(Failure::OriginResponseMismatch("http_header_boundary"))?;
    let head = std::str::from_utf8(&raw[..split])
        .map_err(|_| Failure::OriginResponseMismatch("http_header_utf8"))?;
    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or(Failure::OriginResponseMismatch("http_status_line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or(Failure::OriginResponseMismatch("http_status_code"))?;
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    let body = raw[split + 4..].to_vec();
    if let Some(len) = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        && body.len() != len
    {
        return Err(Failure::OriginResponseMismatch("http_content_length"));
    }
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

fn verify_two_host_responses(
    primary: &HttpResponse,
    primary_expected: &str,
    secondary: &HttpResponse,
    secondary_expected: &str,
    unknown_host: &HttpResponse,
) -> Result<()> {
    if primary.status != 200 {
        return Err(Failure::OriginResponseMismatch("primary_status"));
    }
    if secondary.status != 200 {
        return Err(Failure::OriginResponseMismatch("secondary_status"));
    }
    let primary_text = std::str::from_utf8(&primary.body)
        .map_err(|_| Failure::OriginResponseMismatch("primary_utf8"))?;
    let secondary_text = std::str::from_utf8(&secondary.body)
        .map_err(|_| Failure::OriginResponseMismatch("secondary_utf8"))?;
    if !primary_text.contains(primary_expected) {
        return Err(Failure::OriginResponseMismatch("primary_marker"));
    }
    if !secondary_text.contains(secondary_expected) {
        return Err(Failure::OriginResponseMismatch("secondary_marker"));
    }
    if primary.body == secondary.body {
        return Err(Failure::OriginResponseMismatch("hosts_not_distinct"));
    }
    if (200..300).contains(&unknown_host.status) {
        return Err(Failure::OriginResponseMismatch("unknown_host_accepted"));
    }
    Ok(())
}

fn verify_byte_range_206(
    partial: &HttpResponse,
    full: &HttpResponse,
    start: usize,
    end: usize,
) -> Result<()> {
    if full.status != 200 || full.body.is_empty() {
        return Err(Failure::ByteRangeMismatch("full_media_status"));
    }
    if full.header("accept-ranges") != Some("bytes") {
        return Err(Failure::ByteRangeMismatch("full_accept_ranges"));
    }
    if end >= full.body.len() || start > end {
        return Err(Failure::ByteRangeMismatch("range_out_of_bounds"));
    }
    if partial.status != 206 {
        return Err(Failure::ByteRangeMismatch("status_206"));
    }
    if partial.header("accept-ranges") != Some("bytes") {
        return Err(Failure::ByteRangeMismatch("partial_accept_ranges"));
    }
    let expected_range = format!("bytes {start}-{end}/{}", full.body.len());
    if partial.header("content-range") != Some(expected_range.as_str()) {
        return Err(Failure::ByteRangeMismatch("content_range_206"));
    }
    let expected_len = end - start + 1;
    if partial.header("content-length") != Some(expected_len.to_string().as_str()) {
        return Err(Failure::ByteRangeMismatch("content_length_206"));
    }
    if partial.body != full.body[start..=end] {
        return Err(Failure::ByteRangeMismatch("body_slice_206"));
    }
    Ok(())
}

fn verify_byte_range_416(unsatisfiable: &HttpResponse, total_len: usize) -> Result<()> {
    if unsatisfiable.status != 416 {
        return Err(Failure::ByteRangeMismatch("status_416"));
    }
    let expected = format!("bytes */{total_len}");
    if unsatisfiable.header("content-range") != Some(expected.as_str()) {
        return Err(Failure::ByteRangeMismatch("content_range_416"));
    }
    Ok(())
}

fn verify_if_range(
    matched: &HttpResponse,
    mismatched: &HttpResponse,
    full_body: &[u8],
    start: usize,
    end: usize,
) -> Result<()> {
    if matched.status != 206 || matched.body != full_body[start..=end] {
        return Err(Failure::ByteRangeMismatch("if_range_match"));
    }
    if mismatched.status != 200 || mismatched.body != full_body {
        return Err(Failure::ByteRangeMismatch("if_range_mismatch"));
    }
    Ok(())
}

fn verify_egress_policy_covers_plan(
    policy_bytes: &[u8],
    spec: &PublicEgressSpec,
) -> Result<PublicEgressPolicy> {
    let actual_sha = hex::encode(Sha256::digest(policy_bytes));
    if actual_sha != spec.policy_sha256 {
        return Err(Failure::EgressFenceMismatch("policy_sha256"));
    }
    let policy = PublicEgressPolicy::parse(policy_bytes)
        .map_err(|_| Failure::EgressFenceMismatch("policy_parse"))?;
    for raw in &spec.denied_targets {
        let addr =
            parse_ipv4_endpoint(raw).ok_or(Failure::EgressFenceMismatch("denied_target_parse"))?;
        if !policy.denies(*addr.ip()) {
            return Err(Failure::EgressFenceMismatch("denied_target_uncovered"));
        }
    }
    let uplink = policy.uplink();
    for ip in [uplink.source, uplink.gateway] {
        if !spec
            .denied_targets
            .iter()
            .any(|raw| parse_ipv4_endpoint(raw).is_some_and(|ep| *ep.ip() == ip))
        {
            return Err(Failure::EgressFenceMismatch("uplink_witness_missing"));
        }
    }
    for raw in [&spec.public_tcp_target, &spec.public_dns_target] {
        let addr =
            parse_ipv4_endpoint(raw).ok_or(Failure::EgressFenceMismatch("public_target_parse"))?;
        if policy.denies(*addr.ip()) {
            return Err(Failure::EgressFenceMismatch("public_target_denied"));
        }
    }
    Ok(policy)
}

/// Offline restore of a settled SQLite backup into a disposable target
/// directory without mutating the settled source file, verifying both the
/// backup integrity report and the presence of the synthetic SQLite event.
fn verify_backup_restore_event(
    settled_path: &Path,
    expected_sha256: &str,
    expected_size: u64,
    restore_dir: &Path,
    query: &str,
    expected_event_nonce: &str,
) -> Result<Value> {
    if !valid_readonly_sqlite_query(query) || !opaque_nonzero_hex(expected_event_nonce, 32) {
        return Err(Failure::BackupRestoreMismatch("restore_query_invalid"));
    }
    let verify_source = || {
        arx_guest::compose_backup::verify_portable_sqlite_file(settled_path, expected_size)
            .is_ok_and(|meta| meta.total_bytes == expected_size && meta.digest == expected_sha256)
    };
    if !verify_source() {
        return Err(Failure::BackupRestoreMismatch("settled_source_pin"));
    }
    let report = restore_sqlite_offline(
        settled_path,
        restore_dir,
        "restored.sqlite3",
        expected_sha256,
        expected_size,
    )
    .map_err(|_| Failure::BackupRestoreMismatch("restore_offline_failed"))?;
    if report.size_bytes != expected_size || report.digest != expected_sha256 {
        return Err(Failure::BackupRestoreMismatch("restore_report_mismatch"));
    }
    let open_error = || Failure::BackupRestoreMismatch("restored_db_open");
    let path = &report.path;
    #[cfg(unix)]
    let path = {
        // SQLite's Unix VFS also rejects symlink ancestors under NOFOLLOW.
        // Resolve OS temp-root aliases while leaving the database leaf unfollowed.
        let parent = path.parent().ok_or_else(open_error)?;
        let leaf = path.file_name().ok_or_else(open_error)?;
        if !fs::symlink_metadata(parent)
            .map_err(|_| open_error())?
            .is_dir()
        {
            return Err(open_error());
        }
        fs::canonicalize(parent)
            .map_err(|_| open_error())?
            .join(leaf)
    };
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| open_error())?;
    let mut stmt = db
        .prepare(query)
        .map_err(|_| Failure::BackupRestoreMismatch("restored_query_prepare"))?;
    let mut rows = stmt
        .query([])
        .map_err(|_| Failure::BackupRestoreMismatch("restored_query_exec"))?;
    let mut found = false;
    let mut scanned = 0usize;
    while let Some(row) = rows
        .next()
        .map_err(|_| Failure::BackupRestoreMismatch("restored_query_row"))?
    {
        scanned += 1;
        if scanned > 10_000 {
            return Err(Failure::BackupRestoreMismatch("restored_query_bound"));
        }
        if let Ok(value) = row.get::<_, String>(0)
            && value.contains(expected_event_nonce)
        {
            found = true;
            break;
        }
    }
    drop(rows);
    drop(stmt);
    drop(db);
    if !found {
        return Err(Failure::BackupRestoreMismatch("restored_event_missing"));
    }

    if !verify_source() {
        return Err(Failure::BackupRestoreMismatch("settled_source_mutated"));
    }
    Ok(json!({
        "restored_size_bytes": report.size_bytes,
        "restored_sha256": report.digest,
        "quick_check_ok": true,
        "event_found": true,
        "settled_source_unchanged": true,
    }))
}

fn verify_public_witness(request: &Value, response: &Value) -> Result<()> {
    check(
        request["version"] == 1
            && response["version"] == 1
            && opaque(&request["challenge"], 32).is_some()
            && response["challenge"] == request["challenge"]
            && response["phase"] == request["phase"],
        "public_witness_binding",
    )?;
    let reference = &request["reference"];
    if request["phase"] == "cleanup" {
        return check(
            response["connector_stopped"] == true
                && response["owned_dns_absent"] == 2
                && response["names_no_longer_serve"] == 2,
            "public_cleanup_witness",
        );
    }
    let body_hashes = response["marker_body_sha256"]
        .as_array()
        .ok_or(Failure::Check("public_markers"))?;
    check(
        matches!(request["phase"].as_str(), Some("first" | "republished"))
            && response["matching_markers"] == 2
            && body_hashes.len() == 2
            && body_hashes.iter().all(|v| opaque(v, 64).is_some())
            && body_hashes[0] != body_hashes[1],
        "public_markers",
    )?;
    let start = reference["start"]
        .as_u64()
        .ok_or(Failure::Check("public_range"))?;
    let end = reference["end"]
        .as_u64()
        .ok_or(Failure::Check("public_range"))?;
    let total = reference["total"]
        .as_u64()
        .ok_or(Failure::Check("public_range"))?;
    check(
        start <= end
            && end < total
            && response["full_status"] == 200
            && response["full_sha256"] == reference["full_sha256"]
            && response["partial_status"] == 206
            && response["partial_sha256"] == reference["range_sha256"]
            && response["content_range"] == format!("bytes {start}-{end}/{total}")
            && response["content_length"] == end - start + 1
            && response["accept_ranges"] == "bytes"
            && response["unsatisfiable_status"] == 416
            && response["unsatisfiable_range"] == format!("bytes */{total}"),
        "public_range",
    )
}

#[test]
fn external_witness_rejects_replay_bad_content_and_incomplete_cleanup() {
    let req = json!({"version":1,"phase":"first","challenge":"12".repeat(16),"reference":{"start":2,"end":3,"total":8,"full_sha256":"aa".repeat(32),"range_sha256":"ab".repeat(32)}});
    let res = json!({"version":1,"phase":"first","challenge":req["challenge"],"matching_markers":2,"marker_body_sha256":["cd".repeat(32),"ce".repeat(32)],"full_status":200,"full_sha256":req["reference"]["full_sha256"],"partial_status":206,"partial_sha256":req["reference"]["range_sha256"],"content_range":"bytes 2-3/8","content_length":2,"accept_ranges":"bytes","unsatisfiable_status":416,"unsatisfiable_range":"bytes */8"});
    assert_eq!(verify_public_witness(&req, &res), Ok(()));
    for field in [
        "phase",
        "challenge",
        "matching_markers",
        "partial_sha256",
        "content_range",
        "unsatisfiable_range",
    ] {
        let mut bad = res.clone();
        bad[field] = Value::Null;
        assert!(verify_public_witness(&req, &bad).is_err());
    }
    let mut req = req;
    req["phase"] = json!("cleanup");
    let mut off = json!({"version":1,"phase":"cleanup","challenge":req["challenge"],"connector_stopped":true,"owned_dns_absent":2,"names_no_longer_serve":2});
    assert_eq!(verify_public_witness(&req, &off), Ok(()));
    off["owned_dns_absent"] = json!(1);
    assert!(verify_public_witness(&req, &off).is_err());
}

/// The owner pins the separately reviewed Design §2 receipt. A request sent by a UDP socket
/// alone is not a delivery witness. The qualification records replies and destination captures.
fn verify_network_receipt(plan: &StaticPlan, bytes: &[u8]) -> Result<()> {
    check(
        bytes.len() <= 65_536 && hex::encode(Sha256::digest(bytes)) == plan.network_receipt_sha256,
        "network_receipt_pin",
    )?;
    let r: Value =
        serde_json::from_slice(bytes).map_err(|_| Failure::Check("network_receipt_format"))?;
    let spec = &plan.public_egress;
    check(
        r["version"] == 1
            && r["qualification_sha256"] == plan.qualification_sha256
            && r["m4_qualification_sha256"] == plan.m4_qualification_sha256
            && r["recipe_sha256"] == plan.static_vm.recipe_sha256
            && r["policy_sha256"] == spec.policy_sha256
            && r["qemu_sha256"] == spec.qemu_sha256,
        "network_receipt_binding",
    )?;
    check(
        r["static"]["tcp_target"] == spec.public_tcp_target
            && r["static"]["dns_target"] == spec.public_dns_target
            && r["static"]["public_tcp_connected"] == true
            && r["static"]["udp_dns_answer"] == true
            && r["static"]["ipv6_disabled"] == true
            && r["static"]["root_capable_fixture"] == true
            && r["static"]["route_drift_closed"] == true,
        "static_network_controls",
    )?;
    let rows = r["static"]["denied"]
        .as_array()
        .ok_or(Failure::Check("network_denials"))?;
    check(
        rows.len() == spec.denied_targets.len()
            && spec.denied_targets.iter().all(|target| {
                rows.iter()
                    .filter(|row| {
                        row["target"] == *target
                            && row["tcp_connected"] == false
                            && row["udp_replied"] == false
                            && row["witness_tcp_arrivals"] == 0
                            && row["witness_udp_arrivals"] == 0
                    })
                    .count()
                    == 1
            }),
        "network_denials",
    )?;
    check(
        r["test"]["network_class"] == "restricted"
            && r["test"]["tcp_target"] == spec.public_tcp_target
            && r["test"]["dns_target"] == spec.public_dns_target
            && r["test"]["public_tcp_connected"] == false
            && r["test"]["udp_dns_answer"] == false
            && r["test"]["witness_tcp_arrivals"] == 0
            && r["test"]["witness_udp_arrivals"] == 0,
        "test_network_isolation",
    )
}

#[test]
fn reviewed_receipt_requires_each_transport_and_test_isolation() {
    let mut plan = StaticPlan::parse_bytes(sample_plan_json().to_string().as_bytes()).unwrap();
    let e = &plan.public_egress;
    let denied:Vec<_>=e.denied_targets.iter().map(|t|json!({"target":t,"tcp_connected":false,"udp_replied":false,"witness_tcp_arrivals":0,"witness_udp_arrivals":0})).collect();
    let r = json!({"version":1,"qualification_sha256":plan.qualification_sha256,"m4_qualification_sha256":plan.m4_qualification_sha256,"recipe_sha256":plan.static_vm.recipe_sha256,"policy_sha256":e.policy_sha256,"qemu_sha256":e.qemu_sha256,"static":{"tcp_target":e.public_tcp_target,"dns_target":e.public_dns_target,"public_tcp_connected":true,"udp_dns_answer":true,"ipv6_disabled":true,"root_capable_fixture":true,"route_drift_closed":true,"denied":denied},"test":{"network_class":"restricted","tcp_target":e.public_tcp_target,"dns_target":e.public_dns_target,"public_tcp_connected":false,"udp_dns_answer":false,"witness_tcp_arrivals":0,"witness_udp_arrivals":0}});
    let mut verify = |value: &Value| {
        let bytes = serde_json::to_vec(value).unwrap();
        plan.network_receipt_sha256 = hex::encode(Sha256::digest(&bytes));
        verify_network_receipt(&plan, &bytes)
    };
    assert_eq!(verify(&r), Ok(()));
    for path in [
        "/static/denied/0/tcp_connected",
        "/static/denied/1/udp_replied",
        "/test/udp_dns_answer",
        "/static/denied/0/witness_udp_arrivals",
        "/qemu_sha256",
    ] {
        let mut bad = r.clone();
        *bad.pointer_mut(path).unwrap() = json!(true);
        assert!(verify(&bad).is_err(), "{path}");
    }
}

/// Save a strictly allowlisted projection, never hostnames, script bodies,
/// raw paths, credentials, IPs, or arbitrary MCP text.
fn operation_evidence(response: &Value) -> Value {
    let mut out = json!({});
    if let Some(sha) = opaque(&response["sha256"], 64) {
        out["sha256"] = json!(sha);
    }
    for key in ["exit_code", "revision", "size_bytes"] {
        if let Some(number) = response[key].as_u64() {
            out[key] = json!(number);
        }
    }
    for (key, values) in [
        (
            "action",
            &[
                "template_build",
                "static_create",
                "start",
                "stop",
                "run_script",
                "publish",
                "unpublish",
                "compose_deploy",
                "compose_start",
                "compose_stop",
                "compose_down",
                "compose_backup",
            ][..],
        ),
        (
            "state",
            &[
                "pending",
                "succeeded",
                "failed",
                "interrupted",
                "unresolved",
            ][..],
        ),
        ("cleanup", &["confirmed", "cleanup_unconfirmed"][..]),
        (
            "script_status",
            &[
                "exited",
                "unavailable",
                "timed_out",
                "launch_failed",
                "terminated",
            ][..],
        ),
    ] {
        if let Some(value) = values.iter().find(|value| response[key] == **value) {
            out[key] = json!(value);
        }
    }
    if let Some(code) = reason(&response["reason"]) {
        out["reason"] = json!(code);
    }
    out
}

fn publish_status_evidence(response: &Value) -> Value {
    let mut out = json!({});
    for phase in [
        "unpublished",
        "publishing",
        "published",
        "revoking",
        "cleanup_pending",
    ] {
        if response["phase"] == phase {
            out["phase"] = json!(phase);
        }
    }
    if let Some(count) = response["host_count"].as_u64() {
        out["host_count"] = json!(count);
    }
    if let Some(code) = reason(&response["reason"]) {
        out["reason"] = json!(code);
    }
    out
}

fn compose_status_evidence(response: &Value) -> Value {
    let mut out = json!({});
    for desired in ["none", "started", "stopped", "down"] {
        if response["desired"] == desired {
            out["desired"] = json!(desired);
        }
    }
    if let Some(rev) = response["revision"].as_u64() {
        out["revision"] = json!(rev);
    }
    if let Some(current) = response["current_boot"].as_bool() {
        out["current_boot"] = json!(current);
    }
    out["recovery_required"] = json!(!response["recovery"].is_null());
    out
}

fn assert_evidence_sanitized(evidence: &Value, forbidden_literals: &[&str]) -> Result<()> {
    let rendered =
        serde_json::to_string(evidence).map_err(|_| Failure::Check("evidence_serialize"))?;
    for literal in forbidden_literals {
        if !literal.is_empty() && rendered.contains(literal) {
            return Err(Failure::Check("evidence_sanitization_leak"));
        }
    }
    Ok(())
}

async fn poll<T, F: std::future::Future<Output = Result<Option<T>>>>(
    name: &'static str,
    budget: Duration,
    interval: Duration,
    mut query: impl FnMut() -> F,
) -> Result<T> {
    let deadline = Instant::now() + budget;
    timeout_at(deadline, async {
        loop {
            if let Some(result) = query().await? {
                return Ok(result);
            }
            sleep(interval).await;
        }
    })
    .await
    .map_err(|_| Failure::Timeout(name))?
}

fn sample_plan_json() -> Value {
    json!({
        "daemon": "D:/acceptance/bin/arxd.exe",
        "daemon_sha256": "11".repeat(32),
        "origin_ca_path": "D:/acceptance/private/origin-ca.der",
        "daemon_config": "D:/acceptance/private/daemon.json",
        "client_config": "D:/acceptance/private/client.json",
        "store": {
            "volume_guid": r"\\?\Volume{12345678-1234-1234-1234-1234567890ab}\",
            "volume_serial": "0123456789abcdef",
            "directory_file_id": "22".repeat(16)
        },
        "qualification_sha256": "33".repeat(32),
        "m4_qualification_sha256": "34".repeat(32),
        "evidence_dir": "D:/acceptance/evidence",
        "backup_root": "D:/acceptance/backups",
        "restore_dir": "D:/acceptance/restore",
        "network_receipt": "D:/acceptance/private/network.json",
        "network_receipt_sha256": "73".repeat(32),
        "idle_delay_s": 0,
        "static_vm": {
            "vm_id": "71".repeat(16),
            "generation": "72".repeat(16),
            "name": "calm-life-static",
            "recipe_id": "44".repeat(16),
            "recipe_sha256": "55".repeat(32)
        },
        "public_egress": {
            "manifest": EGRESS_MANIFEST,
            "qemu_sha256": "66".repeat(32),
            "receipt_sha256": "77".repeat(32),
            "policy_path": "D:/acceptance/private/egress-policy.txt",
            "policy_sha256": "88".repeat(32),
            "public_tcp_target": "1.1.1.1:443",
            "public_dns_target": "1.1.1.1:53",
            "denied_targets": [
                "10.0.2.2:443",
                "10.0.2.3:53",
                "169.254.169.254:80",
                "100.64.0.1:443",
                "192.168.1.1:443", "192.168.1.50:443",
                "203.0.114.10:443", "203.0.114.20:443"
            ]
        },
        "compose": {
            "primary": {
                "project_id": "calm-life",
                "release_id": "aa".repeat(16),
                "hostname": "calm.example.invalid",
                "marker_path": "/health",
                "expected_marker": "calm-life-ok",
                "media_path": "/media/intro.mp4",
                "range_start": 4,
                "range_end": 15,
                "event_post_path": "/api/events",
                "event_query_path": "/api/admin/events",
                "admin_auth_path": "/api/admin/events",
                "admin_password_file": "D:/acceptance/private/bearer",
                "sqlite_table_query": "SELECT payload FROM events ORDER BY id DESC LIMIT 64"
            },
            "secondary": {
                "project_id": "synthetic-two",
                "release_id": "bb".repeat(16),
                "hostname": "second.example.invalid",
                "marker_path": "/health",
                "expected_marker": "synthetic-second-ok"
            }
        }
    })
}

#[test]
fn plan_parsing_validates_bounds_pins_egress_and_compose_targets() {
    let base = sample_plan_json();
    let parsed = StaticPlan::parse_bytes(base.to_string().as_bytes()).unwrap();
    assert_eq!(parsed.static_vm.name, "calm-life-static");
    assert_eq!(parsed.public_egress.manifest, EGRESS_MANIFEST);

    // Unknown field is rejected.
    let mut unknown = base.clone();
    unknown["unexpected"] = json!(true);
    assert_eq!(
        StaticPlan::parse_bytes(unknown.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("plan_format")
    );

    for field in [
        "daemon",
        "daemon_config",
        "client_config",
        "evidence_dir",
        "backup_root",
        "restore_dir",
        "origin_ca_path",
    ] {
        let mut wrong = base.clone();
        wrong[field] = json!("C:/private/input");
        assert_eq!(
            StaticPlan::parse_bytes(wrong.to_string().as_bytes()).err(),
            Some(Failure::Check("d_drive_path"))
        );
    }

    // Missing or malformed qualification digest is rejected.
    let mut missing_qual = base.clone();
    missing_qual
        .as_object_mut()
        .unwrap()
        .remove("qualification_sha256");
    assert!(StaticPlan::parse_bytes(missing_qual.to_string().as_bytes()).is_err());

    let mut bad_qual = base.clone();
    bad_qual["qualification_sha256"] = json!("not-hex");
    assert_eq!(
        StaticPlan::parse_bytes(bad_qual.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("qualification_sha256")
    );

    // Invalid static VM name or Windows TEST recipe ID is rejected.
    let mut bad_name = base.clone();
    bad_name["static_vm"]["name"] = json!("-bad.name");
    assert_eq!(
        StaticPlan::parse_bytes(bad_name.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("static_vm_name")
    );

    let mut windows_recipe = base.clone();
    windows_recipe["static_vm"]["recipe_id"] = json!(WINDOWS_TEST_RECIPE_ID);
    assert_eq!(
        StaticPlan::parse_bytes(windows_recipe.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("compose_recipe_id")
    );

    // Stock QEMU 11.1.3 executable or receipt pin is refused.
    let mut stock_qemu = base.clone();
    stock_qemu["public_egress"]["qemu_sha256"] = json!(hex::encode(STOCK_QEMU_1113.sha256));
    assert_eq!(
        StaticPlan::parse_bytes(stock_qemu.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("public_egress_qemu_sha256")
    );

    let mut stock_receipt = base.clone();
    stock_receipt["public_egress"]["receipt_sha256"] =
        json!(hex::encode(STOCK_RECEIPT_1113_SHA256));
    assert_eq!(
        StaticPlan::parse_bytes(stock_receipt.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("public_egress_receipt_sha256")
    );

    // Private/slirp public witness target is rejected.
    let mut private_public = base.clone();
    private_public["public_egress"]["public_tcp_target"] = json!("10.0.2.2:443");
    assert_eq!(
        StaticPlan::parse_bytes(private_public.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("public_tcp_target")
    );

    // Missing slirp DNS alias or CGNAT in denied_targets is rejected.
    let mut missing_alias = base.clone();
    missing_alias["public_egress"]["denied_targets"] = json!([
        "10.0.2.2:443",
        "169.254.169.254:80",
        "100.64.0.1:443",
        "192.168.1.1:443"
    ]);
    assert_eq!(
        StaticPlan::parse_bytes(missing_alias.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("denied_targets_coverage")
    );

    // Same hostname or cross-zone hostnames or identical markers are rejected.
    let mut same_host = base.clone();
    same_host["compose"]["secondary"]["hostname"] = json!("calm.example.invalid");
    assert_eq!(
        StaticPlan::parse_bytes(same_host.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("compose_hostnames")
    );

    let mut cross_zone = base.clone();
    cross_zone["compose"]["secondary"]["hostname"] = json!("second.other.invalid");
    assert_eq!(
        StaticPlan::parse_bytes(cross_zone.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("compose_hostnames")
    );

    let mut same_marker = base.clone();
    same_marker["compose"]["secondary"]["expected_marker"] = json!("calm-life-ok");
    assert_eq!(
        StaticPlan::parse_bytes(same_marker.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("compose_distinct_markers")
    );

    // Inverted byte range and mutating SQLite query are rejected.
    let mut bad_range = base.clone();
    bad_range["compose"]["primary"]["range_start"] = json!(20);
    bad_range["compose"]["primary"]["range_end"] = json!(10);
    assert_eq!(
        StaticPlan::parse_bytes(bad_range.to_string().as_bytes())
            .err()
            .unwrap(),
        Failure::Check("compose_byte_range")
    );

    for bad_query in [
        "DELETE FROM events",
        "SELECT payload FROM events; DROP TABLE events",
        "PRAGMA integrity_check",
    ] {
        let mut mutated = base.clone();
        mutated["compose"]["primary"]["sqlite_table_query"] = json!(bad_query);
        assert_eq!(
            StaticPlan::parse_bytes(mutated.to_string().as_bytes())
                .err()
                .unwrap(),
            Failure::Check("compose_sqlite_query")
        );
    }
}

#[test]
fn discovery_enforces_static_compose_schemas_and_forbids_delete_tools() {
    assert_eq!(discover(Err(Failure::Discovery)), Err(Failure::Discovery));
    let mut tools: Vec<_> = schemas()
        .into_iter()
        .map(|(name, schema)| json!({"name":name,"inputSchema":schema}))
        .collect();
    assert_eq!(discover(Ok(json!({"tools":tools}))), Ok(()));

    // Duplicate tool fails.
    tools.push(tools[0].clone());
    assert_eq!(
        discover(Ok(json!({"tools":tools}))),
        Err(Failure::Discovery)
    );
    tools.pop();

    // Schema drift fails.
    let backup_idx = tools
        .iter()
        .position(|t| t["name"] == "compose_backup")
        .unwrap();
    tools[backup_idx]["inputSchema"]["additionalProperties"] = json!(true);
    assert_eq!(
        discover(Ok(json!({"tools":tools}))),
        Err(Failure::SchemaChanged("compose_backup"))
    );
    tools[backup_idx]["inputSchema"]["additionalProperties"] = json!(false);

    // Removing any required tool fails.
    for required in [
        "static_vm_create",
        "compose_deploy",
        "compose_backup",
        "publish_site",
        "publish_status",
        "vm_snapshot_delete",
    ] {
        let pruned: Vec<_> = tools
            .iter()
            .filter(|t| t["name"] != required)
            .cloned()
            .collect();
        assert_eq!(
            discover(Ok(json!({"tools":pruned}))),
            Err(Failure::MissingTool(required))
        );
    }

    // Exposing any forbidden agent delete/restore/host tool fails closed.
    for forbidden in FORBIDDEN_TOOLS {
        let mut exposed = tools.clone();
        exposed.push(json!({"name":forbidden,"inputSchema":{"type":"object"}}));
        assert_eq!(
            discover(Ok(json!({"tools":exposed}))),
            Err(Failure::ForbiddenToolExposed(forbidden))
        );
    }
}

#[test]
fn refusal_mapping_preserves_closed_reasons_and_redacts_unknown_values() {
    let closed = |r: &str| json!({"capability":{"available":false,"reason":r}});
    for expected in [
        "qualification_missing",
        "network_fence_unverified",
        "disabled",
        "unsupported_platform",
    ] {
        assert_eq!(
            assert_admitted(&closed(expected)),
            Err(Failure::AdmissionClosed(expected))
        );
    }
    assert_eq!(
        assert_admitted(&closed(r"D:\private\secret.dpapi")),
        Err(Failure::AdmissionClosed("unrecognized_refusal"))
    );
    assert!(assert_admitted(&json!({"capability":{"available":"true"}})).is_err());

    assert_eq!(
        assert_refused(
            &json!({"ok":false,"reason":"public_sites_disabled"}),
            "public_sites_disabled"
        ),
        Ok("public_sites_disabled")
    );
    assert_eq!(
        assert_refused(
            &json!({"ok":true,"operation_id":"aa".repeat(16)}),
            "public_sites_disabled"
        ),
        Err(Failure::RefusalMismatch("public_sites_disabled"))
    );
    assert_eq!(
        assert_refused(
            &json!({"ok":false,"reason":"not_found"}),
            "static_delete_forbidden"
        ),
        Err(Failure::RefusalMismatch("not_found"))
    );
    assert_eq!(
        assert_refused(
            &json!({"ok":false,"reason":r"D:\private\leak"}),
            "static_delete_forbidden"
        ),
        Err(Failure::RefusalMismatch("unrecognized_refusal"))
    );

    for (tool, expected) in [
        ("static_vm_delete", "static_delete_forbidden"),
        ("vm_delete", "static_delete_forbidden"),
        ("backup_delete", "static_delete_forbidden"),
        ("vm_backup_delete", "static_delete_forbidden"),
        ("compose_backup_delete", "static_backup_delete_forbidden"),
        ("compose_volume_delete", "static_backup_delete_forbidden"),
        ("compose_project_delete", "static_backup_delete_forbidden"),
        ("compose_backup_restore", "static_backup_delete_forbidden"),
        ("vm_snapshot_delete", "static_backup_delete_forbidden"),
        ("run_command", "host_execution_disabled"),
        ("discover_tools", "host_execution_disabled"),
    ] {
        assert_eq!(expected_delete_refusal(tool), Ok(expected));
        assert_eq!(
            assert_refused(&json!({"ok":false,"reason":expected}), expected),
            Ok(expected)
        );
    }
    assert!(expected_delete_refusal("unknown_tool").is_err());
}

#[test]
fn qualification_receipt_requires_daemon_attestation_and_non_stock_egress() {
    let plan = StaticPlan::parse_bytes(sample_plan_json().to_string().as_bytes()).unwrap();
    let mut catalog = json!({"capability":{"available":true}});
    assert_eq!(
        static_qualification_digest(&catalog, &plan),
        Err(Failure::DaemonQualificationEvidenceUnavailable)
    );

    catalog["capability"]["selected_test_qualification"] = json!({"sha256":"35".repeat(32)});
    assert_eq!(
        static_qualification_digest(&catalog, &plan),
        Err(Failure::DaemonQualificationEvidenceMismatch)
    );
    catalog["capability"]["selected_test_qualification"]["sha256"] =
        json!(plan.m4_qualification_sha256);
    catalog["capability"]["selected_static_qualification"] = json!({
        "sha256": plan.qualification_sha256,
        "compose_recipe": {
            "recipe_id": plan.static_vm.recipe_id,
            "recipe_sha256": plan.static_vm.recipe_sha256
        },
        "public_egress": {
            "manifest": EGRESS_MANIFEST,
            "qemu_sha256": plan.public_egress.qemu_sha256,
            "receipt_sha256": plan.public_egress.receipt_sha256,
            "policy_sha256": plan.public_egress.policy_sha256,
            "network_qualified": true
        }
    });
    assert_eq!(
        static_qualification_digest(&catalog, &plan),
        Ok(plan.qualification_sha256.as_str())
    );

    for (label, mutate) in [
        (
            "wrong_digest",
            Box::new(|c: &mut Value| {
                c["capability"]["selected_static_qualification"]["sha256"] = json!("ff".repeat(32));
            }) as Box<dyn Fn(&mut Value)>,
        ),
        (
            "wrong_recipe",
            Box::new(|c: &mut Value| {
                c["capability"]["selected_static_qualification"]["compose_recipe"]["recipe_sha256"] =
                    json!("ee".repeat(32));
            }),
        ),
        (
            "stock_qemu",
            Box::new(|c: &mut Value| {
                c["capability"]["selected_static_qualification"]["public_egress"]["qemu_sha256"] =
                    json!(hex::encode(STOCK_QEMU_1113.sha256));
            }),
        ),
        (
            "network_unqualified",
            Box::new(|c: &mut Value| {
                c["capability"]["selected_static_qualification"]["public_egress"]["network_qualified"] =
                    json!(false);
            }),
        ),
        (
            "wrong_manifest",
            Box::new(|c: &mut Value| {
                c["capability"]["selected_static_qualification"]["public_egress"]["manifest"] =
                    json!("qemu-11.1.3-windows-x64");
            }),
        ),
    ] {
        let mut wrong = catalog.clone();
        mutate(&mut wrong);
        assert_eq!(
            static_qualification_digest(&wrong, &plan),
            Err(Failure::DaemonQualificationEvidenceMismatch),
            "{label}"
        );
    }
}

#[test]
fn static_origin_witness_requires_bound_tls_port_ca_and_public_egress() {
    let vm = "11".repeat(16);
    let generation = "22".repeat(16);
    let start_op = "33".repeat(16);
    let witness_id = "44".repeat(16);
    let ca_sha = "55".repeat(32);
    let policy_sha = "88".repeat(32);

    let mut status = json!({"state":"running","ready":true,"web_health":"healthy"});
    assert_eq!(
        static_origin_witness(&status, &vm, &generation, &start_op, &policy_sha),
        Err(Failure::StaticOriginEvidenceUnavailable)
    );

    status["static_origin_witness"] = json!({
        "source": "static_origin_tls",
        "vm_id": vm,
        "generation": generation,
        "operation_id": start_op,
        "witness_id": witness_id,
        "origin_port": ORIGIN_PORT_FIRST,
        "origin_server_name": format!("arx-{generation}.invalid"),
        "origin_ca_sha256": ca_sha,
        "network_class": "public_egress",
        "policy_sha256": policy_sha,
    });
    let witness = static_origin_witness(&status, &vm, &generation, &start_op, &policy_sha).unwrap();
    assert_eq!(witness.origin_port, ORIGIN_PORT_FIRST);
    assert_eq!(witness.origin_ca_sha256, ca_sha);

    for (field, value) in [
        ("vm_id", json!(generation)),
        ("generation", json!(vm)),
        ("operation_id", json!(vm)),
        ("witness_id", json!(start_op)),
        ("origin_port", json!(ORIGIN_PORT_FIRST - 1)),
        ("origin_port", json!(ORIGIN_PORT_LAST + 1)),
        ("origin_server_name", json!("calm.example.invalid")),
        ("network_class", json!("restricted_origin")),
        ("policy_sha256", json!("99".repeat(32))),
    ] {
        let mut wrong = status.clone();
        wrong["static_origin_witness"][field] = value;
        assert_eq!(
            static_origin_witness(&wrong, &vm, &generation, &start_op, &policy_sha),
            Err(Failure::StaticOriginEvidenceMismatch),
            "{field}"
        );
    }
}

#[tokio::test]
async fn verified_tls_origin_checks_two_hosts_unknown_host_and_206_416_ranges() {
    use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
    use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    const MEDIA: &[u8] = b"0123456789abcdef-CALM-LIFE-MP4-BYTES-fedcba9876543210";
    const ETAG: &str = "\"calm-life-v1\"";

    let generation = "ab".repeat(16);
    let origin_name = format!("arx-{generation}.invalid");
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
    let leaf_key = KeyPair::generate().unwrap();
    let leaf_cert = CertificateParams::new(vec![origin_name.clone()])
        .unwrap()
        .signed_by(&leaf_key, &ca)
        .unwrap();

    let server_config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf_cert.der().clone()],
                PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
            )
            .unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));

    let server = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(Ok(mut stream)) =
                    timeout(Duration::from_secs(2), acceptor.accept(socket)).await
                else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let Ok(Ok(n)) = timeout(Duration::from_secs(2), stream.read(&mut buf)).await else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let raw = String::from_utf8_lossy(&buf[..n]);
                let mut lines = raw.lines();
                let path = lines
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/");
                let mut host = "";
                let mut range: Option<&str> = None;
                let mut if_range: Option<&str> = None;
                for line in lines {
                    if let Some((k, v)) = line.split_once(':') {
                        let v = v.trim();
                        if k.eq_ignore_ascii_case("host") {
                            host = v;
                        } else if k.eq_ignore_ascii_case("range") {
                            range = Some(v);
                        } else if k.eq_ignore_ascii_case("if-range") {
                            if_range = Some(v);
                        }
                    }
                }
                let resp = match (host, path) {
                    ("calm.example.invalid", "/health") => {
                        let body = "service=calm-life-ok";
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    ("second.example.invalid", "/health") => {
                        let body = "service=synthetic-second-ok";
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    }
                    ("calm.example.invalid", "/media/intro.mp4") => {
                        let allow_range = if_range.is_none_or(|ir| ir == ETAG);
                        if let Some(spec) = range.filter(|_| allow_range) {
                            let bytes_spec = spec.strip_prefix("bytes=").unwrap_or("");
                            let (s, e) = bytes_spec.split_once('-').unwrap_or(("999", "999"));
                            let start = s.parse::<usize>().unwrap_or(usize::MAX);
                            let end = e.parse::<usize>().unwrap_or(usize::MAX);
                            if start < MEDIA.len() && start <= end {
                                let end = end.min(MEDIA.len() - 1);
                                let slice = std::str::from_utf8(&MEDIA[start..=end]).unwrap();
                                format!(
                                    "HTTP/1.1 206 Partial Content\r\nETag: {ETAG}\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{slice}",
                                    MEDIA.len(),
                                    slice.len()
                                )
                            } else {
                                format!(
                                    "HTTP/1.1 416 Range Not Satisfiable\r\nAccept-Ranges: bytes\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                    MEDIA.len()
                                )
                            }
                        } else {
                            let body = std::str::from_utf8(MEDIA).unwrap();
                            format!(
                                "HTTP/1.1 200 OK\r\nETag: {ETAG}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                MEDIA.len()
                            )
                        }
                    }
                    _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .into(),
                };
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    let ca_der = ca.der().as_ref();
    let primary = https_origin_request(
        port,
        &origin_name,
        ca_der,
        "GET",
        "/health",
        "calm.example.invalid",
        &[],
        None,
    )
    .await
    .unwrap();
    let secondary = https_origin_request(
        port,
        &origin_name,
        ca_der,
        "GET",
        "/health",
        "second.example.invalid",
        &[],
        None,
    )
    .await
    .unwrap();
    let unknown = https_origin_request(
        port,
        &origin_name,
        ca_der,
        "GET",
        "/health",
        "unknown.example.invalid",
        &[],
        None,
    )
    .await
    .unwrap();

    verify_two_host_responses(
        &primary,
        "calm-life-ok",
        &secondary,
        "synthetic-second-ok",
        &unknown,
    )
    .unwrap();
    assert_eq!(
        verify_two_host_responses(&primary, "calm-life-ok", &primary, "calm-life-ok", &unknown),
        Err(Failure::OriginResponseMismatch("hosts_not_distinct"))
    );
    assert_eq!(
        verify_two_host_responses(
            &primary,
            "calm-life-ok",
            &secondary,
            "synthetic-second-ok",
            &primary
        ),
        Err(Failure::OriginResponseMismatch("unknown_host_accepted"))
    );

    let full = https_origin_request(
        port,
        &origin_name,
        ca_der,
        "GET",
        "/media/intro.mp4",
        "calm.example.invalid",
        &[],
        None,
    )
    .await
    .unwrap();
    let partial = https_origin_request(
        port,
        &origin_name,
        ca_der,
        "GET",
        "/media/intro.mp4",
        "calm.example.invalid",
        &[("Range", "bytes=4-15")],
        None,
    )
    .await
    .unwrap();
    let unsatisfiable = https_origin_request(
        port,
        &origin_name,
        ca_der,
        "GET",
        "/media/intro.mp4",
        "calm.example.invalid",
        &[(
            "Range",
            &format!("bytes={}-{}", MEDIA.len(), MEDIA.len() + 10),
        )],
        None,
    )
    .await
    .unwrap();
    let if_range_hit = https_origin_request(
        port,
        &origin_name,
        ca_der,
        "GET",
        "/media/intro.mp4",
        "calm.example.invalid",
        &[("Range", "bytes=4-15"), ("If-Range", ETAG)],
        None,
    )
    .await
    .unwrap();
    let if_range_miss = https_origin_request(
        port,
        &origin_name,
        ca_der,
        "GET",
        "/media/intro.mp4",
        "calm.example.invalid",
        &[("Range", "bytes=4-15"), ("If-Range", "\"stale-etag\"")],
        None,
    )
    .await
    .unwrap();

    verify_byte_range_206(&partial, &full, 4, 15).unwrap();
    verify_byte_range_416(&unsatisfiable, full.body.len()).unwrap();
    verify_if_range(&if_range_hit, &if_range_miss, &full.body, 4, 15).unwrap();

    // Mutated partial body or missing Accept-Ranges fails closed.
    let mut bad_body = HttpResponse {
        status: partial.status,
        headers: partial.headers.clone(),
        body: partial.body.clone(),
    };
    bad_body.body[0] ^= 0xff;
    assert_eq!(
        verify_byte_range_206(&bad_body, &full, 4, 15),
        Err(Failure::ByteRangeMismatch("body_slice_206"))
    );
    assert_eq!(
        verify_byte_range_416(&full, full.body.len()),
        Err(Failure::ByteRangeMismatch("status_416"))
    );

    // Wrong SNI fails TLS handshake closed.
    assert_eq!(
        https_origin_request(
            port,
            "wrong-origin.invalid",
            ca_der,
            "GET",
            "/health",
            "calm.example.invalid",
            &[],
            None,
        )
        .await
        .unwrap_err(),
        Failure::OriginResponseMismatch("origin_tls_handshake")
    );

    server.abort();
}

#[test]
fn offline_backup_restore_verifies_wal_event_survival_and_preserves_settled_copy() {
    use arx_guest::compose_backup::backup_sqlite_online;

    let temp = tempfile::tempdir().unwrap();
    let live = temp.path().join("live");
    fs::create_dir(&live).unwrap();
    let live_db = live.join("calm-life.sqlite3");
    let backups = temp.path().join("backups");
    fs::create_dir(&backups).unwrap();
    let settled = backups.join("settled.sqlite3");
    let restore_root = temp.path().join("restore-root");
    fs::create_dir(&restore_root).unwrap();
    #[cfg(unix)]
    let restore_root = {
        // Model a symlink ancestor such as macOS /var even when TMPDIR is canonical.
        let alias = temp.path().join("restore-alias");
        std::os::unix::fs::symlink(&restore_root, &alias).unwrap();
        alias
    };
    let restore_dir = restore_root.join("restore");
    fs::create_dir(&restore_dir).unwrap();

    let nonce = "a5".repeat(16);
    {
        let conn = Connection::open(&live_db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE events (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute("INSERT INTO events(payload) VALUES (?1)", [&nonce])
            .unwrap();
        // Backup while WAL is active (simulating a live Calm Life SQLite DB).
        backup_sqlite_online(
            &live,
            "calm-life.sqlite3",
            &settled,
            1024 * 1024,
            std::time::Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
    }

    let settled_bytes = fs::read(&settled).unwrap();
    let settled_sha = hex::encode(Sha256::digest(&settled_bytes));
    let query = "SELECT payload FROM events ORDER BY id DESC LIMIT 64";

    let evidence = verify_backup_restore_event(
        &settled,
        &settled_sha,
        settled_bytes.len() as u64,
        &restore_dir,
        query,
        &nonce,
    )
    .unwrap();
    assert_eq!(evidence["event_found"], true);
    assert_eq!(evidence["settled_source_unchanged"], true);

    // Missing event nonce fails closed.
    let restore_dir_2 = temp.path().join("restore-missing");
    fs::create_dir(&restore_dir_2).unwrap();
    assert_eq!(
        verify_backup_restore_event(
            &settled,
            &settled_sha,
            settled_bytes.len() as u64,
            &restore_dir_2,
            query,
            &"f0".repeat(16),
        ),
        Err(Failure::BackupRestoreMismatch("restored_event_missing"))
    );

    // Digest mismatch fails closed before restore.
    let restore_dir_3 = temp.path().join("restore-bad-sha");
    fs::create_dir(&restore_dir_3).unwrap();
    assert_eq!(
        verify_backup_restore_event(
            &settled,
            &"00".repeat(32),
            settled_bytes.len() as u64,
            &restore_dir_3,
            query,
            &nonce,
        ),
        Err(Failure::BackupRestoreMismatch("settled_source_pin"))
    );
}

#[test]
fn public_egress_policy_check_enforces_slirp_private_and_enrolled_lan_denies() {
    let seal = |denies: &[&str]| -> Vec<u8> {
        let mut head = String::from(
            "version=1\ninterface_index=7\ninterface_luid=1689399632855040\nsource=192.168.1.50\ngateway=192.168.1.1\n",
        );
        for d in denies {
            head.push_str(&format!("deny={d}\n"));
        }
        let fp = hex::encode(Sha256::digest(head.as_bytes()));
        head.push_str(&format!("fingerprint={fp}\n"));
        head.into_bytes()
    };

    let mut plan = StaticPlan::parse_bytes(sample_plan_json().to_string().as_bytes()).unwrap();
    // Policy includes the enrolled public-numbered LAN / hairpin target (203.0.114.10/32).
    let good_policy = seal(&["192.168.1.0/24", "203.0.114.0/24"]);
    plan.public_egress.policy_sha256 = hex::encode(Sha256::digest(&good_policy));
    verify_egress_policy_covers_plan(&good_policy, &plan.public_egress).unwrap();

    let mut missing_host = plan.public_egress.clone();
    missing_host
        .denied_targets
        .retain(|target| target != "192.168.1.50:443");
    assert_eq!(
        verify_egress_policy_covers_plan(&good_policy, &missing_host),
        Err(Failure::EgressFenceMismatch("uplink_witness_missing"))
    );

    // Missing the enrolled public-numbered LAN prefix fails closed.
    let missing_lan = seal(&["192.168.1.0/24"]);
    let mut bad_plan = plan.public_egress.clone();
    bad_plan.policy_sha256 = hex::encode(Sha256::digest(&missing_lan));
    assert_eq!(
        verify_egress_policy_covers_plan(&missing_lan, &bad_plan),
        Err(Failure::EgressFenceMismatch("denied_target_uncovered"))
    );

    // Policy digest mismatch fails closed.
    assert_eq!(
        verify_egress_policy_covers_plan(&missing_lan, &plan.public_egress),
        Err(Failure::EgressFenceMismatch("policy_sha256"))
    );

    // Public witness target accidentally placed in deny= fails closed.
    let denies_public = seal(&["192.168.1.0/24", "203.0.114.0/24", "1.1.1.1/32"]);
    let mut denied_pub_plan = plan.public_egress.clone();
    denied_pub_plan.policy_sha256 = hex::encode(Sha256::digest(&denies_public));
    assert_eq!(
        verify_egress_policy_covers_plan(&denies_public, &denied_pub_plan),
        Err(Failure::EgressFenceMismatch("public_target_denied"))
    );
}

#[test]
fn evidence_projections_drop_hostnames_passwords_paths_ips_and_scripts() {
    let op = operation_evidence(&json!({
        "operation_id": "aa".repeat(16),
        "vm_id": "bb".repeat(16),
        "backup_id": "cc".repeat(16),
        "action": "compose_backup",
        "state": "succeeded",
        "size_bytes": 4096,
        "sha256": "dd".repeat(32),
        "hostname": "calm.example.invalid",
        "admin_password": "secret-admin-canary",
        "script": "curl https://secret",
        "path": r"D:\acceptance\private\secret.dpapi",
        "reason": r"D:\acceptance\private\leak"
    }));
    assert_eq!(
        op,
        json!({
            "action": "compose_backup",
            "state": "succeeded",
            "size_bytes": 4096,
            "sha256": "dd".repeat(32),
        })
    );

    let pub_ev = publish_status_evidence(&json!({
        "vm_id": "bb".repeat(16),
        "phase": "unpublished",
        "host_count": 0,
        "hosts": [],
        "hostname": "calm.example.invalid",
        "tunnel_id": "secret-tunnel"
    }));
    assert_eq!(pub_ev, json!({"phase": "unpublished", "host_count": 0}));

    let compose_ev = compose_status_evidence(&json!({
        "desired": "started",
        "revision": 2,
        "current_boot": true,
        "operation_id": null,
        "recovery": null,
        "observed": {"secret": "admin-canary"}
    }));
    assert_eq!(
        compose_ev,
        json!({"desired": "started", "revision": 2, "current_boot": true, "recovery_required": false})
    );

    let bundle = json!({"op": op, "pub": pub_ev, "compose": compose_ev});
    assert_evidence_sanitized(
        &bundle,
        &[
            "calm.example.invalid",
            "secret-admin-canary",
            r"D:\acceptance",
            "secret-tunnel",
            "admin-canary",
        ],
    )
    .unwrap();
    assert_eq!(
        assert_evidence_sanitized(
            &json!({"leaked": "calm.example.invalid"}),
            &["calm.example.invalid"]
        ),
        Err(Failure::Check("evidence_sanitization_leak"))
    );
}

#[tokio::test(start_paused = true)]
async fn polling_deadline_bounds_pending_results_and_stuck_calls() {
    let budget = Duration::from_millis(250);
    let started = Instant::now();
    let mut attempts = 0;
    let pending = poll::<(), _>(
        "compose_terminal",
        budget,
        Duration::from_millis(100),
        || {
            attempts += 1;
            std::future::ready(Ok(None))
        },
    )
    .await;
    assert_eq!(pending, Err(Failure::Timeout("compose_terminal")));
    assert_eq!(attempts, 3);
    assert!(started.elapsed() >= budget);
    let stalled = poll::<(), _>("compose_terminal", budget, budget, std::future::pending).await;
    assert_eq!(stalled, Err(Failure::Timeout("compose_terminal")));
}

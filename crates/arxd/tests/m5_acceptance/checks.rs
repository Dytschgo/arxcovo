//! Pure M5 acceptance checks. The owner journey and the CI tests both call
//! these functions. A dropped rejection or active-version check fails the
//! deterministic tests.

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Digest;

pub(super) type Result<T> = std::result::Result<T, Fail>;

pub(super) const GOOD_SCRIPT: &str = "exit 0\n";
pub(super) const BROKEN_SCRIPT: &str = "exit 7\n";
pub(super) const SCRIPT_NAME: &str = "m5-accept";
pub(super) const WINDOWS_RECIPE: &str = "3c2fe2069942ce6c6cebe5c3e700e758";

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Fail {
    Discovery,
    MissingTool(&'static str),
    SchemaChanged(&'static str),
    AdmissionClosed(&'static str),
    QualificationRequired,
    QualificationUnavailable,
    QualificationMismatch,
    Check(&'static str),
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    pub daemon: String,
    pub daemon_sha256: String,
    pub daemon_config: String,
    pub client_config: String,
    pub store: Store,
    pub qualification_sha256: Option<String>,
    pub evidence_dir: String,
    pub ubuntu: Recipe,
    pub windows: Recipe,
    #[serde(default)]
    pub idle_delay_s: u64,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Store {
    pub volume_guid: String,
    pub volume_serial: String,
    pub directory_file_id: String,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recipe {
    pub recipe_id: String,
    pub recipe_sha256: String,
}

pub(super) struct Observed {
    pub passed: Value,
    pub activate_good: Value,
    pub activated: Value,
    pub broken: Value,
    pub activate_broken: Value,
    pub list: Value,
    pub activity: Value,
}

pub(super) fn script_sha256(script: &str) -> String {
    hex::encode(sha2::Sha256::digest(script.as_bytes()))
}

pub(super) fn parse_plan(bytes: &[u8]) -> Result<Plan> {
    if bytes.len() > 65_536 {
        return Err(Fail::Check("input_bound"));
    }
    let mut plan: Plan = serde_json::from_slice(bytes).map_err(|_| Fail::Check("input_format"))?;
    let pin = plan
        .qualification_sha256
        .take()
        .ok_or(Fail::QualificationRequired)?;
    verify_hex(&pin, 64, "qualification_pin")?;
    plan.qualification_sha256 = Some(pin);
    if plan.idle_delay_s > 900 {
        return Err(Fail::Check("idle_delay_bound"));
    }
    for recipe in [&plan.ubuntu, &plan.windows] {
        verify_hex(&recipe.recipe_id, 32, "recipe_id")?;
        verify_hex(&recipe.recipe_sha256, 64, "recipe_sha256")?;
    }
    if plan.ubuntu.recipe_id == plan.windows.recipe_id
        || plan.ubuntu.recipe_id == "ffffffffffffffffffffffffffffffff"
        || plan.windows.recipe_id != WINDOWS_RECIPE
    {
        return Err(Fail::Check("test_recipe_ids"));
    }
    verify_hex(&plan.daemon_sha256, 64, "digest_pin")?;
    verify_hex(&plan.store.volume_serial, 16, "store_volume_pin")?;
    verify_hex(&plan.store.directory_file_id, 32, "store_directory_pin")?;
    Ok(plan)
}

fn verify_hex(value: &str, length: usize, name: &'static str) -> Result<()> {
    let ok = value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    ok.then_some(()).ok_or(Fail::Check(name))
}

fn opaque(value: &Value, len: usize) -> Option<&str> {
    value.as_str().filter(|id| {
        id.len() == len
            && id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn object(properties: Value, required: &[&str]) -> Value {
    let mut result = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false
    });
    if !required.is_empty() {
        result["required"] = json!(required);
    }
    result
}

pub(super) fn schemas() -> Vec<(&'static str, Value)> {
    let id = json!({"type": "string", "pattern": "^[a-f0-9]{32}$"});
    let cursor = json!({"type": "string", "pattern": "^[a-f0-9]{82}$"});
    let page = json!({
        "limit": {"type": "integer", "minimum": 1, "maximum": 20, "default": 20},
        "cursor": cursor
    });
    let mut history = page.clone();
    history["script_id"] = id.clone();
    let version_write = json!({"version_id": id, "request_id": id});
    let legacy_activity = json!({
        "type": "object",
        "properties": {
            "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 20},
            "before_id": {"type": "integer", "minimum": 1, "maximum": i64::MAX},
            "activity_id": {"type": "integer", "minimum": 1, "maximum": i64::MAX},
            "include_reads": {"type": "boolean", "default": false}
        },
        "required": [],
        "additionalProperties": false,
        "not": {"required": ["activity_id", "before_id"]}
    });
    let audit = object(
        json!({
            "source": {"const": "vault"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 20, "default": 20},
            "cursor": {"type": "string", "pattern": "^[a-f0-9]{82}$"}
        }),
        &["source"],
    );
    let catalog_cursor = json!({"type": "string", "maxLength": 96, "pattern": "^[A-Za-z0-9_-]+$"});
    vec![
        (
            "vm_catalog",
            json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "minimum": 1, "maximum": 20, "default": 20},
                    "cursor": catalog_cursor
                },
                "required": [],
                "additionalProperties": false
            }),
        ),
        (
            "list_activity",
            json!({"type": "object", "oneOf": [legacy_activity, audit]}),
        ),
        ("vault_list", object(page, &[])),
        (
            "vault_get",
            object(
                json!({
                    "version_id": id,
                    "include_content": {"type": "boolean", "default": false}
                }),
                &["version_id"],
            ),
        ),
        (
            "vault_add_version",
            object(
                json!({
                    "os": {"type": "string", "enum": ["windows", "ubuntu"]},
                    "name": {"type": "string", "pattern": "^[a-z0-9_-]{1,48}$"},
                    "script": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 4096,
                        "description": "At most 4096 UTF-8 bytes, with no NUL"
                    },
                    "request_id": id
                }),
                &["os", "name", "script", "request_id"],
            ),
        ),
        (
            "vault_test_version",
            object(version_write.clone(), &["version_id", "request_id"]),
        ),
        (
            "vault_activate",
            object(version_write, &["version_id", "request_id"]),
        ),
        ("vault_history", object(history, &["script_id"])),
        ("template_tools", object(json!({}), &[])),
    ]
}

pub(super) fn discover(page: Result<Value>) -> Result<()> {
    let page = page?;
    let tools = page["tools"].as_array().ok_or(Fail::Discovery)?;
    if !page["nextCursor"].is_null() || tools.len() > 64 {
        return Err(Fail::Discovery);
    }
    for (name, schema) in schemas() {
        let mut found = tools.iter().filter(|tool| tool["name"] == name);
        let tool = found.next().ok_or(Fail::MissingTool(name))?;
        if found.next().is_some() {
            return Err(Fail::Discovery);
        }
        if tool["inputSchema"] != schema {
            return Err(Fail::SchemaChanged(name));
        }
    }
    Ok(())
}

const ADMISSION: &[&str] = &[
    "disabled",
    "admission_closed",
    "draining",
    "shutdown",
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
    "recipe_unqualified",
    "owner_active",
    "cleanup_unconfirmed",
    "resource_unavailable",
    "expired",
    "busy",
];

const VAULT: &[&str] = &[
    "invalid",
    "invalid_cursor",
    "stale_key",
    "key_collision",
    "replay_full",
    "not_found",
    "not_draft",
    "tester_unavailable",
    "vm_admission_closed",
    "pass_required",
    "version_superseded",
    "busy",
    "script_limit",
    "version_limit",
    "byte_limit",
    "principal_version_limit",
    "principal_byte_limit",
    "state_unavailable",
    "exited",
    "cleanup_unconfirmed",
    "digest_mismatch",
    "test_inconclusive",
    "unavailable",
    "timed_out",
    "launch_failed",
    "terminated",
    "interrupted",
];

fn typed(value: &Value, allowed: &[&'static str]) -> Option<&'static str> {
    allowed.iter().copied().find(|code| value == *code)
}

pub(super) fn vault_reason(value: &Value) -> Option<&'static str> {
    typed(value, VAULT)
}

/// The M4 selected-record gate. A plan pin is required; the daemon digest is
/// accepted only when it matches that pin and both TEST recipe pins.
pub(super) fn qualification<'a>(
    catalog: &'a Value,
    expected_digest: &str,
    expected_recipes: [(&str, &str, &str); 2],
) -> Result<&'a str> {
    let capability = &catalog["capability"];
    match capability["available"].as_bool() {
        Some(true) => {}
        Some(false) => {
            return Err(Fail::AdmissionClosed(
                typed(&capability["reason"], ADMISSION).unwrap_or("unrecognized_refusal"),
            ));
        }
        None => return Err(Fail::Check("catalog_capability")),
    }
    let selected = &capability["selected_test_qualification"];
    let digest = opaque(&selected["sha256"], 64).ok_or(Fail::QualificationUnavailable)?;
    let scopes = selected["recipes"]
        .as_array()
        .ok_or(Fail::QualificationUnavailable)?;
    if expected_digest != digest
        || scopes.len() != expected_recipes.len()
        || !expected_recipes.into_iter().all(|(guest, id, pin)| {
            scopes
                .iter()
                .filter(|entry| {
                    entry["guest"] == guest
                        && entry["recipe_id"] == id
                        && entry["recipe_sha256"] == pin
                })
                .count()
                == 1
        })
    {
        return Err(Fail::QualificationMismatch);
    }
    Ok(digest)
}

fn keep_id(out: &mut Value, response: &Value, key: &str, len: usize) {
    if let Some(id) = opaque(&response[key], len) {
        out[key] = json!(id);
    }
}

/// Save a projection. Script bodies, content, authors and paths are omitted.
pub(super) fn project(response: &Value) -> Value {
    if let Some(rows) = response["entries"].as_array() {
        return json!({
            "entries": rows.iter().map(project).collect::<Vec<_>>()
        });
    }
    if let Some(rows) = response["activities"].as_array() {
        return json!({
            "activities": rows.iter().map(project_activity).collect::<Vec<_>>()
        });
    }
    let mut out = json!({});
    for key in ["version_id", "script_id", "operation_id"] {
        keep_id(&mut out, response, key, 32);
    }
    keep_id(&mut out, response, "sha256", 64);
    if response["os"] == "ubuntu" || response["os"] == "windows" {
        out["os"] = response["os"].clone();
    }
    if response["name"] == SCRIPT_NAME {
        out["name"] = json!(SCRIPT_NAME);
    }
    if let Some(state) = [
        "draft",
        "testing",
        "active",
        "rejected",
        "superseded",
        "test_inconclusive",
    ]
    .into_iter()
    .find(|state| response["state"] == *state)
    {
        out["state"] = json!(state);
    }
    if let Some(created) = response["created_at_ms"].as_i64() {
        out["created_at_ms"] = json!(created);
    }
    if response["ok"].is_boolean() {
        out["ok"] = response["ok"].clone();
    }
    if let Some(reason) = vault_reason(&response["reason"]) {
        out["reason"] = json!(reason);
    }
    if response["test"].is_object() {
        out["test"] = project_test(&response["test"]);
    }
    out
}

fn project_test(test: &Value) -> Value {
    let mut out = json!({});
    if let Some(status) = [
        "exited",
        "unavailable",
        "timed_out",
        "launch_failed",
        "terminated",
        "test_inconclusive",
        "interrupted",
    ]
    .into_iter()
    .find(|status| test["status"] == *status)
    {
        out["status"] = json!(status);
    }
    if let Some(exit) = test["exit_code"].as_u64() {
        out["exit_code"] = json!(exit);
    }
    if test["cleanup_confirmed"].is_boolean() {
        out["cleanup_confirmed"] = test["cleanup_confirmed"].clone();
    }
    out
}

fn project_activity(row: &Value) -> Value {
    let mut out = json!({});
    if ["vault_add_version", "vault_test_version", "vault_activate"]
        .into_iter()
        .any(|tool| row["tool"] == tool)
    {
        out["tool"] = row["tool"].clone();
    }
    if ["succeeded", "failed", "refused", "started"]
        .into_iter()
        .any(|outcome| row["outcome"] == outcome)
    {
        out["outcome"] = row["outcome"].clone();
    }
    if let Some(reason) = vault_reason(&row["reason"]) {
        out["reason"] = json!(reason);
    }
    if let Some(at) = row["at_ms"].as_i64() {
        out["at_ms"] = json!(at);
    }
    for key in ["version_id", "operation_id"] {
        keep_id(&mut out, row, key, 32);
    }
    keep_id(&mut out, row, "sha256", 64);
    out
}

fn leaked(value: &Value, sentinels: &[&str]) -> bool {
    match value {
        Value::String(text) => sentinels.iter().any(|sentinel| text.contains(sentinel)),
        Value::Array(items) => items.iter().any(|item| leaked(item, sentinels)),
        Value::Object(fields) => fields.iter().any(|(key, item)| {
            sentinels.iter().any(|sentinel| key.contains(sentinel)) || leaked(item, sentinels)
        }),
        _ => false,
    }
}

/// JSON encoding escapes newlines, so compare the decoded values.
pub(super) fn hidden(value: &Value, sentinels: &[&str]) -> Result<()> {
    if leaked(value, sentinels) {
        return Err(Fail::Check("evidence_leaked_script"));
    }
    Ok(())
}

fn check(condition: bool, name: &'static str) -> Result<()> {
    condition.then_some(()).ok_or(Fail::Check(name))
}

fn version_id(row: &Value) -> Result<&str> {
    opaque(&row["version_id"], 32).ok_or(Fail::Check("version_id"))
}

fn test_exit(row: &Value, exit: u64) -> Result<()> {
    let test = &row["test"];
    check(test["status"] == "exited", "test_status")?;
    check(test["exit_code"] == exit, "test_exit")?;
    check(
        test["cleanup_confirmed"] == json!(true),
        "cleanup_confirmed",
    )?;
    check(
        test.as_object().is_some_and(|fields| fields.len() == 3),
        "test_fields",
    )
}

fn row_matches(
    rows: &[Value],
    tool: &str,
    outcome: &str,
    version: Option<&str>,
    reason: Option<&str>,
) -> bool {
    rows.iter().any(|row| {
        row["tool"] == tool
            && row["outcome"] == outcome
            && version.is_none_or(|id| row["version_id"] == id)
            && reason.map_or(row.get("reason").is_none(), |code| row["reason"] == code)
    })
}

pub(super) fn accept_vault(observed: &Observed, good_sha: &str, broken_sha: &str) -> Result<Value> {
    let good = version_id(&observed.passed)?;
    let broken = version_id(&observed.broken)?;
    check(good != broken, "distinct_versions")?;
    check(observed.passed["os"] == "ubuntu", "script_os")?;
    check(observed.passed["name"] == SCRIPT_NAME, "script_name")?;
    check(observed.passed["sha256"] == good_sha, "good_sha256")?;
    check(observed.passed["state"] == "testing", "good_test")?;
    test_exit(&observed.passed, 0)?;
    check(observed.activate_good["ok"] == true, "good_activation")?;
    check(
        observed.activate_good["version_id"] == good
            && opaque(&observed.activate_good["operation_id"], 32).is_some(),
        "good_activation",
    )?;

    broken_rejected(observed, broken, broken_sha)?;
    previous_stays_active(observed, good)?;

    let activity = observed.activity["activities"]
        .as_array()
        .ok_or(Fail::Check("vault_activity"))?;
    check(activity.len() <= 20, "vault_activity")?;
    check(
        observed.activity.get("next_cursor").is_none()
            && observed.list.get("next_cursor").is_none(),
        "vault_page",
    )?;
    for (tool, outcome, version, reason) in [
        ("vault_add_version", "succeeded", Some(good), None),
        ("vault_test_version", "succeeded", Some(good), None),
        ("vault_activate", "succeeded", Some(good), None),
        ("vault_add_version", "succeeded", Some(broken), None),
        ("vault_test_version", "failed", Some(broken), Some("exited")),
        ("vault_activate", "refused", None, Some("pass_required")),
    ] {
        check(
            row_matches(activity, tool, outcome, version, reason),
            "vault_activity",
        )?;
    }

    let proof = json!({
        "good_version_id": good,
        "broken_version_id": broken,
        "good_sha256": good_sha,
        "broken_sha256": broken_sha,
        "passed": project(&observed.passed),
        "activated": project(&observed.activated),
        "broken": project(&observed.broken),
        "activate_broken": project(&observed.activate_broken),
        "active_version_id": good,
        "activity": project(&observed.activity),
    });
    hidden(&proof, &[GOOD_SCRIPT, BROKEN_SCRIPT])?;
    Ok(proof)
}

fn broken_rejected(observed: &Observed, broken: &str, broken_sha: &str) -> Result<()> {
    if observed.broken["state"] != "rejected"
        || observed.broken["sha256"] != broken_sha
        || observed.broken["version_id"] != broken
        || test_exit(&observed.broken, 7).is_err()
        || observed.activate_broken["ok"] != false
        || observed.activate_broken["reason"] != "pass_required"
    {
        return Err(Fail::Check("broken_version_rejected"));
    }
    Ok(())
}

fn previous_stays_active(observed: &Observed, good: &str) -> Result<()> {
    let entries = observed.list["entries"]
        .as_array()
        .ok_or(Fail::Check("previous_version_active"))?;
    let active = entries.len() == 1
        && entries[0]["name"] == SCRIPT_NAME
        && entries[0]["os"] == "ubuntu"
        && entries[0]["active_version_id"] == good;
    if !active
        || observed.activated["version_id"] != good
        || observed.activated["state"] != "active"
        || observed.activated["sha256"] != observed.passed["sha256"]
    {
        return Err(Fail::Check("previous_version_active"));
    }
    Ok(())
}

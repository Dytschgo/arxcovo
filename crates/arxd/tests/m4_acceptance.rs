use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::{Instant, sleep, timeout_at};

#[cfg(all(windows, target_arch = "x86_64"))]
#[path = "m4_acceptance/owner.rs"]
mod owner;

type Result<T> = std::result::Result<T, Failure>;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
enum Failure {
    Discovery,
    MissingTool(&'static str),
    SchemaChanged(&'static str),
    AdmissionClosed(&'static str),
    DaemonQualificationEvidenceUnavailable,
    DaemonQualificationEvidenceMismatch,
    SessionProcessEvidenceUnavailable,
    SessionProcessMismatch,
    WindowsBootEvidenceUnavailable,
    WindowsBootEvidenceMismatch,
    LeaseClockDisagreement,
    LeaseEndedEarly,
    Timeout(&'static str),
    Check(&'static str),
}

fn check(condition: bool, name: &'static str) -> Result<()> {
    condition.then_some(()).ok_or(Failure::Check(name))
}

// Compare schemas by structure. The names and bounds are the reviewed wire
// contract, independent of the daemon implementation under observation.
fn schemas() -> Vec<(&'static str, Value)> {
    let id = json!({"type":"string", "pattern":"^[0-9a-f]{32}$"});
    let ttl = json!({"type":"integer","minimum":60,"maximum":2592000});
    let cursor = json!({"type":"string","maxLength":96,"pattern":"^[A-Za-z0-9_-]+$"});
    let object = |properties: Value, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let vm_request = object(
        json!({"vm_id":id,"request_id":id}),
        &["vm_id", "request_id"],
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
        (
            "vm_catalog",
            object(
                json!({"limit":{"type":"integer","minimum":1,"maximum":20,"default":20},"cursor":cursor}),
                &[],
            ),
        ),
        (
            "vm_template_build",
            object(
                json!({"recipe_id":id,"request_id":id}),
                &["recipe_id", "request_id"],
            ),
        ),
        (
            "vm_create",
            object(
                json!({"template_id":id,"request_id":id,"ttl_s":ttl}),
                &["template_id", "request_id"],
            ),
        ),
        ("vm_get", object(json!({"vm_id":id}), &["vm_id"])),
        ("vm_start", vm_request.clone()),
        ("vm_stop", vm_request),
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
            "vm_operation",
            object(json!({"operation_id":id}), &["operation_id"]),
        ),
        (
            "vm_snapshot_create",
            object(
                json!({"vm_id":id,"request_id":id,
                    "name":{"type":"string","minLength":1,"maxLength":48,"pattern":"^[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*$"}
                }),
                &["vm_id", "request_id"],
            ),
        ),
        (
            "vm_snapshot_list",
            object(
                json!({
                    "vm_id":id,"limit":{"type":"integer","minimum":1,"maximum":4,"default":4},"cursor":cursor
                }),
                &["vm_id"],
            ),
        ),
        (
            "vm_snapshot_restore",
            object(
                json!({"vm_id":id,"snapshot_id":id,"request_id":id}),
                &["vm_id", "snapshot_id", "request_id"],
            ),
        ),
    ]
}

fn discover(page: Result<Value>) -> Result<()> {
    let page = page?;
    let tools = page["tools"].as_array().ok_or(Failure::Discovery)?;
    if !page["nextCursor"].is_null() || tools.len() > 64 {
        return Err(Failure::Discovery);
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

fn reason(value: &Value) -> Option<&'static str> {
    [
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

fn opaque(value: &Value, len: usize) -> Option<&str> {
    value.as_str().filter(|id| {
        id.len() == len
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

// The 7c receipt adapter must read this projection from the process-verified
// session. A plan pin is an expectation only, never the attested digest.
fn qualification_digest<'a>(
    catalog: &'a Value,
    expected_digest: Option<&str>,
    expected_recipes: [(&str, &str, &str); 2],
) -> Result<&'a str> {
    let selected = &catalog["capability"]["selected_test_qualification"];
    let digest =
        opaque(&selected["sha256"], 64).ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    let scopes = selected["recipes"]
        .as_array()
        .ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    if expected_digest != Some(digest)
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
        return Err(Failure::DaemonQualificationEvidenceMismatch);
    }
    Ok(digest)
}

fn bind_session_process(expected: u32, connected_peer: Option<u32>) -> Result<()> {
    let peer = connected_peer.ok_or(Failure::SessionProcessEvidenceUnavailable)?;
    if expected == 0 || peer != expected {
        return Err(Failure::SessionProcessMismatch);
    }
    Ok(())
}

fn windows_boot_witness<'a>(
    status: &'a Value,
    vm_id: &str,
    generation: &str,
    start: &str,
) -> Result<&'a str> {
    let receipt = &status["native_boot_witness"];
    let witness =
        opaque(&receipt["witness_id"], 32).ok_or(Failure::WindowsBootEvidenceUnavailable)?;
    if receipt["source"] != "guest_ready_handshake"
        || receipt["vm_id"] != vm_id
        || receipt["generation"] != generation
        || receipt["operation_id"] != start
        || witness == start
    {
        return Err(Failure::WindowsBootEvidenceMismatch);
    }
    Ok(witness)
}

const LEASE_CLOCK_TOLERANCE_MS: i64 = 1_000;

#[derive(Clone, Copy)]
struct ClockSample {
    utc_ms: i64,
    monotonic: Instant,
}

struct LeaseClock {
    origin: ClockSample,
    utc_deadline: i64,
    monotonic_deadline: Instant,
}

impl LeaseClock {
    fn new(
        origin: ClockSample,
        utc_deadline: i64,
        ttl: Duration,
        dispatch_bound: Duration,
    ) -> Result<Self> {
        let delta = i128::from(utc_deadline) - i128::from(origin.utc_ms);
        let delay = u64::try_from(delta)
            .map(Duration::from_millis)
            .map_err(|_| Failure::Check("lease_deadline_bound"))?;
        check(
            delay >= ttl && delay <= ttl + dispatch_bound,
            "lease_deadline_bound",
        )?;
        Ok(Self {
            origin,
            utc_deadline,
            monotonic_deadline: origin.monotonic + delay,
        })
    }

    fn check_clock(&self, now: ClockSample) -> Result<()> {
        let utc = i128::from(now.utc_ms) - i128::from(self.origin.utc_ms);
        let monotonic = now
            .monotonic
            .checked_duration_since(self.origin.monotonic)
            .ok_or(Failure::LeaseClockDisagreement)?
            .as_millis() as i128;
        if (utc - monotonic).abs() > i128::from(LEASE_CLOCK_TOLERANCE_MS) {
            return Err(Failure::LeaseClockDisagreement);
        }
        Ok(())
    }

    fn observe(&self, status: &Value, now: ClockSample) -> Result<bool> {
        self.check_clock(now)?;
        match status["state"].as_str() {
            Some("stopped") => {
                check(
                    status["expires_at_ms"] == self.utc_deadline,
                    "lease_changed",
                )?;
                Ok(false)
            }
            Some(state @ ("expired" | "teardown")) => {
                if now.utc_ms < self.utc_deadline || now.monotonic < self.monotonic_deadline {
                    return Err(Failure::LeaseEndedEarly);
                }
                if state == "teardown" {
                    check(
                        status["expires_at_ms"] == self.utc_deadline,
                        "lease_changed",
                    )?;
                }
                Ok(state == "expired")
            }
            _ => Err(Failure::Check("expiry_state")),
        }
    }
}

fn retry_unchanged(
    before: &Value,
    replay: &Value,
    before_effect: &Value,
    after_effect: &Value,
) -> Result<()> {
    check(
        opaque(&before["operation_id"], 32).is_some()
            && before["operation_id"] == replay["operation_id"]
            && before["expires_at_ms"] == replay["expires_at_ms"]
            && before_effect == after_effect,
        "same_key_retry_changed_effect",
    )
}

/// Save a projection, never the MCP envelope or arbitrary text returned by it.
fn operation_evidence(response: &Value) -> Value {
    let mut out = json!({});
    for key in [
        "operation_id",
        "vm_id",
        "template_id",
        "recipe_id",
        "snapshot_id",
    ] {
        if let Some(id) = opaque(&response[key], 32) {
            out[key] = json!(id);
        }
    }
    for key in ["exit_code", "expires_at_ms"] {
        if let Some(number) = response[key].as_u64() {
            out[key] = json!(number);
        }
    }
    for (key, values) in [
        (
            "action",
            &[
                "template_build",
                "create",
                "start",
                "run_script",
                "stop",
                "snapshot",
                "restore",
            ][..],
        ),
        (
            "state",
            &["pending", "succeeded", "failed", "interrupted"][..],
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

async fn poll<T, F: Future<Output = Result<Option<T>>>>(
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

#[test]
fn discovery_rejects_transport_failure_duplicate_and_schema_drift() {
    assert_eq!(discover(Err(Failure::Discovery)), Err(Failure::Discovery));
    let mut tools: Vec<_> = schemas()
        .into_iter()
        .map(|(name, schema)| json!({"name":name,"inputSchema":schema}))
        .collect();
    assert_eq!(discover(Ok(json!({"tools":tools}))), Ok(()));
    tools.push(tools[0].clone());
    assert_eq!(
        discover(Ok(json!({"tools":tools}))),
        Err(Failure::Discovery)
    );
    tools.pop();
    tools[1]["inputSchema"]["additionalProperties"] = json!(true);
    assert_eq!(
        discover(Ok(json!({"tools":tools}))),
        Err(Failure::SchemaChanged("vm_catalog"))
    );
}

#[test]
fn discovery_requires_snapshot_restore() {
    let tools: Vec<_> = schemas()
        .into_iter()
        .filter(|(name, _)| *name != "vm_snapshot_restore")
        .map(|(name, schema)| json!({"name":name,"inputSchema":schema}))
        .collect();
    assert_eq!(
        discover(Ok(json!({"tools":tools}))),
        Err(Failure::MissingTool("vm_snapshot_restore"))
    );
}

#[test]
fn closed_admission_preserves_only_typed_reason() {
    let closed = |reason: &str| json!({"capability":{"available":false,"reason":reason}});
    assert_eq!(
        assert_admitted(&closed("qualification_missing")),
        Err(Failure::AdmissionClosed("qualification_missing"))
    );
    assert_eq!(
        assert_admitted(&closed(r"D:\private\receipt.json")),
        Err(Failure::AdmissionClosed("unrecognized_refusal"))
    );
    assert!(assert_admitted(&json!({"capability":{"available":"true"}})).is_err());
}

#[tokio::test(start_paused = true)]
async fn polling_deadline_bounds_pending_results_and_stuck_calls() {
    let budget = Duration::from_millis(250);
    let started = Instant::now();
    let mut attempts = 0;
    let pending = poll::<(), _>("terminal", budget, Duration::from_millis(100), || {
        attempts += 1;
        std::future::ready(Ok(None))
    })
    .await;
    assert_eq!(pending, Err(Failure::Timeout("terminal")));
    assert_eq!(attempts, 3);
    assert!(started.elapsed() >= budget);
    let stalled = poll::<(), _>("terminal", budget, budget, std::future::pending).await;
    assert_eq!(stalled, Err(Failure::Timeout("terminal")));
}

#[test]
fn evidence_drops_script_bodies_and_paths_even_in_known_fields() {
    let structured = operation_evidence(&json!({
        "operation_id":"aa".repeat(16), "action":"run_script", "exit_code":7,
        "script":"private-script-body", "stage":r"D:\private\artifact",
        "vm_id":r"D:\private\artifact", "reason":"private-script-body",
        "output":{"path":r"D:\private\artifact"}
    }));
    assert_eq!(
        structured,
        json!({"operation_id":"aa".repeat(16),"action":"run_script","exit_code":7})
    );
}

#[test]
fn retry_needs_same_receipt_and_unchanged_effect() {
    let original = json!({"operation_id":"ab".repeat(16),"expires_at_ms":42000});
    let effect = json!({"generation":"34".repeat(16),"vm_count":1,"charge":8192});
    assert_eq!(
        retry_unchanged(&original, &original, &effect, &effect),
        Ok(())
    );
    for replay in [
        json!({"operation_id":"bc".repeat(16),"expires_at_ms":42000}),
        json!({"operation_id":"ab".repeat(16),"expires_at_ms":43000}),
    ] {
        assert!(retry_unchanged(&original, &replay, &effect, &effect).is_err());
    }
    let mut duplicate = effect.clone();
    duplicate["vm_count"] = json!(2);
    assert!(retry_unchanged(&original, &original, &effect, &duplicate).is_err());
}

#[test]
fn open_catalog_requires_the_daemons_selected_record_and_exact_test_scopes() {
    let pin = "aa".repeat(32);
    let recipe = "11".repeat(16);
    let second = "22".repeat(16);
    let digest = "bb".repeat(32);
    let expected = [
        ("ubuntu", recipe.as_str(), pin.as_str()),
        ("windows", second.as_str(), pin.as_str()),
    ];
    let mut catalog = json!({"capability":{"available":true}});
    assert_eq!(
        qualification_digest(&catalog, Some(&digest), expected),
        Err(Failure::DaemonQualificationEvidenceUnavailable)
    );
    catalog["capability"]["selected_test_qualification"] = json!({
        "sha256":digest,"recipes":[
            {"guest":"ubuntu","recipe_id":recipe,"recipe_sha256":pin},
            {"guest":"windows","recipe_id":second,"recipe_sha256":pin}
        ]
    });
    assert_eq!(
        qualification_digest(&catalog, Some(&digest), expected),
        Ok(digest.as_str())
    );
    assert_eq!(
        qualification_digest(&catalog, Some(&"cc".repeat(32)), expected),
        Err(Failure::DaemonQualificationEvidenceMismatch)
    );
    assert_eq!(
        qualification_digest(&catalog, None, expected),
        Err(Failure::DaemonQualificationEvidenceMismatch)
    );
    catalog["capability"]["selected_test_qualification"]["recipes"][1]["recipe_sha256"] =
        json!("dd".repeat(32));
    assert_eq!(
        qualification_digest(&catalog, Some(&digest), expected),
        Err(Failure::DaemonQualificationEvidenceMismatch)
    );
}

#[test]
fn session_binding_refuses_unavailable_and_wrong_peer_pid() {
    assert_eq!(
        bind_session_process(18, None),
        Err(Failure::SessionProcessEvidenceUnavailable)
    );
    assert_eq!(
        bind_session_process(18, Some(19)),
        Err(Failure::SessionProcessMismatch)
    );
    assert_eq!(bind_session_process(18, Some(18)), Ok(()));
}

#[test]
fn windows_ready_flag_and_admission_epoch_are_insufficient() {
    let vm = "11".repeat(16);
    let generation = "22".repeat(16);
    let start = "33".repeat(16);
    let witness = "44".repeat(16);
    let mut status = json!({"state":"running","ready":true,"operation_id":start});
    assert_eq!(
        windows_boot_witness(&status, &vm, &generation, &start),
        Err(Failure::WindowsBootEvidenceUnavailable)
    );
    status["native_boot_witness"] = json!({"source":"guest_ready_handshake","vm_id":vm,
        "generation":generation,"operation_id":start,"witness_id":witness});
    assert_eq!(
        windows_boot_witness(&status, &vm, &generation, &start),
        Ok(witness.as_str())
    );
    for (field, mismatched) in [
        ("vm_id", &generation),
        ("generation", &vm),
        ("operation_id", &vm),
        ("witness_id", &start),
    ] {
        let mut wrong = status.clone();
        wrong["native_boot_witness"][field] = json!(mismatched);
        assert_eq!(
            windows_boot_witness(&wrong, &vm, &generation, &start),
            Err(Failure::WindowsBootEvidenceMismatch)
        );
    }
}

#[test]
fn lease_refuses_clock_steps_early_teardown_and_changed_deadline() {
    let origin = ClockSample {
        utc_ms: 100_000,
        monotonic: Instant::now(),
    };
    let ttl = Duration::from_secs(60);
    let lease = LeaseClock::new(origin, 160_000, ttl, Duration::from_secs(15)).unwrap();
    assert!(LeaseClock::new(origin, 176_000, ttl, Duration::from_secs(15)).is_err());
    let now = |elapsed, utc_ms| ClockSample {
        utc_ms,
        monotonic: origin.monotonic + Duration::from_secs(elapsed),
    };
    let live = json!({"state":"stopped","expires_at_ms":160_000});
    let expired = json!({"state":"expired"});
    let teardown = json!({"state":"teardown","expires_at_ms":160_000});
    assert_eq!(lease.observe(&live, now(10, 110_000)), Ok(false));
    assert_eq!(
        lease.observe(&expired, now(10, 160_000)),
        Err(Failure::LeaseClockDisagreement)
    );
    assert_eq!(
        lease.observe(&live, now(10, 108_000)),
        Err(Failure::LeaseClockDisagreement)
    );
    assert_eq!(
        lease.observe(&expired, now(10, 110_000)),
        Err(Failure::LeaseEndedEarly)
    );
    assert_eq!(
        lease.observe(&teardown, now(59, 159_000)),
        Err(Failure::LeaseEndedEarly)
    );
    assert_eq!(lease.observe(&teardown, now(61, 161_000)), Ok(false));
    assert_eq!(lease.observe(&expired, now(61, 161_000)), Ok(true));
    assert_eq!(
        lease.observe(
            &json!({"state":"stopped","expires_at_ms":160_001}),
            now(10, 110_000)
        ),
        Err(Failure::Check("lease_changed"))
    );
}

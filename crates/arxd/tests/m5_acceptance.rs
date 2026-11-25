#[path = "m5_acceptance/checks.rs"]
mod checks;

use checks::{
    BROKEN_SCRIPT, Fail, GOOD_SCRIPT, Observed, SCRIPT_NAME, Submitted, WINDOWS_RECIPE,
    accept_vault, discover, hidden, parse_plan, project, qualification, script_sha256,
    vault_reason,
};
use serde_json::{Value, json};

fn hex_pair(byte: &str, words: usize) -> String {
    byte.repeat(words)
}

fn recipe(id: &str) -> Value {
    json!({"recipe_id": id, "recipe_sha256": hex_pair("ab", 32)})
}

fn plan_value() -> Value {
    json!({
        "daemon": "D:/acceptance/bin/arxd.exe",
        "daemon_sha256": hex_pair("cd", 32),
        "daemon_config": "D:/acceptance/private/daemon.json",
        "client_config": "D:/acceptance/private/client.json",
        "store": {
            "volume_guid": "\\\\?\\Volume{01234567-89ab-cdef-0123-456789abcdef}\\",
            "volume_serial": hex_pair("01", 8),
            "directory_file_id": hex_pair("02", 16)
        },
        "qualification_sha256": hex_pair("ef", 32),
        "evidence_dir": "D:/acceptance/evidence",
        "idle_delay_s": 660,
        "ubuntu": recipe(&hex_pair("11", 16)),
        "windows": recipe(WINDOWS_RECIPE)
    })
}

fn parse_ok(value: &Value) -> checks::Plan {
    parse_plan(&serde_json::to_vec(value).unwrap()).unwrap()
}

#[test]
fn plan_requires_the_m4_qualification_pin_and_both_test_recipes() {
    let plan = parse_ok(&plan_value());
    assert_eq!(
        plan.qualification_sha256.as_deref(),
        Some(hex_pair("ef", 32).as_str())
    );
    assert_eq!(plan.windows.recipe_id, WINDOWS_RECIPE);
    assert_eq!(plan.idle_delay_s, 660);

    let mut missing = plan_value();
    missing
        .as_object_mut()
        .unwrap()
        .remove("qualification_sha256");
    assert_eq!(
        parse_plan(&serde_json::to_vec(&missing).unwrap()),
        Err(Fail::QualificationRequired)
    );
    let mut empty = plan_value();
    empty["qualification_sha256"] = Value::Null;
    assert_eq!(
        parse_plan(&serde_json::to_vec(&empty).unwrap()),
        Err(Fail::QualificationRequired)
    );

    let mut wrong_windows = plan_value();
    wrong_windows["windows"]["recipe_id"] = json!(hex_pair("22", 16));
    assert_eq!(
        parse_plan(&serde_json::to_vec(&wrong_windows).unwrap()),
        Err(Fail::Check("test_recipe_ids"))
    );
    let mut same = plan_value();
    same["ubuntu"]["recipe_id"] = json!(WINDOWS_RECIPE);
    assert_eq!(
        parse_plan(&serde_json::to_vec(&same).unwrap()),
        Err(Fail::Check("test_recipe_ids"))
    );
    let mut extra = plan_value();
    extra["script"] = json!(GOOD_SCRIPT);
    assert_eq!(
        parse_plan(&serde_json::to_vec(&extra).unwrap()),
        Err(Fail::Check("input_format"))
    );
    assert_eq!(
        parse_plan(&[b'{'].repeat(65_537)),
        Err(Fail::Check("input_bound"))
    );
}

fn qualified_catalog() -> Value {
    let plan = parse_ok(&plan_value());
    json!({
        "capability": {
            "available": true,
            "selected_test_qualification": {
                "sha256": plan.qualification_sha256,
                "recipes": [
                    {
                        "guest": "ubuntu",
                        "recipe_id": plan.ubuntu.recipe_id,
                        "recipe_sha256": plan.ubuntu.recipe_sha256
                    },
                    {
                        "guest": "windows",
                        "recipe_id": plan.windows.recipe_id,
                        "recipe_sha256": plan.windows.recipe_sha256
                    }
                ]
            }
        }
    })
}

#[test]
fn qualification_refuses_to_start_without_the_selected_m4_record() {
    let plan = parse_ok(&plan_value());
    let pin = plan.qualification_sha256.unwrap();
    let recipes = [
        (
            "ubuntu",
            plan.ubuntu.recipe_id.as_str(),
            plan.ubuntu.recipe_sha256.as_str(),
        ),
        (
            "windows",
            plan.windows.recipe_id.as_str(),
            plan.windows.recipe_sha256.as_str(),
        ),
    ];
    let catalog = qualified_catalog();
    assert_eq!(qualification(&catalog, &pin, recipes), Ok(pin.as_str()));
    assert_eq!(
        qualification(&json!({"capability": {"available": true}}), &pin, recipes),
        Err(Fail::QualificationUnavailable)
    );
    assert_eq!(
        qualification(&catalog, &hex_pair("aa", 32), recipes),
        Err(Fail::QualificationMismatch)
    );
    assert_eq!(
        qualification(
            &json!({"capability": {"available": false, "reason": "qualification_missing"}}),
            &pin,
            recipes,
        ),
        Err(Fail::AdmissionClosed("qualification_missing"))
    );
    assert_eq!(
        qualification(
            &json!({"capability": {"available": false, "reason": r"D:\private\receipt.json"}}),
            &pin,
            recipes,
        ),
        Err(Fail::AdmissionClosed("unrecognized_refusal"))
    );
}

#[test]
fn discovery_requires_the_vault_contract_and_the_extended_activity_schema() {
    let tools: Vec<_> = checks::schemas()
        .into_iter()
        .map(|(name, schema)| json!({"name": name, "inputSchema": schema}))
        .collect();
    assert_eq!(
        discover(Ok(json!({"tools": tools, "nextCursor": null}))),
        Ok(())
    );

    let mut dropped = tools.clone();
    dropped.retain(|tool| tool["name"] != "vault_activate");
    assert_eq!(
        discover(Ok(json!({"tools": dropped}))),
        Err(Fail::MissingTool("vault_activate"))
    );

    let mut drifted = tools.clone();
    drifted
        .iter_mut()
        .find(|tool| tool["name"] == "list_activity")
        .unwrap()["inputSchema"] = json!({"type": "object"});
    assert_eq!(
        discover(Ok(json!({"tools": drifted}))),
        Err(Fail::SchemaChanged("list_activity"))
    );
}

#[test]
fn evidence_drops_script_bodies_paths_and_authors() {
    let raw = json!({
        "ok": true,
        "version_id": hex_pair("ab", 16),
        "script": GOOD_SCRIPT,
        "content": BROKEN_SCRIPT,
        "author": "owner-principal",
        "path": r"D:\private\vault",
        "name": SCRIPT_NAME,
        "os": "ubuntu",
        "state": "rejected",
        "sha256": hex_pair("11", 32),
        "reason": "pass_required",
        "test": {"status": "exited", "exit_code": 7, "cleanup_confirmed": true, "stdout": GOOD_SCRIPT}
    });
    let projected = project(&raw);
    assert_eq!(hidden(&projected, &[GOOD_SCRIPT, BROKEN_SCRIPT]), Ok(()));
    assert!(projected.get("script").is_none());
    assert!(projected.get("content").is_none());
    assert!(projected.get("author").is_none());
    assert!(projected.get("path").is_none());
    assert!(projected["test"].get("stdout").is_none());
    assert_eq!(projected["reason"], "pass_required");
    assert_eq!(vault_reason(&json!("pass_required")), Some("pass_required"));
    assert_eq!(vault_reason(&json!(r"D:\private\vault")), None);
    let leaked = project(&json!({"script": GOOD_SCRIPT}));
    assert_eq!(
        hidden(&json!({"kept": GOOD_SCRIPT}), &[GOOD_SCRIPT]),
        Err(Fail::Check("evidence_leaked_script"))
    );
    assert_eq!(hidden(&leaked, &[GOOD_SCRIPT]), Ok(()));
}

fn version(id: &str, sha: &str, state: &str, exit: u64) -> Value {
    json!({
        "ok": true,
        "version_id": id,
        "script_id": hex_pair("ef", 16),
        "os": "ubuntu",
        "name": SCRIPT_NAME,
        "sha256": sha,
        "created_at_ms": 1_700_000_000_000_i64,
        "state": state,
        "test": {"status": "exited", "exit_code": exit, "cleanup_confirmed": true}
    })
}

fn ids() -> (String, String, String, String) {
    (
        hex_pair("ef", 16),
        hex_pair("ab", 16),
        hex_pair("cd", 16),
        hex_pair("56", 16),
    )
}

fn bound() -> Submitted {
    let (script, good, broken, operation) = ids();
    Submitted {
        script_id: script,
        good_version_id: good,
        broken_version_id: broken,
        broken_operation_id: operation,
    }
}

fn activity(good: &str, broken: &str, operation: &str) -> Value {
    let row =
        |tool, outcome, version: Option<&str>, operation: Option<&str>, reason: Option<&str>| {
            let mut value = json!({"tool": tool, "outcome": outcome, "at_ms": 10});
            if let Some(version) = version {
                value["version_id"] = json!(version);
            }
            if let Some(operation) = operation {
                value["operation_id"] = json!(operation);
            }
            if let Some(reason) = reason {
                value["reason"] = json!(reason);
            }
            value
        };
    json!({
        "ok": true,
        "activities": [
            row("vault_add_version", "succeeded", Some(good), None, None),
            row("vault_test_version", "started", Some(good), None, None),
            row("vault_test_version", "succeeded", Some(good), None, None),
            row("vault_activate", "succeeded", Some(good), None, None),
            row("vault_add_version", "succeeded", Some(broken), None, None),
            row("vault_test_version", "started", Some(broken), Some(operation), None),
            row("vault_test_version", "failed", Some(broken), Some(operation), Some("exited")),
            row("vault_activate", "refused", None, None, Some("pass_required"))
        ]
    })
}

fn observed() -> (Observed, String, String) {
    let good_sha = script_sha256(GOOD_SCRIPT);
    let broken_sha = script_sha256(BROKEN_SCRIPT);
    let (script, good, broken, operation) = ids();
    let list = json!({
        "ok": true,
        "entries": [{
            "script_id": script,
            "os": "ubuntu",
            "name": SCRIPT_NAME,
            "active_version_id": good
        }]
    });
    let observed = Observed {
        passed: version(&good, &good_sha, "testing", 0),
        activate_good: json!({
            "ok": true,
            "version_id": good,
            "operation_id": hex_pair("12", 16)
        }),
        activated: version(&good, &good_sha, "active", 0),
        broken: version(&broken, &broken_sha, "rejected", 7),
        activate_broken: json!({"ok": false, "reason": "pass_required"}),
        active_before: list.clone(),
        list,
        activity: activity(&good, &broken, &operation),
    };
    (observed, good_sha, broken_sha)
}

#[test]
fn broken_version_rejection_is_required() {
    let (sample, good_sha, broken_sha) = observed();
    let proof = accept_vault(&sample, &bound(), &good_sha, &broken_sha).unwrap();
    assert_eq!(hidden(&proof, &[GOOD_SCRIPT, BROKEN_SCRIPT]), Ok(()));
    assert_eq!(
        proof["active_version_id"],
        sample.list["entries"][0]["active_version_id"]
    );
    assert_eq!(
        project(&sample.list)["entries"][0]["active_version_id"],
        sample.list["entries"][0]["active_version_id"]
    );
    assert_eq!(
        project(&sample.active_before)["entries"][0]["active_version_id"],
        sample.active_before["entries"][0]["active_version_id"]
    );

    let mut accepted = sample_parts();
    accepted.broken["state"] = json!("active");
    assert_eq!(
        accept_vault(&accepted, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("broken_version_rejected"))
    );

    let mut zero = sample_parts();
    zero.broken["test"]["exit_code"] = json!(0);
    assert_eq!(
        accept_vault(&zero, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("broken_version_rejected"))
    );

    let mut dirty = sample_parts();
    dirty.broken["test"]["cleanup_confirmed"] = json!(false);
    assert_eq!(
        accept_vault(&dirty, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("broken_version_rejected"))
    );

    let mut activated = sample_parts();
    activated.activate_broken = json!({
        "ok": true,
        "version_id": activated.broken["version_id"],
        "operation_id": hex_pair("34", 16)
    });
    assert_eq!(
        accept_vault(&activated, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("broken_version_rejected"))
    );

    let mut other = sample_parts();
    other.activate_broken["reason"] = json!("not_draft");
    assert_eq!(
        accept_vault(&other, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("broken_version_rejected"))
    );
}

fn sample_parts() -> Observed {
    observed().0
}

#[test]
fn previous_version_stays_active_is_required() {
    let (sample, good_sha, broken_sha) = observed();
    assert!(accept_vault(&sample, &bound(), &good_sha, &broken_sha).is_ok());

    let mut replaced = sample_parts();
    replaced.list["entries"][0]["active_version_id"] = replaced.broken["version_id"].clone();
    assert_eq!(
        accept_vault(&replaced, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("previous_version_active"))
    );

    let mut earlier = sample_parts();
    earlier.active_before["entries"][0]["active_version_id"] = earlier.broken["version_id"].clone();
    assert_eq!(
        accept_vault(&earlier, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("previous_version_active"))
    );

    let mut superseded = sample_parts();
    superseded.activated["state"] = json!("superseded");
    assert_eq!(
        accept_vault(&superseded, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("previous_version_active"))
    );

    let mut missing = sample_parts();
    missing.list["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("active_version_id");
    assert_eq!(
        accept_vault(&missing, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("previous_version_active"))
    );
}

/// The reviewer's false pass: the rejected version keeps its failing test and
/// the original active version, but it belongs to another script.
#[test]
fn unrelated_script_must_not_pass() {
    let (mut sample, good_sha, broken_sha) = observed();
    sample.broken["script_id"] = json!(hex_pair("99", 16));
    sample.broken["name"] = json!("other-script");
    assert_eq!(
        accept_vault(&sample, &bound(), &good_sha, &broken_sha),
        Err(Fail::Check("submitted_binding"))
    );
}

/// A consistent rejection of some other version, plus a submit operation that
/// is not the one this run queued.
#[test]
fn unrelated_version_must_not_pass() {
    let (mut sample, good_sha, broken_sha) = observed();
    let submitted = bound();
    let other = hex_pair("77", 16);
    sample.broken["version_id"] = json!(&other);
    for row in sample.activity["activities"].as_array_mut().unwrap() {
        if row["version_id"] == submitted.broken_version_id.as_str() {
            row["version_id"] = json!(&other);
        }
    }
    assert_eq!(
        accept_vault(&sample, &submitted, &good_sha, &broken_sha),
        Err(Fail::Check("submitted_binding"))
    );

    let (sample, good_sha, broken_sha) = observed();
    let mut swapped = bound();
    swapped.broken_operation_id = hex_pair("88", 16);
    assert_eq!(
        accept_vault(&sample, &swapped, &good_sha, &broken_sha),
        Err(Fail::Check("submitted_binding"))
    );
}

/// The started and failed rows for the submitted tester operation stay a
/// valid pair. An earlier, unrelated audit row reuses that operation id.
/// Removing the total-row comparison accepts this audit.
#[test]
fn reused_tester_operation_id_must_not_pass() {
    let submitted = bound();
    for previous_row in [0, 1, 2, 3, 4] {
        let (mut sample, good_sha, broken_sha) = observed();
        sample.activity["activities"][previous_row]["operation_id"] =
            json!(submitted.broken_operation_id);
        assert_eq!(
            accept_vault(&sample, &submitted, &good_sha, &broken_sha),
            Err(Fail::Check("submitted_binding"))
        );
    }
}

//! Owner-host M5 journey. Transport, PID binding, store pins and foreground
//! launch are the M4 owner's. This module only adds the vault script steps.
//! Unverified on real hardware. Do not run it from CI.

use super::*;
use std::io::Read;

#[path = "checks.rs"]
mod checks;

const VAULT_BUDGET: Duration = Duration::from_secs(720);

fn map_fail(fail: checks::Fail) -> Failure {
    match fail {
        checks::Fail::Discovery => Failure::Discovery,
        checks::Fail::MissingTool(name) => Failure::MissingTool(name),
        checks::Fail::SchemaChanged(name) => Failure::SchemaChanged(name),
        checks::Fail::AdmissionClosed(reason) => Failure::AdmissionClosed(reason),
        checks::Fail::QualificationRequired => Failure::Check("qualification_required"),
        checks::Fail::QualificationUnavailable => Failure::DaemonQualificationEvidenceUnavailable,
        checks::Fail::QualificationMismatch => Failure::DaemonQualificationEvidenceMismatch,
        checks::Fail::Check(name) => Failure::Check(name),
    }
}

fn read_bounded(file: &mut File) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    safe(
        Read::take(&mut *file, 65_537).read_to_end(&mut bytes),
        "input_read",
    )?;
    check(bytes.len() <= 65_536, "input_bound")?;
    Ok(bytes)
}

fn recipes(plan: &Plan) -> [(&str, &str, &str); 2] {
    [
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
    ]
}

pub(super) fn discover_tools(page: Result<Value>) -> Result<()> {
    checks::discover(page.map_err(|_| checks::Fail::Discovery)).map_err(map_fail)
}

pub(super) fn require_qualification(catalog: &Value, plan: &Plan) -> Result<String> {
    let pin = plan
        .qualification_sha256
        .as_deref()
        .ok_or(Failure::Check("qualification_required"))?;
    checks::qualification(catalog, pin, recipes(plan))
        .map(|digest| digest.to_owned())
        .map_err(map_fail)
}

pub(super) async fn qualified_catalog(
    client: &Client,
    plan: &Plan,
) -> Result<(Vec<Value>, String)> {
    let page = rpc(client, "vm_catalog", json!({})).await?;
    let selected = require_qualification(&page, plan)?;
    let entries = page["entries"]
        .as_array()
        .cloned()
        .ok_or(Failure::Check("catalog_page"))?;
    check(entries.len() <= 20, "catalog_page_bound")?;
    Ok((entries, selected))
}

fn request_id() -> Result<String> {
    let now = now_ms()?;
    check(now >= 0, "utc_clock")?;
    let mut nonce = [0; 8];
    safe(getrandom::fill(&mut nonce), "random_id")?;
    check(nonce != [0; 8], "random_id")?;
    Ok(format!("{now:016x}{}", hex::encode(nonce)))
}

fn record(evidence: &mut Evidence, step: &'static str, value: &Value) -> Result<()> {
    let proof = checks::project(value);
    checks::hidden(&proof, &[checks::GOOD_SCRIPT, checks::BROKEN_SCRIPT]).map_err(map_fail)?;
    evidence.record(step, proof);
    Ok(())
}

async fn vault_call(client: &Client, name: &'static str, args: Value) -> Result<Value> {
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
    let refused = row["ok"] == false;
    check(refused == (result.is_error == Some(true)), name)?;
    if refused {
        check(checks::vault_reason(&row["reason"]).is_some(), name)?;
    }
    Ok(row)
}

async fn vault_ok(client: &Client, name: &'static str, args: Value) -> Result<Value> {
    let row = vault_call(client, name, args).await?;
    if row["ok"] == false {
        return Err(checks::vault_reason(&row["reason"])
            .map_or(Failure::Check(name), Failure::AdmissionClosed));
    }
    check(row["ok"] == true, name)?;
    Ok(row)
}

async fn settled(client: &Client, version: &str) -> Result<Value> {
    poll("vault_test", VAULT_BUDGET, TICK, || async {
        let row = vault_ok(client, "vault_get", json!({"version_id": version})).await?;
        if row["state"] == "testing" && row["test"].is_null() {
            return Ok(None);
        }
        if row["test"].is_object() {
            return Ok(Some(row));
        }
        Err(Failure::Check("vault_test_state"))
    })
    .await
}

async fn add_version(client: &Client, script: &str) -> Result<Value> {
    vault_ok(
        client,
        "vault_add_version",
        json!({
            "os": "ubuntu",
            "name": checks::SCRIPT_NAME,
            "script": script,
            "request_id": request_id()?
        }),
    )
    .await
}

async fn test_version(client: &Client, version: &str) -> Result<Value> {
    vault_ok(
        client,
        "vault_test_version",
        json!({"version_id": version, "request_id": request_id()?}),
    )
    .await
}

pub(super) async fn run() -> Result<()> {
    let plan_path = PathBuf::from(
        env::var_os("ARX_M5_ACCEPTANCE_PLAN").ok_or(Failure::Check("acceptance_plan_required"))?,
    );
    d_path(&plan_path)?;
    let _plan_pins = pin_root(plan_path.parent().ok_or(Failure::Check("plan_parent"))?)?;
    let mut file = open(&plan_path, false, FILE_SHARE_READ)?;
    let parsed = checks::parse_plan(&read_bounded(&mut file)?).map_err(map_fail)?;
    let pin = parsed
        .qualification_sha256
        .ok_or(Failure::Check("qualification_required"))?;
    let plan = Plan {
        daemon: PathBuf::from(parsed.daemon),
        daemon_sha256: parsed.daemon_sha256,
        daemon_config: PathBuf::from(parsed.daemon_config),
        client_config: PathBuf::from(parsed.client_config),
        store: ExpectedVmStore {
            volume_guid: parsed.store.volume_guid,
            volume_serial: parsed.store.volume_serial,
            directory_file_id: parsed.store.directory_file_id,
        },
        qualification_sha256: Some(pin),
        evidence_dir: PathBuf::from(parsed.evidence_dir),
        ubuntu: Recipe {
            recipe_id: parsed.ubuntu.recipe_id,
            recipe_sha256: parsed.ubuntu.recipe_sha256,
        },
        windows: Recipe {
            recipe_id: parsed.windows.recipe_id,
            recipe_sha256: parsed.windows.recipe_sha256,
        },
        idle_delay_s: parsed.idle_delay_s,
    };
    let (daemon_config, evidence_pins, _input_pins, _daemon) = bind_plan(&plan)?;
    run_with_inputs(&plan, &daemon_config, &evidence_pins, Journey::Vault).await
}

pub(super) async fn after_admission(
    client: &Client,
    plan: &Plan,
    evidence: &mut Evidence,
) -> Result<()> {
    let pin = plan
        .qualification_sha256
        .as_deref()
        .ok_or(Failure::Check("qualification_required"))?;
    let selected = evidence
        .qualification_sha256
        .as_deref()
        .ok_or(Failure::DaemonQualificationEvidenceUnavailable)?;
    check(selected == pin, "qualification_pin")?;
    evidence.record("m4_qualification", json!({"sha256": selected}));

    let good_sha = checks::script_sha256(checks::GOOD_SCRIPT);
    let broken_sha = checks::script_sha256(checks::BROKEN_SCRIPT);
    check(good_sha != broken_sha, "script_digest")?;

    let added = add_version(client, checks::GOOD_SCRIPT).await?;
    record(evidence, "good_draft", &added)?;
    let good = opaque(&added["version_id"], 32)
        .ok_or(Failure::Check("good_version"))?
        .to_owned();
    let submitted_test = test_version(client, &good).await?;
    record(evidence, "good_test_submit", &submitted_test)?;
    let passed = settled(client, &good).await?;
    record(evidence, "good_test", &passed)?;
    let script = opaque(&passed["script_id"], 32)
        .ok_or(Failure::Check("script_id"))?
        .to_owned();

    let activate_good = vault_ok(
        client,
        "vault_activate",
        json!({"version_id": &good, "request_id": request_id()?}),
    )
    .await?;
    record(evidence, "good_activation", &activate_good)?;
    let activated = vault_ok(client, "vault_get", json!({"version_id": &good})).await?;
    record(evidence, "good_active", &activated)?;
    let active_before = vault_ok(client, "vault_list", json!({})).await?;
    record(evidence, "active_before", &active_before)?;

    let broken_added = add_version(client, checks::BROKEN_SCRIPT).await?;
    record(evidence, "broken_draft", &broken_added)?;
    let broken = opaque(&broken_added["version_id"], 32)
        .ok_or(Failure::Check("broken_version"))?
        .to_owned();
    check(broken != good, "distinct_versions")?;
    let broken_submit = test_version(client, &broken).await?;
    record(evidence, "broken_test_submit", &broken_submit)?;
    let broken_operation = opaque(&broken_submit["operation_id"], 32)
        .ok_or(Failure::Check("broken_operation"))?
        .to_owned();
    check(
        opaque(&broken_submit["version_id"], 32) == Some(broken.as_str()),
        "broken_operation",
    )?;
    let rejected = settled(client, &broken).await?;
    record(evidence, "broken_test", &rejected)?;
    let activate_broken = vault_call(
        client,
        "vault_activate",
        json!({"version_id": &broken, "request_id": request_id()?}),
    )
    .await?;
    record(evidence, "broken_activation", &activate_broken)?;
    let list = vault_ok(client, "vault_list", json!({})).await?;
    record(evidence, "vault_list", &list)?;
    let activity = vault_ok(client, "list_activity", json!({"source": "vault"})).await?;
    record(evidence, "vault_activity", &activity)?;

    let proof = checks::accept_vault(
        &checks::Observed {
            passed,
            activate_good,
            activated,
            broken: rejected,
            activate_broken,
            active_before,
            list,
            activity,
        },
        &checks::Submitted {
            script_id: script,
            good_version_id: good,
            broken_version_id: broken,
            broken_operation_id: broken_operation,
        },
        &good_sha,
        &broken_sha,
    )
    .map_err(map_fail)?;
    evidence.record("vault_acceptance", proof);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "owner-host acceptance requires a qualified M4 TEST runtime"]
async fn owner_host_m5() {
    if let Err(error) = run().await {
        panic!("M5 acceptance {error:?}");
    }
}

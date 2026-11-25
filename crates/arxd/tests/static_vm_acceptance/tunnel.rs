use super::*;

/// This module is reachable only through the ignored owner public-tunnel entry point.
pub(super) async fn journey(
    accept: &mut Acceptance<'_>,
    plan: &StaticPlan,
    vm: &str,
    directory: &Path,
    origin: &StaticOriginWitness,
    ca: &[u8],
) -> Result<()> {
    let workspace = directory.join(&accept.evidence.run_id);
    safe(fs::create_dir(&workspace), "witness_workspace")?;
    let _workspace_pin = open(&workspace, true, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
    safe(
        arxd::config::checked_owner_private_path(&workspace),
        "witness_permissions",
    )?;
    let mut reference = None;
    let result = async {
        for phase in ["first", "republished"] {
            assert_off(accept, vm).await?;
            let (receipt, _, _) = accept
                .vm_op("publish_site", "publish", 15, json!({"vm_id":vm}), VM_WORK)
                .await?;
            check(receipt["state"] == "succeeded", "publish_operation_failed")?;
            accept.ledger.read(|ledger| {
                ledger.published_pair(
                    vm,
                    &plan.compose.primary.hostname,
                    &plan.compose.secondary.hostname,
                )
            })?;
            let status = rpc(accept.client, "publish_status", json!({"vm_id":vm})).await?;
            let hosts = status["hosts"]
                .as_array()
                .ok_or(Failure::Check("published_routes"))?;
            check(
                status["phase"] == "published"
                    && status["host_count"] == 2
                    && hosts.len() == 2
                    && service_pairs(plan).iter().all(|(_, project, _)| {
                        hosts.iter().any(|child| {
                            child["project_id"] == *project
                                && child["state"] == "verified"
                                && child["ambiguous"] == false
                        })
                    }),
                "published_routes",
            )?;
            if reference.is_none() {
                reference = Some(media_reference(plan, origin, ca).await?);
            }
            let proof = observe(
                &workspace,
                phase,
                reference
                    .as_ref()
                    .ok_or(Failure::Check("media_reference"))?,
            )
            .await?;
            accept
                .evidence
                .record(phase, json!({"independent_witness_sha256":proof}));
            if phase == "first" {
                let (stop, _, _) = accept
                    .vm_op("vm_stop", "stop", 4, json!({"vm_id":vm}), VM_WORK)
                    .await?;
                succeeded(&stop)?;
                assert_off(accept, vm).await?;
                let (start, _, _) = accept
                    .vm_op("vm_start", "start", 3, json!({"vm_id":vm}), VM_WORK)
                    .await?;
                succeeded(&start)?;
                let restarted = rpc(accept.client, "static_vm_get", json!({"vm_id":vm})).await?;
                let new_origin = static_origin_witness(
                    &restarted,
                    vm,
                    &plan.static_vm.generation,
                    &hex::encode(id(&start, "operation_id")?),
                    &plan.public_egress.policy_sha256,
                )?;
                check(
                    new_origin.witness_id != origin.witness_id,
                    "tunnel_restart_witness",
                )?;
            }
        }
        Ok(())
    }
    .await;
    // Attempt cleanup even if an external observer times out or publication partially succeeded.
    let cleanup = async {
        let status = rpc(accept.client, "publish_status", json!({"vm_id":vm})).await?;
        if status["phase"] != "unpublished" {
            let (off, _, _) = accept
                .vm_op(
                    "unpublish_site",
                    "unpublish",
                    16,
                    json!({"vm_id":vm}),
                    VM_WORK,
                )
                .await?;
            succeeded(&off)?;
        }
        assert_off(accept, vm).await
    }
    .await;
    cleanup?;
    let witnessed = async {
        result?;
        let cleanup = observe(
            &workspace,
            "cleanup",
            reference
                .as_ref()
                .ok_or(Failure::Check("media_reference"))?,
        )
        .await?;
        accept.evidence.record(
            "unpublish_cleanup",
            json!({"independent_witness_sha256":cleanup}),
        );
        Ok(())
    }
    .await;
    let state = rpc(accept.client, "static_vm_get", json!({"vm_id":vm})).await?;
    if state["state"] != "stopped" {
        let (stop, _, _) = accept
            .vm_op("vm_stop", "stop", 4, json!({"vm_id":vm}), VM_WORK)
            .await?;
        succeeded(&stop)?;
    }
    witnessed
}
async fn media_reference(
    plan: &StaticPlan,
    origin: &StaticOriginWitness,
    ca: &[u8],
) -> Result<Value> {
    let primary = &plan.compose.primary;
    let media = https_origin_request(
        origin.origin_port,
        &origin.origin_server_name,
        ca,
        "GET",
        &primary.media_path,
        &primary.hostname,
        &[],
        None,
    )
    .await?;
    check(
        media.status == 200 && primary.range_end < media.body.len(),
        "origin_reference_media",
    )?;
    Ok(
        json!({"hosts":[{"name":primary.hostname,"path":primary.marker_path,"marker":primary.expected_marker},
        {"name":plan.compose.secondary.hostname,"path":plan.compose.secondary.marker_path,"marker":plan.compose.secondary.expected_marker}],
        "media_path":primary.media_path,"start":primary.range_start,"end":primary.range_end,"total":media.body.len(),
        "full_sha256":hex::encode(Sha256::digest(&media.body)),
        "range_sha256":hex::encode(Sha256::digest(&media.body[primary.range_start..=primary.range_end]))}),
    )
}

async fn assert_off(accept: &Acceptance<'_>, vm: &str) -> Result<()> {
    let status = rpc(accept.client, "publish_status", json!({"vm_id":vm})).await?;
    check(
        status["phase"] == "unpublished"
            && status["host_count"] == 0
            && status["hosts"].as_array().is_some_and(Vec::is_empty),
        "tunnel_not_clean",
    )?;
    accept.ledger.read(Ledger::publication_empty)
}

async fn observe(dir: &Path, phase: &'static str, reference: &Value) -> Result<String> {
    let challenge = hex::encode(fresh()?);
    let request = json!({"version":1,"phase":phase,"challenge":challenge,"reference":reference});
    write_new(
        &dir.join(format!("{phase}-request.json")),
        &safe(serde_json::to_vec_pretty(&request), "witness_request")?,
    )?;
    let response_path = dir.join(format!("{phase}-response.json"));
    let response = poll(
        "independent_public_witness",
        Duration::from_secs(900),
        TICK,
        || async {
            match response_path.symlink_metadata() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(Failure::Check("public_witness_path")),
                Ok(_) => {
                    let file = open(&response_path, false, FILE_SHARE_READ)?;
                    safe(
                        arxd::config::checked_owner_private_path(&response_path),
                        "public_witness_permissions",
                    )?;
                    Ok(Some(config::<Value>(&file)?))
                }
            }
        },
    )
    .await?;
    verify_public_witness(&request, &response)?;
    Ok(hex::encode(Sha256::digest(safe(
        serde_json::to_vec(&response),
        "witness_digest",
    )?)))
}

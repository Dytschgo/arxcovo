async fn add_vault_version(client: &Client, nonce: u64, script: &str) -> Value {
    let request_id = format!("{:016x}{nonce:016x}", crate::vault::utc_ms().unwrap());
    let reply = call(
        client,
        "vault_add_version",
        json!({"os":"ubuntu","name":"build","script":script,"request_id":request_id}),
    )
    .await;
    assert_eq!(reply["ok"], true, "{reply}");
    reply["version_id"].clone()
}

fn vault_write_args(version: &Value, nonce: u64) -> Value {
    json!({
        "version_id":version,
        "request_id":format!("{:016x}{nonce:016x}", crate::vault::utc_ms().unwrap())
    })
}

async fn vault_version(client: &Client, version: &Value) -> Value {
    let row = call(client, "vault_get", json!({"version_id":version})).await;
    assert_eq!(row["ok"], true, "{row}");
    assert!(!row.to_string().contains(VAULT_CANARY));
    row
}

async fn vault_result(client: &Client, version: &Value) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let row = vault_version(client, version).await;
            if !row["test"].is_null() {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("vault test did not settle")
}

fn vault_vm_counts(harness: &Harness) -> (i64, i64, i64) {
    read_state(harness)
        .query_row(
            "SELECT (SELECT count(*) FROM vm_operations WHERE principal='arxd' AND action=1),
                    (SELECT count(*) FROM vm_instances WHERE principal='arxd'),
                    (SELECT count(*) FROM vm_disk_claims d JOIN vm_instances v ON d.vm=v.id
                     WHERE v.principal='arxd')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
}

async fn seal_vault_template(harness: &Harness, client: &Client, nonce: u64) {
    let build = call(
        client,
        "vm_template_build",
        json!({"recipe_id":hex::encode(RECIPE.0),"request_id":key(nonce)}),
    )
    .await;
    assert_eq!(build["ok"], true, "{build}");
    // This worker is the only job in the fresh fixture. Join it instead of
    // querying SQLite repeatedly while the fixture is sealing the template.
    let jobs = harness.fixture(Fixture::take_jobs);
    harness.hold_build.notify_one();
    tokio::time::timeout(Duration::from_secs(15), jobs.join())
        .await
        .expect("template fixture worker timed out")
        .expect("template fixture worker failed");

    let sealed = call(
        client,
        "vm_operation",
        json!({"operation_id":build["operation_id"]}),
    )
    .await;
    assert_eq!(sealed["state"], "succeeded", "{sealed}");
    assert!(sealed["template_id"].is_string(), "{sealed}");
}

#[tokio::test]
async fn vault_tester_requires_exit_and_reconciled_throwaway_custody() {
    assert!(log::STATIC_MAX_LEVEL <= log::LevelFilter::Info);
    assert!(tracing::level_filters::STATIC_MAX_LEVEL <= tracing::level_filters::LevelFilter::INFO);
    let mut h = Harness::start(false, false).await;
    h.supervisor().open_vault(h.temp.path()).unwrap();
    let client = h.client(0).await;
    let other = h.client(1).await;
    assert!(
        client
            .list_tools(None)
            .await
            .unwrap()
            .tools
            .iter()
            .any(|tool| tool.name == "vault_test_version")
    );
    seal_vault_template(&h, &client, 601).await;

    let good = add_vault_version(&client, 1, VAULT_EXIT_0).await;
    let args = vault_write_args(&good, 2);
    let receipt = call(&client, "vault_test_version", args.clone()).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    assert!(!receipt.to_string().contains(VAULT_CANARY));
    assert_eq!(
        call(&client, "vault_test_version", args.clone()).await,
        receipt
    );
    assert_eq!(
        call(&client, "vault_activate", vault_write_args(&good, 3)).await["reason"],
        "pass_required"
    );
    assert_eq!(
        call(&client, "vm_list", json!({})).await["entries"],
        json!([])
    );
    h.hold_script.notify_one();
    let passed = vault_result(&client, &good).await;
    assert_eq!(
        passed["test"],
        json!({"status":"exited","exit_code":0,"cleanup_confirmed":true})
    );
    assert_eq!(call(&client, "vault_test_version", args).await, receipt);
    assert_eq!(vault_vm_counts(&h), (1, 0, 0));
    assert_eq!(
        call(&client, "vault_activate", vault_write_args(&good, 4)).await["ok"],
        true
    );

    let broken = add_vault_version(&client, 5, VAULT_EXIT_7).await;
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&broken, 6)).await["ok"],
        true
    );
    let rejected = vault_result(&client, &broken).await;
    assert_eq!(rejected["state"], "rejected");
    assert_eq!(
        rejected["test"],
        json!({"status":"exited","exit_code":7,"cleanup_confirmed":true})
    );
    assert_eq!(vault_vm_counts(&h), (2, 0, 0));
    assert_eq!(
        call(&client, "vault_activate", vault_write_args(&broken, 7)).await["reason"],
        "pass_required"
    );

    let draft = add_vault_version(&client, 8, VAULT_EXIT_0).await;
    let fixture = h.supervisor().lock().unwrap().fixture.take();
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&draft, 9)).await["reason"],
        "vm_admission_closed"
    );
    h.supervisor().lock().unwrap().fixture = fixture;
    assert_eq!(vault_version(&client, &draft).await["state"], "draft");
    assert_eq!(vault_vm_counts(&h), (2, 0, 0));
    assert_eq!(
        call(&other, "vault_get", json!({"version_id":good})).await["reason"],
        "not_found"
    );
    assert_eq!(
        call(&client, "vault_list", json!({})).await["entries"][0]["active_version_id"],
        good
    );
    let audit = call(&client, "list_activity", json!({"source":"vault"})).await;
    assert!(!audit.to_string().contains(VAULT_CANARY));
    assert!(
        audit["activities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["tool"] == "vault_test_version")
    );
    assert_eq!(
        call(&client, "vm_list", json!({})).await["entries"],
        json!([])
    );
    assert!(
        !fs::read(h.temp.path().join("activity.sqlite3"))
            .unwrap()
            .windows(VAULT_CANARY.len())
            .any(|window| window == VAULT_CANARY.as_bytes())
    );
    drop(other);
    drop(client);
    h.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_tester_quarantines_unconfirmed_disposal() {
    let mut h = Harness::start(false, false).await;
    h.supervisor().open_vault(h.temp.path()).unwrap();
    let client = h.client(0).await;
    seal_vault_template(&h, &client, 602).await;

    let version = add_vault_version(&client, 1, VAULT_EXIT_0).await;
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&version, 2)).await["ok"],
        true
    );
    until(|| h.fixture(|fixture| fixture.script_hold.is_none())).await;
    h.fixture(|fixture| {
        let mut live = fixture.live.lock().unwrap();
        let vm = *live.boots.keys().next().unwrap();
        live.stuck.insert(vm);
    });
    h.hold_script.notify_one();
    let row = vault_result(&client, &version).await;
    assert_eq!(row["state"], "rejected");
    assert_eq!(
        row["test"],
        json!({"status":"exited","exit_code":0,"cleanup_confirmed":false})
    );
    assert_eq!(vault_vm_counts(&h), (1, 1, 1));
    assert_eq!(
        call(&client, "vault_activate", vault_write_args(&version, 3)).await["reason"],
        "pass_required"
    );
    let audit = call(&client, "list_activity", json!({"source":"vault"})).await;
    assert!(
        audit["activities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["reason"] == "cleanup_unconfirmed")
    );
    assert!(!audit.to_string().contains(VAULT_CANARY));
    drop(client);
    assert_eq!(
        h.shutdown().await.unwrap_err().to_string(),
        "server-drain-unconfirmed"
    );
    assert_eq!(vault_vm_counts(&h), (1, 1, 1));
}

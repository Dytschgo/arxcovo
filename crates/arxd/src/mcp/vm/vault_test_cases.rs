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
    tokio::time::timeout(Duration::from_secs(12), async {
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

async fn seal_vault_template(harness: &Harness, client: &Client, nonce: u64) -> Value {
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
    sealed["template_id"].clone()
}

async fn wait_for_vault_script(harness: &Harness) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while harness.fixture(|fixture| fixture.script_hold.is_some()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("vault guest worker was not admitted");
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
async fn vault_tester_retains_charge_when_next_retry_exceeds_cutoff() {
    let mut h = Harness::start(false, false).await;
    h.supervisor().open_vault(h.temp.path()).unwrap();
    let client = h.client(0).await;
    seal_vault_template(&h, &client, 602).await;

    let version = add_vault_version(&client, 1, VAULT_EXIT_0).await;
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&version, 2)).await["ok"],
        true
    );
    wait_for_vault_script(&h).await;
    h.fixture(|fixture| {
        let mut live = fixture.live.lock().unwrap();
        let vm = *live.boots.keys().next().unwrap();
        live.stuck.insert(vm);
    });
    h.hold_script.notify_one();
    let row = vault_result(&client, &version).await;
    assert_eq!(row["state"], "test_inconclusive");
    assert_eq!(
        row["test"],
        json!({"status":"test_inconclusive","cleanup_confirmed":false})
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
            .any(|row| row["reason"] == "test_inconclusive")
    );
    assert!(!audit.to_string().contains(VAULT_CANARY));
    drop(client);
    assert_eq!(
        h.shutdown().await.unwrap_err().to_string(),
        "server-drain-unconfirmed"
    );
    assert_eq!(vault_vm_counts(&h), (1, 1, 1));
}

#[tokio::test]
async fn vault_tester_gets_next_turn_ahead_of_periodic_backlog() {
    let mut h = Harness::start(false, false).await;
    h.supervisor().open_vault(h.temp.path()).unwrap();
    let client = h.client(0).await;
    let template = seal_vault_template(&h, &client, 603).await;
    for nonce in [604, 605] {
        let older = call(
            &client,
            "vm_create",
            json!({"template_id":template,"request_id":key(nonce),"ttl_s":60}),
        )
        .await;
        assert_eq!(older["ok"], true, "{older}");
        tokio::time::timeout(
            Duration::from_secs(15),
            h.fixture(Fixture::take_jobs).join(),
        )
        .await
        .unwrap()
        .unwrap();
        let older = call(
            &client,
            "vm_operation",
            json!({"operation_id":older["operation_id"]}),
        )
        .await;
        assert_eq!(older["state"], "succeeded", "{older}");
    }

    let version = add_vault_version(&client, 1, VAULT_EXIT_0).await;
    let receipt = call(&client, "vault_test_version", vault_write_args(&version, 2)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    wait_for_vault_script(&h).await;
    // An ordinary turn selects both old leases before the vault is due.
    // Hold the first item while the vault queues, and park a second hook
    // that would keep the vault behind the whole batch without yielding.
    h.now.store(UTC + 60_000, Ordering::SeqCst);
    let reaper = h.fixture(|f| f.reaper());
    let mut stall = Stall::install(&reaper.after_selection);
    let controller = h.supervisor().controller.clone();
    let batch_reaper = reaper.clone();
    let sweep = tokio::task::spawn_blocking(move || {
        controller.sweep_with(&batch_reaper, &*batch_reaper.custody)
    });
    stall.entered().await;
    let mut second = Stall::install(&reaper.after_selection);

    let (waiting, observed) = tokio::sync::oneshot::channel();
    *h.supervisor().vault_cleanup_waiting.lock().unwrap() = Some(waiting);
    h.hold_script.notify_one();
    tokio::time::timeout(Duration::from_secs(5), observed)
        .await
        .unwrap()
        .unwrap();
    assert!(vault_version(&client, &version).await["test"].is_null());
    assert_eq!(vault_vm_counts(&h), (1, 1, 1));

    stall.release_and_join().await;
    let passed = tokio::select! {
        biased;
        _ = second.entered() => panic!("ordinary backlog started another item before vault custody"),
        result = vault_result(&client, &version) => result,
    };
    assert!(sweep.await.unwrap().unwrap());
    assert!(second.not_entered());
    assert_eq!(
        passed["test"],
        json!({"status":"exited","exit_code":0,"cleanup_confirmed":true})
    );
    assert_eq!(vault_vm_counts(&h), (1, 0, 0));
    let remaining = || {
        read_state(&h)
            .query_row(
                "SELECT count(*) FROM vm_instances WHERE principal='alpha'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    };
    assert_eq!(remaining(), 1, "the next ordinary item was deferred");
    h.sweep().await;
    assert_eq!(remaining(), 0);
    assert_eq!(
        call(&client, "vault_activate", vault_write_args(&version, 3)).await["ok"],
        true
    );
    drop(client);
    h.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_tester_drain_joins_pending_guest_and_vault_settlement() {
    let mut h = Harness::start(false, false).await;
    let supervisor = h.supervisor().clone();
    supervisor.open_vault(h.temp.path()).unwrap();
    let client = h.client(0).await;
    seal_vault_template(&h, &client, 605).await;

    let version = add_vault_version(&client, 1, VAULT_EXIT_0).await;
    let receipt = call(&client, "vault_test_version", vault_write_args(&version, 2)).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    wait_for_vault_script(&h).await;
    let mut teardown = Stall::install(&supervisor.stall_vault_teardown);
    let mut recovery = Stall::install(&supervisor.stall_recovery);
    let (mut lock, holder) = hold_vault_custody_lock(&h);
    lock.entered().await;
    let (waiting, observed) = tokio::sync::oneshot::channel();
    *supervisor.vault_drain_waiting.lock().unwrap() = Some(waiting);
    let mut server = h.server.take().unwrap();
    h.interrupt.take().unwrap().send(()).unwrap();
    teardown.entered().await;
    tokio::select! {
        _ = recovery.entered() => panic!("recovery started before the vault job settled"),
        result = &mut server => panic!("foreground returned before the vault job settled: {result:?}"),
        result = tokio::time::timeout(Duration::from_secs(3), observed) => result.unwrap().unwrap(),
    }
    assert!(recovery.not_entered());
    assert_eq!(vault_vm_counts(&h), (1, 1, 1));

    teardown.release_and_join().await;
    recovery.entered().await;
    let result = crate::vault::mcp::invoke(
        supervisor.vault.get().unwrap(),
        "alpha",
        "vault_get",
        json!({"version_id":version}).as_object().unwrap().clone(),
        None,
    )
    .unwrap();
    assert_eq!(result["state"], "test_inconclusive");
    assert_eq!(
        result["test"],
        json!({"status":"test_inconclusive","cleanup_confirmed":false})
    );
    // The vault has settled while the custody lock is still held. Recovery
    // can only clear the charged clone after the holder is released.
    assert_eq!(vault_vm_counts(&h), (1, 1, 1));
    lock.release_and_join().await;
    holder.await.unwrap();
    recovery.release_and_join().await;
    server.await.unwrap().unwrap();
    assert_eq!(vault_vm_counts(&h), (1, 0, 0));
}

fn hold_vault_custody_lock(harness: &Harness) -> (Stall, JoinHandle<()>) {
    let slot = Mutex::default();
    let stall = Stall::install(&slot);
    let hook = slot.into_inner().unwrap().unwrap();
    let controller = harness.supervisor().controller.clone();
    let holder = tokio::task::spawn_blocking(move || {
        controller.hold_sweeps_for_test(|| drop(hook.enter()));
    });
    (stall, holder)
}

#[tokio::test]
async fn vault_tester_timeout_is_inconclusive_and_same_version_can_retry() {
    let mut h = Harness::start(false, false).await;
    let supervisor = h.supervisor().clone();
    supervisor.open_vault(h.temp.path()).unwrap();
    let client = h.client(0).await;
    seal_vault_template(&h, &client, 606).await;

    let first = add_vault_version(&client, 1, VAULT_EXIT_0).await;
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&first, 2)).await["ok"],
        true
    );
    h.hold_script.notify_one();
    assert_eq!(
        vault_result(&client, &first).await["test"]["cleanup_confirmed"],
        true
    );
    assert_eq!(
        call(&client, "vault_activate", vault_write_args(&first, 3)).await["ok"],
        true
    );

    let replacement = add_vault_version(&client, 4, VAULT_EXIT_0).await;
    let old_args = vault_write_args(&replacement, 5);
    let (mut lock, holder) = hold_vault_custody_lock(&h);
    lock.entered().await;
    let receipt = call(&client, "vault_test_version", old_args.clone()).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    let inconclusive = vault_result(&client, &replacement).await;
    assert_eq!(inconclusive["state"], "test_inconclusive");
    assert_eq!(
        inconclusive["test"],
        json!({"status":"test_inconclusive","cleanup_confirmed":false})
    );
    assert_eq!(call(&client, "vault_test_version", old_args).await, receipt);
    assert_eq!(
        call(&client, "vault_activate", vault_write_args(&replacement, 6)).await["reason"],
        "pass_required"
    );
    assert_eq!(vault_version(&client, &first).await["state"], "active");
    assert_eq!(vault_vm_counts(&h), (2, 1, 1));

    lock.release_and_join().await;
    holder.await.unwrap();
    h.sweep().await;
    let retry = call(
        &client,
        "vault_test_version",
        vault_write_args(&replacement, 7),
    )
    .await;
    assert_eq!(retry["ok"], true, "{retry}");
    assert_ne!(retry["operation_id"], receipt["operation_id"]);
    assert_eq!(
        vault_result(&client, &replacement).await["test"]["cleanup_confirmed"],
        true
    );
    assert_eq!(
        call(&client, "vault_activate", vault_write_args(&replacement, 8)).await["ok"],
        true
    );
    assert_eq!(vault_vm_counts(&h), (3, 0, 0));
    drop(client);
    h.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_tester_reserves_an_ordinary_turn_across_successive_submissions() {
    let mut h = Harness::start(false, false).await;
    let supervisor = h.supervisor().clone();
    supervisor.open_vault(h.temp.path()).unwrap();
    let client = h.client(0).await;
    let template = seal_vault_template(&h, &client, 607).await;
    let ordinary = call(
        &client,
        "vm_create",
        json!({"template_id":template,"request_id":key(608),"ttl_s":60}),
    )
    .await;
    assert_eq!(ordinary["ok"], true, "{ordinary}");
    h.fixture(Fixture::take_jobs).join().await.unwrap();

    let first = add_vault_version(&client, 1, VAULT_EXIT_0).await;
    let mut first_queued = Stall::install(&supervisor.stall_vault_acquisition);
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&first, 2)).await["ok"],
        true
    );
    h.hold_script.notify_one();
    first_queued.entered().await;
    h.sweep().await;
    assert_eq!(vault_vm_counts(&h), (1, 1, 1), "first ordinary turn yields");
    first_queued.release_and_join().await;
    assert_eq!(
        vault_result(&client, &first).await["test"]["cleanup_confirmed"],
        true
    );

    let second = add_vault_version(&client, 3, VAULT_EXIT_0).await;
    let mut second_queued = Stall::install(&supervisor.stall_vault_acquisition);
    let (waiting, observed) = tokio::sync::oneshot::channel();
    *supervisor.vault_cleanup_waiting.lock().unwrap() = Some(waiting);
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&second, 4)).await["ok"],
        true
    );
    second_queued.entered().await;
    h.now.store(UTC + 60_000, Ordering::SeqCst);
    second_queued.release_and_join().await;
    tokio::time::timeout(Duration::from_secs(4), observed)
        .await
        .expect("vault must defer to the reserved ordinary turn")
        .unwrap();
    assert_eq!(vault_vm_counts(&h), (2, 1, 1));
    h.sweep().await;
    assert_eq!(
        read_state(&h)
            .query_row(
                "SELECT count(*) FROM vm_instances WHERE principal='alpha'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0,
        "owed turn retired the unrelated lease"
    );
    assert_eq!(
        vault_result(&client, &second).await["test"]["cleanup_confirmed"],
        true
    );
    drop(client);
    h.shutdown().await.unwrap();
}

#[tokio::test]
async fn vault_missing_sealed_template_allows_same_version_retry() {
    let mut h = Harness::start(false, false).await;
    h.supervisor().open_vault(h.temp.path()).unwrap();
    let client = h.client(0).await;
    let version = add_vault_version(&client, 1, VAULT_EXIT_0).await;
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&version, 2)).await["ok"],
        true
    );
    assert_eq!(
        vault_result(&client, &version).await["state"],
        "test_inconclusive"
    );
    assert_eq!(vault_vm_counts(&h), (0, 0, 0));
    seal_vault_template(&h, &client, 608).await;
    assert_eq!(
        call(&client, "vault_test_version", vault_write_args(&version, 3)).await["ok"],
        true
    );
    h.hold_script.notify_one();
    assert_eq!(
        vault_result(&client, &version).await["test"]["cleanup_confirmed"],
        true
    );
    drop(client);
    h.shutdown().await.unwrap();
}

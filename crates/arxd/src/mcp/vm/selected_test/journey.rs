//! One foreground, one native ledger and both TEST recipes.

use super::*;
use crate::mcp::vm::tests::{UTC, request};
use crate::vm::qualification::{FixtureQualification, StartPair};
use crate::vm::runtime::ScriptAuthority;
use crate::vm::test_issuer::tests::{config, profile};
use arx_guest::protocol::ScriptStatus;
use std::sync::atomic::AtomicI64;

pub(super) async fn run(
    source: &FixtureQualification,
    supervisor: &Supervisor,
    owner: &native_run::Run,
    start: native_run::Start,
    issuer: &SelectedTest,
) -> anyhow::Result<()> {
    let target = owner.target();
    let pair = StartPair {
        slots: start.active.pair.0,
        evidence: start.active.pair.1,
    };
    let stopped = || owner.stop_signalled() || supervisor.cancel.is_cancelled();
    let revoked = || issuer.epoch().effects_revoked();
    let Ok(workspace) = source.hold_run(
        &supervisor.controller,
        target.generation,
        target.epoch,
        pair,
        start.worker.deadline(),
        &stopped,
        &revoked,
    ) else {
        // As in production: nothing was created, so the committed epoch
        // goes to recovery and the start settles instead of staying pending.
        owner.hand_off(supervisor).await;
        return Ok(());
    };
    let mut launch = GuestPeer {
        source,
        issuer,
        generation: target.generation,
        epoch: target.epoch,
    };
    owner
        .live(
            supervisor,
            &start.worker,
            &mut launch,
            issuer,
            || issuer.epoch().effects_revoked(),
            async |()| {
                let proof = source.release_run(&workspace, pair);
                owner.settle_proof(supervisor, &proof).await;
            },
        )
        .await;
    Ok(())
}

struct GuestPeer<'a> {
    source: &'a FixtureQualification,
    issuer: &'a SelectedTest,
    generation: Id,
    epoch: Id,
}

impl GuestPeer<'_> {
    /// One sample per synthetic exchange. A sample refused by transient
    /// store contention is retried like any background worker read, but
    /// never past cancellation or revocation.
    async fn sample(
        &self,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<Instant, NativeRefusal> {
        crate::vm::test_issuer::retry_contended_async(
            &|| cancel.is_cancelled(),
            &|| self.issuer.epoch().effects_revoked(),
            refresh,
        )
        .await
    }
}

impl native_run::LiveLaunch for GuestPeer<'_> {
    type Proof = ();

    async fn boot(
        &mut self,
        cancel: &Shutdown,
        _: Instant,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        self.sample(cancel, refresh).await.map(|_| ())
    }

    fn check_live(
        &self,
        refresh: impl FnOnce() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        refresh().map(|_| ())
    }

    async fn ping(
        &mut self,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(), NativeRefusal> {
        self.sample(cancel, refresh).await.map(|_| ())
    }

    async fn run_script(
        &mut self,
        authority: ScriptAuthority,
        script: String,
        timeout_s: u32,
        deadline: Instant,
        cancel: &Shutdown,
        refresh: impl FnMut() -> Result<Instant, NativeRefusal>,
    ) -> Result<(ScriptStatus, Option<u32>), NativeRefusal> {
        if Instant::now() >= deadline
            || cancel.is_cancelled()
            || !authority.names(self.generation, self.epoch, &script, timeout_s)
        {
            return Err(NativeRefusal::GuestEndpointUnverified);
        }
        self.sample(cancel, refresh).await?;
        assert_eq!(script.trim(), "exit 0");
        Ok((ScriptStatus::Exited, Some(0)))
    }

    async fn stop(&mut self) -> Option<()> {
        while self.source.hold_stop.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Some(())
    }
}

// A failing assertion must still wake the blocking live worker.
struct CloseOnDrop(Arc<Supervisor>);
impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.0.close();
    }
}

struct Session {
    inventory: VmInventory,
    _connection: Connection,
    now: Arc<AtomicI64>,
    nonce: u64,
    last: String,
}

impl Session {
    fn new(supervisor: &Arc<Supervisor>, now: Arc<AtomicI64>) -> Self {
        let connection = supervisor.connection("alice".into());
        Self {
            inventory: connection.inventory(),
            _connection: connection,
            now,
            nonce: 0,
            last: String::new(),
        }
    }

    fn call(&self, name: &str, args: Value) -> Outcome<Value> {
        self.inventory.dispatch(request(name, args), || false)
    }

    fn mutate(&mut self, name: &str, args: Value) -> Outcome<Value> {
        self.nonce += 1;
        self.last = format!(
            "{:016x}{:016x}",
            self.now.load(Ordering::SeqCst),
            self.nonce
        );
        self.replay(name, args)
    }

    /// The same request again, under the previous request ID.
    fn replay(&self, name: &str, mut args: Value) -> Outcome<Value> {
        args["request_id"] = json!(self.last);
        self.call(name, args)
    }

    async fn succeeded(&self, receipt: Value) -> Value {
        let id = &receipt["operation_id"];
        assert!(id.is_string(), "{receipt}");
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let row = self
                    .call("vm_operation", json!({"operation_id":id}))
                    .unwrap();
                if row["state"] != "pending" {
                    assert_eq!(row["state"], "succeeded", "{row}");
                    // A worker settles its operation before it drops its
                    // staging registration; the next request on the VM
                    // waits for that, as vault teardown does.
                    if let Some(vm) = row["vm_id"].as_str() {
                        let vm = parse_id(vm).unwrap();
                        let staging = &self.inventory.supervisor.test_staging;
                        while !matches!(staging.with_entry(vm, |owner| owner.is_none()), Ok(true)) {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    }
                    return row;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("foreground operation did not settle")
    }

    /// Ordinary reaper turns until `done`, as the periodic reaper would run.
    async fn sweep_until(&self, supervisor: &Arc<Supervisor>, done: impl Fn(&Self) -> bool) {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let supervisor = supervisor.clone();
                tokio::task::spawn_blocking(move || supervisor.sweep())
                    .await
                    .unwrap()
                    .unwrap();
                if done(self) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(60)).await;
            }
        })
        .await
        .expect("ordinary reaper turns did not settle");
    }

    fn count(&self, sql: &str) -> i64 {
        self.inventory
            .supervisor
            .controller
            .native_store()
            .read_only(|db| crate::store::vm::count(db, sql, []))
            .unwrap()
    }

    fn vm_state(&self, vm: Id) -> i64 {
        self.inventory
            .supervisor
            .controller
            .native_store()
            .read_only(|db| {
                Ok(
                    db.query_row("SELECT state FROM vm_instances WHERE id=?1", [vm], |row| {
                        row.get(0)
                    })?,
                )
            })
            .unwrap()
    }
}

fn id_of(value: &Value) -> Id {
    parse_id(value.as_str().unwrap()).unwrap()
}

/// The live owner registered for `vm`: `None` once it has left, otherwise
/// whether it still publishes a witness.
fn owner(supervisor: &Supervisor, vm: Id) -> Option<bool> {
    // The registry is try-locked; retry a momentary contention.
    for _ in 0..1000 {
        if let Ok(owner) = supervisor
            .native_runs
            .with_entry(vm, |owner| owner.map(|owner| owner.witnessed()))
        {
            return owner;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("TEST run registry unavailable");
}

fn set_busy(supervisor: &Supervisor, value: bool) {
    let Source::Fixture(source) = installed(supervisor).unwrap().source() else {
        panic!("fixture selection required");
    };
    source.busy.store(value, Ordering::SeqCst);
}

fn inventory(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    std::fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            entry.file_type().unwrap().is_dir().then_some(entry.path())
        })
        .flat_map(|dir| {
            let mut files = std::fs::read_dir(dir)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    assert!(entry.file_type().unwrap().is_file());
                    let path = entry.path();
                    (
                        path.strip_prefix(root).unwrap().to_owned(),
                        std::fs::read(path).unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(files.len(), 3);
            files.sort_by(|a, b| a.0.cmp(&b.0));
            assert_eq!(
                files
                    .iter()
                    .filter(|(path, bytes)| {
                        path.file_name().unwrap() == "generation.lease" && bytes.is_empty()
                    })
                    .count(),
                1
            );
            files
        })
        .collect()
}

#[tokio::test]
async fn closed_and_opted_in_foreground_use_the_same_native_two_golden_ledger() {
    let dir = crate::service::application::tests::private_temp();
    let root = dir.path();
    let now = Arc::new(AtomicI64::new(UTC));
    let origin = Instant::now();
    let source = FixtureQualification::open(root, origin);
    let recipes = crate::vm::test_issuer::SelectedSource::recipes(&source);
    let path = |leaf| root.join(leaf).to_str().unwrap().replace('\\', "/");
    let state = root.to_str().unwrap().replace('\\', "/");
    let mut profile = profile(&path("missing.json"), &path("dossier"));
    profile["state"] = json!(state);
    profile["volume"] = json!(&state[..3]);
    let runtime = VmRuntimeProfile::parse(&serde_json::to_vec(&profile).unwrap()).unwrap();
    let mut supervisor = Supervisor::new(
        root,
        VmPolicy::default(),
        ForegroundClock {
            utc: Some(now.clone()),
        },
        Some((recipes.ubuntu.recipe_id, recipes.ubuntu.recipe_sha256)),
        NativeRefusal::QualificationMissing,
        Some(runtime),
    )
    .unwrap();
    for path in [
        root.to_owned(),
        root.join("activity.sqlite3"),
        root.join("activity.lock"),
    ] {
        crate::dashboard::tests::set_owner_private(&path);
    }
    {
        let supervisor = Arc::get_mut(&mut supervisor).unwrap();
        supervisor.custody = Some(Arc::new(crate::config::VmCustody::test_only(root).unwrap()));
        // The production installation: the configured dossier is absent.
        supervisor.install_test();
    }

    let ubuntu_recipe = json!(hex::encode(recipes.ubuntu.recipe_id));
    let windows_recipe = json!(hex::encode(recipes.windows.recipe_id));
    {
        let mut session = Session::new(&supervisor, now.clone());
        assert_eq!(
            session.call("vm_catalog", json!({})).unwrap()["capability"]["available"],
            false
        );
        assert!(
            session
                .mutate("vm_template_build", json!({"recipe_id":ubuntu_recipe}))
                .is_err()
        );
        assert_eq!(session.count("SELECT count(*) FROM vm_operations"), 0);
        assert_eq!(session.count("SELECT count(*) FROM vm_disk_claims"), 0);
        assert_eq!(session.count("SELECT count(*) FROM vm_generations"), 0);
        assert!(inventory(root).is_empty());
    }

    Arc::get_mut(&mut supervisor)
        .unwrap()
        .install_test_with(|controller, selection| {
            TestIssuer::open_owned_fixture(controller, Source::Fixture(Box::new(source)), selection)
        });
    supervisor.startup_sweep().unwrap();
    let vault = crate::service::application::tests::private_temp();
    supervisor.open_vault(vault.path()).unwrap();
    let _close = CloseOnDrop(supervisor.clone());
    let mut session = Session::new(&supervisor, now.clone());

    // The catalog shows exactly the seal and both recipe pins before idle,
    // while every new effect still requires live idleness.
    let catalog = session.call("vm_catalog", json!({})).unwrap();
    assert_eq!(catalog["capability"]["available"], true);
    assert_eq!(
        catalog["capability"]["selected_test_qualification"],
        json!({
            "sha256": hex::encode(config().selection().seal_sha256),
            "recipes": [
                {"guest":"ubuntu","recipe_id":ubuntu_recipe,"recipe_sha256":hex::encode(recipes.ubuntu.recipe_sha256)},
                {"guest":"windows","recipe_id":windows_recipe,"recipe_sha256":hex::encode(recipes.windows.recipe_sha256)},
            ]
        })
    );
    assert!(supervisor.vault_test_available(&supervisor.lock().unwrap()));
    let denied = session
        .mutate("vm_template_build", json!({"recipe_id":ubuntu_recipe}))
        .unwrap_err();
    assert_eq!(denied.0, "owner_activity_unverified");
    assert_eq!(session.count("SELECT count(*) FROM vm_operations"), 0);
    assert_eq!(session.count("SELECT count(*) FROM vm_disk_claims"), 0);
    assert_eq!(session.count("SELECT count(*) FROM vm_generations"), 0);
    assert!(inventory(root).is_empty());
    set_busy(&supervisor, false);

    let now_ticks = now.clone();
    let advance = tokio::spawn(async move {
        loop {
            now_ticks.fetch_max(UTC + origin.elapsed().as_millis() as i64, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let ubuntu = session
        .mutate("vm_template_build", json!({"recipe_id":ubuntu_recipe}))
        .unwrap();
    let ubuntu_template = session.succeeded(ubuntu).await["template_id"].clone();
    let windows = session
        .mutate("vm_template_build", json!({"recipe_id":windows_recipe}))
        .unwrap();
    let windows_template = session.succeeded(windows).await["template_id"].clone();
    let goldens = inventory(root);
    assert_eq!(goldens.len(), 6);
    // A new build request reuses the exact sealed pair after the bounded
    // custody check, with no new generation, artifact or claim.
    let reuse = session
        .mutate("vm_template_build", json!({"recipe_id":windows_recipe}))
        .unwrap();
    session.succeeded(reuse).await;
    assert_eq!(session.count("SELECT count(*) FROM vm_generations"), 2);
    assert_eq!(inventory(root), goldens);

    let ubuntu = session
        .mutate(
            "vm_create",
            json!({"template_id":ubuntu_template,"ttl_s":1800}),
        )
        .unwrap();
    let ubuntu_vm = session.succeeded(ubuntu).await["vm_id"].clone();
    let windows = session
        .mutate(
            "vm_create",
            json!({"template_id":windows_template,"ttl_s":2400}),
        )
        .unwrap();
    let windows_vm = session.succeeded(windows).await["vm_id"].clone();

    let capture = session
        .mutate(
            "vm_snapshot_create",
            json!({"vm_id":ubuntu_vm,"name":"baseline"}),
        )
        .unwrap();
    let point = session.succeeded(capture).await["snapshot_id"].clone();
    let restore = session
        .mutate(
            "vm_snapshot_restore",
            json!({"vm_id":ubuntu_vm,"snapshot_id":point}),
        )
        .unwrap();
    session.succeeded(restore).await;
    // Deletion has no copy worker; the reaper disposes the point.
    let delete = session
        .mutate(
            "vm_snapshot_delete",
            json!({"vm_id":ubuntu_vm,"snapshot_id":point}),
        )
        .unwrap()["operation_id"]
        .clone();
    session
        .sweep_until(&supervisor, |session| {
            session.count("SELECT count(*) FROM vm_restore_points") == 0
                && session
                    .call("vm_operation", json!({"operation_id":delete}))
                    .unwrap()["state"]
                    != "pending"
        })
        .await;
    assert_eq!(
        session
            .call("vm_operation", json!({"operation_id":delete}))
            .unwrap()["state"],
        "succeeded"
    );

    // A refusal inside the selected dispatch is audited like any other.
    let ubuntu_id = id_of(&ubuntu_vm);
    let refusals =
        |session: &Session| session.count("SELECT count(*) FROM activity WHERE status=4");
    let before = refusals(&session);
    let held = supervisor.test_staging.register(ubuntu_id, ()).unwrap();
    assert_eq!(
        session
            .mutate("vm_snapshot_create", json!({"vm_id":ubuntu_vm}))
            .unwrap_err()
            .0,
        "busy"
    );
    drop(held);
    assert_eq!(refusals(&session), before + 1);

    let start = session
        .mutate("vm_start", json!({"vm_id":windows_vm}))
        .unwrap();
    session.succeeded(start.clone()).await;
    let status = session.call("vm_get", json!({"vm_id":windows_vm})).unwrap();
    assert_eq!(status["ready"], true);
    assert_eq!(
        status["native_boot_witness"]["operation_id"],
        start["operation_id"]
    );
    assert_eq!(status["native_boot_witness"]["vm_id"], windows_vm);
    let script = session
        .mutate(
            "vm_run_script",
            json!({"vm_id":windows_vm,"script":"exit 0"}),
        )
        .unwrap();
    let script = session.succeeded(script).await;
    assert_eq!(script["script_status"], "exited");
    assert_eq!(script["exit_code"], 0);

    set_busy(&supervisor, true);
    assert!(
        session
            .mutate("vm_snapshot_create", json!({"vm_id":ubuntu_vm}))
            .is_err()
    );
    let stop = session
        .mutate("vm_stop", json!({"vm_id":windows_vm}))
        .unwrap();
    session.succeeded(stop).await;
    let stopped = session.call("vm_get", json!({"vm_id":windows_vm})).unwrap();
    assert_eq!(stopped["ready"], false);
    assert!(stopped["native_boot_witness"].is_null(), "{stopped}");
    set_busy(&supervisor, false);

    // Retire the displaced Ubuntu generation before admitting the vault job.
    supervisor.sweep().unwrap();
    let wall = crate::vault::utc_ms().unwrap();
    let version = session
        .call(
            "vault_add_version",
            json!({
                "os":"ubuntu","name":"journey","script":"exit 0",
                "request_id":format!("{wall:016x}{:016x}", 1u64)
            }),
        )
        .unwrap()["version_id"]
        .clone();
    session
        .call(
            "vault_test_version",
            json!({
                "version_id":version,"request_id":format!("{wall:016x}{:016x}", 2u64)
            }),
        )
        .unwrap();
    let record = tokio::time::timeout(Duration::from_secs(18), async {
        loop {
            let row = session
                .call("vault_get", json!({"version_id":version}))
                .unwrap();
            if !row["test"].is_null() {
                break row;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("vault cleanup did not settle");
    assert_eq!(
        record["test"],
        json!({"status":"exited","exit_code":0,"cleanup_confirmed":true})
    );
    assert_eq!(
        session.count("SELECT count(*) FROM vm_instances WHERE principal='arxd'"),
        0
    );
    assert_eq!(
        session.count("SELECT count(*) FROM vm_instances WHERE principal='alice'"),
        2
    );
    // The targeted turn that settled the live vault run's release leaves
    // disposal to a later turn, and only a complete ordinary turn clears the
    // daemon-wide custody flag (the landed reaper contract).
    supervisor.sweep().unwrap();
    assert!(
        !supervisor
            .controller
            .foreground_custody_uncertain()
            .unwrap()
    );

    // Extension of a stopped TEST VM, with an exact replay.
    let extended = session
        .mutate("vm_extend", json!({"vm_id":windows_vm,"ttl_s":3000}))
        .unwrap();
    assert_eq!(
        session
            .replay("vm_extend", json!({"vm_id":windows_vm,"ttl_s":3000}))
            .unwrap(),
        extended
    );
    assert_eq!(
        session.call("vm_get", json!({"vm_id":windows_vm})).unwrap()["expires_at_ms"],
        extended["expires_at_ms"]
    );

    // Extension of a running TEST VM. Every selected effect needs a fresh
    // epoch whose UTC and monotonic clocks agree, so it precedes any jump.
    let windows_id = id_of(&windows_vm);
    let start = session
        .mutate("vm_start", json!({"vm_id":windows_vm}))
        .unwrap();
    session.succeeded(start).await;
    let running = session
        .mutate("vm_extend", json!({"vm_id":windows_vm,"ttl_s":3600}))
        .unwrap();
    let status = session.call("vm_get", json!({"vm_id":windows_vm})).unwrap();
    assert_eq!(status["expires_at_ms"], running["expires_at_ms"]);
    assert!(running["expires_at_ms"].as_i64() > extended["expires_at_ms"].as_i64());
    assert_eq!(status["ready"], true);
    let stop = session
        .mutate("vm_stop", json!({"vm_id":windows_vm}))
        .unwrap();
    session.succeeded(stop).await;
    assert_eq!(session.vm_state(windows_id), 1);

    // Ordinary expiry ends a live run: the first turn withdraws it and
    // waits out its release instead of only waking a retry.
    let start = session
        .mutate("vm_start", json!({"vm_id":ubuntu_vm}))
        .unwrap();
    session.succeeded(start).await;
    assert_eq!(
        session.call("vm_get", json!({"vm_id":ubuntu_vm})).unwrap()["ready"],
        true
    );
    assert_eq!(owner(&supervisor, ubuntu_id), Some(true));
    now.fetch_add(1_801_000, Ordering::SeqCst);
    {
        let supervisor = supervisor.clone();
        tokio::task::spawn_blocking(move || supervisor.sweep())
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        owner(&supervisor, ubuntu_id),
        None,
        "expiry left the live run owner in place"
    );
    session
        .sweep_until(&supervisor, |session| {
            session.count("SELECT count(*) FROM vm_instances") == 1
        })
        .await;
    assert!(
        session
            .mutate("vm_extend", json!({"vm_id":ubuntu_vm,"ttl_s":600}))
            .is_err()
    );
    supervisor.sweep().unwrap();
    assert!(
        !supervisor
            .controller
            .foreground_custody_uncertain()
            .unwrap()
    );

    // Ordinary expiry of the stopped VM disposes everything but the goldens.
    now.fetch_add(3_601_000, Ordering::SeqCst);
    session
        .sweep_until(&supervisor, |session| {
            session.count("SELECT count(*) FROM vm_instances") == 0
        })
        .await;
    assert_eq!(session.count("SELECT count(*) FROM vm_restore_points"), 0);
    assert_eq!(session.count("SELECT count(*) FROM vm_disk_claims"), 0);
    assert_eq!(session.count("SELECT count(*) FROM vm_generations"), 2);
    assert_eq!(
        session.count("SELECT count(*) FROM vm_operations WHERE status=0"),
        0
    );
    assert_eq!(inventory(root), goldens);

    supervisor.close();
    supervisor
        .drain_workers(Duration::from_secs(5))
        .await
        .unwrap();
    supervisor.recover(Duration::from_secs(5)).await.unwrap();
    assert_eq!(inventory(root), goldens);
    advance.abort();
}

/// A foreground whose fixture selection is installed through the
/// production installation route, after its startup sweep.
fn fixture_foreground(root: &Path, now: &Arc<AtomicI64>, origin: Instant) -> Arc<Supervisor> {
    let source = FixtureQualification::open(root, origin);
    let recipes = crate::vm::test_issuer::SelectedSource::recipes(&source);
    let path = |leaf| root.join(leaf).to_str().unwrap().replace('\\', "/");
    let state = root.to_str().unwrap().replace('\\', "/");
    let mut profile = profile(&path("missing.json"), &path("dossier"));
    profile["state"] = json!(state);
    profile["volume"] = json!(&state[..3]);
    let runtime = VmRuntimeProfile::parse(&serde_json::to_vec(&profile).unwrap()).unwrap();
    let mut supervisor = Supervisor::new(
        root,
        VmPolicy::default(),
        ForegroundClock {
            utc: Some(now.clone()),
        },
        Some((recipes.ubuntu.recipe_id, recipes.ubuntu.recipe_sha256)),
        NativeRefusal::QualificationMissing,
        Some(runtime),
    )
    .unwrap();
    for path in [
        root.to_owned(),
        root.join("activity.sqlite3"),
        root.join("activity.lock"),
    ] {
        crate::dashboard::tests::set_owner_private(&path);
    }
    {
        let supervisor = Arc::get_mut(&mut supervisor).unwrap();
        supervisor.custody = Some(Arc::new(crate::config::VmCustody::test_only(root).unwrap()));
        supervisor.install_test_with(|controller, selection| {
            TestIssuer::open_owned_fixture(controller, Source::Fixture(Box::new(source)), selection)
        });
    }
    supervisor.startup_sweep().unwrap();
    supervisor
}

/// The UTC clock follows the monotonic one, as on a real host.
fn tick(now: &Arc<AtomicI64>, origin: Instant) -> tokio::task::JoinHandle<()> {
    let now = now.clone();
    tokio::spawn(async move {
        loop {
            now.fetch_max(UTC + origin.elapsed().as_millis() as i64, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
}

/// I12/T49/T50: closure withdraws the live witness and refuses fresh
/// effects, while the exact owner stop still admits and settles with its
/// receipt instead of entering recovery.
#[tokio::test]
async fn closed_admission_withdraws_witness_and_settles_exact_owner_stop() {
    let dir = crate::service::application::tests::private_temp();
    let root = dir.path();
    let now = Arc::new(AtomicI64::new(UTC));
    let origin = Instant::now();
    let supervisor = fixture_foreground(root, &now, origin);
    let advance = tick(&now, origin);
    let _close = CloseOnDrop(supervisor.clone());
    let mut session = Session::new(&supervisor, now.clone());
    set_busy(&supervisor, false);
    let recipes = installed(&supervisor).unwrap().recipes();
    let build = session
        .mutate(
            "vm_template_build",
            json!({"recipe_id":hex::encode(recipes.windows.recipe_id)}),
        )
        .unwrap();
    let template = session.succeeded(build).await["template_id"].clone();
    let create = session
        .mutate("vm_create", json!({"template_id":template,"ttl_s":2400}))
        .unwrap();
    let windows_vm = session.succeeded(create).await["vm_id"].clone();
    let windows_id = id_of(&windows_vm);
    let start = session
        .mutate("vm_start", json!({"vm_id":windows_vm}))
        .unwrap();
    session.succeeded(start.clone()).await;
    let status = session.call("vm_get", json!({"vm_id":windows_vm})).unwrap();
    assert_eq!(status["ready"], true);
    assert_eq!(
        status["native_boot_witness"]["operation_id"],
        start["operation_id"]
    );

    // The held guest stop keeps the drained owner registered until the
    // stop is admitted.
    let Source::Fixture(fixture) = installed(&supervisor).unwrap().source() else {
        unreachable!();
    };
    fixture.hold_stop.store(true, Ordering::SeqCst);
    supervisor.close();
    tokio::time::timeout(Duration::from_secs(5), async {
        while owner(&supervisor, windows_id) != Some(false) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("closure did not withdraw the live witness");
    assert_eq!(
        session
            .mutate("vm_extend", json!({"vm_id":windows_vm,"ttl_s":3600}))
            .unwrap_err()
            .0,
        "shutdown"
    );
    let stop = session
        .mutate("vm_stop", json!({"vm_id":windows_vm}))
        .unwrap();
    fixture.hold_stop.store(false, Ordering::SeqCst);
    let stop = id_of(&stop["operation_id"]);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = supervisor
                .controller
                .operation_status("alice", stop)
                .unwrap()
                .unwrap();
            if status.state != crate::vm::Status::Pending
                && owner(&supervisor, windows_id).is_none()
            {
                assert_eq!(status.state, crate::vm::Status::Succeeded);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("closed-admission stop did not settle");
    assert_eq!(
        session.vm_state(windows_id),
        1,
        "the receipted stop entered recovery"
    );
    supervisor
        .drain_workers(Duration::from_secs(5))
        .await
        .unwrap();
    supervisor.recover(Duration::from_secs(5)).await.unwrap();
    advance.abort();
}

/// P2-2: a caller-owned Static VM reaching the selected TEST dispatch is
/// refused, audited, as unqualified rather than unknown.
#[tokio::test]
async fn selected_dispatch_refuses_a_static_vm_as_unqualified() {
    let dir = crate::service::application::tests::private_temp();
    let root = dir.path();
    let now = Arc::new(AtomicI64::new(UTC));
    let supervisor = fixture_foreground(root, &now, Instant::now());
    let _close = CloseOnDrop(supervisor.clone());
    let session = Session::new(&supervisor, now.clone());
    set_busy(&supervisor, false);
    supervisor
        .controller
        .native_store()
        .transact(|db| {
            db.execute_batch(
                "INSERT INTO vm_static_instances(id,name,principal,host,template,source_generation,
                    recipe_digest,disk_digest,vars_digest,state,persistent,staging,origin_generation)
                 SELECT unhex('02020202020202020202020202020202'),'web-site','alice',id,zeroblob(16),
                    unhex('20000000000000000000000000000001'),zeroblob(32),zeroblob(32),zeroblob(32),
                    1,4096,0,unhex('20000000000000000000000000000001')
                 FROM vm_state_host WHERE singleton=1;
                 INSERT INTO vm_generations(id,static_id,generation,active)
                 VALUES(unhex('20000000000000000000000000000001'),
                    unhex('02020202020202020202020202020202'),1,1);",
            )?;
            Ok(())
        })
        .unwrap();
    let audited = "SELECT count(*) FROM activity WHERE status=4
        AND vm=unhex('02020202020202020202020202020202')";
    assert_eq!(session.count(audited), 0);
    let selected = supervisor.selected_test.clone().unwrap();
    let refused = {
        let mut gate = supervisor.lock().unwrap();
        selected.dispatch(
            &session.inventory,
            &mut gate,
            Action::Start as i64,
            Mutation {
                key: RequestId::parse(&crate::mcp::vm::tests::key(7)).unwrap(),
                id: [2; 16],
                work: Work::Start,
            },
        )
    };
    assert_eq!(refused.unwrap_err().0, "qualification_missing");
    assert_eq!(session.count(audited), 1);
}

use super::*;
use crate::service::application::{Hooks, ServiceApplication};
use crate::service::{Application, ExitCode};
use std::sync::{Mutex, atomic::AtomicBool};

#[derive(Default)]
struct Gate {
    reached: AtomicBool,
    release: AtomicBool,
}
impl Gate {
    fn hold(&self) {
        self.reached.store(true, Ordering::Release);
        let end = std::time::Instant::now() + Duration::from_secs(15);
        while !self.release.load(Ordering::Acquire) && std::time::Instant::now() < end {
            std::thread::yield_now();
        }
        assert!(self.release.load(Ordering::Acquire), "bounded test gate");
    }
    async fn reached(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !self.reached.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    fn open(&self) {
        self.release.store(true, Ordering::Release);
    }
}
struct TestHooks {
    domain: Arc<StartupDomain>,
    seam: Arc<dyn Fn(&'static str) + Send + Sync>,
}
impl Hooks for TestHooks {
    fn load(&self) -> Result<(Layout, ServiceConfig), ExitCode> {
        unreachable!()
    }
    fn layout(&self, _: &Layout) -> Result<Vec<crate::service::native::Observed>, ExitCode> {
        unreachable!()
    }
    fn store(&self, _: &Layout) -> Result<Activity, ExitCode> {
        unreachable!()
    }
    fn bind(&self, _: &Layout, _: String) -> Result<crate::local::Listener, ExitCode> {
        unreachable!()
    }
    fn preflight(&self, _: &Layout, _: &ServiceConfig) -> Unavailable {
        Unavailable::MechanismUnavailable
    }
    fn executor(&self, _: &Layout, _: &ServiceConfig) -> Arc<Execution> {
        self.domain.domain.handle()
    }
    fn start_discovery(&self) -> Option<StartupSetup> {
        self.domain.setup.lock().unwrap().take()
    }
    fn discovery_barrier(&self, point: &'static str) {
        (self.seam)(point);
    }
}
fn probe() -> exec::RunResult {
    let mut r = result();
    r.stdout = exec::CapturedOutput {
        head: table(),
        tail: vec![],
        dropped_bytes: 0,
    };
    r
}
fn rows(dir: &tempfile::TempDir) -> Vec<(String, String, String, Option<String>)> {
    let db = rusqlite::Connection::open(dir.path().join("activity.sqlite3")).unwrap();
    db.prepare(
        "SELECT actor_kind,principal,outcome,reason FROM activity WHERE tool='discover_tools'",
    )
    .unwrap()
    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
    .unwrap()
    .map(Result::unwrap)
    .collect()
}
type App = ServiceApplication<TestHooks>;
fn app(
    dir: &tempfile::TempDir,
    failure: bool,
    waiting: bool,
    seam: Arc<dyn Fn(&'static str) + Send + Sync>,
    step: impl Fn(
        &exec::RunSpec,
        &Cancellation,
        &Cancellation,
    ) -> Result<exec::RunResult, exec::RunError>
    + Send
    + Sync
    + 'static,
) -> (
    App,
    Arc<Activity>,
    Arc<Execution>,
    Arc<AtomicUsize>,
    Readiness,
) {
    let allocations = Arc::new(AtomicUsize::new(0));
    let count = allocations.clone();
    let setup_seam = seam.clone();
    let mut value = fixture();
    value["limits"]["memory_bytes"] = json!(16777216);
    value["limits"]["max_concurrent_runs"] = json!(2);
    let config = parse(value, &FakeResolver::default()).unwrap();
    let location = locate(b"0::/arxd.service/supervisor\n").unwrap();
    let expected_parent = location.runs();
    let setup_components = location.unit_components().to_vec();
    let d = Arc::new(StartupDomain::new(
        &Layout::for_test(Platform::Linux),
        &config,
        location,
        move |owner| {
            assert_eq!(owner.cgroup_parent, Some(expected_parent));
            count.fetch_add(1, Ordering::AcqRel);
            Ok(Box::new(FakePort(Box::new(step))))
        },
        move |location, stop| {
            assert_eq!(location.unit_components(), setup_components);
            setup_seam("setup");
            let outcome = crate::exec::hierarchy::fake::startup_setup(
                location,
                stop,
                failure,
                waiting,
                setup_seam.clone(),
            );
            setup_seam("s10");
            outcome
        },
    ));
    let state = d.readiness.clone();
    let e = d.domain.handle();
    assert!(Arc::ptr_eq(&e, &d.domain.handle()));
    let mut a = Activity::open_service_fixture(dir.path()).unwrap();
    a.dedicated = Some(e.clone());
    let a = Arc::new(a);
    (
        ServiceApplication::discovery_fixture(TestHooks { domain: d, seam }, a.clone()),
        a,
        e,
        allocations,
        state,
    )
}
async fn done(app: &mut App) {
    // A held post-dispatch barrier gives deterministic ownership to drain.
    tokio::task::yield_now().await;
    app.drain().await.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w1_w4_t20b_order_single_domain() {
    let dir = private_temp();
    let log = Arc::new(Mutex::new(Vec::new()));
    let events = log.clone();
    let gate = Arc::new(Gate::default());
    let g = gate.clone();
    let seam = Arc::new(move |p| {
        events.lock().unwrap().push(p);
        if p == "dispatch" {
            g.hold();
        }
    });
    let events = log.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let (mut app, a, e, allocations, _) = app(&dir, false, false, seam, move |_, _, _| {
        events.lock().unwrap().push("port");
        if count.fetch_add(1, Ordering::AcqRel) == 0 {
            Ok(probe())
        } else {
            Err(exec::RunError::NotStarted(exec::NotStarted::Containment))
        }
    });
    app.start_discovery();
    app.start_discovery();
    gate.reached().await;
    assert_eq!(
        *log.lock().unwrap(),
        ["setup", "s9", "s10", "ready", "row", "port", "dispatch"]
    );
    assert_eq!(
        rows(&dir),
        [("daemon".into(), "arxd".into(), "succeeded".into(), None)]
    );
    assert!(list(&a).activities.is_empty());
    assert!(a.tool_cache().is_some());
    assert_eq!(allocations.load(Ordering::Acquire), 1);
    let clone = e.clone();
    drop(e);
    assert!(Arc::ptr_eq(a.dedicated.as_ref().unwrap(), &clone));
    for handle in [clone.clone(), a.dedicated.as_ref().unwrap().clone()] {
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        let out = handle.dispatch(Some(request()), a.clone(), id).await;
        assert!(
            out.err()
                .is_some_and(|f| f.execution_reason == Some(Unavailable::MechanismUnavailable))
        );
    }
    assert_eq!(
        calls.load(Ordering::Acquire),
        2,
        "shared latch across startup and handles"
    );
    gate.open();
    done(&mut app).await;
    assert_eq!(
        &log.lock().unwrap()[8..],
        ["drain_execution", "drain_sequence", "drain_blocking"]
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w2_w3_shutdown_boundaries() {
    for point in [
        "failure",
        "wait",
        "s9",
        "s10",
        "ready",
        "row",
        "admission",
        "port",
    ] {
        let dir = private_temp();
        let gate = Arc::new(Gate::default());
        let g = gate.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (mut app, a, e, _, state) = app(
            &dir,
            point == "failure",
            point == "wait",
            Arc::new(move |p| {
                if p == point || (point == "failure" && p == "setup") {
                    g.hold();
                }
            }),
            {
                let g = gate.clone();
                move |_, _, stop| {
                    count.fetch_add(1, Ordering::AcqRel);
                    if point == "port" {
                        g.hold();
                        assert!(stop.is_cancelled());
                        let mut r = probe();
                        r.outcome = exec::Outcome::Shutdown;
                        r.exit_code = None;
                        return Ok(r);
                    }
                    Ok(probe())
                }
            },
        );
        if point == "admission" {
            let g = gate.clone();
            e.admission_barrier_for_test(Arc::new(move || g.hold()));
        }
        app.start_discovery();
        gate.reached().await;
        if point != "failure" {
            app.stop_accepting().unwrap();
        }
        gate.open();
        if point == "failure" {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !app.discovery_finished_for_test() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(rows(&dir).is_empty(), "failure never writes a daemon row");
        }
        done(&mut app).await;
        if matches!(point, "wait" | "s9" | "s10") {
            assert_eq!(
                state.check(),
                Err(Unavailable::Stopping),
                "Stopped, never Ready"
            );
        } else if point == "ready" {
            assert_eq!(
                state.check(),
                Ok(()),
                "already Ready; shutdown is independent"
            );
        }
        let r = rows(&dir);
        if matches!(point, "row" | "admission" | "port") {
            assert_eq!(r.len(), 1);
            assert_eq!(r[0].2, if point == "port" { "failed" } else { "refused" });
            if point != "port" {
                assert_eq!(r[0].3.as_deref(), Some("shutdown"));
            }
        } else {
            assert!(r.is_empty());
        }
        assert_eq!(calls.load(Ordering::Acquire), usize::from(point == "port"));
        assert!(a.tool_cache().is_none());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w9_stuck_drain_is_failure_not_interruption() {
    let dir = private_temp();
    let gate = Arc::new(Gate::default());
    let g = gate.clone();
    let (mut app, a, _, _, _) = app(&dir, false, false, Arc::new(|_| {}), move |_, _, _| {
        g.hold();
        Ok(probe())
    });
    app.start_discovery();
    gate.reached().await;
    app.stop_accepting().unwrap();
    // The production ten-second bound is covered with paused time in
    // service::tests::stuck_drain_is_bounded_and_never_claims_stop_audit.
    assert!(
        tokio::time::timeout(Duration::from_millis(100), app.drain())
            .await
            .is_err(),
        "stuck sequence remains owned"
    );
    assert_eq!(rows(&dir)[0].2, "started");
    assert!(!gate.release.load(Ordering::Acquire));
    gate.open();
    // The timed out blocking call remains owned by the sequence, not interrupted.
    done(&mut app).await;
    assert_eq!(rows(&dir)[0].2, "succeeded");
    assert!(a.tool_cache().is_some());
}
#[tokio::test]
async fn w5_only_success_response_and_strict_tickets() {
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let initial = a.discovery_observation().unwrap();
    let seed = json!({"outcome":"succeeded", "tools":["older"], "observed_identity":{"id":null}});
    a.cache_discovery(initial, &seed);
    for case in ["K", "M", "G", "refused"] {
        let e = execution(bridge(move |_, _, _| match case {
            "G" => Err(exec::RunError::AfterSpawn),
            "refused" => Err(exec::RunError::NotStarted(exec::NotStarted::Unsupported)),
            "K" => Ok(result()),
            _ => {
                let mut r = probe();
                r.exit_code = Some(2);
                Ok(r)
            }
        }));
        let observation = a.discovery_observation().unwrap();
        let id = a.start_dedicated("alpha", "discover_tools").unwrap();
        if let Ok(response) = e.dispatch(None, a.clone(), id).await {
            a.cache_discovery(observation, &response);
        }
        assert_eq!(a.tool_cache().unwrap().tools, seed["tools"]);
        e.drain().await.unwrap();
    }
    let early = a.discovery_observation().unwrap();
    let late = a.discovery_observation().unwrap();
    assert!(late.0 > early.0);
    let newer = json!({"outcome":"succeeded", "tools":["new"],"observed_identity":{"id":"response-metadata-marker"}});
    a.cache_discovery(late, &newer);
    a.cache_discovery(early, &seed);
    a.cache_discovery(late, &seed);
    let c = a.tool_cache().unwrap();
    assert_eq!(c.ticket, late.0);
    assert_eq!(c.tools, newer["tools"]);
    assert_eq!(c.observed_identity, newer["observed_identity"]);
    assert_eq!(c.observed_at_ms, late.1);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn w5_overlapping_discoveries_keep_later_start() {
    let dir = private_temp();
    let gate = Arc::new(Gate::default());
    let g = gate.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let seq = count.clone();
    let b = bridge(move |_, _, _| {
        let n = seq.fetch_add(1, Ordering::AcqRel);
        if n == 0 {
            g.hold();
        }
        let mut r = probe();
        if n == 1 {
            r.stdout.head = String::from_utf8(table())
                .unwrap()
                .replace("synthetic 1", "synthetic 2")
                .into_bytes();
        }
        Ok(r)
    });
    let mut v = fixture();
    v["limits"]["memory_bytes"] = json!(16777216);
    v["limits"]["max_concurrent_runs"] = json!(2);
    let c = parse(v, &FakeResolver::default()).unwrap();
    let e = Execution::unix_bridge_for_test(Arc::new(b), &c);
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let early = a.discovery_observation().unwrap();
    let id = a.start_dedicated("alpha", "discover_tools").unwrap();
    let first = e.dispatch(None, a.clone(), id);
    gate.reached().await;
    let late = a.discovery_observation().unwrap();
    let id = a.start_dedicated("alpha", "discover_tools").unwrap();
    let second = e.dispatch(None, a.clone(), id).await.ok().unwrap();
    a.cache_discovery(late, &second);
    gate.open();
    let first = first.await.ok().unwrap();
    a.cache_discovery(early, &first);
    assert_ne!(first["tools"], second["tools"]);
    assert_eq!(a.tool_cache().unwrap().tools, second["tools"]);
    e.drain().await.unwrap();
}
#[tokio::test]
async fn w6_w7_mcp_cache_and_no_cache_golden() {
    use rmcp::{ServiceExt, model::CallToolRequestParams};
    let dir = private_temp();
    let e = execution(bridge(|_, _, _| Ok(probe())));
    let mut activity = Activity::open_service_fixture(dir.path()).unwrap();
    activity.dedicated = Some(e.clone());
    let a = Arc::new(activity);
    let inventory = crate::mcp::Inventory {
        activity: a.clone(),
        execution: None,
        principal: Some(crate::mcp::Principal("alpha".into())),
    };
    let (s, c) = tokio::io::duplex(65536);
    let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
    let client = ().serve(c).await.unwrap();
    let before = client
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap();
    let before: Value = serde_json::from_str(&before.content[0].as_text().unwrap().text).unwrap();
    let h = &before["hosts"][0];
    assert_eq!(h["tools"], json!([]));
    assert!(h["tools_observed_at"].is_null());
    assert!(h["last_observed_identity"].is_null());
    let response = client
        .call_tool(CallToolRequestParams::new("discover_tools"))
        .await
        .unwrap();
    let response: Value =
        serde_json::from_str(&response.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(response["outcome"], "succeeded");
    let after = client
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap();
    let after: Value = serde_json::from_str(&after.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(after["hosts"][0]["tools"], response["tools"]);
    assert_eq!(
        after["hosts"][0]["last_observed_identity"],
        response["observed_identity"]
    );
    assert_eq!(
        after["hosts"][0]["tools_observed_at"],
        a.tool_cache().unwrap().observed_at_ms
    );
    let text = after.to_string();
    assert!(!text.contains("no_new_privs") && !text.contains("sudo") && !text.contains(MARKER));
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
    e.drain().await.unwrap();
    // Pin the complete no-cache rendering block to the accepted golden source.
    let golden = include_str!("e7-no-cache-golden.txt");
    let now = include_str!("../../../mcp.rs");
    let block = |s: &str| {
        s.split("        let mut value = json!({\"hosts\"")
            .nth(1)
            .unwrap()
            .split("        self.activity\n            .finish(id, \"succeeded\")")
            .next()
            .unwrap()
            .to_owned()
    };
    let new = block(now);
    let cache = new
        .find("        if let Some(cache) = self.activity.tool_cache()")
        .unwrap();
    assert_eq!(&new[..cache], golden);
}
#[test]
fn w8_production_and_domain_source_pins() {
    let application = include_str!("../../application.rs");
    let native = application
        .split("impl Hooks for Native {")
        .nth(1)
        .unwrap()
        .split("fn store_category")
        .next()
        .unwrap();
    assert!(!native.contains("start_discovery"));
    let start = application
        .split("    fn start_discovery(&mut self) {")
        .nth(1)
        .unwrap()
        .split("    async fn serve")
        .next()
        .unwrap();
    assert!(start.starts_with("\n        // No build-account backend: never probe the privileged service's PATH.\n        #[cfg(test)]\n        {"));
    let block_start = start.find("#[cfg(test)]").unwrap();
    let open = start[block_start..].find('{').unwrap() + block_start;
    let mut depth = 0;
    let mut end = open;
    for (offset, ch) in start[open..].char_indices() {
        if ch == '{' {
            depth += 1;
        }
        if ch == '}' {
            depth -= 1;
        }
        if depth == 0 {
            end = open + offset + 1;
            break;
        }
    }
    assert_eq!(start[end..].trim(), "}", "production start remains empty");
    assert_eq!(
        include_str!("e7.rs")
            .matches(concat!("StartupDomain", "::new("))
            .count(),
        3
    );
    let e = include_str!("../../executor.rs");
    let configured = e
        .split("pub(crate) fn configured")
        .nth(1)
        .unwrap()
        .split("    pub")
        .next()
        .unwrap();
    assert!(configured.contains("Self::native("));
    assert!(!configured.contains("UnixBridge"));
    assert!(e.contains("\"available\":false"));
    assert!(e.contains("\"service_hardening\":null"));
    let wiring = include_str!("../../unix_bridge.rs")
        .split("struct StartupDomain")
        .nth(1)
        .unwrap();
    assert_eq!(wiring.matches("AdmissionDomain::new(").count(), 1);
    assert_eq!(wiring.matches("Readiness::pending()").count(), 1);
    assert!(!wiring.contains("Runner::new("));
    let handle = include_str!("../../unix_bridge.rs")
        .split("fn handle(&self)")
        .nth(1)
        .unwrap()
        .split("// Inactive owned")
        .next()
        .unwrap();
    assert!(!handle.contains("::new("));
    assert!(handle.contains("self.execution.clone()"));
}
#[tokio::test]
async fn w2_nonstandard_and_hardening_fail_before_ready() {
    let mut v = fixture();
    v["rights"] = json!("admin");
    v["limits"]["memory_bytes"] = json!(16777216);
    let c = parse(
        v,
        &FakeResolver {
            admin: true,
            ..Default::default()
        },
    )
    .unwrap();
    for platform in [Platform::Linux, Platform::Macos, Platform::Windows] {
        let c = if platform == Platform::Linux {
            c.clone()
        } else {
            config()
        };
        let domain = StartupDomain::new(
            &Layout::for_test(platform),
            &c,
            locate(b"0::/arxd.service/supervisor\n").unwrap(),
            |_| panic!("nonstandard factory must not run"),
            |_, _| panic!("nonstandard setup must not run"),
        );
        let setup = domain.setup.lock().unwrap().take().unwrap();
        assert!(!setup(Cancellation::default()));
        assert!(domain.setup.lock().unwrap().is_none());
    }
    let outcome = crate::exec::hierarchy::fake::startup_setup(
        &locate(b"0::/arxd.service/supervisor\n").unwrap(),
        &Cancellation::default(),
        true,
        false,
        Arc::new(|_| {}),
    );
    assert!(matches!(
        outcome,
        exec::hierarchy::setup::SetupOutcome::Unavailable(
            exec::hierarchy::setup::SetupFailure::Hardening
        )
    ));
    // Exercise the wiring's actual transition as well as the typed setup result.
    let domain = StartupDomain::new(
        &Layout::for_test(Platform::Linux),
        &config(),
        locate(b"0::/arxd.service/supervisor\n").unwrap(),
        |_| {
            Ok(Box::new(FakePort(Box::new(|_, _, _| {
                panic!("hardening port must not run")
            }))))
        },
        move |_, _| outcome,
    );
    assert!(!domain.setup.lock().unwrap().take().unwrap()(
        Cancellation::default()
    ));
    assert!(domain.domain.handle().capability()["available"] == false);
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let id = a.start_dedicated("alpha", "discover_tools").unwrap();
    let e = domain.domain.handle();
    let out = e.dispatch(None, a, id).await;
    assert!(
        out.err()
            .is_some_and(|e| e.execution_reason == Some(Unavailable::MechanismUnavailable))
    );
    e.drain().await.unwrap();
}

// Exercise allocation in the actual callers, never placing Activity tickets.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pa1_actual_callers_reverse_finish() {
    use rmcp::{ServiceExt, model::CallToolRequestParams};
    for startup_first in [true, false] {
        let dir = private_temp();
        let gate = Arc::new(Gate::default());
        let g = gate.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let held_call = usize::from(!startup_first);
        let (mut app, a, e, _, _) = app(&dir, false, false, Arc::new(|_| {}), move |_, _, _| {
            let n = count.fetch_add(1, Ordering::AcqRel);
            if n == held_call {
                g.hold();
            }
            let mut r = probe();
            r.stdout.head = String::from_utf8(table())
                .unwrap()
                .replace("synthetic 1", &format!("synthetic {}", n + 10))
                .into_bytes();
            Ok(r)
        });
        app.start_discovery();
        if !startup_first {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !app.discovery_finished_for_test() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(a.tool_cache().is_some(), "existing cache will be replaced");
        }
        let inventory = crate::mcp::Inventory {
            activity: a.clone(),
            execution: None,
            principal: Some(crate::mcp::Principal("alpha".into())),
        };
        let (s, c) = tokio::io::duplex(65536);
        let server = tokio::spawn(async move { inventory.serve(s).await.unwrap().waiting().await });
        let client = ().serve(c).await.unwrap();
        let first = if startup_first {
            None
        } else {
            let peer = client.peer().clone();
            Some(tokio::spawn(async move {
                peer.call_tool(CallToolRequestParams::new("discover_tools"))
                    .await
                    .unwrap()
            }))
        };
        gate.reached().await;
        let newer = client
            .call_tool(CallToolRequestParams::new("discover_tools"))
            .await
            .unwrap();
        let newer: Value = serde_json::from_str(&newer.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(newer["outcome"], "succeeded");
        let ticket = a.tool_cache().unwrap().ticket;
        gate.open();
        if let Some(first) = first {
            first.await.unwrap();
        }
        done(&mut app).await;
        let hosts = client
            .call_tool(CallToolRequestParams::new("list_hosts"))
            .await
            .unwrap();
        let hosts: Value = serde_json::from_str(&hosts.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(
            a.tool_cache().unwrap().ticket,
            ticket,
            "older completion cannot replace newer start"
        );
        assert_eq!(hosts["hosts"][0]["tools"], newer["tools"]);
        assert_eq!(
            calls.load(Ordering::Acquire),
            if startup_first { 2 } else { 3 }
        );
        client.cancel().await.unwrap();
        server.await.unwrap().unwrap();
        e.drain().await.unwrap();
    }
}

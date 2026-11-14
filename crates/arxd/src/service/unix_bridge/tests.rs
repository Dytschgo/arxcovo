use super::*;
use crate::service::{
    application::tests::private_temp,
    config::tests::{FakeResolver, fixture, parse},
    executor::Execution,
};
use crate::store::Activity;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
const MARKER: &str = "ARX-D-PRIVATE-MARKER";
type Step = dyn Fn(&exec::RunSpec, &Cancellation, &Cancellation) -> Result<exec::RunResult, exec::RunError>
    + Send
    + Sync;
struct FakePort(Box<Step>);
impl RunnerPort for FakePort {
    fn cleanup_residue(&self) -> bool {
        false
    }
    fn run(
        &self,
        s: &exec::RunSpec,
        c: &Cancellation,
        h: &Cancellation,
    ) -> Result<exec::RunResult, exec::RunError> {
        (self.0)(s, c, h)
    }
}
fn ready() -> Readiness {
    let r = Readiness::pending();
    r.set_ready();
    r
}
fn config() -> ServiceConfig {
    let mut v = fixture();
    v["limits"]["memory_bytes"] = json!(16777216);
    v["workspace"] = json!("/synthetic-nonexistent-workspace");
    parse(v, &FakeResolver::default()).unwrap()
}
fn bridge(
    step: impl Fn(
        &exec::RunSpec,
        &Cancellation,
        &Cancellation,
    ) -> Result<exec::RunResult, exec::RunError>
    + Send
    + Sync
    + 'static,
) -> UnixBridge {
    UnixBridge::new(
        &Layout::for_test(Platform::Linux),
        &config(),
        Some("/synthetic/unit/runs".into()),
        |_| Ok(Box::new(FakePort(Box::new(step)))),
        Box::new(|| Ok(())),
        ready(),
    )
}
fn execution(b: UnixBridge) -> Arc<Execution> {
    Execution::unix_bridge_for_test(Arc::new(b), &config())
}

fn request() -> Request {
    serde_json::from_value(
        json!({"tool":"cargo","args":["-x","--","","世界",MARKER],"cwd":"synthetic-cwd"}),
    )
    .unwrap()
}
fn result() -> exec::RunResult {
    exec::RunResult {
        outcome: exec::Outcome::Exited,
        exit_code: Some(0),
        identity: exec::ObservedIdentity {
            uid: 1002,
            gid: 1002,
            daemon_was_root: true,
        },
        tree_boundary: exec::TreeBoundary::LinuxCgroupV2,
        tree_cleanup_confirmed: true,
        stdout: exec::CapturedOutput {
            head: MARKER.as_bytes().to_vec(),
            tail: vec![255],
            dropped_bytes: 7,
        },
        stderr: exec::CapturedOutput {
            head: vec![],
            tail: vec![],
            dropped_bytes: 0,
        },
    }
}
async fn dispatch(b: UnixBridge, request: Option<Request>) -> (Value, Value) {
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let e = execution(b);
    let id = a
        .start_dedicated(
            "alpha",
            if request.is_some() {
                "run_command"
            } else {
                "discover_tools"
            },
        )
        .unwrap();
    let value = e.dispatch(request, a.clone(), id).await.ok().unwrap();
    let listed = serde_json::to_value(list(&a)).unwrap();
    assert!(!listed.to_string().contains(MARKER));
    assert!(a.finish(id, "failed").is_err());
    e.drain().await.unwrap();
    drop(a);
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let p = entry.unwrap().path();
        if p.is_file() {
            assert!(
                !std::fs::read(p)
                    .unwrap()
                    .windows(MARKER.len())
                    .any(|b| b == MARKER.as_bytes())
            );
        }
    }
    (value, listed)
}
#[test]
fn t1_derivation_factory_once_and_refusals() {
    let c = config();
    let l = Layout::for_test(Platform::Linux);
    let d = derive(&l, &c, "/fake/runs".into()).ok().unwrap();
    assert_eq!(d.account, "builder");
    assert_eq!((d.uid, d.gid), (1002, 1002));
    assert_eq!(d.workspace, PathBuf::from(c.workspace()));
    assert_eq!(
        d.protected_directories,
        vec![
            PathBuf::from("/etc/arxcovo"),
            PathBuf::from("/var/lib/arxcovo")
        ]
    );
    assert_eq!(d.environment, *c.environment());
    assert_eq!(d.cgroup_parent, Some("/fake/runs".into()));
    assert!(!d.admin);
    assert_eq!((d.memory_bytes, d.pids), (16777216, 32));
    for platform in [Platform::Macos, Platform::Windows] {
        assert!(matches!(
            derive(&Layout::for_test(platform), &c, "/x".into()),
            Err(Unavailable::MechanismUnavailable)
        ));
    }
    for (key, value) in [
        ("name", json!("bad name")),
        ("name", json!("a".repeat(129))),
        ("gid", json!(0)),
        ("gid", Value::Null),
    ] {
        let mut v = fixture();
        v["limits"]["memory_bytes"] = json!(16777216);
        v["build_account"][key] = value;
        let c = ServiceConfig::parse_inventory(
            &serde_json::to_vec(&v).unwrap(),
            &l,
            &FakeResolver::default(),
        )
        .unwrap();
        assert!(matches!(
            derive(&l, &c, "/x".into()),
            Err(Unavailable::PreflightFailed)
        ));
    }
    for (field, value) in [
        ("memory_bytes", json!(1024)),
        ("memory_bytes", json!(68719476737u64)),
        ("pids", json!(1025)),
    ] {
        let mut v = fixture();
        v["limits"]["memory_bytes"] = json!(16777216);
        v["limits"][field] = value;
        assert!(matches!(
            derive(
                &l,
                &parse(v, &FakeResolver::default()).unwrap(),
                "/x".into()
            ),
            Err(Unavailable::PreflightFailed)
        ));
    }
    let calls = AtomicUsize::new(0);
    let b = UnixBridge::new(
        &l,
        &c,
        Some("/x".into()),
        |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakePort(Box::new(|_, _, _| Ok(result())))))
        },
        Box::new(|| Ok(())),
        ready(),
    );
    for _ in 0..2 {
        b.preflight().unwrap();
        let _ = b.run(
            Some(&request()),
            &Cancellation::default(),
            &Cancellation::default(),
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut b = b;
    b.readiness = Readiness::pending();
    assert_eq!(b.preflight(), Err(Unavailable::MechanismUnavailable));
}
#[test]
fn t2_bounded_supervisor_parser() {
    assert_eq!(
        locate(b"0::/system.slice/arxd.service/supervisor\n").map(|l| l.runs()),
        Some("/sys/fs/cgroup/system.slice/arxd.service/runs".into())
    );
    for bytes in [
        b"0::/supervisor".as_slice(),
        b"0::/a/supervisor",
        b"0::/system.slice/foo.service/supervisor",
        b"0::/arxd.service/x/supervisor",
        b"0::/arxd.service.d/supervisor",
        b"0::/unit",
        b"0::/unit/not-supervisor",
        b"1::/unit/supervisor",
        b"0::/a/../supervisor",
        b"0::/a/./supervisor",
        b"0::/a//supervisor",
        b"0::/a/supervisor\n0::/b/supervisor",
        b"0::/a\0/supervisor",
        b"0::/a/supervisor\n\n",
        &[b'x'; 4097],
    ] {
        assert!(locate(bytes).is_none());
    }
}
#[test]
fn t3_exact_specs() {
    let binary = Layout::for_test(Platform::Linux).binary().to_path_buf();
    let r = request();
    let s = spec(&binary, Some(&r));
    assert_eq!(s.executable, binary);
    assert!(s.cwd.as_os_str().is_empty());
    assert_eq!(s.timeout, Duration::from_secs(60));
    assert_eq!(s.output_bytes, 4096);
    let mut expected = vec![
        "exec-child".to_string(),
        "tool".into(),
        "cargo".into(),
        "synthetic-cwd".into(),
    ];
    expected.extend(r.args.clone());
    assert_eq!(s.arguments, expected);
    let mut r = r;
    r.tool = None;
    r.executable = Some("/absolute/target".into());
    assert_eq!(
        spec(&binary, Some(&r)).arguments[1..3],
        ["path", "/absolute/target"]
    );
    let s = spec(&binary, None);
    assert_eq!(s.arguments, ["tool-probe"]);
    assert!(s.cwd.as_os_str().is_empty());
    assert_eq!((s.timeout, s.output_bytes), (Duration::from_secs(60), 4096));
}
#[tokio::test]
async fn t4_phase_matrix_and_panic_finish() {
    use exec::{NotStarted as N, RunError as E};
    for (error, reason, quarantine) in [
        (E::NotStarted(N::Invalid), "execution_failed", false),
        (E::NotStarted(N::Resource), "execution_failed", false),
        (E::NotStarted(N::Unsupported), "platform_unsupported", false),
        (
            E::NotStarted(N::ServiceIdentity),
            "mechanism_unavailable",
            false,
        ),
        (
            E::NotStarted(N::Containment),
            "mechanism_unavailable",
            false,
        ),
        (
            E::NotStarted(N::AccountChanged),
            "account_identity_changed",
            false,
        ),
        (E::NotStarted(N::AccountPolicy), "preflight_failed", false),
        (E::NotStarted(N::PrivateRoots), "preflight_failed", false),
        (E::NotStarted(N::Shutdown), "stopping", false),
        (E::NotStarted(N::Cancelled), "", false),
        (E::SpawnFailed, "cleanup_unconfirmed", true),
        (E::AfterSpawn, "cleanup_unconfirmed", true),
    ] {
        let e = execution(bridge(move |_, _, _| Err(error)));
        let dir = private_temp();
        let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        let outcome = e.dispatch(Some(request()), a.clone(), id).await;
        let v = match outcome {
            Ok(v) => v,
            Err(f) => json!({"reason":f.execution_reason.unwrap().as_str()}),
        };
        if reason.is_empty() {
            assert!(v["reason"].is_null());
        } else {
            assert_eq!(v["reason"], reason, "phase {error:?}");
        }
        assert_eq!(e.capability()["reason"] == "quarantined", quarantine);
        assert!(v.get("stdout").is_none());
        assert!(v.get("stderr").is_none());
        assert!(!serde_json::to_string(&list(&a)).unwrap().contains(MARKER));
        assert!(a.finish(id, "failed").is_err());
        e.drain().await.unwrap();
    }
    for seam in 0..3 {
        let mut b = bridge(move |_, _, _| {
            if seam == 0 {
                panic!("ARX-T10-FIXED-PANIC");
            }
            Ok(result())
        });
        b.seam = seam;
        let (v, _) = dispatch(b, if seam == 2 { None } else { Some(request()) }).await;
        assert_eq!(v["reason"], "cleanup_unconfirmed");
    }
}
#[tokio::test]
async fn t5_result_mapping_and_output_release() {
    for case in 0..8 {
        let (v, _) = dispatch(
            bridge(move |s, _, _| {
                assert!(s.cwd.as_os_str().is_empty());
                let mut r = result();
                match case {
                    1 => r.exit_code = Some(7),
                    2 => {
                        r.outcome = exec::Outcome::TimedOut;
                        r.exit_code = Some(0)
                    }
                    3 => r.tree_cleanup_confirmed = false,
                    4 => r.exit_code = None,
                    5 => {
                        r.identity.uid = 3141592653;
                        r.identity.gid = 3141592653
                    }
                    6 => r.tree_boundary = exec::TreeBoundary::MacProcessGroupBestEffort,
                    7 => r.outcome = exec::Outcome::Shutdown,
                    _ => {}
                };
                Ok(r)
            }),
            Some(request()),
        )
        .await;
        let reason = match case {
            0 => Value::Null,
            1 => Value::Null,
            2 => Value::Null,
            3 | 6 => json!("cleanup_unconfirmed"),
            4 => json!("execution_failed"),
            5 => json!("identity_mismatch"),
            7 => Value::Null,
            _ => unreachable!(),
        };
        assert_eq!(v["reason"], reason, "case {case}");
        if case == 5 {
            assert_eq!(
                v["stdout"],
                json!({"head":"","tail":"","dropped_bytes":null})
            );
            assert!(!v.to_string().contains("3141592653"));
        } else {
            assert_eq!(v["stdout"]["head"], MARKER);
            assert_eq!(v["stdout"]["dropped_bytes"], 7);
        }
        if case == 2 || case == 7 {
            assert!(v["metadata"]["exit_code"].is_null());
        }
    }
}
fn table() -> Vec<u8> {
    TOOLS
        .iter()
        .map(|n| json!({"version":"synthetic 1","found":true,"name":n}).to_string() + "\n")
        .collect::<String>()
        .add_rights()
}
#[tokio::test]
async fn t6_discovery_exact_table() {
    for case in 0..19 {
        let (v, _) = dispatch(
            bridge(move |_, _, _| {
                let mut r = result();
                let mut bytes = table();
                match case {
                    17 => {
                        bytes = String::from_utf8(bytes)
                            .unwrap()
                            .lines()
                            .take(TOOLS.len())
                            .map(|l| l.to_owned() + "\n")
                            .collect::<String>()
                            .into_bytes();
                    }
                    18 => bytes.extend_from_slice(
                        b"{\"rights\":{\"no_new_privs\":true,\"sudo\":\"absent\"}}\n",
                    ),
                    1 => r.stdout.dropped_bytes = 1,
                    2 => bytes.reverse(),
                    3 => bytes.extend_from_slice(b"{}\n"),
                    4 => bytes = vec![],
                    5 => {
                        bytes = String::from_utf8(bytes)
                            .unwrap()
                            .replace("synthetic 1", "/private/path")
                            .into_bytes()
                    }
                    6 => r.exit_code = Some(2),
                    7 => r.identity.uid = 3141592653,
                    8 => {
                        bytes = String::from_utf8(bytes)
                            .unwrap()
                            .replace(",\"version\":\"synthetic 1\"", "")
                            .into_bytes()
                    }
                    9 => {
                        bytes = String::from_utf8(bytes)
                            .unwrap()
                            .replace("\"synthetic 1\"", "\"\"")
                            .into_bytes()
                    }
                    12 => {
                        let text = String::from_utf8(bytes).unwrap();
                        let mut rows: Vec<_> = text.lines().collect();
                        rows.swap(0, 1);
                        bytes = (rows.join("\n") + "\n").into_bytes();
                    }
                    13 => {
                        let text = String::from_utf8(bytes).unwrap();
                        let rows: Vec<_> = text.lines().skip(1).collect();
                        bytes = (rows.join("\n") + "\n").into_bytes();
                    }
                    14 => {
                        bytes = String::from_utf8(bytes)
                            .unwrap()
                            .replace("{", "{\"extra\":true,")
                            .into_bytes()
                    }
                    15 => {
                        bytes = String::from_utf8(bytes)
                            .unwrap()
                            .replace("\"found\":true", "\"found\":false")
                            .into_bytes()
                    }
                    16 => {
                        bytes = String::from_utf8(bytes)
                            .unwrap()
                            .replace("\"found\":true", "\"found\":true,\"found\":true")
                            .into_bytes()
                    }
                    10 => bytes.push(255),
                    11 => {
                        r.outcome = exec::Outcome::TimedOut;
                        r.exit_code = Some(0)
                    }
                    _ => {}
                };
                r.stdout.head = bytes;
                r.stdout.tail.clear();
                if case != 1 {
                    r.stdout.dropped_bytes = 0;
                }
                Ok(r)
            }),
            None,
        )
        .await;
        if case == 0 {
            assert_eq!(v["tools"].as_array().unwrap().len(), TOOLS.len());
        } else {
            assert!(v["tools"].as_array().is_none_or(|a| a.is_empty()));
            assert_eq!(
                v["reason"],
                if case == 7 {
                    "identity_mismatch"
                } else {
                    "execution_failed"
                }
            );
        }
    }
}
#[tokio::test]
async fn t7_caller_tokens_and_drop_independent() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let e = execution(bridge(move |_, c, h| {
        seen.fetch_add(1, Ordering::SeqCst);
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while !c.is_cancelled() && !h.is_cancelled() && std::time::Instant::now() < until {
            std::thread::yield_now();
        }
        assert!(c.is_cancelled() || h.is_cancelled());
        let mut r = result();
        r.outcome = if h.is_cancelled() {
            exec::Outcome::Shutdown
        } else {
            exec::Outcome::Cancelled
        };
        Ok(r)
    }));
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let id = a.start_dedicated("alpha", "run_command").unwrap();
    let future = e.dispatch(Some(request()), a.clone(), id);
    tokio::time::timeout(Duration::from_secs(3), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    e.drain().await.unwrap();
    assert_eq!(
        future.await.ok().unwrap()["metadata"]["stop_reason"],
        "shutdown"
    );
    let e = execution(bridge(|_, _, _| Ok(result())));
    drop(e.clone());
    assert_ne!(e.capability()["reason"], "stopping");
    let c = Cancellation::default();
    let h = Cancellation::default();
    let b = bridge(|_, c, _| {
        assert!(c.is_cancelled());
        Err(exec::RunError::NotStarted(exec::NotStarted::Cancelled))
    });
    c.cancel();
    assert!(matches!(
        b.run(Some(&request()), &c, &h),
        Report::CancelledBeforeSpawn
    ));
    drop(b);
    assert!(!h.is_cancelled());
}
#[test]
fn t9_production_selection_stays_inert_source() {
    for (platform, id, gid) in [
        (Platform::Linux, "1002", Some(1002)),
        (Platform::Macos, "1002", Some(1002)),
        (Platform::Windows, "S-1-5-21-100-200-300-1002", None),
    ] {
        let native = Execution::inactive_native_for_test(platform, id, gid);
        assert_eq!(
            native.preflight_for_test(),
            Err(Unavailable::MechanismUnavailable)
        );
        assert_eq!(native.capability()["reason"], "mechanism_unavailable");
        let dir = private_temp();
        let activity = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let row = activity.start_dedicated("alpha", "run_command").unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let failure = rt
            .block_on(async { native.dispatch(Some(request()), activity, row).await })
            .err()
            .unwrap();
        assert_eq!(
            failure.execution_reason,
            Some(Unavailable::MechanismUnavailable)
        );
        rt.block_on(native.drain()).unwrap();
    }

    let source = include_str!("../executor.rs");
    let start = source.find("pub(crate) fn configured(").unwrap();
    let end = source[start..].find("pub(crate) fn set_reason").unwrap();
    let configured = &source[start..start + end];
    assert!(configured.contains("Self::native("));
    assert!(!configured.contains("UnixBridge"));
}
#[test]
fn t10_reexec_diagnostics() {
    for backtrace in [false, true] {
        let temp = private_temp();
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.env_clear()
            .env("ARX_STAGE_D_FIXTURE", "1")
            .env("TMP", temp.path())
            .env("TEMP", temp.path())
            .env("TMPDIR", temp.path())
            .args([
                "--exact",
                "service::unix_bridge::tests::t10_fixture",
                "--ignored",
                "--nocapture",
            ]);
        if backtrace {
            cmd.env("RUST_BACKTRACE", "1");
        }
        let output = cmd.output().unwrap();
        assert!(output.status.success());
        let mut combined = output.stderr;
        combined.extend_from_slice(&output.stdout);
        let text = String::from_utf8_lossy(&combined);
        assert!(!text.contains(MARKER));
        assert!(!text.contains("3141592653"), "identity marker withheld");
        for fixed in [
            "ARX-PA6-FIXED-PANIC",
            "ARX-T10-FIXED-PANIC",
            "ARX-T10-MAPPING-PANIC",
            "ARX-T10-DISCOVERY-PANIC",
            "ARX-T10-NEGATIVE-LEAK",
        ] {
            assert_eq!(text.matches(fixed).count(), 1);
        }
    }
}
#[test]
#[ignore = "only re-executed by t10 parent"]
fn t10_fixture() {
    assert_eq!(std::env::var("ARX_STAGE_D_FIXTURE").unwrap(), "1");
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        t8_private_inputs_all_phases_inner().await;
        t4_phase_matrix_and_panic_finish_inner().await;
        e6::pa6_panic_residue_report_and_gate().await;
    });
}
async fn t4_phase_matrix_and_panic_finish_inner() {
    for seam in 0..4 {
        let mut b = bridge(move |_, _, _| match seam {
            0 => panic!("ARX-T10-FIXED-PANIC"),
            3 => panic!("{}", "ARX-T10-NEGATIVE-LEAK"),
            _ => Ok(result()),
        });
        b.seam = if seam == 3 { 0 } else { seam };
        let _ = dispatch(b, if seam == 2 { None } else { Some(request()) }).await;
    }
}

fn list(a: &Activity) -> crate::store::ActivityPage {
    let read = a.start("alpha", "list_activity").unwrap();
    let p = a
        .list(
            "alpha",
            read,
            &crate::store::ListActivity {
                limit: 50,
                before_id: None,
                include_reads: false,
            },
        )
        .unwrap();
    a.finish(read, "succeeded").unwrap();
    p
}

#[tokio::test]
async fn t8_private_inputs_all_phases() {
    t8_private_inputs_all_phases_inner().await;
}
async fn t8_private_inputs_all_phases_inner() {
    use exec::{NotStarted as N, RunError as E};
    for error in [
        E::NotStarted(N::Invalid),
        E::NotStarted(N::Unsupported),
        E::NotStarted(N::Cancelled),
        E::NotStarted(N::Shutdown),
        E::NotStarted(N::ServiceIdentity),
        E::NotStarted(N::AccountChanged),
        E::NotStarted(N::AccountPolicy),
        E::NotStarted(N::PrivateRoots),
        E::NotStarted(N::Containment),
        E::NotStarted(N::Resource),
        E::SpawnFailed,
        E::AfterSpawn,
    ] {
        let mut v = fixture();
        v["limits"]["memory_bytes"] = json!(16777216);
        v["build_account"]["name"] = json!(MARKER);
        v["workspace"] = json!(format!("/synthetic/{MARKER}"));
        v["environment"]["PATH"] = json!(format!("/synthetic/{MARKER}"));
        let layout = Layout::for_test(Platform::Linux);
        let c = ServiceConfig::parse_inventory(
            &serde_json::to_vec(&v).unwrap(),
            &layout,
            &FakeResolver::default(),
        )
        .unwrap();
        let marker_seen = Arc::new(AtomicUsize::new(0));
        let port_seen = marker_seen.clone();
        let b = UnixBridge::new(
            &layout,
            &c,
            Some(PathBuf::from(format!("/synthetic/{MARKER}/runs"))),
            |owner| {
                assert_eq!(owner.account, MARKER);
                assert!(owner.workspace.to_str().unwrap().contains(MARKER));
                assert!(owner.environment["PATH"].contains(MARKER));
                assert!(
                    owner
                        .cgroup_parent
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .contains(MARKER)
                );
                Ok(Box::new(FakePort(Box::new(move |s, _, _| {
                    port_seen.store(
                        usize::from(s.arguments.iter().any(|v| v.contains(MARKER))),
                        Ordering::SeqCst,
                    );
                    Err(error)
                }))))
            },
            Box::new(|| Ok(())),
            ready(),
        );
        let dir = private_temp();
        let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let e = execution(b);
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        let out = e.dispatch(Some(request()), a.clone(), id).await;
        assert_eq!(
            marker_seen.load(Ordering::SeqCst),
            1,
            "port marker observed"
        );
        if let Ok(v) = out {
            assert!(!v.to_string().contains(MARKER));
        } else {
            let f = out.err().unwrap();
            assert!(!f.reason.as_str().contains(MARKER));
        }
        assert!(!serde_json::to_string(&list(&a)).unwrap().contains(MARKER));
        e.drain().await.unwrap();
        drop(e);
        drop(a);
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let p = entry.unwrap().path();
            if p.is_file() {
                assert!(
                    !std::fs::read(p)
                        .unwrap()
                        .windows(MARKER.len())
                        .any(|v| v == MARKER.as_bytes())
                );
            }
        }
    }
    for matched in [false, true] {
        let mut v = fixture();
        v["limits"]["memory_bytes"] = json!(16777216);
        v["build_account"]["identity"] = json!("3141592653");
        v["build_account"]["gid"] = json!(3141592653u32);
        let c = ServiceConfig::parse_inventory(
            &serde_json::to_vec(&v).unwrap(),
            &Layout::for_test(Platform::Linux),
            &FakeResolver::default(),
        )
        .unwrap();
        let b = bridge(move |_, _, _| {
            let mut r = result();
            r.identity.uid = 3141592653;
            r.identity.gid = 3141592653;
            Ok(r)
        });
        let e = if matched {
            Execution::unix_bridge_for_test(Arc::new(b), &c)
        } else {
            execution(b)
        };
        let dir = private_temp();
        let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
        let id = a.start_dedicated("alpha", "run_command").unwrap();
        let v = e
            .dispatch(Some(request()), a.clone(), id)
            .await
            .ok()
            .unwrap();
        assert_eq!(v["stdout"]["head"], if matched { MARKER } else { "" });
        if !matched {
            assert!(!v.to_string().contains("3141592653"));
        }
        e.drain().await.unwrap();
    }
}
#[test]
fn t3_fake_port_receives_exact_request() {
    for mode in ["path", "tool", "probe"] {
        let mut r = request();
        r.timeout_s = 7;
        r.output_bytes = 333;
        if mode == "path" {
            r.tool = None;
            r.executable = Some("/synthetic/target".into());
        }
        let expected = exec::RunSpec {
            executable: Layout::for_test(Platform::Linux).binary().into(),
            arguments: if mode == "probe" {
                vec!["tool-probe".into()]
            } else {
                let mut args = vec![
                    "exec-child".into(),
                    mode.into(),
                    r.executable.clone().or_else(|| r.tool.clone()).unwrap(),
                    r.cwd.clone(),
                ];
                args.extend(r.args.clone());
                args
            },
            cwd: PathBuf::new(),
            timeout: Duration::from_secs(if mode == "probe" { 60 } else { r.timeout_s }),
            output_bytes: if mode == "probe" {
                4096
            } else {
                r.output_bytes
            },
        };
        let b = bridge(move |s, _, _| {
            assert_eq!(s.executable, expected.executable);
            assert_eq!(s.arguments, expected.arguments);
            assert_eq!(s.cwd, expected.cwd);
            assert_eq!(s.timeout, expected.timeout);
            assert_eq!(s.output_bytes, expected.output_bytes);
            Ok(result())
        });
        let report = b.run(
            if mode == "probe" { None } else { Some(&r) },
            &Cancellation::default(),
            &Cancellation::default(),
        );
        assert!(
            if mode == "probe" {
                matches!(report, Report::Discovered { .. })
            } else {
                matches!(report, Report::Completed { .. })
            },
            "port assertions must survive the bridge panic boundary"
        );
    }
}
#[test]
fn t1_preflight_order_stored_refusal_and_readiness() {
    let c = config();
    let l = Layout::for_test(Platform::Linux);
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let b = UnixBridge::new(
        &l,
        &c,
        Some("/fake/runs".into()),
        |_| Err(Unavailable::PreflightFailed),
        Box::new(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Readiness::pending(),
    );
    for _ in 0..2 {
        assert_eq!(b.preflight(), Err(Unavailable::PreflightFailed));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let mut b = b;
    b.check = Box::new(|| Err(Unavailable::AccountIdentityChanged));
    assert_eq!(b.preflight(), Err(Unavailable::AccountIdentityChanged));
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
    assert!(matches!(
        derive(&l, &c, "/fake/runs".into()),
        Err(Unavailable::MechanismUnavailable)
    ));
}
#[test]
fn t4_exact_reports_and_t5_evidence_without_assessment_oracle() {
    use exec::{NotStarted as N, RunError as E};
    for (error, expected) in [
        (E::NotStarted(N::Invalid), Creation::NotInvoked),
        (E::NotStarted(N::Resource), Creation::NotInvoked),
        (E::SpawnFailed, Creation::Unknown),
        (E::AfterSpawn, Creation::Created),
    ] {
        match failure(error) {
            Report::Failed { creation } => assert_eq!(creation, expected),
            _ => panic!("wrong report shape"),
        }
    }
    assert!(matches!(
        failure(E::NotStarted(N::Cancelled)),
        Report::CancelledBeforeSpawn
    ));
    let mut r = result();
    r.tree_boundary = exec::TreeBoundary::MacProcessGroupBestEffort;
    r.tree_cleanup_confirmed = false;
    let ev = evidence(&r);
    assert_eq!(ev.point, Some(ObservationPoint::PreExec));
    assert_eq!(ev.tree_boundary, Some(TreeBoundary::ProcessGroup));
    assert_eq!(ev.cleanup_confirmed, Some(false));
    assert_eq!(ev.stop, Some(StopReason::Exited));
    assert_eq!(ev.exit_code, Some(0));
    assert!(matches!(
        ev.observation,
        Some(Observation::Unix {
            uid: 1002,
            gid: 1002
        })
    ));
    for outcome in [
        exec::Outcome::TimedOut,
        exec::Outcome::Cancelled,
        exec::Outcome::Shutdown,
    ] {
        r.outcome = outcome;
        assert_eq!(evidence(&r).exit_code, None);
    }
}
#[tokio::test]
async fn t7_dropped_request_cancels_running_fake_and_joins() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let e = execution(bridge(move |_, c, h| {
        seen.store(1, Ordering::SeqCst);
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while !c.is_cancelled() && std::time::Instant::now() < until {
            std::thread::yield_now();
        }
        assert!(c.is_cancelled());
        assert!(!h.is_cancelled());
        let mut r = result();
        r.outcome = exec::Outcome::Cancelled;
        seen.store(2, Ordering::SeqCst);
        Ok(r)
    }));
    let dir = private_temp();
    let a = Arc::new(Activity::open_service_fixture(dir.path()).unwrap());
    let id = a.start_dedicated("alpha", "run_command").unwrap();
    let f = e.dispatch(Some(request()), a.clone(), id);
    tokio::time::timeout(Duration::from_secs(3), async {
        while calls.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(f);
    tokio::time::timeout(Duration::from_secs(3), async {
        while calls.load(Ordering::SeqCst) != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    e.drain().await.unwrap();
    let page = serde_json::to_value(list(&a)).unwrap();
    assert_eq!(page["activities"][0]["stop_reason"], "cancelled");
}

#[test]
fn t3_max_agent_tail_stays_separate_from_prefix() {
    let mut r = request();
    r.tool = None;
    r.executable = Some(format!("/{}", "x".repeat(4095)));
    r.cwd = "c".repeat(4096);
    r.args = vec!["a".repeat(512); 128];
    let b = bridge(|s, _, _| {
        assert_eq!(s.arguments.len(), 132);
        assert_eq!(s.arguments[2].len(), 4096);
        assert_eq!(s.arguments[3].len(), 4096);
        assert_eq!(
            s.arguments[4..].iter().map(String::len).sum::<usize>(),
            65536
        );
        assert!(s.cwd.as_os_str().is_empty());
        Ok(result())
    });
    assert!(matches!(
        b.run(Some(&r), &Cancellation::default(), &Cancellation::default()),
        Report::Completed { .. }
    ));
    // Validator refusal is represented by the same typed Invalid phase; the
    // actual phased boundary assertions remain in the unchanged Stage B suite.
    r.args.push("extra".into());
    let b = bridge(|s, _, _| {
        assert_eq!(s.arguments.len(), 133);
        Err(exec::RunError::NotStarted(exec::NotStarted::Invalid))
    });
    assert!(matches!(
        b.run(Some(&r), &Cancellation::default(), &Cancellation::default()),
        Report::Failed {
            creation: Creation::NotInvoked
        }
    ));
}

trait TestRights {
    fn add_rights(self) -> Vec<u8>;
}
impl TestRights for String {
    fn add_rights(self) -> Vec<u8> {
        (self + "{\"rights\":{\"no_new_privs\":true,\"sudo\":\"absent\"}}\n").into_bytes()
    }
}

mod e6;

mod e7;

fn wiring_location(
    file: exec::hierarchy::setup::FsResult<exec::hierarchy::setup::ProcFile>,
) -> Option<exec::hierarchy::parse::Location> {
    let file = file.ok()?;
    if file.magic != exec::hierarchy::setup::PROC_MAGIC {
        return None;
    }
    locate(&file.bytes)
}

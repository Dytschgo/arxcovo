use super::super::{
    config::{Account, AccountKind},
    layout::Platform,
    owner_plan::{Action, Operation},
};
use super::*;
use std::cell::RefCell;
fn argv(a: &[&str]) -> Vec<OsString> {
    a.iter().map(OsString::from).collect()
}
fn configure() -> Vec<OsString> {
    argv(&[
        "service",
        "configure",
        "--build-account",
        "builder",
        "--client-account",
        "1001",
        "--rights",
        "standard",
        "--workspace",
        "/work",
        "--agent",
        "test=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ])
}
fn config_args() -> ConfigureArgs {
    match parse(&configure()).ok().unwrap() {
        Command::Configure(a) => a,
        _ => panic!(),
    }
}
#[test]
fn parser_matrix_and_fixture_fence() {
    for verb in ["install", "start", "stop", "status", "uninstall"] {
        assert!(parse(&argv(&["service", verb])).is_ok());
    }
    assert!(parse(&configure()).is_ok());
    for a in [
        argv(&[
            "client-config",
            "new",
            "--principal",
            "test",
            "--out",
            "/private/file",
        ]),
        argv(&[
            "client-config",
            "new",
            "--out",
            "/private/file",
            "--principal",
            "test",
        ]),
    ] {
        assert!(parse(&a).is_ok());
    }
    for a in [
        argv(&[]),
        argv(&["service"]),
        argv(&["client-config"]),
        argv(&["client-config", "new"]),
        argv(&[
            "client-config",
            "new",
            "--principal",
            "marker",
            "--out",
            "marker",
            "extra",
        ]),
        argv(&["service", "run"]),
        argv(&["service", "install", "extra"]),
        argv(&["service", "run", "extra"]),
        argv(&["service", "run", "--config", "private-marker"]),
        argv(&[
            "client-config",
            "new",
            "--principal",
            "test",
            "--out",
            "/file",
            "extra",
        ]),
    ] {
        assert!(parse(&a).is_err());
    }
    for (flag, val) in [
        ("--workspace", "/duplicate"),
        ("--help", "x"),
        ("--", "x"),
        ("-e", "x"),
        ("--env", "missing"),
        ("--env", "=x"),
        ("--agent", "p="),
        ("--agent", "=x"),
        ("--env=K", "x"),
    ] {
        let mut a = configure();
        a.extend(argv(&[flag, val]));
        assert!(parse(&a).is_err());
    }
    for val in ["Admin", "--other", ""] {
        let mut a = configure();
        a[7] = val.into();
        assert!(parse(&a).is_err());
    }
    let mut a = configure();
    a[6] = "--rights=admin".into();
    assert!(parse(&a).is_err());
    for (flag, n) in [("--env", 17), ("--agent", 64)] {
        let mut a = configure();
        for _ in 0..n {
            a.extend(argv(&[flag, "K=value"]));
        }
        assert!(parse(&a).is_err());
    }
    let mut a = configure();
    a.extend(argv(&[
        "--env",
        "K=x",
        "--env",
        "K=x",
        "--agent",
        "test=hash",
    ]));
    let Command::Configure(c) = parse(&a).ok().unwrap() else {
        panic!()
    };
    assert_eq!(c.environment, vec![("K".into(), "x".into()); 2]);
    assert_eq!(c.agents.len(), 2);
    assert!(parse(&vec![OsString::from("x"); 171]).is_err());
    let mut a = configure();
    a[3] = "x".repeat(4201).into();
    assert!(parse(&a).is_err());
    let mut a = configure();
    for _ in 0..9 {
        a.extend([
            OsString::from("--env"),
            OsString::from(format!("K={}", "x".repeat(3900))),
        ]);
    }
    assert!(parse(&a).is_err());
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut a = configure();
        a[3] = OsString::from_wide(&[0xd800]);
        assert!(parse(&a).is_err());
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut a = configure();
        a[3] = OsString::from_vec(vec![0xff]);
        assert!(parse(&a).is_err());
    }
}

fn marker_configure() -> Vec<OsString> {
    argv(&[
        "service",
        "configure",
        "--build-account",
        "marker-build",
        "--client-account",
        "marker-client",
        "--rights",
        "admin",
        "--workspace",
        "/marker-workspace",
        "--env",
        "PATH=/marker-tools",
        "--env",
        "LANG=marker-lang",
        "--agent",
        "marker-one=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--agent",
        "marker-two=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    ])
}
fn marker_args() -> ConfigureArgs {
    ConfigureArgs {
        build: "marker-build".into(),
        client: "marker-client".into(),
        rights: Rights::Admin,
        workspace: "/marker-workspace".into(),
        environment: vec![
            ("PATH".into(), "/marker-tools".into()),
            ("LANG".into(), "marker-lang".into()),
        ],
        agents: vec![
            ("marker-one".into(), "a".repeat(64)),
            ("marker-two".into(), "b".repeat(64)),
        ],
    }
}
#[test]
fn parser_exact_bounds_and_guide_forms() {
    let mut maximal = configure();
    maximal.truncate(10);
    for _ in 0..16 {
        maximal.extend(argv(&["--env", "A=b=c"]));
    }
    for _ in 0..64 {
        maximal.extend(argv(&[
            "--agent",
            "marker=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ]));
    }
    assert_eq!(maximal.len(), 170);
    assert!(parse(&maximal).is_ok());
    let mut single = configure();
    single[3] = "x".repeat(4200).into();
    assert_eq!(single[3].len(), 4200);
    assert!(parse(&single).is_ok());
    let mut total = configure();
    for _ in 0..8 {
        total.extend(argv(&["--env", &format!("A={}", "x".repeat(3900))]));
    }
    let size: usize = total.iter().map(|a| a.len()).sum();
    total.extend(argv(&[
        "--env",
        &format!("A={}", "x".repeat(32768 - size - 7)),
    ]));
    assert_eq!(total.iter().map(|a| a.len()).sum::<usize>(), 32768);
    assert!(parse(&total).is_ok());
    total.last_mut().unwrap().push("x");
    assert!(parse(&total).is_err());
    let guide = argv(&[
        "service",
        "configure",
        "--build-account",
        "Arxcovo",
        "--client-account",
        "marker-login",
        "--rights",
        "standard",
        "--workspace",
        r"D:\ArxcovoData\work",
        "--env",
        r"PATH=C:\ArxcovoTools\cargo\bin;C:\Windows\System32;C:\Windows",
        "--env",
        r"RUSTUP_HOME=C:\ArxcovoTools\rustup",
        "--env",
        r"CARGO_HOME=D:\ArxcovoData\work\.cargo",
        "--env",
        r"SystemRoot=C:\Windows",
        "--agent",
        "claude-code=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ]);
    let Command::Configure(g) = parse(&guide).ok().unwrap() else {
        panic!()
    };
    assert_eq!(g.environment.len(), 4);
    assert_eq!(g.workspace, r"D:\ArxcovoData\work");
    assert_eq!(
        g.environment[0].1,
        r"C:\ArxcovoTools\cargo\bin;C:\Windows\System32;C:\Windows"
    );
    assert!(parse(&marker_configure()).is_ok());
    let mut equal = configure();
    equal.extend(argv(&["--env", "A=b=c"]));
    let Command::Configure(c) = parse(&equal).ok().unwrap() else {
        panic!()
    };
    assert_eq!(c.environment, [("A".into(), "b=c".into())]);
    for index in [2, 4, 6, 8] {
        let mut a = configure();
        a.drain(index..index + 2);
        assert!(parse(&a).is_err());
    }
    for index in [2, 4] {
        let mut a = argv(&[
            "client-config",
            "new",
            "--principal",
            "marker",
            "--out",
            "/marker",
        ]);
        a.drain(index..index + 2);
        assert!(parse(&a).is_err());
    }
    let mut odd = configure();
    odd.push("--env".into());
    assert!(parse(&odd).is_err());
    for flag in ["--Workspace", "--work", "--build", "--client", "--right"] {
        let mut a = configure();
        a[8] = flag.into();
        assert!(parse(&a).is_err());
    }
    let marker = marker_args();
    assert!(
        marker
            .agents
            .iter()
            .all(|(p, h)| super::super::config::valid_principal(p)
                && h.len() == 64
                && h.bytes().all(|b| b.is_ascii_hexdigit()))
    );
}
#[test]
fn request_forwards_all_marker_fields() {
    let a = marker_args();
    let r = a.request();
    assert_eq!(r.build_account, a.build);
    assert_eq!(r.client_account, a.client);
    assert_eq!(r.rights, a.rights);
    assert_eq!(r.workspace, a.workspace);
    assert_eq!(r.environment, a.environment);
    assert_eq!(r.agents, a.agents);
}
#[test]
fn right_reports_use_library_display_in_dispatch() {
    for (report, token) in [
        (RightReport::Removed, "batch-right-removed"),
        (RightReport::AlreadyAbsent, "batch-right-already-absent"),
        (
            RightReport::RetainedUnproven,
            "batch-right-retained-unproven",
        ),
        (
            RightReport::RetainedNotArxcovo,
            "batch-right-retained-not-arxcovo",
        ),
        (RightReport::NotRecorded, "batch-right-not-recorded"),
    ] {
        assert_eq!(right_output(report), token);
        assert_eq!(right_output(report), report.to_string());
        let mut ops = RecordingOps {
            calls: vec![],
            fail: None,
            report,
        };
        let (mut out, mut err) = (vec![], vec![]);
        assert_eq!(
            dispatch(Command::Uninstall, &mut ops, &mut out, &mut err),
            0
        );
        assert_eq!(out, format!("{token}\n").as_bytes());
        assert!(err.is_empty());
        assert_eq!(ops.calls, ["uninstall"]);
    }
}
struct RecordingOps {
    calls: Vec<&'static str>,
    fail: Option<CliError>,
    report: RightReport,
}
impl RecordingOps {
    fn call(&mut self, n: &'static str) -> Result<(), CliError> {
        self.calls.push(n);
        self.fail.take().map_or(Ok(()), Err)
    }
}
impl OwnerOps for RecordingOps {
    fn install(&mut self) -> Result<(), CliError> {
        self.call("install")
    }
    fn configure(&mut self, a: &ConfigureArgs) -> Result<(), CliError> {
        assert!(*a == marker_args());
        self.call("configure")
    }
    fn start(&mut self) -> Result<(), CliError> {
        self.call("start")
    }
    fn stop(&mut self) -> Result<(), CliError> {
        self.call("stop")
    }
    fn status(&mut self) -> Result<&'static str, CliError> {
        self.call("status")?;
        Ok("running")
    }
    fn uninstall(&mut self) -> Result<Option<RightReport>, CliError> {
        self.call("uninstall")?;
        Ok(Some(self.report))
    }
    fn client(&mut self, a: &ClientArgs) -> Result<String, CliError> {
        assert_eq!(a.principal, "test");
        assert_eq!(a.out, "/private/file");
        self.call("client")?;
        Ok("a".repeat(64))
    }
}
#[test]
fn dispatch_each_verb_and_fixed_outputs() {
    for (args, call, output) in [
        (argv(&["service", "install"]), "install", String::new()),
        (marker_configure(), "configure", String::new()),
        (argv(&["service", "start"]), "start", String::new()),
        (argv(&["service", "stop"]), "stop", String::new()),
        (argv(&["service", "status"]), "status", "running\n".into()),
        (
            argv(&["service", "uninstall"]),
            "uninstall",
            "batch-right-not-recorded\n".into(),
        ),
        (
            argv(&[
                "client-config",
                "new",
                "--principal",
                "test",
                "--out",
                "/private/file",
            ]),
            "client",
            format!("test={}\n", "a".repeat(64)),
        ),
    ] {
        let mut ops = RecordingOps {
            calls: vec![],
            fail: None,
            report: RightReport::NotRecorded,
        };
        let (mut out, mut err) = (vec![], vec![]);
        assert_eq!(
            dispatch(parse(&args).ok().unwrap(), &mut ops, &mut out, &mut err),
            0
        );
        assert_eq!(ops.calls, [call]);
        assert_eq!(out, output.as_bytes());
        assert!(err.is_empty());
    }
}
#[test]
fn exhaustive_errors_have_fixed_tokens_and_codes() {
    for args in [
        argv(&["service", "install"]),
        marker_configure(),
        argv(&["service", "start"]),
        argv(&["service", "stop"]),
        argv(&["service", "status"]),
        argv(&["service", "uninstall"]),
        argv(&[
            "client-config",
            "new",
            "--principal",
            "test",
            "--out",
            "/private/file",
        ]),
    ] {
        let services = [
            (Error::Config, "service-config", 3),
            (Error::Identity, "service-identity", 3),
            (Error::AccountMissing, "service-account-missing", 3),
            (Error::Rights, "service-rights", 3),
            (Error::Workspace, "service-workspace", 3),
            (Error::Permissions, "service-permissions", 4),
            (Error::OwnerRequired, "service-owner-required", 4),
            (Error::Layout, "service-layout", 7),
            (
                Error::NativeAdapterMissing,
                "service-native-adapter-missing",
                7,
            ),
            (Error::Manager, "service-manager", 1),
            (Error::NotInstalled, "service-not-installed", 1),
            (Error::Foreign, "service-foreign-registration", 1),
            (Error::Active, "service-active", 1),
        ];
        let mut cases: Vec<_> = services
            .into_iter()
            .map(|(e, t, c)| {
                assert_eq!(t, e.to_string());
                (CliError::Service(e), t, c)
            })
            .collect();
        for (e, t) in [
            (Failure::InvalidPlan, "service-owner-plan"),
            (Failure::Io, "service-owner-io"),
            (Failure::Conflict, "service-owner-conflict"),
            (Failure::Busy, "service-owner-busy"),
            (Failure::RecoveryRequired, "service-owner-recovery-required"),
            (Failure::RightUnproven, "service-owner-right-unproven"),
            (Failure::Unsupported, "service-owner-unsupported"),
        ] {
            assert_eq!(t, e.to_string());
            cases.push((CliError::Owner(e), t, 1));
        }
        for (e, t, c) in [
            (ClientError::Argument, "client-config-argument", 3),
            (ClientError::Elevated, "client-config-elevated", 4),
            (ClientError::Parent, "client-config-parent", 4),
            (ClientError::Exists, "client-config-exists", 1),
            (ClientError::Io, "client-config-io", 1),
            (ClientError::Partial, "client-config-partial", 1),
        ] {
            cases.push((CliError::Client(e), t, c));
        }
        for (e, t, c) in cases {
            let mut ops = RecordingOps {
                calls: vec![],
                fail: Some(e),
                report: RightReport::NotRecorded,
            };
            let (mut out, mut err) = (vec![], vec![]);
            let cmd = parse(&args).ok().unwrap();
            assert_eq!(dispatch(cmd, &mut ops, &mut out, &mut err), c);
            assert!(out.is_empty());
            assert_eq!(err, format!("arxd: {t}\n").as_bytes());
        }
    }
}
struct Counting {
    calls: RefCell<Vec<String>>,
    stale: bool,
    missing: bool,
}
impl Resolver for Counting {
    fn account(&self, n: &str) -> Result<Account, Error> {
        self.calls.borrow_mut().push(format!("account:{n}"));
        if self.missing && n == "old-builder" {
            return Err(Error::AccountMissing);
        };
        Ok(Account {
            identity: if n == "builder" || n == "old-builder" {
                "1002".into()
            } else {
                n.into()
            },
            gid: Some(1002),
            kind: AccountKind::User,
            administrative: false,
        })
    }
    fn workspace_safe(&self, p: &str) -> Result<bool, Error> {
        self.calls.borrow_mut().push(format!("workspace:{p}"));
        Ok(!(self.stale && p == "/old"))
    }
}
#[test]
fn unix_full_previous_validation_and_no_agent_merge() {
    for platform in [Platform::Linux, Platform::Macos] {
        let l = Layout::for_test(platform);
        let ctx = OwnerContext {
            admin: true,
            running_binary: l.binary(),
        };
        for (stale, missing, changed, expected) in [
            (false, false, false, None),
            (true, false, false, Some(Error::Workspace)),
            (false, true, false, Some(Error::Identity)),
            (false, false, true, Some(Error::Identity)),
        ] {
            let res = Counting {
                calls: RefCell::new(vec![]),
                stale: false,
                missing: false,
            };
            let mut oldargs = config_args();
            oldargs.workspace = "/old".into();
            oldargs.build = "old-builder".into();
            oldargs.agents[0].0 = "old-agent".into();
            if changed {
                oldargs.build = "1003".into();
            }
            let old = ServiceConfig::from_request(&oldargs.request(), &l, &res)
                .unwrap()
                .serialized(false)
                .unwrap();
            let res = Counting {
                calls: RefCell::new(vec![]),
                stale,
                missing,
            };
            let mut executed = false;
            let result = unix_configure(
                || Ok(Some(old.clone())),
                &res,
                &l,
                &ctx,
                &config_args(),
                |p| {
                    executed = true;
                    let op = p
                        .operations()
                        .iter()
                        .find_map(|o| match o {
                            Operation::AtomicConfig {
                                bytes, expected, ..
                            } => Some((bytes, expected)),
                            _ => None,
                        })
                        .unwrap();
                    assert!(op.1.as_ref() == Some(&old));
                    let v: serde_json::Value = serde_json::from_slice(op.0).unwrap();
                    assert_eq!(v["agents"].as_array().unwrap().len(), 1);
                    assert_eq!(v["agents"][0]["principal"], "test");
                    Ok(())
                },
            );
            assert_eq!(result, expected.map_or(Ok(()), Err));
            assert_eq!(executed, expected.is_none());
            let mut expected_calls = vec![
                "account:builder",
                "account:1001",
                "account:1001",
                "account:builder",
                "workspace:/work",
                "account:1001",
                "account:old-builder",
            ];
            if !missing {
                expected_calls.push("workspace:/old");
            }
            if expected.is_none() {
                expected_calls.extend([
                    "account:1001",
                    "account:builder",
                    "workspace:/work",
                    "account:1001",
                    "account:old-builder",
                    "workspace:/old",
                ]);
            }
            if changed {
                expected_calls[6] = "account:1003";
                expected_calls.extend([
                    "account:1001",
                    "account:builder",
                    "workspace:/work",
                    "account:1001",
                    "account:1003",
                    "workspace:/old",
                ]);
            }
            assert_eq!(*res.calls.borrow(), expected_calls);
            if missing {
                assert_eq!(
                    error(CliError::Service(result.unwrap_err())),
                    ("service-identity", 3)
                );
            }
        }
        let res = Counting {
            calls: RefCell::new(vec![]),
            stale: false,
            missing: false,
        };
        assert_eq!(
            unix_configure(
                || Ok(Some(b"invalid".to_vec())),
                &res,
                &l,
                &ctx,
                &config_args(),
                |_| panic!("no mutation")
            ),
            Err(Error::Config)
        );
    }
}
#[test]
fn unix_verbs_use_exact_owner_plans_without_config() {
    for platform in [Platform::Linux, Platform::Macos] {
        let l = Layout::for_test(platform);
        let c = OwnerContext {
            admin: true,
            running_binary: l.binary(),
        };
        for action in [
            Action::Install,
            Action::Start,
            Action::Stop,
            Action::Uninstall,
        ] {
            let expected = owner_plan::owner_plan(&l, &c, action).unwrap();
            let mut called = false;
            unix_verb(action, &l, &c, |p| {
                called = true;
                assert!(p.operations() == expected.operations());
                if action == Action::Start {
                    assert!(p.operations().contains(&Operation::StartPreflight));
                }
                Ok(())
            })
            .unwrap();
            assert!(called);
        }
    }
}

#[test]
fn native_entry_signature_compile_check_only() {
    let _create: fn(&str, &str) -> Result<String, ClientError> = crate::client_config::create;
}

#[cfg(windows)]
#[test]
fn windows_configure_fully_parses_previous_before_batch() {
    use crate::service::{owner_transactions::*, owner_windows::ConfigureSession};
    struct Res {
        stale: bool,
        missing: bool,
        calls: RefCell<Vec<String>>,
    }
    impl Resolver for Res {
        fn account(&self, n: &str) -> Result<Account, Error> {
            self.calls.borrow_mut().push(format!("account:{n}"));
            if self.missing && n == "old-builder" {
                return Err(Error::AccountMissing);
            }
            Ok(Account {
                identity: match n {
                    "builder" | "old-builder" => "S-1-5-21-1-2-3-1002",
                    "changed-builder" => "S-1-5-21-1-2-3-1003",
                    "1001" => "S-1-5-21-1-2-3-1001",
                    n => n,
                }
                .into(),
                gid: None,
                kind: AccountKind::User,
                administrative: false,
            })
        }
        fn workspace_safe(&self, p: &str) -> Result<bool, Error> {
            self.calls.borrow_mut().push(format!("workspace:{p}"));
            Ok(!(self.stale && p == "C:/old"))
        }
    }
    struct Store(bool);
    impl OwnerStore for Store {
        fn record(&mut self) -> Result<Option<(OwnerRecord, [u8; 32])>, Failure> {
            Ok(if self.0 {
                Some((
                    OwnerRecord::decode(
                        br#"{"version":1,"sid":"S-1-5-21-1-2-3-1002","state":"granted"}"#,
                    )
                    .unwrap(),
                    [1; 32],
                ))
            } else {
                None
            })
        }
        fn replace_record(
            &mut self,
            _: Option<[u8; 32]>,
            _: &OwnerRecord,
        ) -> Result<[u8; 32], Failure> {
            panic!()
        }
        fn stage_config(&mut self, _: &[u8], _: Option<[u8; 32]>) -> Result<(), Failure> {
            panic!()
        }
        fn publish_config(&mut self) -> Result<(), Failure> {
            panic!()
        }
    }
    struct Right(bool);
    impl BatchRight for Right {
        fn observe(&mut self, _: &str) -> Result<bool, Failure> {
            Ok(self.0)
        }
        fn grant(&mut self, _: &str) -> Result<(), Failure> {
            panic!()
        }
        fn revoke(&mut self, _: &str) -> Result<(), Failure> {
            panic!()
        }
    }
    struct Session {
        prev: Vec<u8>,
        calls: Vec<&'static str>,
        recorded: bool,
    }
    impl ConfigureSession for Session {
        fn read_config(&mut self) -> Result<Option<Vec<u8>>, Failure> {
            self.calls.push("read");
            Ok(Some(self.prev.clone()))
        }
        fn observe_batch(&mut self, sid: &str) -> Result<BatchObservation, Failure> {
            self.calls.push("observe");
            assert_eq!(sid, "S-1-5-21-1-2-3-1002");
            observe_batch(sid, &mut Store(self.recorded), &mut Right(self.recorded))
        }
        fn configure(&mut self, req: &Configure) -> Result<(), Failure> {
            self.calls.push("configure");
            assert!(req.expected() == Some(self.prev.as_slice()));
            assert_eq!(req.adds_right(), !self.recorded);
            let v: serde_json::Value = serde_json::from_slice(req.bytes()).unwrap();
            assert_eq!(v["granted_batch_right"], true);
            Ok(())
        }
    }
    let l = Layout::for_test(Platform::Windows);
    let ctx = OwnerContext {
        admin: true,
        running_binary: l.binary(),
    };
    let mut args = config_args();
    args.workspace = "C:/new".into();
    let res = Res {
        stale: false,
        missing: false,
        calls: RefCell::new(vec![]),
    };
    let new = ServiceConfig::from_request(&args.request(), &l, &res).unwrap();
    args.workspace = "C:/old".into();
    args.build = "old-builder".into();
    let prev = ServiceConfig::from_request(&args.request(), &l, &res)
        .unwrap()
        .serialized(false)
        .unwrap();
    for (stale, missing, changed, recorded) in [
        (true, false, false, false),
        (false, true, false, false),
        (false, false, true, false),
        (false, false, false, false),
        (false, false, false, true),
    ] {
        let res = Res {
            stale,
            missing,
            calls: RefCell::new(vec![]),
        };
        let previous = if changed {
            args.build = "changed-builder".into();
            ServiceConfig::from_request(
                &args.request(),
                &l,
                &Res {
                    stale: false,
                    missing: false,
                    calls: RefCell::new(vec![]),
                },
            )
            .unwrap()
            .serialized(false)
            .unwrap()
        } else {
            prev.clone()
        };
        let mut session = Session {
            prev: previous,
            calls: vec![],
            recorded,
        };
        let result = windows_configure(&mut session, &l, &ctx, &new, &res);
        if stale || missing || changed {
            let CliError::Service(e) = result.err().unwrap() else {
                panic!()
            };
            assert_eq!(
                e,
                if stale {
                    Error::Workspace
                } else {
                    Error::Identity
                }
            );
            assert_eq!(
                error(CliError::Service(e)),
                (
                    if stale {
                        "service-workspace"
                    } else {
                        "service-identity"
                    },
                    3
                )
            );
            assert_eq!(
                session.calls,
                if changed {
                    vec!["read", "observe"]
                } else {
                    vec!["read"]
                }
            );
        } else {
            assert!(result.is_ok());
            assert_eq!(session.calls, ["read", "observe", "configure"]);
        }
        if missing {
            assert_eq!(
                *res.calls.borrow(),
                ["account:S-1-5-21-1-2-3-1001", "account:old-builder"]
            );
            assert!(!res.calls.borrow().iter().any(|p| p == "workspace:C:/old"));
        } else {
            assert!(res.calls.borrow().iter().any(|p| p == "workspace:C:/old"));
        }
    }
    let mut session = Session {
        prev: b"invalid".to_vec(),
        calls: vec![],
        recorded: false,
    };
    assert!(matches!(
        windows_configure(&mut session, &l, &ctx, &new, &res),
        Err(CliError::Service(Error::Config))
    ));
    assert_eq!(session.calls, ["read"]);
}

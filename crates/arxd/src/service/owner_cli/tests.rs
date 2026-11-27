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
        cluster: None,
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
    for (index, flag) in [
        (8, "--Workspace"),
        (8, "--work"),
        (2, "--build"),
        (4, "--client"),
        (6, "--right"),
    ] {
        let mut a = configure();
        a[index] = flag.into();
        assert!(parse(&a).is_err(), "abbreviated or wrong-case flag refused");
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
    fn cluster_config(&mut self, a: &cluster_install::Request) -> Result<u32, CliError> {
        assert!(*a == cluster_install::tests::request(Role::Joiner));
        self.call("cluster-config")?;
        Ok(1100)
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
        (
            cluster_config_argv(Role::Joiner),
            "cluster-config",
            "daemon-uid=1100\n".into(),
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
        cluster_config_argv(Role::Joiner),
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
        for (e, t, c) in [
            (InstallError::Argument, "cluster-config-argument", 3),
            (InstallError::Account, "cluster-config-account", 3),
            (InstallError::Address, "cluster-config-address", 3),
            (InstallError::Layout, "cluster-config-layout", 3),
            (
                InstallError::OwnerRequired,
                "cluster-config-owner-required",
                4,
            ),
            (InstallError::Exists, "cluster-config-exists", 1),
            (InstallError::Io, "cluster-config-io", 1),
            (InstallError::Unsupported, "cluster-config-unsupported", 7),
        ] {
            assert_eq!(t, e.to_string());
            cases.push((CliError::Cluster(e), t, c));
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
                |new| {
                    let v: serde_json::Value =
                        serde_json::from_slice(&new.serialized(false).unwrap()).unwrap();
                    assert!(v.get("cluster").is_none());
                    Ok(())
                },
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
                |_| Ok(()),
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

fn cluster_config_argv(role: Role) -> Vec<OsString> {
    let r = cluster_install::tests::request(role);
    let mut a = argv(&[
        "service",
        "cluster-config",
        "--out",
        &r.out,
        "--role",
        if role == Role::Founder {
            "founder"
        } else {
            "joiner"
        },
        "--workspace",
        &r.workspace,
        "--state",
        &r.state,
        "--credentials",
        &r.credentials,
        "--wrap",
        &r.wrap,
        "--custody",
        &r.custody,
        "--history",
        &r.history,
        "--owner-socket",
        &r.owner_socket,
        "--peer",
        &r.peer,
        "--enrollment",
        &r.enrollment,
        "--operator-account",
        &r.operator_account,
        "--control-group",
        &r.control_group,
        "--agent-account",
        &r.agent_account,
        "--build-account",
        &r.build_account,
    ]);
    if let Some(issuer) = &r.issuer_peer {
        a.extend(argv(&["--issuer-peer", issuer]));
    }
    a
}

fn parsed_cluster(a: &[OsString]) -> Option<cluster_install::Request> {
    match parse(a) {
        Ok(Command::ClusterConfig(r)) => Some(r),
        _ => None,
    }
}

#[test]
fn cluster_config_parser_maps_every_flag_exactly_once() {
    for role in [Role::Founder, Role::Joiner] {
        let parsed = parsed_cluster(&cluster_config_argv(role)).unwrap();
        assert!(parsed == cluster_install::tests::request(role));
        // Flag order is free.
        let mut reversed = cluster_config_argv(role);
        let pairs = reversed.split_off(2);
        let mut chunks = pairs.chunks(2).collect::<Vec<_>>();
        chunks.reverse();
        reversed.extend(chunks.into_iter().flatten().cloned());
        assert!(parsed_cluster(&reversed).unwrap() == cluster_install::tests::request(role));
        // Every flag except the joiner-only issuer is required.
        let full = cluster_config_argv(role);
        for index in (2..full.len()).step_by(2) {
            let mut a = full.clone();
            let flag = a[index].clone();
            a.drain(index..index + 2);
            let parsed = parsed_cluster(&a);
            assert_eq!(parsed.is_some(), flag == "--issuer-peer", "{flag:?}");
        }
        // Duplicates, unknown or abbreviated flags, flag-like or empty values
        // and a dangling flag are usage errors.
        for extra in [
            ["--out", "/etc/arxcovo-cluster/other.json"],
            ["--role", "founder"],
            ["--daemon-account", "arxd-cluster"],
            ["--owner_socket", "/a/b"],
            ["--Out", "/a/b"],
            ["--cluster-config", "/a/b"],
            ["--founder-cluster", "07"],
        ] {
            let mut a = full.clone();
            a.extend(argv(&extra));
            assert!(parse(&a).is_err(), "{extra:?}");
        }
        for value in ["", "--peer"] {
            let mut a = full.clone();
            a[3] = value.into();
            assert!(parse(&a).is_err());
        }
        let mut a = full.clone();
        a.push("--peer".into());
        assert!(parse(&a).is_err());
    }
    let mut a = cluster_config_argv(Role::Founder);
    a.extend(argv(&["--issuer-peer", "192.168.1.10:7400"]));
    assert!(parsed_cluster(&a).unwrap().issuer_peer.is_some());
    for role in ["Founder", "member", "create-founder", "join"] {
        let mut a = cluster_config_argv(Role::Founder);
        a[5] = role.into();
        assert!(parse(&a).is_err(), "{role}");
    }
    for a in [
        argv(&["service", "cluster-config"]),
        argv(&["service", "cluster-configure"]),
        argv(&["service", "cluster"]),
    ] {
        assert!(parsed_cluster(&a).is_none());
    }
}

#[test]
fn cluster_config_dispatch_checks_arguments_before_native_authority() {
    let mut a = cluster_config_argv(Role::Joiner);
    let index = a.iter().position(|v| v == "--owner-socket").unwrap() + 1;
    a[index] = "/owner.sock".into();
    let mut ops = RecordingOps {
        calls: vec![],
        fail: None,
        report: RightReport::NotRecorded,
    };
    let (mut out, mut err) = (vec![], vec![]);
    assert_eq!(
        dispatch(parse(&a).ok().unwrap(), &mut ops, &mut out, &mut err),
        3
    );
    assert!(ops.calls.is_empty());
    assert!(out.is_empty());
    assert_eq!(err, b"arxd: cluster-config-argument\n");
}

#[test]
fn configure_cluster_flags_come_as_a_pair() {
    let mut a = configure();
    a.extend(argv(&[
        "--cluster-config",
        "/etc/arxcovo-cluster/cluster.json",
        "--cluster-intent",
        "create-founder",
    ]));
    let Command::Configure(c) = parse(&a).ok().unwrap() else {
        panic!()
    };
    assert_eq!(
        c.cluster,
        Some((
            "/etc/arxcovo-cluster/cluster.json".into(),
            "create-founder".into()
        ))
    );
    let request = c.request();
    let cluster = request.cluster.as_ref().unwrap();
    assert_eq!(cluster.config, "/etc/arxcovo-cluster/cluster.json");
    assert_eq!(cluster.intent, "create-founder");
    assert!(config_args().cluster.is_none());
    assert!(config_args().request().cluster.is_none());
    for extra in [
        &["--cluster-config", "/etc/arxcovo-cluster/cluster.json"][..],
        &["--cluster-intent", "join"],
        &[
            "--cluster-config",
            "/a/b",
            "--cluster-intent",
            "join",
            "--cluster-intent",
            "join",
        ],
        &[
            "--cluster-config",
            "/a/b",
            "--cluster-config",
            "/a/c",
            "--cluster-intent",
            "join",
        ],
        &["--cluster-config", "--cluster-intent"],
    ] {
        let mut a = configure();
        a.extend(argv(extra));
        assert!(parse(&a).is_err(), "{extra:?}");
    }
    // client-config never accepts cluster flags.
    assert!(
        parse(&argv(&[
            "client-config",
            "new",
            "--principal",
            "test",
            "--out",
            "/private/file",
            "--cluster-config",
            "/a/b",
        ]))
        .is_err()
    );
}

struct ClusterAccounts;
impl Resolver for ClusterAccounts {
    fn account(&self, n: &str) -> Result<Account, Error> {
        Ok(Account {
            identity: match n {
                "builder" => "1002",
                "arxd-cluster" => "1100",
                other => other,
            }
            .into(),
            gid: Some(if n == "arxd-cluster" { 1100 } else { 1002 }),
            kind: AccountKind::User,
            administrative: false,
        })
    }
    fn workspace_safe(&self, _: &str) -> Result<bool, Error> {
        Ok(true)
    }
}

#[test]
fn unix_configure_vets_cluster_launch_before_reading_or_mutating() {
    let mut args = config_args();
    args.cluster = Some(("/etc/arxcovo-cluster/cluster.json".into(), "join".into()));
    let l = Layout::for_test(Platform::Linux);
    let ctx = OwnerContext {
        admin: true,
        running_binary: l.binary(),
    };
    let mut written = None;
    unix_configure(
        || Ok(None),
        &ClusterAccounts,
        &l,
        &ctx,
        &args,
        |new| {
            let v: serde_json::Value =
                serde_json::from_slice(&new.serialized(false).unwrap()).unwrap();
            assert_eq!(
                v["cluster"],
                serde_json::json!({
                    "config": "/etc/arxcovo-cluster/cluster.json",
                    "intent": "join",
                    "daemon_uid": 1100,
                    "daemon_gid": 1100,
                })
            );
            Ok(())
        },
        |p| {
            written = p.operations().iter().find_map(|o| match o {
                Operation::AtomicConfig { bytes, .. } => Some(bytes.clone()),
                _ => None,
            });
            Ok(())
        },
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&written.unwrap()).unwrap();
    assert_eq!(v["cluster"]["intent"], "join");
    // A refused cluster preflight reads and mutates nothing.
    assert_eq!(
        unix_configure(
            || panic!("no previous read"),
            &ClusterAccounts,
            &l,
            &ctx,
            &args,
            |_| Err(Error::Config),
            |_| panic!("no mutation"),
        ),
        Err(Error::Config)
    );
    // macOS refuses the cluster setting before preflight.
    let l = Layout::for_test(Platform::Macos);
    assert_eq!(
        unix_configure(
            || panic!("no previous read"),
            &ClusterAccounts,
            &l,
            &ctx,
            &args,
            |_| panic!("no preflight"),
            |_| panic!("no mutation"),
        ),
        Err(Error::Config)
    );
}

#[test]
fn flag_free_configure_replaces_a_legacy_stored_launch() {
    let l = Layout::for_test(Platform::Linux);
    let ctx = OwnerContext {
        admin: true,
        running_binary: l.binary(),
    };
    let mut legacy = config_args();
    legacy.cluster = Some(("/etc/arxcovo-cluster/cluster.json".into(), "join".into()));
    let previous = ServiceConfig::from_request(&legacy.request(), &l, &ClusterAccounts)
        .unwrap()
        .serialized(false)
        .unwrap();
    // A stored path with a redundant internal slash, which the start-time
    // component walk accepts but the strict owner spelling rule refuses.
    let mut value: serde_json::Value = serde_json::from_slice(&previous).unwrap();
    value["cluster"]["config"] = "/etc//arxcovo-cluster/cluster.json".into();
    let previous = serde_json::to_vec(&value).unwrap();
    assert!(ServiceConfig::parse(&previous, &l, &ClusterAccounts).is_ok());
    let mut replaced = None;
    unix_configure(
        || Ok(Some(previous.clone())),
        &ClusterAccounts,
        &l,
        &ctx,
        &config_args(),
        |_| Ok(()),
        |p| {
            replaced = p.operations().iter().find_map(|o| match o {
                Operation::AtomicConfig {
                    bytes, expected, ..
                } => Some((bytes.clone(), expected.clone())),
                _ => None,
            });
            Ok(())
        },
    )
    .unwrap();
    let (bytes, expected) = replaced.unwrap();
    // The plan expects the previous file exactly as the reader parsed it,
    // including the legacy spelling.
    let canonical = ServiceConfig::parse(&previous, &l, &ClusterAccounts)
        .unwrap()
        .serialized(false)
        .unwrap();
    assert_eq!(expected.as_ref(), Some(&canonical));
    let old: serde_json::Value = serde_json::from_slice(&expected.unwrap()).unwrap();
    assert_eq!(
        old["cluster"]["config"],
        "/etc//arxcovo-cluster/cluster.json"
    );
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v.get("cluster").is_none());
    // New owner input keeps the strict canonical spelling.
    let mut strict = config_args();
    strict.cluster = Some(("/etc//arxcovo-cluster/cluster.json".into(), "join".into()));
    assert_eq!(
        unix_configure(
            || panic!("no previous read"),
            &ClusterAccounts,
            &l,
            &ctx,
            &strict,
            |_| panic!("no preflight"),
            |_| panic!("no mutation"),
        ),
        Err(Error::Config)
    );
}

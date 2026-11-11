use super::*;
fn settings() -> Settings {
    Settings {
        mode: Mode::CurrentUser,
        acknowledge: Acknowledge::Shared,
        workspace: PathBuf::from("C:\\workspace"),
        environment: BTreeMap::from([("PATH".into(), "C:\\tools".into())]),
        limits: Limits {
            memory_bytes: 16777216,
            pids: 8,
        },
    }
}
#[test]
fn strict_config_and_argument_schema() {
    for v in [
        serde_json::json!({}),
        serde_json::json!({"mode":"other","acknowledge":"shares_owner_user_files_and_credentials"}),
        serde_json::json!({"mode":"development_current_user"}),
    ] {
        assert!(serde_json::from_value::<Settings>(v).is_err());
    }
    for v in [
        serde_json::json!({"tool":"cargo","mode":"development_current_user"}),
        serde_json::json!({"executable":"cargo.exe"}),
        serde_json::json!({"tool":"cargo","executable":"C:\\x.exe"}),
        serde_json::json!({"tool":"cargo","identity":"x"}),
        serde_json::json!({"tool":"cargo","sid":"S-1-5-18"}),
        serde_json::json!({"tool":"cargo","env":{}}),
    ] {
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
    let ok: Request =
        serde_json::from_value(serde_json::json!({"tool":"cargo","cwd":"a\\b/c"})).unwrap();
    assert!(ok.validate(), "positive control");
    for cwd in [
        "..", "a/../b", "C:x", "\\x", "/x", "//server", "a\\.\\b", "a:stream", "a ", "a.", "a|b",
    ] {
        let r: Request =
            serde_json::from_value(serde_json::json!({"tool":"cargo","cwd":cwd})).unwrap();
        assert!(!r.validate(), "cwd case {cwd:?}");
    }
    for (k, v) in [
        ("timeout_s", 0),
        ("timeout_s", 3601),
        ("output_bytes", 1),
        ("output_bytes", 8193),
    ] {
        let mut x = serde_json::json!({"tool":"cargo"});
        x[k] = v.into();
        assert!(!serde_json::from_value::<Request>(x).unwrap().validate());
    }
    for bad in [
        serde_json::json!({"tool":"cmd"}),
        serde_json::json!({"tool":"cargo.exe"}),
        serde_json::json!({"tool":"cargo","args":vec!["a"; 129]}),
        serde_json::json!({"tool":"cargo","args":["a\u{0}b"]}),
        serde_json::json!({"tool":"cargo","args":["x".repeat(8193)]}),
        serde_json::json!({"tool":"cargo","args":vec!["x".repeat(8192); 9]}),
    ] {
        assert!(!serde_json::from_value::<Request>(bad).unwrap().validate());
    }
    let edge: Request = serde_json::from_value(
        serde_json::json!({"tool":"cargo","args":vec!["a"; 128],"timeout_s":3600,"output_bytes":2}),
    )
    .unwrap();
    assert!(edge.validate(), "inclusive bounds");
}
#[test]
fn environment_block_is_exact_and_sorted_case_insensitively() {
    let mut s = settings();
    s.environment
        .insert("TEMP".into(), "C:\\private-temp".into());
    let block = String::from_utf16_lossy(&environment(&s));
    assert_eq!(block, "PATH=C:\\tools\0TEMP=C:\\private-temp\0\0");
    // Byte order would put B_UPPER first; Windows requires case-insensitive order.
    s.environment = BTreeMap::from([
        ("B_UPPER".into(), "2".into()),
        ("a_lower".into(), "1".into()),
    ]);
    let block = String::from_utf16_lossy(&environment(&s));
    assert_eq!(block, "a_lower=1\0B_UPPER=2\0\0");
}
#[test]
fn redaction_is_case_and_slash_insensitive_longest_first_and_keeps_context() {
    let mut s = settings();
    s.environment
        .insert("TEMP".into(), "C:\\workspace\\tmp".into());
    let text = redact_with(
        "c:/WORKSPACE/a C:\\workspace\\TMP/b\nuser=S-1-5-21-1-2-3\tS-1-x",
        vec![
            (s.workspace.to_string_lossy().into_owned(), "<workspace>"),
            (s.environment["TEMP"].clone(), "<temp>"),
        ],
    );
    assert_eq!(text, "<workspace>/a <temp>/b\nuser=<sid>\tS-1-x");
    let unicode = redact_with("\u{e9}C:\\W\u{e9}", vec![("c:\\w".into(), "<w>")]);
    assert_eq!(unicode, "\u{e9}<w>\u{e9}");
}
#[test]
fn redaction_terminates_when_a_label_contains_its_needle() {
    // A user named `user` or `temp`, or a one-letter name, must not hang the worker.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = redact_with(
            "user temp e",
            vec![
                ("user".into(), "<user>"),
                ("temp".into(), "<temp>"),
                ("e".into(), "<user>"),
            ],
        );
        let _ = tx.send(out);
    });
    let out = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("redaction must terminate");
    assert!(!out.is_empty() && !out.contains(" e") && out.starts_with('<'));
}
#[test]
fn worst_case_actual_sdk_execution_json_fits_response_bound() {
    use rmcp::model::*;
    for ch in ['\0', '"', '\\', '\u{fffd}', '\n'] {
        let s = bounded_text(ch.to_string().repeat(24576), 8192);
        let r = ResultData {
            execution_mode: MODE,
            claim: CLAIM,
            dedicated: false,
            stop_reason: StopReason::OutputLimit,
            exit_code: None,
            cleanup_confirmed: true,
            matches_current_user: true,
            stdout: s.clone(),
            stderr: s,
            stdout_dropped_bytes: u64::MAX,
            stderr_dropped_bytes: u64::MAX,
        };
        let inner = serde_json::to_string(&r).unwrap();
        let result = CallToolResult::success(vec![ContentBlock::text(inner)]);
        let bytes =
            serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":i64::MAX,"result":result}))
                .unwrap();
        assert!(bytes.len() < 65536, "{}", bytes.len());
    }
}
#[test]
fn capability_reports_honest_mode_and_fixed_unavailability() {
    let e = Execution::policy_fixture(settings());
    let c = e.capability();
    assert_eq!(c["available"], true);
    assert!(c["reason"].is_null());
    assert_eq!(c["mode"], MODE);
    assert_eq!(c["claim"], CLAIM);
    assert_eq!(c["isolation"], "none_shares_owner_user");
    assert_eq!(c["dedicated"], false);
    assert!(c["rights"].is_null());
    assert_eq!(
        (c["integrity"].as_str(), c["elevated"].as_bool()),
        (Some("medium"), Some(false))
    );
    e.shutdown();
    assert_eq!(
        (
            e.capability()["available"].as_bool(),
            e.capability()["reason"].as_str()
        ),
        (Some(false), Some("shutdown"))
    );
    e.quarantined.store(true, Ordering::Release);
    assert_eq!(e.capability()["reason"], "quarantined");
}
#[test]
fn development_metadata_never_carries_observed_identity_or_rights() {
    let r: Request =
        serde_json::from_value(serde_json::json!({"tool":"git","args":["a"]})).unwrap();
    let m = metadata(Some(&r), None, Some(Reason::Busy));
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["identity_mode"], "development_current_user");
    assert_eq!(
        (v["program"].as_str(), v["argc"].as_u64()),
        (Some("git"), Some(1))
    );
    for key in ["observed_kind", "observed_id", "observed_gid", "rights"] {
        assert!(v[key].is_null(), "{key}");
    }
}
/// Classification only: each case changes one field of a valid development
/// configuration and must be refused before any executor exists.
#[test]
fn development_configuration_classification() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (workspace, state, tools) = (root.join("ws"), root.join("state"), root.join("tools"));
    for d in [&workspace, &state, &tools] {
        std::fs::create_dir(d).unwrap();
    }
    let base = serde_json::json!({
        "state_dir": state,
        "agents": [{"principal":"a","token_sha256":"0".repeat(64)}],
        "development_execution": {
            "mode": "development_current_user",
            "acknowledge": "shares_owner_user_files_and_credentials",
            "workspace": workspace,
            "environment": {"PATH": tools, "SystemRoot": "C:\\Windows"},
            "limits": {"memory_bytes": 16777216, "pids": 1}
        }
    });
    let parse = |v: &serde_json::Value| serde_json::from_value::<crate::config::Config>(v.clone());
    let positive = parse(&base).unwrap().validate_development();
    assert_eq!(
        positive.is_ok(),
        cfg!(windows),
        "positive control / non-Windows refusal"
    );
    // Schema refusals (any platform).
    for (pointer, value) in [
        (
            "/development_execution/acknowledge",
            serde_json::json!("yes"),
        ),
        (
            "/development_execution/acknowledge",
            serde_json::Value::Null,
        ),
        (
            "/development_execution/mode",
            serde_json::json!("dedicated_account"),
        ),
        ("/development_execution/limits/extra", serde_json::json!(1)),
        ("/development_execution/executor", serde_json::json!("x")),
    ] {
        let mut v = base.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        v.pointer_mut(parent).unwrap()[key] = value;
        assert!(parse(&v).is_err(), "schema case {pointer}");
    }
    let mut missing = base.clone();
    missing["development_execution"]
        .as_object_mut()
        .unwrap()
        .remove("acknowledge");
    assert!(parse(&missing).is_err());
    // Semantic refusals (validated after parsing).
    let semantic: Vec<(&str, serde_json::Value)> = vec![
        ("/listen", serde_json::json!("127.0.0.1:0")),
        ("/development_execution/workspace", serde_json::json!(state)),
        ("/development_execution/workspace", serde_json::json!(root)),
        (
            "/development_execution/workspace",
            serde_json::json!(state.join("ws")),
        ),
        (
            "/development_execution/workspace",
            serde_json::json!("relative"),
        ),
        (
            "/development_execution/workspace",
            serde_json::json!(root.join("absent")),
        ),
        (
            "/development_execution/environment/PATH",
            serde_json::json!("tools"),
        ),
        (
            "/development_execution/environment/PATH",
            serde_json::json!(root.join("absent")),
        ),
        (
            "/development_execution/environment/USERPROFILE",
            serde_json::json!("C:\\x"),
        ),
        (
            "/development_execution/environment/TEMP",
            serde_json::json!("a\u{0}b"),
        ),
        (
            "/development_execution/environment/TEMP",
            serde_json::json!("x".repeat(4097)),
        ),
        (
            "/development_execution/limits/memory_bytes",
            serde_json::json!(16777215),
        ),
        (
            "/development_execution/limits/memory_bytes",
            serde_json::json!(68719476737u64),
        ),
        ("/development_execution/limits/pids", serde_json::json!(0)),
        (
            "/development_execution/limits/pids",
            serde_json::json!(1025),
        ),
    ];
    for (pointer, value) in semantic {
        let mut v = base.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        v.pointer_mut(parent).unwrap()[key] = value;
        let refused = parse(&v).map(|c| c.validate_development().is_err());
        assert!(refused.unwrap_or(true), "semantic case {pointer}");
    }
    let mut no_path = base.clone();
    no_path["development_execution"]["environment"]
        .as_object_mut()
        .unwrap()
        .remove("PATH");
    assert!(parse(&no_path).unwrap().validate_development().is_err());
    let mut s = parse(&base).unwrap().development_execution.unwrap();
    for (k, v) in (0..17).map(|i| (format!("K{i}"), String::new())) {
        s.environment.insert(k, v);
    }
    assert!(s.validate().is_err(), "more than 16 keys");
    // Disjointness is case- and slash-insensitive in both directions.
    let s = parse(&base).unwrap().development_execution.unwrap();
    let upper = PathBuf::from(
        workspace
            .to_string_lossy()
            .to_uppercase()
            .replace('\\', "/"),
    );
    assert!(s.disjoint(&upper).is_err());
    assert!(s.disjoint(&workspace.join("x")).is_err());
    assert!(s.disjoint(&root).is_err());
    assert!(
        s.disjoint(&root.join("ws2")).is_ok(),
        "sibling prefix is disjoint"
    );
}

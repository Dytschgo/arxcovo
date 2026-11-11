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
        serde_json::json!({"tool":"cargo","identity":"x"}),
    ] {
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
    for cwd in ["..", "a/../b", "C:x", "\\x", "//server", "a\\.\\b"] {
        let r: Request =
            serde_json::from_value(serde_json::json!({"tool":"cargo","cwd":cwd})).unwrap();
        assert!(!r.validate());
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
}
#[test]
fn config_environment_is_exact_and_redaction_is_best_effort() {
    let mut s = settings();
    s.environment
        .insert("TEMP".into(), "C:\\private-temp".into());
    let block = String::from_utf16_lossy(&environment(&s));
    assert_eq!(block, "PATH=C:\\tools\0TEMP=C:\\private-temp\0\0");
    let text = redact("c:/WORKSPACE/a C:\\private-temp/a S-1-5-21-1".into(), &s);
    assert!(text.contains("<workspace>"));
    assert!(text.contains("<temp>"));
    assert!(!text.contains("S-1-"));
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
        };
        let inner = serde_json::to_string(&r).unwrap();
        let result = CallToolResult::success(vec![ContentBlock::text(inner)]);
        let bytes =
            serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":i64::MAX,"result":result}))
                .unwrap();
        assert!(bytes.len() < 65536, "{}", bytes.len());
    }
}

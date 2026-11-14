#[test]
fn missing_configuration_starts_no_listener() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_arxd"))
        .output()
        .expect("binary starts");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("arxd: transport-frame-or-io")
    );
}

#[tokio::test]
async fn stdio_setup_reports_argument_errors_before_loading_client_config() {
    use std::{process::Stdio, time::Duration};

    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("private client.json");
    let path = missing.to_str().unwrap();
    let usage = "arxd: usage: arxd mcp-stdio --client-config FILE\n";
    for (args, code, diagnostic) in [
        (vec!["mcp-stdio"], 2, usage),
        (vec!["mcp-stdio", "--client-config"], 2, usage),
        (vec!["mcp-stdio", "--client-conf", path], 2, usage),
        (
            vec!["mcp-stdio", "--client-config", path, "extra"],
            2,
            usage,
        ),
        (
            vec!["mcp-stdio", "--client-config", path],
            1,
            "arxd: client-config\n",
        ),
    ] {
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_arxd"))
                .args(&args)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("setup error must exit promptly")
        .expect("binary starts");
        assert!(
            output.stdout.is_empty(),
            "diagnostics must not enter MCP stdout"
        );
        assert_eq!(output.status.code(), Some(code));
        assert_eq!(output.stderr, diagnostic.as_bytes());
    }
    assert!(!missing.exists(), "setup errors must not create a config");
}

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

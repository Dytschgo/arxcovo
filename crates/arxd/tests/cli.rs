#[test]
fn scaffold_starts_without_host_side_effects() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_arxd"))
        .output()
        .expect("binary starts");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "arxd 0.1.0: scaffold only\n"
    );
    assert!(output.stderr.is_empty());
}

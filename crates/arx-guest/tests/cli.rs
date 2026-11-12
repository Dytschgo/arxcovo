#[test]
fn scaffold_starts_without_host_side_effects() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_arx-guest"))
        .output()
        .expect("binary starts");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "arx-guest 0.1.0: scaffold only\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn ci_test_profile_keeps_runtime_checks_when_debuginfo_is_off() {
    assert!(
        std::hint::black_box(cfg!(debug_assertions)),
        "debug assertions must stay enabled in test builds"
    );
    let overflowed =
        std::panic::catch_unwind(|| std::hint::black_box(u8::MAX) + std::hint::black_box(1u8));
    assert!(
        overflowed.is_err(),
        "overflow checks and unwinding panics must stay enabled"
    );
    assert!(
        std::panic::Location::caller().file() == file!(),
        "compiled caller location smoke check"
    );
}

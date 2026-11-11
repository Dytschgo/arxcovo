use std::{process::Stdio, time::Duration};

async fn binary(args: &[&str]) -> std::process::Output {
    tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_arxd"))
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap()
}
#[tokio::test]
async fn service_run_requires_exact_arguments() {
    for args in [
        vec!["service"],
        vec!["service", "run", "--config", "private-marker"],
        vec!["service", "install"],
        vec!["service", "run", "extra"],
    ] {
        let output = binary(&args).await;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"arxd: usage: arxd service run\n");
    }
}
#[tokio::test]
async fn service_entry_fails_closed_without_manager_or_integration() {
    let output = binary(&["service", "run"]).await;
    assert!(output.stdout.is_empty());
    #[cfg(windows)]
    {
        assert_eq!(output.status.code(), Some(10));
        assert_eq!(output.stderr, b"arxd: service-manager-required\n");
    }
    #[cfg(unix)]
    {
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            output.stderr,
            b"arxd: service-layout-or-integration-unavailable\n"
        );
    }
}

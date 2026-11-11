use std::{path::PathBuf, sync::Arc};
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        // Error sources can contain configuration or OS paths. Keep stdout empty.
        eprintln!(
            "arxd: startup or server failure (check private configuration and state permissions)"
        );
        std::process::exit(1);
    }
}
async fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 || args[0] != "--config" {
        anyhow::bail!("usage: arxd --config <absolute-path>");
    }
    let config = Arc::new(arxd::config::Config::load(&PathBuf::from(&args[1]))?);
    let activity = Arc::new(arxd::store::Activity::open(&config.state_dir)?);
    let listener = arxd::http::bind(config.clone()).await?;
    let router = arxd::http::router(config, listener.local_addr()?, activity);
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

use std::{path::PathBuf, sync::Arc};
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!(
            "arxd: startup or server failure (check private configuration and state permissions)"
        );
        std::process::exit(1);
    }
    // Tokio stdin uses a blocking OS read that cannot be cancelled. Explicitly
    // terminate after transport EOF so a client holding stdin open cannot keep
    // an otherwise finished bridge alive during runtime shutdown.
    std::process::exit(0);
}
async fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 3 && args[0] == "mcp-stdio" && args[1] == "--client-config" {
        let config = arxd::local::ClientConfig::load(&PathBuf::from(&args[2]))?;
        return arxd::local::bridge(config).await;
    }
    if args.len() != 2 || args[0] != "--config" {
        anyhow::bail!("invalid arguments");
    }
    let config = Arc::new(arxd::config::Config::load(&PathBuf::from(&args[1]))?);
    let activity = Arc::new(arxd::store::Activity::open(&config.state_dir)?);
    let local = config
        .local
        .as_ref()
        .map(arxd::local::Listener::bind)
        .transpose()?;
    let http = if config.listen.is_some() {
        Some(arxd::http::bind(config.clone()).await?)
    } else {
        None
    };
    if local.is_none() && http.is_none() {
        anyhow::bail!("no configured listener");
    }
    let local_config = config.clone();
    let local_activity = activity.clone();
    let local_run = async move {
        match local {
            Some(listener) => listener.run(local_config, local_activity).await,
            None => std::future::pending().await,
        }
    };
    let http_run = async move {
        match http {
            Some(listener) => {
                let router = arxd::http::router(config, listener.local_addr()?, activity);
                axum::serve(listener, router).await?;
                Ok::<_, anyhow::Error>(())
            }
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        result = local_run => result,
        result = http_run => result,
        _ = tokio::signal::ctrl_c() => Ok(()),
    }
}

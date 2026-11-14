use anyhow::Context;
use std::{path::PathBuf, sync::Arc};
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    if args.first().is_some_and(|arg| {
        use std::os::unix::ffi::OsStrExt;
        arg.as_bytes().starts_with(b"exec-child") || arg.as_bytes().starts_with(b"tool-probe")
    }) {
        std::process::exit(arxd::unix_shim::entry(&args));
    }
    if args.first().is_some_and(|arg| arg == "service") {
        let code = if args.len() == 2 && args[1] == "run" {
            arxd::service::run()
        } else {
            std::process::exit(arxd::service::owner_cli::main(&args));
        };
        if code != arxd::service::ExitCode::Clean {
            eprintln!("arxd: {}", code.category());
        }
        std::process::exit(code as i32);
    }
    if args.first().is_some_and(|arg| arg == "client-config") {
        std::process::exit(arxd::service::owner_cli::main(&args));
    }
    if args.first().is_some_and(|arg| arg == "mcp-stdio")
        && (args.len() != 3 || args[1] != "--client-config")
    {
        eprintln!("arxd: usage: arxd mcp-stdio --client-config FILE");
        std::process::exit(2);
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("foreground runtime")
        .block_on(foreground());
}

async fn foreground() {
    if let Err(error) = run().await {
        // Only fixed categories, never the source chain (paths/credentials/OS errors).
        let category = error.to_string();
        let safe = match category.as_str() {
            "client-config"
            | "peer-connect-timeout"
            | "peer-connect-or-identity"
            | "peer-auth-timeout"
            | "peer-auth-rejected"
            | "server-config"
            | "server-state"
            | "server-bind"
            | "server-listener" => category.as_str(),
            _ => "transport-frame-or-io",
        };
        eprintln!("arxd: {safe}");
        std::process::exit(if safe == "server-config" { 3 } else { 1 });
    }
    // Tokio stdin uses a blocking OS read that cannot be cancelled. Explicitly
    // terminate after transport EOF so a client holding stdin open cannot keep
    // an otherwise finished bridge alive during runtime shutdown.
    std::process::exit(0);
}
async fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 3 && args[0] == "mcp-stdio" && args[1] == "--client-config" {
        let config =
            arxd::local::ClientConfig::load(&PathBuf::from(&args[2])).context("client-config")?;
        return arxd::local::bridge(config).await;
    }
    if args.len() != 2 || args[0] != "--config" {
        anyhow::bail!("invalid arguments");
    }
    let config =
        Arc::new(arxd::config::Config::load(&PathBuf::from(&args[1])).context("server-config")?);
    let activity =
        Arc::new(arxd::store::Activity::open(&config.state_dir).context("server-state")?);
    let local = config
        .local
        .as_ref()
        .map(arxd::local::Listener::bind)
        .transpose()
        .context("server-bind")?;
    let http = if config.listen.is_some() {
        Some(
            arxd::http::bind(config.clone())
                .await
                .context("server-bind")?,
        )
    } else {
        None
    };
    if local.is_none() && http.is_none() {
        anyhow::bail!("no configured listener");
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut local_shutdown = shutdown_rx.clone();
    let local_config = config.clone();
    let local_activity = activity.clone();
    let local_run = async move {
        match local {
            Some(listener) => tokio::select! {
                result = listener.run(local_config, local_activity) => result.context("server-listener"),
                _ = local_shutdown.changed() => Ok(()),
            },
            None => std::future::pending().await,
        }
    };
    let execution = config.execution.clone();
    let http_run = async move {
        match http {
            Some(listener) => {
                let router = arxd::http::router(config, listener.local_addr()?, activity);
                serve_http(listener, router, shutdown_rx).await?;
                Ok::<_, anyhow::Error>(())
            }
            None => std::future::pending().await,
        }
    };
    tokio::pin!(local_run, http_run);
    let result = tokio::select! {
        result = &mut local_run => result,
        result = &mut http_run => result,
        _ = tokio::signal::ctrl_c() => {
            if let Some(execution) = &execution { execution.shutdown(); }
            let _ = shutdown_tx.send(true);
            // Bound HTTP drain; local connections are aborted when this returns.
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut http_run).await.unwrap_or(Ok(()))
        },
    };
    if let Some(execution) = &execution {
        execution.shutdown();
        execution.drain().await;
    }
    result
}

async fn serve_http(
    listener: tokio::net::TcpListener,
    router: axum::Router,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = shutdown.changed().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn http_shutdown_drains_inflight_response() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let (start, finish) = (started.clone(), release.clone());
        let router = axum::Router::new().route(
            "/",
            axum::routing::get(move || async move {
                start.notify_one();
                finish.notified().await;
                "complete"
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let server = tokio::spawn(serve_http(listener, router, rx));
        let request = tokio::spawn(async move {
            reqwest::get(format!("http://{address}/"))
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        tx.send(true).unwrap();
        assert!(!server.is_finished());
        release.notify_one();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), request)
                .await
                .unwrap()
                .unwrap(),
            "complete"
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

use super::{Boundary, DashboardConfig};
use anyhow::{Result, ensure};
use axum::{Extension, Router, http::uri::Authority};
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::watch,
    task::JoinSet,
};
use tokio_rustls::TlsAcceptor;

const CONNECTIONS: usize = 32;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Listener {
    socket: TcpListener,
    tls: Option<TlsAcceptor>,
    pub(super) boundary: Boundary,
}

impl Listener {
    pub async fn bind(config: &DashboardConfig) -> Result<Self> {
        config.validate()?;
        let tls = config
            .tls
            .as_ref()
            .map(|tls| {
                let certificate: Vec<_> = tls
                    .certificates
                    .iter()
                    .map(|path| crate::local::private_file(path).map(CertificateDer::from))
                    .collect::<Result<_>>()?;
                let key = PrivateKeyDer::try_from(crate::local::private_file(&tls.private_key)?)
                    .map_err(|_| anyhow::anyhow!("invalid dashboard TLS key"))?;
                let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_protocol_versions(&[&rustls::version::TLS13])?
                .with_no_client_auth()
                .with_single_cert(certificate, key)?;
                Ok::<_, anyhow::Error>(TlsAcceptor::from(Arc::new(config)))
            })
            .transpose()?;
        let socket = TcpListener::bind(config.listen).await?;
        let address = socket.local_addr()?;
        let authority = match &config.tls {
            Some(tls) => canonical_authority(&tls.authority, config.listen)?,
            None if address.port() == 80 => "localhost".to_owned(),
            None => format!("localhost:{}", address.port()),
        };
        Ok(Self {
            socket,
            tls,
            boundary: Boundary::new(authority, config.tls.is_some()),
        })
    }

    pub async fn run(self, router: Router, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let mut tasks = JoinSet::new();
        let Listener { socket, tls, .. } = self;
        let result = loop {
            tokio::select! {
                biased;
                _ = shutdown.wait_for(|stop| *stop) => break Ok(()),
                _ = tasks.join_next(), if !tasks.is_empty() => {}
                connected = socket.accept() => {
                    let (stream, peer) = match connected {
                        Ok(value) => value,
                        Err(error) => break Err(error.into()),
                    };
                    if tasks.len() >= CONNECTIONS {
                        continue;
                    }
                    let router = router.clone().layer(Extension(peer));
                    let tls = tls.clone();
                    tasks.spawn(async move {
                        if let Some(tls) = tls {
                            if let Ok(Ok(stream)) = tokio::time::timeout(HANDSHAKE_TIMEOUT, tls.accept(stream)).await {
                                serve(stream, router).await;
                            }
                        } else {
                            serve(stream, router).await;
                        }
                    });
                }
            }
        };
        tasks.shutdown().await;
        result
    }
}

async fn serve(io: impl AsyncRead + AsyncWrite + Unpin + Send + 'static, router: Router) {
    let mut http = hyper::server::conn::http1::Builder::new();
    http.timer(TokioTimer::new())
        .header_read_timeout(HANDSHAKE_TIMEOUT)
        .max_headers(48)
        .max_buf_size(16 * 1024)
        .keep_alive(false);
    let _ = tokio::time::timeout(
        CONNECTION_TIMEOUT,
        http.serve_connection(TokioIo::new(io), TowerToHyperService::new(router)),
    )
    .await;
}

/// Returns the configured HTTPS authority in the form a browser serializes
/// into `Host` and `Origin`: lowercase host, and the port only when it is
/// not the HTTPS default 443 (written without leading zeros).
pub(super) fn canonical_authority(raw: &str, listen: SocketAddr) -> Result<String> {
    ensure!(
        raw.len() <= 255
            && raw
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-:[]".contains(&b)),
        "invalid dashboard authority"
    );
    let authority: Authority = raw.parse()?;
    let host = authority.host();
    let port = match &raw[host.len()..] {
        "" => 443,
        rest => {
            let digits = rest
                .strip_prefix(':')
                .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                .ok_or_else(|| anyhow::anyhow!("invalid dashboard authority port"))?;
            digits.parse::<u16>()?
        }
    };
    ensure!(
        !host.is_empty() && port != 0 && listen.port() == port,
        "dashboard authority must match listener port"
    );
    Ok(if port == 443 {
        host.to_owned()
    } else {
        format!("{host}:{port}")
    })
}

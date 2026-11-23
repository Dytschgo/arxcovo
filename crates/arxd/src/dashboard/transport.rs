use super::{Boundary, DashboardConfig, SystemClock, session::Sessions};
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
    pub(super) sessions: Arc<Sessions>,
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
            sessions: Arc::new(Sessions::new()),
        })
    }

    pub async fn run(self, router: Router, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let mut tasks = JoinSet::new();
        let Listener {
            socket,
            tls,
            sessions,
            ..
        } = self;
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
        let (_, streams) = tokio::join!(tasks.shutdown(), sessions.stop_streams(&SystemClock));
        result.and(streams)
    }
}

pub(super) async fn serve(
    io: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    router: Router,
) {
    let mut http = hyper::server::conn::http1::Builder::new();
    http.timer(TokioTimer::new())
        .header_read_timeout(HANDSHAKE_TIMEOUT)
        .max_headers(48)
        .max_buf_size(16 * 1024);
    let _ = tokio::time::timeout(
        CONNECTION_TIMEOUT,
        http.serve_connection(TokioIo::new(io), TowerToHyperService::new(router))
            .with_upgrades(),
    )
    .await;
}

/// Returns the configured HTTPS authority in the form a browser serializes
/// into `Host` and `Origin`, with the port only when it is not the HTTPS
/// default 443 (written without leading zeros). The host must already be in
/// its browser (WHATWG URL) spelling; any other spelling is refused, so the
/// stored authority always equals what browsers send.
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
        port != 0 && listen.port() == port,
        "dashboard authority must match listener port"
    );
    canonical_host(host)?;
    Ok(if port == 443 {
        host.to_owned()
    } else {
        format!("{host}:{port}")
    })
}

/// Refuses a host unless it is already in the WHATWG URL serialization:
/// compressed lowercase IPv6 with hex pieces only (no embedded IPv4),
/// dotted-quad IPv4, or a lowercase ASCII DNS name without empty labels
/// (one trailing dot is kept by browsers and allowed).
fn canonical_host(host: &str) -> Result<()> {
    if let Some(literal) = host.strip_prefix('[') {
        let literal = literal
            .strip_suffix(']')
            .ok_or_else(|| anyhow::anyhow!("invalid dashboard authority host"))?;
        let address: std::net::Ipv6Addr = literal
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid dashboard IPv6 literal"))?;
        let canonical = ipv6_url_form(&address);
        ensure!(
            literal == canonical,
            "dashboard authority host must be written as [{canonical}]"
        );
        return Ok(());
    }
    ensure!(
        !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b)),
        "invalid dashboard authority host"
    );
    let labels: Vec<&str> = host.strip_suffix('.').unwrap_or(host).split('.').collect();
    ensure!(
        labels.iter().all(|label| !label.is_empty()),
        "invalid dashboard authority host"
    );
    // WHATWG treats a host whose last label is a number (decimal or 0x hex)
    // as IPv4; only the strict dotted-quad spelling without a trailing dot
    // serializes unchanged.
    let last = labels.last().expect("split yields a label");
    let numeric = last.bytes().all(|b| b.is_ascii_digit())
        || last
            .strip_prefix("0x")
            .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()));
    if numeric {
        let address: std::net::Ipv4Addr = host
            .parse()
            .map_err(|_| anyhow::anyhow!("dashboard IPv4 authority must be a dotted quad"))?;
        ensure!(
            address.to_string() == host,
            "dashboard IPv4 authority must be a dotted quad"
        );
    }
    Ok(())
}

/// The WHATWG URL IPv6 serializer: lowercase hex pieces, the first longest
/// run of two or more zero pieces compressed to `::`, never IPv4 notation.
fn ipv6_url_form(address: &std::net::Ipv6Addr) -> String {
    let pieces = address.segments();
    let mut compress = None;
    let (mut best, mut index) = (1, 0);
    while index < pieces.len() {
        let run = pieces[index..]
            .iter()
            .take_while(|&&piece| piece == 0)
            .count();
        if run > best {
            (best, compress) = (run, Some(index));
        }
        index += run.max(1);
    }
    let mut output = String::new();
    let mut index = 0;
    while index < pieces.len() {
        if Some(index) == compress {
            output.push_str(if index == 0 { "::" } else { ":" });
            index += best;
            continue;
        }
        output.push_str(&format!("{:x}", pieces[index]));
        if index != pieces.len() - 1 {
            output.push(':');
        }
        index += 1;
    }
    output
}

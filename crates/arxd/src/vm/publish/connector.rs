//! One supervised connector, tied to an immutable image and authenticated loopback origin.
use std::{
    fmt, fs,
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener},
    num::NonZeroU16,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, ServerName, pem::PemObject},
};
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, sleep, timeout},
};
use tokio_rustls::TlsConnector;

use super::{
    Hostname, TunnelId,
    cloudflare::status_code,
    hex_id,
    windows::{HeldPath, Process, read_secret},
};
use crate::vm::paths::{ArtifactPin, LocalPath};

const MAX_IMAGE: u64 = 128 * 1024 * 1024;
const MAX_CA: u64 = 8192;
const MAX_CREDENTIAL: usize = 2048;
const IO_TIMEOUT: Duration = Duration::from_millis(200);
const TICK: Duration = Duration::from_millis(20);
const GRACE: Duration = Duration::from_millis(180);
const KILL_WAIT: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) enum ConnectorError {
    Custody,
    Credential,
    BinaryReceipt,
    Origin,
    Spawn,
    Crashed,
    Readiness,
    CleanupPending,
}

pub(in crate::vm) struct TunnelCredential(zeroize::Zeroizing<Vec<u8>>);

impl fmt::Debug for TunnelCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TunnelCredential([redacted])")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "PascalCase")]
struct CredentialJson<'a> {
    account_tag: &'a str,
    tunnel_secret: &'a str,
    #[serde(rename = "TunnelID")]
    tunnel_id: &'a str,
    #[serde(default)]
    endpoint: &'a str,
}

impl TunnelCredential {
    pub(in crate::vm) fn load(
        path: &Path,
        account: &str,
        tunnel: &TunnelId,
    ) -> Result<Self, ConnectorError> {
        let value = read_secret(path, MAX_CREDENTIAL).map_err(|_| ConnectorError::Credential)?;
        let json: CredentialJson<'_> =
            serde_json::from_slice(&value).map_err(|_| ConnectorError::Credential)?;
        let secret = json.tunnel_secret.as_bytes();
        // The locally created credential contains the standard base64 encoding of 32 bytes.
        if !hex_id(account)
            || json.account_tag != account
            || json.tunnel_id != tunnel.as_str()
            || !json.endpoint.is_empty()
            || secret.len() != 44
            || secret[43] != b'='
            || !secret[..43]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
            || !b"AEIMQUYcgkosw048".contains(&secret[42])
        {
            return Err(ConnectorError::Credential);
        }
        Ok(Self(value))
    }
}

/// Constructed by B2 only from the qualified owner install and pinned origin generation.
pub(in crate::vm) struct Plan<'a> {
    pub(in crate::vm) executable: &'a Path,
    pub(in crate::vm) binary_receipt: ArtifactPin,
    pub(in crate::vm) private_root: &'a Path,
    pub(in crate::vm) hostname: &'a Hostname,
    pub(in crate::vm) tunnel: &'a TunnelId,
    pub(in crate::vm) credential: &'a TunnelCredential,
    pub(in crate::vm) origin_port: NonZeroU16,
    pub(in crate::vm) origin_name: &'a Hostname,
    pub(in crate::vm) ca_path: &'a Path,
    pub(in crate::vm) ca_receipt: ArtifactPin,
}

pub(in crate::vm) struct Connector {
    process: Process,
    ready_port: NonZeroU16,
    origin: (NonZeroU16, Hostname, TlsConnector),
    _config: Config,
    _binary: HeldPath,
    _ca: HeldPath,
    _root: HeldPath,
}

impl Connector {
    /// Retry a pre-ready crash once, after proving the old Job empty.
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "exercised by the independent connector integration fixture"
    )]
    pub(in crate::vm) async fn start(plan: &Plan<'_>) -> Result<Self, ConnectorError> {
        use crate::service::Shutdown;
        Self::start_cancellable(plan, &Shutdown::default(), &Shutdown::default(), &mut None).await
    }

    pub(in crate::vm) async fn start_cancellable(
        plan: &Plan<'_>,
        retired: &crate::service::Shutdown,
        shutdown: &crate::service::Shutdown,
        unsettled: &mut Option<Self>,
    ) -> Result<Self, ConnectorError> {
        let result = Self::start_once(plan, retired, shutdown, unsettled).await;
        if matches!(&result, Err(ConnectorError::Crashed))
            && !retired.is_cancelled()
            && !shutdown.is_cancelled()
        {
            return Self::start_once(plan, retired, shutdown, unsettled).await;
        }
        result
    }

    async fn start_once(
        plan: &Plan<'_>,
        retired: &crate::service::Shutdown,
        shutdown: &crate::service::Shutdown,
        unsettled: &mut Option<Self>,
    ) -> Result<Self, ConnectorError> {
        let binary = HeldPath::open(plan.executable, false, true)
            .map_err(|_| ConnectorError::BinaryReceipt)?;
        if !plan
            .binary_receipt
            .verify(&binary.file, MAX_IMAGE)
            .unwrap_or(false)
        {
            return Err(ConnectorError::BinaryReceipt);
        }
        let root =
            HeldPath::open(plan.private_root, true, true).map_err(|_| ConnectorError::Custody)?;
        let ca = HeldPath::open(plan.ca_path, false, true).map_err(|_| ConnectorError::Origin)?;
        let tls = origin_tls(plan, &ca)?;
        probe_origin(plan.origin_port, plan.origin_name, &tls).await?;
        if retired.is_cancelled() || shutdown.is_cancelled() {
            return Err(ConnectorError::Origin);
        }
        let config = Config::create(plan, &root, &ca)?;
        let config_arg = config.path.to_str().ok_or(ConnectorError::Custody)?;
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| ConnectorError::Readiness)?;
        let port = listener
            .local_addr()
            .map_err(|_| ConnectorError::Readiness)?
            .port();
        let ready_port = NonZeroU16::new(port).ok_or(ConnectorError::Readiness)?;
        let metrics = format!("127.0.0.1:{port}");
        drop(listener);
        let process = Process::spawn(
            &binary.path,
            &[
                "--config",
                config_arg,
                "tunnel",
                "--no-autoupdate",
                "--metrics",
                &metrics,
                "--grace-period",
                "100ms",
                "run",
            ],
            &root.path,
            std::str::from_utf8(&plan.credential.0).map_err(|_| ConnectorError::Credential)?,
        )
        .map_err(|_| ConnectorError::Spawn)?;
        let mut connector = Self {
            process,
            ready_port,
            origin: (plan.origin_port, plan.origin_name.clone(), tls),
            _config: config,
            _binary: binary,
            _ca: ca,
            _root: root,
        };
        let outcome = tokio::select! {
            biased;
            _ = retired.cancelled() => Err(ConnectorError::Origin),
            _ = shutdown.cancelled() => Err(ConnectorError::Origin),
            result = async {
                connector.await_ready().await?;
                connector.origin_ready().await?;
                if !connector.running() {
                    return Err(ConnectorError::Crashed);
                }
                Ok(())
            } => result,
        };
        if let Err(error) = outcome {
            if let Err(uncertain) = connector.stop().await {
                *unsettled = Some(connector);
                return Err(uncertain);
            }
            return Err(error);
        }
        Ok(connector)
    }

    async fn await_ready(&self) -> Result<(), ConnectorError> {
        let limit = if cfg!(test) {
            Duration::from_millis(650)
        } else {
            Duration::from_secs(8)
        };
        let result = timeout(limit, async {
            loop {
                if !self.process.running() {
                    return Err(ConnectorError::Crashed);
                }
                if self.ready().await {
                    return Ok(());
                }
                sleep(TICK).await;
            }
        })
        .await;
        result.unwrap_or(Err(ConnectorError::Readiness))
    }

    pub(in crate::vm) fn running(&self) -> bool {
        self.process.running()
    }

    pub(in crate::vm) async fn origin_ready(&self) -> Result<(), ConnectorError> {
        let (port, name, tls) = &self.origin;
        probe_origin(*port, name, tls).await
    }

    pub(in crate::vm) async fn ready(&self) -> bool {
        let port = self.ready_port.get();
        timeout(IO_TIMEOUT, async {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.ok()?;
            let local = stream.local_addr().ok()?.port();
            if !self.process.owns_peer(port, local) {
                return None;
            }
            stream
                .write_all(b"GET /ready HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .await
                .ok()?;
            // Only the bounded status line is material. No body reaches application logs.
            let mut line = [0u8; 128];
            for end in 0..line.len() {
                line[end] = stream.read_u8().await.ok()?;
                if line[end] == b'\n' {
                    let text = std::str::from_utf8(&line[..=end])
                        .ok()?
                        .strip_suffix("\r\n")?;
                    return (status_code(text) == Some(200) && self.process.owns_peer(port, local))
                        .then_some(());
                }
            }
            None
        })
        .await
        .is_ok_and(|result| result.is_some())
    }

    /// Keep ownership if bounded settlement fails. Dropping the handle still kills the Job.
    pub(in crate::vm) async fn stop(&mut self) -> Result<(), ConnectorError> {
        if self.process.empty().unwrap_or(false) {
            return Ok(());
        }
        self.process.interrupt();
        if self.wait_empty(GRACE).await {
            return Ok(());
        }
        let _ = self.process.kill();
        if self.wait_empty(KILL_WAIT).await {
            return Ok(());
        }
        Err(ConnectorError::CleanupPending)
    }

    async fn wait_empty(&self, duration: Duration) -> bool {
        let deadline = Instant::now() + duration;
        loop {
            if self.process.empty().unwrap_or(false) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            sleep(TICK).await;
        }
    }
}

fn origin_tls(plan: &Plan<'_>, ca: &HeldPath) -> Result<TlsConnector, ConnectorError> {
    let refusal = ConnectorError::Origin;
    if !plan.origin_name.as_str().ends_with(".invalid") {
        return Err(refusal);
    }
    let mut pem = Vec::new();
    (&ca.file)
        .take(MAX_CA + 1)
        .read_to_end(&mut pem)
        .map_err(|_| refusal)?;
    if !plan.ca_receipt.verify(&pem[..], MAX_CA).unwrap_or(false) {
        return Err(refusal);
    }
    let mut certs = CertificateDer::pem_slice_iter(&pem);
    let cert = certs.next().ok_or(refusal)?.map_err(|_| refusal)?;
    if certs.next().is_some() {
        return Err(refusal);
    }
    let mut roots = RootCertStore::empty();
    roots.add(cert).map_err(|_| refusal)?;
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|_| refusal)?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

async fn probe_origin(
    port: NonZeroU16,
    origin_name: &Hostname,
    tls: &TlsConnector,
) -> Result<(), ConnectorError> {
    let failure = ConnectorError::Origin;
    let name = ServerName::try_from(origin_name.as_str().to_owned()).map_err(|_| failure)?;
    timeout(IO_TIMEOUT, async {
        let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port.get())).await?;
        tls.connect(name, socket).await
    })
    .await
    .map_err(|_| failure)?
    .map_err(|_| failure)?;
    Ok(())
}

struct Config {
    path: PathBuf,
    held: Option<HeldPath>,
}

impl Config {
    fn create(plan: &Plan<'_>, root: &HeldPath, ca: &HeldPath) -> Result<Self, ConnectorError> {
        let refusal = ConnectorError::Custody;
        let ca = ca.path.to_str().ok_or(refusal)?;
        let root = LocalPath::parse(root.path.to_str().ok_or(refusal)?).map_err(|_| refusal)?;
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| refusal)?;
        let local = root
            .join(&format!("ingress-{}.yml", hex::encode(nonce)))
            .map_err(|_| refusal)?;
        let path = PathBuf::from(local.as_str());
        let text = super::ingress(
            plan.tunnel,
            std::array::from_ref(plan.hostname),
            plan.origin_port,
            plan.origin_name,
            ca,
        );
        // Owned by TokenUser with a protected DACL before any content exists.
        let mut output = super::windows::create_private(&path).map_err(|_| refusal)?;
        let mut config = Self { path, held: None };
        output.write_all(text.as_bytes()).map_err(|_| refusal)?;
        drop(output);
        let held = HeldPath::open(&config.path, false, true).map_err(|_| refusal)?;
        let mut actual = Vec::new();
        (&held.file)
            .take((text.len() + 1) as u64)
            .read_to_end(&mut actual)
            .map_err(|_| refusal)?;
        if actual != text.as_bytes() {
            return Err(refusal);
        }
        config.held = Some(held);
        Ok(config)
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        drop(self.held.take());
        let _ = fs::remove_file(&self.path);
    }
}

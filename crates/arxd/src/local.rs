//! Opt-in local inventory transport. No execution or service authority.
use anyhow::{Result, bail};
use rmcp::{ServiceExt, transport::async_rw::AsyncRwTransport};
use serde::Deserialize;
use std::{io::Read, path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(windows)]
#[path = "local_windows.rs"]
mod platform;
#[cfg(unix)]
#[path = "local_unix.rs"]
mod platform;

pub const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
pub const SESSION_TIMEOUT: Duration = Duration::from_secs(300);
pub const FRAME_LIMIT: usize = 8192;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub address: String,
    /// Windows SID or Unix decimal UID allowed to connect at the OS boundary.
    pub client_identity: String,
}
impl Endpoint {
    pub fn validate(&self) -> Result<()> {
        platform::validate_address(&self.address)?;
        platform::validate_identity(&self.client_identity)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    pub address: String,
    pub expected_server_identity: String,
    // Kept in the private configuration, never accepted through argv or environment.
    token: String,
}
fn private_file(path: &Path) -> Result<Vec<u8>> {
    crate::config::checked_path(path)?;
    crate::security::private(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("parent required"))?;
    crate::security::private(parent)?;
    platform::owned(path)?;
    platform::owned(parent)?;
    if !path.is_file() {
        bail!("file required");
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(16385)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16384 {
        bail!("configuration too large");
    }
    Ok(bytes)
}
impl ClientConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let value: Self = serde_json::from_slice(&private_file(path)?)
            .map_err(|_| anyhow::anyhow!("invalid client configuration"))?;
        platform::validate_address(&value.address)?;
        platform::validate_identity(&value.expected_server_identity)?;
        if value.token.len() != 64
            || !value
                .token
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            bail!("invalid credential");
        }
        Ok(value)
    }
}

/// Exposed for the daemon and tests; construction reserves the endpoint exclusively.
pub struct Listener(platform::Listener);
impl Listener {
    pub fn bind(endpoint: &Endpoint) -> Result<Self> {
        endpoint.validate()?;
        Ok(Self(platform::Listener::bind(endpoint)?))
    }
    pub async fn run(
        mut self,
        config: Arc<crate::config::Config>,
        activity: Arc<crate::store::Activity>,
    ) -> Result<()> {
        let capacity = Arc::new(tokio::sync::Semaphore::new(8));
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                stream = self.0.accept() => {
                    let stream = stream?;
                    let Ok(permit) = capacity.clone().try_acquire_owned() else { continue; };
                    let config = config.clone();
                    let activity = activity.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let _ = tokio::time::timeout(SESSION_TIMEOUT, connection(stream, config, activity)).await;
                    });
                }
                _ = tasks.join_next(), if !tasks.is_empty() => {}
            }
        }
        // JoinSet aborts all connections when the listener future is dropped.
    }
}
async fn connection<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    mut stream: S,
    config: Arc<crate::config::Config>,
    activity: Arc<crate::store::Activity>,
) -> Result<()> {
    let principal = tokio::time::timeout(AUTH_TIMEOUT, async {
        let mut preamble = [0u8; 70]; // "ARX1 " + exactly 64 hex + newline
        stream.read_exact(&mut preamble).await?;
        if &preamble[..5] != b"ARX1 " || preamble[69] != b'\n' {
            bail!("invalid authentication");
        }
        let token = std::str::from_utf8(&preamble[5..69])?;
        config
            .authenticate(token)
            .ok_or_else(|| anyhow::anyhow!("authentication failed"))
    })
    .await??;
    stream.write_all(b"OK\n").await?;
    let (read, write) = tokio::io::split(stream);
    let handler = crate::mcp::Inventory {
        activity,
        principal: Some(crate::mcp::Principal(principal)),
    };
    let service = handler
        .serve(AsyncRwTransport::new_server(read, write).with_max_line_length(FRAME_LIMIT))
        .await?;
    service.waiting().await?;
    Ok(())
}

/// No credential bytes are written until connect() has verified kernel identity.
pub async fn bridge(config: ClientConfig) -> Result<()> {
    let mut stream = tokio::time::timeout(AUTH_TIMEOUT, platform::connect(&config)).await??;
    tokio::time::timeout(AUTH_TIMEOUT, async {
        stream.write_all(b"ARX1 ").await?;
        stream.write_all(config.token.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        let mut ack = [0; 3];
        stream.read_exact(&mut ack).await?;
        if &ack != b"OK\n" {
            bail!("authentication failed");
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    drop(config);
    let (read, write) = tokio::io::split(stream);
    // MCP's official newline-delimited stdio wire format, unchanged end to end.
    // The SDK owns initialize/tool dispatch on the daemon; the bridge owns no state.
    tokio::select! {
        result = forward(tokio::io::stdin(), write) => result,
        result = forward(read, tokio::io::stdout()) => result,
    }
}
async fn forward<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut read: R,
    mut write: W,
) -> Result<()> {
    loop {
        let mut line = Vec::new();
        let present = tokio::time::timeout(SESSION_TIMEOUT, async {
            loop {
                let mut byte = [0];
                if read.read(&mut byte).await? == 0 {
                    if !line.is_empty() {
                        bail!("incomplete frame");
                    }
                    return Ok(false);
                }
                if line.len() >= FRAME_LIMIT {
                    bail!("frame too large");
                }
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    return Ok::<_, anyhow::Error>(true);
                }
            }
        })
        .await??;
        if !present {
            return Ok(());
        }
        tokio::time::timeout(AUTH_TIMEOUT, async {
            write.write_all(&line).await?;
            write.flush().await
        })
        .await??;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bridge_frames_are_bounded_and_partial_frames_are_withheld() {
        for payload in [vec![b'x'; FRAME_LIMIT + 1], b"partial".to_vec()] {
            let (mut input, reader) = tokio::io::duplex(FRAME_LIMIT * 2);
            let (writer, mut output) = tokio::io::duplex(FRAME_LIMIT * 2);
            input.write_all(&payload).await.unwrap();
            drop(input);
            assert!(forward(reader, writer).await.is_err());
            let mut bytes = Vec::new();
            output.read_to_end(&mut bytes).await.unwrap();
            assert!(bytes.is_empty());
        }
        let (mut input, reader) = tokio::io::duplex(128);
        let (writer, mut output) = tokio::io::duplex(128);
        input.write_all(b"{\"jsonrpc\":\"2.0\"}\n").await.unwrap();
        drop(input);
        forward(reader, writer).await.unwrap();
        let mut bytes = Vec::new();
        output.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"{\"jsonrpc\":\"2.0\"}\n");
    }
}

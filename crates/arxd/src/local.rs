//! Opt-in local inventory transport. No execution or service authority.
use anyhow::{Context, Result, bail};
use rmcp::{ServiceExt, transport::async_rw::AsyncRwTransport};
use serde::Deserialize;
use std::{io::Read, path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

#[cfg(windows)]
#[path = "local_windows.rs"]
mod platform;
#[cfg(unix)]
#[path = "local_unix.rs"]
mod platform;

pub const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
pub const FRAME_TIMEOUT: Duration = Duration::from_secs(300);
pub const FRAME_LIMIT: usize = 8192;
pub const RESPONSE_LIMIT: usize = 65536;

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

// Credential-query failure is a rejected peer, never an authenticated fallback.
fn verified_peer<S>(stream: S, identity_matches: Result<bool>) -> Option<S> {
    match identity_matches {
        Ok(true) => Some(stream),
        _ => None,
    }
}

/// Exposed for the daemon and tests; construction reserves the endpoint exclusively.
// None is a rejected/disconnected peer; Err is a listener failure.
trait Acceptor {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;
    fn accept(&mut self) -> impl std::future::Future<Output = Result<Option<Self::Stream>>> + Send;
}
pub struct Listener(platform::Listener);
impl Listener {
    pub fn bind(endpoint: &Endpoint) -> Result<Self> {
        endpoint.validate()?;
        Ok(Self(platform::Listener::bind(endpoint)?))
    }
    pub async fn run(
        self,
        config: Arc<crate::config::Config>,
        activity: Arc<crate::store::Activity>,
    ) -> Result<()> {
        run_listener(self.0, config, activity).await
    }
}
async fn run_listener(
    mut listener: impl Acceptor,
    config: Arc<crate::config::Config>,
    activity: Arc<crate::store::Activity>,
) -> Result<()> {
    let capacity = Arc::new(tokio::sync::Semaphore::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    let mut reported_rejection = false;
    loop {
        while tasks.try_join_next().is_some() {}
        tokio::select! {
            stream = listener.accept() => {
                let Some(stream) = stream? else {
                    if !reported_rejection {
                        eprintln!("arxd: server-peer-rejected");
                        reported_rejection = true;
                    }
                    // Rate-limit rejection work, including credential lookup failures.
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    continue;
                };
                let Ok(permit) = capacity.clone().try_acquire_owned() else {
                    drop(stream);
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    continue;
                };
                let config = config.clone();
                let activity = activity.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = connection(stream, config, activity).await;
                });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
    // JoinSet aborts all connections when the listener future is dropped.
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
    tokio::time::timeout(AUTH_TIMEOUT, stream.write_all(b"OK\n")).await??;
    let (read, write) = tokio::io::split(stream);
    let handler = crate::mcp::Inventory {
        execution: config.execution.clone(),
        activity,
        principal: Some(crate::mcp::Principal(principal)),
    };
    // Bound SDK requests at 8 KiB and response frames at 64 KiB. Idle authenticated
    // sessions may retain one of eight slots; partial frames and writes time out.
    let (sdk, pump) = tokio::io::duplex(FRAME_LIMIT);
    let (sdk_read, sdk_write) = tokio::io::split(sdk);
    let (pump_read, pump_write) = tokio::io::split(pump);
    let protocol = async {
        let service = tokio::time::timeout(
            AUTH_TIMEOUT,
            handler.serve(
                AsyncRwTransport::new_server(sdk_read, sdk_write).with_max_line_length(FRAME_LIMIT),
            ),
        )
        .await??;
        service.waiting().await?;
        Ok::<_, anyhow::Error>(())
    };
    tokio::select! {
        result = protocol => result?,
        result = forward(read, pump_write) => result?,
        result = forward_bounded(pump_read, write, RESPONSE_LIMIT) => result?,
    }
    Ok(())
}

/// No credential bytes are written until connect() has verified kernel identity.
pub async fn bridge(config: ClientConfig) -> Result<()> {
    bridge_io(config, tokio::io::stdin(), tokio::io::stdout()).await
}
/// The stdio bridge over any byte streams; tests drive it with in-memory pipes.
async fn bridge_io<I: AsyncRead + Unpin, O: AsyncWrite + Unpin>(
    config: ClientConfig,
    input: I,
    output: O,
) -> Result<()> {
    let mut stream = tokio::time::timeout(AUTH_TIMEOUT, platform::connect(&config))
        .await
        .context("peer-connect-timeout")?
        .context("peer-connect-or-identity")?;
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
    .await
    .context("peer-auth-timeout")?
    .context("peer-auth-rejected")?;
    drop(config);
    let (read, write) = tokio::io::split(stream);
    // MCP's official newline-delimited stdio wire format, unchanged end to end.
    // The SDK owns initialize/tool dispatch on the daemon; the bridge owns no state.
    tokio::select! {
        result = forward(input, write) => result,
        result = forward_bounded(read, output, RESPONSE_LIMIT) => result,
    }
}
async fn forward<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(read: R, write: W) -> Result<()> {
    forward_bounded(read, write, FRAME_LIMIT).await
}
async fn forward_bounded<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    read: R,
    mut write: W,
    limit: usize,
) -> Result<()> {
    let mut read = BufReader::with_capacity(limit, read);
    loop {
        let mut first = [0];
        if read.read(&mut first).await? == 0 {
            return Ok(());
        }
        let mut line = vec![first[0]];
        let present = tokio::time::timeout(FRAME_TIMEOUT, async {
            if first[0] == b'\n' {
                return Ok(true);
            }
            loop {
                let mut byte = [0];
                if read.read(&mut byte).await? == 0 {
                    if !line.is_empty() {
                        bail!("incomplete frame");
                    }
                    return Ok(false);
                }
                if line.len() >= limit {
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
    struct Scripted(std::collections::VecDeque<Result<Option<tokio::io::DuplexStream>>>);
    impl Acceptor for Scripted {
        type Stream = tokio::io::DuplexStream;
        async fn accept(&mut self) -> Result<Option<Self::Stream>> {
            match self.0.pop_front() {
                Some(value) => value,
                None => std::future::pending().await,
            }
        }
    }
    #[tokio::test]
    async fn rejected_peer_then_sdk_initialize_and_fatal_listener_error() {
        use sha2::{Digest, Sha256};
        #[cfg(target_os = "macos")]
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        #[cfg(not(target_os = "macos"))]
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        unsafe {
            use std::{os::windows::ffi::OsStrExt, ptr};
            use windows_sys::Win32::{
                Foundation::LocalFree,
                Security::{Authorization::*, *},
            };
            let identity = platform::current_identity().unwrap();
            let text: Vec<u16> = format!("D:P(A;OICI;FA;;;{identity})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut sd = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    1,
                    &mut sd,
                    ptr::null_mut()
                ),
                0
            );
            let (mut present, mut defaulted) = (0, 0);
            let mut acl = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted),
                0
            );
            let path: Vec<u16> = dir
                .path()
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            assert_eq!(
                SetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    acl,
                    ptr::null_mut()
                ),
                0
            );
            LocalFree(sd);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        // The test seam injects the same rejected-peer outcome used by failed
        // kernel credential lookups; subsequent traffic uses the real SDK.
        let token = "a".repeat(64);
        let cfg = Arc::new(crate::config::Config {
            execution: None,
            development_execution: None,
            local: None,
            listen: None,
            state_dir: dir.path().into(),
            agents: vec![crate::config::Agent {
                principal: "fixture".into(),
                token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
            }],
        });
        let activity = Arc::new(crate::store::Activity::open(dir.path()).unwrap());
        let (mut client, server) = tokio::io::duplex(FRAME_LIMIT);
        let (rejected, _other) = tokio::io::duplex(128);
        let rejected = verified_peer(
            rejected,
            Err(std::io::Error::from(std::io::ErrorKind::NotConnected).into()),
        );
        let task = tokio::spawn(run_listener(
            Scripted([Ok(rejected), Ok(Some(server))].into()),
            cfg.clone(),
            activity.clone(),
        ));
        client
            .write_all(format!("ARX1 {token}\n").as_bytes())
            .await
            .unwrap();
        let mut ack = [0; 3];
        tokio::time::timeout(AUTH_TIMEOUT, client.read_exact(&mut ack))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&ack, b"OK\n");
        let (read, write) = tokio::io::split(client);
        let peer = tokio::time::timeout(
            AUTH_TIMEOUT,
            ().serve(AsyncRwTransport::new_client(read, write)),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(peer.list_tools(None).await.is_ok());
        tokio::time::pause();
        tokio::time::advance(FRAME_TIMEOUT * 2).await;
        tokio::task::yield_now().await;
        assert!(
            peer.list_tools(None).await.is_ok(),
            "initialized SDK session must survive idle beyond the old total lease"
        );
        peer.cancel().await.unwrap();
        assert!(!task.is_finished());
        task.abort();
        let _ = task.await;
        let result = tokio::time::timeout(
            AUTH_TIMEOUT,
            run_listener(
                Scripted([Err(anyhow::anyhow!("injected listener failure"))].into()),
                cfg,
                activity,
            ),
        )
        .await
        .unwrap();
        assert!(
            result.is_err(),
            "unrecoverable listener errors must terminate, never spin"
        );
    }
    #[tokio::test]
    async fn response_frame_limit_includes_newline_and_withholds_oversize() {
        for size in [RESPONSE_LIMIT, RESPONSE_LIMIT + 1] {
            let mut payload = vec![b'x'; size];
            payload[size - 1] = b'\n';
            let (mut input, reader) = tokio::io::duplex(RESPONSE_LIMIT * 2);
            let (writer, mut output) = tokio::io::duplex(RESPONSE_LIMIT * 2);
            input.write_all(&payload).await.unwrap();
            drop(input);
            let result = forward_bounded(reader, writer, RESPONSE_LIMIT).await;
            let mut bytes = Vec::new();
            output.read_to_end(&mut bytes).await.unwrap();
            if size == RESPONSE_LIMIT {
                assert!(result.is_ok());
                assert_eq!(bytes, payload);
            } else {
                assert!(result.is_err());
                assert!(bytes.is_empty());
            }
        }
    }
    #[tokio::test(start_paused = true)]
    async fn idle_frames_survive_but_partial_frames_expire() {
        let (mut input, reader) = tokio::io::duplex(128);
        let (writer, mut output) = tokio::io::duplex(128);
        let task = tokio::spawn(forward(reader, writer));
        tokio::task::yield_now().await;
        tokio::time::advance(FRAME_TIMEOUT * 2).await;
        assert!(!task.is_finished());
        input.write_all(b"ok\n").await.unwrap();
        let mut frame = [0; 3];
        output.read_exact(&mut frame).await.unwrap();
        assert_eq!(&frame, b"ok\n");
        input.write_all(b"partial").await.unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(FRAME_TIMEOUT + Duration::from_secs(1)).await;
        assert!(task.await.unwrap().is_err());
    }
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
#[cfg(all(test, windows))]
#[path = "local_execution_tests.rs"]
mod execution_tests;

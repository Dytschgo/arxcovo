//! Authenticated local transport; executor selection stays internal to startup.
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

#[cfg(windows)]
pub(crate) use platform::current_identity;

pub(crate) use platform::owned as require_owner;

pub(crate) fn vm_supervisor_identity() -> Result<String> {
    #[cfg(windows)]
    return current_identity();
    #[cfg(unix)]
    Ok(unsafe { libc::geteuid() }.to_string())
}

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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientFile {
    address: String,
    expected_server_identity: String,
    token: String,
    #[cfg(target_os = "linux")]
    delegated: Option<platform::delegated::Profile>,
}

pub struct LoadedClient {
    ordinary: ClientConfig,
    #[cfg(target_os = "linux")]
    delegated: Option<platform::delegated::Profile>,
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) use platform::delegated::endpoint_controls;
#[cfg(target_os = "linux")]
pub(crate) use platform::delegated::{AgentListener, Profile, group_member};

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
    pub fn load(path: &Path) -> Result<LoadedClient> {
        let file: ClientFile = serde_json::from_slice(&private_file(path)?)
            .map_err(|_| anyhow::anyhow!("invalid client configuration"))?;
        let value = Self {
            address: file.address,
            expected_server_identity: file.expected_server_identity,
            token: file.token,
        };
        #[cfg(target_os = "linux")]
        match &file.delegated {
            Some(profile) => platform::delegated::validate(&value, profile)?,
            None => platform::validate_address(&value.address)?,
        }
        #[cfg(not(target_os = "linux"))]
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
        Ok(LoadedClient {
            ordinary: value,
            #[cfg(target_os = "linux")]
            delegated: file.delegated,
        })
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
pub struct Listener(platform::Listener, Vec<crate::service::native::Observed>);
impl Listener {
    pub fn bind(endpoint: &Endpoint) -> Result<Self> {
        endpoint.validate()?;
        Ok(Self(platform::Listener::bind(endpoint)?, Vec::new()))
    }
    pub(crate) fn bind_service(
        layout: &crate::service::layout::Layout,
        client: String,
    ) -> Result<Self> {
        crate::service::native::require_service_identity()?;
        let endpoint = Endpoint {
            address: layout
                .endpoint()
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("endpoint"))?
                .into(),
            client_identity: client,
        };
        platform::validate_identity(&endpoint.client_identity)?;
        #[cfg(unix)]
        let pins = vec![crate::service::native::endpoint_guard(layout)?];
        #[cfg(windows)]
        let pins = Vec::new();
        Ok(Self(platform::Listener::bind_service(&endpoint)?, pins))
    }
    pub(crate) async fn run_service(
        self,
        config: Arc<crate::config::Config>,
        activity: Arc<crate::store::Activity>,
        stop: crate::service::Shutdown,
    ) -> Result<()> {
        let _pins = self.1;
        run_listener_until(self.0, config, activity, stop).await
    }
    pub async fn run(
        self,
        config: Arc<crate::config::Config>,
        activity: Arc<crate::store::Activity>,
    ) -> Result<()> {
        run_listener(self.0, config, activity).await
    }
    /// Stop accepting connections and join aborted connection tasks. Detached
    /// SDK request handlers are fenced by execution admission, not this join.
    pub async fn run_until(
        self,
        config: Arc<crate::config::Config>,
        activity: Arc<crate::store::Activity>,
        stop: crate::service::Shutdown,
    ) -> Result<()> {
        run_listener_until(self.0, config, activity, stop).await
    }
}

/// The foreground candidate has its own store, listener and shutdown path.
pub async fn run_vm_profile(profile: crate::config::VmForeground) -> Result<()> {
    use crate::mcp::vm::Supervisor;
    let crate::config::VmForeground { config, profile } = profile;
    let endpoint = config.local.as_ref().context("server-config")?;
    let supervisor = Supervisor::open(&config.state_dir, &profile).context("server-state")?;
    supervisor.sweep().context("server-state")?;
    let listener = Listener::bind(endpoint).context("server-bind")?;
    let stop = crate::service::Shutdown::default();
    let run = run_vm_listener(listener, Arc::new(config), supervisor.clone(), stop.clone());
    tokio::pin!(run);
    let (result, listener_done) = tokio::select! {
        result = &mut run => (result.context("server-listener"), true),
        result = supervisor.tick() => (result.context("server-state"), false),
        result = tokio::signal::ctrl_c() => (result.context("server-listener"), false),
    };
    supervisor.close();
    stop.cancel();
    let listener_drain = if listener_done {
        Ok(())
    } else {
        tokio::time::timeout(Duration::from_secs(5), run)
            .await
            .context("server-drain-unconfirmed")
            .and_then(|result| result.context("server-listener"))
    };
    supervisor
        .drain()
        .await
        .context("server-drain-unconfirmed")?;
    result.and(listener_drain)
}

pub(crate) async fn run_vm_listener(
    listener: Listener,
    config: Arc<crate::config::Config>,
    supervisor: Arc<crate::mcp::vm::Supervisor>,
    stop: crate::service::Shutdown,
) -> Result<()> {
    run_listener_with(
        listener.0,
        config,
        None,
        stop,
        ConnectionProfile {
            #[cfg(target_os = "linux")]
            build: None,
            vm: Some(supervisor),
        },
    )
    .await
}
async fn run_listener(
    listener: impl Acceptor,
    config: Arc<crate::config::Config>,
    activity: Arc<crate::store::Activity>,
) -> Result<()> {
    run_listener_until(
        listener,
        config,
        activity,
        crate::service::Shutdown::default(),
    )
    .await
}
#[derive(Clone, Default)]
struct ConnectionProfile {
    #[cfg(target_os = "linux")]
    build: Option<Arc<crate::service::cluster_build::BuildService>>,
    vm: Option<Arc<crate::mcp::vm::Supervisor>>,
}

async fn run_listener_until(
    listener: impl Acceptor,
    config: Arc<crate::config::Config>,
    activity: Arc<crate::store::Activity>,
    stop: crate::service::Shutdown,
) -> Result<()> {
    run_listener_with(
        listener,
        config,
        Some(activity),
        stop,
        ConnectionProfile::default(),
    )
    .await
}

#[cfg(target_os = "linux")]
pub(crate) async fn run_delegated(
    listener: AgentListener,
    config: Arc<crate::config::Config>,
    activity: Arc<crate::store::Activity>,
    stop: crate::service::Shutdown,
    build: Arc<crate::service::cluster_build::BuildService>,
) -> Result<()> {
    run_listener_with(
        listener,
        config,
        Some(activity),
        stop,
        ConnectionProfile {
            build: Some(build),
            vm: None,
        },
    )
    .await
}

async fn run_listener_with(
    mut listener: impl Acceptor,
    config: Arc<crate::config::Config>,
    activity: Option<Arc<crate::store::Activity>>,
    stop: crate::service::Shutdown,
    profile: ConnectionProfile,
) -> Result<()> {
    let capacity = Arc::new(tokio::sync::Semaphore::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    let mut reported_rejection = false;
    let result = loop {
        while tasks.try_join_next().is_some() {}
        tokio::select! {
            biased;
            _ = stop.cancelled() => break Ok(()),
            stream = listener.accept() => {
                let stream = match stream {
                    Ok(stream) => stream,
                    Err(error) => {
                        if let Some(vm) = &profile.vm {
                            vm.close();
                        }
                        break Err(error);
                    }
                };
                let Some(stream) = stream else {
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
                let profile = profile.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = connection(stream, config, activity, profile).await;
                });
            }
            _ = tasks.join_next(), if !tasks.is_empty() => {}
        }
    };
    drop(listener);
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    result
    // ServiceApplication cancels and joins dedicated workers after connections stop.
}
#[cfg(target_os = "linux")]
struct CloseDelegated(crate::service::Shutdown);

#[cfg(target_os = "linux")]
impl Drop for CloseDelegated {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

async fn connection<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    mut stream: S,
    config: Arc<crate::config::Config>,
    activity: Option<Arc<crate::store::Activity>>,
    profile: ConnectionProfile,
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
    let vm = profile
        .vm
        .as_ref()
        .map(|vm| vm.connection(principal.clone()));
    // Bound SDK requests at 8 KiB and response frames at 64 KiB. Idle authenticated
    // sessions may retain one of eight slots; partial frames and writes time out.
    let (sdk, pump) = tokio::io::duplex(FRAME_LIMIT);
    let (sdk_read, sdk_write) = tokio::io::split(sdk);
    let (pump_read, pump_write) = tokio::io::split(pump);
    #[cfg(target_os = "linux")]
    let close = profile
        .build
        .as_ref()
        .map(|_| CloseDelegated(crate::service::Shutdown::default()));
    let protocol = async {
        let transport =
            AsyncRwTransport::new_server(sdk_read, sdk_write).with_max_line_length(FRAME_LIMIT);
        if let Some(connection) = &vm {
            let service =
                tokio::time::timeout(AUTH_TIMEOUT, connection.inventory().serve(transport))
                    .await??;
            service.waiting().await?;
            return Ok::<_, anyhow::Error>(());
        }
        let handler = crate::mcp::Inventory {
            execution: config.execution.clone(),
            activity: activity.context("missing inventory")?,
            principal: Some(crate::mcp::Principal(principal)),
        };
        #[cfg(target_os = "linux")]
        if let (Some(build), Some(close)) = (profile.build, &close) {
            let service = tokio::time::timeout(
                AUTH_TIMEOUT,
                crate::mcp::DelegatedInventory {
                    local: handler,
                    build,
                    closed: close.0.clone(),
                }
                .serve(transport),
            )
            .await??;
            service.waiting().await?;
            return Ok::<_, anyhow::Error>(());
        }
        let _ = profile;
        let service = tokio::time::timeout(AUTH_TIMEOUT, handler.serve(transport)).await??;
        service.waiting().await?;
        Ok::<_, anyhow::Error>(())
    };
    tokio::select! {
        result = protocol => result?,
        result = forward_checked(read, pump_write, FRAME_LIMIT, vm.is_some()) => result?,
        result = forward_bounded(pump_read, write, RESPONSE_LIMIT) => result?,
    }
    Ok(())
}

/// No credential bytes are written until connect() has verified kernel identity.
pub async fn bridge(config: LoadedClient) -> Result<()> {
    #[cfg(target_os = "linux")]
    if let Some(profile) = config.delegated {
        let stream = tokio::time::timeout(
            AUTH_TIMEOUT,
            platform::delegated::connect(&config.ordinary, &profile),
        )
        .await
        .context("peer-connect-timeout")?
        .context("peer-connect-or-identity")?;
        return bridge_connected(
            stream,
            config.ordinary,
            tokio::io::stdin(),
            tokio::io::stdout(),
        )
        .await;
    }
    bridge_io(config.ordinary, tokio::io::stdin(), tokio::io::stdout()).await
}
/// The stdio bridge over any byte streams; tests drive it with in-memory pipes.
async fn bridge_io<I: AsyncRead + Unpin, O: AsyncWrite + Unpin>(
    config: ClientConfig,
    input: I,
    output: O,
) -> Result<()> {
    let stream = tokio::time::timeout(AUTH_TIMEOUT, platform::connect(&config))
        .await
        .context("peer-connect-timeout")?
        .context("peer-connect-or-identity")?;
    bridge_connected(stream, config, input, output).await
}

async fn bridge_connected(
    mut stream: impl AsyncRead + AsyncWrite + Unpin,
    config: ClientConfig,
    input: impl AsyncRead + Unpin,
    output: impl AsyncWrite + Unpin,
) -> Result<()> {
    authenticate_bridge(&mut stream, &config.token).await?;
    drop(config);
    let (read, write) = tokio::io::split(stream);
    // MCP's official newline-delimited stdio wire format, unchanged end to end.
    // The SDK owns initialize/tool dispatch on the daemon; the bridge owns no state.
    tokio::select! {
        result = forward(input, write) => result,
        result = forward_bounded(read, output, RESPONSE_LIMIT) => result,
    }
}

async fn authenticate_bridge(
    stream: &mut (impl AsyncRead + AsyncWrite + Unpin),
    token: &str,
) -> Result<()> {
    tokio::time::timeout(AUTH_TIMEOUT, async {
        stream.write_all(b"ARX1 ").await?;
        stream.write_all(token.as_bytes()).await?;
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
    .context("peer-auth-rejected")
}

#[cfg(test)]
pub(crate) async fn connect_vm_fixture(
    address: &str,
    identity: &str,
    token: &str,
) -> Result<impl AsyncRead + AsyncWrite + Unpin + Send + use<>> {
    let config = ClientConfig {
        address: address.to_owned(),
        expected_server_identity: identity.to_owned(),
        token: token.to_owned(),
    };
    let mut stream = tokio::time::timeout(AUTH_TIMEOUT, platform::connect(&config))
        .await
        .context("peer-connect-timeout")?
        .context("peer-connect-or-identity")?;
    authenticate_bridge(&mut stream, &config.token).await?;
    Ok(stream)
}
async fn forward<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(read: R, write: W) -> Result<()> {
    forward_bounded(read, write, FRAME_LIMIT).await
}
async fn forward_bounded<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    read: R,
    write: W,
    limit: usize,
) -> Result<()> {
    forward_checked(read, write, limit, false).await
}

async fn forward_checked<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    read: R,
    mut write: W,
    limit: usize,
    vm: bool,
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
        if vm {
            let value = check_json(&line)?;
            if let Some(arguments) = value.pointer("/params/arguments") {
                anyhow::ensure!(arguments.is_object(), "invalid tool envelope");
            }
        }
        tokio::time::timeout(AUTH_TIMEOUT, async {
            write.write_all(&line).await?;
            write.flush().await
        })
        .await??;
    }
}

/// Reject duplicate (including escape-equivalent) object keys before rmcp
/// materializes a map. Limits apply to arrays and objects alike.
pub(crate) fn check_json(bytes: &[u8]) -> Result<serde_json::Value> {
    use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
    use serde_json::{Map, Value};

    struct Bounded(u8);
    impl<'de> DeserializeSeed<'de> for Bounded {
        type Value = Value;
        fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> Result<Value, D::Error> {
            if self.0 >= 16 {
                return Err(D::Error::custom("JSON nesting limit"));
            }
            de.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for Bounded {
        type Value = Value;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("bounded JSON")
        }
        fn visit_unit<E: Error>(self) -> Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_bool<E: Error>(self, v: bool) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_i64<E: Error>(self, v: i64) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_u64<E: Error>(self, v: u64) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_f64<E: Error>(self, v: f64) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_str<E: Error>(self, v: &str) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element_seed(Bounded(self.0 + 1))? {
                values.push(value);
            }
            Ok(Value::Array(values))
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
            let mut values = Map::new();
            while let Some(key) = map.next_key::<String>()? {
                if values.contains_key(&key) {
                    return Err(A::Error::custom("duplicate JSON key"));
                }
                values.insert(key, map.next_value_seed(Bounded(self.0 + 1))?);
            }
            Ok(Value::Object(values))
        }
    }
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let result = Bounded(0).deserialize(&mut de)?;
    de.end()?;
    Ok(result)
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
            execution_settings: None,
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

#[cfg(test)]
pub(crate) async fn fixture_connect(endpoint: Endpoint) -> impl AsyncRead + AsyncWrite + Unpin {
    #[cfg(windows)]
    let identity = platform::current_identity().unwrap();
    #[cfg(unix)]
    let identity = unsafe { libc::geteuid() }.to_string();
    platform::connect(&ClientConfig {
        address: endpoint.address,
        expected_server_identity: identity,
        token: "a".repeat(64),
    })
    .await
    .unwrap()
}

#[cfg(test)]
mod service_bridge_tests {
    #[tokio::test]
    async fn fixed_service_endpoint_requires_privileged_server_identity_before_connect() {
        #[cfg(windows)]
        let identity = "S-1-5-32-544";
        #[cfg(unix)]
        let identity = "1001";
        let config = super::ClientConfig {
            address: crate::service::layout::Layout::system()
                .unwrap()
                .endpoint()
                .to_str()
                .unwrap()
                .into(),
            expected_server_identity: identity.into(),
            token: "a".repeat(64),
        };
        // The fixed category is returned before touching a real pipe/socket.
        let error = super::platform::connect(&config).await.err().unwrap();
        assert_eq!(error.to_string(), "service identity required");
    }
}

use super::{AUTH_TIMEOUT, ClientConfig, Endpoint, LoadedClient, bridge_connected};
use anyhow::{Result, bail};
use std::{
    ffi::c_void,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::Path,
    ptr,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{Pipes::GetNamedPipeServerProcessId, Threading::*},
};

pub fn validate_identity(value: &str) -> Result<()> {
    if value.len() > 184
        || !value.starts_with("S-1-")
        || !value
            .bytes()
            .all(|c| c.is_ascii_digit() || c == b'S' || c == b'-')
    {
        bail!("invalid SID");
    }
    unsafe {
        let mut sid = ptr::null_mut();
        let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        if ConvertStringSidToSidW(wide.as_ptr(), &mut sid) == 0 {
            bail!("invalid SID");
        }
        LocalFree(sid);
    }
    Ok(())
}
pub fn validate_address(value: &str) -> Result<()> {
    let Some(name) = value.strip_prefix(r"\\.\pipe\arxcovo-") else {
        bail!("invalid endpoint");
    };
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        bail!("invalid endpoint");
    }
    Ok(())
}
unsafe fn sid_text(sid: *mut c_void) -> Result<String> {
    unsafe {
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            bail!("identity unavailable");
        }
        let mut len = 0;
        while *text.add(len) != 0 && len < 184 {
            len += 1;
        }
        let result = String::from_utf16(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(result?)
    }
}
pub fn current_identity() -> Result<String> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            bail!("identity unavailable");
        }
        let mut size = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
        let mut data = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        let ok = GetTokenInformation(token, TokenUser, data.as_mut_ptr().cast(), size, &mut size);
        CloseHandle(token);
        if ok == 0 {
            bail!("identity unavailable");
        }
        sid_text((*data.as_ptr().cast::<TOKEN_USER>()).User.Sid)
    }
}
pub fn owned(path: &Path) -> Result<()> {
    unsafe {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut owner = ptr::null_mut();
        let mut sd = ptr::null_mut();
        if GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut sd,
        ) != 0
        {
            bail!("owner unavailable");
        }
        let actual = sid_text(owner);
        LocalFree(sd);
        if actual? != current_identity()? {
            bail!("owner mismatch");
        }
        Ok(())
    }
}
fn create(endpoint: &Endpoint, first: bool) -> Result<NamedPipeServer> {
    let owner = current_identity()?;
    // Explicit client rights omit FILE_CREATE_PIPE_INSTANCE (same bit as append).
    // NETWORK deny is independent of PIPE_REJECT_REMOTE_CLIENTS.
    let sddl = format!(
        "O:{owner}D:P(D;;GA;;;NU)(A;;GA;;;{owner})(A;;0x12019b;;;{})",
        endpoint.client_identity
    );
    unsafe {
        let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut sd = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut sd,
            ptr::null_mut(),
        ) == 0
        {
            bail!("endpoint security unavailable");
        }
        let mut attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(16)
            .in_buffer_size(8192)
            .out_buffer_size(8192)
            .create_with_security_attributes_raw(
                &endpoint.address,
                (&mut attrs as *mut SECURITY_ATTRIBUTES).cast(),
            );
        LocalFree(sd);
        Ok(result?)
    }
}
pub struct Listener {
    next: NamedPipeServer,
    endpoint: Endpoint,
}
impl Listener {
    pub fn bind_service(endpoint: &Endpoint) -> Result<Self> {
        crate::service::native::require_service_identity()?;
        Self::bind(endpoint)
    }
    pub fn bind(endpoint: &Endpoint) -> Result<Self> {
        Ok(Self {
            next: create(endpoint, true)?,
            endpoint: endpoint.clone(),
        })
    }
}
impl super::Acceptor for Listener {
    type Stream = NamedPipeServer;
    async fn accept(&mut self) -> Result<Option<NamedPipeServer>> {
        if let Err(error) = self.next.connect().await {
            if !matches!(error.raw_os_error(), Some(109 | 232 | 233)) {
                return Err(error.into());
            }
            // Reuse the reserved instance after a disconnected client. Failure
            // to reset it is a listener failure, never a tight retry loop.
            if let Err(reset) = self.next.disconnect()
                && !matches!(reset.raw_os_error(), Some(109 | 232 | 233))
            {
                return Err(reset.into());
            }
            return Ok(None);
        }
        // Keep the existing instance reserved while replenishing. Retry transient
        // creation failures at most three times; persistent failure is fatal.
        for attempt in 0..3 {
            match create(&self.endpoint, false) {
                Ok(next) => {
                    return Ok(super::verified_peer(
                        std::mem::replace(&mut self.next, next),
                        Ok(true),
                    ));
                }
                Err(error) if attempt == 2 => return Err(error),
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
            }
        }
        unreachable!()
    }
}

fn validate_client_endpoint(address: &str, expected_identity: &str) -> Result<()> {
    validate_address(address)?;
    if address.eq_ignore_ascii_case(r"\\.\pipe\arxcovo-arxd") && expected_identity != "S-1-5-18" {
        bail!("service identity required");
    }
    Ok(())
}

pub async fn connect(config: &ClientConfig) -> Result<NamedPipeClient> {
    validate_client_endpoint(&config.address, &config.expected_server_identity)?;
    unsafe {
        let wide: Vec<u16> = config.address.encode_utf16().chain(Some(0)).collect();
        // Specific rights match the DACL, and anonymous SQOS prevents impersonation.
        let handle = loop {
            let handle = CreateFileW(
                wide.as_ptr(),
                0x12019b,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
                ptr::null_mut(),
            );
            if handle != INVALID_HANDLE_VALUE {
                break handle;
            }
            if GetLastError() != ERROR_PIPE_BUSY {
                bail!("endpoint unavailable");
            }
            // Caller bounds the whole connect/identity stage to AUTH_TIMEOUT.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        let stream = NamedPipeClient::from_raw_handle(handle.cast())?;
        let mut owner = ptr::null_mut();
        let mut sd = ptr::null_mut();
        let status = GetSecurityInfo(
            stream.as_raw_handle(),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut sd,
        );
        if status != 0 {
            bail!("endpoint identity unavailable");
        }
        let actual = sid_text(owner);
        LocalFree(sd);
        if actual? != config.expected_server_identity {
            bail!("endpoint identity mismatch");
        }
        Ok(stream)
    }
}

#[derive(Debug)]
pub enum ForegroundBridgeError {
    EndpointMismatch,
    ConnectTimeout,
    ConnectRejected,
    ProcessUnavailable,
    ProcessMismatch,
}

/// The owner-checked connection whose kernel server PID matched the foreground.
/// Consuming it for the bridge keeps the verified handle through auth and MCP.
pub struct ForegroundBridge {
    pipe: NamedPipeClient,
    config: ClientConfig,
}

impl ForegroundBridge {
    pub async fn forward(self, io: impl AsyncRead + AsyncWrite + Unpin) -> Result<()> {
        let (input, output) = tokio::io::split(io);
        bridge_connected(self.pipe, self.config, input, output).await
    }
}

/// Open one owner-checked connection and bind it to the spawned foreground.
/// The caller must retain its child process handle for the whole session.
pub async fn bind_foreground_bridge(
    config: LoadedClient,
    expected_address: &str,
    expected_pid: u32,
) -> Result<ForegroundBridge, ForegroundBridgeError> {
    bind_foreground_pipe(
        config.ordinary,
        expected_address,
        expected_pid,
        kernel_server_pid,
    )
    .await
}

fn kernel_server_pid(pipe: &NamedPipeClient) -> Option<u32> {
    let mut pid = 0;
    // The client owns this live pipe handle and the output is one writable DWORD.
    (unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } != 0).then_some(pid)
}

async fn bind_foreground_pipe(
    config: ClientConfig,
    expected_address: &str,
    expected_pid: u32,
    server_pid: impl FnOnce(&NamedPipeClient) -> Option<u32>,
) -> Result<ForegroundBridge, ForegroundBridgeError> {
    use ForegroundBridgeError::*;

    if config.address != expected_address {
        return Err(EndpointMismatch);
    }
    let pipe = tokio::time::timeout(AUTH_TIMEOUT, connect(&config))
        .await
        .map_err(|_| ConnectTimeout)?
        .map_err(|_| ConnectRejected)?;
    let peer = server_pid(&pipe)
        .filter(|pid| *pid != 0)
        .ok_or(ProcessUnavailable)?;
    if peer != expected_pid {
        return Err(ProcessMismatch);
    }
    Ok(ForegroundBridge { pipe, config })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        time::timeout,
    };

    const FIXTURE_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn pipe_fixture() -> (ClientConfig, Endpoint, NamedPipeServer) {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).unwrap();
        let endpoint = Endpoint {
            address: format!(r"\\.\pipe\arxcovo-m4-test-{}", hex::encode(nonce)),
            client_identity: current_identity().unwrap(),
        };
        let server = create(&endpoint, true).unwrap();
        let config = ClientConfig {
            address: endpoint.address.clone(),
            expected_server_identity: endpoint.client_identity.clone(),
            token: FIXTURE_TOKEN.into(),
        };
        (config, endpoint, server)
    }

    async fn rejected_pipe(
        expected: u32,
        query: impl FnOnce(&NamedPipeClient) -> Option<u32>,
        verify: impl FnOnce(ForegroundBridgeError),
    ) {
        let (config, endpoint, mut server) = pipe_fixture();
        // Drive the credential-writing entry even if a mutated gate lets this
        // peer through. Any byte on the server side is a test failure.
        let client = async {
            match bind_foreground_pipe(config, &endpoint.address, expected, query).await {
                Err(error) => verify(error),
                Ok(bound) => {
                    let (_sdk, io) = tokio::io::duplex(256);
                    let _ = bound.forward(io).await;
                    panic!("accepted an unverified foreground pipe");
                }
            }
        };
        let no_auth = async {
            server.connect().await.unwrap();
            let mut byte = [0; 1];
            assert_eq!(server.read(&mut byte).await.unwrap(), 0, "auth byte sent");
        };
        timeout(AUTH_TIMEOUT, async { tokio::join!(no_auth, client) })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn foreground_pipe_missing_pid_sends_no_credentials() {
        for pid in [None, Some(0)] {
            rejected_pipe(
                std::process::id(),
                |pipe| {
                    assert_eq!(kernel_server_pid(pipe), Some(std::process::id()));
                    pid
                },
                |error| assert!(matches!(error, ForegroundBridgeError::ProcessUnavailable)),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn foreground_pipe_wrong_pid_sends_no_credentials() {
        rejected_pipe(std::process::id() ^ 1, kernel_server_pid, |error| {
            assert!(matches!(error, ForegroundBridgeError::ProcessMismatch));
        })
        .await;
    }

    #[tokio::test]
    async fn foreground_pipe_verified_instance_carries_auth_and_mcp() {
        const REQUEST: &[u8] = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
        const RESPONSE: &[u8] = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
        let (config, endpoint, mut server) = pipe_fixture();
        let bound = bind_foreground_bridge(
            LoadedClient { ordinary: config },
            &endpoint.address,
            std::process::id(),
        )
        .await
        .unwrap();
        timeout(AUTH_TIMEOUT, server.connect())
            .await
            .unwrap()
            .unwrap();
        // A fresh connection could open this other instance with the same
        // owner and PID. Only the verified instance responds to auth and MCP.
        let _standby = create(&endpoint, false).unwrap();
        let (mut sdk, io) = tokio::io::duplex(256);
        let client = async { bound.forward(io).await.unwrap() };
        let receive = async {
            let mut auth = [0; 70];
            server.read_exact(&mut auth).await.unwrap();
            assert_eq!(&auth[..5], b"ARX1 ");
            assert!(auth[5..69] == *FIXTURE_TOKEN.as_bytes());
            assert_eq!(auth[69], b'\n');
            server.write_all(b"OK\n").await.unwrap();
            let mut received = [0; REQUEST.len()];
            server.read_exact(&mut received).await.unwrap();
            assert_eq!(&received, REQUEST);
            server.write_all(RESPONSE).await.unwrap();
        };
        let round_trip = async move {
            sdk.write_all(REQUEST).await.unwrap();
            let mut received = [0; RESPONSE.len()];
            sdk.read_exact(&mut received).await.unwrap();
            assert_eq!(&received, RESPONSE);
        };
        timeout(AUTH_TIMEOUT, async {
            tokio::join!(receive, client, round_trip);
        })
        .await
        .unwrap();
    }

    #[test]
    fn direct_client_endpoint_policy_refuses_nonlocal_or_invalid_addresses() {
        for address in [
            "",
            r"\\remote\pipe\arxcovo-arxd",
            r"\\.\PIPE\arxcovo-arxd",
            r"\\.\pipe\other-service",
            r"\\.\pipe\arxcovo-",
            r"\\.\pipe\arxcovo-arxd\extra",
        ] {
            assert!(validate_client_endpoint(address, "S-1-5-18").is_err());
        }
    }

    #[test]
    fn service_pipe_casing_requires_system_without_pipe_io() {
        for address in [
            r"\\.\pipe\arxcovo-arxd",
            r"\\.\pipe\arxcovo-ARXD",
            r"\\.\pipe\arxcovo-ArXd",
        ] {
            validate_address(address).unwrap();
            assert!(validate_client_endpoint(address, "S-1-5-21-100-200-300-1001").is_err());
            assert!(validate_client_endpoint(address, "S-1-5-18").is_ok());
        }
        for address in [
            r"\\.\pipe\arxcovo-arxd-extra",
            r"\\.\pipe\arxcovo-development",
        ] {
            validate_address(address).unwrap();
            assert!(validate_client_endpoint(address, "S-1-5-21-100-200-300-1001").is_ok());
        }
    }
}

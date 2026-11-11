use super::{ClientConfig, Endpoint};
use anyhow::{Result, bail};
use std::{
    ffi::c_void,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::Path,
    ptr,
};
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::Threading::*,
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

pub async fn connect(config: &ClientConfig) -> Result<NamedPipeClient> {
    if config.address == r"\\.\pipe\arxcovo-arxd" && config.expected_server_identity != "S-1-5-18" {
        bail!("service identity required");
    }
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

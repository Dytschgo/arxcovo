use arxd::config::ForegroundConfig;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, async_rw::AsyncRwTransport,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::process::{Child, Command};

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_arxd"));
    command.kill_on_drop(true);
    command
}

fn save(path: &Path, body: impl AsRef<[u8]>) {
    #[cfg(unix)]
    {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap()
            .write_all(body.as_ref())
            .unwrap();
    }
    #[cfg(windows)]
    std::fs::write(path, body).unwrap();
}

#[cfg(unix)]
fn private_temp() -> (tempfile::TempDir, String, String) {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().canonicalize().unwrap();
    let temp = tempfile::tempdir_in(root).unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let uid = unsafe { libc::geteuid() };
    (temp, uid.to_string(), (uid ^ 1).to_string())
}

#[cfg(windows)]
fn private_temp() -> (tempfile::TempDir, String, String) {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let temp = tempfile::tempdir().unwrap();
    let sid = unsafe {
        let mut handle = ptr::null_mut();
        assert_ne!(
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle),
            0
        );
        let mut len = 0;
        GetTokenInformation(handle, TokenUser, ptr::null_mut(), 0, &mut len);
        let mut buffer = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
        assert_ne!(
            GetTokenInformation(handle, TokenUser, buffer.as_mut_ptr().cast(), len, &mut len),
            0
        );
        CloseHandle(handle);
        let owner = (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let mut owner_text = ptr::null_mut();
        assert_ne!(ConvertSidToStringSidW(owner, &mut owner_text), 0);
        let len = (0..184).find(|i| *owner_text.add(*i) == 0).unwrap();
        let sid = String::from_utf16(std::slice::from_raw_parts(owner_text, len)).unwrap();
        LocalFree(owner_text.cast());
        let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor: *mut c_void = ptr::null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut()
            ),
            0
        );
        let (mut present, mut defaulted) = (0, 0);
        let mut acl = ptr::null_mut();
        assert_ne!(
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
            0
        );
        let path: Vec<u16> = temp
            .path()
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        assert_eq!(
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION
                    | OWNER_SECURITY_INFORMATION,
                owner,
                ptr::null_mut(),
                acl,
                ptr::null_mut()
            ),
            0
        );
        LocalFree(descriptor);
        sid
    };
    let alternate = if sid == "S-1-5-21-11-22-33-1001" {
        "S-1-5-21-11-22-33-1002"
    } else {
        "S-1-5-21-11-22-33-1001"
    };
    (temp, sid, alternate.into())
}

fn endpoint(temp: &Path) -> String {
    #[cfg(unix)]
    return temp.join("mcp.sock").to_str().unwrap().into();
    #[cfg(windows)]
    format!(
        r"\\.\pipe\arxcovo-test-{}",
        temp.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .trim_start_matches('.')
    )
}

fn endpoint_ready(address: &str) -> bool {
    #[cfg(unix)]
    return Path::new(address).exists();
    #[cfg(windows)]
    {
        let wide: Vec<u16> = address.encode_utf16().chain(Some(0)).collect();
        unsafe { windows_sys::Win32::System::Pipes::WaitNamedPipeW(wide.as_ptr(), 1) != 0 }
    }
}

async fn foreground(config: &Path, address: &str) -> Child {
    let mut child = binary()
        .arg("--config")
        .arg(config)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        while !endpoint_ready(address) {
            assert!(
                child.try_wait().unwrap().is_none(),
                "foreground exited before binding"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    child
}

async fn bridge(path: &Path) -> (Child, rmcp::service::RunningService<rmcp::RoleClient, ()>) {
    let mut bridge = binary()
        .arg("mcp-stdio")
        .arg("--client-config")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let transport =
        AsyncRwTransport::new_client(bridge.stdout.take().unwrap(), bridge.stdin.take().unwrap());
    let client = tokio::time::timeout(Duration::from_secs(4), ().serve(transport))
        .await
        .unwrap()
        .unwrap();
    (bridge, client)
}

#[cfg(windows)]
async fn inspect_vm(client: &rmcp::service::RunningService<rmcp::RoleClient, ()>, reason: &str) {
    let tools = client.list_tools(None).await.unwrap();
    let names = tools
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "list_hosts",
            "list_activity",
            "vm_catalog",
            "vm_list",
            "vm_template_build",
            "vm_create",
            "vm_get",
            "vm_operation",
            "vm_start",
            "vm_stop",
            "vm_run_script",
            "vm_extend"
        ]
    );
    let response = client
        .call_tool(CallToolRequestParams::new("vm_catalog"))
        .await
        .unwrap();
    let result = response.structured_content.unwrap();
    assert_eq!(
        result["capability"],
        json!({"available":false,"reason":reason})
    );
    assert_eq!(result["entries"][0]["state"], "unqualified");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let args = json!({"recipe_id":result["entries"][0]["recipe_id"],"request_id":format!("{now:016x}{:016x}", 1)});
    let response = client
        .call_tool(
            CallToolRequestParams::new("vm_template_build")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(response.is_error, Some(true));
    assert_eq!(
        response.structured_content.unwrap(),
        json!({"ok":false,"reason":reason})
    );
}

#[tokio::test]
async fn foreground_profile_boundaries() {
    let (temp, owner, client_identity) = private_temp();
    let root = temp.path();
    let address = endpoint(root);
    let token = "a".repeat(64);
    let config_path = root.join("server.json");
    let client_path = root.join("client.json");
    save(
        &client_path,
        json!({"address":address,"expected_server_identity":owner,"token":token}).to_string(),
    );
    let asset = root.join("asset");
    save(&asset, b"fixture");
    let asset = json!({"path":asset,"length":7,"sha256":"a".repeat(64)});
    let recipes = json!([{"iso":asset,"kernel":asset,"initrd":asset,"guest":asset,"firmware_code":asset,
        "firmware_vars":asset,"qemu_receipt":asset}]);
    let agents = json!([{"principal":"fixture","token_sha256":hex::encode(Sha256::digest(token.as_bytes()))}]);

    let rejected_state = root.join("rejected");
    std::fs::create_dir(&rejected_state).unwrap();
    let good = json!({"state_dir":rejected_state,"agents":agents,"local":{"address":address,
        "client_identity":client_identity},"test_vms":{"enabled":false,"recipes":recipes}});
    save(&config_path, good.to_string());
    assert!(matches!(
        ForegroundConfig::load(&config_path).unwrap(),
        ForegroundConfig::TestVms(_)
    ));
    let mut candidates = vec![];
    for (field, value) in [
        ("listen", json!("127.0.0.1:0")),
        ("execution", Value::Null),
        ("development_execution", Value::Null),
        ("test_vms", Value::Null),
    ] {
        let mut config = good.clone();
        config[field] = value;
        candidates.push(config.to_string());
    }
    let mut same = good.clone();
    same["local"]["client_identity"] = json!(owner);
    candidates.push(same.to_string());
    let mut reserved = good.clone();
    let service = arxd::service::layout::Layout::system().unwrap();
    let service = service.endpoint().to_str().unwrap();
    reserved["local"]["address"] = if cfg!(windows) {
        json!(service.to_ascii_uppercase())
    } else {
        json!(service)
    };
    candidates.push(reserved.to_string());
    candidates.push(format!("{{\"test_vms\":null,{}", &good.to_string()[1..]));
    for invalid in candidates {
        save(&config_path, invalid);
        assert!(ForegroundConfig::load(&config_path).is_err());
    }
    let exit = binary()
        .arg("--config")
        .arg(&config_path)
        .output()
        .await
        .unwrap();
    assert_eq!(exit.status.code(), Some(3));
    assert_eq!(exit.stderr, b"arxd: server-config\n");
    assert_eq!(std::fs::read_dir(&rejected_state).unwrap().count(), 0);
    assert!(!endpoint_ready(&address));

    for (label, enabled) in [
        ("legacy", None),
        ("disabled", Some(false)),
        ("unqualified", Some(true)),
    ] {
        let state = root.join(label);
        std::fs::create_dir(&state).unwrap();
        let mut config = json!({"state_dir":state,"agents":agents,"local":{"address":address,"client_identity":owner}});
        if let Some(enabled) = enabled {
            config["test_vms"] = json!({"enabled":enabled,"recipes":recipes});
            config["local"]["client_identity"] = json!(client_identity);
        }
        // Each process gets its own socket on Unix, where deliberate hard
        // termination leaves the socket pathname behind.
        #[cfg(unix)]
        let address = root
            .join(format!("{label}.sock"))
            .to_str()
            .unwrap()
            .to_owned();
        #[cfg(windows)]
        let address = address.clone();
        config["local"]["address"] = json!(address);
        save(&config_path, config.to_string());
        save(
            &client_path,
            json!({"address":address,"expected_server_identity":owner,"token":token}).to_string(),
        );
        let mut server = foreground(&config_path, &address).await;
        match enabled {
            None => {
                let (mut process, client) = bridge(&client_path).await;
                let tools = client.list_tools(None).await.unwrap();
                assert!(tools.tools.iter().all(|tool| !tool.name.starts_with("vm_")));
                client.cancel().await.unwrap();
                let _ = process.kill().await;
            }
            #[cfg(windows)]
            Some(enabled) => {
                let (mut process, client) = bridge(&client_path).await;
                inspect_vm(
                    &client,
                    if !enabled {
                        "disabled"
                    } else if cfg!(target_arch = "x86_64") {
                        "qualification_missing"
                    } else {
                        "unsupported_platform"
                    },
                )
                .await;
                client.cancel().await.unwrap();
                let _ = process.kill().await;
            }
            #[cfg(unix)]
            Some(_) => {
                let result = binary()
                    .arg("mcp-stdio")
                    .arg("--client-config")
                    .arg(&client_path)
                    .stdin(Stdio::null())
                    .output()
                    .await
                    .unwrap();
                assert!(
                    !result.status.success(),
                    "strict UID fence admitted the supervisor as a client"
                );
            }
        }
        server.kill().await.unwrap();
        assert_eq!(
            std::fs::read_dir(&state).unwrap().count(),
            2,
            "candidate created non-state artifacts"
        );
    }

    let state = root.join("http");
    std::fs::create_dir(&state).unwrap();
    let config = std::sync::Arc::new(arxd::config::Config {
        listen: Some("127.0.0.1:0".parse().unwrap()),
        state_dir: state.clone(),
        agents: serde_json::from_value(agents).unwrap(),
        ..Default::default()
    });
    let activity = std::sync::Arc::new(arxd::store::Activity::open(&state).unwrap());
    let listener = arxd::http::bind(config.clone()).await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = arxd::http::router(config, address, activity);
    let http = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("http://{address}/mcp"))
            .auth_header(&token),
    );
    let client = ().serve(transport).await.unwrap();
    assert!(
        client
            .list_tools(None)
            .await
            .unwrap()
            .tools
            .iter()
            .all(|tool| !tool.name.starts_with("vm_"))
    );
    assert!(
        client
            .call_tool(CallToolRequestParams::new("vm_catalog"))
            .await
            .is_err()
    );
    client.cancel().await.unwrap();
    http.abort();
    let _ = http.await;
}

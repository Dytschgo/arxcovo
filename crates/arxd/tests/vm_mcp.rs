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

/// Write an owner-private fixture file. The mode, owner and DACL are set on
/// the object itself, so neither the umask nor the token's default owner
/// (Builtin Administrators on elevated hosted Windows runners) leaks in.
/// An overwrite reapplies and re-verifies the same protection.
fn save(path: &Path, body: impl AsRef<[u8]>) {
    #[cfg(unix)]
    {
        use std::{
            io::Write,
            os::unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .unwrap();
        file.write_all(body.as_ref()).unwrap();
    }
    #[cfg(windows)]
    {
        std::fs::write(path, body).unwrap();
        win::protect(path, false);
    }
    assert_private(path, false);
}

/// Create an owner-private fixture directory (0700, or TokenUser owner with
/// a protected, inheritable TokenUser-only DACL).
fn private_dir(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new().mode(0o700).create(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    {
        std::fs::create_dir(path).unwrap();
        win::protect(path, true);
    }
    assert_private(path, true);
}

/// Distinguishes the three properties the product checks: the Unix mode and
/// owner UID, or the Windows owner SID and the exact protected DACL.
fn assert_private(path: &Path, directory: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path).unwrap();
        assert_eq!(metadata.is_dir(), directory, "{}", path.display());
        assert_eq!(
            metadata.mode() & 0o7777,
            if directory { 0o700 } else { 0o600 },
            "fixture mode: {}",
            path.display()
        );
        assert_eq!(
            metadata.uid(),
            unsafe { libc::geteuid() },
            "fixture owner UID: {}",
            path.display()
        );
    }
    #[cfg(windows)]
    {
        assert_eq!(std::fs::symlink_metadata(path).unwrap().is_dir(), directory);
        let (owner_is_user, dacl_is_private) = win::inspect(path, directory);
        assert!(owner_is_user, "fixture owner SID: {}", path.display());
        assert!(dacl_is_private, "fixture DACL: {}", path.display());
    }
}

#[cfg(unix)]
fn private_temp() -> (tempfile::TempDir, String, String) {
    use std::os::unix::fs::PermissionsExt;
    // Canonical, so macOS /var is not an alias of /private/var.
    let root = std::env::temp_dir().canonicalize().unwrap();
    let temp = tempfile::tempdir_in(root).unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_private(temp.path(), true);
    let uid = unsafe { libc::geteuid() };
    (temp, uid.to_string(), (uid ^ 1).to_string())
}

#[cfg(windows)]
fn private_temp() -> (tempfile::TempDir, String, String) {
    let temp = tempfile::tempdir().unwrap();
    win::protect(temp.path(), true);
    assert_private(temp.path(), true);
    let sid = win::user_text();
    let alternate = if sid == "S-1-5-21-11-22-33-1001" {
        "S-1-5-21-11-22-33-1002"
    } else {
        "S-1-5-21-11-22-33-1001"
    };
    (temp, sid, alternate.into())
}

#[cfg(windows)]
mod win {
    use std::{os::windows::ffi::OsStrExt, os::windows::io::AsRawHandle, path::Path, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    fn token_info(class: TOKEN_INFORMATION_CLASS) -> Vec<usize> {
        unsafe {
            let mut handle = ptr::null_mut();
            assert_ne!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle),
                0
            );
            let mut len = 0;
            GetTokenInformation(handle, class, ptr::null_mut(), 0, &mut len);
            let mut buffer = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
            let ok = GetTokenInformation(handle, class, buffer.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(handle);
            assert_ne!(ok, 0);
            buffer
        }
    }

    fn user_sid(user: &[usize]) -> PSID {
        // The buffer came from TokenUser and outlives every use of the SID.
        unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    pub(super) fn user_text() -> String {
        let user = token_info(TokenUser);
        unsafe {
            let mut text = ptr::null_mut();
            assert_ne!(ConvertSidToStringSidW(user_sid(&user), &mut text), 0);
            let len = (0..184).find(|i| *text.add(*i) == 0).unwrap();
            let sid = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
            LocalFree(text.cast());
            sid
        }
    }

    /// Whether new objects already default to TokenUser as their owner.
    pub(super) fn token_owner_is_user() -> bool {
        let user = token_info(TokenUser);
        let owner = token_info(TokenOwner);
        unsafe {
            EqualSid(
                user_sid(&user),
                (*owner.as_ptr().cast::<TOKEN_OWNER>()).Owner,
            ) != 0
        }
    }

    /// TokenUser owns the object, with a protected DACL that grants only
    /// TokenUser (inheritably on directories). Both fields are set on this
    /// exact object; inheritance from the parent supplies neither.
    pub(super) fn protect(path: &Path, directory: bool) {
        let user = token_info(TokenUser);
        let flags = if directory { "OICI" } else { "" };
        let sddl: Vec<u16> = format!("D:P(A;{flags};FA;;;{})", user_text())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut()
                ),
                0
            );
            let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
                0
            );
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let status = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION
                    | DACL_SECURITY_INFORMATION
                    | PROTECTED_DACL_SECURITY_INFORMATION,
                user_sid(&user),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            );
            LocalFree(descriptor);
            assert_eq!(status, 0, "protect {}", path.display());
        }
    }

    /// (owner equals TokenUser, DACL is protected with exactly one TokenUser
    /// FullControl grant), read from an opened handle without following links.
    pub(super) fn inspect(path: &Path, directory: bool) -> (bool, bool) {
        use std::os::windows::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap();
        let user = token_info(TokenUser);
        unsafe {
            let (mut owner, mut dacl, mut descriptor) =
                (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            assert_eq!(
                GetSecurityInfo(
                    file.as_raw_handle(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    &mut owner,
                    ptr::null_mut(),
                    &mut dacl,
                    ptr::null_mut(),
                    &mut descriptor,
                ),
                0
            );
            let owner_is_user = !owner.is_null() && EqualSid(owner, user_sid(&user)) != 0;
            let (mut control, mut revision) = (0, 0);
            let protected = GetSecurityDescriptorControl(descriptor, &mut control, &mut revision)
                != 0
                && control & SE_DACL_PROTECTED != 0;
            let mut ace = ptr::null_mut();
            let private = protected
                && !dacl.is_null()
                && (*dacl.cast::<ACL>()).AceCount == 1
                && GetAce(dacl, 0, &mut ace) != 0
                && {
                    let header = &*ace.cast::<ACE_HEADER>();
                    let allow = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                    let sid = ptr::addr_of!(allow.SidStart).cast_mut().cast();
                    header.AceType == 0
                        && header.AceFlags & INHERITED_ACE as u8 == 0
                        && (header.AceFlags & (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8
                            != 0)
                            == directory
                        && allow.Mask == 0x1f01ff
                        && EqualSid(sid, user_sid(&user)) != 0
                };
            LocalFree(descriptor);
            (owner_is_user, private)
        }
    }
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

/// The VM profile's configured client and the identity it must not be.
/// Windows requires a client SID distinct from the supervisor. POSIX is a
/// diagnostic-only profile whose socket is private to the supervisor UID,
/// so its client must be that same UID.
fn vm_clients(owner: &str, alternate: &str) -> (String, String) {
    if cfg!(windows) {
        (alternate.to_owned(), owner.to_owned())
    } else {
        (owner.to_owned(), alternate.to_owned())
    }
}

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

    let (vm_client, wrong_client) = vm_clients(&owner, &client_identity);
    let rejected_state = root.join("rejected");
    private_dir(&rejected_state);
    let good = json!({"state_dir":rejected_state,"agents":agents,"local":{"address":address,
        "client_identity":vm_client},"test_vms":{"enabled":false,"recipes":recipes}});
    #[cfg(windows)]
    default_owner_leg(&config_path, &good.to_string());
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
    // The client identity rule is checked by its own message, so a generic
    // permissions failure cannot pass for it.
    let mut wrong = good.clone();
    wrong["local"]["client_identity"] = json!(wrong_client);
    save(&config_path, wrong.to_string());
    assert_eq!(
        ForegroundConfig::load(&config_path)
            .err()
            .expect("wrong VM client accepted")
            .to_string(),
        if cfg!(windows) {
            "VM client and supervisor must differ"
        } else {
            "VM client must be the supervisor"
        }
    );
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
    let mut unknown = good.clone();
    unknown["test_vms"]["extra"] = json!(1);
    candidates.push(unknown.to_string());
    for invalid in candidates {
        save(&config_path, invalid);
        assert!(ForegroundConfig::load(&config_path).is_err());
    }
    // A link or reparse-point root naming the valid state is refused.
    let link = root.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&rejected_state, &link).unwrap();
    #[cfg(windows)]
    assert!(
        std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&rejected_state)
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    let mut linked = good.clone();
    linked["state_dir"] = json!(link);
    save(&config_path, linked.to_string());
    let error = ForegroundConfig::load(&config_path)
        .err()
        .expect("linked VM root accepted")
        .to_string();
    assert!(
        ["links forbidden", "reparse points forbidden"].contains(&error.as_str()),
        "{error}"
    );
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
        private_dir(&state);
        let mut config = json!({"state_dir":state,"agents":agents,"local":{"address":address,"client_identity":owner}});
        // The omitted profile keeps legacy selection, including HTTP.
        let http = enabled.is_none().then(|| {
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            config["listen"] = json!(format!("127.0.0.1:{port}"));
            port
        });
        if let Some(enabled) = enabled {
            config["test_vms"] = json!({"enabled":enabled,"recipes":recipes});
            config["local"]["client_identity"] = json!(vm_client);
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
                http_has_no_vm_routes(http.unwrap(), &token).await;
            }
            // The configured client reaches the diagnostics through the
            // production bridge on every platform; POSIX is diagnostic-only.
            Some(enabled) => {
                let (mut process, client) = bridge(&client_path).await;
                inspect_vm(
                    &client,
                    if !enabled {
                        "disabled"
                    } else if cfg!(all(windows, target_arch = "x86_64")) {
                        "qualification_missing"
                    } else {
                        "unsupported_platform"
                    },
                )
                .await;
                client.cancel().await.unwrap();
                let _ = process.kill().await;
            }
        }
        server.kill().await.unwrap();
        let mut entries = std::fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        entries.sort();
        // The legacy Inventory also opens the M5 vault store. The VM candidate
        // returns before the vault opens, so it creates only its own store.
        let expected: &[&str] = match enabled {
            None => &[
                "activity.lock",
                "activity.sqlite3",
                "vault.lock",
                "vault.sqlite3",
            ],
            Some(_) => &["activity.lock", "activity.sqlite3"],
        };
        assert_eq!(entries, expected, "{label}: unexpected state entries");
    }
}

/// The pre-fix fixture wrote files under the private root's inherited DACL,
/// leaving their owner to the token default. Where that default is not
/// TokenUser (elevated hosted runners), this is the red leg: the DACL passes
/// and the real loader refuses the owner. `save` then fixes only the owner
/// and DACL of the same path and bytes.
#[cfg(windows)]
fn default_owner_leg(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    let (owner_is_user, _) = win::inspect(path, false);
    let token_owner_is_user = win::token_owner_is_user();
    assert_eq!(owner_is_user, token_owner_is_user);
    let loaded = ForegroundConfig::load(path)
        .map(drop)
        .map_err(|e| e.to_string());
    eprintln!("default owner leg: token_owner_is_user={token_owner_is_user} load={loaded:?}");
    if !token_owner_is_user {
        assert_eq!(loaded, Err("owner mismatch".into()));
    }
}

/// Probe the legacy binary's HTTP listener: unchanged tools, no VM routes.
async fn http_has_no_vm_routes(port: u16, token: &str) {
    tokio::time::timeout(Duration::from_secs(4), async {
        while tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("http://127.0.0.1:{port}/mcp"))
            .auth_header(token),
    );
    let client = ().serve(transport).await.unwrap();
    let tools = client.list_tools(None).await.unwrap().tools;
    assert!(tools.iter().any(|tool| tool.name == "list_activity"));
    assert!(tools.iter().all(|tool| !tool.name.starts_with("vm_")));
    assert!(
        client
            .call_tool(CallToolRequestParams::new("vm_catalog"))
            .await
            .is_err()
    );
    client.cancel().await.unwrap();
}

/// The validated VM root is pinned until the store opens: a replacement
/// under the same canonical name is impossible (Windows holds the
/// directory without delete sharing) or refused before any state is
/// created in it (POSIX compares the pinned directory identity).
#[tokio::test]
async fn validated_root_replacement_is_refused() {
    let (temp, owner, alternate) = private_temp();
    let root = temp.path();
    let parent = root.join("parent");
    private_dir(&parent);
    let state = parent.join("state");
    private_dir(&state);
    let token = "a".repeat(64);
    let agents = json!([{"principal":"fixture","token_sha256":hex::encode(Sha256::digest(token.as_bytes()))}]);
    let (vm_client, _) = vm_clients(&owner, &alternate);
    let config_path = root.join("server.json");
    let address = endpoint(root);
    save(
        &config_path,
        json!({"state_dir":state,"agents":agents,"local":{"address":address,
            "client_identity":vm_client},"test_vms":{"enabled":false}})
        .to_string(),
    );
    // POSIX: an ancestor another principal could rename entries in is
    // refused; a sticky one is not, as the root entry is ours.
    #[cfg(unix)]
    for (mode, refused) in [(0o777, true), (0o1777, false), (0o700, false)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(mode)).unwrap();
        let loaded = ForegroundConfig::load(&config_path)
            .map(drop)
            .map_err(|error| error.to_string());
        assert_eq!(
            loaded,
            if refused {
                Err("replaceable VM state ancestor".to_owned())
            } else {
                Ok(())
            },
            "{mode:o}"
        );
    }
    let ForegroundConfig::TestVms(profile) = ForegroundConfig::load(&config_path).unwrap() else {
        panic!("candidate profile expected");
    };
    let moved = root.join("moved");
    #[cfg(windows)]
    {
        // ERROR_SHARING_VIOLATION: the pin holds the root and each ancestor
        // without delete sharing, so neither can be renamed away.
        let sharing = Some(32);
        for (from, to) in [(&state, &moved), (&parent, &moved)] {
            let swap = std::fs::rename(from, to).expect_err("pinned VM path renamed");
            assert_eq!(swap.raw_os_error(), sharing, "{swap}");
        }
        drop(profile);
        // The same renames succeed once the pin is dropped.
        std::fs::rename(&state, &moved).unwrap();
        std::fs::rename(&moved, &state).unwrap();
        std::fs::rename(&parent, &moved).unwrap();
        std::fs::rename(&moved, &parent).unwrap();
    }
    #[cfg(unix)]
    {
        // POSIX keeps only the descriptor: a same-UID rename succeeds, and an
        // equally private directory under the same name is refused before
        // the store creates anything in either directory.
        std::fs::rename(&state, &moved).unwrap();
        private_dir(&state);
        let result =
            tokio::time::timeout(Duration::from_secs(2), arxd::local::run_vm_profile(profile))
                .await
                .expect("the replacement root was served");
        assert_eq!(result.unwrap_err().to_string(), "server-state");
        assert_eq!(std::fs::read_dir(&state).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&moved).unwrap().count(), 0);
        assert!(!endpoint_ready(&address), "the replacement root was bound");
    }
}

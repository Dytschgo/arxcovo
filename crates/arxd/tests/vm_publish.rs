//! Fake-backed publication controls. All sockets are IPv4 loopback.

#[cfg(all(windows, target_arch = "x86_64"))]
#[allow(dead_code)]
#[path = "../src/security.rs"]
mod security;
#[path = "../src/vm"]
mod vm {
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub use arxd::vm::paths;
    #[path = "publish/mod.rs"]
    mod publish;

    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use axum::{
        Json, Router,
        body::to_bytes,
        extract::{Request, State},
        http::{Method, StatusCode},
        response::{IntoResponse, Response},
    };
    use publish::{
        AgentPublishing, Hostname, Refusal, TunnelId, admit,
        cloudflare::{CloudflareDns, DnsApi, DnsBearer, DnsError, Reconciled, Removal, Route},
    };
    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
        task::JoinHandle,
        time::timeout,
    };

    const ZONE: &str = "1234567890abcdef1234567890abcdef";
    const TUNNEL: &str = "12345678-1234-4321-8234-123456789abc";
    const NAME: &str = "site.example.invalid";
    const NONCE: &str = "11223344556677889900aabbccddeeff";
    const ID: &str = "aabbccddeeff00112233445566778899";
    const FOREIGN_ID: &str = "99aabbccddeeff001122334455667788";
    const OTHER_ID: &str = "00112233445566778899aabbccddeeff";
    const CANARY: &str = "synthetic_dont_print_87e3";
    const LEG: Duration = Duration::from_millis(1500);

    #[test]
    fn owner_policy_and_all_publish_preconditions_fail_closed() {
        use AgentPublishing::{Disabled, Enabled};
        let name = Hostname::parse(NAME).unwrap();
        assert_eq!(AgentPublishing::default(), Disabled);
        assert_eq!(
            admit(Disabled, true, true, Some(&name)),
            Err(Refusal::Disabled)
        );
        assert_eq!(
            admit(Enabled, false, true, Some(&name)),
            Err(Refusal::CredentialMissing)
        );
        assert_eq!(
            admit(Enabled, true, false, Some(&name)),
            Err(Refusal::StaticVmUnhealthy)
        );
        assert_eq!(
            admit(Enabled, true, true, None),
            Err(Refusal::HostnameMissing)
        );
        assert_eq!(admit(Enabled, true, true, Some(&name)), Ok(()));
        for invalid in [
            "*.example.com",
            "127.0.0.1",
            "::1",
            "host.0x1",
            "UPPER.example",
            "foo.example.",
            "evil.com/x",
        ] {
            assert!(
                Hostname::parse(invalid).is_err(),
                "accepted invalid owner binding"
            );
        }
    }

    fn route() -> Route {
        Route::new(
            ZONE,
            Hostname::parse(NAME).unwrap(),
            &TunnelId::parse(TUNNEL).unwrap(),
            NONCE,
        )
        .unwrap()
    }

    #[derive(Default)]
    struct DnsState {
        records: Vec<Value>,
        post_count: usize,
        delete_count: usize,
        failed_create_reply: bool,
        /// Committed beside the owned record by the next POST.
        foreign_on_post: Option<Value>,
        /// (release, committed): the next POST commits only after release, in a task that
        /// outlives the client's abandoned request.
        post_gate: Option<(oneshot::Receiver<()>, oneshot::Sender<()>)>,
        hostile: Option<Hostile>,
    }

    #[derive(Clone, Copy)]
    enum Hostile {
        Error,
        Duplicate,
        Oversize,
        Paginated,
        Redirect,
        Stall,
    }

    struct FakeDns {
        client: CloudflareDns,
        state: Arc<Mutex<DnsState>>,
        task: JoinHandle<()>,
    }

    impl FakeDns {
        async fn start() -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let client = CloudflareDns::fixture(
                DnsBearer::new(CANARY.to_owned()).unwrap(),
                listener.local_addr().unwrap(),
            );
            let state = Arc::new(Mutex::new(DnsState::default()));
            let app = Router::new().fallback(handle_dns).with_state(state.clone());
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            Self {
                client,
                state,
                task,
            }
        }
    }

    impl Drop for FakeDns {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn handle_dns(State(state): State<Arc<Mutex<DnsState>>>, req: Request) -> Response {
        let (parts, body) = req.into_parts();
        let auth = format!("Bearer {CANARY}");
        if parts
            .headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            != Some(auth.as_str())
            || parts.headers.get("host").and_then(|h| h.to_str().ok()) != Some("api.cloudflare.com")
        {
            return StatusCode::BAD_REQUEST.into_response();
        }
        let body = to_bytes(body, 4096).await.unwrap();
        let hostile = state.lock().unwrap().hostile;
        if let Some(mode) = hostile {
            return match mode {
                Hostile::Error => {
                    (StatusCode::BAD_GATEWAY, format!("remote error {CANARY}")).into_response()
                }
                Hostile::Duplicate => {
                    format!(r#"{{"success":true,"success":false,"result":"{CANARY}"}}"#)
                        .into_response()
                }
                Hostile::Oversize => ("a".repeat(140 * 1024) + CANARY).into_response(),
                Hostile::Paginated => page(&[], 2),
                Hostile::Redirect => (
                    StatusCode::TEMPORARY_REDIRECT,
                    [("location", "http://127.0.0.1:1/redirect")],
                    CANARY,
                )
                    .into_response(),
                Hostile::Stall => std::future::pending().await,
            };
        }
        if parts.method == Method::POST {
            let gate = state.lock().unwrap().post_gate.take();
            if let Some((release, committed)) = gate {
                let mut record: Value = serde_json::from_slice(&body).unwrap();
                record["id"] = json!(ID);
                state.lock().unwrap().post_count += 1;
                let shared = state.clone();
                tokio::spawn(async move {
                    if release.await.is_ok() {
                        shared.lock().unwrap().records.push(record);
                        let _ = committed.send(());
                    }
                });
                return std::future::pending().await;
            }
        }
        let prefix = format!("/client/v4/zones/{ZONE}/dns_records");
        let mut state = state.lock().unwrap();
        match (parts.method, parts.uri.path().strip_prefix(&prefix)) {
            (Method::GET, Some("")) => {
                if parts.uri.query() != Some("name.exact=site.example.invalid&page=1&per_page=100")
                {
                    return StatusCode::BAD_REQUEST.into_response();
                }
                page(&state.records, 1)
            }
            (Method::POST, Some("")) => {
                state.post_count += 1;
                let mut record: Value = serde_json::from_slice(&body).unwrap();
                record["id"] = json!(ID);
                state.records.push(record.clone());
                if let Some(foreign) = state.foreign_on_post.take() {
                    state.records.push(foreign);
                }
                if state.failed_create_reply {
                    // The mutation happened but its acknowledgement did not arrive.
                    return (StatusCode::BAD_GATEWAY, CANARY).into_response();
                }
                result(record)
            }
            (Method::GET, Some(child)) => match state
                .records
                .iter()
                .find(|r| child.strip_prefix('/') == r["id"].as_str())
            {
                Some(record) => result(record.clone()),
                None => StatusCode::NOT_FOUND.into_response(),
            },
            (Method::DELETE, Some(child)) => {
                let id = child.strip_prefix('/').unwrap();
                state.delete_count += 1;
                state.records.retain(|r| r["id"] != id);
                result(json!({ "id": id }))
            }
            _ => StatusCode::BAD_REQUEST.into_response(),
        }
    }

    fn result(record: Value) -> Response {
        Json(json!({ "success": true, "result": record })).into_response()
    }

    fn page(records: &[Value], total_pages: usize) -> Response {
        Json(json!({
            "success": true, "result": records,
            "result_info": { "page": 1, "total_pages": total_pages, "total_count": records.len() }
        }))
        .into_response()
    }

    #[tokio::test]
    async fn dns_recovers_a_mutation_without_reposting_and_preserves_foreign_records() {
        timeout(LEG, async {
            let fake = FakeDns::start().await;
            let route = route();
            let foreign = json!({
                "id": FOREIGN_ID, "name": NAME, "type": "TXT", "content": "foreign"
            });
            fake.state.lock().unwrap().records.push(foreign.clone());
            assert_eq!(fake.client.preflight(&route).await, Err(DnsError::Conflict));
            assert_eq!(fake.state.lock().unwrap().post_count, 0);
            {
                let mut state = fake.state.lock().unwrap();
                state.records.clear();
                state.failed_create_reply = true;
            }
            assert!(fake.client.create(&route).await.is_err());
            let Reconciled::Owned(id) = fake.client.reconcile(&route).await.unwrap() else {
                panic!("lost mutation failed ownership reconciliation");
            };
            assert_eq!(id.as_str(), ID);
            // A same-name foreign record survives removal of the attributed ID.
            fake.state.lock().unwrap().records.push(foreign.clone());
            assert_eq!(
                fake.client.remove(&route, &id).await,
                Ok(Removal::ForeignOrAmbiguous)
            );
            assert_eq!(
                fake.client.remove(&route, &id).await,
                Ok(Removal::ForeignOrAmbiguous)
            );
            let state = fake.state.lock().unwrap();
            assert_eq!(state.records, [foreign]);
            assert_eq!(state.post_count, 1);
            assert_eq!(state.delete_count, 1);
        })
        .await
        .unwrap();
    }

    fn owned_record(id: &str) -> Value {
        json!({
            "id": id, "name": NAME, "type": "CNAME", "proxied": true,
            "content": format!("{TUNNEL}.cfargotunnel.com"), "comment": format!("arxcovo:{NONCE}")
        })
    }

    #[tokio::test]
    async fn dns_recovers_a_uniquely_owned_id_beside_foreign_records() {
        timeout(LEG, async {
            let foreign = json!({
                "id": FOREIGN_ID, "name": NAME, "type": "TXT", "content": "foreign"
            });
            // A lost POST reply, then a valid POST reply, each racing a foreign record.
            for lost_reply in [true, false] {
                let fake = FakeDns::start().await;
                let route = route();
                fake.client.preflight(&route).await.unwrap();
                {
                    let mut state = fake.state.lock().unwrap();
                    state.failed_create_reply = lost_reply;
                    state.foreign_on_post = Some(foreign.clone());
                }
                let expected = if lost_reply {
                    DnsError::Rejected
                } else {
                    DnsError::Conflict
                };
                assert_eq!(fake.client.create(&route).await, Err(expected));
                let Reconciled::OwnedWithConflict(id) =
                    fake.client.reconcile(&route).await.unwrap()
                else {
                    panic!("the uniquely owned record was not recovered");
                };
                assert_eq!(id.as_str(), ID);
                assert_eq!(fake.client.preflight(&route).await, Err(DnsError::Conflict));
                for _ in 0..2 {
                    assert_eq!(
                        fake.client.remove(&route, &id).await,
                        Ok(Removal::ForeignOrAmbiguous)
                    );
                }
                let state = fake.state.lock().unwrap();
                assert_eq!(state.records, std::slice::from_ref(&foreign));
                assert_eq!((state.post_count, state.delete_count), (1, 1));
            }

            // Two marked records leave no attributable ID and nothing is deleted.
            let fake = FakeDns::start().await;
            let route = route();
            fake.state.lock().unwrap().records = vec![owned_record(ID), owned_record(OTHER_ID)];
            assert_eq!(
                fake.client.reconcile(&route).await,
                Ok(Reconciled::ForeignOrAmbiguous)
            );
            assert_eq!(fake.client.preflight(&route).await, Err(DnsError::Conflict));
            assert_eq!(fake.state.lock().unwrap().delete_count, 0);

            // The sole marked record is still plainly owned.
            fake.state.lock().unwrap().records = vec![owned_record(ID)];
            let Reconciled::Owned(id) = fake.client.reconcile(&route).await.unwrap() else {
                panic!("the sole owned record was not attributed");
            };
            assert_eq!(id.as_str(), ID);
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dns_settles_a_delayed_post_by_lookup_without_reposting() {
        timeout(LEG, async {
            let fake = FakeDns::start().await;
            let route = route();
            let (release, release_rx) = oneshot::channel();
            let (committed_tx, committed) = oneshot::channel();
            fake.state.lock().unwrap().post_gate = Some((release_rx, committed_tx));
            // The single POST exceeds its bounded deadline before the fake commits it.
            assert_eq!(fake.client.create(&route).await, Err(DnsError::Unavailable));
            assert_eq!(fake.client.reconcile(&route).await, Ok(Reconciled::Absent));
            release.send(()).unwrap();
            committed.await.unwrap();
            let Reconciled::Owned(id) = fake.client.reconcile(&route).await.unwrap() else {
                panic!("the late commit was not attributed");
            };
            assert_eq!(id.as_str(), ID);
            assert_eq!(fake.state.lock().unwrap().post_count, 1);
        })
        .await
        .unwrap();
    }

    /// One raw HTTP/1.1 reply from an IPv4 loopback socket, decoded by the production transport.
    async fn raw_preflight(wire: Vec<u8>) -> Result<(), DnsError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let client = CloudflareDns::fixture(
            DnsBearer::new(CANARY.to_owned()).unwrap(),
            listener.local_addr().unwrap(),
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let read = socket.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "request ended before its header");
                request.extend_from_slice(&buffer[..read]);
            }
            socket.write_all(&wire).await.unwrap();
            socket.shutdown().await.unwrap();
            request
        });
        let result = client.preflight(&route()).await;
        let request = server.await.unwrap();
        assert!(
            request.starts_with(format!("GET /client/v4/zones/{ZONE}/dns_records?").as_bytes())
        );
        result
    }

    #[tokio::test]
    async fn dns_chunked_framing_is_strict_through_the_transport() {
        const LISTING: &str = r#"{"success":true,"result":[],"result_info":{"page":1,"total_pages":1,"total_count":0}}"#;
        const HEAD: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n";
        let chunks = |parts: &[&str]| -> String {
            parts
                .iter()
                .map(|part| format!("{:x}\r\n{part}\r\n", part.len()))
                .collect()
        };
        let (a, rest) = LISTING.split_at(7);
        let (b, c) = rest.split_at(40);
        let body = chunks(&[a, b, c]);
        timeout(LEG, async {
            let valid = format!("{HEAD}{body}0\r\n\r\n");
            assert_eq!(raw_preflight(valid.clone().into_bytes()).await, Ok(()));
            let truncated = format!("{HEAD}{:x}\r\n{}", LISTING.len(), &LISTING[..20]);
            let bad_delimiter = format!("{HEAD}{:x}\r\n{LISTING}XX0\r\n\r\n", LISTING.len());
            let missing_terminator = format!("{HEAD}{body}");
            let trailing = format!("{valid}extra");
            let both = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nTransfer-Encoding: chunked\r\n\r\n{body}0\r\n\r\n",
                body.len() + 5
            );
            for wire in [truncated, bad_delimiter, missing_terminator, trailing, both] {
                assert_eq!(
                    raw_preflight(wire.into_bytes()).await,
                    Err(DnsError::InvalidResponse)
                );
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dns_refuses_to_delete_a_modified_owned_id() {
        timeout(LEG, async {
            let fake = FakeDns::start().await;
            let route = route();
            let id = fake.client.create(&route).await.unwrap();
            fake.state.lock().unwrap().records[0]["comment"] = json!("replaced");
            assert_eq!(
                fake.client.remove(&route, &id).await,
                Ok(Removal::ForeignOrAmbiguous)
            );
            assert_eq!(fake.state.lock().unwrap().delete_count, 0);
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dns_response_limits_and_errors_are_closed_and_redacted() {
        assert!(!format!("{:?}", DnsBearer::new(CANARY.to_owned()).unwrap()).contains(CANARY));
        assert!(DnsBearer::new(format!("{CANARY}\r\nInjected: yes")).is_err());
        timeout(LEG, async {
            let fake = FakeDns::start().await;
            for (mode, expected) in [
                (Hostile::Error, DnsError::Rejected),
                (Hostile::Duplicate, DnsError::InvalidResponse),
                (Hostile::Oversize, DnsError::InvalidResponse),
                (Hostile::Paginated, DnsError::InvalidResponse),
                (Hostile::Redirect, DnsError::Rejected),
                (Hostile::Stall, DnsError::Unavailable),
            ] {
                fake.state.lock().unwrap().hostile = Some(mode);
                let error = fake.client.preflight(&route()).await.unwrap_err();
                assert_eq!(error, expected);
                assert!(!format!("{error:?}").contains(CANARY));
            }
        })
        .await
        .unwrap();
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    mod native {
        use std::{
            fs,
            num::NonZeroU16,
            os::windows::{
                ffi::OsStrExt,
                fs::OpenOptionsExt,
                io::{AsRawHandle, FromRawHandle, OwnedHandle},
            },
            path::{Path, PathBuf},
            process::Command,
            ptr::null_mut,
        };

        use arxd::vm::paths::ArtifactPin;
        use rcgen::{CertificateParams, CertifiedIssuer, IsCa, KeyPair};
        use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
        use sha2::{Digest, Sha256};
        use tokio_rustls::TlsAcceptor;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{Authorization::*, *},
            Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT},
            System::Threading::{
                GetCurrentProcess, GetExitCodeProcess, OpenProcess, OpenProcessToken,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        };

        use super::publish::connector::{Connector, ConnectorError, Plan, TunnelCredential};
        use super::*;

        const ORIGIN_NAME: &str = "generation.example.invalid";
        const FAKE_SECRET: &str = "SyntheticDontPrint012345678901234567890abcA=";
        const FIXTURE_ENV: &str = "ARX_PUBLISH_CONNECTOR_FIXTURE";
        const FIXTURE_NAME: &str = "vm::native::connector_fixture";

        // The separate executable receives cloudflared's actual argument list and
        // environment. Compile it from inline Rust to keep the B1 file fence.
        const FAKE: &str = stringify! {
            use std::{env, fs, io::{Read, Write}, net::TcpListener};
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn SetConsoleCtrlHandler(handler: unsafe extern "system" fn(u32) -> i32, add: i32) -> i32;
            }
            unsafe extern "system" fn ignore_interrupt(_: u32) -> i32 { 1 }
            fn main() {
                unsafe { SetConsoleCtrlHandler(ignore_interrupt, 1); }
                let args: Vec<_> = env::args().skip(1).collect();
                let credential = env::var("TUNNEL_CRED_CONTENTS").unwrap();
                assert!(credential.contains("SyntheticDontPrint"));
                assert!(env::var_os("SystemRoot").is_some());
                assert!(env::var_os("TUNNEL_TOKEN").is_none());
                assert!(env::var_os("PATH").is_none());
                let mut witness = fs::OpenOptions::new().append(true).create(true)
                    .open("fake-witness.txt").unwrap();
                writeln!(witness, "{}|{}|{}", std::process::id(), args.join("|"),
                    credential.contains("SyntheticDontPrint")).unwrap();
                println!("{}", credential);
                eprintln!("{}", credential);
                if std::path::Path::new("crash.flag").exists() {
                    std::process::exit(17);
                }
                let metrics = args.iter().position(|arg| arg == "--metrics").unwrap();
                let listener = TcpListener::bind(&args[metrics + 1]).unwrap();
                for mut stream in listener.incoming().map(Result::unwrap) {
                    let mut request = [0u8; 256];
                    if stream.read(&mut request).unwrap_or_default() == 0 {
                        continue;
                    }
                    // Keep the accepted socket alive until the supervising client closes
                    // it so TCP owner attribution observes the actual child PID.
                    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
                    let _ = stream.write_all(response);
                    let _ = stream.read(&mut request);
                }
            }
        };

        fn fixture_binary(root: &Path) -> (PathBuf, ArtifactPin) {
            let source = root.join("cloudflared-fake.rs");
            let image = root.join("cloudflared-fake.exe");
            fs::write(&source, FAKE).unwrap();
            let output = Command::new("rustc")
                .arg("--edition=2024")
                .arg(&source)
                .arg("-o")
                .arg(&image)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let pin = pin(&fs::read(&image).unwrap());
            (image, pin)
        }

        /// A word-aligned token information buffer for the current process.
        fn token_info(class: TOKEN_INFORMATION_CLASS) -> Vec<usize> {
            unsafe {
                let mut token = null_mut();
                assert_ne!(
                    OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
                    0
                );
                let token = OwnedHandle::from_raw_handle(token);
                let mut size = 0;
                GetTokenInformation(token.as_raw_handle(), class, null_mut(), 0, &mut size);
                assert!((1..=4096).contains(&size));
                let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
                assert_ne!(
                    GetTokenInformation(
                        token.as_raw_handle(),
                        class,
                        buffer.as_mut_ptr().cast(),
                        size,
                        &mut size
                    ),
                    0
                );
                buffer
            }
        }

        fn user_sid(user: &[usize]) -> PSID {
            // The buffer came from TokenUser and outlives every use of the SID.
            unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        }

        fn token_owner_is_user() -> bool {
            let user = token_info(TokenUser);
            let owner = token_info(TokenOwner);
            unsafe {
                EqualSid(
                    user_sid(&user),
                    (*owner.as_ptr().cast::<TOKEN_OWNER>()).Owner,
                ) != 0
            }
        }

        /// Reads the owner from an opened handle: (equals TokenUser, is Builtin Administrators).
        fn owner_of(path: &Path) -> (bool, bool) {
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)
                .unwrap();
            let user = token_info(TokenUser);
            unsafe {
                let (mut owner, mut descriptor) = (null_mut(), null_mut());
                assert_eq!(
                    GetSecurityInfo(
                        file.as_raw_handle(),
                        SE_FILE_OBJECT,
                        OWNER_SECURITY_INFORMATION,
                        &mut owner,
                        null_mut(),
                        null_mut(),
                        null_mut(),
                        &mut descriptor,
                    ),
                    0
                );
                assert!(!owner.is_null());
                let result = (
                    EqualSid(owner, user_sid(&user)) != 0,
                    IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0,
                );
                LocalFree(descriptor);
                result
            }
        }

        /// Test-only ownership witness. Production requires TokenUser on every held object,
        /// so a hosted failure here names ownership instead of a closed custody error.
        fn assert_owned(label: &str, path: &Path) {
            let (user, admins) = owner_of(path);
            assert!(
                user,
                "{label}: fixture owner must equal TokenUser (builtin_admins={admins})"
            );
        }

        fn set_owner_to_user(path: &Path) {
            let user = token_info(TokenUser);
            let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let status = unsafe {
                SetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION,
                    user_sid(&user),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            assert_eq!(status, 0);
        }

        fn protect_fixture(root: &Path) {
            // Protect the fixture if hosted CI uses shared scratch.
            unsafe {
                let user = token_info(TokenUser);
                let sid = user_sid(&user);
                let mut sid_text = null_mut();
                assert_ne!(ConvertSidToStringSidW(sid, &mut sid_text), 0);
                let length = (0..184).find(|&i| *sid_text.add(i) == 0).unwrap();
                let sid = String::from_utf16(std::slice::from_raw_parts(sid_text, length)).unwrap();
                LocalFree(sid_text.cast());
                let sddl: Vec<_> = format!("D:P(A;OICI;FA;;;{sid})")
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let mut descriptor = null_mut();
                assert_ne!(
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        sddl.as_ptr(),
                        1,
                        &mut descriptor,
                        null_mut()
                    ),
                    0
                );
                let (mut present, mut defaulted, mut acl) = (0, 0, null_mut());
                assert_ne!(
                    GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
                    0
                );
                let path: Vec<_> = root.as_os_str().encode_wide().chain(Some(0)).collect();
                let result = SetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    acl,
                    null_mut(),
                );
                LocalFree(descriptor);
                assert_eq!(result, 0);
            }
        }

        fn pin(bytes: &[u8]) -> ArtifactPin {
            ArtifactPin {
                size: bytes.len() as u64,
                sha256: Sha256::digest(bytes).into(),
            }
        }

        fn encode_ca_pem(der: &[u8]) -> String {
            const ALPHABET: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut base64 = Vec::new();
            for bytes in der.chunks(3) {
                let a = bytes[0];
                let b = bytes.get(1).copied().unwrap_or_default();
                let c = bytes.get(2).copied().unwrap_or_default();
                base64.extend([
                    ALPHABET[(a >> 2) as usize],
                    ALPHABET[((a & 3) << 4 | (b >> 4)) as usize],
                    if bytes.len() > 1 {
                        ALPHABET[((b & 15) << 2 | (c >> 6)) as usize]
                    } else {
                        b'='
                    },
                    if bytes.len() > 2 {
                        ALPHABET[(c & 63) as usize]
                    } else {
                        b'='
                    },
                ]);
            }
            let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
            for line in base64.chunks(64) {
                pem.push_str(std::str::from_utf8(line).unwrap());
                pem.push('\n');
            }
            pem.push_str("-----END CERTIFICATE-----\n");
            pem
        }

        async fn origin(root: &Path) -> (NonZeroU16, PathBuf, ArtifactPin, JoinHandle<()>) {
            let mut params = CertificateParams::default();
            params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            let ca = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
            let key = KeyPair::generate().unwrap();
            let cert = CertificateParams::new(vec![ORIGIN_NAME.to_owned()])
                .unwrap()
                .signed_by(&key, &ca)
                .unwrap();
            let pem = encode_ca_pem(ca.der());
            let ca_path = root.join("origin-ca.pem");
            fs::write(&ca_path, pem.as_bytes()).unwrap();
            let tls = ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let port = NonZeroU16::new(listener.local_addr().unwrap().port()).unwrap();
            let acceptor = TlsAcceptor::from(Arc::new(tls));
            let task = tokio::spawn(async move {
                while let Ok((socket, _)) = listener.accept().await {
                    let _ = timeout(Duration::from_millis(200), acceptor.accept(socket)).await;
                }
            });
            (port, ca_path, pin(pem.as_bytes()), task)
        }

        fn credential_json() -> String {
            json!({"AccountTag": ZONE, "TunnelID": TUNNEL, "TunnelSecret": FAKE_SECRET}).to_string()
        }

        /// The previous DACL-only setup under the creating token's default owner. Where that
        /// owner differs from TokenUser, as on an elevated hosted runner, this is the red leg:
        /// the DACL passes, the owner differs and the real loader refuses. Assigning the same
        /// leaf to TokenUser is the green leg with unchanged path, bytes and DACL.
        fn ownership_probe(root: &Path) {
            let tunnel = TunnelId::parse(TUNNEL).unwrap();
            protect_fixture(root);
            let leaf = root.join("credential.json");
            fs::write(&leaf, credential_json()).unwrap();
            let token_owner_is_user = token_owner_is_user();
            let (owner_is_user, owner_is_admins) = owner_of(&leaf);
            let private = crate::security::private(&leaf).is_ok();
            let first = TunnelCredential::load(&leaf, ZONE, &tunnel).map(drop);
            eprintln!(
                "connector ownership probe: token_owner_is_user={token_owner_is_user} leaf_owner_is_user={owner_is_user} leaf_owner_is_builtin_admins={owner_is_admins} private_dacl={private} load={first:?}"
            );
            assert!(private, "fixture DACL must pass the private-DACL predicate");
            assert_eq!(owner_is_user, token_owner_is_user);
            if owner_is_user {
                assert_eq!(first, Ok(()));
                eprintln!("connector ownership probe: red leg not applicable on this token");
                return;
            }
            assert_eq!(first, Err(ConnectorError::Credential));
            set_owner_to_user(&leaf);
            assert_owned("probe credential", &leaf);
            assert!(crate::security::private(&leaf).is_ok());
            assert_eq!(
                TunnelCredential::load(&leaf, ZONE, &tunnel).map(drop),
                Ok(())
            );
            eprintln!("connector ownership probe: red leg refused, owned leaf loaded");
        }

        /// Runs the fixture once in an exact-test child so only that child's TokenOwner changes.
        #[tokio::test]
        async fn connector_publish_unpublish_receipt_and_restart_are_bounded() {
            assert!(
                std::env::var_os(FIXTURE_ENV).is_none(),
                "connector fixture must not recurse"
            );
            let output = timeout(
                Duration::from_secs(120),
                tokio::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        FIXTURE_NAME,
                        "--ignored",
                        "--nocapture",
                        "--test-threads",
                        "1",
                        "--format",
                        "pretty",
                        "--color",
                        "never",
                    ])
                    .env(FIXTURE_ENV, "1")
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .expect("connector fixture exceeded its outer deadline")
            .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            for text in [&stdout, &stderr] {
                assert!(
                    !text.contains(FAKE_SECRET) && !text.contains(CANARY),
                    "connector fixture output contained a synthetic credential"
                );
            }
            // Ownership probe and leg timing lines only; credentials were excluded above.
            eprintln!("{stderr}");
            assert!(
                output.status.success(),
                "connector fixture failed\n{stdout}\n{stderr}"
            );
            assert!(
                stdout.contains(&format!("test {FIXTURE_NAME} ... ok"))
                    && stdout.contains("test result: ok. 1 passed;"),
                "connector fixture did not run exactly once\n{stdout}"
            );
        }

        #[tokio::test]
        #[ignore = "subprocess-only connector fixture; changes only the child's TokenOwner"]
        async fn connector_fixture() {
            assert_eq!(std::env::var(FIXTURE_ENV).as_deref(), Ok("1"));
            let probe = tempfile::Builder::new()
                .prefix("publish-probe-")
                .tempdir()
                .unwrap();
            ownership_probe(probe.path());
            // New objects, including the connector's generated YAML, get TokenUser as owner.
            arxd::service::windows::set_current_token_owner().expect("normalize TokenOwner");
            assert!(token_owner_is_user());

            let temp = tempfile::Builder::new()
                .prefix("publish-")
                .tempdir()
                .unwrap();
            let root = temp.path();
            protect_fixture(root);
            let (binary, binary_receipt) = fixture_binary(root);
            let (origin_port, ca_path, ca_receipt, origin_task) = origin(root).await;
            let route = route();
            let fake = FakeDns::start().await;
            let hostname = Hostname::parse(NAME).unwrap();
            let tunnel = TunnelId::parse(TUNNEL).unwrap();
            let credential_path = root.join("credential.json");
            fs::write(&credential_path, credential_json()).unwrap();
            for (label, path) in [
                ("fixture root", root),
                ("fake executable", binary.as_path()),
                ("origin CA", ca_path.as_path()),
                ("credential", credential_path.as_path()),
            ] {
                assert_owned(label, path);
            }
            let credential = TunnelCredential::load(&credential_path, ZONE, &tunnel).unwrap();
            assert!(!format!("{credential:?}").contains(FAKE_SECRET));
            let wrong_name = Hostname::parse("wrong.example.invalid").unwrap();
            let origin_name = Hostname::parse(ORIGIN_NAME).unwrap();
            let bad_receipt = ArtifactPin {
                sha256: [0; 32],
                ..binary_receipt
            };
            let mut plan = Plan {
                executable: &binary,
                binary_receipt: bad_receipt,
                private_root: root,
                hostname: &hostname,
                tunnel: &tunnel,
                credential: &credential,
                origin_port,
                origin_name: &origin_name,
                ca_path: &ca_path,
                ca_receipt,
            };
            let witness_path = root.join("fake-witness.txt");

            assert!(matches!(
                timeout(LEG, Connector::start(&plan)).await.unwrap(),
                Err(ConnectorError::BinaryReceipt)
            ));
            plan.binary_receipt = binary_receipt;
            plan.origin_name = &wrong_name;
            assert!(matches!(
                timeout(LEG, Connector::start(&plan)).await.unwrap(),
                Err(ConnectorError::Origin)
            ));
            plan.origin_name = &origin_name;
            assert!(!witness_path.exists());

            let publish_started = std::time::Instant::now();
            let (mut connector, id) = timeout(LEG, async {
                assert_eq!(
                    admit(AgentPublishing::Enabled, true, true, Some(&hostname)),
                    Ok(())
                );
                fake.client.preflight(&route).await.unwrap();
                let connector = Connector::start(&plan).await.unwrap();
                assert!(connector.ready().await);
                let id = fake.client.create(&route).await.unwrap();
                let record = &fake.state.lock().unwrap().records[0];
                assert_eq!(record["comment"], format!("arxcovo:{NONCE}"));
                assert_eq!(record["content"], format!("{TUNNEL}.cfargotunnel.com"));
                assert_eq!(record["proxied"], true);
                (connector, id)
            })
            .await
            .expect("publish exceeded its deadline");
            let publish_elapsed = publish_started.elapsed();

            let witness = fs::read_to_string(&witness_path).unwrap();
            assert!(!witness.contains(FAKE_SECRET));
            let fields: Vec<_> = witness.trim_end().split('|').collect();
            assert_eq!(fields.len(), 11);
            assert_eq!(fields[1], "--config");
            assert_eq!(&fields[3..6], &["tunnel", "--no-autoupdate", "--metrics"]);
            assert!(fields[6].starts_with("127.0.0.1:"));
            assert_eq!(&fields[7..], &["--grace-period", "100ms", "run", "true"]);
            let pid = fields[0].parse().unwrap();
            assert_owned("generated ingress config", Path::new(fields[2]));
            let config = fs::read_to_string(fields[2]).unwrap();
            assert!(!config.contains(FAKE_SECRET));
            // Verification is the connector default; the key is absent, never written as false.
            assert!(!config.contains("noTLSVerify"));
            assert!(config.contains(&format!("originServerName: '{ORIGIN_NAME}'")));
            assert!(config.contains(&format!("caPool: '{}'", ca_path.display())));
            assert!(config.contains(&format!("https://127.0.0.1:{origin_port}")));
            assert!(config.ends_with("http_status:404\n"));

            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            assert!(!process.is_null());
            let process = unsafe { OwnedHandle::from_raw_handle(process) };
            let unpublish_started = std::time::Instant::now();
            timeout(LEG, async {
                connector.stop().await.unwrap();
                assert!(!connector.ready().await);
                assert_eq!(fake.client.remove(&route, &id).await, Ok(Removal::Absent));
            })
            .await
            .expect("unpublish exceeded its deadline");
            let unpublish_elapsed = unpublish_started.elapsed();
            drop(connector);
            let mut code = 0;
            assert_ne!(
                unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) },
                0
            );
            assert_eq!(
                code, 1,
                "uncooperative child must exit through Job termination"
            );

            fs::write(root.join("crash.flag"), "").unwrap();
            let restart_started = std::time::Instant::now();
            let error = match timeout(LEG, Connector::start(&plan))
                .await
                .expect("crash and restart exceeded their deadline")
            {
                Err(error) => error,
                Ok(_) => panic!("crashing fake reached readiness"),
            };
            assert_eq!(error, ConnectorError::Crashed);
            assert!(!format!("{error:?}").contains(FAKE_SECRET));
            assert_eq!(
                fs::read_to_string(&witness_path).unwrap().lines().count(),
                3
            );
            assert_eq!(fake.state.lock().unwrap().post_count, 1);
            let restart_elapsed = restart_started.elapsed();
            eprintln!(
                "Publication leg timing: publish {publish_elapsed:?}, unpublish {unpublish_elapsed:?}, crash/restart {restart_elapsed:?}"
            );
            assert!(publish_elapsed <= LEG && unpublish_elapsed <= LEG && restart_elapsed <= LEG);
            origin_task.abort();
        }
    }
}

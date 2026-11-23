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
    use tokio::{net::TcpListener, task::JoinHandle, time::timeout};

    const ZONE: &str = "1234567890abcdef1234567890abcdef";
    const TUNNEL: &str = "12345678-1234-4321-8234-123456789abc";
    const NAME: &str = "site.example.invalid";
    const NONCE: &str = "11223344556677889900aabbccddeeff";
    const ID: &str = "aabbccddeeff00112233445566778899";
    const FOREIGN_ID: &str = "99aabbccddeeff001122334455667788";
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
            os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
            path::{Path, PathBuf},
            process::Command,
        };

        use arxd::vm::paths::ArtifactPin;
        use rcgen::{CertificateParams, CertifiedIssuer, IsCa, KeyPair};
        use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
        use sha2::{Digest, Sha256};
        use tokio_rustls::TlsAcceptor;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        use super::publish::connector::{Connector, ConnectorError, Plan, TunnelCredential};
        use super::*;

        const ORIGIN_NAME: &str = "generation.example.invalid";
        const FAKE_SECRET: &str = "SyntheticDontPrint012345678901234567890abcA=";

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

        fn protect_fixture(root: &Path) {
            use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
            use windows_sys::Win32::{
                Foundation::LocalFree,
                Security::{Authorization::*, *},
                System::Threading::{GetCurrentProcess, OpenProcessToken},
            };
            // Protect the fixture if hosted CI uses shared scratch.
            unsafe {
                let mut token = null_mut();
                assert_ne!(
                    OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
                    0
                );
                let token = OwnedHandle::from_raw_handle(token);
                let mut size = 0;
                GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size);
                let mut user = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
                assert_ne!(
                    GetTokenInformation(
                        token.as_raw_handle(),
                        TokenUser,
                        user.as_mut_ptr().cast(),
                        size,
                        &mut size
                    ),
                    0
                );
                let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
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

        #[tokio::test]
        async fn connector_publish_unpublish_receipt_and_restart_are_bounded() {
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
            let contents =
                json!({"AccountTag": ZONE, "TunnelID": TUNNEL, "TunnelSecret": FAKE_SECRET});
            fs::write(&credential_path, contents.to_string()).unwrap();
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
            let config = fs::read_to_string(fields[2]).unwrap();
            assert!(!config.contains(FAKE_SECRET));
            assert!(config.contains("noTLSVerify: false"));
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

use super::*;
use crate::{
    dashboard::{
        request_key,
        tests::{Fixture, ORIGIN, body},
    },
    development::{Execution, ResultData},
    store::{Outcome, Reason, StopReason},
    vm::Clock,
};
use axum::{
    body::Body,
    http::{HeaderValue, Method, StatusCode},
};
use serde_json::Value;
use tokio::{
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::{
    WebSocketStream, client_async,
    tungstenite::{Message as ClientMessage, client::IntoClientRequest},
};

type Peer = WebSocketStream<TcpStream>;

struct Wire {
    listener: JoinHandle<()>,
    address: std::net::SocketAddr,
}

impl Wire {
    async fn new(fixture: &Fixture) -> Self {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = tcp.local_addr().unwrap();
        let router = fixture.app.clone();
        let listener = tokio::spawn(async move {
            let mut children = JoinSet::new();
            loop {
                tokio::select! {
                    _ = children.join_next(), if !children.is_empty() => {}
                    conn = tcp.accept() => {
                        let (socket, _) = conn.unwrap();
                        children.spawn(crate::dashboard::transport::serve(socket, router.clone()));
                    }
                }
            }
        });
        Self { listener, address }
    }

    async fn connect(
        &self,
        path: &str,
        cookie: Option<&str>,
        origin: Option<&str>,
    ) -> Result<Peer, WireError> {
        let mut request = format!("ws://localhost:4444{path}")
            .into_client_request()
            .unwrap();
        for (name, value) in [("cookie", cookie), ("origin", origin)] {
            if let Some(value) = value {
                request.headers_mut().insert(name, value.parse().unwrap());
            }
        }
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_static("Bearer fixture-agent"),
        );
        client_async(request, TcpStream::connect(self.address).await.unwrap())
            .await
            .map(|(peer, _)| peer)
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        self.listener.abort();
    }
}

async fn send(peer: &mut Peer, value: Value) {
    peer.send(ClientMessage::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn receive(peer: &mut Peer) -> ClientMessage {
    timeout(Duration::from_secs(2), peer.next())
        .await
        .expect("server frame deadline")
        .expect("server frame")
        .expect("valid server frame")
}

async fn receive_json(peer: &mut Peer) -> Value {
    serde_json::from_str(&receive(peer).await.into_text().unwrap()).unwrap()
}

async fn close_code(peer: &mut Peer) -> u16 {
    let ClientMessage::Close(Some(close)) = receive(peer).await else {
        panic!("expected close")
    };
    close.code.into()
}

async fn post(f: &Fixture, path: &str, cookie: &str, csrf: &str) -> Response {
    f.send(
        f.request(Method::POST, path, Some(ORIGIN))
            .header("cookie", cookie)
            .header("x-arx-csrf", csrf),
        Body::empty(),
    )
    .await
}

async fn ticket(f: &Fixture, path: &str, cookie: &str, csrf: &str) -> (String, String) {
    let response = post(f, path, cookie, csrf).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value = body(response).await;
    (
        value["streamPath"].as_str().unwrap().to_owned(),
        value["ticket"].as_str().unwrap().to_owned(),
    )
}

async fn logout(f: &Fixture, cookie: &str, csrf: &str) {
    assert_eq!(
        f.send(
            f.request(Method::DELETE, "/api/v1/session", Some(ORIGIN))
                .header("cookie", cookie)
                .header("x-arx-csrf", csrf),
            Body::empty()
        )
        .await
        .status(),
        204
    );
}

fn output(reason: StopReason, stdout: String, stderr: String) -> ResultData {
    ResultData {
        execution_mode: "current_user",
        claim: "fixture",
        dedicated: false,
        stop_reason: reason,
        exit_code: (reason == StopReason::Exited).then_some(0),
        cleanup_confirmed: true,
        matches_current_user: true,
        stdout,
        stderr,
        stdout_dropped_bytes: 0,
        stderr_dropped_bytes: 0,
    }
}

fn terminal_fixture(executor: Arc<Execution>) -> (Fixture, String) {
    let f = Fixture::with_streams(false, Backends::new(Some(executor)));
    f.activity.cache_discovery(
        f.activity.discovery_observation().unwrap(),
        &json!({
            "execution_mode":"current_user", "outcome":"succeeded",
            "tools":[{"name":"git","found":true},{"name":"node","found":false}]
        }),
    );
    let path = format!("/api/v1/hosts/{}/terminal", f.activity.host_id().unwrap());
    (f, path)
}

fn http_status(result: Result<Peer, WireError>) -> u16 {
    match result {
        Err(WireError::Http(response)) => response.status().as_u16(),
        _ => panic!("expected HTTP rejection"),
    }
}

#[tokio::test]
async fn wire_ticket_bound_to_origin_cookie_route_and_single_use() {
    let (f, route) = terminal_fixture(Execution::owner_fixture(|_, _| panic!("no runs")));
    let (cookie, csrf) = f.login(None).await;
    let (other_cookie, _) = f.login(None).await;
    assert_eq!(post(&f, &route, &cookie, "wrong").await.status(), 403);
    assert_eq!(post(&f, &route, &other_cookie, &csrf).await.status(), 403);
    assert_eq!(
        post(&f, "/api/v1/hosts/unknown/terminal", &cookie, &csrf)
            .await
            .status(),
        404
    );
    let (path, proof) = ticket(&f, &route, &cookie, &csrf).await;
    let (other_path, _) = ticket(&f, &route, &cookie, &csrf).await;
    let wire = Wire::new(&f).await;
    assert_eq!(
        http_status(
            wire.connect(&path, Some(&cookie), Some("http://wrong.invalid"))
                .await
        ),
        403
    );
    assert_eq!(
        http_status(wire.connect(&path, Some(&cookie), None).await),
        403
    );
    assert_eq!(
        http_status(wire.connect(&path, None, Some(ORIGIN)).await),
        401
    );
    assert_eq!(
        http_status(
            wire.connect(
                &format!("{path}?ticket=fixture"),
                Some(&cookie),
                Some(ORIGIN)
            )
            .await
        ),
        400
    );
    for (path, cookie) in [(&other_path, &cookie), (&path, &other_cookie)] {
        let mut peer = wire
            .connect(path, Some(cookie), Some(ORIGIN))
            .await
            .unwrap();
        send(&mut peer, json!({"type":"auth","ticket":proof})).await;
        assert_eq!(close_code(&mut peer).await, 4408);
    }
    let mut broken = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(
        &mut broken,
        json!({"type":"auth","ticket":proof,"extra":true}),
    )
    .await;
    assert_eq!(close_code(&mut broken).await, 4408);
    let mut wrong_proof = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(
        &mut wrong_proof,
        json!({"type":"auth","ticket":"e".repeat(64)}),
    )
    .await;
    assert_eq!(close_code(&mut wrong_proof).await, 4408);
    let mut first = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(&mut first, json!({"type":"auth","ticket":proof})).await;
    assert_eq!(receive_json(&mut first).await["type"], "ready");
    let mut replay = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(&mut replay, json!({"type":"auth","ticket":proof})).await;
    assert_eq!(close_code(&mut replay).await, 4408);
    logout(&f, &cookie, &csrf).await;
    assert_eq!(close_code(&mut first).await, 4401);
}

#[tokio::test]
async fn wire_terminal_exercises_cancel_shared_busy_fence_and_content_free_audit() {
    let (entered, mut running) = tokio::sync::mpsc::unbounded_channel();
    let (release, proceed) = std::sync::mpsc::channel();
    let proceed = std::sync::Mutex::new(proceed);
    let executor = Execution::owner_fixture(move |request, cancel| {
        if request.args == ["hold"] {
            entered.send(()).unwrap();
            proceed
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            assert!(cancel.is_cancelled());
            Ok(output(StopReason::Cancelled, String::new(), String::new()))
        } else {
            Ok(output(
                StopReason::Exited,
                "OUTPUT_SENTINEL".repeat(300),
                "é".repeat(500),
            ))
        }
    });
    let (f, route) = terminal_fixture(executor.clone());
    let (cookie, csrf) = f.login(None).await;
    let (path, proof) = ticket(&f, &route, &cookie, &csrf).await;
    let wire = Wire::new(&f).await;
    let mut peer = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(&mut peer, json!({"type":"auth","ticket":proof})).await;
    assert_eq!(receive_json(&mut peer).await["type"], "ready");
    send(
        &mut peer,
        json!({"type":"run","tool":"git","args":["ARGUMENT_SENTINEL"]}),
    )
    .await;
    let start = receive_json(&mut peer).await;
    assert_eq!(start["type"], "started");
    assert_eq!(start["args"], json!(["ARGUMENT_SENTINEL"]));
    let mut total = 0;
    loop {
        let value = receive_json(&mut peer).await;
        assert_eq!(value["commandId"], start["commandId"]);
        match value["type"].as_str().unwrap() {
            "output" => total += value["data"].as_str().unwrap().len(),
            "exit" => {
                assert_eq!(value["outcome"], "succeeded");
                assert_eq!(value["exitCode"], 0);
                assert_eq!(value["truncated"], true);
                break;
            }
            _ => panic!("expected output then exit"),
        }
    }
    assert_eq!(total, 4096);
    send(&mut peer, json!({"type":"run","tool":"node","args":[]})).await;
    assert_eq!(receive_json(&mut peer).await["code"], "invalid_arguments");
    send(
        &mut peer,
        json!({"type":"run","tool":"git","args":["hold"]}),
    )
    .await;
    assert_eq!(receive_json(&mut peer).await["type"], "started");
    running.recv().await.unwrap();
    let agent = f
        .activity
        .start_execution("owner", "discover_tools", executor.identity_mode())
        .unwrap();
    let busy = executor
        .dispatch_admitted(executor.admit().unwrap(), None, f.activity.clone(), agent)
        .await;
    assert_eq!(busy.unwrap_err(), Reason::Busy);
    send(&mut peer, json!({"type":"cancel"})).await;
    send(&mut peer, json!({"type":"run","tool":"git","args":[]})).await;
    assert_eq!(receive_json(&mut peer).await["code"], "busy");
    release.send(()).unwrap();
    assert_eq!(receive_json(&mut peer).await["outcome"], "interrupted");
    let activity = f
        .send(
            f.request(Method::GET, "/api/v1/activity", None)
                .header("cookie", &cookie),
            Body::empty(),
        )
        .await;
    let activity = body(activity).await;
    let rows = activity["entries"].as_array().unwrap();
    let owner: Vec<_> = rows
        .iter()
        .filter(|row| row["actor"]["kind"] == "owner")
        .map(|row| row["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(
        owner,
        [
            Outcome::Refused,
            Outcome::Interrupted,
            Outcome::Refused,
            Outcome::Succeeded
        ]
        .map(Outcome::as_str)
    );
    let raw = std::fs::read(f._dir.path().join("activity.sqlite3")).unwrap();
    for marker in [b"ARGUMENT_SENTINEL".as_slice(), b"OUTPUT_SENTINEL"] {
        assert!(!raw.windows(marker.len()).any(|window| window == marker));
    }
    let read = f.activity.start("owner", "list_activity").unwrap();
    let page = f
        .activity
        .list(
            "owner",
            read,
            &crate::store::ListActivity {
                limit: 50,
                before_id: None,
                include_reads: true,
            },
        )
        .unwrap();
    assert!(
        !page
            .activities
            .iter()
            .any(|row| row.tool.as_str() == "run_command")
    );
}

async fn registered(f: &Fixture, target: Target) -> (Connection, String, String, String, String) {
    let (cookie, csrf) = f.login(None).await;
    let key = key(&cookie);
    let (id, proof) = f
        .sessions
        .mint(key, Some(&csrf), ORIGIN, target, f.clock.as_ref())
        .ok()
        .unwrap();
    let socket = f
        .sessions
        .register_stream(key, f.clock.clone())
        .ok()
        .unwrap();
    (socket, cookie, csrf, id, proof)
}

fn key(cookie: &str) -> Option<[u8; 32]> {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("cookie", cookie.parse().unwrap());
    request_key(&headers)
}

#[tokio::test(start_paused = true)]
async fn revision_and_commit_share_the_live_session_and_clear_unsent_console_image() {
    let f = Fixture::with_streams(false, Backends::new(None));
    let target = Target::Console {
        vm: [1; 16],
        generation: 2,
    };
    let (connection, cookie, csrf, id, proof) = registered(&f, target.clone()).await;
    assert_eq!(connection.redeem(&id, &proof, ORIGIN).ok(), Some(target));
    let stream = connection.stream();
    let binary = Message::Binary(axum::body::Bytes::from_static(b"buffered image"));
    stream.queue_console([message(json!({"type":"frame"})), binary]);
    let (sent, mut frames) = tokio::sync::mpsc::unbounded_channel();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let copy = gate.clone();
    let sink = Box::pin(futures_util::sink::unfold(true, move |first, message| {
        let sent = sent.clone();
        let gate = copy.clone();
        async move {
            sent.send(message).unwrap();
            if first {
                gate.acquire().await.unwrap().forget();
            }
            Ok::<_, ()>(false)
        }
    }));
    let copy = stream.clone();
    let worker = tokio::spawn(async move { writer(&copy, sink).await });
    assert!(matches!(frames.recv().await.unwrap(), Message::Text(_)));
    logout(&f, &cookie, &csrf).await;
    stream.queue([message(json!({"type":"post-revocation"}))]);
    gate.add_permits(1);
    assert!(
        matches!(frames.recv().await.unwrap(), Message::Close(Some(close)) if close.code == 4401)
    );
    worker.await.unwrap();
    assert!(frames.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn stalled_writer_drops_by_revocation_deadline() {
    let f = Fixture::with_streams(false, Backends::new(None));
    let (connection, cookie, csrf, id, proof) =
        registered(&f, Target::Terminal("fixture".into())).await;
    assert!(connection.redeem(&id, &proof, ORIGIN).is_ok());
    let stream = connection.stream();
    stream.queue([message(json!({"type":"ready"}))]);
    let (entered, mut busy) = tokio::sync::mpsc::unbounded_channel();
    let sink = Box::pin(futures_util::sink::unfold((), move |(), _| {
        entered.send(()).unwrap();
        pending::<Result<(), ()>>()
    }));
    let task = tokio::spawn(async move { writer(&stream, sink).await });
    busy.recv().await.unwrap();
    logout(&f, &cookie, &csrf).await;
    f.clock.advance(Duration::from_secs(5));
    tokio::time::advance(Duration::from_secs(5)).await;
    timeout(Duration::from_millis(5), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn expired_tickets_and_old_idle_callbacks_cannot_change_authority() {
    let f = Fixture::with_streams(false, Backends::new(None));
    let target = Target::Console {
        vm: [1; 16],
        generation: 1,
    };
    let (expired, cookie, csrf, id, proof) = registered(&f, target.clone()).await;
    f.clock.advance(Duration::from_secs(30));
    assert!(matches!(expired.redeem(&id, &proof, ORIGIN), Err(close) if close.code == 4408));
    let key = key(&cookie);
    let (id, proof) = f
        .sessions
        .mint(key, Some(&csrf), ORIGIN, target, f.clock.as_ref())
        .ok()
        .unwrap();
    let connection = f
        .sessions
        .register_stream(key, f.clock.clone())
        .ok()
        .unwrap();
    assert!(connection.redeem(&id, &proof, ORIGIN).is_ok());
    let stream = connection.stream();
    let original_deadline = stream.deadline().unwrap();
    let almost_idle = Duration::from_secs(12 * 3600 - 31);
    f.clock.advance(almost_idle);
    let renewal = f
        .send(
            f.request(Method::GET, "/api/v1/session", None)
                .header("cookie", &cookie),
            Body::empty(),
        )
        .await;
    assert_eq!(renewal.status(), 200);
    f.clock.advance(Duration::from_secs(1));
    assert_eq!(f.clock.now().monotonic, original_deadline);
    let renewed = stream.deadline().unwrap();
    assert!(renewed > original_deadline);
    stream.queue([message(json!({"type":"ready"}))]);
    assert!(matches!(stream.commit(), Some(Write::Message(..))));
    assert_eq!(stream.deadline(), Some(renewed));
    f.clock.advance(Duration::from_secs(12 * 3600));
    assert!(matches!(expired.redeem(&id, &proof, ORIGIN), Err(close) if close.code == 4401));
    assert!(matches!(stream.commit(), Some(Write::Close(close, _)) if close.code == 4401));
}

#[tokio::test(start_paused = true)]
async fn expiry_and_logout_win_before_process_start() {
    let executor = Execution::owner_fixture(|_, _| panic!("revoked command attempted spawn"));
    let (f, _) = terminal_fixture(executor.clone());
    let target = Target::Terminal(f.activity.host_id().unwrap());
    let (cookie, csrf) = f.login(None).await;
    let key_a = key(&cookie);
    f.clock.advance(Duration::from_secs(12 * 3600 - 1));
    let (id, proof) = f
        .sessions
        .mint(key_a, Some(&csrf), ORIGIN, target.clone(), f.clock.as_ref())
        .ok()
        .unwrap();
    let connection = f
        .sessions
        .register_stream(key_a, f.clock.clone())
        .ok()
        .unwrap();
    connection.redeem(&id, &proof, ORIGIN).ok().unwrap();
    let stream = connection.stream();
    let (admission, cancel) = stream.admit_terminal(|| executor.admit()).unwrap();
    let cancel = cancel.unwrap();
    f.clock.advance(Duration::from_secs(1));
    assert!(stream.deadline().is_none());
    assert!(cancel.is_cancelled());
    assert!(
        stream
            .admit_terminal::<()>(|| panic!("expired admission callback"))
            .is_none()
    );
    let request = serde_json::from_value(json!({"tool":"git"})).unwrap();
    assert!(matches!(
        executor.run_owner_command(&request, &cancel, || panic!("expired start")),
        Err(Reason::Shutdown)
    ));
    drop(admission);
    let (connection, cookie, csrf, id, proof) = registered(&f, target).await;
    logout(&f, &cookie, &csrf).await;
    assert!(matches!(connection.redeem(&id, &proof, ORIGIN), Err(close) if close.code == 4401));
}

#[tokio::test]
async fn terminal_ticket_cannot_authorize_a_console_or_another_vm() {
    let f = Fixture::with_streams(false, Backends::new(None));
    let (console, cookie, csrf, console_id, console_proof) = registered(
        &f,
        Target::Console {
            vm: [1; 16],
            generation: 1,
        },
    )
    .await;
    let key = key(&cookie);
    let (_, terminal_proof) = f
        .sessions
        .mint(
            key,
            Some(&csrf),
            ORIGIN,
            Target::Terminal("fixture".into()),
            f.clock.as_ref(),
        )
        .ok()
        .unwrap();
    let (_, other_vm_proof) = f
        .sessions
        .mint(
            key,
            Some(&csrf),
            ORIGIN,
            Target::Console {
                vm: [2; 16],
                generation: 1,
            },
            f.clock.as_ref(),
        )
        .ok()
        .unwrap();
    for proof in [terminal_proof, other_vm_proof] {
        assert!(
            matches!(console.redeem(&console_id, &proof, ORIGIN), Err(close) if close.code == 4408)
        );
    }
    assert_eq!(
        console.redeem(&console_id, &console_proof, ORIGIN).ok(),
        Some(Target::Console {
            vm: [1; 16],
            generation: 1
        })
    );
}

#[tokio::test(start_paused = true)]
async fn missing_first_message_times_out_and_shutdown_drains_a_pending_upgrade() {
    let (f, route) = terminal_fixture(Execution::owner_fixture(|_, _| panic!("no runs")));
    let (cookie, csrf) = f.login(None).await;
    let (path, _) = ticket(&f, &route, &cookie, &csrf).await;
    let (client, server) = tokio::io::duplex(64 * 1024);
    let request = format!("ws://localhost:4444{path}")
        .into_client_request()
        .unwrap();
    let (mut parts, ()) = request.into_parts();
    parts.headers.insert("cookie", cookie.parse().unwrap());
    parts.headers.insert("origin", ORIGIN.parse().unwrap());
    let server = tokio::spawn(crate::dashboard::transport::serve(server, f.app.clone()));
    let (mut peer, _) = client_async(axum::http::Request::from_parts(parts, ()), client)
        .await
        .unwrap();
    f.clock.advance(AUTH_TIMEOUT);
    tokio::time::advance(AUTH_TIMEOUT).await;
    assert!(
        matches!(peer.next().await.unwrap().unwrap(), ClientMessage::Close(Some(frame)) if u16::from(frame.code) == 4408)
    );
    server.await.unwrap();

    let registered = f
        .sessions
        .register_stream(key(&cookie), f.clock.clone())
        .ok()
        .unwrap();
    let stream = registered.stream();
    let sessions = f.sessions.clone();
    let clock = f.clock.clone();
    let drain = tokio::spawn(async move { sessions.stop_streams(clock.as_ref()).await });
    let mut revision = stream.revision().unwrap();
    if !stream.closing() {
        revision.changed().await.unwrap();
    }
    assert!(matches!(stream.commit(), Some(Write::Close(close, _)) if close.code == 1001));
    assert!(
        f.sessions
            .register_stream(key(&cookie), f.clock.clone())
            .is_err()
    );
    drop(registered);
    assert!(drain.await.unwrap().is_ok());
}

#[tokio::test]
async fn oversized_fragmented_message_is_not_delivered_to_the_executor() {
    use tokio_tungstenite::tungstenite::protocol::frame::{
        Frame,
        coding::{Data, OpCode},
    };
    let (f, route) = terminal_fixture(Execution::owner_fixture(|_, _| panic!("no runs")));
    let (cookie, csrf) = f.login(None).await;
    let (path, proof) = ticket(&f, &route, &cookie, &csrf).await;
    let wire = Wire::new(&f).await;
    let mut peer = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(&mut peer, json!({"type":"auth","ticket":proof})).await;
    assert_eq!(receive_json(&mut peer).await["type"], "ready");
    for (len, opcode, end) in [
        (TEXT_LIMIT / 2, Data::Text, false),
        (TEXT_LIMIT / 2 + 1, Data::Continue, true),
    ] {
        peer.send(ClientMessage::Frame(Frame::message(
            vec![b'x'; len],
            OpCode::Data(opcode),
            end,
        )))
        .await
        .unwrap();
    }
    assert_eq!(close_code(&mut peer).await, 1009);
}

struct Display {
    generation: std::sync::atomic::AtomicU64,
    stopped: std::sync::atomic::AtomicBool,
}

struct Sources(Arc<Display>);

impl console::Sources for Sources {
    fn lease(&self, vm: [u8; 16]) -> Option<Arc<dyn console::Lease>> {
        (vm == [1; 16]).then(|| self.0.clone() as Arc<dyn console::Lease>)
    }
}

impl console::Lease for Display {
    fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }

    fn state(&self) -> Result<(), crate::vm::console::CaptureError> {
        if self.stopped.load(std::sync::atomic::Ordering::Acquire) {
            return Err(crate::vm::console::CaptureError::Stopped);
        }
        Ok(())
    }

    fn capture(
        &self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Option<crate::vm::console::Frame>,
                        crate::vm::console::CaptureError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            let bytes = hex::decode(crate::vm::console::tests::PNG).unwrap().into();
            crate::vm::console::Frame::from_png(bytes).map(Some)
        })
    }
}

#[tokio::test]
async fn wire_console_returns_one_png_pair_then_vm_stop() {
    let display = Arc::new(Display {
        generation: 1.into(),
        stopped: false.into(),
    });
    let mut backends = Backends::new(None);
    backends.console = Some(Arc::new(Sources(display.clone())));
    let f = Fixture::with_streams(true, backends);
    let (cookie, csrf) = f.login(None).await;
    let route = format!("/api/v1/vms/{}/console", hex::encode([1; 16]));
    let unavailable = Fixture::with_streams(true, Backends::new(None));
    let (unavailable_cookie, unavailable_csrf) = unavailable.login(None).await;
    assert_eq!(
        post(&unavailable, &route, &unavailable_cookie, &unavailable_csrf)
            .await
            .status(),
        503
    );
    let (path, proof) = ticket(&f, &route, &cookie, &csrf).await;
    let wire = Wire::new(&f).await;
    let mut peer = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(&mut peer, json!({"type":"auth","ticket":proof})).await;
    let meta = receive_json(&mut peer).await;
    assert_eq!(
        (
            meta["seq"].as_u64(),
            meta["width"].as_u64(),
            meta["height"].as_u64()
        ),
        (Some(1), Some(2), Some(3))
    );
    assert_eq!(meta["mime"], "image/png");
    assert!(
        matches!(receive(&mut peer).await, ClientMessage::Binary(bytes) if bytes == hex::decode(crate::vm::console::tests::PNG).unwrap())
    );
    display
        .stopped
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(
        receive_json(&mut peer).await,
        json!({"type":"closed","reason":"VM stopped"})
    );
    assert_eq!(close_code(&mut peer).await, 1000);
    display
        .stopped
        .store(false, std::sync::atomic::Ordering::Release);
    let (path, proof) = ticket(&f, &route, &cookie, &csrf).await;
    display
        .generation
        .store(2, std::sync::atomic::Ordering::Release);
    let mut replaced = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(&mut replaced, json!({"type":"auth","ticket":proof})).await;
    assert_eq!(close_code(&mut replaced).await, 4403);
}

#[tokio::test(start_paused = true)]
async fn one_capture_job_fans_out_while_a_slow_viewer_loses_its_writer() {
    let vm = [1; 16];
    let generation = 1;
    let target = Target::Console { vm, generation };
    let display = Arc::new(Display {
        generation: generation.into(),
        stopped: false.into(),
    });
    let mut backends = Backends::new(None);
    backends.console = Some(Arc::new(Sources(display)));
    let f = Fixture::with_streams(false, backends);
    let (slow, _, _, slow_id, slow_proof) = registered(&f, target.clone()).await;
    let (fast, _, _, fast_id, fast_proof) = registered(&f, target.clone()).await;
    assert!(slow.redeem(&slow_id, &slow_proof, ORIGIN).is_ok());
    assert!(fast.redeem(&fast_id, &fast_proof, ORIGIN).is_ok());
    let slow_stream = slow.stream();
    let fast_stream = fast.stream();
    let (sent, mut entering_send) = tokio::sync::mpsc::unbounded_channel();
    let sink = Box::pin(futures_util::sink::unfold((), move |(), frame| {
        sent.send(frame).unwrap();
        pending::<Result<(), ()>>()
    }));
    let slow_writer = tokio::spawn(async move {
        writer(&slow_stream, sink).await;
        drop(slow);
    });
    assert!(console::check(&f.state, &fast_stream, vm, generation));
    assert!(f.sessions.capture(target, f.clock.clone()).is_none());
    let Some(Message::Text(slow_meta)) = entering_send.recv().await else {
        panic!("no slow frame")
    };
    let Some(Write::Message(Message::Text(fast_meta), _)) = fast_stream.commit() else {
        panic!("no fast frame")
    };
    assert_eq!(slow_meta, fast_meta);
    assert!(matches!(
        fast_stream.commit(),
        Some(Write::Message(Message::Binary(_), _))
    ));
    assert!(matches!(fast_stream.commit(), Some(Write::Wait)));
    f.clock.advance(Duration::from_secs(1));
    tokio::time::advance(Duration::from_secs(1)).await;
    let mut revision = fast_stream.revision().unwrap();
    let (meta, _) = loop {
        match fast_stream.commit() {
            Some(Write::Message(Message::Text(meta), deadline)) => break (meta, deadline),
            Some(Write::Wait) => revision.changed().await.unwrap(),
            _ => panic!("missing next frame"),
        }
    };
    assert_eq!(serde_json::from_str::<Value>(&meta).unwrap()["seq"], 2);
    assert!(matches!(
        fast_stream.commit(),
        Some(Write::Message(Message::Binary(_), _))
    ));
    assert!(entering_send.try_recv().is_err());
    f.clock.advance(Duration::from_secs(4));
    tokio::time::advance(Duration::from_secs(4)).await;
    slow_writer.await.unwrap();
    assert!(entering_send.recv().await.is_none());
    drop(fast);
}

#[tokio::test]
async fn disconnected_command_cancels_and_finishes_owner_activity() {
    let (entered, mut running) = tokio::sync::mpsc::unbounded_channel();
    let (release, proceed) = std::sync::mpsc::channel();
    let proceed = std::sync::Mutex::new(proceed);
    let executor = Execution::owner_fixture(move |_, cancel| {
        entered.send(()).unwrap();
        proceed
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(cancel.is_cancelled());
        Ok(output(StopReason::Cancelled, String::new(), String::new()))
    });
    let (f, _) = terminal_fixture(executor.clone());
    let target = Target::Terminal(f.activity.host_id().unwrap());
    let (connection, _, _, id, proof) = registered(&f, target).await;
    assert!(connection.redeem(&id, &proof, ORIGIN).is_ok());
    assert!(
        terminal::receive(
            &f.state,
            &connection.stream(),
            &json!({"type":"run","tool":"git","args":[]}).to_string()
        )
        .await
    );
    timeout(Duration::from_secs(2), running.recv())
        .await
        .unwrap()
        .unwrap();
    drop(connection);
    release.send(()).unwrap();
    assert!(executor.drain().await);
    let entries = f
        .activity
        .dashboard_list(
            &crate::store::ListActivity {
                limit: 20,
                before_id: None,
                include_reads: false,
            },
            None,
            None,
        )
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor, "owner");
    assert_eq!(entries[0].outcome, Outcome::Interrupted);
}

#[tokio::test]
async fn audit_failure_after_started_ends_the_command_without_output() {
    let (release, proceed) = std::sync::mpsc::channel::<std::path::PathBuf>();
    let proceed = std::sync::Mutex::new(proceed);
    let executor = Execution::owner_fixture(move |_, _| {
        let database = proceed
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        // Remove the started owner row so its terminal audit cannot commit.
        let raw = rusqlite::Connection::open(database).unwrap();
        assert_eq!(
            raw.execute("DELETE FROM activity WHERE actor_kind='owner'", [])
                .unwrap(),
            1
        );
        Ok(output(
            StopReason::Exited,
            "OUTPUT_SENTINEL".into(),
            String::new(),
        ))
    });
    let (f, route) = terminal_fixture(executor.clone());
    let (cookie, csrf) = f.login(None).await;
    let (path, proof) = ticket(&f, &route, &cookie, &csrf).await;
    let wire = Wire::new(&f).await;
    let mut peer = wire
        .connect(&path, Some(&cookie), Some(ORIGIN))
        .await
        .unwrap();
    send(&mut peer, json!({"type":"auth","ticket":proof})).await;
    assert_eq!(receive_json(&mut peer).await["type"], "ready");
    send(&mut peer, json!({"type":"run","tool":"git","args":[]})).await;
    assert_eq!(receive_json(&mut peer).await["type"], "started");
    release
        .send(f._dir.path().join("activity.sqlite3"))
        .unwrap();
    // A content-free error, then the fixed close; never output or exit.
    assert_eq!(
        receive_json(&mut peer).await,
        json!({"type":"error","code":"storage_failed",
            "message":"The dashboard could not run the command."})
    );
    let ClientMessage::Close(Some(close)) = receive(&mut peer).await else {
        panic!("expected close")
    };
    assert_eq!(u16::from(close.code), 1011);
    assert_eq!(close.reason.as_str(), "command audit failed");
    // The unconfirmed terminal audit keeps the drain signal negative.
    assert!(!executor.drain().await);
}

/// A lease whose capture parks until released and then reports `Stopped`,
/// as an old generation's display does once its VM was replaced.
struct HeldDisplay {
    generation: std::sync::atomic::AtomicU64,
    entered: tokio::sync::mpsc::UnboundedSender<()>,
    gate: tokio::sync::Semaphore,
}

struct HeldSources(Arc<HeldDisplay>);

impl console::Sources for HeldSources {
    fn lease(&self, vm: [u8; 16]) -> Option<Arc<dyn console::Lease>> {
        (vm == [1; 16]).then(|| self.0.clone() as Arc<dyn console::Lease>)
    }
}

impl console::Lease for HeldDisplay {
    fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }

    fn state(&self) -> Result<(), crate::vm::console::CaptureError> {
        Ok(())
    }

    fn capture(
        &self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Option<crate::vm::console::Frame>,
                        crate::vm::console::CaptureError,
                    >,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            self.entered.send(()).unwrap();
            self.gate.acquire().await.unwrap().forget();
            Err(crate::vm::console::CaptureError::Stopped)
        })
    }
}

#[tokio::test(start_paused = true)]
async fn replacement_during_a_held_capture_wins_over_its_stale_stop() {
    let vm = [1; 16];
    let target = Target::Console { vm, generation: 1 };
    let (entered, mut capturing) = tokio::sync::mpsc::unbounded_channel();
    let display = Arc::new(HeldDisplay {
        generation: 1.into(),
        entered,
        gate: tokio::sync::Semaphore::new(0),
    });
    let mut backends = Backends::new(None);
    backends.console = Some(Arc::new(HeldSources(display.clone())));
    let f = Fixture::with_streams(false, backends);
    let (viewer, _, _, id, proof) = registered(&f, target.clone()).await;
    assert!(viewer.redeem(&id, &proof, ORIGIN).is_ok());
    let stream = viewer.stream();
    assert!(console::check(&f.state, &stream, vm, 1));
    timeout(Duration::from_secs(2), capturing.recv())
        .await
        .unwrap()
        .unwrap();
    // The VM is replaced while the old generation's capture is in flight.
    display
        .generation
        .store(2, std::sync::atomic::Ordering::Release);
    let mut revision = stream.revision().unwrap();
    display.gate.add_permits(1);
    let close = loop {
        match stream.commit() {
            Some(Write::Close(close, _)) => break close,
            Some(Write::Wait) => revision.changed().await.unwrap(),
            Some(Write::Message(message, _)) => panic!("stale close notice {message:?}"),
            None => panic!("viewer released"),
        }
    };
    assert_eq!(close.code, 4403);
}

#[tokio::test(start_paused = true)]
async fn detected_replacement_overrides_only_an_untaken_ordinary_close() {
    let f = Fixture::with_streams(false, Backends::new(None));
    let target = Target::Console {
        vm: [1; 16],
        generation: 1,
    };
    let stopped = Close {
        code: 1000,
        reason: "VM stopped",
    };
    let notice = || Some(message(json!({"type":"closed","reason":"VM stopped"})));
    // Not yet taken by the writer: the replacement wins and the notice is dropped.
    let (first, _, _, id, proof) = registered(&f, target.clone()).await;
    assert!(first.redeem(&id, &proof, ORIGIN).is_ok());
    let stream = first.stream();
    stream.close(stopped, notice());
    stream.close(Close::TARGET, None);
    assert!(matches!(stream.commit(), Some(Write::Close(close, _)) if close.code == 4403));
    // Already taken: the ordinary close stands.
    let (second, _, _, id, proof) = registered(&f, target).await;
    assert!(second.redeem(&id, &proof, ORIGIN).is_ok());
    let stream = second.stream();
    stream.close(stopped, notice());
    assert!(matches!(
        stream.commit(),
        Some(Write::Message(Message::Text(_), _))
    ));
    stream.close(Close::TARGET, None);
    assert!(matches!(stream.commit(), Some(Write::Close(close, _)) if close.code == 1000));
    drop((first, second));
}

/// Ignores transport keepalive frames while waiting for the server's close.
async fn close_skipping_keepalive<S>(peer: &mut WebSocketStream<S>) -> u16
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        let frame = timeout(Duration::from_secs(2), peer.next())
            .await
            .expect("server frame deadline")
            .expect("server frame")
            .expect("valid server frame");
        match frame {
            ClientMessage::Ping(_) | ClientMessage::Pong(_) => {}
            ClientMessage::Close(Some(close)) => return close.code.into(),
            other => panic!("unexpected frame before close: {other:?}"),
        }
    }
}

#[tokio::test]
async fn running_command_ends_on_session_expiry_or_shutdown_despite_traffic() {
    for case in ["idle", "absolute", "shutdown"] {
        let (entered, mut running) = tokio::sync::mpsc::unbounded_channel();
        // Gated worker: it keeps running until the stream cancels it.
        let executor = Execution::owner_fixture(move |_, cancel| {
            entered.send(()).unwrap();
            let start = std::time::Instant::now();
            while !cancel.is_cancelled() {
                assert!(start.elapsed() < Duration::from_secs(5), "never cancelled");
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(output(
                StopReason::Cancelled,
                "OUTPUT_SENTINEL".into(),
                String::new(),
            ))
        });
        let (f, route) = terminal_fixture(executor.clone());
        let (cookie, csrf) = f.login(None).await;
        if case == "absolute" {
            // Keep the session idle-fresh over HTTP until 3 h of its 7 d remain.
            for _ in 0..15 {
                f.clock.advance(Duration::from_secs(11 * 3600));
                let renewal = f
                    .send(
                        f.request(Method::GET, "/api/v1/session", None)
                            .header("cookie", &cookie),
                        Body::empty(),
                    )
                    .await;
                assert_eq!(renewal.status(), 200);
            }
        }
        let (path, proof) = ticket(&f, &route, &cookie, &csrf).await;
        let wire = Wire::new(&f).await;
        let mut peer = wire
            .connect(&path, Some(&cookie), Some(ORIGIN))
            .await
            .unwrap();
        send(&mut peer, json!({"type":"auth","ticket":proof})).await;
        assert_eq!(receive_json(&mut peer).await["type"], "ready");
        send(&mut peer, json!({"type":"run","tool":"git","args":[]})).await;
        assert_eq!(receive_json(&mut peer).await["type"], "started");
        timeout(Duration::from_secs(2), running.recv())
            .await
            .unwrap()
            .unwrap();
        // Stream traffic is not session activity. Wait for each pong so the
        // server has consumed the traffic before it closes the socket.
        for _ in 0..3 {
            peer.send(ClientMessage::Ping(Default::default()))
                .await
                .unwrap();
            assert!(matches!(receive(&mut peer).await, ClientMessage::Pong(_)));
        }
        let mut stop = None;
        match case {
            // The jump also passes the 4 h terminal lifetime; the session
            // check runs first, so the stream still ends with 4401.
            "idle" => f.clock.advance(Duration::from_secs(12 * 3600)),
            // Within both the terminal lifetime and the idle window.
            "absolute" => f.clock.advance(Duration::from_secs(3 * 3600)),
            _ => {
                let sessions = f.sessions.clone();
                let clock = f.clock.clone();
                stop = Some(tokio::spawn(async move {
                    sessions.stop_streams(clock.as_ref()).await
                }));
            }
        }
        assert_eq!(
            close_skipping_keepalive(&mut peer).await,
            if case == "shutdown" { 1001 } else { 4401 },
            "{case}"
        );
        assert!(executor.drain().await, "{case}");
        if let Some(stop) = stop {
            assert!(stop.await.unwrap().is_ok());
        }
        let entries = f
            .activity
            .dashboard_list(
                &crate::store::ListActivity {
                    limit: 20,
                    before_id: None,
                    include_reads: false,
                },
                None,
                None,
            )
            .unwrap();
        assert_eq!(entries.len(), 1, "{case}");
        assert_eq!(entries[0].actor, "owner");
        assert_eq!(entries[0].outcome, Outcome::Interrupted, "{case}");
    }
}

#[tokio::test(start_paused = true)]
async fn pre_authentication_control_frames_share_the_socket_budget() {
    let (f, route) = terminal_fixture(Execution::owner_fixture(|_, _| panic!("no runs")));
    let (cookie, csrf) = f.login(None).await;
    let (path, _) = ticket(&f, &route, &cookie, &csrf).await;
    let (client, server) = tokio::io::duplex(64 * 1024);
    let request = format!("ws://localhost:4444{path}")
        .into_client_request()
        .unwrap();
    let (mut parts, ()) = request.into_parts();
    parts.headers.insert("cookie", cookie.parse().unwrap());
    parts.headers.insert("origin", ORIGIN.parse().unwrap());
    let server = tokio::spawn(crate::dashboard::transport::serve(server, f.app.clone()));
    let (mut peer, _) = client_async(axum::http::Request::from_parts(parts, ()), client)
        .await
        .unwrap();
    // Paused time grants no refill: the 17th frame exceeds the 16-frame burst.
    for _ in 0..17 {
        peer.send(ClientMessage::Ping(Default::default()))
            .await
            .unwrap();
    }
    assert_eq!(close_skipping_keepalive(&mut peer).await, 1008);
    server.await.unwrap();
}

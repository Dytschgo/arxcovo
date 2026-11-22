use arx_guest::{Request, Response, ScriptExecution, read_frame, write_frame};
use arxd::{
    config::{Config, VmRuntimeProfile},
    service::Shutdown,
    vm::{
        guest::GuestChannel,
        paths::{Artifact, ArtifactRole, LocalPath},
        qemu_args::{BootPurpose, BootRecipe, NETWORK, PlanKind, SerialName},
        qemu_img,
        qmp::{ChannelError, Qmp, RunStatus},
        runtime::{
            CandidateRuntime, NativeMutation, NativeRefusal, QualificationEvidence,
            StopDisposition, StopEvidence, capacity, fresh_observation,
        },
    },
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};
use tokio::io::{
    AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex, split,
};

fn profile() -> Value {
    json!({
        "enabled": true, "owner_sid": "S-1-5-21-1-2-3-1001",
        "supervisor_sid": "S-1-5-21-1-2-3-1002", "bundle": "E:/pinned bundle",
        "volume": "D:/", "state": "D:/test_vms/state",
        "disk_ceiling_bytes": 10_737_418_240_u64, "manifest": "qemu-11.1.3-windows-x64",
    })
}

fn parse(value: &Value) -> anyhow::Result<VmRuntimeProfile> {
    VmRuntimeProfile::parse(&serde_json::to_vec(value).unwrap())
}

#[test]
fn candidate_profile_is_closed_and_separate_from_foreground() {
    let original = profile();
    assert_eq!(
        parse(&original).unwrap().manifest_id(),
        "qemu-11.1.3-windows-x64"
    );
    for forbidden in [
        "hostfwd",
        "guestfwd",
        "smb",
        "tftp",
        "netdev",
        "args",
        "listen",
        "local",
        "execution",
        "development_execution",
        "service_endpoint",
    ] {
        let mut value = original.clone();
        value[forbidden] = json!("untrusted");
        assert!(parse(&value).is_err(), "{forbidden}");
    }
    for (field, value) in [
        ("bundle", json!("D:/shared")),
        ("state", json!("E:/state")),
        ("manifest", json!("installed-qemu")),
        ("owner_sid", original["supervisor_sid"].clone()),
        ("disk_ceiling_bytes", json!(0)),
    ] {
        let mut profile = original.clone();
        profile[field] = value;
        assert!(parse(&profile).is_err(), "{field}");
    }
    assert!(VmRuntimeProfile::parse(&[b' '; 16_385]).is_err());
    assert!(
        serde_json::from_value::<Config>(json!({
            "state_dir": "D:/state", "agents": [], "test_vms": original,
        }))
        .is_err()
    );
}

#[test]
fn spelling_rejects_aliases_and_overlapping_roots() {
    for raw in [
        r"\\server\share\a",
        r"\\?\D:\vm",
        r"\\.\D:\vm",
        r"D:vm",
        r"D:\vm\.\file",
        r"D:\vm\..\file",
        r"D:\vm\file:stream",
        r"D:\vm\\file",
        r"D:\vm\file.",
        r"D:\vm\file ",
        r"D:\vm\NUL.txt",
        r"D:\vm\COM1.txt",
        r"D:\vm\LONGFI~1",
        "D:/vm/file\0name",
        "D:/vm/hostfwd=tcp",
        "D:/vm/a,b",
        "D:/vm/share;",
    ] {
        assert!(LocalPath::parse(raw).is_err(), "{raw:?}");
    }
    let root = LocalPath::parse("D:/Test VM").unwrap();
    assert!(root.contains(&LocalPath::parse("d:/test vm/disk").unwrap()));
    assert!(!root.overlaps(&LocalPath::parse("D:/Test VM backup").unwrap()));
    assert!(root.join("D:/unowned").is_err());
}

#[test]
fn unavailable_runtime_cannot_dispatch_any_mutation() {
    let enabled = parse(&profile()).unwrap();
    let mut disabled = profile();
    disabled["enabled"] = json!(false);
    let disabled = parse(&disabled).unwrap();
    let effects = Cell::new(0);
    let active_reason = if cfg!(all(windows, target_arch = "x86_64")) {
        NativeRefusal::QualificationMissing
    } else {
        NativeRefusal::UnsupportedPlatform
    };
    for (profile, expected) in [
        (None, NativeRefusal::Disabled),
        (Some(&disabled), NativeRefusal::Disabled),
        (Some(&enabled), active_reason),
    ] {
        let candidate = CandidateRuntime::new(profile);
        for op in [
            NativeMutation::CopyMedia,
            NativeMutation::Boot,
            NativeMutation::CreateImage,
        ] {
            assert_eq!(
                candidate.dispatch(op, |_| effects.set(effects.get() + 1)),
                Err(expected)
            );
        }
    }
    assert_eq!(effects.get(), 0);
}

#[test]
fn qualification_reports_each_missing_mechanism() {
    use NativeRefusal::*;
    let mut evidence = QualificationEvidence::default();
    macro_rules! verify {
        ($field:ident, $refusal:ident) => {
            assert_eq!(evidence.check(), Err($refusal));
            evidence.$field = true;
        };
    }
    verify!(identity, IdentityUnverified);
    verify!(distribution, DistributionUnverified);
    verify!(path_custody, PathCustodyUnverified);
    verify!(job, JobUnverified);
    verify!(whpx, WhpxUnverified);
    verify!(qmp, QmpUnverified);
    verify!(guest_endpoint, GuestEndpointUnverified);
    verify!(disk_ceiling, DiskCeilingUnverified);
    verify!(owner_activity, OwnerActivityUnverified);
    verify!(reserves, ReservesUnverified);
    verify!(network_fence, NetworkFenceUnverified);
    assert_eq!(evidence.check(), Ok(()));
}

#[test]
fn stale_observations_and_uncertain_release_do_not_authorize_work() {
    let start = Instant::now();
    assert_eq!(
        fresh_observation(
            start,
            start + Duration::from_secs(6),
            NativeRefusal::OwnerActivityUnverified
        ),
        Err(NativeRefusal::OwnerActivityUnverified)
    );
    // (allocated, inner_free, outer_free, total, ceiling, floor)
    assert_eq!(capacity(75, 20, 40, 100, 100, 15).unwrap().free_bytes, 35); // min(40, 20 + 15)
    assert_eq!(capacity(75, 20, 25, 100, 100, 15).unwrap().free_bytes, 25); // outer bound
    assert_eq!(
        capacity(75, 26, 25, 100, 100, 15).err(),
        Some(NativeRefusal::DiskCeilingUnverified) // allocated + inner_free > total
    );
    assert_eq!(
        capacity(0, 1, 25, 101, 100, 15).err(),
        Some(NativeRefusal::DiskCeilingUnverified) // total > ceiling
    );
    // A volume exactly at the configured ceiling is admissible.
    assert_eq!(capacity(0, 0, 10, 100, 100, 15).unwrap().free_bytes, 10);
    assert_eq!(
        capacity(0, 0, 10, 0, 100, 15).err(),
        Some(NativeRefusal::DiskCeilingUnverified) // zero-sized volume
    );
    assert_eq!(
        capacity(0, 1, 5, 10, 10, u64::MAX).err(),
        Some(NativeRefusal::DiskCeilingUnverified) // inner_free + floor overflows
    );
    assert_eq!(
        capacity(u64::MAX, 1, 5, u64::MAX, u64::MAX, 0).err(),
        Some(NativeRefusal::DiskCeilingUnverified) // allocated + inner_free overflows
    );
    let mut release = StopEvidence {
        process_exited: true,
        guest_shutdown: true,
        ..Default::default()
    };
    assert_eq!(release.disposition(), StopDisposition::ChargedQuarantine);
    release.job_empty = true;
    release.workers_joined = true;
    assert_eq!(release.disposition(), StopDisposition::ChargedQuarantine);
    release.artifacts_reopened = true;
    assert_eq!(release.disposition(), StopDisposition::DirtyRelease);
    release.clean_process_exit = true;
    assert_eq!(release.disposition(), StopDisposition::CleanRelease);
}

fn artifact(role: ArtifactRole, path: &str) -> Artifact {
    Artifact::new(role, LocalPath::parse(path).unwrap()).unwrap()
}

#[test]
fn qemu_has_one_restricted_network_and_explicit_block_and_monitor_roles() {
    use ArtifactRole::*;
    let image = artifact(Qemu, "E:/bundle/qemu-system-x86_64.exe");
    let data = artifact(Data, "E:/bundle/pc-bios");
    let code = artifact(FirmwareCode, "E:/bundle/edk2-x86_64-code.fd");
    let vars = artifact(Vars, "D:/vms/vars.fd");
    let disk = artifact(GuestDisk, "D:/vms/current.qcow2");
    let backing = artifact(Golden, "E:/golden/base.qcow2");
    let iso = artifact(InstallMedia, "E:/iso/installer.iso");
    let serial = SerialName::generate().unwrap();
    let mut recipe = BootRecipe {
        image: &image,
        data: &data,
        firmware: &code,
        vars: &vars,
        disk: &disk,
        backing: Some(&backing),
        serial: &serial,
        boot: BootPurpose::Installer(&iso),
        ram_mib: 4096,
        vcpus: 2,
    };
    let plan = recipe.plan().unwrap();
    assert_eq!(plan.kind(), PlanKind::Boot);
    assert!(plan.offline_lease().is_none());
    let args = plan.args();
    let values = |key: &str| {
        args.windows(2)
            .filter(|pair| pair[0] == key)
            .map(|p| p[1].as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(values("-netdev"), [NETWORK]);
    assert_eq!(
        values("-machine"),
        ["pc-q35-11.1,pflash0=code,pflash1=vars"]
    );
    assert_eq!(values("-mon"), ["chardev=arxqmp,mode=control"]);
    assert_eq!(values("-accel"), ["whpx"]);
    assert!(args.iter().any(|arg| arg == "-S"));
    assert!(
        values("-chardev")
            .iter()
            .any(|arg| arg == &"stdio,id=arxqmp,signal=off")
    );
    let blocks: Vec<Value> = values("-blockdev")
        .iter()
        .map(|arg| serde_json::from_str(arg).unwrap())
        .collect();
    assert_eq!(blocks.len(), 5);
    assert_eq!(blocks[0]["read-only"], true);
    assert_eq!(blocks[1]["read-only"], false);
    assert_eq!(blocks[2]["backing"], Value::Null);
    assert_eq!(blocks[3]["backing"], "golden");
    assert_eq!(blocks[4]["node-name"], "installer");
    for forbidden in [
        "hostfwd", "guestfwd", "smb", "tftp", "socket", "-qmp", "-vnc",
    ] {
        assert!(args.iter().all(|arg| !arg.contains(forbidden)));
    }
    recipe.backing = Some(&disk);
    assert!(recipe.plan().is_err());
}

/// `CommandPlan` has private fields and no public constructor, so argv can
/// come only from these builders; the accessors are read-only views.
#[test]
fn sealed_plans_expose_read_only_argv_and_a_fixed_kind() {
    let tool = artifact(ArtifactRole::QemuImg, "E:/bundle/qemu-img.exe");
    let golden = artifact(ArtifactRole::Golden, "D:/test_vms/golden.qcow2");
    let mut header = vec![0; 65_536];
    header[..4].copy_from_slice(b"QFI\xfb");
    for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 104)] {
        header[at..at + 4].copy_from_slice(&val.to_be_bytes());
    }
    header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
    let plan = qemu_img::golden_info(&tool, &golden, &header).unwrap();
    assert_eq!(plan.kind(), PlanKind::Inspect);
    assert!(plan.offline_lease().is_none());
    assert_eq!(plan.image().as_str(), r"E:\bundle\qemu-img.exe");
    assert_eq!(
        plan.args(),
        [
            "info",
            "--output=json",
            "-f",
            "qcow2",
            r"D:\test_vms\golden.qcow2"
        ]
    );
    // A role mismatch yields no plan at all.
    assert!(qemu_img::golden_info(&golden, &golden, &header).is_err());
}

#[test]
fn image_metadata_cannot_introduce_external_files_or_backing() {
    let mut header = vec![0; 65_536];
    header[..4].copy_from_slice(b"QFI\xfb");
    for (at, val) in [(4, 3u32), (20, 16), (96, 4), (100, 104)] {
        header[at..at + 4].copy_from_slice(&val.to_be_bytes());
    }
    header[24..32].copy_from_slice(&1_048_576u64.to_be_bytes());
    assert_eq!(qemu_img::inspect_header(&header, None).unwrap(), 1_048_576);
    header[79] = 4; // External data incompatible feature.
    assert!(qemu_img::inspect_header(&header, None).is_err());
    header[79] = 0;
    header[15] = 120;
    header[19] = 10;
    assert!(qemu_img::inspect_header(&header, None).is_err());
    let path = LocalPath::parse("D:/slot/disk.qcow2").unwrap();
    let mut info = json!({
        "filename": path.as_str(), "format": "qcow2", "virtual-size": 1_048_576,
        "actual-size": 65_536, "dirty-flag": false,
    });
    assert_eq!(
        qemu_img::parse_info(
            &serde_json::to_vec(&info).unwrap(),
            &path,
            1_048_576,
            1_048_576,
            None
        )
        .unwrap(),
        65_536
    );
    info["backing-filename"] = json!("http://unowned");
    assert!(
        qemu_img::parse_info(
            &serde_json::to_vec(&info).unwrap(),
            &path,
            1_048_576,
            1_048_576,
            None
        )
        .is_err()
    );
}

const GREETING: &[u8] = b"{\"QMP\":{\"version\":{\"qemu\":{\"major\":11,\"minor\":1,\"micro\":3},\"package\":\"\"},\"capabilities\":[]}}\r\n";

async fn command(peer: &mut BufReader<DuplexStream>, name: &str, id: u16) {
    let mut line = String::new();
    peer.read_line(&mut line).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({"execute": name, "id": id})
    );
}

async fn record(peer: &mut BufReader<DuplexStream>, record: Value) {
    peer.get_mut()
        .write_all(format!("{record}\n").as_bytes())
        .await
        .unwrap();
}

async fn hello(peer: &mut BufReader<DuplexStream>) {
    for fragment in GREETING.chunks(7) {
        peer.get_mut().write_all(fragment).await.unwrap();
        tokio::task::yield_now().await;
    }
    command(peer, "qmp_capabilities", 1).await;
    record(peer, json!({"event": "RESET"})).await;
    record(peer, json!({"return": {}, "id": 1})).await;
}

async fn start(peer: &mut BufReader<DuplexStream>) {
    command(peer, "query-status", 2).await;
    record(
        peer,
        json!({"return": {"status": "prelaunch", "running": false}, "id": 2}),
    )
    .await;
    command(peer, "cont", 3).await;
    record(peer, json!({"event": "RESUME"})).await;
    record(peer, json!({"id": 3, "return": {}})).await;
}

#[tokio::test]
async fn qmp_correlates_fragmented_greeting_interleaved_events_and_shutdown() {
    let (host, peer) = duplex(64);
    let (read, write) = split(host);
    let fixture = tokio::spawn(async move {
        let mut peer = BufReader::new(peer);
        hello(&mut peer).await;
        start(&mut peer).await;
        command(&mut peer, "query-status", 4).await;
        record(
            &mut peer,
            json!({"return": {"status": "running", "running": true}, "id": 4}),
        )
        .await;
        command(&mut peer, "system_powerdown", 5).await;
        record(
            &mut peer,
            json!({"event": "SHUTDOWN", "data": {"guest": true, "reason": "guest-shutdown"}}),
        )
        .await;
        record(&mut peer, json!({"return": {}, "id": 5})).await;
    });
    let cancel = Shutdown::default();
    let mut qmp = Qmp::handshake(read, write, &cancel).await.unwrap();
    qmp.release_startup_hold(&cancel, || true).await.unwrap();
    assert_eq!(qmp.status(&cancel).await, Ok(RunStatus::Running));
    qmp.powerdown(&cancel).await.unwrap();
    fixture.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn qmp_shutdown_is_bounded_and_host_reset_is_not_guest_shutdown() {
    let (host, peer) = duplex(512);
    let (read, write) = split(host);
    let fixture = tokio::spawn(async move {
        let mut peer = BufReader::new(peer);
        hello(&mut peer).await;
        start(&mut peer).await;
        command(&mut peer, "system_powerdown", 4).await;
        record(&mut peer, json!({"return": {}, "id": 4})).await;
        record(
            &mut peer,
            json!({"event": "SHUTDOWN", "data": {"guest": false, "reason": "host-qmp-quit"}}),
        )
        .await;
        std::future::pending::<()>().await;
    });
    let cancel = Shutdown::default();
    let mut qmp = Qmp::handshake(read, write, &cancel).await.unwrap();
    qmp.release_startup_hold(&cancel, || true).await.unwrap();
    assert_eq!(qmp.powerdown(&cancel).await, Err(ChannelError::Timeout));
    fixture.abort();
    assert!(fixture.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn qmp_revalidates_after_prelaunch_status_and_refusal_sends_no_cont() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let (host, peer) = duplex(4096);
    let (read, write) = split(host);
    let status_sent = Arc::new(AtomicBool::new(false));
    let sent = status_sent.clone();
    let fixture = tokio::spawn(async move {
        let mut peer = BufReader::new(peer);
        hello(&mut peer).await;
        command(&mut peer, "query-status", 2).await;
        sent.store(true, Ordering::SeqCst);
        record(
            &mut peer,
            json!({"return": {"status": "prelaunch", "running": false}, "id": 2}),
        )
        .await;
        // A refusal closes the channel instead of sending `cont`.
        let mut rest = String::new();
        peer.read_line(&mut rest).await.unwrap();
        rest
    });
    let cancel = Shutdown::default();
    let mut qmp = Qmp::handshake(read, write, &cancel).await.unwrap();
    let calls = Cell::new(0);
    let released = qmp
        .release_startup_hold(&cancel, || {
            calls.set(calls.get() + 1);
            // The sample is taken only after the prelaunch status receipt.
            assert!(status_sent.load(Ordering::SeqCst));
            false
        })
        .await;
    assert_eq!(released, Err(ChannelError::Refused));
    assert_eq!(calls.get(), 1);
    assert_eq!(qmp.status(&cancel).await, Err(ChannelError::Closed));
    drop(qmp);
    assert_eq!(fixture.await.unwrap(), "", "`cont` reached the peer");
}

#[tokio::test]
async fn qmp_rejects_invalid_results_duplicate_id_and_event_flood() {
    for (result, expected) in [
        (
            json!({"return": {"status": "running", "running": false}, "id": 2}),
            ChannelError::Invalid,
        ),
        (json!({"return": {}, "id": 1}), ChannelError::Unexpected),
        (Value::Null, ChannelError::Oversize),
    ] {
        let (host, peer) = duplex(4096);
        let (read, write) = split(host);
        let fixture = tokio::spawn(async move {
            let mut peer = BufReader::new(peer);
            hello(&mut peer).await;
            command(&mut peer, "query-status", 2).await;
            if result.is_null() {
                for _ in 0..65 {
                    record(&mut peer, json!({"event": "RESET"})).await;
                }
            } else {
                record(&mut peer, result).await;
            }
        });
        let cancel = Shutdown::default();
        let mut qmp = Qmp::handshake(read, write, &cancel).await.unwrap();
        assert_eq!(qmp.status(&cancel).await, Err(expected));
        assert_eq!(qmp.status(&cancel).await, Err(ChannelError::Closed));
        fixture.await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn qmp_setup_distinguishes_forged_version_partial_eof_and_timeout() {
    for (input, error) in [
        (b"{\"QMP\":{\"version\":{\"qemu\":{\"major\":11,\"minor\":1,\"micro\":0},\"package\":\"\"},\"capabilities\":[]}}\n".as_slice(), ChannelError::Invalid),
        (b"{".as_slice(), ChannelError::Eof),
    ] {
        let (host, mut peer) = duplex(128);
        peer.write_all(input).await.unwrap();
        drop(peer);
        let (reader, writer) = split(host);
        assert_eq!(Qmp::handshake(reader, writer, &Shutdown::default()).await.err(), Some(error));
    }
    let (host, _silent_peer) = duplex(128);
    let (reader, writer) = split(host);
    assert_eq!(
        Qmp::handshake(reader, writer, &Shutdown::default())
            .await
            .err(),
        Some(ChannelError::Timeout)
    );
}

async fn guest_request(stream: &mut DuplexStream) -> Request {
    let size = stream.read_u32().await.unwrap();
    let mut bytes = size.to_be_bytes().to_vec();
    bytes.resize(4 + size as usize, 0);
    stream.read_exact(&mut bytes[4..]).await.unwrap();
    read_frame(&mut bytes.as_slice()).unwrap().unwrap()
}

async fn guest_reply(stream: &mut DuplexStream, response: Response) {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &response).unwrap();
    for chunk in bytes.chunks(3) {
        stream.write_all(chunk).await.unwrap();
    }
}

#[tokio::test]
async fn guest_requires_current_echo_support_and_matching_request_id() {
    for (forged, scripts) in [
        (true, ScriptExecution::Available),
        (false, ScriptExecution::Unavailable),
        (false, ScriptExecution::Available),
    ] {
        let (host, mut peer) = duplex(128);
        let fixture = tokio::spawn(async move {
            let Request::Hello { version, challenge } = guest_request(&mut peer).await else {
                panic!("expected hello")
            };
            let challenge = if forged {
                format!(
                    "{}{}",
                    if challenge.starts_with('0') { "1" } else { "0" },
                    &challenge[1..]
                )
            } else {
                challenge
            };
            guest_reply(
                &mut peer,
                Response::HelloAck {
                    version,
                    challenge,
                    script_execution: scripts,
                },
            )
            .await;
            if !forged && scripts == ScriptExecution::Available {
                let Request::Ping { id } = guest_request(&mut peer).await else {
                    panic!("expected ping")
                };
                guest_reply(&mut peer, Response::Pong { id: id + 1 }).await;
            }
        });
        let cancel = Shutdown::default();
        let result = GuestChannel::handshake(host, &cancel).await;
        if forged || scripts == ScriptExecution::Unavailable {
            assert_eq!(result.err(), Some(ChannelError::Unexpected));
        } else {
            let mut guest = result.unwrap();
            assert_eq!(guest.ping(&cancel).await, Err(ChannelError::Unexpected));
            assert_eq!(guest.ping(&cancel).await, Err(ChannelError::Closed));
        }
        fixture.await.unwrap();
    }
}

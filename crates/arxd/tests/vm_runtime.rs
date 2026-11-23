use arx_guest::{Request, Response, ScriptExecution, read_frame, write_frame};
use arxd::{
    config::{Config, InvalidVmProfile, VmRuntimeProfile},
    service::Shutdown,
    vm::{
        guest::{FRAME_BOUND, GUEST_READY_BUDGET, GuestChannel, MAX_CHALLENGES, RETRY_FIRST},
        paths::{Artifact, ArtifactPin, ArtifactRole, FileIdentity, LocalPath},
        qemu_args::{
            BootPurpose, BootRecipe, GUEST_PORT_ID, NETWORK, PlanKind, QEMU_VERSION, SerialName,
            StaticNetwork,
        },
        qemu_img,
        qmp::{ChannelError, Qmp, RunStatus},
        runtime::{
            CandidateRuntime, InstallerBudget, InstallerFailure, InstallerFailureReason,
            InstallerPhase, InstallerRecipe, InstallerStage, NativeMutation, NativeRefusal,
            QualificationEvidence, StopDisposition, StopEvidence, capacity, fresh_observation,
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
    assert_eq!(QEMU_VERSION, "11.1.3");
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
fn previous_qemu_manifest_is_refused_with_a_typed_error() {
    let mut previous = profile();
    previous["manifest"] = json!("qemu-11.1.0-windows-x64");
    assert!(matches!(parse(&previous), Err(error) if error.is::<InvalidVmProfile>()));
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

fn put(cluster: &mut [u8], at: usize, bytes: &[u8]) {
    cluster[at..at + bytes.len()].copy_from_slice(bytes);
}

/// Synthetic cluster 0 for the fixed create recipe
/// (`qemu-img create -f qcow2 -o compat=1.1,cluster_size=65536,lazy_refcounts=off`),
/// using the documented layout.
/// QEMU 11.1.x `block/qcow2.h` declares the packed big-endian `QCowHeader`
/// as 112 bytes for a new version-3 image. `qcow2_update_header` in
/// `block/qcow2.c` then writes the backing-format extension (only when
/// backed), the feature-name table, the end marker, and finally the backing
/// file name. Layout fields the parser does not check (L1 and refcount
/// placement) carry representative values for a 1 MiB image.
fn qcow2_11_1(backing: Option<&str>) -> Vec<u8> {
    let mut c = vec![0u8; 65_536];
    put(&mut c, 0, b"QFI\xfb"); // magic
    put(&mut c, 4, &3u32.to_be_bytes()); // version
    put(&mut c, 20, &16u32.to_be_bytes()); // cluster_bits: cluster_size=65536
    put(&mut c, 24, &(1u64 << 20).to_be_bytes()); // size
    put(&mut c, 36, &1u32.to_be_bytes()); // l1_size
    put(&mut c, 40, &0x3_0000u64.to_be_bytes()); // l1_table_offset
    put(&mut c, 48, &0x1_0000u64.to_be_bytes()); // refcount_table_offset
    put(&mut c, 56, &1u32.to_be_bytes()); // refcount_table_clusters
    // crypt_method, nb_snapshots, snapshots_offset and the incompatible,
    // compatible (lazy_refcounts=off) and autoclear feature words stay zero.
    put(&mut c, 96, &4u32.to_be_bytes()); // refcount_order: 16-bit refcounts
    put(&mut c, 100, &112u32.to_be_bytes()); // header_length
    // compression_type (byte 104) = 0 (zlib); bytes 105..112 are padding.
    let mut at = 112;
    if backing.is_some() {
        put(&mut c, at, &0xe279_2acau32.to_be_bytes()); // backing file format
        put(&mut c, at + 4, &5u32.to_be_bytes());
        put(&mut c, at + 8, b"qcow2");
        at += 16;
    }
    let features: [(u8, u8, &str); 8] = [
        (0, 0, "dirty bit"),
        (0, 1, "corrupt bit"),
        (0, 2, "external data file"),
        (0, 3, "compression type"),
        (0, 4, "extended L2 entries"),
        (1, 0, "lazy refcounts"),
        (2, 0, "bitmaps"),
        (2, 1, "raw external data"),
    ];
    put(&mut c, at, &0x6803_f857u32.to_be_bytes()); // feature name table
    put(&mut c, at + 4, &(48 * features.len() as u32).to_be_bytes());
    at += 8;
    for (kind, bit, name) in features {
        c[at] = kind;
        c[at + 1] = bit;
        put(&mut c, at + 2, name.as_bytes());
        at += 48;
    }
    at += 8; // end of header extensions
    if let Some(name) = backing {
        put(&mut c, 8, &(at as u64).to_be_bytes()); // backing_file_offset
        put(&mut c, 16, &(name.len() as u32).to_be_bytes()); // backing_file_size
        put(&mut c, at, name.as_bytes());
    }
    c
}

#[test]
fn qemu_11_1_create_headers_are_accepted_and_bounded() {
    let golden = LocalPath::parse("E:/golden/base.qcow2").unwrap();
    let plain = qcow2_11_1(None);
    let backed = qcow2_11_1(Some(golden.as_str()));
    assert_eq!(qemu_img::inspect_header(&plain, None), Ok(1 << 20));
    assert_eq!(
        qemu_img::inspect_header(&backed, Some(&golden)),
        Ok(1 << 20)
    );
    // The PR's own golden inspection accepts the creator's output.
    let tool = artifact(ArtifactRole::QemuImg, "E:/bundle/qemu-img.exe");
    let golden_role = artifact(ArtifactRole::Golden, "E:/golden/base.qcow2");
    assert!(qemu_img::golden_info(&tool, &golden_role, &plain).is_ok());
    // zstd is pinned as compression type 1 with incompatible bit 3.
    let mut zstd = plain.clone();
    zstd[104] = 1;
    zstd[79] |= 8;
    assert_eq!(qemu_img::inspect_header(&zstd, None), Ok(1 << 20));
    type Mutation = fn(&mut Vec<u8>);
    let refused: &[(&str, Mutation)] = &[
        ("zstd without its feature bit", |h| h[104] = 1),
        ("unknown compression type", |h| h[104] = 2),
        ("compression bit with zlib", |h| h[79] |= 8),
        ("nonzero padding", |h| h[111] = 1),
        ("header_length beyond the pinned layout", |h| h[103] = 120),
        ("header_length below version 3", |h| h[103] = 96),
        ("dirty", |h| h[79] |= 1),
        ("corrupt", |h| h[79] |= 2),
        ("external data file feature", |h| h[79] |= 4),
        ("extended L2 entries", |h| h[79] |= 16),
        ("unknown incompatible bit", |h| h[79] |= 32),
        ("unknown high incompatible bit", |h| h[72] |= 0x80),
        ("lazy refcounts", |h| h[87] |= 1),
        ("autoclear bitmaps", |h| h[95] |= 1),
        ("external data file extension", |h| {
            h[112..116].copy_from_slice(&0x4441_5441u32.to_be_bytes())
        }),
        ("extension beyond the cluster", |h| {
            h[116..120].copy_from_slice(&0x0010_0000u32.to_be_bytes())
        }),
        ("unexpected backing reference", |h| {
            h[8..16].copy_from_slice(&600u64.to_be_bytes());
            h[16..20].copy_from_slice(&4u32.to_be_bytes());
            h[600..604].copy_from_slice(b"evil");
        }),
    ];
    for (name, change) in refused {
        let mut header = plain.clone();
        change(&mut header);
        assert!(
            qemu_img::inspect_header(&header, None).is_err(),
            "accepted: {name}"
        );
    }
    // Backing references must be exactly the approved file, never missing.
    let other = LocalPath::parse("E:/golden/other.qcow2").unwrap();
    assert!(qemu_img::inspect_header(&backed, Some(&other)).is_err());
    assert!(qemu_img::inspect_header(&backed, None).is_err());
    assert!(qemu_img::inspect_header(&plain, Some(&golden)).is_err());
}

#[test]
fn boot_metadata_may_reference_only_the_approved_golden() {
    use ArtifactRole::*;
    let image = artifact(Qemu, "E:/bundle/qemu-system-x86_64.exe");
    let data = artifact(Data, "E:/bundle/pc-bios");
    let code = artifact(FirmwareCode, "E:/bundle/edk2-x86_64-code.fd");
    let vars = artifact(Vars, "D:/vms/vars.fd");
    let disk = artifact(GuestDisk, "D:/vms/current.qcow2");
    let golden = artifact(Golden, "E:/golden/base.qcow2");
    let serial = SerialName::generate().unwrap();
    let plain = qcow2_11_1(None);
    let backed = qcow2_11_1(Some(r"E:\golden\base.qcow2"));
    let foreign = qcow2_11_1(Some(r"E:\golden\other.qcow2"));
    let mut external = backed.clone();
    external[79] |= 4; // QCOW2_INCOMPAT_DATA_FILE
    let plan = |disk_header: &[u8], backing: Option<&Artifact>, backing_header: Option<&[u8]>| {
        BootRecipe {
            image: &image,
            data: &data,
            firmware: &code,
            vars: &vars,
            disk: &disk,
            disk_header,
            backing,
            backing_header,
            serial: &serial,
            boot: BootPurpose::Installed,
            ram_mib: 2048,
            vcpus: 2,
        }
        .plan()
        .is_ok()
    };
    assert!(
        plan(&backed, Some(&golden), Some(&plain)),
        "approved backed boot"
    );
    assert!(plan(&plain, None, None), "approved unbacked boot");
    assert!(
        !plan(&foreign, Some(&golden), Some(&plain)),
        "foreign backing reference"
    );
    assert!(
        !plan(&external, Some(&golden), Some(&plain)),
        "external data file"
    );
    assert!(!plan(&backed, None, None), "missing backing");
    assert!(!plan(&plain, Some(&golden), Some(&plain)), "extra backing");
    assert!(
        !plan(&backed, Some(&golden), Some(&backed)),
        "golden with its own backing"
    );
    assert!(
        !plan(&backed, Some(&golden), None),
        "golden without its header"
    );
    assert!(!plan(&plain, None, Some(&plain)), "header without a golden");
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
    let serial = SerialName::generate().unwrap();
    let golden_header = qcow2_11_1(None);
    let disk_header = qcow2_11_1(Some(r"E:\golden\base.qcow2"));
    let mut recipe = BootRecipe {
        image: &image,
        data: &data,
        firmware: &code,
        vars: &vars,
        disk: &disk,
        disk_header: &disk_header,
        backing: Some(&backing),
        backing_header: Some(&golden_header),
        serial: &serial,
        boot: BootPurpose::Installed,
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
    assert_eq!(blocks.len(), 4);
    assert_eq!(blocks[0]["read-only"], true);
    assert_eq!(blocks[1]["read-only"], false);
    assert_eq!(blocks[2]["backing"], Value::Null);
    assert_eq!(blocks[3]["backing"], "golden");
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
async fn qmp_refuses_previous_qemu_version_before_capability_negotiation() {
    let (host, mut peer) = duplex(128);
    peer.write_all(b"{\"QMP\":{\"version\":{\"qemu\":{\"major\":11,\"minor\":1,\"micro\":0},\"package\":\"\"},\"capabilities\":[]}}\n")
        .await
        .unwrap();
    let (reader, writer) = split(host);
    assert_eq!(
        Qmp::handshake(reader, writer, &Shutdown::default())
            .await
            .err(),
        Some(ChannelError::Invalid)
    );
    let mut command = [0_u8; 1];
    assert_eq!(peer.read(&mut command).await.unwrap(), 0);
}

#[tokio::test(start_paused = true)]
async fn qmp_setup_distinguishes_partial_eof_and_timeout() {
    let (host, mut peer) = duplex(128);
    peer.write_all(b"{").await.unwrap();
    drop(peer);
    let (reader, writer) = split(host);
    assert_eq!(
        Qmp::handshake(reader, writer, &Shutdown::default())
            .await
            .err(),
        Some(ChannelError::Eof)
    );

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

type HostQmp = Qmp<tokio::io::ReadHalf<DuplexStream>, tokio::io::WriteHalf<DuplexStream>>;
type Readiness =
    tokio::task::JoinHandle<(Result<GuestChannel<DuplexStream>, ChannelError>, HostQmp)>;

/// A QMP peer past the startup `cont` that forwards injected raw bytes, and a
/// serial pipe pair standing in for the verified Windows pipe. The paused
/// tokio clock is the injected clock in every readiness test.
async fn started_vm() -> (
    HostQmp,
    tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    DuplexStream,
    DuplexStream,
) {
    let (host, peer) = duplex(4096);
    let (read, write) = split(host);
    let (events, mut inject) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(async move {
        let mut peer = BufReader::new(peer);
        hello(&mut peer).await;
        start(&mut peer).await;
        while let Some(bytes) = inject.recv().await {
            peer.get_mut().write_all(&bytes).await.unwrap();
        }
        std::future::pending::<()>().await;
    });
    let cancel = Shutdown::default();
    let mut qmp = Qmp::handshake(read, write, &cancel).await.unwrap();
    qmp.release_startup_hold(&cancel, || true).await.unwrap();
    let (serial, guest) = duplex(8192);
    (qmp, events, serial, guest)
}

async fn installer_qmp(
    stage: InstallerStage,
    cancel: &Shutdown,
    before_ack: Vec<Value>,
) -> (HostQmp, BufReader<DuplexStream>) {
    let (host, peer) = duplex(8192);
    let (read, write) = split(host);
    let fixture = async move {
        let mut peer = BufReader::new(peer);
        peer.get_mut().write_all(GREETING).await.unwrap();
        command(&mut peer, "qmp_capabilities", 1).await;
        record(&mut peer, json!({"return": {}, "id": 1})).await;
        command(&mut peer, "query-status", 2).await;
        record(
            &mut peer,
            json!({"return": {"status": "prelaunch", "running": false}, "id": 2}),
        )
        .await;
        command(&mut peer, "cont", 3).await;
        for event in before_ack {
            record(&mut peer, event).await;
        }
        record(&mut peer, json!({"return": {}, "id": 3})).await;
        peer
    };
    let launch = async {
        let mut qmp = Qmp::installer_handshake(read, write, cancel, stage)
            .await
            .unwrap();
        let did_revalidate = Cell::new(false);
        qmp.release_startup_hold(cancel, || {
            did_revalidate.set(true);
            true
        })
        .await
        .unwrap();
        assert!(did_revalidate.get());
        qmp
    };
    tokio::join!(launch, fixture)
}

fn installer_shutdown() -> Value {
    json!({"event": "SHUTDOWN", "data": {"guest": true, "reason": "guest-shutdown"}})
}

fn installer_reset() -> Value {
    json!({"event": "RESET", "data": {"guest": true, "reason": "guest-reset"}})
}

#[tokio::test(start_paused = true)]
async fn installer_observes_reset_stop_and_guest_poweroff_without_sending_another_command() {
    let phase = InstallerPhase::WindowsSetup;
    let budget = InstallerBudget::start(phase.recipe());
    let stage = budget.stage(phase, Duration::MAX).unwrap();
    let cancel = Shutdown::default();
    let (mut qmp, mut peer) = installer_qmp(stage, &cancel, vec![installer_reset()]).await;
    record(&mut peer, installer_reset()).await;
    record(&mut peer, json!({"event": "STOP"})).await;
    record(&mut peer, installer_shutdown()).await;
    assert_eq!(qmp.observe_installer_shutdown(&cancel).await, Ok(()));
    let mut extra = [0];
    assert!(polled_pending(Box::pin(peer.read(&mut extra)).as_mut()));

    let (mut qmp, _peer) = installer_qmp(stage, &cancel, vec![installer_shutdown()]).await;
    assert_eq!(qmp.observe_installer_shutdown(&cancel).await, Ok(()));
}

#[tokio::test(start_paused = true)]
async fn installer_deadline_clips_startup_progress_and_later_stages() {
    use tokio::time::{Instant, advance};
    let zero = Instant::now();
    let ubuntu = InstallerBudget::start(InstallerRecipe::Ubuntu);
    let windows = InstallerBudget::start(InstallerRecipe::Windows);
    let hour = Duration::from_secs(3600);
    let setup = InstallerPhase::WindowsSetup;
    assert_eq!(ubuntu.clip(Duration::MAX), zero + hour);
    assert!(ubuntu.stage(setup, hour).is_none());
    assert_eq!(windows.clip(Duration::MAX), zero + 3 * hour);

    let cancel = Shutdown::default();
    let short = Duration::from_secs(2);
    let stage = windows.stage(setup, short).unwrap();
    let (host, _silent_peer) = duplex(1024);
    let (read, write) = split(host);
    assert_eq!(
        Qmp::installer_handshake(read, write, &cancel, stage)
            .await
            .err(),
        Some(ChannelError::Timeout)
    );
    assert_eq!(Instant::now(), zero + short);
    let stage = windows.stage(setup, Duration::MAX).unwrap();
    let (mut qmp, mut peer) = installer_qmp(stage, &cancel, vec![]).await;
    let mut observe = Box::pin(qmp.observe_installer_shutdown(&cancel));
    for leap in [hour - short * 2, hour - short, hour - short] {
        assert!(polled_pending(observe.as_mut()));
        advance(leap).await;
        record(&mut peer, installer_reset()).await;
    }
    assert!(polled_pending(observe.as_mut()));
    let finalize = windows
        .stage(InstallerPhase::WindowsFinalize, hour)
        .unwrap();
    assert_eq!(finalize.deadline(), zero + 3 * hour);
    advance(zero + 3 * hour - Instant::now()).await;
    record(&mut peer, installer_shutdown()).await;
    assert_eq!(observe.await, Err(ChannelError::Timeout));
    assert_eq!(windows.clip(hour), zero + 3 * hour);
}

#[tokio::test(start_paused = true)]
async fn installer_refuses_early_exit_unsafe_events_and_exhausted_reset_or_event_budget() {
    let phase = InstallerPhase::WindowsSetup;
    let budget = InstallerBudget::start(phase.recipe());
    let stage = budget.stage(phase, Duration::MAX).unwrap();
    let cancel = Shutdown::default();
    for (events, expected) in [
        (vec![], ChannelError::Eof),
        (
            vec![json!({"event": "GUEST_PANICKED"})],
            ChannelError::Unexpected,
        ),
        (
            vec![
                json!({"event": "RESET", "data": {"guest": false, "reason": "host-qmp-system-reset"}}),
            ],
            ChannelError::Unexpected,
        ),
        (
            vec![json!({"event": "SHUTDOWN", "data": {"guest": true, "reason": "guest-reset"}})],
            ChannelError::Unexpected,
        ),
        (
            vec![json!({"event": "SHUTDOWN", "data": {"guest": false, "reason": "host-qmp-quit"}})],
            ChannelError::Unexpected,
        ),
        (
            vec![json!({"event": "SHUTDOWN", "data": {"guest": true}})],
            ChannelError::Invalid,
        ),
        (vec![installer_reset(); 17], ChannelError::Unexpected),
        (vec![json!({"event": "RESUME"}); 65], ChannelError::Oversize),
    ] {
        let (mut qmp, mut peer) = installer_qmp(stage, &cancel, vec![]).await;
        for event in events {
            record(&mut peer, event).await;
        }
        drop(peer);
        assert_eq!(qmp.observe_installer_shutdown(&cancel).await, Err(expected));
    }
    for phase in [
        InstallerPhase::UbuntuInstall,
        InstallerPhase::WindowsFinalize,
    ] {
        let budget = InstallerBudget::start(phase.recipe());
        let stage = budget.stage(phase, Duration::MAX).unwrap();
        let (mut qmp, mut peer) = installer_qmp(stage, &cancel, vec![]).await;
        record(&mut peer, installer_reset()).await;
        assert_eq!(
            qmp.observe_installer_shutdown(&cancel).await,
            Err(ChannelError::Unexpected)
        );
    }
}

#[tokio::test(start_paused = true)]
async fn installer_stop_without_shutdown_and_windows_idle_are_bounded_and_cancel_is_immediate() {
    let phase = InstallerPhase::WindowsSetup;
    let budget = InstallerBudget::start(phase.recipe());
    let stage = budget.stage(phase, Duration::MAX).unwrap();
    let cancel = Shutdown::default();
    let (mut qmp, mut peer) = installer_qmp(stage, &cancel, vec![]).await;
    let start = tokio::time::Instant::now();
    record(&mut peer, json!({"event": "STOP"})).await;
    assert_eq!(
        qmp.observe_installer_shutdown(&cancel).await,
        Err(ChannelError::Timeout)
    );
    assert_eq!(start.elapsed(), Duration::from_secs(2));

    let (mut qmp, _peer) = installer_qmp(stage, &cancel, vec![]).await;
    let start = tokio::time::Instant::now();
    assert_eq!(
        qmp.observe_installer_shutdown(&cancel).await,
        Err(ChannelError::Timeout)
    );
    assert_eq!(start.elapsed(), Duration::from_secs(3600));

    let (mut qmp, _peer) = installer_qmp(stage, &cancel, vec![]).await;
    let mut observe = Box::pin(qmp.observe_installer_shutdown(&cancel));
    assert!(polled_pending(observe.as_mut()));
    let start = tokio::time::Instant::now();
    cancel.cancel();
    assert_eq!(observe.await, Err(ChannelError::Cancelled));
    assert_eq!(start.elapsed(), Duration::ZERO);
}

#[test]
fn installer_release_requires_process_job_workers_and_reopen_before_detach() {
    for (exited, empty, joined, reopen, shutdown, code_zero, expected) in [
        (
            false,
            true,
            true,
            true,
            true,
            true,
            StopDisposition::ChargedQuarantine,
        ),
        (
            true,
            false,
            true,
            true,
            true,
            true,
            StopDisposition::ChargedQuarantine,
        ),
        (
            true,
            true,
            false,
            true,
            true,
            true,
            StopDisposition::ChargedQuarantine,
        ),
        (
            true,
            true,
            true,
            false,
            true,
            true,
            StopDisposition::ChargedQuarantine,
        ),
        (
            true,
            true,
            true,
            true,
            false,
            true,
            StopDisposition::DirtyRelease,
        ),
        (
            true,
            true,
            true,
            true,
            true,
            false,
            StopDisposition::DirtyRelease,
        ),
        (
            true,
            true,
            true,
            true,
            true,
            true,
            StopDisposition::CleanRelease,
        ),
    ] {
        let mut evidence = StopEvidence {
            guest_shutdown: shutdown,
            clean_process_exit: code_zero,
            process_exited: exited,
            job_empty: empty,
            workers_joined: joined,
            artifacts_reopened: false,
        };
        let mut reopened = false;
        evidence.verify_release(|| {
            reopened = true;
            reopen
        });
        assert_eq!(reopened, exited && empty && joined);
        assert_eq!(evidence.disposition(), expected);
    }
}

#[tokio::test(start_paused = true)]
async fn installer_failure_keeps_its_phase_and_charges_uncertain_recovery() {
    use InstallerFailureReason::*;
    use StopDisposition::*;
    let phase = InstallerPhase::UbuntuInstall;
    let budget = InstallerBudget::start(phase.recipe());
    let limit = Duration::from_secs(2);
    let stage = budget.stage(phase, limit).unwrap();
    assert_eq!(stage.complete(Ok(()), CleanRelease, false), Ok(()));
    for (result, observed, cancelled, reason, recovery) in [
        (Err(Timeout), DirtyRelease, false, Timeout, DirtyRelease),
        (
            Err(Timeout),
            ChargedQuarantine,
            false,
            Timeout,
            ChargedQuarantine,
        ),
        (
            Ok(()),
            ChargedQuarantine,
            false,
            ReleaseUnverified,
            ChargedQuarantine,
        ),
        (Ok(()), CleanRelease, true, Cancelled, DirtyRelease),
        (
            Err(ProcessFailed),
            DirtyRelease,
            false,
            ProcessFailed,
            DirtyRelease,
        ),
    ] {
        assert_eq!(
            stage.complete(result, observed, cancelled),
            Err(InstallerFailure {
                phase,
                reason,
                recovery
            })
        );
    }
    tokio::time::advance(limit).await;
    assert_eq!(
        stage.complete(Ok(()), CleanRelease, false),
        Err(InstallerFailure {
            phase,
            reason: Timeout,
            recovery: DirtyRelease
        })
    );
}

fn readiness(mut qmp: HostQmp, serial: DuplexStream, cancel: Shutdown) -> Readiness {
    tokio::spawn(async move {
        let result = GuestChannel::ready(serial, &mut qmp, &cancel).await;
        (result, qmp)
    })
}

fn line(record: Value) -> Vec<u8> {
    format!("{record}\n").into_bytes()
}

fn port(open: bool) -> Vec<u8> {
    line(json!({"event": "VSERPORT_CHANGE", "data": {"id": GUEST_PORT_ID, "open": open}}))
}

/// The next Hello the guest side receives, bounded so a missing one fails.
async fn guest_hello(guest: &mut DuplexStream) -> String {
    let request = tokio::time::timeout(GUEST_READY_BUDGET * 2, guest_request(guest))
        .await
        .expect("no Hello reached the guest");
    let Request::Hello { challenge, .. } = request else {
        panic!("expected hello")
    };
    challenge
}

fn ack_frame(challenge: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_frame(
        &mut bytes,
        &Response::HelloAck {
            version: 1,
            challenge: challenge.to_owned(),
            script_execution: ScriptExecution::Available,
        },
    )
    .unwrap();
    bytes
}

async fn guest_ack(guest: &mut DuplexStream, challenge: &str) {
    guest.write_all(&ack_frame(challenge)).await.unwrap();
}

/// True when no byte has been written to the guest side. End of stream,
/// after readiness returns and drops the host end, also wrote nothing.
async fn quiet(guest: &mut DuplexStream) -> bool {
    !matches!(
        tokio::time::timeout(Duration::from_millis(1), guest.read_u8()).await,
        Ok(Ok(_))
    )
}

/// Counts the Hellos waiting on the guest side, stopping at silence or EOF.
async fn drain_hellos(guest: &mut DuplexStream) -> usize {
    let mut count = 0;
    loop {
        let Ok(Ok(size)) = tokio::time::timeout(Duration::from_millis(1), guest.read_u32()).await
        else {
            return count;
        };
        let mut bytes = size.to_be_bytes().to_vec();
        bytes.resize(4 + size as usize, 0);
        guest.read_exact(&mut bytes[4..]).await.unwrap();
        let request: Request = read_frame(&mut bytes.as_slice()).unwrap().unwrap();
        assert!(matches!(request, Request::Hello { .. }), "expected hello");
        count += 1;
    }
}

/// Answers one Ping on the guest side.
fn pong_once(mut guest: DuplexStream) -> tokio::task::JoinHandle<DuplexStream> {
    tokio::spawn(async move {
        let Request::Ping { id } = guest_request(&mut guest).await else {
            panic!("expected ping")
        };
        guest_reply(&mut guest, Response::Pong { id }).await;
        guest
    })
}

fn polled_pending<F: std::future::Future>(future: std::pin::Pin<&mut F>) -> bool {
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    future.poll(&mut context).is_pending()
}

#[tokio::test(start_paused = true)]
async fn guest_requires_current_echo_support_and_matching_request_id() {
    for (forged, scripts) in [
        (true, ScriptExecution::Available),
        (false, ScriptExecution::Unavailable),
        (false, ScriptExecution::Available),
    ] {
        let (qmp, events, serial, mut guest) = started_vm().await;
        let task = readiness(qmp, serial, Shutdown::default());
        events.send(port(true)).unwrap();
        let challenge = guest_hello(&mut guest).await;
        let challenge = if forged {
            // A challenge the host never issued.
            format!(
                "{}{}",
                if challenge.starts_with('0') { "1" } else { "0" },
                &challenge[1..]
            )
        } else {
            challenge
        };
        guest_reply(
            &mut guest,
            Response::HelloAck {
                version: 1,
                challenge,
                script_execution: scripts,
            },
        )
        .await;
        let (result, _qmp) = task.await.unwrap();
        if forged || scripts == ScriptExecution::Unavailable {
            assert_eq!(result.err(), Some(ChannelError::Unexpected));
            continue;
        }
        let mut channel = result.unwrap();
        let peer = tokio::spawn(async move {
            let Request::Ping { id } = guest_request(&mut guest).await else {
                panic!("expected ping")
            };
            guest_reply(&mut guest, Response::Pong { id: id + 1 }).await;
            guest
        });
        let cancel = Shutdown::default();
        assert_eq!(channel.ping(&cancel).await, Err(ChannelError::Unexpected));
        assert_eq!(channel.ping(&cancel).await, Err(ChannelError::Closed));
        drop(peer.await.unwrap());
    }
}

/// OVMF's VirtioSerialDxe opens the port before Linux and drops the Hello it
/// takes at ExitBootServices. A close pauses retries; the next open gets a
/// fresh challenge.
#[tokio::test(start_paused = true)]
async fn firmware_consumes_a_hello_and_the_next_open_gets_a_fresh_one() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events.send(port(true)).unwrap(); // firmware opens the port
    let firmware = guest_hello(&mut guest).await; // never answered
    events.send(port(false)).unwrap(); // ExitBootServices resets the queue
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert!(
        quiet(&mut guest).await,
        "retries continued while the port was closed"
    );
    events.send(port(true)).unwrap(); // the Linux service opens the port
    let fresh = guest_hello(&mut guest).await;
    assert_ne!(fresh, firmware);
    guest_ack(&mut guest, &fresh).await;
    assert!(task.await.unwrap().0.is_ok());
}

/// A stale buffered Hello and the fresh one both reach the service, which
/// answers each in order. The first valid echo is accepted, and the later
/// ack, read in the same chunk, is skipped by the command channel.
#[tokio::test(start_paused = true)]
async fn stale_and_fresh_hellos_are_both_acked_and_the_first_echo_is_accepted() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events.send(port(true)).unwrap(); // Hello 1, left buffered by firmware
    events.send(port(false)).unwrap();
    events.send(port(true)).unwrap(); // Hello 2 for the Linux service
    let stale = guest_hello(&mut guest).await;
    let fresh = guest_hello(&mut guest).await;
    assert_ne!(stale, fresh);
    let mut both = ack_frame(&stale);
    both.extend(ack_frame(&fresh));
    guest.write_all(&both).await.unwrap();
    let (result, _qmp) = task.await.unwrap();
    let mut channel = result.unwrap();
    let peer = pong_once(guest);
    assert_eq!(channel.ping(&Shutdown::default()).await, Ok(()));
    drop(peer.await.unwrap());
}

/// QEMU coalesces a rapid close and reopen into a repeated open.
#[tokio::test(start_paused = true)]
async fn a_coalesced_repeated_open_sends_a_fresh_challenge() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events.send(port(true)).unwrap();
    let lost = guest_hello(&mut guest).await;
    events.send(port(true)).unwrap(); // no close was reported in between
    let fresh = guest_hello(&mut guest).await;
    assert_ne!(lost, fresh);
    guest_ack(&mut guest, &fresh).await;
    assert!(task.await.unwrap().0.is_ok());
}

#[tokio::test(start_paused = true)]
async fn with_no_events_at_all_a_retry_reaches_the_guest() {
    let (qmp, _events, serial, mut guest) = started_vm().await;
    let start = tokio::time::Instant::now();
    let task = readiness(qmp, serial, Shutdown::default());
    let challenge = guest_hello(&mut guest).await;
    assert_eq!(start.elapsed(), RETRY_FIRST);
    guest_ack(&mut guest, &challenge).await;
    assert!(task.await.unwrap().0.is_ok());
}

#[tokio::test(start_paused = true)]
async fn issued_challenges_are_capped() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    for _ in 0..=MAX_CHALLENGES {
        events.send(port(true)).unwrap();
    }
    let (result, _qmp) = task.await.unwrap();
    assert_eq!(result.err(), Some(ChannelError::Refused));
    assert_eq!(drain_hellos(&mut guest).await, MAX_CHALLENGES);
}

#[tokio::test(start_paused = true)]
async fn the_budget_bounds_every_retry() {
    let (qmp, _events, serial, mut guest) = started_vm().await;
    let start = tokio::time::Instant::now();
    let task = readiness(qmp, serial, Shutdown::default());
    let (result, _qmp) = task.await.unwrap();
    assert_eq!(result.err(), Some(ChannelError::Timeout));
    assert_eq!(start.elapsed(), GUEST_READY_BUDGET);
    // Retries at 1, 3, 7 and then every 8 s up to 175 s, all inside the budget.
    let sent = drain_hellos(&mut guest).await;
    assert!((20..=26).contains(&sent), "{sent} retries");
}

#[tokio::test(start_paused = true)]
async fn a_challenge_from_outside_this_boot_is_rejected() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events.send(port(true)).unwrap();
    guest_hello(&mut guest).await;
    guest_ack(&mut guest, &"a".repeat(32)).await;
    assert_eq!(task.await.unwrap().0.err(), Some(ChannelError::Unexpected));
}

/// An open and the expired deadline are both ready on the first poll after
/// expiry: no Hello may be written.
#[tokio::test(start_paused = true)]
async fn no_hello_is_written_at_or_after_the_deadline() {
    let (mut qmp, events, serial, mut guest) = started_vm().await;
    let cancel = Shutdown::default();
    let mut ready = Box::pin(GuestChannel::ready(serial, &mut qmp, &cancel));
    assert!(polled_pending(ready.as_mut())); // the deadline is fixed now
    events.send(port(true)).unwrap();
    // Let the QMP peer forward the event without polling `ready`.
    tokio::time::sleep(Duration::from_millis(1)).await;
    tokio::time::advance(GUEST_READY_BUDGET).await;
    assert_eq!(ready.await.err(), Some(ChannelError::Timeout));
    assert!(
        quiet(&mut guest).await,
        "a Hello was written after the deadline"
    );
}

#[tokio::test(start_paused = true)]
async fn other_ports_are_ignored_and_malformed_port_events_refuse() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events
        .send(line(
            json!({"event": "VSERPORT_CHANGE", "data": {"id": "other-port", "open": true}}),
        ))
        .unwrap();
    events.send(line(json!({"event": "RESUME"}))).unwrap();
    // Before the first 1 s retry: another port's open released no Hello.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        quiet(&mut guest).await,
        "another port's open released a Hello"
    );
    events.send(port(true)).unwrap();
    let challenge = guest_hello(&mut guest).await;
    guest_ack(&mut guest, &challenge).await;
    assert!(task.await.unwrap().0.is_ok());

    for malformed in [
        json!({"event": "VSERPORT_CHANGE", "data": {"id": GUEST_PORT_ID}}),
        json!({"event": "VSERPORT_CHANGE", "data": {"id": GUEST_PORT_ID, "open": "yes"}}),
        json!({"event": "VSERPORT_CHANGE", "data": {"id": GUEST_PORT_ID, "open": true, "x": 1}}),
        json!({"event": "VSERPORT_CHANGE"}),
        json!({
            "event": "VSERPORT_CHANGE",
            "data": {"id": GUEST_PORT_ID, "open": true},
            "timestamp": {"seconds": 1, "microseconds": 1_000_000},
        }),
    ] {
        let (qmp, events, serial, mut guest) = started_vm().await;
        let task = readiness(qmp, serial, Shutdown::default());
        events.send(line(malformed)).unwrap();
        assert_eq!(task.await.unwrap().0.err(), Some(ChannelError::Invalid));
        assert!(quiet(&mut guest).await);
    }
}

/// The serial branch has consumed half an ack when a port event wins the
/// select. The event is handled once, and the ack completes intact.
#[tokio::test(start_paused = true)]
async fn a_partial_reply_survives_losing_the_select_to_a_port_event() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events.send(port(true)).unwrap();
    let first = guest_hello(&mut guest).await;
    let ack = ack_frame(&first);
    let (head, tail) = ack.split_at(ack.len() / 2);
    guest.write_all(head).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    events.send(port(true)).unwrap(); // wins while the reply is partial
    let second = guest_hello(&mut guest).await;
    assert_ne!(first, second);
    guest.write_all(tail).await.unwrap();
    let (result, _qmp) = task.await.unwrap();
    let mut channel = result.unwrap();
    // The guest answers the second Hello too; the channel skips that ack.
    guest_ack(&mut guest, &second).await;
    let peer = pong_once(guest);
    assert_eq!(channel.ping(&Shutdown::default()).await, Ok(()));
    drop(peer.await.unwrap());
}

/// The QMP branch has consumed half a port event when the retry timer wins
/// the select. The record completes intact and yields exactly one open.
#[tokio::test(start_paused = true)]
async fn a_partial_qmp_record_survives_losing_the_select_to_a_retry() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    let open = port(true);
    let (head, tail) = open.split_at(open.len() / 2);
    events.send(head.to_vec()).unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let retried = guest_hello(&mut guest).await; // the 1 s retry won
    events.send(tail.to_vec()).unwrap();
    let opened = guest_hello(&mut guest).await; // one event, one Hello
    assert_ne!(retried, opened);
    guest_ack(&mut guest, &opened).await;
    assert!(task.await.unwrap().0.is_ok());
    assert!(quiet(&mut guest).await, "the event was duplicated");
}

#[tokio::test(start_paused = true)]
async fn guest_readiness_tolerates_a_late_service_within_the_fixed_budget() {
    // The service answers late but inside the budget, here the oldest retry.
    let (qmp, events, serial, mut guest) = started_vm().await;
    let start = tokio::time::Instant::now();
    let task = readiness(qmp, serial, Shutdown::default());
    tokio::time::sleep(GUEST_READY_BUDGET - Duration::from_secs(1)).await;
    events.send(port(true)).unwrap();
    let challenge = guest_hello(&mut guest).await;
    guest_ack(&mut guest, &challenge).await;
    assert!(task.await.unwrap().0.is_ok());
    assert!(start.elapsed() >= GUEST_READY_BUDGET - Duration::from_secs(1));

    // Never answers: refused exactly at the budget.
    let (qmp, _events, serial, _guest) = started_vm().await;
    let start = tokio::time::Instant::now();
    let task = readiness(qmp, serial, Shutdown::default());
    assert_eq!(task.await.unwrap().0.err(), Some(ChannelError::Timeout));
    assert_eq!(start.elapsed(), GUEST_READY_BUDGET);
}

#[tokio::test(start_paused = true)]
async fn guest_readiness_refuses_unissued_garbage_and_stalled_acks() {
    // An ack written before any Hello, for a challenge never issued.
    let (qmp, _events, serial, mut guest) = started_vm().await;
    guest_ack(&mut guest, &"f".repeat(32)).await;
    let task = readiness(qmp, serial, Shutdown::default());
    assert_eq!(task.await.unwrap().0.err(), Some(ChannelError::Unexpected));
    // An oversized frame prefix, and a well-sized frame that is not a response.
    for garbage in [u32::MAX.to_be_bytes().to_vec(), b"\0\0\0\x05junk!".to_vec()] {
        let (qmp, events, serial, mut guest) = started_vm().await;
        let task = readiness(qmp, serial, Shutdown::default());
        events.send(port(true)).unwrap();
        guest_hello(&mut guest).await;
        guest.write_all(&garbage).await.unwrap();
        let error = task.await.unwrap().0.err();
        assert!(matches!(
            error,
            Some(ChannelError::Oversize | ChannelError::Invalid)
        ));
    }
    // A reply that starts and then stalls is bounded by FRAME_BOUND.
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events.send(port(true)).unwrap();
    guest_hello(&mut guest).await;
    tokio::time::sleep(Duration::from_secs(30)).await;
    guest.write_all(&[0]).await.unwrap();
    let started = tokio::time::Instant::now();
    assert_eq!(task.await.unwrap().0.err(), Some(ChannelError::Timeout));
    assert_eq!(started.elapsed(), FRAME_BOUND);
}

#[tokio::test(start_paused = true)]
async fn guest_readiness_stops_promptly_on_cancellation() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let cancel = Shutdown::default();
    let task = readiness(qmp, serial, cancel.clone());
    events.send(port(true)).unwrap();
    guest_hello(&mut guest).await;
    tokio::time::sleep(Duration::from_secs(30)).await;
    let cancelled_at = tokio::time::Instant::now();
    cancel.cancel();
    assert_eq!(task.await.unwrap().0.err(), Some(ChannelError::Cancelled));
    assert_eq!(cancelled_at.elapsed(), Duration::ZERO);
}

/// Tokio polls ready I/O before an expired timer. With the reply and the
/// deadline both ready on the first poll after expiry, readiness refuses.
#[tokio::test(start_paused = true)]
async fn a_reply_ready_together_with_the_expired_deadline_is_refused() {
    let (mut qmp, events, serial, mut guest) = started_vm().await;
    let cancel = Shutdown::default();
    let mut ready = Box::pin(GuestChannel::ready(serial, &mut qmp, &cancel));
    assert!(polled_pending(ready.as_mut())); // the deadline is fixed now
    events.send(port(true)).unwrap();
    // Let the QMP peer forward the event without polling `ready`.
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert!(polled_pending(ready.as_mut())); // open observed, Hello sent
    let challenge = guest_hello(&mut guest).await;
    guest_ack(&mut guest, &challenge).await; // the reply is ready
    tokio::time::advance(GUEST_READY_BUDGET).await; // expiry, `ready` unpolled
    assert_eq!(ready.await.err(), Some(ChannelError::Timeout));
}

fn identified(role: ArtifactRole, path: &str, file: u8) -> Artifact {
    artifact(role, path).with_identity(FileIdentity {
        volume: 1,
        file: [file; 16],
    })
}

fn pinned(role: ArtifactRole, path: &str, file: u8) -> Artifact {
    identified(role, path, file).with_pin(ArtifactPin {
        size: 1,
        sha256: [file; 32],
    })
}

fn block_arg(driver: &str, node: &str, path: &str, read_only: bool) -> Value {
    json!({
        "driver": driver, "node-name": node, "read-only": read_only,
        "file": {"driver": "file", "filename": path, "read-only": read_only}
    })
}

#[test]
fn exact_argv_for_each_boot_purpose() {
    use ArtifactRole::*;
    use InstallerPhase::*;
    let image = identified(Qemu, "E:/bundle/qemu-system-x86_64.exe", 1);
    let data = identified(Data, "E:/bundle/pc-bios", 2);
    let code = identified(FirmwareCode, "E:/bundle/code.fd", 3);
    let vars = identified(Vars, "D:/vms/vars.fd", 4);
    let setup_vars = pinned(WindowsSetupVars, "D:/vms/vars.fd", 4);
    let disk = identified(GuestDisk, "D:/vms/current.qcow2", 5);
    let test_disk = identified(TestInstallDisk, "D:/vms/current.qcow2", 5);
    let ubuntu_iso = pinned(UbuntuInstallIso, "E:/iso/ubuntu.iso", 6);
    let cidata = pinned(UbuntuCidata, "D:/vms/cidata.iso", 7);
    let kernel = pinned(UbuntuKernel, "E:/iso/vmlinuz", 8);
    let initrd = pinned(UbuntuInitrd, "E:/iso/initrd", 9);
    let windows_iso = pinned(WindowsInstallIso, "E:/iso/windows.iso", 10);
    let answer = pinned(WindowsAnswerSeed, "D:/vms/answer.iso", 11);
    let drivers = pinned(WindowsDriverMedium, "E:/iso/drivers.iso", 12);
    let golden = artifact(Golden, "D:/vms/golden.qcow2");
    let serial = SerialName::generate().unwrap();
    let header = qcow2_11_1(None);
    let backed = qcow2_11_1(Some(r"D:\vms\golden.qcow2"));
    let recipe = |boot, disk, vars| BootRecipe {
        image: &image,
        data: &data,
        firmware: &code,
        vars,
        disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot,
        ram_mib: 2048,
        vcpus: 2,
    };
    let installed = recipe(BootPurpose::Installed, &disk, &vars);
    let ubuntu = BootPurpose::UbuntuInstall {
        iso: &ubuntu_iso,
        cidata: &cidata,
        kernel: &kernel,
        initrd: &initrd,
    };
    let windows = BootPurpose::WindowsSetup {
        iso: &windows_iso,
        answer: &answer,
        drivers: &drivers,
    };
    for (mut recipe, phase, linked) in [
        (installed, None, false),
        (recipe(BootPurpose::Installed, &disk, &vars), None, true),
        (
            recipe(ubuntu, &test_disk, &vars),
            Some(UbuntuInstall),
            false,
        ),
        (
            recipe(windows, &test_disk, &setup_vars),
            Some(WindowsSetup),
            false,
        ),
        (
            recipe(BootPurpose::WindowsFinalize, &test_disk, &vars),
            Some(WindowsFinalize),
            false,
        ),
    ] {
        if linked {
            recipe.disk_header = &backed;
            recipe.backing = Some(&golden);
            recipe.backing_header = Some(&header);
        }
        let pipe = format!("pipe,id=arxguest,path={}", serial.basename());
        let mut expected = [
            "-no-user-config", "-nodefaults", "-display", "none", "-serial", "none",
            "-monitor", "none", "-accel", "whpx", "-S", "-chardev",
            "stdio,id=arxqmp,signal=off", "-mon", "chardev=arxqmp,mode=control",
            "-netdev", "user,id=arxnet0,restrict=on,ipv6=off",
            "-device", "virtio-net-pci,netdev=arxnet0",
            "-device", "virtio-serial-pci,id=arxserial",
            "-device", if phase == Some(WindowsSetup) {
                "virtio-blk-pci,drive=guestdisk"
            } else {
                "virtio-blk-pci,drive=guestdisk,bootindex=2"
            },
            "-L", r"E:\bundle\pc-bios", "-machine", "pc-q35-11.1,pflash0=code,pflash1=vars",
            "-smp", "2", "-m", "2048", "-chardev",
            if phase.is_none() { &pipe } else { "null,id=arxguest" },
            "-device", "virtserialport,bus=arxserial.0,chardev=arxguest,id=arxguestport,name=org.arxcovo.agent.0",
        ].map(str::to_owned).to_vec();
        for spec in [
            block_arg("raw", "code", r"E:\bundle\code.fd", true),
            block_arg("raw", "vars", r"D:\vms\vars.fd", false),
        ] {
            expected.extend(["-blockdev".into(), spec.to_string()]);
        }
        if linked {
            let mut spec = block_arg("qcow2", "golden", r"D:\vms\golden.qcow2", true);
            spec["backing"] = Value::Null;
            expected.extend(["-blockdev".into(), spec.to_string()]);
        }
        let mut spec = block_arg("qcow2", "guestdisk", r"D:\vms\current.qcow2", false);
        spec["backing"] = if linked { json!("golden") } else { Value::Null };
        expected.extend(["-blockdev".into(), spec.to_string()]);
        let mut optical = |node, path, device: &str| {
            expected.extend([
                "-blockdev".into(),
                block_arg("raw", node, path, true).to_string(),
                "-device".into(),
                device.into(),
            ]);
        };
        match phase {
            Some(UbuntuInstall) => {
                optical(
                    "osiso",
                    r"E:\iso\ubuntu.iso",
                    "ide-cd,drive=osiso,bus=ide.0",
                );
                optical(
                    "cidata",
                    r"D:\vms\cidata.iso",
                    "ide-cd,drive=cidata,bus=ide.1",
                );
                expected.extend(
                    [
                        "-kernel",
                        r"E:\iso\vmlinuz",
                        "-initrd",
                        r"E:\iso\initrd",
                        "-append",
                        "autoinstall ds=nocloud ---",
                    ]
                    .map(str::to_owned),
                );
            }
            Some(WindowsSetup) => {
                optical(
                    "osiso",
                    r"E:\iso\windows.iso",
                    "ide-cd,drive=osiso,bus=ide.0",
                );
                expected.extend(
                    ["-device", "ich9-ahci,id=arxmedia,bus=pcie.0,addr=0x1e"].map(str::to_owned),
                );
                for (node, path, device) in [
                    (
                        "answer",
                        r"D:\vms\answer.iso",
                        "ide-cd,drive=answer,bus=arxmedia.0",
                    ),
                    (
                        "drivers",
                        r"E:\iso\drivers.iso",
                        "ide-cd,drive=drivers,bus=arxmedia.1",
                    ),
                ] {
                    expected.extend([
                        "-blockdev".into(),
                        block_arg("raw", node, path, true).to_string(),
                        "-device".into(),
                        device.into(),
                    ]);
                }
            }
            None | Some(WindowsFinalize) => {}
        }
        let plan = recipe.plan().unwrap();
        assert_eq!(
            plan.kind(),
            phase.map_or(PlanKind::Boot, PlanKind::InstallerBoot)
        );
        assert_eq!(plan.args(), expected);
        assert!(plan.offline_lease().is_none());
    }

    let unpinned = identified(UbuntuKernel, "E:/iso/vmlinuz", 8);
    let aliased = pinned(UbuntuCidata, "E:/iso/alias.iso", 6);
    let same_path = pinned(UbuntuCidata, "e:/ISO/UBUNTU.ISO", 13);
    let missing_identity = artifact(WindowsAnswerSeed, "D:/vms/answer.iso").with_pin(ArtifactPin {
        size: 1,
        sha256: [11; 32],
    });
    for invalid in [
        recipe(ubuntu, &disk, &vars),
        recipe(windows, &test_disk, &vars),
        recipe(BootPurpose::WindowsFinalize, &test_disk, &setup_vars),
        recipe(BootPurpose::Installed, &test_disk, &vars),
        recipe(
            BootPurpose::UbuntuInstall {
                iso: &windows_iso,
                cidata: &cidata,
                kernel: &kernel,
                initrd: &initrd,
            },
            &test_disk,
            &vars,
        ),
        recipe(
            BootPurpose::UbuntuInstall {
                iso: &ubuntu_iso,
                cidata: &cidata,
                kernel: &unpinned,
                initrd: &initrd,
            },
            &test_disk,
            &vars,
        ),
        recipe(
            BootPurpose::UbuntuInstall {
                iso: &ubuntu_iso,
                cidata: &aliased,
                kernel: &kernel,
                initrd: &initrd,
            },
            &test_disk,
            &vars,
        ),
        recipe(
            BootPurpose::UbuntuInstall {
                iso: &ubuntu_iso,
                cidata: &same_path,
                kernel: &kernel,
                initrd: &initrd,
            },
            &test_disk,
            &vars,
        ),
        recipe(
            BootPurpose::WindowsSetup {
                iso: &windows_iso,
                answer: &missing_identity,
                drivers: &drivers,
            },
            &test_disk,
            &setup_vars,
        ),
    ] {
        assert!(invalid.plan().is_err());
    }
    let mut overlay = recipe(ubuntu, &test_disk, &vars);
    overlay.backing = Some(&golden);
    overlay.backing_header = Some(&header);
    overlay.disk_header = &backed;
    assert!(overlay.plan().is_err());
}

#[tokio::test(start_paused = true)]
async fn command_rejects_a_duplicate_late_ack() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events.send(port(true)).unwrap();
    let stale = guest_hello(&mut guest).await;
    events.send(port(false)).unwrap();
    events.send(port(true)).unwrap();
    let fresh = guest_hello(&mut guest).await;
    guest_ack(&mut guest, &fresh).await;
    let (result, _qmp) = task.await.unwrap();
    let mut channel = result.unwrap();

    let peer = tokio::spawn(async move {
        let Request::Ping { .. } = guest_request(&mut guest).await else {
            panic!("expected ping")
        };
        let mut duplicate = ack_frame(&stale);
        duplicate.extend(ack_frame(&stale));
        guest.write_all(&duplicate).await.unwrap();
        guest
    });
    assert_eq!(
        channel.ping(&Shutdown::default()).await,
        Err(ChannelError::Unexpected)
    );
    drop(peer.await.unwrap());
}

#[tokio::test(start_paused = true)]
async fn run_script_rejects_a_mismatched_response_id() {
    let (qmp, events, serial, mut guest) = started_vm().await;
    let task = readiness(qmp, serial, Shutdown::default());
    events.send(port(true)).unwrap();
    let challenge = guest_hello(&mut guest).await;
    guest_ack(&mut guest, &challenge).await;
    let (result, _qmp) = task.await.unwrap();
    let mut channel = result.unwrap();

    let peer = tokio::spawn(async move {
        let Request::RunScript { id, .. } = guest_request(&mut guest).await else {
            panic!("expected script request")
        };
        guest_reply(
            &mut guest,
            Response::ScriptResult {
                id: id + 1,
                status: arx_guest::ScriptStatus::Exited,
                exit_code: Some(0),
            },
        )
        .await;
        guest
    });
    assert_eq!(
        channel
            .run_script(&Shutdown::default(), "exit 0".into(), 1)
            .await,
        Err(ChannelError::Unexpected)
    );
    drop(peer.await.unwrap());
}

#[test]
fn static_web_plan_is_unbacked_and_forwards_exactly_one_loopback_port() {
    use ArtifactRole::*;
    use arxd::vm::static_web::{ORIGIN_PORT_FIRST, ORIGIN_PORT_LAST, OriginPortLease};
    let lease = OriginPortLease::acquire().unwrap();
    assert!((ORIGIN_PORT_FIRST..=ORIGIN_PORT_LAST).contains(&lease.port()));
    let image = artifact(Qemu, "D:/qemu/qemu-system-x86_64.exe");
    let data = artifact(Data, "D:/qemu/share");
    let firmware = artifact(FirmwareCode, "D:/firmware/code.fd");
    let vars = artifact(StaticVars, "D:/vm/vars.fd");
    let disk = artifact(StaticDisk, "D:/vm/disk.qcow2");
    let golden = artifact(Golden, "D:/vm/golden.qcow2");
    let header = qcow2_11_1(None);
    let serial = SerialName::generate().unwrap();
    let mut recipe = BootRecipe {
        image: &image,
        data: &data,
        firmware: &firmware,
        vars: &vars,
        disk: &disk,
        disk_header: &header,
        backing: None,
        backing_header: None,
        serial: &serial,
        boot: BootPurpose::InstalledStaticWeb(&lease, StaticNetwork::RestrictedOrigin),
        ram_mib: 256,
        vcpus: 1,
    };
    let plan = recipe.plan().unwrap();
    assert_eq!(plan.kind(), PlanKind::StaticWebBoot);
    assert_eq!(plan.static_origin_lease().unwrap().port(), lease.port());
    assert_eq!(
        plan.args()
            .windows(2)
            .filter(|pair| pair[0] == "-netdev")
            .map(|pair| &pair[1])
            .collect::<Vec<_>>(),
        vec![&format!(
            "user,id=arxnet0,restrict=on,ipv6=off,hostfwd=tcp:127.0.0.1:{}-10.0.2.15:443",
            lease.port()
        )]
    );
    recipe.backing = Some(&golden);
    recipe.backing_header = Some(&header);
    recipe.disk_header = &header;
    assert!(recipe.plan().is_err());
    recipe.backing = None;
    recipe.backing_header = None;
    recipe.boot = BootPurpose::Installed;
    assert!(
        recipe.plan().is_err(),
        "TEST plan acquired static custody roles"
    );
}

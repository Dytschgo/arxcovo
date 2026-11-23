#![cfg(target_os = "linux")]

use arxd::{
    service::Shutdown,
    vm::{
        paths::ArtifactPin,
        qmp::{Qmp, RunStatus},
    },
};
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path, process::Stdio, time::Duration};
use tokio::process::Command;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    qemu: String,
    qemu_size: u64,
    qemu_sha256: String,
    firmware: String,
    firmware_size: u64,
    firmware_sha256: String,
}

fn pinned(path: &str, size: u64, digest: &str, bound: u64) {
    let path = Path::new(path);
    assert!(path.is_absolute() && path.canonicalize().unwrap() == path);
    assert!(path.is_file());
    assert!(
        ArtifactPin {
            size,
            sha256: hex::decode(digest).unwrap().try_into().unwrap()
        }
        .verify(File::open(path).unwrap(), bound)
        .unwrap()
    );
}

#[tokio::test]
#[ignore = "manual TCG smoke with an explicitly supplied owner-staged QEMU 11.1.3 manifest"]
async fn optional_tcg_qmp_startup_hold() {
    let manifest =
        std::env::var_os("ARX_M4_TCG_MANIFEST").expect("explicit TCG fixture manifest is required");
    let manifest = Path::new(&manifest);
    assert!(manifest.is_absolute() && manifest.canonicalize().unwrap() == manifest);
    let mut bytes = Vec::new();
    File::open(manifest)
        .unwrap()
        .take(4097)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 4096);
    let fixture: Fixture = serde_json::from_slice(&bytes).unwrap();
    pinned(
        &fixture.qemu,
        fixture.qemu_size,
        &fixture.qemu_sha256,
        512 * 1024 * 1024,
    );
    pinned(
        &fixture.firmware,
        fixture.firmware_size,
        &fixture.firmware_sha256,
        16 * 1024 * 1024,
    );
    let scratch = tempfile::tempdir().unwrap();
    let mut child = Command::new(&fixture.qemu)
        .env_clear()
        .current_dir(scratch.path())
        .args([
            "-no-user-config",
            "-nodefaults",
            "-display",
            "none",
            "-serial",
            "none",
            "-monitor",
            "none",
            "-machine",
            "pc-q35-11.1",
            "-accel",
            "tcg",
            "-S",
            "-nic",
            "none",
            "-bios",
            &fixture.firmware,
            "-chardev",
            "stdio,id=arxqmp,signal=off",
            "-mon",
            "chardev=arxqmp,mode=control",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut qmp = Qmp::handshake(
        child.stdout.take().unwrap(),
        child.stdin.take().unwrap(),
        &Shutdown::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        qmp.status(&Shutdown::default()).await,
        Ok(RunStatus::Prelaunch)
    );
    child.start_kill().unwrap();
    tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap();
}

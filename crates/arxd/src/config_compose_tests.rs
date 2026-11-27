use super::*;
use crate::{
    mcp::vm::Supervisor,
    store::vm::VmStore,
    vm::compose_secrets::{Dpapi, SecretSource},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    fs,
    process::{Command, Stdio},
};

fn icacls(path: &Path, args: &[&str]) {
    assert!(
        Command::new("icacls")
            .arg(path)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
}

fn set_owner(path: &Path) {
    icacls(
        path,
        &[
            "/setowner",
            &format!("*{}", crate::local::current_identity().unwrap()),
        ],
    );
}

fn measured(path: &Path) -> ExpectedVmStore {
    fixture_directory_pin(&pin_directory(path).unwrap())
}

pub(crate) fn fixture_directory_pin(file: &std::fs::File) -> ExpectedVmStore {
    let id = full_directory_identity(file).unwrap();
    let name = held_volume_path(file).unwrap();
    let name = name.to_str().unwrap();
    ExpectedVmStore {
        volume_guid: name[..name.find(r"}\").unwrap() + 2]
            .to_ascii_lowercase()
            .replacen("volume", "Volume", 1),
        volume_serial: format!("{:016x}", id.volume),
        directory_file_id: hex::encode(id.file),
    }
}

struct Roots {
    dir: tempfile::TempDir,
    state: PathBuf,
    secrets: PathBuf,
    config: PathBuf,
}

impl Roots {
    fn new() -> Self {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let path = Path::new(dir.path().to_str().unwrap().trim_start_matches(r"\\?\"));
        icacls(
            path,
            &[
                "/inheritance:r",
                "/grant:r",
                &format!("*{}:(OI)(CI)F", crate::local::current_identity().unwrap()),
                "*S-1-5-18:(OI)(CI)F",
                "*S-1-5-32-544:(OI)(CI)F",
            ],
        );
        set_owner(path);
        let state = path.join("state");
        let secrets = path.join("state-secrets");
        let config = path.join("foreground.json");
        for root in [&state, &secrets] {
            fs::create_dir(root).unwrap();
            set_owner(root);
        }
        fs::write(&config, []).unwrap();
        set_owner(&config);
        Self {
            dir,
            state,
            secrets,
            config,
        }
    }

    fn profile(&self) -> Value {
        json!({
            "state_dir": self.state,
            "local": {
                "address": r"\\.\pipe\arxcovo-custody-fixture",
                "client_identity": "S-1-5-21-100-200-300-1001"
            },
            "agents": [{"principal": "fixture", "token_sha256": "a".repeat(64)}],
            "test_vms": {
                "enabled": false,
                "expected_store": measured(&self.state),
                "compose_secret_root": self.secrets,
                "expected_compose_secret_root": measured(&self.secrets),
            }
        })
    }

    fn load(&self, profile: &Value) -> Result<VmForeground> {
        fs::write(&self.config, serde_json::to_vec(profile).unwrap()).unwrap();
        match ForegroundConfig::load(&self.config)? {
            ForegroundConfig::TestVms(vm) => Ok(vm),
            ForegroundConfig::Inventory(_) => panic!("VM profile required"),
        }
    }
}

fn release_count(root: &Path) -> i64 {
    VmStore::open(root, 1)
        .unwrap()
        .transact(|tx| {
            Ok(
                tx.query_row("SELECT count(*) FROM vm_compose_releases", [], |row| {
                    row.get(0)
                })?,
            )
        })
        .unwrap()
}

/// DELETE access, sharing read, write and delete. A pin that omits
/// `FILE_SHARE_DELETE` refuses this even when it still shares nothing writable.
fn open_for_delete(path: &Path) -> std::io::Result<fs::File> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { fs::File::from_raw_handle(handle) })
    }
}

#[test]
fn held_roots_and_ancestors_deny_competing_directory_writes() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::ERROR_SHARING_VIOLATION,
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
    };

    let roots = Roots::new();
    let writer = |path: &Path| {
        fs::OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    };
    let assert_sharing_denied = |open: std::io::Result<fs::File>| {
        assert_eq!(
            open.unwrap_err().raw_os_error(),
            Some(ERROR_SHARING_VIOLATION as i32)
        );
    };
    let parent = roots.state.parent().unwrap();
    let paths = [roots.state.as_path(), roots.secrets.as_path(), parent];
    for path in paths {
        drop(writer(path).expect("disposable directory supports a write open"));
        drop(open_for_delete(path).expect("disposable directory supports a delete open"));
    }

    // Verify both the absolute anchoring open and the production relative walk.
    // Delete-only access is separate from the write open: sharing delete without
    // sharing write still admits this probe.
    let anchor = pin_directory(parent).unwrap();
    assert_sharing_denied(writer(parent));
    assert_sharing_denied(open_for_delete(parent));
    drop(anchor);

    let vm = roots.load(&roots.profile()).unwrap();
    for path in paths {
        assert_sharing_denied(writer(path));
        assert_sharing_denied(open_for_delete(path));
    }
    fs::write(roots.state.join("new-child"), b"fixture").unwrap();
    vm.custody.verify().unwrap();
    drop(vm);
    for path in paths {
        drop(writer(path).expect("write open succeeds after custody ends"));
        drop(open_for_delete(path).expect("delete open succeeds after custody ends"));
    }
}

#[test]
fn opens_enrolled_store_and_dpapi_and_enrolls_release_there() {
    let roots = Roots::new();
    let canary = protect(0);
    let reference = write_blob(&roots.secrets, &canary);
    let mut profile = roots.profile();
    profile["test_vms"]["compose_releases"] = json!([{
        "vm_id": "01".repeat(16),
        "release_id": "02".repeat(16),
        "secrets": [{"slot": "password", "epoch": 1, "reference": hex::encode(reference)}],
        "manifest": {
            "schema": arx_guest::compose::MANIFEST_SCHEMA,
            "project_id": "fixture",
            "volumes": [], "secrets": ["password"],
            "services": [{
                "name":"api",
                "image": format!("example.invalid/api@sha256:{}", "03".repeat(32)),
                "platform": arx_guest::compose::PLATFORM,
                "uid": 10001, "gid": 10001, "read_only_rootfs":true,
                "environment":[], "mounts":[], "secrets":["password"],
                "limits":{"cpu_millis":500,"memory_mib":512,"pids":32}
            }]
        },
    }]);
    VmStore::open(&roots.state, 1)
        .unwrap()
        .transact(|tx| {
            tx.execute_batch(
                "INSERT INTO vm_static_instances
                (id, name, principal, host, template, source_generation, recipe_digest,
                 disk_digest, vars_digest, state, persistent, staging)
                SELECT X'01010101010101010101010101010101','fixture','fixture',id,
                       zeroblob(16),zeroblob(16),zeroblob(32),zeroblob(32),zeroblob(32),1,1,0
                FROM vm_state_host",
            )?;
            Ok(())
        })
        .unwrap();
    let vm = roots.load(&profile).unwrap();
    let root_path = vm.custody.state_path(&vm.config.state_dir);
    let source = Dpapi::new(vm.custody.clone());
    let supervisor = root_path
        .access(|root| {
            assert!(root.to_str().unwrap().starts_with(r"\\?\Volume{"));
            Supervisor::open(root, vm.profile, vm.custody)
        })
        .unwrap();
    assert!(source.open(reference).unwrap().as_slice() == CANARY);
    supervisor.verify_roots().unwrap();
    drop(supervisor);
    assert_eq!(release_count(&roots.state), 1);
    assert!(!roots.secrets.join("activity.sqlite3").exists());
}

#[test]
fn mismatches_and_incomplete_pins_refuse_before_writes() {
    let roots = Roots::new();
    let profile = roots.profile();
    for key in ["expected_store", "expected_compose_secret_root"] {
        for (field, value) in [
            (
                "volume_guid",
                json!(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\"),
            ),
            ("volume_serial", json!("0".repeat(16))),
            ("directory_file_id", json!("0".repeat(32))),
            ("directory_file_id", json!("abcd")),
        ] {
            let mut changed = profile.clone();
            changed["test_vms"][key][field] = value;
            assert!(roots.load(&changed).is_err(), "{key} {field}");
        }
        let mut changed = profile.clone();
        changed["test_vms"][key] = Value::Null;
        assert!(roots.load(&changed).is_err());
    }
    for key in [
        "expected_store",
        "expected_compose_secret_root",
        "compose_secret_root",
    ] {
        let mut changed = profile.clone();
        changed["test_vms"].as_object_mut().unwrap().remove(key);
        assert!(roots.load(&changed).is_err());
    }
    assert_eq!(fs::read_dir(&roots.state).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&roots.secrets).unwrap().count(), 0);
}

#[test]
fn aliases_ancestry_and_reparse_paths_are_refused() {
    let roots = Roots::new();
    let profile = roots.profile();
    assert!(roots.load(&profile).is_ok(), "adjacent sibling names");
    let mut alternate = profile.clone();
    alternate["test_vms"]["compose_secret_root"] = json!(
        roots
            .secrets
            .to_str()
            .unwrap()
            .to_ascii_lowercase()
            .replace('\\', "/")
    );
    assert!(
        roots.load(&alternate).is_ok(),
        "ordinary drive and slash spelling"
    );
    let mut check_pair = |state: &Path, secrets: &Path, expected_secrets: &Path| {
        alternate["state_dir"] = json!(state);
        alternate["test_vms"]["expected_store"] = json!(measured(state));
        alternate["test_vms"]["compose_secret_root"] = json!(secrets);
        alternate["test_vms"]["expected_compose_secret_root"] = json!(measured(expected_secrets));
        assert!(roots.load(&alternate).is_err());
    };
    let same = PathBuf::from(roots.state.to_str().unwrap().to_ascii_lowercase());
    check_pair(&roots.state, &same, &roots.state);
    let nested = roots.state.join("nested");
    fs::create_dir(&nested).unwrap();
    set_owner(&nested);
    check_pair(&roots.state, &nested, &nested);
    check_pair(&nested, &roots.state, &roots.state);

    let alias = roots.dir.path().join("alias");
    let alias = PathBuf::from(alias.to_str().unwrap().trim_start_matches(r"\\?\"));
    assert!(
        Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&alias)
            .arg(&roots.state)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    check_pair(&roots.secrets, &alias, &roots.state);
    check_pair(&roots.secrets, &alias.join("nested"), &nested);
    fs::remove_dir(&alias).unwrap();

    let raw = roots.secrets.to_str().unwrap();
    alternate = profile;
    for path in [format!("{raw}\\."), format!(r"\\?\{raw}")] {
        alternate["test_vms"]["compose_secret_root"] = json!(path);
        assert!(roots.load(&alternate).is_err());
    }
}

#[test]
fn static_compose_custody_verifies_four_disjoint_pins_before_opening_state() {
    use crate::vm::runtime::NativeRefusal;

    let roots = Roots::new();
    let parent = roots.state.parent().unwrap();
    let dossier = parent.join("dossier");
    let backup = parent.join("backup");
    for path in [&dossier, &backup] {
        fs::create_dir(path).unwrap();
        set_owner(path);
    }
    for path in [&roots.state, &roots.secrets, &dossier, &backup] {
        icacls(path, &["/inheritance:d"]);
    }
    let seal = b"synthetic seal for custody only";
    fs::write(dossier.join("qualification.json"), seal).unwrap();
    let enrollment = parent.join("docker.json");
    fs::write(&enrollment, br#"{"class":"docker"}"#).unwrap();
    set_owner(&enrollment);
    let backup_json = json!({
        "root": backup,
        "expected_root": measured(&backup),
        "targets": [{
            "vm_id": "aa".repeat(16), "project_id":"app", "volume":"data",
            "database":"store.sqlite3", "max_bytes":8192
        }]
    });
    let mut raw = roots.profile();
    raw["test_vms"]["enabled"] = json!(true);
    raw["test_vms"]["static_compose"] = json!({
        "selection": {
            "dossier": dossier,
            "expected_dossier_root": measured(&dossier),
            "enrollment": enrollment,
            "seal_sha256": hex::encode(Sha256::digest(seal)),
            "scope": "aa".repeat(32)
        }
    });
    // Exercise the root holder on CI even if its temp volume is C:.
    // The separate profile validation test checks the D: restriction.
    let flat = |profile: &Value| {
        let mut vm: TestVms = serde_json::from_value(profile["test_vms"].clone()).unwrap();
        let mut static_profile = vm.static_compose.take().unwrap();
        static_profile.backup = Some(serde_json::from_value(profile["backup"].clone()).unwrap());
        let state = RootPin::acquire_enrolled(&roots.state, vm.expected_store.as_ref().unwrap())?;
        let secret = ComposeRoot {
            configured: roots.secrets.clone(),
            pin: RootPin::acquire_enrolled(
                &roots.secrets,
                &vm.expected_compose_secret_root.as_ref().unwrap().0,
            )?,
        };
        StaticComposeCustody::acquire(&static_profile, &roots.state, &state, Some(&secret))
    };
    raw["backup"] = backup_json;
    let held = flat(&raw).unwrap();
    held.verify().unwrap();
    assert!(open_for_delete(&backup).is_err());
    assert!(open_for_delete(&dossier).is_err());
    drop(held);

    for (pointer, value) in [
        (
            "/test_vms/expected_store/directory_file_id",
            json!("00".repeat(16)),
        ),
        (
            "/test_vms/expected_compose_secret_root/volume_serial",
            json!("00".repeat(8)),
        ),
        (
            "/test_vms/static_compose/selection/expected_dossier_root/directory_file_id",
            json!("00".repeat(16)),
        ),
        (
            "/test_vms/static_compose/selection/seal_sha256",
            json!("11".repeat(32)),
        ),
        (
            "/backup/expected_root/volume_guid",
            json!(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\"),
        ),
    ] {
        let mut wrong = raw.clone();
        *wrong.pointer_mut(pointer).unwrap() = value;
        assert!(flat(&wrong).is_err(), "{pointer}");
    }
    // Identical and nested physical roots must both fail with correct pins.
    let nested = roots.secrets.join("nested");
    fs::create_dir(&nested).unwrap();
    set_owner(&nested);
    icacls(&nested, &["/inheritance:d"]);
    for overlap in [&roots.state, &roots.secrets, &dossier, &nested] {
        let mut wrong = raw.clone();
        wrong["backup"]["root"] = json!(overlap);
        wrong["backup"]["expected_root"] = json!(measured(overlap));
        assert!(flat(&wrong).is_err());
    }
    let junction = parent.join("dossier-link");
    assert!(
        Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&dossier)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    let mut wrong = raw.clone();
    wrong["test_vms"]["static_compose"]["selection"]["dossier"] = json!(junction);
    assert!(flat(&wrong).is_err());
    fs::remove_dir(&junction).unwrap();

    icacls(&backup, &["/inheritance:e"]);
    assert!(
        flat(&raw).is_err(),
        "private ACL must be protected from inheritance"
    );

    raw.as_object_mut().unwrap().remove("backup");
    let vm = roots.load(&raw).unwrap();
    assert_eq!(
        vm.custody.select_static_dossier().err(),
        Some(NativeRefusal::NetworkFenceUnverified),
    );
    vm.custody.verify().unwrap();
    assert!(!roots.state.join("activity.sqlite3").exists());
}

const CANARY: &[u8] = b"synthetic compose custody fixture";

fn protect(flags: u32) -> Vec<u8> {
    use std::ptr::null;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData},
    };
    let mut bytes = CANARY.to_vec();
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        assert_ne!(
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                flags | CRYPTPROTECT_UI_FORBIDDEN,
                &mut output
            ),
            0
        );
        let blob = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        blob
    }
}

fn blob_path(directory: &Path, reference: crate::vm::Id) -> PathBuf {
    directory.join(format!("{}.dpapi", hex::encode(reference)))
}

fn write_blob(directory: &Path, blob: &[u8]) -> crate::vm::Id {
    let reference = Sha256::digest(blob)[..16].try_into().unwrap();
    let path = blob_path(directory, reference);
    fs::write(&path, blob).unwrap();
    set_owner(&path);
    reference
}

fn unavailable(result: Result<zeroize::Zeroizing<Vec<u8>>>) {
    assert_eq!(
        result.err().unwrap().to_string(),
        "Compose secret unavailable"
    );
}

#[test]
fn held_dpapi_rechecks_bindings_and_refuses_invalid_leaves() {
    use windows_sys::Win32::Security::Cryptography::CRYPTPROTECT_LOCAL_MACHINE;
    let roots = Roots::new();
    let vm = roots.load(&roots.profile()).unwrap();
    let source = Dpapi::new(vm.custody.clone());
    let blob = protect(0);
    let reference = write_blob(&roots.secrets, &blob);
    let path = blob_path(&roots.secrets, reference);
    let held = vm.custody.open_ciphertext(reference).unwrap();
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    assert!(fs::rename(&path, path.with_extension("moved")).is_err());
    drop(held);
    let oversized = write_blob(&roots.secrets, &vec![0; 16385]);
    let machine = write_blob(&roots.secrets, &protect(CRYPTPROTECT_LOCAL_MACHINE));
    for reference in [[0; 16], oversized, machine] {
        unavailable(source.open(reference));
    }
    fs::write(&path, b"replaced fixture").unwrap();
    unavailable(source.open(reference));
    fs::write(&path, blob).unwrap();
    icacls(&path, &["/grant", "*S-1-1-0:R"]);
    unavailable(source.open(reference));
    assert!(
        !vm.custody.lost.is_cancelled(),
        "leaf failure preserves root custody"
    );
    icacls(&path, &["/remove:g", "*S-1-1-0"]);
    assert!(source.open(reference).unwrap().as_slice() == CANARY);
    assert!(fs::rename(&roots.secrets, roots.secrets.with_extension("moved")).is_err());
    assert!(fs::rename(&roots.state, roots.state.with_extension("moved")).is_err());

    let boundary = Cell::new(0);
    unavailable(source.read_with_binding_check(reference, || {
        boundary.set(boundary.get() + 1);
        let name = if boundary.get() == 1 {
            &roots.secrets
        } else {
            &roots.state
        };
        vm.custody.verify_pair(&roots.state, name)
    }));
    assert_eq!(boundary.get(), 2);
    assert!(vm.custody.lost.is_cancelled());
    unavailable(source.open(reference));
}

#[test]
fn root_acl_drift_after_load_refuses_before_supervisor_open() {
    let roots = Roots::new();
    let vm = roots.load(&roots.profile()).unwrap();
    icacls(&roots.secrets, &["/grant", "*S-1-1-0:R"]);
    let state = vm.custody.state_path(&vm.config.state_dir);
    assert!(
        state
            .access(|path| Supervisor::open(path, vm.profile, vm.custody))
            .is_err()
    );
    assert_eq!(fs::read_dir(&roots.state).unwrap().count(), 0);
}

#[test]
fn static_launch_copy_holds_independent_pins_without_altering_source() {
    let roots = Roots::new();
    let mut profile = roots.profile();
    let source_profile = profile["test_vms"].as_object_mut().unwrap();
    source_profile.remove("expected_store");
    source_profile.remove("expected_compose_secret_root");
    let source = serde_json::to_vec(&profile).unwrap();
    fs::write(&roots.config, &source).unwrap();
    let store = measured(&roots.state);
    let secret = ExpectedComposeSecretRoot(measured(&roots.secrets));
    let launch = pinned_static_vm_config(&roots.config, &store, &secret).unwrap();
    launch.verify().unwrap();
    assert!(fs::rename(&roots.secrets, roots.secrets.with_extension("moved")).is_err());
    assert_eq!(fs::read(&roots.config).unwrap(), source);
    let copy: Value = serde_json::from_slice(launch.bytes()).unwrap();
    assert!(roots.load(&copy).is_ok());
    assert!(pinned_static_vm_config(&roots.config, &store, &secret).is_err());
    assert_eq!(fs::read_dir(&roots.state).unwrap().count(), 0);
}

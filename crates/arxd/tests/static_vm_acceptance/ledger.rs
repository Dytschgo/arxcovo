use super::*;
use arxd::{config::EnrolledVmStore, vm::paths::FileIdentity};
use rusqlite::{Connection, OpenFlags, params};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions, read_dir},
    io::{self, Read},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::*;

pub(super) type Id = [u8; 16];
type Hash = [u8; 32];

const MAX_ARTIFACT: u64 = arx_guest::compose_backup::MAX_BACKUP_BYTES;

pub(super) fn safe<T, E>(result: std::result::Result<T, E>, label: &'static str) -> Result<T> {
    result.map_err(|_| Failure::Check(label))
}

pub(super) fn id(row: &Value, key: &'static str) -> Result<Id> {
    let hex_str = opaque(&row[key], 32).ok_or(Failure::Check(key))?;
    let mut out = [0; 16];
    safe(hex::decode_to_slice(hex_str, &mut out), key)?;
    check(out != [0; 16], key)?;
    Ok(out)
}

pub(super) fn digest(file: &File, max: u64) -> Result<(u64, Hash)> {
    let mut sha = Sha256::new();
    let copied = safe(io::copy(&mut file.take(max + 1), &mut sha), "hash_read")?;
    check(copied > 0 && copied <= max, "hash_bound")?;
    Ok((copied, sha.finalize().into()))
}

pub(super) fn config<T: for<'a> Deserialize<'a>>(file: &File) -> Result<T> {
    let mut bytes = Vec::new();
    safe(file.take(65_537).read_to_end(&mut bytes), "input_read")?;
    check(bytes.len() <= 65_536, "input_bound")?;
    safe(serde_json::from_slice(&bytes), "input_format")
}

#[derive(PartialEq, Eq)]
pub(super) struct Identity {
    pub volume: [u8; 8],
    pub file: Id,
}

pub(super) fn identity(
    handle: windows_sys::Win32::Foundation::HANDLE,
    directory: bool,
) -> Result<Identity> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    let mut info = FILE_ID_INFO::default();
    let valid = unsafe {
        GetFileType(handle) == FILE_TYPE_DISK
            && GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            ) != 0
            && GetFileInformationByHandleEx(
                handle,
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            ) != 0
            && GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            ) != 0
    };
    check(
        valid
            && tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
            && standard.Directory == directory
            && !standard.DeletePending
            && (directory || standard.NumberOfLinks == 1),
        "unsafe_file_identity",
    )?;
    Ok(Identity {
        volume: info.VolumeSerialNumber.to_be_bytes(),
        file: info.FileId.Identifier,
    })
}

pub(super) fn open(path: &Path, directory: bool, share: u32) -> Result<File> {
    safe(arxd::config::checked_path(path), "unsafe_local_path")?;
    let file = safe(
        OpenOptions::new()
            .read(true)
            .share_mode(share)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT
                    | if directory {
                        FILE_FLAG_BACKUP_SEMANTICS
                    } else {
                        0
                    },
            )
            .open(path),
        "local_open",
    )?;
    identity(file.as_raw_handle(), directory)?;
    Ok(file)
}

/// Hold the entire root chain without delete sharing while opening children.
pub(super) fn pin_root(root: &Path) -> Result<Vec<File>> {
    let chain: Vec<_> = root.ancestors().filter(|path| path.has_root()).collect();
    chain
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .rev()
        .map(|path| open(path, true, FILE_SHARE_READ | FILE_SHARE_WRITE))
        .collect()
}

pub(super) fn store_identity(file: &File) -> Result<FileIdentity> {
    let mut info = FILE_ID_INFO::default();
    check(
        unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                std::ptr::from_mut(&mut info).cast(),
                size_of_val(&info) as u32,
            )
        } != 0,
        "store_file_identity",
    )?;
    Ok(FileIdentity {
        volume: info.VolumeSerialNumber,
        file: info.FileId.Identifier,
    })
}

fn built_vm_schema_version() -> Result<u32> {
    include_str!("../../src/store/vm.rs")
        .lines()
        .find_map(|line| line.trim().strip_prefix("const VM_SCHEMA_VERSION: u32 ="))
        .and_then(|value| value.trim().strip_suffix(';'))
        .and_then(|value| value.parse().ok())
        .ok_or(Failure::Check("built_ledger_version"))
}

pub(super) struct Ledger {
    db: Connection,
    root: PathBuf,
    _pins: Vec<File>,
    _database: File,
}

pub(super) struct LiveLedger {
    ledger: Ledger,
    store: EnrolledVmStore,
}

impl LiveLedger {
    pub fn open(configured: &Path, expected: &ExpectedVmStore) -> Result<Self> {
        let store = safe(
            EnrolledVmStore::open(configured, expected),
            "foreground_store_binding",
        )?;
        let ledger = safe(store.read(Ledger::open), "foreground_store_binding")??;
        Ok(Self { ledger, store })
    }

    pub fn read<T>(&self, batch: impl FnOnce(&Ledger) -> Result<T>) -> Result<T> {
        safe(
            self.store.read(|_| batch(&self.ledger)),
            "foreground_store_binding",
        )?
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct StaticVmLedger {
    pub generation: Id,
    pub persistent_charge: i64,
    disk_file: Id,
    vars_file: Id,
}

pub(super) struct SettledBackupLedger {
    _pins: (File, File),
    pub settled_path: PathBuf,
}

impl Ledger {
    /// Preflight a stopped VM store before foreground launch. Accepts an empty
    /// directory or an idle v15 store with no in-flight operations, no active
    /// publication intent, and no foreign files.
    pub fn preflight(state: &Path) -> Result<()> {
        let _state = pin_root(state)?;
        match state.join("activity.sqlite3").symlink_metadata() {
            Ok(_) => {
                for suffix in ["-journal", "-wal", "-shm"] {
                    match state
                        .join(format!("activity.sqlite3{suffix}"))
                        .symlink_metadata()
                    {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        _ => return Err(Failure::Check("unsettled_startup_ledger")),
                    }
                }
                let ledger = Self::open(state)?;
                ledger.quiet_for_static()?;
                ledger.publication_empty()?;
                ledger.no_foreign_root_entries()
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => check(
                safe(read_dir(state), "fresh_state")?.next().is_none(),
                "fresh_state",
            ),
            Err(_) => Err(Failure::Check("ledger_preflight")),
        }
    }

    pub fn open(state: &Path) -> Result<Self> {
        let pins = pin_root(state)?;
        let path = state.join("activity.sqlite3");
        let database = open(&path, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
        check(
            safe(database.metadata(), "ledger_metadata")?.len() <= 4 * 1024 * 1024,
            "ledger_size_bound",
        )?;
        for suffix in ["-journal", "-wal", "-shm"] {
            let sidecar = state.join(format!("activity.sqlite3{suffix}"));
            match sidecar.symlink_metadata() {
                Ok(_) => {
                    open(
                        &sidecar,
                        false,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    )?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(Failure::Check("ledger_sidecar")),
            }
        }
        let db = safe(
            Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_NOFOLLOW
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ),
            "ledger_open",
        )?;
        let mut handle: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        let rc = unsafe {
            rusqlite::ffi::sqlite3_file_control(
                db.handle(),
                c"main".as_ptr(),
                rusqlite::ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                std::ptr::from_mut(&mut handle).cast(),
            )
        };
        check(
            rc == rusqlite::ffi::SQLITE_OK
                && identity(handle, false)? == identity(database.as_raw_handle(), false)?,
            "ledger_identity",
        )?;
        safe(db.busy_timeout(Duration::from_secs(1)), "ledger_timeout")?;
        check(
            safe(
                db.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0)),
                "ledger_version",
            )? == built_vm_schema_version()?,
            "ledger_version",
        )?;
        Ok(Self {
            db,
            root: state.to_path_buf(),
            _pins: pins,
            _database: database,
        })
    }

    pub fn quiet_for_static(&self) -> Result<()> {
        check(
            safe(
                self.db.query_row(
                    "SELECT (SELECT count(*) FROM vm_instances)
                  + (SELECT count(*) FROM vm_static_instances)
                  + (SELECT count(*) FROM vm_build_intents WHERE state<>'sealed')
                  + (SELECT count(*) FROM vm_operations WHERE status=0)
                  + (SELECT count(*) FROM vm_compose_operations WHERE receipt IS NULL AND closed IS NULL)
                  + (SELECT count(*) FROM vm_compose_backup_ops WHERE status='pending')
                  + (SELECT count(*) FROM vm_compose_backups WHERE state IN ('staging','partial_uncertain'))",
                    [],
                    |r| r.get::<_, i64>(0),
                ),
                "ledger_quiet",
            )? == 0,
            "ledger_not_isolated",
        )
    }

    pub fn publication_empty(&self) -> Result<()> {
        check(
            safe(
                self.db.query_row(
                    "SELECT (SELECT count(*) FROM vm_publication)
                  + (SELECT count(*) FROM vm_publication_hosts)",
                    [],
                    |r| r.get::<_, i64>(0),
                ),
                "publication_empty",
            )? == 0,
            "publication_not_empty",
        )
    }

    pub fn no_foreign_root_entries(&self) -> Result<()> {
        for (number, entry) in safe(read_dir(&self.root), "media_inventory")?.enumerate() {
            check(number < 256, "media_inventory_bound")?;
            let path = safe(entry, "media_inventory")?.path();
            let name = path.file_name().and_then(|n| n.to_str());
            if matches!(
                name,
                Some(
                    "activity.sqlite3"
                        | "activity.lock"
                        | "vault.sqlite3"
                        | "vault.lock"
                        | "vault.sqlite3-journal"
                )
            ) {
                open(&path, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
                continue;
            }
            check(opaque(&json!(name), 32).is_some(), "unattributed_media")?;
            let _dir = open(&path, true, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
        }
        Ok(())
    }

    pub fn static_vm(&self, vm: Id, name: &str, principal: &str) -> Result<StaticVmLedger> {
        let not_ephemeral = safe(
            self.db
                .query_row("SELECT count(*) FROM vm_instances WHERE id=?1", [vm], |r| {
                    r.get::<_, i64>(0)
                }),
            "static_not_ephemeral",
        )?;
        check(not_ephemeral == 0, "static_not_ephemeral")?;

        let (generation, persistent_charge, disk_file, vars_file): (Id, i64, Id, Id) = safe(
            self.db.query_row(
                "SELECT g.id, s.persistent, d.file_id, v.file_id
                 FROM vm_static_instances s
                 JOIN vm_generations g ON g.static_id=s.id AND g.active=1
                 JOIN vm_artifacts d ON d.generation_id=g.id AND d.role='static_disk' AND d.state='active'
                 JOIN vm_artifacts v ON v.generation_id=g.id AND v.role='static_vars' AND v.state='active'
                 WHERE s.id=?1 AND s.name=?2 AND s.principal=?3 AND s.state IN (1,2)",
                params![vm, name, principal],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            ),
            "static_vm_ledger",
        )?;
        check(
            persistent_charge > 0 && disk_file != vars_file,
            "static_vm_pair",
        )?;
        Ok(StaticVmLedger {
            generation,
            persistent_charge,
            disk_file,
            vars_file,
        })
    }

    pub fn compose_volumes_intact(&self, vm: Id, project: &str) -> Result<i64> {
        let count = safe(
            self.db.query_row(
                "SELECT count(*) FROM vm_compose_volumes WHERE vm=?1 AND project=?2",
                params![vm, project],
                |r| r.get::<_, i64>(0),
            ),
            "compose_volumes",
        )?;
        check(count > 0, "compose_volumes_missing")?;
        Ok(count)
    }

    /// Verify a settled Compose backup row in `vm_compose_backups` and match
    /// its recorded digest, size, and NTFS file identity against the physical
    /// settled file under `backup_root/settled/<backup_id>.sqlite3`.
    pub fn compose_backup(
        &self,
        vm: Id,
        project: &str,
        backup_id: Id,
        expected_size: u64,
        expected_sha256: &str,
        backup_root: &Path,
    ) -> Result<SettledBackupLedger> {
        let (size, sha, retained, volume_id, dir_id, file_id): (u64, Hash, i64, [u8; 8], Id, Id) =
            safe(
                self.db.query_row(
                    "SELECT size_bytes, sha256, retained_charge, volume_id, directory_id, file_id
                 FROM vm_compose_backups
                 WHERE id=?1 AND vm=?2 AND project=?3 AND state='settled'
                   AND host_staging_charge=0 AND guest_staging_charge=0 AND settled_ms IS NOT NULL",
                    params![backup_id, vm, project],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)? as u64,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                        ))
                    },
                ),
                "settled_backup_ledger",
            )?;
        check(
            size == expected_size && hex::encode(sha) == expected_sha256 && retained == size as i64,
            "settled_backup_pin",
        )?;

        let settled_dir = backup_root.join("settled");
        let dir_handle = open(&settled_dir, true, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
        let dir_ident = identity(dir_handle.as_raw_handle(), true)?;
        let settled_path = settled_dir.join(format!("{}.sqlite3", hex::encode(backup_id)));
        let file_handle = open(&settled_path, false, FILE_SHARE_READ)?;
        let file_ident = identity(file_handle.as_raw_handle(), false)?;
        check(
            file_ident.volume == volume_id
                && file_ident.file == file_id
                && dir_ident.volume == volume_id
                && dir_ident.file == dir_id,
            "settled_backup_identity",
        )?;
        let (actual_len, actual_sha) = digest(&file_handle, MAX_ARTIFACT)?;
        check(
            actual_len == size && actual_sha == sha,
            "settled_backup_bytes",
        )?;
        Ok(SettledBackupLedger {
            _pins: (dir_handle, file_handle),
            settled_path,
        })
    }

    pub fn activity_operation(
        &self,
        operation: Id,
        request: Id,
        principal: &str,
        action: i64,
    ) -> Result<i64> {
        safe(
            self.db.query_row(
                "SELECT a.id FROM vm_operations o JOIN activity a ON a.id=o.activity_id
                 WHERE o.operation_id=?1 AND o.request_key=?2 AND o.principal=?3
                   AND a.actor='agent' AND a.principal=o.principal AND a.action=?4
                   AND o.action=a.action AND a.status=o.status AND a.status<>0",
                params![operation, request, principal, action],
                |r| r.get(0),
            ),
            "operation_activity_correlation",
        )
    }

    pub fn activity_refusal_count(
        &self,
        principal: &str,
        action: i64,
        reason_code: i64,
    ) -> Result<i64> {
        safe(
            self.db.query_row(
                "SELECT count(*) FROM activity
                 WHERE actor='agent' AND principal=?1 AND action=?2 AND status=4 AND reason=?3",
                params![principal, action, reason_code],
                |r| r.get(0),
            ),
            "refusal_activity_correlation",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::private_fixture;
    use super::*;
    use arxd::vm::{Clock, ClockReading, VmController, VmPolicy};

    struct StoreClock;

    impl Clock for StoreClock {
        fn now(&self) -> ClockReading {
            ClockReading {
                utc_ms: 1_800_000_000_000,
                monotonic: std::time::Instant::now(),
            }
        }
    }

    #[test]
    fn static_ledger_preflight_checks_schema_version_and_isolation() {
        let temporary = private_fixture::private_dir();
        let state = temporary.path();
        assert_eq!(Ledger::preflight(state), Ok(()));

        let controller = VmController::open(state, VmPolicy::default(), StoreClock).unwrap();
        let db_path = state.join("activity.sqlite3");
        let db = Connection::open(&db_path).unwrap();
        let version: u32 = db
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(built_vm_schema_version(), Ok(version));
        drop(db);
        drop(controller);

        assert_eq!(Ledger::preflight(state), Ok(()));

        let orphan = state.join("foreign.tmp");
        fs::write(&orphan, b"stray").unwrap();
        assert_eq!(
            Ledger::preflight(state),
            Err(Failure::Check("unattributed_media"))
        );
        fs::remove_file(&orphan).unwrap();

        let sidecar = state.join("activity.sqlite3-wal");
        fs::write(&sidecar, b"wal").unwrap();
        assert_eq!(
            Ledger::preflight(state),
            Err(Failure::Check("unsettled_startup_ledger"))
        );
        fs::remove_file(&sidecar).unwrap();

        for unsupported in [0, version - 1, version + 1] {
            let db = Connection::open(&db_path).unwrap();
            db.pragma_update(None, "user_version", unsupported).unwrap();
            drop(db);
            assert_eq!(
                Ledger::preflight(state),
                Err(Failure::Check("ledger_version"))
            );
        }
    }

    #[test]
    fn static_ledger_verifies_backup_identity_volumes_and_empty_publication() {
        let temporary = private_fixture::private_dir();
        let state = temporary.path().join("state");
        let backup_root = temporary.path().join("backups");
        fs::create_dir(&state).unwrap();
        fs::create_dir_all(backup_root.join("settled")).unwrap();

        let controller = VmController::open(&state, VmPolicy::default(), StoreClock).unwrap();
        drop(controller);

        let vm_id: Id = [0x11; 16];
        let gen_id: Id = [0x22; 16];
        let op_id: Id = [0x33; 16];
        let backup_id: Id = [0x44; 16];

        let settled_dir = backup_root.join("settled");
        let settled_file = settled_dir.join(format!("{}.sqlite3", hex::encode(backup_id)));
        let payload = b"SQLite format 3\0synthetic-settled-bytes";
        fs::write(&settled_file, payload).unwrap();
        let payload_sha: Hash = Sha256::digest(payload).into();
        let dir_ident = identity(
            open(&settled_dir, true, FILE_SHARE_READ | FILE_SHARE_WRITE)
                .unwrap()
                .as_raw_handle(),
            true,
        )
        .unwrap();
        let file_ident = identity(
            open(&settled_file, false, FILE_SHARE_READ)
                .unwrap()
                .as_raw_handle(),
            false,
        )
        .unwrap();

        let db = Connection::open(state.join("activity.sqlite3")).unwrap();
        let host_id: Id = db
            .query_row("SELECT id FROM vm_state_host WHERE singleton=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        db.execute(
            "INSERT INTO vm_static_instances(id,name,principal,host,template,source_generation,recipe_digest,disk_digest,vars_digest,state,persistent,staging)
             VALUES(?1,'calm-life-static','agent-1',?2,?3,?4,?5,?6,?7,2,4096,0)",
            params![vm_id, host_id, [0x55u8; 16], [0x66u8; 16], [0x77u8; 32], [0x88u8; 32], [0x99u8; 32]],
        )
        .unwrap();
        db.execute(
            "INSERT INTO vm_generations(id,static_id,generation,active) VALUES(?1,?2,1,1)",
            params![gen_id, vm_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO vm_compose_projects(vm,project,desired,last_revision) VALUES(?1,'calm-life','started',1)",
            [vm_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO vm_compose_volumes(vm,project,slot,uid,gid,charge) VALUES(?1,'calm-life','data',1000,1000,1)",
            [vm_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO vm_compose_backups(id,operation_id,principal,vm,project,generation,boot,revision,volume,database,state,host_staging_charge,guest_staging_charge,retained_charge,size_bytes,sha256,volume_id,directory_id,file_id,created_ms,settled_ms,namespace_tracked)
             VALUES(?1,?2,'agent-1',?3,'calm-life',?4,1,1,'data','calm.sqlite3','settled',0,0,?5,?5,?6,?7,?8,?9,100,200,1)",
            params![
                backup_id,
                op_id,
                vm_id,
                gen_id,
                payload.len() as i64,
                payload_sha,
                file_ident.volume,
                dir_ident.file,
                file_ident.file,
            ],
        )
        .unwrap();
        drop(db);

        let ledger = Ledger::open(&state).unwrap();
        ledger.publication_empty().unwrap();
        assert_eq!(ledger.compose_volumes_intact(vm_id, "calm-life"), Ok(1));
        let verified = ledger
            .compose_backup(
                vm_id,
                "calm-life",
                backup_id,
                payload.len() as u64,
                &hex::encode(payload_sha),
                &backup_root,
            )
            .unwrap();

        assert_eq!(verified.settled_path, settled_file);

        // Wrong digest fails closed.
        assert_eq!(
            ledger
                .compose_backup(
                    vm_id,
                    "calm-life",
                    backup_id,
                    payload.len() as u64,
                    &"00".repeat(32),
                    &backup_root,
                )
                .err(),
            Some(Failure::Check("settled_backup_pin"))
        );

        // Any row in vm_publication causes publication_empty to fail closed.
        drop(ledger);
        let db = Connection::open(state.join("activity.sqlite3")).unwrap();
        db.execute(
            "INSERT INTO vm_publication(singleton,vm,generation,boot,revision,tunnel,zone,credential_ref,nonce,phase)
             VALUES(1,?1,?2,?3,?4,'00000000-0000-0000-0000-000000000001','00000000000000000000000000000001','dns.dpapi',?5,'published')",
            params![vm_id, gen_id, [0x12u8; 16], [0x34u8; 32], "56".repeat(16)],
        )
        .unwrap();
        drop(db);
        let ledger = Ledger::open(&state).unwrap();
        assert_eq!(
            ledger.publication_empty(),
            Err(Failure::Check("publication_not_empty"))
        );
    }
}

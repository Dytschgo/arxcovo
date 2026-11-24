use super::*;
use arxd::vm::{paths::LocalPath, qemu_img::inspect_header};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
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

const MAX_ARTIFACT: u64 = 200 * 1024 * 1024 * 1024;
type Hash = [u8; 32];

pub(super) fn safe<T, E>(result: std::result::Result<T, E>, label: &'static str) -> Result<T> {
    result.map_err(|_| Failure::Check(label))
}

pub(super) fn id(row: &Value, key: &'static str) -> Result<Id> {
    let hex = opaque(&row[key], 32).ok_or(Failure::Check(key))?;
    let mut id = [0; 16];
    safe(hex::decode_to_slice(hex, &mut id), key)?;
    check(id != [0; 16], key)?;
    Ok(id)
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
struct Identity {
    volume: [u8; 8],
    file: Id,
}

fn identity(handle: windows_sys::Win32::Foundation::HANDLE, directory: bool) -> Result<Identity> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    let mut id = FILE_ID_INFO::default();
    // Each output is a fixed-size structure from a live file handle.
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
                (&mut id as *mut FILE_ID_INFO).cast(),
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
        volume: id.VolumeSerialNumber.to_be_bytes(),
        file: id.FileId.Identifier,
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

pub(super) struct Ledger {
    db: Connection,
    root: PathBuf,
    _pins: Vec<File>,
    database: File,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Leaf {
    generation: Id,
    role: &'static str,
    key: Id,
    volume: [u8; 8],
    directory: Id,
    file: Id,
    digest: Option<Hash>,
    bytes: Option<u64>,
}

pub(super) type Pair = [Leaf; 2];

impl Leaf {
    fn path(&self, root: &Path) -> PathBuf {
        root.join(hex::encode(self.generation)).join(format!(
            "{}-{}",
            self.role,
            hex::encode(self.key)
        ))
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct Claim {
    pub active: i64,
    pub snapshot: i64,
    pub staged: i64,
    pub generations: i64,
}

impl Claim {
    pub fn evidence(&self) -> Value {
        json!({"persistent":self.active,"snapshot":self.snapshot,"staging":self.staged,"generations":self.generations})
    }
}

#[derive(PartialEq, Eq)]
pub(super) struct PairProof {
    pub contents: [(u64, Hash); 2],
    pub allocated: [u64; 2],
}

impl PairProof {
    pub fn evidence(&self) -> Value {
        json!({
            "disk":{"bytes":self.contents[0].0,"sha256":hex::encode(self.contents[0].1),"allocated_bytes":self.allocated[0]},
            "vars":{"bytes":self.contents[1].0,"sha256":hex::encode(self.contents[1].1),"allocated_bytes":self.allocated[1]},
        })
    }
}

// The store constant is private. Read its declaration from the same crate
// source used to build this test, and refuse if it is no longer readable.
fn built_vm_schema_version() -> Result<u32> {
    include_str!("../../src/store/vm.rs")
        .lines()
        .find_map(|line| line.trim().strip_prefix("const VM_SCHEMA_VERSION: u32 ="))
        .and_then(|value| value.trim().strip_suffix(';'))
        .and_then(|value| value.parse().ok())
        .ok_or(Failure::Check("built_ledger_version"))
}

impl Ledger {
    /// The real foreground sweeps at startup. Check a stopped store before
    /// launching it, so a wrong profile cannot sweep someone else's VMs.
    pub fn preflight(state: &Path, recipes: [(&str, &str); 2]) -> Result<()> {
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
                ledger.quiet()?;
                let templates = ledger.goldens()?;
                check(
                    templates.len() <= recipes.len(),
                    "isolated_golden_inventory",
                )?;
                let mut selected = Vec::new();
                for template in templates {
                    let guest = safe(
                        ledger.db.query_row(
                            "SELECT guest FROM vm_templates WHERE id=?1",
                            [template],
                            |row| row.get::<_, i64>(0),
                        ),
                        "golden_guest",
                    )?;
                    let (recipe, pin) = match guest {
                        1 => recipes[0],
                        2 => recipes[1],
                        _ => return Err(Failure::Check("golden_guest")),
                    };
                    let (pair, _) = ledger.golden(template, recipe, pin, guest)?;
                    ledger.inspect(&pair, None, true)?;
                    selected.push(pair);
                }
                ledger.inventory(&selected.iter().collect::<Vec<_>>())
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
        // SQLite retains ownership of this borrowed VFS handle.
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
            database,
        })
    }

    pub fn quiet(&self) -> Result<()> {
        check(
            safe(
                self.db.query_row(
                    "SELECT (SELECT count(*) FROM vm_instances)
                  + (SELECT count(*) FROM vm_static_instances)
                  + (SELECT count(*) FROM vm_restore_points)
                  + (SELECT count(*) FROM vm_snapshot_work)
                  + (SELECT count(*) FROM vm_build_intents WHERE state<>'sealed')
                  + (SELECT count(*) FROM vm_operations WHERE status=0)
                  + (SELECT count(*) FROM vm_disk_claims)",
                    [],
                    |r| r.get::<_, i64>(0),
                ),
                "ledger_quiet",
            )? == 0,
            "ledger_not_isolated",
        )
    }

    pub fn goldens(&self) -> Result<Vec<Id>> {
        let mut statement = safe(
            self.db
                .prepare("SELECT id FROM vm_templates ORDER BY id LIMIT 3"),
            "ledger_goldens",
        )?;
        let ids = safe(statement.query_map([], |r| r.get(0)), "ledger_goldens")?;
        safe(ids.collect(), "ledger_goldens")
    }

    pub fn golden(&self, template: Id, recipe: &str, pin: &str, guest: i64) -> Result<(Pair, i64)> {
        let (generation, charge) = safe(
            self.db.query_row(
                "SELECT g.id,t.bytes FROM vm_templates t
             JOIN vm_build_intents b ON b.template_id=t.id
             JOIN vm_generations g ON g.build_id=b.id AND g.active=1
             JOIN vm_artifacts d ON d.generation_id=g.id AND d.role='disk' AND d.state='sealed'
             JOIN vm_artifacts v ON v.generation_id=g.id AND v.role='vars' AND v.state='sealed'
             WHERE t.id=?1 AND b.state='sealed' AND b.health_verified=1 AND b.recipe_id=?2
               AND b.recipe_digest=t.recipe_digest AND lower(hex(t.recipe_digest))=?3 AND t.guest=?4
               AND d.content_digest=t.disk_digest AND v.content_digest=t.vars_digest",
                params![template, recipe, pin, guest],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ),
            "sealed_recipe_pins",
        )?;
        Ok((self.pair(generation, "sealed")?, charge))
    }

    pub fn vm_pair(&self, vm: Id) -> Result<Pair> {
        let generation = safe(
            self.db.query_row(
                "SELECT id FROM vm_generations WHERE vm_id=?1 AND active=1",
                [vm],
                |r| r.get(0),
            ),
            "active_generation",
        )?;
        self.pair(generation, "active")
    }

    pub fn point(&self, vm: Id, snapshot: Id, template: Id, provenance: Id) -> Result<(Pair, i64)> {
        let (generation, charge) = safe(
            self.db.query_row(
                "SELECT p.generation,p.charge FROM vm_restore_points p
             JOIN vm_instances i ON i.id=p.vm JOIN vm_templates t ON t.id=i.template
             WHERE p.id=?1 AND p.vm=?2 AND p.state='ready' AND t.id=?3 AND p.provenance=?4",
                params![snapshot, vm, template, provenance],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ),
            "snapshot_ledger",
        )?;
        Ok((self.pair(generation, "sealed")?, charge))
    }

    fn pair(&self, generation: Id, state: &str) -> Result<Pair> {
        let leaf = |role| {
            safe(
                self.db.query_row(
                    "SELECT object_key,volume_id,directory_id,file_id,content_digest,bytes
             FROM vm_artifacts WHERE generation_id=?1 AND role=?2 AND state=?3",
                    params![generation, role, state],
                    |r| {
                        Ok(Leaf {
                            generation,
                            role,
                            key: r.get(0)?,
                            volume: r.get(1)?,
                            directory: r.get(2)?,
                            file: r.get(3)?,
                            digest: r.get(4)?,
                            bytes: r.get(5)?,
                        })
                    },
                ),
                "ledger_pair",
            )
        };
        let pair = [leaf("disk")?, leaf("vars")?];
        check(
            pair[0].key != pair[1].key && pair[0].file != pair[1].file,
            "private_pair",
        )?;
        Ok(pair)
    }

    pub fn generation(pair: &Pair) -> Id {
        pair[0].generation
    }

    pub fn inspect(&self, pair: &Pair, backing: Option<&Pair>, sealed: bool) -> Result<PairProof> {
        let backup = backing.map(|pair| pair[0].path(&self.root));
        let backup = backup
            .as_ref()
            .map(|path| {
                let text = path.to_str().ok_or(Failure::Check("backing_path"))?;
                safe(LocalPath::parse(text), "backing_path")
            })
            .transpose()?;
        let dir = open(
            &self.root.join(hex::encode(pair[0].generation)),
            true,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )?;
        let dir_id = identity(dir.as_raw_handle(), true)?;
        let mut proof = PairProof {
            contents: [(0, [0; 32]); 2],
            allocated: [0; 2],
        };
        for (index, leaf) in pair.iter().enumerate() {
            let mut file = open(&leaf.path(&self.root), false, FILE_SHARE_READ)?;
            let id = identity(file.as_raw_handle(), false)?;
            check(
                id.file == leaf.file
                    && id.volume == leaf.volume
                    && dir_id.volume == id.volume
                    && dir_id.file == leaf.directory,
                "artifact_identity",
            )?;
            // NTFS allocated length is independent of the qcow2 virtual size.
            let mut standard = FILE_STANDARD_INFO::default();
            check(
                unsafe {
                    GetFileInformationByHandleEx(
                        file.as_raw_handle(),
                        FileStandardInfo,
                        (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                        size_of::<FILE_STANDARD_INFO>() as u32,
                    )
                } != 0,
                "allocated_length",
            )?;
            proof.allocated[index] =
                safe(u64::try_from(standard.AllocationSize), "allocated_length")?;
            let contents = digest(&file, MAX_ARTIFACT)?;
            if sealed {
                check(
                    leaf.bytes == Some(contents.0) && leaf.digest == Some(contents.1),
                    "sealed_content_pin",
                )?;
            }
            if index == 0 {
                safe(std::io::Seek::rewind(&mut file), "disk_rewind")?;
                let mut cluster = vec![0; 65_536];
                safe(file.read_exact(&mut cluster), "qcow_header")?;
                safe(inspect_header(&cluster, backup.as_ref()), "qcow_lineage")?;
            }
            proof.contents[index] = contents;
        }
        Ok(proof)
    }

    pub fn distinct(&self, first: &Pair, second: &Pair) -> Result<()> {
        check(
            first[0].generation != second[0].generation
                && first.iter().all(|a| {
                    second
                        .iter()
                        .all(|b| a.key != b.key && (a.volume != b.volume || a.file != b.file))
                }),
            "aliased_pairs",
        )
    }

    pub fn gone(&self, pair: &Pair) -> Result<bool> {
        let retained = safe(
            self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM vm_generations WHERE id=?1)",
                [pair[0].generation],
                |r| r.get::<_, bool>(0),
            ),
            "retirement_ledger",
        )?;
        if retained {
            return Ok(false);
        }
        let dir = self.root.join(hex::encode(pair[0].generation));
        let dir = match dir.symlink_metadata() {
            Ok(_) => open(&dir, true, FILE_SHARE_READ | FILE_SHARE_WRITE)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(_) => return Err(Failure::Check("retired_directory")),
        };
        let dir = identity(dir.as_raw_handle(), true)?;
        check(
            dir.file == pair[0].directory && dir.volume == pair[0].volume,
            "retired_directory_identity",
        )?;
        for leaf in pair {
            match leaf.path(&self.root).symlink_metadata() {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                _ => return Err(Failure::Check("retired_media_still_present")),
            }
        }
        Ok(true)
    }

    pub fn release(&self, pair: &Pair, start: Id, clean: bool) -> Result<()> {
        let (epoch, disposition) = safe(
            self.db.query_row(
                "SELECT epoch,disposition FROM vm_snapshot_release WHERE generation=?1",
                [Self::generation(pair)],
                |r| Ok((r.get::<_, Id>(0)?, r.get::<_, Option<String>>(1)?)),
            ),
            "boot_release_epoch",
        )?;
        check(
            epoch == start && disposition.as_deref() == clean.then_some("clean"),
            "boot_release_epoch",
        )
    }

    pub fn charge(&self, vm: Id) -> Result<Claim> {
        safe(self.db.query_row(
            "SELECT (SELECT coalesce(sum(persistent),0) FROM vm_disk_claims WHERE vm=?1),
                    (SELECT coalesce(sum(charge),0) FROM vm_restore_points WHERE vm=?1),
                    (SELECT coalesce(sum(staging),0) FROM vm_disk_claims WHERE vm=?1)
                      + (SELECT coalesce(sum(staging+persistent),0) FROM vm_snapshot_work WHERE vm=?1),
                    (SELECT count(*) FROM vm_generations WHERE vm_id=?1)",
            [vm], |r| Ok(Claim { active:r.get(0)?,snapshot:r.get(1)?,staged:r.get(2)?,generations:r.get(3)? }),
        ), "ledger_charge")
    }

    pub fn effect(&self, vm: Id) -> Result<Value> {
        let (vms, operations, media) = safe(
            self.db.query_row(
                "SELECT (SELECT count(*) FROM vm_instances),
                    (SELECT count(*) FROM vm_operations),
                    (SELECT count(*) FROM vm_artifacts)",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            ),
            "clone_retry_ledger",
        )?;
        Ok(json!({"vms":vms,"operations":operations,"media":media,
            "generation":hex::encode(Self::generation(&self.vm_pair(vm)?)),
            "charges":self.charge(vm)?.evidence()}))
    }

    pub fn metadata_bytes(&self) -> Result<u64> {
        Ok(safe(self.database.metadata(), "ledger_metadata")?.len())
    }

    pub fn aggregate(&self) -> Result<i64> {
        safe(self.db.query_row(
            "SELECT coalesce(sum(bytes),0) FROM (
                SELECT bytes FROM vm_templates UNION ALL SELECT persistent+staging FROM vm_disk_claims
                UNION ALL SELECT staging_bytes FROM vm_build_intents
                UNION ALL SELECT persistent+staging FROM vm_static_instances
                UNION ALL SELECT charge FROM vm_restore_points
                UNION ALL SELECT persistent+staging FROM vm_snapshot_work)",
            [], |r| r.get(0),
        ), "aggregate_charge")
    }

    pub fn activity(
        &self,
        operation: Id,
        request: Id,
        principal: &str,
        action: i64,
    ) -> Result<i64> {
        safe(self.db.query_row(
            "SELECT a.id FROM vm_operations o JOIN activity a ON a.id=o.activity_id
             WHERE o.operation_id=?1 AND o.request_key=?2 AND o.principal=?3
               AND a.actor='agent' AND a.principal=o.principal AND a.action=?4 AND o.action=a.action
               AND a.status=o.status AND a.status<>0 AND a.cleanup_confirmed IS o.cleanup_confirmed",
            params![operation, request, principal, action], |r| r.get(0),
        ), "operation_activity_correlation")
    }

    pub fn expiry(
        &self,
        vm: Id,
        principal: &str,
        activity: i64,
        deadline: i64,
        expired: i64,
    ) -> Result<()> {
        let found = safe(
            self.db
                .query_row(
                    "SELECT a.id FROM vm_tombstones t JOIN activity a ON a.id=t.activity_id
             WHERE t.id=?1 AND t.principal=?2 AND t.activity_id=?3 AND t.expired_at_ms=?4
               AND a.actor='daemon' AND a.principal='arxd' AND a.vm_owner=t.principal
               AND a.vm=t.id AND a.action=7 AND a.status=1 AND a.cleanup_confirmed=1
               AND a.started_ms>=?5 AND a.finished_ms>=a.started_ms",
                    params![vm, principal, activity, expired, deadline],
                    |r| r.get::<_, i64>(0),
                )
                .optional(),
            "daemon_expiry",
        )?;
        check(found == Some(activity), "daemon_expiry")
    }

    pub fn reconciled(
        &self,
        vm: Id,
        generation: Id,
        cause: &str,
        restore: Option<Id>,
        expiry: Option<i64>,
    ) -> Result<i64> {
        safe(
            self.db.query_row(
                "SELECT a.id FROM vm_snapshot_reconciliation r
             JOIN activity a ON a.id=r.activity_id
             LEFT JOIN vm_operations o ON o.activity_id=r.source_activity
             WHERE r.vm=?1 AND r.generation=?2 AND r.cause=?3
               AND o.operation_id IS ?4 AND r.expiry_activity IS ?5
               AND a.vm=r.vm AND a.action=19 AND a.actor='daemon' AND a.principal='arxd'
               AND a.status=1 AND a.cleanup_confirmed=1",
                params![vm, generation, cause, restore, expiry],
                |r| r.get(0),
            ),
            "snapshot_reconciliation",
        )
    }
    /// Confirm that every remaining media leaf belongs to a selected golden.
    /// Empty generation directories can remain after disposal.
    pub fn inventory(&self, pairs: &[&Pair]) -> Result<()> {
        let expected: Vec<_> = pairs
            .iter()
            .flat_map(|pair| pair.iter().map(|leaf| leaf.path(&self.root)))
            .collect();
        let count = safe(
            self.db
                .query_row("SELECT count(*) FROM vm_artifacts", [], |r| {
                    r.get::<_, usize>(0)
                }),
            "artifact_inventory",
        )?;
        check(count == expected.len(), "artifact_inventory")?;
        let mut seen = 0;
        for (number, entry) in safe(read_dir(&self.root), "media_inventory")?.enumerate() {
            check(number < 256, "media_inventory_bound")?;
            let dir = safe(entry, "media_inventory")?.path();
            let name = dir.file_name().and_then(|name| name.to_str());
            // The TEST foreground opens the vault on this same root. Its
            // DELETE-mode rollback journal is supported; WAL/SHM are refused.
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
                open(&dir, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
                continue;
            }
            let name = json!(name);
            check(opaque(&name, 32).is_some(), "unattributed_media")?;
            let _dir = open(&dir, true, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
            for entry in safe(read_dir(&dir), "media_inventory")? {
                let path = safe(entry, "media_inventory")?.path();
                check(expected.contains(&path), "unattributed_media")?;
                seen += 1;
            }
        }
        check(seen == expected.len(), "physical_inventory")
    }
}

#[test]
fn startup_preflight_refuses_occupied_or_foreign_inventory() {
    let temporary = tempfile::tempdir().unwrap();
    let state = temporary.path();
    let recipes = [("ubuntu", "unused"), ("windows", "unused")];
    assert_eq!(Ledger::preflight(state, recipes), Ok(()));

    let orphan = state.join("untracked");
    std::fs::write(&orphan, b"fixture").unwrap();
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("fresh_state"))
    );

    let database = Connection::open(state.join("activity.sqlite3")).unwrap();
    database
        .execute_batch(
            "CREATE TABLE vm_instances (id INTEGER);
         CREATE TABLE vm_static_instances (id INTEGER);
         CREATE TABLE vm_restore_points (id INTEGER);
         CREATE TABLE vm_snapshot_work (id INTEGER);
         CREATE TABLE vm_build_intents (state TEXT);
         CREATE TABLE vm_operations (status INTEGER);
         CREATE TABLE vm_disk_claims (id INTEGER);
         CREATE TABLE vm_templates (id BLOB);
         CREATE TABLE vm_artifacts (id BLOB);",
        )
        .unwrap();
    database
        .pragma_update(None, "user_version", built_vm_schema_version().unwrap())
        .unwrap();
    drop(database);

    let sidecar = state.join("activity.sqlite3-journal");
    std::fs::write(&sidecar, b"fixture").unwrap();
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("unsettled_startup_ledger"))
    );
    std::fs::remove_file(sidecar).unwrap();

    // The same foreign file must also fail with an existing, idle ledger.
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&orphan).unwrap();
    let dir = state.join("aa".repeat(16));
    std::fs::create_dir(&dir).unwrap();
    let orphan = dir.join("untracked");
    std::fs::write(&orphan, b"fixture").unwrap();
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("unattributed_media"))
    );
    std::fs::remove_file(&orphan).unwrap();
    assert_eq!(Ledger::preflight(state, recipes), Ok(()));

    let database = Connection::open(state.join("activity.sqlite3")).unwrap();
    database
        .execute("INSERT INTO vm_instances VALUES (1)", [])
        .unwrap();
    drop(database);
    assert_eq!(
        Ledger::preflight(state, recipes),
        Err(Failure::Check("ledger_not_isolated"))
    );
}

#[test]
fn ledger_accepts_the_store_version_written_by_the_built_crate() {
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

    let temporary = private_fixture::private_dir();
    let state = temporary.path();
    let recipes = [("ubuntu", "unused"), ("windows", "unused")];
    let controller = VmController::open(state, VmPolicy::default(), StoreClock).unwrap();

    // Check the reader against a real freshly migrated store. Read the
    // version from that store rather than stamping it with the parser.
    let path = state.join("activity.sqlite3");
    let database = Connection::open(&path).unwrap();
    let version: u32 = database
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(built_vm_schema_version(), Ok(version));
    drop(database);
    assert!(Ledger::open(state).is_ok());
    drop(controller);
    assert_eq!(Ledger::preflight(state, recipes), Ok(()));

    for name in ["vault.sqlite3", "vault.lock", "vault.sqlite3-journal"] {
        std::fs::write(state.join(name), b"vault fixture").unwrap();
    }
    assert_eq!(Ledger::preflight(state, recipes), Ok(()));
    for name in [
        "vault.sqlite3-backup",
        "vault.sqlite3-wal",
        "vault.sqlite3-shm",
    ] {
        let unknown = state.join(name);
        std::fs::write(&unknown, b"foreign fixture").unwrap();
        assert_eq!(
            Ledger::preflight(state, recipes),
            Err(Failure::Check("unattributed_media"))
        );
        std::fs::remove_file(unknown).unwrap();
    }
    let vault_lock = state.join("vault.lock");
    std::fs::remove_file(&vault_lock).unwrap();
    std::fs::create_dir(&vault_lock).unwrap();
    assert!(Ledger::preflight(state, recipes).is_err());
    std::fs::remove_dir(vault_lock).unwrap();

    for unsupported in [0, version - 1, version + 1] {
        let database = Connection::open(&path).unwrap();
        database
            .pragma_update(None, "user_version", unsupported)
            .unwrap();
        drop(database);
        let original = std::fs::read(&path).unwrap();
        assert!(matches!(
            Ledger::open(state),
            Err(Failure::Check("ledger_version"))
        ));
        assert_eq!(
            Ledger::preflight(state, recipes),
            Err(Failure::Check("ledger_version"))
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

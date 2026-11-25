//! Local console maintenance. Only this module can construct a completion proof.
use super::*;
use crate::store::vm::VmStore;
use anyhow::{Context, bail};
use std::ffi::OsString;
use std::io::{BufRead, IsTerminal};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct OwnerBackup {
    pub id: Id,
    pub operation_id: Id,
    pub state: BackupRecordState,
    pub directory_id: Option<Id>,
    pub witness: Option<HostFileWitness>,
    pub namespace_tracked: bool,
    pub sha256: Option<[u8; 32]>,
    pub owner: Option<String>,
    pub started_ms: Option<i64>,
    pub finished_ms: Option<i64>,
}

pub(crate) struct OwnerRetirementProof(Id);

impl OwnerRetirementProof {
    pub(crate) fn backup_id(&self) -> Id {
        self.0
    }
}

enum Action {
    List,
    Retire { root: PathBuf, id: Id },
}

fn parse(args: &[OsString]) -> Option<(PathBuf, Action)> {
    let [flag, state, rest @ ..] = args else {
        return None;
    };
    if flag != "--state-dir" {
        return None;
    }
    let action = match rest {
        [list] if list == "--list" => Action::List,
        [root_flag, root, id_flag, id]
            if root_flag == "--backup-root" && id_flag == "--backup-id" =>
        {
            let mut parsed = [0; 16];
            hex::decode_to_slice(id.to_str()?, &mut parsed).ok()?;
            Action::Retire {
                root: PathBuf::from(root),
                id: parsed,
            }
        }
        _ => return None,
    };
    Some((PathBuf::from(state), action))
}

/// Entry point for the standalone owner CLI.
pub fn backup_retire_cli(args: &[OsString]) -> i32 {
    let Some((state, action)) = parse(args) else {
        eprintln!("usage: arxd backup-retire --state-dir DIR --list");
        eprintln!("       arxd backup-retire --state-dir DIR --backup-root DIR --backup-id ID");
        return 2;
    };
    if let Err(error) = run(&state, action) {
        eprintln!("arxd: backup retirement refused: {error}");
        return 1;
    }
    0
}

fn require_authority(elevated: bool, check_owner: impl FnOnce() -> Result<()>) -> Result<()> {
    ensure!(!elevated, "use the non-elevated store-owner account");
    check_owner().context("store owner required")
}

fn run(state: &Path, action: Action) -> Result<()> {
    let owner = crate::local::vm_supervisor_identity().context("cannot identify local account")?;
    require_authority(elevated(&owner).context("cannot verify elevation")?, || {
        crate::config::checked_path(state)?;
        crate::local::require_owner(state)
    })?;
    let pin = crate::config::RootPin::acquire(state).context("cannot pin VM state directory")?;
    let db = VmStore::open_for_backup_owner(state, matches!(action, Action::Retire { .. }))
        .context("cannot lock and open the current VM store; stop the controller first")?;
    pin.verify(state).context("VM state directory changed")?;
    match action {
        Action::List => {
            let rows = db
                .read_backup_owner(|db| store::owner_backups(db, None))
                .context("cannot read backup ledger")?;
            for row in rows {
                let status = match (row.state, row.started_ms, row.finished_ms, row.directory_id) {
                    (_, _, Some(_), _) => "retired",
                    (_, Some(_), None, _) => "retirement-pending",
                    (BackupRecordState::Settled, _, _, _) => "settled",
                    (_, _, _, None) if !row.namespace_tracked => "partial-unbound-legacy",
                    _ => "partial-uncertain",
                };
                let digest = row.sha256.map_or_else(|| "-".to_owned(), hex::encode);
                println!("{} {status} {digest}", hex::encode(row.id));
            }
        }
        Action::Retire { root, id } => {
            let input = std::io::stdin();
            let is_terminal = input.is_terminal();
            let custody = ManagedBackupDir::open(&root, state, None, false)
                .context("cannot open the existing private backup namespace")?;
            retire(
                &db,
                &custody,
                id,
                &owner,
                || {
                    eprintln!("Type backup ID {} to retire it:", hex::encode(id));
                    confirm(is_terminal, input.lock(), id)
                },
                || {},
            )?;
            println!("{} retired", hex::encode(id));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn elevated(_owner: &str) -> Result<bool> {
    Ok(unsafe {
        libc::getuid() == 0
            || libc::getuid() != libc::geteuid()
            || libc::getgid() != libc::getegid()
    })
}

#[cfg(windows)]
fn elevated(owner: &str) -> Result<bool> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{Security::*, System::Threading::*};
    if ["S-1-5-18", "S-1-5-19", "S-1-5-20"].contains(&owner) {
        return Ok(true);
    }
    let mut raw = std::ptr::null_mut();
    ensure!(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } != 0,
        "cannot open current token"
    );
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut size = 0;
    ensure!(
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        } != 0
            && size as usize == size_of::<TOKEN_ELEVATION>(),
        "cannot read token elevation"
    );
    Ok(elevation.TokenIsElevated != 0)
}

fn confirm(terminal: bool, input: impl BufRead, id: Id) -> Result<()> {
    ensure!(terminal, "interactive console input required");
    // One exact 32-character ID followed by LF or CRLF.
    let mut line = Vec::new();
    input
        .take(34)
        .read_until(b'\n', &mut line)
        .context("cannot read confirmation")?;
    let line = line
        .strip_suffix(b"\n")
        .context("missing backup ID confirmation")?;
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    ensure!(
        line == hex::encode(id).as_bytes(),
        "backup ID confirmation mismatch"
    );
    Ok(())
}

fn now_ms() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}

fn retire(
    db: &VmStore,
    custody: &ManagedBackupDir,
    id: Id,
    owner: &str,
    confirmation: impl FnOnce() -> Result<()>,
    after_verify: impl FnOnce(),
) -> Result<()> {
    let row = db
        .read_backup_owner(|db| store::owner_backups(db, Some(id)))
        .context("cannot read backup ledger")?
        .pop()
        .context("backup unavailable for retirement")?;
    if let Some(previous) = &row.owner {
        ensure!(previous == owner, "retirement belongs to another owner");
    }
    ensure!(row.finished_ms.is_none(), "backup already retired");
    custody
        .pin
        .verify(&custody.root)
        .context("backup root changed")?;
    match row.directory_id {
        Some(expected) => ensure!(
            expected == custody.namespace.id,
            "backup namespace identity mismatch"
        ),
        None => ensure!(
            row.namespace_tracked,
            "legacy partial has no recorded namespace identity"
        ),
    }
    let stage_dir = &custody.namespace.staging;
    let settled_dir = &custody.namespace.settled;
    let staging = pinned_leaf(stage_dir, &custody.staging_path(row.operation_id)?)?;
    let settled = pinned_leaf(settled_dir, &custody.settled_path(id)?)?;
    let settled_backup = row.state == BackupRecordState::Settled;
    if settled_backup {
        let witness = row.witness.context("settled backup has no file witness")?;
        if row.started_ms.is_none() {
            let file =
                open_leaf(settled_dir, &settled).context("settled backup leaf unavailable")?;
            ensure!(
                file.metadata()?.is_file()
                    && !is_reparse(&file.metadata()?)
                    && file_identity(&file)? == witness.identity,
                "settled backup leaf identity mismatch"
            );
        }
    }
    confirmation()?;

    // Commit the attribution before touching the filesystem. A retry can
    // confirm absence after a process crash without releasing charge early.
    if row.started_ms.is_none() {
        let now = now_ms()?;
        db.transact(|tx| store::prepare_owner_retirement(tx, id, owner, now))
            .context("cannot prepare retirement in VM ledger")?;
    }
    if settled_backup {
        let witness = row.witness.context("settled backup has no file witness")?;
        ensure!(
            remove_attributed_file(stage_dir, &staging, witness)?
                && remove_verified(settled_dir, &settled, witness, after_verify)?,
            "backup custody removal could not be confirmed"
        );
    }
    ensure!(
        absent(stage_dir, &staging)? && absent(settled_dir, &settled)?,
        "backup custody still contains a leaf; charge retained"
    );
    custody
        .pin
        .verify(&custody.root)
        .context("backup root changed")?;
    let now = now_ms()?;
    db.transact(|tx| store::complete_owner_retirement(tx, OwnerRetirementProof(id), now))
        .context("cannot complete retirement in VM ledger")
}

#[cfg(windows)]
fn pinned_leaf(dir: &File, path: &Path) -> Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_GUID};
    let mut buffer = vec![0; 32768];
    let n = unsafe {
        GetFinalPathNameByHandleW(
            dir.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    ensure!(
        n != 0 && n < buffer.len(),
        "backup custody requires a local volume path"
    );
    let mut pinned = PathBuf::from(OsString::from_wide(&buffer[..n]));
    pinned.push(path.file_name().context("missing backup leaf name")?);
    Ok(pinned)
}

#[cfg(unix)]
fn pinned_leaf(_dir: &File, path: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(
        path.file_name().context("missing backup leaf name")?,
    ))
}

#[cfg(windows)]
fn open_leaf(_dir: &File, path: &Path) -> Result<File> {
    open_readonly_no_reparse(path)
}

#[cfg(unix)]
fn open_leaf(dir: &File, path: &Path) -> Result<File> {
    let name = leaf_name(path)?;
    Ok(open_at(dir, &name, libc::O_RDONLY | libc::O_NONBLOCK)?)
}

#[cfg(unix)]
fn open_at(dir: &File, name: &std::ffi::CStr, flags: i32) -> std::io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(windows)]
fn absent(_dir: &File, path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => bail!("cannot verify backup leaf absence"),
    }
}

#[cfg(unix)]
fn absent(dir: &File, path: &Path) -> Result<bool> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;

    fn missing(dir: &File, name: &std::ffi::CStr) -> Result<bool> {
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0
        {
            return Ok(false);
        }
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            return Ok(true);
        }
        bail!("cannot verify backup leaf absence")
    }

    let name = leaf_name(path)?;
    if !missing(dir, &name)? {
        return Ok(false);
    }
    let slot = CString::new(format!(".cleanup-{}", name.to_str()?))?;
    match open_at(dir, &slot, libc::O_RDONLY | libc::O_DIRECTORY) {
        Ok(held) => missing(&held, c"leaf"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => bail!("cannot inspect backup cleanup slot"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mcp::vm::tests::backup_fixture_root,
        store::vm::{backup_owner_fixture, disk_bytes},
    };
    use rusqlite::params;

    const NOW: i64 = 1_800_000_000_000;

    struct Fixture {
        db: VmStore,
        custody: ManagedBackupDir,
        state: tempfile::TempDir,
        _root: tempfile::TempDir,
        target: Target,
        generation: Id,
    }

    impl Fixture {
        fn new() -> Result<Self> {
            let (state, db, target, generation) = backup_owner_fixture()?;
            let root = backup_fixture_root();
            let custody = ManagedBackupDir::open(root.path(), state.path(), None, false)?;
            Ok(Self {
                db,
                custody,
                state,
                _root: root,
                target,
                generation,
            })
        }

        fn admit(&self, request: u8) -> Result<Result<BackupIntent, BackupFault>> {
            self.db.transact(|tx| {
                store::admit_backup(
                    tx,
                    "alice",
                    &self.target,
                    crate::vm::RequestId {
                        key: [request; 16],
                        issued_at_ms: NOW,
                    },
                    self.generation,
                    1,
                    NOW,
                    |_, _| Ok(Ok(())),
                )
            })
        }

        fn settle(&self, request: u8, bytes: &[u8]) -> Result<BackupIntent> {
            let intent = self.admit(request)?.unwrap();
            let mut writer = self.custody.begin_staging(intent.id)?;
            let witness = writer.witness();
            writer.write_chunk(bytes)?;
            drop(writer);
            // The verification of SQLite bytes is covered by E4. Reproduce
            // its hard-linked, identity-bound layout, including staging residue.
            fs::hard_link(
                self.custody.staging_path(intent.id)?,
                self.custody.settled_path(intent.backup_id)?,
            )?;
            self.db.transact(|tx| {
                store::settle_backup_succeeded(
                    tx,
                    &intent,
                    bytes.len() as u64,
                    Sha256::digest(bytes).into(),
                    witness.identity.volume,
                    witness.directory_id,
                    witness.identity.file,
                    NOW,
                )
            })?;
            Ok(intent)
        }

        fn charge(&self) -> Result<u64> {
            self.db
                .read_backup_owner(|db| disk_bytes(db)?.context("fixture charge unavailable"))
        }
    }

    fn typed(id: Id) -> Result<()> {
        confirm(true, format!("{}\n", hex::encode(id)).as_bytes(), id)
    }

    #[test]
    fn owner_retirement_authority_refuses_elevation_and_non_owner() {
        assert!(require_authority(true, || Ok(())).is_err());
        assert!(require_authority(false, || bail!("owner mismatch")).is_err());
        assert!(require_authority(false, || Ok(())).is_ok());
    }

    #[test]
    fn owner_retirement_requires_exact_interactive_confirmation_before_any_write() -> Result<()> {
        let f = Fixture::new()?;
        let backup = f.settle(1, b"settled")?;
        let id = backup.backup_id;
        let typed_id = format!("{}\n", hex::encode(id));
        for (terminal, input) in [
            (false, typed_id.as_str()),
            (true, ""),
            (true, "wrong\n"),
            (true, typed_id.trim_end()),
        ] {
            assert!(
                retire(
                    &f.db,
                    &f.custody,
                    id,
                    "owner",
                    || confirm(terminal, input.as_bytes(), id),
                    || {}
                )
                .is_err()
            );
            assert!(f.custody.settled_path(id)?.is_file());
            assert!(f.custody.staging_path(backup.id)?.is_file());
            let row =
                f.db.read_backup_owner(|db| store::owner_backups(db, Some(id)))?
                    .pop()
                    .unwrap();
            assert_eq!(row.started_ms, None);
        }
        assert!(confirm(true, format!("{}\r\n", hex::encode(id)).as_bytes(), id).is_ok());
        Ok(())
    }

    #[test]
    fn owner_retirement_store_lock_refuses_a_running_controller_and_list_is_read_only() -> Result<()>
    {
        let f = Fixture::new()?;
        let path = f.state.path().to_owned();
        assert!(VmStore::open_for_backup_owner(&path, true).is_err());
        assert!(VmStore::open_for_backup_owner(&path, false).is_err());
        drop(f);
        let (state, controller, _, _) = backup_owner_fixture()?;
        drop(controller);
        let catalog = VmStore::open_for_backup_owner(state.path(), false)?;
        catalog.read_backup_owner(|db| {
            assert!(store::owner_backups(db, None)?.is_empty());
            assert!(
                db.execute_batch("CREATE TABLE forbidden (x INTEGER)")
                    .is_err()
            );
            Ok(())
        })
    }

    #[test]
    fn owner_retirement_audits_and_admits_a_third_backup() -> Result<()> {
        let f = Fixture::new()?;
        let bytes = b"first backup";
        let first = f.settle(1, bytes)?;
        let second = f.settle(2, b"second backup")?;
        assert!(f.admit(3)?.is_err());
        let charge = f.charge()?;
        let id = first.backup_id;
        let owner = "fixture-owner";
        let before = now_ms()?;
        retire(&f.db, &f.custody, id, owner, || typed(id), || {})?;
        assert!(!f.custody.settled_path(id)?.exists());
        assert!(!f.custody.staging_path(first.id)?.exists());
        assert!(f.custody.settled_path(second.backup_id)?.is_file());
        assert_eq!(f.charge()?, charge - bytes.len() as u64);
        f.db.read_backup_owner(|db| {
            let row = store::owner_backups(db, Some(id))?.pop().unwrap();
            assert_eq!(row.id, id);
            assert_eq!(row.owner.as_deref(), Some(owner));
            assert!(row.started_ms.is_some_and(|t| t >= before));
            assert!(row.finished_ms >= row.started_ms);
            assert!(row.finished_ms.is_some_and(|t| t <= now_ms().unwrap()));
            let audited_digest: [u8; 32] = db.query_row(
                "SELECT sha256 FROM vm_compose_backup_retirements WHERE backup_id=?1",
                [id],
                |record| record.get(0),
            )?;
            assert_eq!(audited_digest, <[u8; 32]>::from(Sha256::digest(bytes)));
            assert_eq!(row.state, BackupRecordState::Settled);
            assert_eq!(store::settled_backup_count(db, &f.target)?, 1);
            assert!(
                db.execute(
                    "DELETE FROM vm_compose_backup_retirements WHERE backup_id=?1",
                    [id]
                )
                .is_err()
            );
            assert!(
                db.execute(
                    "UPDATE vm_compose_backup_retirements SET owner='other' WHERE backup_id=?1",
                    [id]
                )
                .is_err()
            );
            assert!(
                db.execute("DELETE FROM vm_compose_backups WHERE id=?1", [id])
                    .is_err()
            );
            Ok(())
        })?;
        assert!(f.admit(3)?.is_ok());
        Ok(())
    }

    #[test]
    fn owner_retirement_preserves_a_replaced_leaf_and_pending_charge() -> Result<()> {
        let f = Fixture::new()?;
        let id = f.settle(1, b"retire me")?.backup_id;
        let leaf = f.custody.settled_path(id)?;
        let saved = leaf.with_extension("saved");
        let charge = f.charge()?;
        fs::rename(&leaf, &saved)?;
        fs::write(&leaf, b"already replaced")?;
        assert!(retire(&f.db, &f.custody, id, "owner", || typed(id), || {}).is_err());
        assert_eq!(fs::read(&leaf)?, b"already replaced");
        assert_eq!(f.charge()?, charge);
        fs::remove_file(&leaf)?;
        fs::rename(&saved, &leaf)?;

        let result = retire(
            &f.db,
            &f.custody,
            id,
            "owner",
            || typed(id),
            || {
                let rename = fs::rename(&leaf, &saved);
                #[cfg(windows)]
                assert!(
                    rename.is_err(),
                    "held DELETE handle must exclude replacement"
                );
                #[cfg(unix)]
                {
                    rename.unwrap();
                    fs::write(&leaf, b"replacement").unwrap();
                }
            },
        );
        #[cfg(windows)]
        {
            result?;
            assert!(!leaf.exists());
            assert!(f.charge()? < charge);
        }
        #[cfg(unix)]
        {
            assert!(result.is_err());
            assert_eq!(fs::read(&leaf)?, b"replacement");
            assert_eq!(f.charge()?, charge);
        }
        Ok(())
    }

    #[test]
    fn owner_retirement_resumes_after_removal_and_checks_the_recorded_owner() -> Result<()> {
        let f = Fixture::new()?;
        let backup = f.settle(1, b"settled")?;
        let id = backup.backup_id;
        let charge = f.charge()?;
        f.db.transact(|db| store::prepare_owner_retirement(db, id, "owner", NOW))?;
        let (staging, settled) = (
            f.custody.staging_path(backup.id)?,
            f.custody.settled_path(id)?,
        );
        let row =
            f.db.read_backup_owner(|db| store::owner_backups(db, Some(id)))?
                .pop()
                .unwrap();
        assert!(remove_attributed_file(
            &f.custody.namespace.staging,
            &staging,
            row.witness.unwrap()
        )?);
        assert!(remove_attributed_file(
            &f.custody.namespace.settled,
            &settled,
            row.witness.unwrap()
        )?);
        assert_eq!(f.charge()?, charge);
        assert!(retire(&f.db, &f.custody, id, "different", || typed(id), || {}).is_err());
        retire(&f.db, &f.custody, id, "owner", || typed(id), || {})?;
        assert_eq!(f.charge()?, charge - 7);
        Ok(())
    }

    #[test]
    fn owner_retirement_resolves_only_witnessless_partials_with_proven_absence() -> Result<()> {
        let f = Fixture::new()?;
        let intent = f.admit(1)?.unwrap();
        let id = intent.backup_id;
        f.db.transact(|tx| {
            assert!(store::bind_backup_namespace(
                tx,
                id,
                f.custody.namespace.id
            )?);
            store::settle_backup_failed(
                tx,
                &intent,
                "simulated_guest_phase_crash",
                true,
                false,
                NOW,
            )
        })?;
        let charge = f.charge()?;
        let alternate = backup_fixture_root();
        let foreign = ManagedBackupDir::open(alternate.path(), f.state.path(), None, false)?;
        assert!(retire(&f.db, &foreign, id, "owner", || typed(id), || {}).is_err());
        let leftover = f.custody.staging_path(intent.id)?;
        fs::write(&leftover, b"unattributed")?;
        assert!(retire(&f.db, &f.custody, id, "owner", || typed(id), || {}).is_err());
        assert_eq!(fs::read(&leftover)?, b"unattributed");
        assert_eq!(f.charge()?, charge);
        fs::remove_file(leftover)?;
        retire(&f.db, &f.custody, id, "owner", || typed(id), || {})?;
        assert_eq!(f.charge()?, charge - 65_536);
        f.db.read_backup_owner(|db| {
            let row = store::owner_backups(db, Some(id))?.pop().unwrap();
            assert_eq!(row.sha256, None);
            assert_eq!(row.owner.as_deref(), Some("owner"));
            assert!(row.finished_ms.is_some());
            Ok(())
        })?;

        let pre_bind = f.admit(2)?.unwrap();
        f.db.transact(|tx| {
            store::settle_backup_failed(tx, &pre_bind, "pre_bind_crash", true, false, NOW)
        })?;
        retire(
            &f.db,
            &f.custody,
            pre_bind.backup_id,
            "owner",
            || typed(pre_bind.backup_id),
            || {},
        )?;
        let legacy = f.admit(3)?.unwrap();
        f.db.transact(|tx| {
            tx.execute(
                "UPDATE vm_compose_backups SET namespace_tracked=0 WHERE id=?1",
                params![legacy.backup_id],
            )?;
            store::settle_backup_failed(tx, &legacy, "legacy_crash", true, false, NOW)
        })?;
        let charge = f.charge()?;
        assert!(
            retire(
                &f.db,
                &f.custody,
                legacy.backup_id,
                "owner",
                || typed(legacy.backup_id),
                || {}
            )
            .is_err()
        );
        assert_eq!(f.charge()?, charge);
        Ok(())
    }
}

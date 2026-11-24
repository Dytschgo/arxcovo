use arxd::{store::Activity, vm::*};
use rusqlite::{Connection, params};
use std::{
    path::Path,
    sync::{Arc, Barrier, Mutex},
    thread,
    time::{Duration, Instant},
};

const NOW: i64 = 1_800_000_000_000;
const TEMPLATE: Id = [1; 16];
const VM: Id = [2; 16];
const MIB: u64 = 1024 * 1024;
/// The VM store's `user_version`. `store::vm::VM_SCHEMA_VERSION` is
/// crate-private, so this integration test mirrors it; update both together.
const VM_SCHEMA_VERSION: i64 = 11;
/// The shared activity store's `user_version` (crate-private in `store`),
/// which must stay distinct from the VM store's.
const ACTIVITY_SCHEMA_VERSION: i64 = 5;

#[derive(Clone)]
struct TestClock {
    reading: Arc<Mutex<ClockReading>>,
    next_sample: Arc<Mutex<Option<Arc<ClockPause>>>>,
}

struct ClockPause {
    sampled: Barrier,
    resume: Barrier,
}

impl TestClock {
    fn new() -> Self {
        Self {
            reading: Arc::new(Mutex::new(ClockReading {
                utc_ms: NOW,
                monotonic: Instant::now(),
            })),
            next_sample: Arc::default(),
        }
    }

    fn wall(&self, now: i64) {
        self.reading.lock().unwrap().utc_ms = now;
    }

    fn advance(&self, seconds: u64) {
        self.reading.lock().unwrap().monotonic += Duration::from_secs(seconds);
    }

    fn pause_next_sample(&self) -> Arc<ClockPause> {
        let pause = Arc::new(ClockPause {
            sampled: Barrier::new(2),
            resume: Barrier::new(2),
        });
        assert!(
            self.next_sample
                .lock()
                .unwrap()
                .replace(pause.clone())
                .is_none()
        );
        pause
    }
}

impl Clock for TestClock {
    fn now(&self) -> ClockReading {
        let reading = *self.reading.lock().unwrap();
        let pause = self.next_sample.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.sampled.wait();
            pause.resume.wait();
        }
        reading
    }
}

fn private_dir() -> tempfile::TempDir {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let dir = tempfile::tempdir_in(root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    protect(dir.path());
    dir
}

#[cfg(windows)]
fn protect(path: &Path) {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    unsafe {
        let mut token = ptr::null_mut();
        assert_ne!(
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
            0
        );
        let mut bytes = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut bytes);
        let mut data = vec![0usize; (bytes as usize).div_ceil(std::mem::size_of::<usize>())];
        assert_ne!(
            GetTokenInformation(
                token,
                TokenUser,
                data.as_mut_ptr().cast(),
                bytes,
                &mut bytes
            ),
            0
        );
        CloseHandle(token);
        let user = (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let mut text = ptr::null_mut();
        assert_ne!(ConvertSidToStringSidW(user, &mut text), 0);
        let len = (0..184).find(|&i| *text.add(i) == 0).unwrap();
        let sid = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
        LocalFree(text.cast());

        let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = ptr::null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            ),
            0
        );
        let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
        assert_ne!(
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted),
            0
        );
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_eq!(
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            ),
            0
        );
        LocalFree(descriptor);
    }
}

fn db(dir: &Path) -> Connection {
    Connection::open(dir.join("activity.sqlite3")).unwrap()
}

fn scalar(dir: &Path, sql: &str) -> i64 {
    db(dir).query_row(sql, [], |row| row.get(0)).unwrap()
}

fn host() -> HostObservation {
    HostObservation {
        owner_idle: Some(true),
        logical_cpus: Some(8),
        available_ram: Some(32 * 1024 * MIB),
        capacity: Some(Capacity {
            free_bytes: 400 * 1024 * MIB,
            managed_bytes: 0,
        }),
    }
}

fn resources() -> Resources {
    Resources {
        vcpus: 4,
        guest_ram: 4 * 1024 * MIB,
        overhead_ram: 256 * MIB,
    }
}

struct Fixture {
    controller: VmController<TestClock>,
    dir: tempfile::TempDir,
    clock: TestClock,
}

impl Fixture {
    fn new(policy: VmPolicy) -> Self {
        let dir = private_dir();
        let clock = TestClock::new();
        let controller = VmController::open(dir.path(), policy, clock.clone()).unwrap();
        Self {
            controller,
            dir,
            clock,
        }
    }

    fn sealed() -> Self {
        let f = Self::new(VmPolicy::default());
        f.seal(1).unwrap();
        f
    }

    fn seal(&self, byte: u8) -> Result<(), Refusal> {
        self.controller
            .seal_template(
                SealedTemplate {
                    id: [byte; 16],
                    guest: Guest::UbuntuX86_64,
                    recipe_digest: [byte; 32],
                    disk_digest: [5; 32],
                    vars_digest: [6; 32],
                    disk: DiskClaim {
                        images: MIB,
                        ..DiskClaim::default()
                    },
                },
                host(),
                resources(),
            )
            .unwrap()
    }

    fn clone_vm(&self, vm: Id, nonce: u64, ttl: Option<u64>) -> Admission {
        self.admit(request(
            vm,
            nonce,
            Operation::Clone {
                template: TEMPLATE,
                lease_seconds: ttl,
            },
            DiskClaim {
                images: MIB,
                ..DiskClaim::default()
            },
        ))
    }

    fn admit(&self, request: Request<'_>) -> Admission {
        self.controller.admit(request, host(), resources()).unwrap()
    }

    fn ready(&self, vm: Id, ttl: Option<u64>) {
        self.finish(self.clone_vm(vm, u64::from(vm[0]), ttl));
    }

    fn running(&self) {
        self.ready(VM, None);
        self.finish(self.admit(request(VM, 100, Operation::Start, DiskClaim::default())));
    }

    fn finish(&self, admission: Admission) -> i64 {
        let pending = admitted(admission);
        assert_eq!(
            self.controller
                .finish(&pending, Completion::Succeeded)
                .unwrap(),
            Status::Succeeded
        );
        pending.activity_id()
    }
}

fn key(now: i64, nonce: u64) -> RequestId {
    RequestId::parse(&format!("{now:016x}{nonce:016x}")).unwrap()
}

fn request(vm: Id, nonce: u64, operation: Operation<'_>, disk: DiskClaim) -> Request<'_> {
    Request {
        principal: "alice",
        request_id: key(NOW, nonce),
        vm,
        operation,
        disk,
    }
}

fn script(vm: Id, nonce: u64, text: &str) -> Request<'_> {
    request(
        vm,
        nonce,
        Operation::Script {
            script: text,
            timeout_seconds: 10,
        },
        DiskClaim::default(),
    )
}

fn admitted(result: Admission) -> PendingOperation {
    match result {
        Admission::Admitted(pending) => pending,
        other => panic!("expected admission, got {other:?}"),
    }
}

fn refused(result: Admission, expected: Refusal) {
    assert!(
        matches!(result, Admission::Refused(reason) if reason == expected),
        "{result:?}"
    );
}

#[test]
fn shared_and_vm_openers_keep_distinct_versions_and_locks() {
    let dir = private_dir();
    let shared = Activity::open(dir.path()).unwrap();
    let a = shared.start("alice", "run_command").unwrap();
    shared.finish(a, "succeeded").unwrap();
    drop(shared);
    let path = dir.path().join("activity.sqlite3");
    let original = std::fs::read(&path).unwrap();
    drop(Activity::open(dir.path()).unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        scalar(dir.path(), "PRAGMA user_version"),
        ACTIVITY_SCHEMA_VERSION
    );
    assert!(VmController::open(dir.path(), VmPolicy::default(), TestClock::new()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        scalar(
            dir.path(),
            "SELECT count(*) FROM sqlite_schema WHERE name LIKE 'vm_%'"
        ),
        0
    );

    let f = Fixture::sealed();
    assert_eq!(
        scalar(f.dir.path(), "PRAGMA user_version"),
        VM_SCHEMA_VERSION
    );
    assert!(VmController::open(f.dir.path(), VmPolicy::default(), f.clock.clone()).is_err());
    let Fixture {
        controller,
        dir,
        clock,
    } = f;
    drop(controller);
    assert!(Activity::open(dir.path()).is_err());
    assert_eq!(scalar(dir.path(), "PRAGMA user_version"), VM_SCHEMA_VERSION);
    drop(VmController::open(dir.path(), VmPolicy::default(), clock).unwrap());
    assert_eq!(scalar(dir.path(), "SELECT count(*) FROM vm_templates"), 1);

    db(dir.path())
        .pragma_update(None, "user_version", VM_SCHEMA_VERSION + 1)
        .unwrap();
    assert!(Activity::open(dir.path()).is_err());
    assert!(VmController::open(dir.path(), VmPolicy::default(), TestClock::new()).is_err());
}

#[test]
fn shared_v1_v2_v3_migrations_preserve_rows_sequence_and_delegated_references() {
    for version in 1..=3 {
        let dir = private_dir();
        drop(Activity::open(dir.path()).unwrap());
        let db = db(dir.path());
        if version == 1 {
            db.execute_batch(
                "DROP TABLE activity;
                CREATE TABLE activity(id INTEGER PRIMARY KEY AUTOINCREMENT,principal TEXT NOT NULL,
                tool TEXT NOT NULL,outcome TEXT NOT NULL);",
            )
            .unwrap();
        } else {
            db.execute_batch("ALTER TABLE activity DROP COLUMN identity_mode;")
                .unwrap();
            if version == 3 {
                db.execute_batch("ALTER TABLE activity ADD COLUMN identity_mode TEXT;")
                    .unwrap();
            }
        }
        if version == 1 {
            db.execute_batch("INSERT INTO activity VALUES(41,'alice','run_command','succeeded');")
                .unwrap();
        } else {
            db.execute_batch(
                "INSERT INTO activity(id,principal,tool,outcome,actor_kind,class)
                 VALUES(41,'alice','run_command','succeeded','agent','action');",
            )
            .unwrap();
        }
        db.execute_batch(
            "UPDATE sqlite_sequence SET seq=80 WHERE name='activity';
             INSERT INTO delegated_build VALUES(zeroblob(32),zeroblob(32),41,1);",
        )
        .unwrap();
        db.pragma_update(None, "user_version", version).unwrap();
        drop(db);
        let shared = Activity::open(dir.path()).unwrap();
        assert_eq!(
            scalar(dir.path(), "PRAGMA user_version"),
            ACTIVITY_SCHEMA_VERSION
        );
        assert_eq!(
            scalar(
                dir.path(),
                "SELECT id FROM activity WHERE principal='alice'
             AND tool='run_command' AND outcome='succeeded'"
            ),
            41
        );
        assert_eq!(
            scalar(
                dir.path(),
                "SELECT activity_id FROM delegated_build WHERE forwarded=1"
            ),
            41
        );
        assert_eq!(shared.start("alice", "run_command").unwrap(), 81);
    }
}

#[test]
fn vm_admission_rolls_back_intent_activity_and_key_on_sql_failure() {
    let f = Fixture::sealed();
    db(f.dir.path())
        .execute_batch(
            "CREATE TRIGGER inject_claim_failure BEFORE INSERT ON vm_disk_claims
        BEGIN SELECT RAISE(ABORT,'fixture rejects claim'); END;",
        )
        .unwrap();
    let clone = || {
        request(
            VM,
            2,
            Operation::Clone {
                template: TEMPLATE,
                lease_seconds: None,
            },
            DiskClaim {
                images: MIB,
                ..DiskClaim::default()
            },
        )
    };
    assert!(f.controller.admit(clone(), host(), resources()).is_err());
    for table in ["vm_instances", "vm_operations", "vm_disk_claims"] {
        assert_eq!(
            scalar(f.dir.path(), &format!("SELECT count(*) FROM {table}")),
            0
        );
    }
    assert_eq!(scalar(f.dir.path(), "SELECT count(*) FROM activity"), 1);
    db(f.dir.path())
        .execute_batch("DROP TRIGGER inject_claim_failure")
        .unwrap();
    f.finish(f.admit(clone()));
}

#[test]
fn sqlite_headroom_refuses_new_work_and_preserves_completion_and_cleanup() {
    let f = Fixture::sealed();
    f.ready(VM, Some(60));
    let pending = admitted(f.admit(request(VM, 3, Operation::Start, DiskClaim::default())));
    let raw = db(f.dir.path());
    raw.execute_batch(
        "CREATE TABLE pressure(payload BLOB);
        INSERT INTO pressure VALUES(zeroblob(3960000));",
    )
    .unwrap();
    refused(f.clone_vm([3; 16], 4, None), Refusal::StateHeadroom);
    assert_eq!(
        f.controller
            .finish(&pending, Completion::Succeeded)
            .unwrap(),
        Status::Succeeded
    );
    f.clock.wall(NOW + 60_000);
    let claim = f.controller.claim_expired().unwrap()[0];
    f.controller
        .finish_expiry(claim, CleanupResult::Confirmed)
        .unwrap();
    raw.execute_batch("DROP TABLE pressure").unwrap();
    f.finish(f.clone_vm([3; 16], 4, None));
}

#[test]
fn scoped_script_retries_only_return_status_and_never_persist_input() {
    use sha2::{Digest, Sha256};
    let f = Fixture::sealed();
    f.running();
    let secret = "vm-policy fixture script sentinel café -- exactly these bytes";
    let pending = admitted(f.admit(script(VM, 101, secret)));
    let same = || f.admit(script(VM, 101, "\0 invalid changed arguments"));
    assert!(matches!(
        same(),
        Admission::Existing(Receipt {
            status: Status::Pending,
            ..
        })
    ));
    refused(
        f.admit(script([3; 16], 101, "changed VM")),
        Refusal::KeyCollision,
    );
    refused(
        f.admit(request(VM, 101, Operation::Stop, DiskClaim::default())),
        Refusal::KeyCollision,
    );
    let mut foreign = script(VM, 101, "same key, different principal");
    foreign.principal = "bob";
    refused(f.admit(foreign), Refusal::NotFound);
    assert_eq!(
        f.controller
            .finish(&pending, Completion::Succeeded)
            .unwrap(),
        Status::Succeeded
    );
    assert!(
        matches!(same(), Admission::Existing(Receipt { activity_id, status: Status::Succeeded })
        if activity_id == pending.activity_id())
    );

    let raw = db(f.dir.path());
    let digest = Sha256::digest(secret.as_bytes());
    let hex = hex::encode(digest);
    let sentinels = [secret.as_bytes(), digest.as_slice(), hex.as_bytes()];
    for table in [
        "activity",
        "vm_operations",
        "vm_instances",
        "vm_disk_claims",
    ] {
        let mut stmt = raw.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..columns {
                if let Ok(bytes) = row.get_ref(i).unwrap().as_bytes() {
                    assert!(
                        sentinels
                            .iter()
                            .all(|s| !bytes.windows(s.len()).any(|w| w == *s))
                    );
                }
            }
        }
    }
    let bytes = std::fs::read(f.dir.path().join("activity.sqlite3")).unwrap();
    assert!(
        sentinels
            .iter()
            .all(|s| !bytes.windows(s.len()).any(|w| w == *s))
    );

    f.clock.wall(NOW - 1);
    assert!(matches!(same(), Admission::Existing(_)));
    f.clock.wall(NOW + RETRY_WINDOW_MS);
    assert!(matches!(same(), Admission::Existing(_)));
    f.clock.wall(NOW + RETRY_WINDOW_MS + 1);
    refused(same(), Refusal::StaleKey);
    refused(
        f.admit(script(VM, 102, "stale first dispatch")),
        Refusal::StaleKey,
    );
    f.clock.wall(NOW);
    refused(same(), Refusal::StaleKey);
}

#[test]
fn full_ledger_retains_replay_after_receipt_pruning_then_recovers() {
    let f = Fixture::sealed();
    f.running();
    let first = f.finish(f.admit(script(VM, 101, "bounded fixture")));
    let mut raw = db(f.dir.path());
    let tx = raw.transaction().unwrap();
    for nonce in 102_u64..610 {
        tx.execute(
            "INSERT INTO activity(principal,actor,vm,action,started_ms,status,finished_ms,terminal_order)
            VALUES('alice','agent',?1,2,?2,1,?2,?3)",
            params![VM, NOW, nonce],
        )
        .unwrap();
        let key = [NOW.to_be_bytes(), nonce.to_be_bytes()].concat();
        tx.execute(
            "INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,valid_through_ms,
                status,script_result) VALUES('alice',?1,?2,2,?3,?4,1,'legacy')",
            params![key, VM, tx.last_insert_rowid(), NOW + RETRY_WINDOW_MS],
        )
        .unwrap();
    }
    // Set up the bounded history in one transaction, leaving the original
    // receipt as the oldest entry. The real next completion must prune it.
    tx.execute(
        "DELETE FROM activity WHERE id<>?1 AND id IN
        (SELECT id FROM activity ORDER BY id DESC LIMIT -1 OFFSET 127)",
        [first],
    )
    .unwrap();
    tx.commit().unwrap();
    f.finish(f.admit(script(VM, 610, "last available slot")));
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM vm_operations"),
        512
    );
    assert_eq!(
        scalar(
            f.dir.path(),
            "SELECT count(*) FROM activity WHERE status<>0"
        ),
        128
    );
    assert_eq!(
        db(f.dir.path())
            .query_row("SELECT count(*) FROM activity WHERE id=?1", [first], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    assert!(
        matches!(f.admit(script(VM, 101, "different")), Admission::Existing(Receipt { activity_id, status: Status::Succeeded }) if activity_id == first)
    );
    refused(f.admit(script(VM, 611, "new")), Refusal::ReplayFull);
    let later = NOW + RETRY_WINDOW_MS + 1;
    f.clock.wall(later);
    let mut fresh = script(VM, 611, "new");
    fresh.request_id = key(later, 611);
    f.finish(f.admit(fresh));
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM vm_operations"),
        1
    );
    refused(f.admit(script(VM, 101, "old")), Refusal::StaleKey);
    f.clock.wall(NOW);
    refused(f.admit(script(VM, 101, "still old")), Refusal::StaleKey);
}

#[test]
fn concurrent_duplicate_admission_has_one_worker() {
    let f = Fixture::sealed();
    f.running();
    let barrier = Barrier::new(2);
    let results = thread::scope(|s| {
        (0..2)
            .map(|_| {
                s.spawn(|| {
                    barrier.wait();
                    f.admit(script(VM, 101, "concurrent dispatch"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut worker = None;
    let mut lookup = None;
    for result in results {
        match result {
            Admission::Admitted(pending) => assert!(worker.replace(pending).is_none()),
            Admission::Existing(receipt) => assert!(lookup.replace(receipt).is_none()),
            other => panic!("unanticipated admission: {other:?}"),
        }
    }
    assert_eq!(worker.unwrap().activity_id(), lookup.unwrap().activity_id);
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM activity WHERE action=2"),
        1
    );
}

#[test]
fn utc_lease_and_monotonic_work_deadlines_use_separate_clocks() {
    let f = Fixture::sealed();
    f.running();
    assert_eq!(
        scalar(f.dir.path(), "SELECT expires_ms FROM vm_instances"),
        NOW + 604_800_000
    );
    assert_eq!(
        f.controller
            .extend_lease("alice", VM, key(NOW, 201), 60)
            .unwrap(),
        Err(Refusal::Invalid)
    );
    assert_eq!(
        f.controller
            .extend_lease("bob", VM, key(NOW, 201), 604_800)
            .unwrap(),
        Err(Refusal::NotFound)
    );
    let shift = NOW + 30_000;
    f.clock.wall(shift);
    let expires = f
        .controller
        .extend_lease("alice", VM, key(NOW, 202), DEFAULT_LEASE_SECONDS)
        .unwrap()
        .unwrap();
    assert_eq!(expires, shift + 604_800_000);
    assert_eq!(
        f.controller
            .extend_lease("alice", VM, key(NOW, 203), u64::MAX)
            .unwrap(),
        Err(Refusal::Invalid)
    );

    let pending = admitted(f.admit(script(VM, 101, "still within deadline")));
    f.clock.wall(NOW + 500_000_000);
    assert_eq!(
        f.controller
            .finish(&pending, Completion::Succeeded)
            .unwrap(),
        Status::Succeeded
    );
    f.clock.wall(NOW - 1000);
    let mut late = script(VM, 102, "deadline passes despite backward UTC");
    late.request_id = key(NOW - 1000, 102);
    // The preceding finish did not consult the replay clock.
    let pending = admitted(f.admit(late));
    f.clock.advance(10);
    assert_eq!(
        f.controller
            .finish(&pending, Completion::Succeeded)
            .unwrap(),
        Status::Interrupted
    );
    assert!(f.controller.claim_expired().unwrap().is_empty());
    assert_eq!(f.controller.recovery_intents().unwrap()[0].vm, VM);

    f.clock.wall(253_402_300_799_000);
    refused(
        f.admit(Request {
            request_id: key(253_402_300_799_000, 555),
            ..request(
                [3; 16],
                555,
                Operation::Clone {
                    template: TEMPLATE,
                    lease_seconds: Some(60),
                },
                DiskClaim {
                    images: MIB,
                    ..DiskClaim::default()
                },
            )
        }),
        Refusal::Invalid,
    );
}

#[test]
fn metadata_clone_at_utc_deadline_settles_as_staging_before_sweep() {
    let f = Fixture::sealed();
    let pending = admitted(f.clone_vm(VM, 20, Some(60)));
    f.clock.wall(NOW + 60_000);
    f.clock.advance(1);
    assert!(f.clock.now().monotonic < pending.deadline());

    assert_eq!(
        f.controller
            .finish(&pending, Completion::Succeeded)
            .unwrap(),
        Status::Interrupted
    );
    assert_eq!(
        scalar(
            f.dir.path(),
            "SELECT count(*) FROM vm_instances WHERE cleanup_route='staging'"
        ),
        1
    );
    assert_eq!(
        scalar(
            f.dir.path(),
            "SELECT count(*) FROM vm_operations WHERE action=1 AND status=3 AND cleanup_confirmed=0"
        ),
        1
    );
    assert_eq!(
        scalar(f.dir.path(), "SELECT sum(persistent) FROM vm_disk_claims"),
        MIB as i64
    );
    assert!(matches!(
        f.clone_vm(VM, 20, Some(60)),
        Admission::Existing(Receipt { activity_id, status: Status::Interrupted })
            if activity_id == pending.activity_id()
    ));
    assert!(f.controller.claim_expired().unwrap().is_empty());
    assert!(f.controller.expired_status("alice", VM).unwrap().is_none());
}

#[test]
fn extension_wins_if_it_samples_before_the_deadline() {
    let f = Fixture::sealed();
    f.ready(VM, Some(60));
    f.clock.wall(NOW + 59_999);
    let pause = f.clock.pause_next_sample();
    thread::scope(|s| {
        let extend = s.spawn(|| {
            f.controller
                .extend_lease("alice", VM, key(NOW, 20), 120)
                .unwrap()
        });
        pause.sampled.wait();
        f.clock.wall(NOW + 60_000);
        let sweep = s.spawn(|| f.controller.claim_expired().unwrap());
        pause.resume.wait();
        assert_eq!(extend.join().unwrap(), Ok(NOW + 179_999));
        assert!(sweep.join().unwrap().is_empty());
    });
    assert_eq!(
        scalar(f.dir.path(), "SELECT expires_ms FROM vm_instances"),
        NOW + 179_999
    );
}

#[test]
fn expiry_claim_wins_even_if_utc_moves_back_before_extension() {
    let f = Fixture::sealed();
    f.ready(VM, Some(60));
    f.clock.wall(NOW + 60_000);
    let pause = f.clock.pause_next_sample();
    let (claims, extension) = thread::scope(|s| {
        let sweep = s.spawn(|| f.controller.claim_expired().unwrap());
        pause.sampled.wait();
        f.clock.wall(NOW + 59_999);
        let extend = s.spawn(|| {
            f.controller
                .extend_lease("alice", VM, key(NOW, 20), 120)
                .unwrap()
        });
        let second = s.spawn(|| f.controller.claim_expired().unwrap());
        pause.resume.wait();
        assert!(second.join().unwrap().is_empty());
        (sweep.join().unwrap(), extend.join().unwrap())
    });
    assert_eq!(extension, Err(Refusal::Expired));
    let [claim] = claims.as_slice() else {
        panic!("one expiry claim expected")
    };
    f.clock.wall(NOW - 1);
    assert_eq!(
        f.controller
            .extend_lease("alice", VM, key(NOW - 1, 21), 120)
            .unwrap(),
        Err(Refusal::Expired)
    );
    assert!(f.controller.claim_expired().unwrap().is_empty());
    f.controller
        .finish_expiry(*claim, CleanupResult::Unconfirmed)
        .unwrap();
    assert_eq!(f.controller.expiry_intents().unwrap(), [*claim]);
    assert_eq!(
        scalar(f.dir.path(), "SELECT sum(persistent) FROM vm_disk_claims"),
        MIB as i64
    );
    f.controller
        .finish_expiry(*claim, CleanupResult::Confirmed)
        .unwrap();
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM vm_disk_claims"),
        0
    );
    assert_eq!(scalar(f.dir.path(), "SELECT count(*) FROM vm_templates"), 1);
    assert_eq!(
        scalar(
            f.dir.path(),
            "SELECT count(*) FROM activity WHERE actor='daemon' AND vm_owner='alice'
        AND action=7 AND status=1 AND cleanup_confirmed=1"
        ),
        1
    );
}

#[test]
fn restart_retains_expiry_and_staging_cleanup_intents_without_replay() {
    let f = Fixture::sealed();
    f.ready(VM, Some(60));
    let creating = [3; 16];
    let clone = || {
        request(
            creating,
            3,
            Operation::Clone {
                template: TEMPLATE,
                lease_seconds: None,
            },
            DiskClaim {
                images: MIB,
                staging: MIB,
                metadata: 4096,
            },
        )
    };
    admitted(f.admit(clone()));
    f.clock.wall(NOW + 60_000);
    let [claim]: [ExpiryClaim; 1] = f.controller.claim_expired().unwrap().try_into().unwrap();
    let Fixture {
        controller,
        dir,
        clock,
    } = f;
    drop(controller);
    let controller = VmController::open(dir.path(), VmPolicy::default(), clock).unwrap();
    assert_eq!(controller.expiry_intents().unwrap(), [claim]);
    assert!(controller.claim_expired().unwrap().is_empty());
    assert!(
        controller
            .sweep_once(&runtime::CandidateRuntime::new(None))
            .unwrap()
    );
    assert_eq!(
        scalar(dir.path(), "SELECT sum(staging) FROM vm_disk_claims"),
        MIB as i64
    );
    assert!(matches!(
        controller.admit(clone(), host(), resources()).unwrap(),
        Admission::Existing(Receipt {
            status: Status::Interrupted,
            ..
        })
    ));
    assert_eq!(controller.expiry_intents().unwrap(), [claim]);
    assert_eq!(controller.expired_status("alice", creating).unwrap(), None);
    assert_eq!(scalar(dir.path(), "SELECT count(*) FROM vm_disk_claims"), 2);
    assert_eq!(
        scalar(
            dir.path(),
            "SELECT count(*) FROM activity WHERE action=10 AND
        principal='arxd' AND vm_owner='alice' AND cleanup_confirmed=0"
        ),
        1
    );
}

#[test]
fn catalog_pending_instance_and_snapshot_caps_hold() {
    let f = Fixture::sealed();
    for byte in 2..=4 {
        f.seal(byte).unwrap();
    }
    assert_eq!(f.seal(5), Err(Refusal::TemplateLimit));
    let pending: Vec<_> = (10..18)
        .map(|byte| admitted(f.clone_vm([byte; 16], byte.into(), None)))
        .collect();
    refused(f.clone_vm([18; 16], 18, None), Refusal::PendingFull);
    for receipt in pending {
        assert_eq!(
            f.controller
                .finish(&receipt, Completion::Succeeded)
                .unwrap(),
            Status::Succeeded
        );
    }
    refused(f.clone_vm([18; 16], 18, None), Refusal::InstanceLimit);
    let snapshot = |vm, byte, nonce| {
        request(
            [vm; 16],
            nonce,
            Operation::Snapshot {
                snapshot: [byte; 16],
            },
            DiskClaim {
                images: MIB,
                staging: 4096,
                metadata: 4096,
            },
        )
    };
    // The metadata API has no paired-copy authority.
    refused(f.admit(snapshot(10, 1, 101)), Refusal::Invalid);
    assert_eq!(scalar(f.dir.path(), "SELECT count(*) FROM vm_snapshots"), 0);
}

#[test]
fn disk_claims_serialize_with_staging_metadata_and_checked_arithmetic() {
    let f = Fixture::new(VmPolicy {
        disk_cap: 14 * MIB,
        ..VmPolicy::default()
    });
    f.seal(1).unwrap();
    let clone = |vm, claim| {
        request(
            [vm; 16],
            vm.into(),
            Operation::Clone {
                template: TEMPLATE,
                lease_seconds: None,
            },
            claim,
        )
    };
    refused(
        f.admit(clone(
            2,
            DiskClaim {
                images: 2 * MIB,
                staging: 2 * MIB,
                metadata: 1,
            },
        )),
        Refusal::Disk,
    );
    refused(
        f.admit(clone(
            2,
            DiskClaim {
                images: u64::MAX,
                staging: 1,
                metadata: 1,
            },
        )),
        Refusal::Disk,
    );
    refused(
        f.admit(clone(
            2,
            DiskClaim {
                images: i64::MAX as u64,
                staging: 1,
                metadata: 0,
            },
        )),
        Refusal::Disk,
    );

    let barrier = Barrier::new(2);
    let decisions = thread::scope(|s| {
        (2..4)
            .map(|vm| {
                let barrier = &barrier;
                let f = &f;
                s.spawn(move || {
                    barrier.wait();
                    f.admit(clone(
                        vm,
                        DiskClaim {
                            images: MIB,
                            staging: MIB,
                            metadata: MIB,
                        },
                    ))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        decisions
            .iter()
            .filter(|a| matches!(a, Admission::Admitted(_)))
            .count(),
        1
    );
    assert_eq!(
        decisions
            .iter()
            .filter(|a| matches!(a, Admission::Refused(Refusal::Disk)))
            .count(),
        1
    );
    assert_eq!(
        scalar(
            f.dir.path(),
            "SELECT sum(persistent+staging) FROM vm_disk_claims"
        ),
        (3 * MIB) as i64
    );
}

#[test]
fn host_unknown_owner_activity_cpu_and_ram_refuse_without_claims() {
    let f = Fixture::sealed();
    let cases = [
        (
            HostObservation {
                capacity: None,
                ..host()
            },
            resources(),
            Refusal::UnknownHost,
        ),
        (
            HostObservation {
                owner_idle: None,
                ..host()
            },
            resources(),
            Refusal::UnknownHost,
        ),
        (
            HostObservation {
                owner_idle: Some(false),
                ..host()
            },
            resources(),
            Refusal::OwnerActive,
        ),
        (
            HostObservation {
                logical_cpus: Some(6),
                ..host()
            },
            resources(),
            Refusal::Cpu,
        ),
        (
            HostObservation {
                available_ram: Some(16 * 1024 * MIB),
                ..host()
            },
            resources(),
            Refusal::Ram,
        ),
        (
            host(),
            Resources {
                overhead_ram: u64::MAX,
                ..resources()
            },
            Refusal::Ram,
        ),
        (
            HostObservation {
                capacity: Some(Capacity {
                    free_bytes: 1,
                    managed_bytes: 0,
                }),
                ..host()
            },
            resources(),
            Refusal::Disk,
        ),
    ];
    for (observation, resources, reason) in cases {
        let request = request(
            VM,
            2,
            Operation::Clone {
                template: TEMPLATE,
                lease_seconds: None,
            },
            DiskClaim {
                images: MIB,
                ..DiskClaim::default()
            },
        );
        refused(
            f.controller.admit(request, observation, resources).unwrap(),
            reason,
        );
    }
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM vm_operations"),
        0
    );
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM activity WHERE status=4"),
        cases.len() as i64
    );
}

#[test]
fn shutdown_is_durable_and_blocks_dispatch_after_lookup() {
    let f = Fixture::sealed();
    f.running();
    let pending = admitted(f.admit(script(VM, 101, "interrupted")));
    assert!(f.controller.recover_after_drain().is_err());
    f.controller.close_admission().unwrap();
    assert!(matches!(
        f.admit(script(VM, 101, "retry during drain")),
        Admission::Existing(Receipt {
            status: Status::Pending,
            ..
        })
    ));
    assert!(f.controller.recovery_intents().unwrap().is_empty());
    f.controller.recover_after_drain().unwrap();
    assert!(matches!(
        f.admit(script(VM, 101, "retry")),
        Admission::Existing(Receipt {
            status: Status::Interrupted,
            ..
        })
    ));
    refused(
        f.admit(script(VM, 102, "after shutdown")),
        Refusal::Shutdown,
    );
    assert!(
        f.controller
            .finish(&pending, Completion::Succeeded)
            .is_err()
    );
    let claim = f.controller.recovery_intents().unwrap()[0];
    assert_eq!(claim.vm, VM);
    f.controller
        .finish_recovery(claim, CleanupResult::Unconfirmed)
        .unwrap();
    assert_eq!(f.controller.recovery_intents().unwrap(), [claim]);
    f.controller
        .finish_recovery(claim, CleanupResult::Confirmed)
        .unwrap();
    assert!(
        f.controller
            .finish_recovery(claim, CleanupResult::Confirmed)
            .is_err()
    );
    assert!(f.controller.recovery_intents().unwrap().is_empty());
}

#[test]
fn extension_replays_the_original_expiry_across_reopen() {
    let f = Fixture::sealed();
    f.ready(VM, Some(60));
    let original = key(NOW, 20);
    assert_eq!(
        f.controller
            .extend_lease("alice", VM, original, 120)
            .unwrap(),
        Ok(NOW + 120_000)
    );
    f.clock.wall(NOW + 30_000);
    assert_eq!(
        f.controller
            .extend_lease("alice", VM, original, u64::MAX)
            .unwrap(),
        Ok(NOW + 120_000)
    );
    assert_eq!(
        f.controller
            .extend_lease("alice", [3; 16], original, 180)
            .unwrap(),
        Err(Refusal::KeyCollision)
    );
    assert_eq!(
        f.controller.extend_lease("bob", VM, original, 180).unwrap(),
        Err(Refusal::NotFound)
    );
    let mut collision = request(VM, 20, Operation::Start, DiskClaim::default());
    collision.request_id = original;
    refused(f.admit(collision), Refusal::KeyCollision);
    assert_eq!(
        f.controller
            .extend_lease("alice", VM, key(NOW, 21), 180)
            .unwrap(),
        Ok(NOW + 210_000)
    );
    let Fixture {
        controller,
        dir,
        clock,
    } = f;
    drop(controller);
    let controller = VmController::open(dir.path(), VmPolicy::default(), clock).unwrap();
    assert_eq!(
        controller.extend_lease("alice", VM, original, 240).unwrap(),
        Ok(NOW + 120_000)
    );
    assert_eq!(
        scalar(dir.path(), "SELECT expires_ms FROM vm_instances"),
        NOW + 210_000
    );
    assert_eq!(
        scalar(
            dir.path(),
            "SELECT count(*) FROM activity WHERE action=8 AND status=1"
        ),
        2
    );
}

#[test]
fn late_cleanup_confirmation_survives_terminal_pruning_and_backward_utc() {
    let f = Fixture::sealed();
    f.running();
    let pending = admitted(f.admit(script(VM, 101, "uncertain cleanup fixture")));
    assert_eq!(
        f.controller
            .finish(&pending, Completion::CleanupUnconfirmed)
            .unwrap(),
        Status::Interrupted
    );
    let claim = f.controller.recovery_intents().unwrap()[0];
    db(f.dir.path()).execute_batch(
        "DELETE FROM activity WHERE status<>0;
         WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM seq WHERE n<128)
         INSERT INTO activity(principal,actor,vm,action,started_ms,finished_ms,status,terminal_order)
         SELECT 'alice','agent',zeroblob(16),8,1800000000000,1800000000000,1,n FROM seq;",
    ).unwrap();
    f.clock.wall(NOW - 1);
    f.controller
        .finish_recovery(claim, CleanupResult::Confirmed)
        .unwrap();
    assert_eq!(
        scalar(
            f.dir.path(),
            "SELECT count(*) FROM activity WHERE status<>0"
        ),
        128
    );
    let proof: (i64, bool, i64) = db(f.dir.path())
        .query_row(
            "SELECT status,cleanup_confirmed,terminal_order FROM activity WHERE id=?1",
            [claim.activity_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(proof, (1, true, 129));
}

#[test]
fn hardlinked_vm_database_cannot_reconcile_another_roots_pending_work() {
    let f = Fixture::sealed();
    let pending = admitted(f.clone_vm(VM, 2, None));
    let alias = private_dir();
    let original_path = f.dir.path().join("activity.sqlite3");
    let alias_path = alias.path().join("activity.sqlite3");
    let mut active = Some(f.controller);
    if let Err(error) = std::fs::hard_link(&original_path, &alias_path) {
        // Some filesystems require the original to be closed to create a
        // link. The persisted pending row still detects an unsafe reopen.
        drop(active.take());
        if let Err(retry) = std::fs::hard_link(&original_path, &alias_path) {
            if retry.kind() == std::io::ErrorKind::Unsupported {
                return;
            }
            panic!("hard-link fixture: {error}; closed-file retry: {retry}");
        }
    }
    let rejected = VmController::open(alias.path(), VmPolicy::default(), f.clock.clone());
    let Err(error) = rejected else {
        panic!("hard-linked VM state was accepted")
    };
    assert!(error.to_string().contains("single-link"), "{error:#}");
    assert_eq!(scalar(f.dir.path(), "SELECT status FROM vm_operations"), 0);
    std::fs::remove_file(alias_path).unwrap();
    if let Some(controller) = active {
        assert_eq!(
            controller.finish(&pending, Completion::Succeeded).unwrap(),
            Status::Succeeded
        );
    }
}

#[test]
fn caller_saturation_leaves_exactly_one_cleanup_slot_per_instance() {
    let f = Fixture::sealed();
    let _workers: Vec<_> = (10..18)
        .map(|vm| admitted(f.clone_vm([vm; 16], vm.into(), Some(60))))
        .collect();
    refused(f.clone_vm([18; 16], 18, Some(60)), Refusal::PendingFull);
    f.clock.wall(NOW + 60_000);
    let barrier = Barrier::new(2);
    let (first, second) = thread::scope(|s| {
        let sweep = || {
            barrier.wait();
            f.controller.claim_expired().unwrap()
        };
        let first = s.spawn(sweep);
        let second = s.spawn(sweep);
        (first.join().unwrap(), second.join().unwrap())
    });
    assert!(first.is_empty() || second.is_empty());
    let claims: Vec<_> = first.into_iter().chain(second).collect();
    assert_eq!(claims.len(), 8);
    assert_eq!(
        scalar(
            f.dir.path(),
            "SELECT count(*) FROM activity WHERE status=0 AND action<=6"
        ),
        8
    );
    assert_eq!(
        scalar(
            f.dir.path(),
            "SELECT count(*) FROM activity WHERE status=0 AND action IN (7,10)"
        ),
        8
    );
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM activity WHERE status=0"),
        16
    );
    f.controller
        .finish_expiry(claims[0], CleanupResult::Confirmed)
        .unwrap();
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM activity WHERE status=0"),
        14
    );
    assert!(f.controller.claim_expired().unwrap().is_empty());
}

#[test]
fn confirmed_expiry_keeps_only_a_bounded_owner_scoped_tombstone() {
    let f = Fixture::sealed();
    f.ready(VM, Some(60));
    f.clock.wall(NOW + 60_000);
    let claim = f.controller.claim_expired().unwrap()[0];
    f.controller
        .finish_expiry(claim, CleanupResult::Unconfirmed)
        .unwrap();
    assert_eq!(f.controller.expired_status("alice", VM).unwrap(), None);
    f.controller
        .finish_expiry(claim, CleanupResult::Confirmed)
        .unwrap();
    assert_eq!(
        f.controller.expired_status("alice", VM).unwrap(),
        Some(ExpiredVm {
            expired_at_ms: NOW + 60_000,
            activity_id: claim.activity_id,
        })
    );
    assert_eq!(f.controller.expired_status("bob", VM).unwrap(), None);
    assert_eq!(scalar(f.dir.path(), "SELECT count(*) FROM vm_instances"), 0);
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM vm_disk_claims"),
        0
    );
    assert_eq!(scalar(f.dir.path(), "SELECT count(*) FROM vm_templates"), 1);
    refused(f.clone_vm(VM, 3, Some(60)), Refusal::Busy);
    for vm in 10..42 {
        f.ready([vm; 16], Some(60));
        f.clock.wall(NOW + (i64::from(vm) - 8) * 60_000);
        let claim = f.controller.claim_expired().unwrap()[0];
        f.controller
            .finish_expiry(claim, CleanupResult::Confirmed)
            .unwrap();
    }
    assert_eq!(
        scalar(f.dir.path(), "SELECT count(*) FROM vm_tombstones"),
        32
    );
    assert_eq!(f.controller.expired_status("alice", VM).unwrap(), None);
    let Fixture {
        controller,
        dir,
        clock,
    } = f;
    drop(controller);
    let controller = VmController::open(dir.path(), VmPolicy::default(), clock.clone()).unwrap();
    let last = controller
        .expired_status("alice", [41; 16])
        .unwrap()
        .unwrap();
    clock.wall(last.expired_at_ms + 24 * 60 * 60 * 1000);
    assert_eq!(controller.expired_status("alice", [41; 16]).unwrap(), None);
    assert_eq!(scalar(dir.path(), "SELECT count(*) FROM vm_tombstones"), 0);
}

#[test]
fn request_id_encoding_is_strict() {
    for key in [
        "",
        "0000000000000000000000000000000",
        "0000000000000000000000000000000A",
        "ffffffffffffffff0000000000000000",
        "0000000000000000000000000000000z",
    ] {
        assert_eq!(RequestId::parse(key), Err(Refusal::Invalid));
    }
    assert!(RequestId::parse("0000000000000000000000000000000a").is_ok());
}

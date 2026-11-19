//! Existing-store inspection precedes every create-capable opener. No Engine,
//! credential release, key unseal or runtime handoff is implemented here.
use super::{
    checked_clock::{CheckedClock, HostClock},
    preflight,
};
use crate::cluster::{
    enrollment::{
        ClusterId,
        codec::{self, Reader},
    },
    join_receipt::{ExpectedIdentity, PublicIdentity},
    log_store::{MAX_LIVE, VoteLogStore},
    startup_plan::{Owner, Plan, Refusal},
    state_store::{Entry, Init, LogPos, MembershipBlob, StateStore, StoreError, get_pos, put_pos},
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, io::Read, path::Path};
type Result<T> = std::result::Result<T, Refusal>;

pub(super) struct Prepared {
    owner: Owner,
    pub observation: Observation,
}
pub(super) enum Observation {
    Fresh,
    UntouchedGenesis {
        cluster: ClusterId,
    },
    Recovered {
        floor: u64,
        sample: u64,
        applied: Option<LogPos>,
        membership: Option<(LogPos, MembershipBlob)>,
        clock: HostClock,
    },
}
/// `expected` and root authentication come from a separately trusted caller.
/// The root's fingerprint is in expected, never trusted from the record alone.
pub(super) fn prepare(
    plan: Plan,
    expected: Option<&ExpectedIdentity>,
    fresh_cluster: Option<ClusterId>,
    require_historical_genesis: bool,
) -> Result<Prepared> {
    if plan.mode.recovery() && require_historical_genesis {
        return Err(Refusal::Provenance);
    }
    if !plan.mode.recovery() && expected.is_some() {
        return Err(Refusal::Identity);
    }
    let owner = Owner::acquire(plan)?;
    if !owner.plan.mode.recovery() {
        if owner.has_stores() {
            let _locks = store_locks(&owner)?;
            let cluster = fresh_cluster.ok_or(Refusal::Provenance)?;
            let StateInspection {
                snapshot,
                applied,
                membership,
            } = inspect_state(&owner.plan.state, cluster)?;
            let log = inspect_log(&owner.plan.state, cluster)?;
            let state = snapshot_state(&snapshot).map_err(|_| Refusal::Store)?;
            let untouched = crate::cluster::enrollment::EnrollmentState::new(
                cluster,
                std::num::NonZeroU64::new(2).unwrap(),
            );
            if applied.is_some()
                || membership.is_some()
                || log.last.is_some()
                || log.has_vote
                || state != untouched
            {
                return Err(Refusal::Provenance);
            }
            owner.recheck()?;
            return Ok(Prepared {
                owner,
                observation: Observation::UntouchedGenesis { cluster },
            });
        }
        return Ok(Prepared {
            owner,
            observation: Observation::Fresh,
        });
    }
    let expected = expected.ok_or(Refusal::Identity)?;
    if owner.plan.mode.founder() != expected.origin.founder() {
        return Err(Refusal::Identity);
    }
    let record = PublicIdentity::decode(&owner.read("identity.json", 8192)?)
        .map_err(|_| Refusal::Identity)?;
    let root = owner.read("ca.der", 4096)?;
    if owner.read(record.artifact(), 65536)?.is_empty() {
        return Err(Refusal::Identity);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Refusal::Clock)?;
    record
        .verify(
            expected,
            &root,
            rustls::pki_types::UnixTime::since_unix_epoch(now),
        )
        .map_err(|_| Refusal::Identity)?;
    owner.recheck()?;
    // Existing SQL locks are held throughout immutable inspection. The outer
    // owner stays held across their release and the current-store openers.
    let locks = store_locks(&owner)?;
    let StateInspection {
        snapshot,
        applied,
        membership,
    } = inspect_state(&owner.plan.state, expected.cluster)?;
    let log = inspect_log(&owner.plan.state, expected.cluster)?;
    validate_preflight(applied, &log).map_err(|_| Refusal::Store)?;
    if applied.is_none_or(|p| {
        p.index < expected.origin.position().index || p < expected.origin.position()
    }) {
        return Err(Refusal::Identity);
    }
    let inspected = snapshot_state(&snapshot).map_err(|_| Refusal::Store)?;
    inspected
        .authorize_peer(expected.cluster, expected.node, expected.key)
        .map_err(|_| Refusal::Identity)?;
    HostClock::new(inspected.committed_ms())
        .sample_ms()
        .map_err(|_| Refusal::Clock)?;
    owner.recheck()?;
    drop(locks);
    #[cfg(test)]
    tests::mark_mutable_open();
    let sm = StateStore::open(
        &owner.plan.state,
        Init {
            cluster: expected.cluster,
            first_free: std::num::NonZeroU64::new(2).unwrap(),
        },
    )
    .map_err(|_| Refusal::Store)?;
    if sm.snapshot() != snapshot || sm.membership() != membership.as_ref() {
        return Err(Refusal::Store);
    }
    let mut log =
        VoteLogStore::open(&owner.plan.state, expected.cluster).map_err(|_| Refusal::Store)?;
    preflight(&mut log, &sm).map_err(|_| Refusal::Store)?;
    sm.state()
        .authorize_peer(expected.cluster, expected.node, expected.key)
        .map_err(|_| Refusal::Identity)?;
    let floor = sm.state().committed_ms();
    let clock = HostClock::new(floor);
    let sample = clock.sample_ms().map_err(|_| Refusal::Clock)?;
    drop(log);
    drop(sm);
    owner.recheck()?;
    Ok(Prepared {
        owner,
        observation: Observation::Recovered {
            floor,
            sample,
            applied,
            membership,
            clock,
        },
    })
}
fn store_locks(owner: &Owner) -> Result<Vec<std::fs::File>> {
    let mut locks = Vec::new();
    for stem in ["cluster-state", "cluster-log"] {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(owner.plan.state.join(format!("{stem}.lock")))
            .map_err(|_| Refusal::Ownership)?;
        f.try_lock().map_err(|_| Refusal::Ownership)?;
        locks.push(f);
    }
    owner.recheck()?;
    Ok(locks)
}
fn readonly(dir: &Path, stem: &str, max_pages: u64, schema: &[(&str, &str)]) -> Result<Connection> {
    let path = dir.join(format!("{stem}.sqlite3"));
    let len = std::fs::metadata(&path).map_err(|_| Refusal::Store)?.len();
    if len < 4096 || len > max_pages * 4096 {
        return Err(Refusal::Store);
    }
    let mut header = [0u8; 100];
    std::fs::File::open(&path)
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|_| Refusal::Store)?;
    // Current stores use rollback journals. Never let immutable mode conceal
    // WAL-mode data or accept trailing pages outside the declared image.
    if &header[..16] != b"SQLite format 3\0" || header[18] != 1 || header[19] != 1 {
        return Err(Refusal::Store);
    }
    let text = path
        .to_str()
        .ok_or(Refusal::Unavailable)?
        .replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let mut uri = String::from("file:");
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/:_- .".contains(&b) && b != b' ' {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    let db = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| Refusal::Store)?;
    let defensive = db
        .set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(|_| Refusal::Store)?;
    let trusted = db
        .set_db_config(
            rusqlite::config::DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA,
            false,
        )
        .map_err(|_| Refusal::Store)?;
    if !defensive || trusted {
        return Err(Refusal::Store);
    }
    #[cfg(test)]
    tests::observe_schema_expression(&db);
    for (key, exact, max) in [
        ("user_version", Some(2), 2),
        ("page_size", Some(4096), 4096),
        ("page_count", None, max_pages),
    ] {
        let n: u64 = db
            .pragma_query_value(None, key, |r| r.get(0))
            .map_err(|_| Refusal::Store)?;
        if n == 0 || n > max || exact.is_some_and(|e| n != e) {
            return Err(Refusal::Store);
        }
        if key == "page_count" && n * 4096 != len {
            return Err(Refusal::Store);
        }
    }
    db.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA query_only=ON;")
        .map_err(|_| Refusal::Store)?;
    let mut stmt = db
        .prepare("SELECT type,substr(name,1,65),substr(sql,1,513) FROM sqlite_schema ORDER BY name LIMIT 4")
        .map_err(|_| Refusal::Store)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| Refusal::Store)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Refusal::Store)?;
    if rows.len() != schema.len()
        || rows
            .iter()
            .zip(schema)
            .any(|((t, n, s), (name, sql))| t != "table" || n != name || s != sql)
    {
        return Err(Refusal::Store);
    }
    drop(stmt);
    // Integrity traversal evaluates CHECK expressions. Only the exact bounded
    // current schemas are permitted to reach it, even with trusted_schema off.
    #[cfg(test)]
    tests::mark_integrity();
    let integrity: String = db
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .map_err(|_| Refusal::Store)?;
    if integrity != "ok" {
        return Err(Refusal::Store);
    }
    Ok(db)
}
struct StateInspection {
    snapshot: Vec<u8>,
    applied: Option<LogPos>,
    membership: Option<(LogPos, MembershipBlob)>,
}
fn inspect_state(dir: &Path, cluster: ClusterId) -> Result<StateInspection> {
    let db = readonly(
        dir,
        "cluster-state",
        64,
        &[(
            "sm",
            "CREATE TABLE sm(singleton INTEGER PRIMARY KEY CHECK(singleton=1), snapshot BLOB NOT NULL CHECK(length(snapshot)<=16384))",
        )],
    )?;
    let mut stmt = db
        .prepare("SELECT singleton,snapshot FROM sm LIMIT 2")
        .map_err(|_| Refusal::Store)?;
    let mut rows = stmt.query([]).map_err(|_| Refusal::Store)?;
    let row = rows
        .next()
        .map_err(|_| Refusal::Store)?
        .ok_or(Refusal::Store)?;
    if row.get::<_, i64>(0).map_err(|_| Refusal::Store)? != 1 {
        return Err(Refusal::Store);
    }
    let raw = row
        .get_ref(1)
        .map_err(|_| Refusal::Store)?
        .as_blob()
        .map_err(|_| Refusal::Store)?;
    if raw.len() > 16384 {
        return Err(Refusal::Store);
    }
    let blob = raw.to_vec();
    let (applied, membership) =
        StateStore::inspect_snapshot(&blob, cluster).map_err(|_| Refusal::Store)?;
    if rows.next().map_err(|_| Refusal::Store)?.is_some() {
        return Err(Refusal::Store);
    }
    Ok(StateInspection {
        snapshot: blob,
        applied,
        membership,
    })
}
fn snapshot_state(
    blob: &[u8],
) -> std::result::Result<crate::cluster::enrollment::EnrollmentState, StoreError> {
    // Called only after inspect_snapshot has validated the complete canonical image.
    let mut r = Reader(&blob[..blob.len() - 32]);
    r.bytes(10)?;
    if r.flag()? {
        get_pos(&mut r)?;
    }
    if r.flag()? {
        get_pos(&mut r)?;
        let n = r.u16()? as usize;
        r.bytes(n)?;
    }
    codec::decode(&mut r)
}
struct LogInspection {
    purged: Option<LogPos>,
    last: Option<LogPos>,
    rows: Vec<LogPos>,
    has_vote: bool,
}
fn reader(b: &[u8], max: usize) -> std::result::Result<Reader<'_>, StoreError> {
    if !(32..=max).contains(&b.len()) {
        return Err(StoreError::Corrupt);
    }
    let (p, h) = b.split_at(b.len() - 32);
    if Sha256::digest(p).as_slice() != h {
        return Err(StoreError::Corrupt);
    }
    Ok(Reader(p))
}
fn position(r: &mut Reader<'_>) -> std::result::Result<LogPos, StoreError> {
    let p = get_pos(r)?;
    if p.index > i64::MAX as u64 {
        return Err(StoreError::Corrupt);
    }
    Ok(p)
}
fn inspect_log(dir: &Path, cluster: ClusterId) -> Result<LogInspection> {
    let db = readonly(
        dir,
        "cluster-log",
        2048,
        &[
            (
                "log",
                "CREATE TABLE log(idx INTEGER PRIMARY KEY CHECK(idx>=0), body BLOB NOT NULL CHECK(length(body)<=4163))",
            ),
            (
                "meta",
                "CREATE TABLE meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1), blob BLOB NOT NULL CHECK(length(blob)<=256))",
            ),
        ],
    )?;
    inspect_log_connection(&db, cluster).map_err(|_| Refusal::Store)
}
fn inspect_log_connection(
    db: &Connection,
    cluster: ClusterId,
) -> std::result::Result<LogInspection, StoreError> {
    let mut stmt = db.prepare("SELECT singleton,blob FROM meta LIMIT 2")?;
    let mut rows = stmt.query([])?;
    let row = rows.next()?.ok_or(StoreError::Corrupt)?;
    if row.get::<_, i64>(0)? != 1 {
        return Err(StoreError::Corrupt);
    }
    let raw = row.get_ref(1)?;
    let b = raw.as_blob().map_err(|_| StoreError::Corrupt)?;
    let mut r = reader(b, 256)?;
    if r.bytes(8)? != b"ARXLM1\0\0" || r.u16()? != 2 || r.array::<16>()? != cluster {
        return Err(StoreError::Corrupt);
    }
    let has_vote = r.flag()?;
    let mut canonical = b"ARXLM1\0\0".to_vec();
    canonical.extend(2u16.to_be_bytes());
    canonical.extend(cluster);
    canonical.push(u8::from(has_vote));
    if has_vote {
        canonical.extend(r.u64()?.to_be_bytes());
        canonical.extend(r.u64()?.to_be_bytes());
        canonical.push(u8::from(r.flag()?));
    }
    let purged = if r.flag()? {
        Some(position(&mut r)?)
    } else {
        None
    };
    let tail = if r.flag()? {
        let first = r.u64()?;
        let last = position(&mut r)?;
        if first > last.index
            || last.index - first >= MAX_LIVE as u64
            || purged.is_some_and(|p| p.index.checked_add(1) != Some(first) || last <= p)
        {
            return Err(StoreError::Corrupt);
        }
        Some((first, last))
    } else {
        None
    };
    canonical.push(u8::from(purged.is_some()));
    if let Some(p) = purged {
        put_pos(&mut canonical, p);
    }
    canonical.push(u8::from(tail.is_some()));
    if let Some((first, last)) = tail {
        canonical.extend(first.to_be_bytes());
        put_pos(&mut canonical, last);
    }
    if !r.0.is_empty() || canonical.as_slice() != &b[..b.len() - 32] || rows.next()?.is_some() {
        return Err(StoreError::Corrupt);
    }
    let mut stmt = db.prepare("SELECT idx,body FROM log ORDER BY idx LIMIT 513")?;
    let mut rows = stmt.query([])?;
    let mut positions = Vec::new();
    while let Some(row) = rows.next()? {
        if positions.len() == MAX_LIVE {
            return Err(StoreError::Corrupt);
        }
        let index: i64 = row.get(0)?;
        let raw = row.get_ref(1)?;
        let b = raw.as_blob().map_err(|_| StoreError::Corrupt)?;
        let mut r = reader(b, 4163)?;
        if r.bytes(8)? != b"ARXLE2\0\0" {
            return Err(StoreError::Corrupt);
        }
        let p = position(&mut r)?;
        if index < 0
            || index as u64 != p.index
            || positions.last().is_some_and(|prev: &LogPos| {
                prev.index.checked_add(1) != Some(p.index) || *prev >= p
            })
        {
            return Err(StoreError::Corrupt);
        }
        let entry = match r.bytes(1)?[0] {
            0 => Entry::Blank,
            t @ (1 | 2 | 4 | 5 | 6) => Entry::Enrollment(codec::decode_command(t, &mut r)?),
            3 => {
                let n = r.u16()? as usize;
                let m = MembershipBlob::new(r.bytes(n)?.to_vec())?;
                crate::cluster::membership::Membership::decode(m.bytes())
                    .map_err(|_| StoreError::Membership)?;
                Entry::Membership(m)
            }
            _ => return Err(StoreError::Corrupt),
        };
        let mut canonical = b"ARXLE2\0\0".to_vec();
        put_pos(&mut canonical, p);
        match entry {
            Entry::Blank => canonical.push(0),
            Entry::Enrollment(c) => codec::encode_command(&c, &mut canonical),
            Entry::Membership(m) => {
                canonical.push(3);
                canonical.extend((m.bytes().len() as u16).to_be_bytes());
                canonical.extend(m.bytes());
            }
        }
        if !r.0.is_empty() || canonical.as_slice() != &b[..b.len() - 32] {
            return Err(StoreError::Corrupt);
        }
        positions.push(p);
    }
    match (tail, positions.first(), positions.last()) {
        (None, None, None) => {}
        (Some((f, l)), Some(a), Some(b))
            if a.index == f
                && *b == l
                && positions.len() == (l.index - f + 1) as usize
                && purged.is_none_or(|p| *a > p) => {}
        _ => return Err(StoreError::Corrupt),
    }
    Ok(LogInspection {
        purged,
        last: tail.map(|(_, l)| l).or(purged),
        rows: positions,
        has_vote,
    })
}
fn validate_preflight(
    applied: Option<LogPos>,
    log: &LogInspection,
) -> std::result::Result<(), StoreError> {
    if applied.is_some_and(|a| a.index > i64::MAX as u64) {
        return Err(StoreError::Corrupt);
    }
    let valid = match applied {
        Some(a) => match log.purged {
            Some(p) if p.index > a.index => false,
            Some(p) if p.index == a.index => p == a,
            _ if log.last.is_none_or(|l| l.index < a.index) => log.last < Some(a),
            _ => log.rows.contains(&a),
        },
        None => log.purged.is_none(),
    };
    valid.then_some(()).ok_or(StoreError::Corrupt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{
        enrollment::{Command, FoundMembers, NodeId},
        join_receipt::{Origin, tests::fixture},
        startup_plan::tests::Layout,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    thread_local! {
        static SCHEMA_PROBE: std::cell::RefCell<Option<Arc<AtomicUsize>>> = const { std::cell::RefCell::new(None) };
        static MUTABLE_OPENS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        static PROBE_CONNECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        static INTEGRITY_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    pub(super) fn mark_mutable_open() {
        MUTABLE_OPENS.with(|n| n.set(n.get() + 1));
    }
    pub(super) fn mark_integrity() {
        INTEGRITY_CALLS.with(|n| n.set(n.get() + 1));
    }
    // Test-only harmless CHECK evaluation counter, deliberately INNOCUOUS so
    // trusted_schema=OFF alone cannot masquerade as schema-before-traversal.
    fn register_schema_probe(db: &Connection, count: Arc<AtomicUsize>) {
        unsafe extern "C" fn evaluate(
            context: *mut rusqlite::ffi::sqlite3_context,
            _: i32,
            _: *mut *mut rusqlite::ffi::sqlite3_value,
        ) {
            let count =
                unsafe { &*rusqlite::ffi::sqlite3_user_data(context).cast::<AtomicUsize>() };
            count.fetch_add(1, Ordering::SeqCst);
            unsafe {
                rusqlite::ffi::sqlite3_result_int(context, 1);
            }
        }
        unsafe extern "C" fn release(data: *mut std::ffi::c_void) {
            drop(unsafe { Arc::from_raw(data.cast::<AtomicUsize>()) });
        }
        let code = unsafe {
            rusqlite::ffi::sqlite3_create_function_v2(
                db.handle(),
                c"arx_schema_probe".as_ptr(),
                1,
                rusqlite::ffi::SQLITE_UTF8 | rusqlite::ffi::SQLITE_INNOCUOUS,
                Arc::into_raw(count).cast_mut().cast(),
                Some(evaluate),
                None,
                None,
                Some(release),
            )
        };
        assert_eq!(code, rusqlite::ffi::SQLITE_OK);
    }
    pub(super) fn observe_schema_expression(db: &Connection) {
        SCHEMA_PROBE.with(|slot| {
            if let Some(count) = slot.borrow().as_ref() {
                PROBE_CONNECTIONS.with(|n| n.set(n.get() + 1));
                register_schema_probe(db, count.clone());
            }
        });
    }
    struct ResetProbe;
    impl Drop for ResetProbe {
        fn drop(&mut self) {
            SCHEMA_PROBE.with(|slot| *slot.borrow_mut() = None);
        }
    }
    fn init() -> Init {
        Init {
            cluster: [1; 16],
            first_free: NodeId::new(2).unwrap(),
        }
    }
    fn initialized(l: &Layout) {
        l.write("state/cluster-owner.lock", b"");
        drop(StateStore::open(&l.root.path().join("state"), init()).unwrap());
        drop(VoteLogStore::open(&l.root.path().join("state"), [1; 16]).unwrap());
    }
    fn identity(l: &Layout) -> ExpectedIdentity {
        let (bytes, root, expected) = fixture(
            1,
            Origin::Found {
                term: 1,
                leader: 1,
                index: 0,
            },
        );
        l.write("credentials/identity.json", &bytes);
        l.write("credentials/ca.der", &root);
        l.write("credentials/sealed-key", b"opaque-synthetic-seal");
        expected
    }
    fn found(l: &Layout, e: &ExpectedIdentity, time: u64) {
        let mut sm = StateStore::open(&l.root.path().join("state"), init()).unwrap();
        let entry = Entry::Enrollment(Command::Found {
            cluster: [1; 16],
            members: FoundMembers::new([(e.node, e.key)]).unwrap(),
            proposed_at_ms: time,
        });
        let p = e.origin.position();
        sm.apply(p, &entry).unwrap();
        drop(sm);
        let mut log = VoteLogStore::open(&l.root.path().join("state"), [1; 16]).unwrap();
        log.append(&[(p, entry)]).unwrap();
    }
    #[test]
    fn joiner_recovery_uses_redeem_and_excludes_own_anchor() {
        use crate::cluster::enrollment::{JoinCode, KeyDigest};
        use crate::cluster::membership::Membership;
        let l = Layout::new();
        initialized(&l);
        let founder = KeyDigest::of_spki(b"synthetic-founder").unwrap();
        let (record, root, expected) = fixture(
            2,
            Origin::Redeem {
                term: 1,
                leader: 1,
                index: 3,
                anchors: vec![(1, *founder.as_bytes())],
            },
        );
        l.write("credentials/identity.json", &record);
        l.write("credentials/ca.der", &root);
        l.write("credentials/sealed-key", b"opaque");
        let dir = l.root.path().join("state");
        let mut sm = StateStore::open(&dir, init()).unwrap();
        let m = Membership::new(
            vec![[1].into_iter().collect()],
            [(1, "127.0.0.1:7001".into())].into_iter().collect(),
        )
        .unwrap();
        let p = |index| LogPos {
            term: 1,
            leader: 1,
            index,
        };
        let mut entries = vec![
            (
                p(0),
                Entry::Enrollment(Command::Found {
                    cluster: [1; 16],
                    members: FoundMembers::new([(NodeId::new(1).unwrap(), founder)]).unwrap(),
                    proposed_at_ms: 10,
                }),
            ),
            (
                p(1),
                Entry::Membership(MembershipBlob::new(m.encode().unwrap()).unwrap()),
            ),
        ];
        for (p, e) in &entries {
            sm.apply(*p, e).unwrap();
        }
        let anchor = sm.state().voter_anchor(&m).unwrap();
        let code = JoinCode::generate([1; 16], expected.ca, anchor, |b: &mut [u8]| {
            b.fill(8);
            Ok::<_, ()>(())
        })
        .unwrap();
        entries.push((
            p(2),
            Entry::Enrollment(Command::Mint {
                cluster: [1; 16],
                actor: [4; 16],
                request: [5; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: 11,
            }),
        ));
        entries.push((
            p(3),
            Entry::Enrollment(Command::redeem(
                [1; 16],
                [6; 16],
                code.code_id(),
                &code.proof(),
                expected.key,
                anchor,
                12,
            )),
        ));
        for (p, e) in &entries[2..] {
            sm.apply(*p, e).unwrap();
        }
        assert_eq!(sm.state().next_node_id().get(), 3);
        drop(sm);
        let mut log = VoteLogStore::open(&dir, [1; 16]).unwrap();
        log.append(&entries).unwrap();
        drop(log);
        let before = l.snapshot();
        let prepared = prepare(l.plan("recover-joiner"), Some(&expected), None, false).unwrap();
        match &prepared.observation {
            Observation::Recovered {
                floor: 12,
                membership: Some((position, blob)),
                ..
            } => {
                assert_eq!(*position, p(1));
                assert_eq!(blob.bytes(), m.encode().unwrap());
            }
            _ => panic!("exact recovered membership required"),
        }
        drop(prepared);
        assert_eq!(l.snapshot(), before);
        assert!(matches!(
            prepare(l.plan("recover-founder"), Some(&expected), None, false),
            Err(Refusal::Identity)
        ));
        assert_eq!(l.snapshot(), before);
        assert!(matches!(
            prepare(l.plan("recover-joiner"), Some(&expected), None, true),
            Err(Refusal::Provenance)
        ));
    }
    #[test]
    fn recovery_accepts_wall_behind_floor_within_skew() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let floor = wall + crate::cluster::enrollment::SKEW_MS;
        found(&l, &expected, floor);
        let prepared = prepare(l.plan("recover-founder"), Some(&expected), None, false).unwrap();
        if let Observation::Recovered {
            floor: f, sample, ..
        } = prepared.observation
        {
            assert_eq!(f, floor);
            assert!(sample + 30000 >= floor);
        } else {
            panic!("recovery required");
        }
    }
    #[test]
    fn fresh_and_verified_untouched_genesis() {
        let l = Layout::new();
        let p = prepare(l.plan("create-founder"), None, None, false).unwrap();
        assert!(matches!(p.observation, Observation::Fresh));
        drop(p);
        let l = Layout::new();
        initialized(&l);
        let before = l.snapshot();
        let p = prepare(l.plan("join"), None, Some([1; 16]), false).unwrap();
        assert!(
            matches!(p.observation, Observation::UntouchedGenesis {cluster} if cluster==[1;16])
        );
        drop(p);
        assert_eq!(l.snapshot(), before);
        let l = Layout::new();
        l.write("state/cluster-owner.lock", b"");
        drop(
            StateStore::open(
                &l.root.path().join("state"),
                Init {
                    first_free: NodeId::new(3).unwrap(),
                    ..init()
                },
            )
            .unwrap(),
        );
        drop(VoteLogStore::open(&l.root.path().join("state"), [1; 16]).unwrap());
        let before = l.snapshot();
        assert!(matches!(
            prepare(l.plan("join"), None, Some([1; 16]), false),
            Err(Refusal::Provenance)
        ));
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn actual_floor_current_binding_reopen_and_owner_drop() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 123456);
        let before = l.snapshot();
        for _ in 0..2 {
            let p = prepare(l.plan("recover-founder"), Some(&expected), None, false).unwrap();
            match &p.observation {
                Observation::Recovered {
                    floor,
                    sample,
                    applied,
                    membership,
                    clock,
                } => {
                    assert_eq!(*floor, 123456);
                    assert!(*sample >= *floor);
                    assert_eq!(*applied, Some(expected.origin.position()));
                    assert!(membership.is_none());
                    assert!(clock.sample_ms().is_ok());
                }
                _ => panic!("expected recovered"),
            }
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .open(l.root.path().join("state/cluster-owner.lock"))
                .unwrap();
            assert!(f.try_lock().is_err());
            drop(p);
            assert!(f.try_lock().is_ok());
            drop(f);
        }
        assert_eq!(l.snapshot(), before);
        assert!(matches!(
            prepare(l.plan("recover-founder"), Some(&expected), None, true),
            Err(Refusal::Provenance)
        ));
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn existing_invalid_pairs_refuse_unchanged() {
        for variant in [
            "zero",
            "sqlite-zero",
            "empty-schema",
            "partial-schema",
            "extra-schema",
            "image-type",
            "row-type",
            "one-sided",
            "unsupported",
            "image",
            "metadata",
            "row",
            "wal",
            "journal",
            "shm",
            "no-owner",
            "no-state-lock",
        ] {
            let l = Layout::new();
            initialized(&l);
            let expected = identity(&l);
            found(&l, &expected, 1);
            let state = l.root.path().join("state/cluster-state.sqlite3");
            let log = l.root.path().join("state/cluster-log.sqlite3");
            match variant {
                "zero" => l.write("state/cluster-state.sqlite3", b""),
                "sqlite-zero" => {
                    std::fs::remove_file(&state).unwrap();
                    let db = Connection::open(&state).unwrap();
                    db.execute_batch("CREATE TABLE unrelated(x); PRAGMA user_version=0;")
                        .unwrap();
                }
                "one-sided" => std::fs::remove_file(&log).unwrap(),
                "empty-schema" => {
                    std::fs::remove_file(&state).unwrap();
                    Connection::open(&state)
                        .unwrap()
                        .execute_batch("VACUUM;")
                        .unwrap();
                }
                "partial-schema" => Connection::open(&state)
                    .unwrap()
                    .execute_batch("DROP TABLE sm;")
                    .unwrap(),
                "extra-schema" => Connection::open(&state)
                    .unwrap()
                    .execute_batch("CREATE TABLE extra(x);")
                    .unwrap(),
                "image-type" => {
                    Connection::open(&state)
                        .unwrap()
                        .execute("UPDATE sm SET snapshot='text'", [])
                        .unwrap();
                }
                "row-type" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute("UPDATE log SET body='text'", [])
                        .unwrap();
                }
                "unsupported" => Connection::open(&state)
                    .unwrap()
                    .execute_batch("PRAGMA user_version=9")
                    .unwrap(),
                "image" => {
                    Connection::open(&state)
                        .unwrap()
                        .execute("UPDATE sm SET snapshot=zeroblob(40)", [])
                        .unwrap();
                }
                "metadata" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute("UPDATE meta SET blob=zeroblob(40)", [])
                        .unwrap();
                }
                "row" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute("UPDATE log SET body=zeroblob(40)", [])
                        .unwrap();
                }
                "wal" => l.write("state/cluster-state.sqlite3-wal", b"pending"),
                "journal" => l.write("state/cluster-state.sqlite3-journal", b"pending"),
                "shm" => l.write("state/cluster-state.sqlite3-shm", b""),
                "no-owner" => {
                    std::fs::remove_file(l.root.path().join("state/cluster-owner.lock")).unwrap()
                }
                _ => std::fs::remove_file(l.root.path().join("state/cluster-state.lock")).unwrap(),
            }
            let before = l.snapshot();
            assert!(
                prepare(l.plan("recover-founder"), Some(&expected), None, false).is_err(),
                "{variant}"
            );
            assert_eq!(l.snapshot(), before, "{variant}");
        }
    }
    #[test]
    fn current_store_log_conformance_and_independent_corruption() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 17);
        let dir = l.root.path().join("state");
        assert_eq!(
            inspect_log(&dir, [1; 16]).unwrap().rows,
            vec![expected.origin.position()]
        );
        let db = Connection::open(dir.join("cluster-log.sqlite3")).unwrap();
        let original: Vec<u8> = db
            .query_row("SELECT blob FROM meta", [], |r| r.get(0))
            .unwrap();
        for offset in [0, 8, 26] {
            let mut bad = original.clone();
            bad[offset] ^= 1;
            let n = bad.len() - 32;
            let h = Sha256::digest(&bad[..n]);
            bad[n..].copy_from_slice(&h);
            db.execute("UPDATE meta SET blob=?1", [bad]).unwrap();
            assert!(inspect_log(&dir, [1; 16]).is_err());
        }
        db.execute("UPDATE meta SET blob=?1", [original]).unwrap();
        assert!(inspect_log(&dir, [1; 16]).is_ok());
        db.execute("UPDATE log SET idx=2", []).unwrap();
        assert!(inspect_log(&dir, [1; 16]).is_err());
    }
    #[test]
    fn missing_artifact_wrong_binding_and_behind_floor_unchanged() {
        for variant in ["missing", "binding", "floor"] {
            let l = Layout::new();
            initialized(&l);
            let mut expected = identity(&l);
            let wall = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            found(
                &l,
                &expected,
                if variant == "floor" { wall + 60000 } else { 1 },
            );
            if variant == "missing" {
                std::fs::remove_file(l.root.path().join("credentials/sealed-key")).unwrap();
            }
            if variant == "binding" {
                expected.key = crate::cluster::enrollment::KeyDigest::of_spki(b"foreign").unwrap();
            }
            let before = l.snapshot();
            assert!(prepare(l.plan("recover-founder"), Some(&expected), None, false).is_err());
            assert_eq!(l.snapshot(), before);
        }
    }
    #[test]
    fn unsupported_schema_refuses_without_evaluating_check() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 17);
        let count = Arc::new(AtomicUsize::new(0));
        let db = Connection::open(l.root.path().join("state/cluster-state.sqlite3")).unwrap();
        register_schema_probe(&db, count.clone());
        db.execute_batch(
            "CREATE TABLE extra(x CHECK(arx_schema_probe(x))); INSERT INTO extra VALUES(1);",
        )
        .unwrap();
        count.store(0, Ordering::SeqCst);
        let checked: String = db
            .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(checked, "ok");
        assert!(
            count.load(Ordering::SeqCst) > 0,
            "fixture CHECK must be observable"
        );
        drop(db);
        count.store(0, Ordering::SeqCst);
        let before = l.snapshot();
        SCHEMA_PROBE.with(|slot| *slot.borrow_mut() = Some(count.clone()));
        let _reset = ResetProbe;
        let owner = Owner::acquire(l.plan("recover-founder")).unwrap();
        let _locks = store_locks(&owner).unwrap();
        PROBE_CONNECTIONS.with(|n| n.set(0));
        INTEGRITY_CALLS.with(|n| n.set(0));
        let result = inspect_state(&owner.plan.state, [1; 16]);
        assert!(matches!(result, Err(Refusal::Store)));
        assert_eq!(
            PROBE_CONNECTIONS.with(|n| n.get()),
            1,
            "must reach read-only SQLite inspection"
        );
        assert_eq!(
            INTEGRITY_CALLS.with(|n| n.get()),
            0,
            "unsupported schema reached integrity traversal"
        );
        assert_eq!(l.snapshot(), before);
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "unsupported CHECK executed before schema rejection"
        );
    }
    #[test]
    fn oversized_applied_index_refuses_before_mutable_open() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 17);
        let db = Connection::open(l.root.path().join("state/cluster-state.sqlite3")).unwrap();
        let mut blob: Vec<u8> = db
            .query_row("SELECT snapshot FROM sm", [], |r| r.get(0))
            .unwrap();
        assert_eq!(blob[10], 1);
        blob[27..35].copy_from_slice(&(i64::MAX as u64 + 1).to_be_bytes());
        let n = blob.len() - 32;
        let hash = Sha256::digest(&blob[..n]);
        blob[n..].copy_from_slice(&hash);
        assert_eq!(
            StateStore::inspect_snapshot(&blob, [1; 16])
                .unwrap()
                .0
                .unwrap()
                .index,
            i64::MAX as u64 + 1
        );
        db.execute("UPDATE sm SET snapshot=?1", [blob]).unwrap();
        drop(db);
        let before = l.snapshot();
        MUTABLE_OPENS.with(|n| n.set(0));
        assert!(matches!(
            prepare(l.plan("recover-founder"), Some(&expected), None, false),
            Err(Refusal::Store)
        ));
        assert_eq!(l.snapshot(), before);
        assert_eq!(
            MUTABLE_OPENS.with(|n| n.get()),
            0,
            "invalid applied index reached mutable store opener"
        );
    }
}

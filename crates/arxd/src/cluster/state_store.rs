//! Local state-machine persistence probe. No quorum, transport or credential ingress.
use super::enrollment::{
    ClusterId, Command, EnrollmentState, NodeId, Outcome,
    codec::{self, Reader},
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
const LIMIT: usize = 16384;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogPos {
    pub term: u64,
    pub leader: u64,
    pub index: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MembershipBlob(Vec<u8>);
impl MembershipBlob {
    pub fn new(bytes: Vec<u8>) -> Result<Self, StoreError> {
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(StoreError::Corrupt);
        }
        Ok(Self(bytes))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Blank,
    Enrollment(Command),
    Membership(MembershipBlob),
}
#[derive(Clone, Copy, Debug)]
pub struct Init {
    pub cluster: ClusterId,
    pub first_free: NodeId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    Io,
    Sql,
    Corrupt,
    Unsupported,
    WrongCluster,
    OutOfOrder,
    Poisoned,
    TooLarge,
    StaleSnapshot,
    Full,
    Conflict,
    VoteRegressed,
    Membership,
}
impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Io => "private state unavailable",
            Self::Sql => "state storage failed",
            Self::Corrupt => "corrupt state",
            Self::Unsupported => "unsupported state version",
            Self::WrongCluster => "wrong cluster",
            Self::OutOfOrder => "out of order",
            Self::Poisoned => "state store poisoned",
            Self::TooLarge => "snapshot too large",
            Self::StaleSnapshot => "stale snapshot",
            Self::Full => "log full",
            Self::Conflict => "log conflict",
            Self::VoteRegressed => "vote regressed",
            Self::Membership => "membership out of bounds",
        })
    }
}
impl std::error::Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Sql
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Image {
    applied: Option<LogPos>,
    membership: Option<(LogPos, MembershipBlob)>,
    state: EnrollmentState,
}
pub(super) fn put_pos(b: &mut Vec<u8>, p: LogPos) {
    b.extend(p.term.to_be_bytes());
    b.extend(p.leader.to_be_bytes());
    b.extend(p.index.to_be_bytes());
}
pub(super) fn get_pos(r: &mut Reader<'_>) -> Result<LogPos, StoreError> {
    Ok(LogPos {
        term: r.u64()?,
        leader: r.u64()?,
        index: r.u64()?,
    })
}
fn encode(i: &Image) -> Vec<u8> {
    let mut b = b"ARXSM1\0\0".to_vec();
    b.extend(3u16.to_be_bytes());
    b.push(u8::from(i.applied.is_some()));
    if let Some(p) = i.applied {
        put_pos(&mut b, p);
    }
    b.push(u8::from(i.membership.is_some()));
    if let Some((p, m)) = &i.membership {
        put_pos(&mut b, *p);
        b.extend((m.0.len() as u16).to_be_bytes());
        b.extend(&m.0);
    }
    codec::encode(&i.state, &mut b);
    let hash = Sha256::digest(&b);
    b.extend(hash);
    b
}
fn decode(b: &[u8]) -> Result<Image, StoreError> {
    if b.len() > LIMIT || b.len() < 32 {
        return Err(StoreError::Corrupt);
    }
    let (payload, hash) = b.split_at(b.len() - 32);
    if Sha256::digest(payload).as_slice() != hash {
        return Err(StoreError::Corrupt);
    }
    let mut r = Reader(payload);
    if r.bytes(8)? != b"ARXSM1\0\0" {
        return Err(StoreError::Corrupt);
    }
    match r.u16()? {
        3 => {}
        1 | 2 => return Err(StoreError::Unsupported),
        _ => return Err(StoreError::Corrupt),
    }
    let applied = if r.flag()? {
        Some(get_pos(&mut r)?)
    } else {
        None
    };
    let membership = if r.flag()? {
        let p = get_pos(&mut r)?;
        let len = r.u16()? as usize;
        let m = MembershipBlob::new(r.bytes(len)?.to_vec())?;
        super::membership::Membership::decode(m.bytes()).map_err(|_| StoreError::Membership)?;
        let a = applied.ok_or(StoreError::Corrupt)?;
        if p.index > a.index || p.term > a.term || (p.index == a.index && p != a) {
            return Err(StoreError::Corrupt);
        }
        Some((p, m))
    } else {
        None
    };
    let state = codec::decode(&mut r)?;
    codec::validate_origins(&state, applied)?;
    if !r.0.is_empty() {
        return Err(StoreError::Corrupt);
    }
    let i = Image {
        applied,
        membership,
        state,
    };
    if encode(&i) != b {
        return Err(StoreError::Corrupt);
    }
    Ok(i)
}
/// Observes completion of preceding field destructors, not SQLite/OS close success.
#[derive(Clone)]
pub(super) struct Released(Arc<AtomicBool>);
impl Released {
    pub(super) fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
pub(super) struct ReleaseAck(Arc<AtomicBool>);
impl ReleaseAck {
    pub(super) fn released(&self) -> Released {
        Released(self.0.clone())
    }
}
impl Drop for ReleaseAck {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub(super) fn release_pair() -> (ReleaseAck, Released) {
    let flag = Arc::new(AtomicBool::new(false));
    (ReleaseAck(flag.clone()), Released(flag))
}
#[cfg(test)]
pub(in crate::cluster) struct DropGate {
    pub(in crate::cluster) entered: std::sync::mpsc::Sender<()>,
    pub(in crate::cluster) release: std::sync::mpsc::Receiver<()>,
}
#[cfg(test)]
impl Drop for DropGate {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let _ = self.release.recv();
    }
}
pub struct StateStore {
    db: Connection,
    image: Image,
    blob: Vec<u8>,
    poisoned: bool,
    #[cfg(test)]
    gate: Option<DropGate>,
    _lock: Option<File>,
    #[cfg(test)]
    hook: Option<(std::path::PathBuf, bool)>,
    _bound: Option<super::bound_open::BoundLease>,
    // Must stay last: struct fields drop in declaration order.
    release: ReleaseAck,
}
impl std::fmt::Debug for StateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateStore")
            .field("image", &self.image)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}
fn private(path: &Path) -> Result<(), StoreError> {
    crate::config::checked_path(path).map_err(|_| StoreError::Io)?;
    crate::security::private(path).map_err(|_| StoreError::Io)
}
fn options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}
pub(super) fn open_db(
    dir: &Path,
    stem: &str,
    max_pages: u64,
) -> Result<(File, Connection), StoreError> {
    if !matches!(stem, "cluster-state" | "cluster-log") {
        return Err(StoreError::Unsupported);
    }
    private(dir)?;
    for name in [
        &format!("{stem}.lock"),
        &format!("{stem}.sqlite3"),
        &format!("{stem}.sqlite3-journal"),
        &format!("{stem}.sqlite3-wal"),
        &format!("{stem}.sqlite3-shm"),
    ] {
        let path = dir.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(m) => {
                private(&path)?;
                if !m.is_file() {
                    return Err(StoreError::Io);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(StoreError::Io),
        }
    }
    let lock_path = dir.join(format!("{stem}.lock"));
    let lock = options().open(&lock_path).map_err(|_| StoreError::Io)?;
    lock.try_lock().map_err(|_| StoreError::Io)?;
    private(&lock_path)?;
    let path = dir.join(format!("{stem}.sqlite3"));
    let file = options().open(&path).map_err(|_| StoreError::Io)?;
    private(&path)?;
    drop(file);
    let db = Connection::open(&path)?;
    configure_db(&db, stem, max_pages)?;
    Ok((lock, db))
}
pub(super) fn configure_db(db: &Connection, stem: &str, max_pages: u64) -> Result<(), StoreError> {
    db.busy_timeout(Duration::from_millis(250))?;
    db.execute_batch(&format!("PRAGMA locking_mode=EXCLUSIVE; PRAGMA journal_mode=TRUNCATE; PRAGMA locking_mode=NORMAL; PRAGMA synchronous=FULL; PRAGMA fullfsync=ON; PRAGMA temp_store=MEMORY; PRAGMA max_page_count={max_pages};"))?;
    let mode: String = db.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    for (key, max, exact) in [
        ("synchronous", 2, true),
        ("busy_timeout", 250, true),
        ("fullfsync", 1, true),
        ("max_page_count", max_pages, true),
        ("page_size", 4096, stem == "cluster-log"),
        ("page_count", max_pages, false),
    ] {
        let v: u64 = db.pragma_query_value(None, key, |r| r.get(0))?;
        if (exact && v != max) || v > max {
            return Err(StoreError::Unsupported);
        }
    }
    if mode != "truncate" {
        return Err(StoreError::Unsupported);
    }
    Ok(())
}
type SnapshotPositions = (Option<LogPos>, Option<(LogPos, MembershipBlob)>);
pub(super) struct InitializedState {
    image: Image,
    blob: Vec<u8>,
    release: ReleaseAck,
}
impl StateStore {
    #[cfg(test)]
    pub(super) fn bound_pointer(&self) -> usize {
        (unsafe { self.db.handle() }) as usize
    }

    pub(super) fn released(&self) -> Released {
        self.release.released()
    }
    #[cfg(test)]
    pub(in crate::cluster) fn hold_drop(&mut self, gate: DropGate) {
        self.gate = Some(gate);
    }
    /// Pure bounded inspection before any consensus dispatch can mutate state.
    pub(super) fn inspect_snapshot(
        blob: &[u8],
        cluster: ClusterId,
    ) -> Result<SnapshotPositions, StoreError> {
        let image = decode(blob)?;
        if codec::cluster(&image.state) != cluster {
            return Err(StoreError::WrongCluster);
        }
        Ok((image.applied, image.membership))
    }
    pub(super) fn peek(blob: &[u8]) -> Result<SnapshotPositions, StoreError> {
        let image = decode(blob)?;
        Ok((image.applied, image.membership))
    }
    fn initialize_db(
        db: &mut Connection,
        dir: &Path,
        init: Init,
    ) -> Result<InitializedState, StoreError> {
        let tx = db.transaction()?;
        let v: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if v == 0 {
            let blob = encode(&Image {
                applied: None,
                membership: None,
                state: EnrollmentState::new(init.cluster, init.first_free),
            });
            tx.execute_batch("CREATE TABLE sm(singleton INTEGER PRIMARY KEY CHECK(singleton=1), snapshot BLOB NOT NULL CHECK(length(snapshot)<=16384)); PRAGMA user_version=2;")?;
            tx.execute("INSERT INTO sm VALUES(1,?1)", [&blob])?;
        } else if v != 2 {
            return Err(StoreError::Unsupported);
        }
        let blob:Vec<u8>=tx.query_row("SELECT snapshot FROM sm WHERE singleton=1 AND typeof(snapshot)='blob' AND length(snapshot)<=16384",[],|r|r.get(0))?;
        let rows: u64 =
            tx.query_row("SELECT count(*) FROM (SELECT 1 FROM sm LIMIT 2)", [], |r| {
                r.get(0)
            })?;
        if rows != 1 {
            return Err(StoreError::Corrupt);
        }
        let image = decode(&blob)?;
        if codec::cluster(&image.state) != init.cluster {
            return Err(StoreError::WrongCluster);
        }
        tx.commit()?;
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let p = dir.join(format!("cluster-state.sqlite3{suffix}"));
            if p.exists() {
                private(&p)?;
            }
        }
        Ok(InitializedState {
            image,
            blob,
            release: release_pair().0,
        })
    }
    pub(super) fn initialize_bound(
        pending: &mut super::bound_open::BoundDb,
        init: Init,
    ) -> Result<InitializedState, StoreError> {
        super::state_store::configure_db(&pending.db, "cluster-state", 64)?;
        #[cfg(test)]
        super::bound_open::tests::initialization(&pending.db, &pending.dir)?;
        Self::initialize_db(&mut pending.db, &pending.dir, init)
    }
    pub(super) fn from_initialized(
        bound: super::bound_open::BoundDb,
        initialized: InitializedState,
    ) -> Self {
        let super::bound_open::BoundDb { db, dir: _, lease } = bound;
        let InitializedState {
            image,
            blob,
            release,
        } = initialized;
        Self {
            db,
            image,
            blob,
            poisoned: false,
            #[cfg(test)]
            gate: None,
            _lock: None,
            #[cfg(test)]
            hook: None,
            _bound: Some(lease),
            release,
        }
    }
    pub fn open(dir: &Path, init: Init) -> Result<Self, StoreError> {
        let (lock, mut db) = open_db(dir, "cluster-state", 64)?;
        let initialized = Self::initialize_db(&mut db, dir, init)?;
        let InitializedState {
            image,
            blob,
            release,
        } = initialized;
        Ok(Self {
            db,
            image,
            blob,
            poisoned: false,
            #[cfg(test)]
            gate: None,
            _lock: Some(lock),
            #[cfg(test)]
            hook: None,
            _bound: None,
            release,
        })
    }
    pub fn applied(&self) -> Option<LogPos> {
        self.image.applied
    }
    pub fn membership(&self) -> Option<&(LogPos, MembershipBlob)> {
        self.image.membership.as_ref()
    }
    pub fn state(&self) -> &EnrollmentState {
        &self.image.state
    }
    pub fn snapshot(&self) -> Vec<u8> {
        self.blob.clone()
    }
    fn persist(&mut self, image: Image) -> Result<(), StoreError> {
        let blob = encode(&image);
        if blob.len() > LIMIT {
            return Err(StoreError::TooLarge);
        }
        let result = (|| -> Result<(), StoreError> {
            let tx = self.db.transaction()?;
            if tx.execute("UPDATE sm SET snapshot=?1 WHERE singleton=1", [&blob])? != 1 {
                return Err(StoreError::Sql);
            }
            #[cfg(test)]
            if let Some((path, false)) = &self.hook {
                tx.cache_flush()?;
                tests::pause(path);
            }
            tx.commit()?;
            #[cfg(test)]
            if let Some((path, true)) = &self.hook {
                tests::pause(path);
            }
            Ok(())
        })();
        if result.is_err() {
            self.poisoned = true;
            result?;
        }
        self.image = image;
        self.blob = blob;
        Ok(())
    }
    pub fn apply(&mut self, pos: LogPos, entry: &Entry) -> Result<Option<Outcome>, StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        if self.image.applied.map_or(pos.index != 0, |p| {
            p.index.checked_add(1) != Some(pos.index) || pos.term < p.term
        }) {
            return Err(StoreError::OutOfOrder);
        }
        let mut image = self.image.clone();
        image.applied = Some(pos);
        let outcome = match entry {
            Entry::Blank => None,
            Entry::Enrollment(c) => {
                let membership = image
                    .membership
                    .as_ref()
                    .map(|(_, m)| {
                        super::membership::Membership::decode(m.bytes())
                            .map_err(|_| StoreError::Membership)
                    })
                    .transpose()?;
                Some(image.state.apply(c, pos, membership.as_ref()))
            }
            Entry::Membership(m) => {
                super::membership::Membership::decode(m.bytes())
                    .map_err(|_| StoreError::Membership)?;
                image.membership = Some((pos, m.clone()));
                None
            }
        };
        self.persist(image)?;
        Ok(outcome)
    }
    pub fn install(&mut self, blob: &[u8]) -> Result<(), StoreError> {
        if self.poisoned {
            return Err(StoreError::Poisoned);
        }
        if blob.len() > LIMIT {
            return Err(StoreError::TooLarge);
        }
        let image = decode(blob)?;
        if codec::cluster(&image.state) != codec::cluster(&self.image.state) {
            return Err(StoreError::WrongCluster);
        }
        let p = image.applied.ok_or(StoreError::StaleSnapshot)?;
        if self
            .applied()
            .is_some_and(|a| p.index <= a.index || p.term < a.term)
        {
            return Err(StoreError::StaleSnapshot);
        }
        self.persist(image)
    }
}
#[cfg(test)]
pub(super) mod tests;

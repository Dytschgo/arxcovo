//! Bounded synchronous vote/log persistence. Checksums detect corruption, not forgery.
//! SQL failure has an unknown commit outcome: memory stays pre-state; reopen before reuse.
use super::{
    enrollment::{
        ClusterId,
        codec::{self, Reader},
    },
    state_store::{Entry, LogPos, MembershipBlob, StoreError, get_pos, open_db, put_pos},
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::{fs::File, path::Path};
pub const MAX_APPEND: usize = 64;
pub const MAX_LIVE: usize = 512;
const MAX_ROW: usize = 4163;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StoredVote {
    pub term: u64,
    pub node: u64,
    pub committed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogState {
    pub purged: Option<LogPos>,
    pub last: Option<LogPos>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Meta {
    cluster: ClusterId,
    vote: Option<StoredVote>,
    purged: Option<LogPos>,
    tail: Option<(u64, LogPos)>,
}
fn seal(mut b: Vec<u8>) -> Vec<u8> {
    let hash = Sha256::digest(&b);
    b.extend(hash);
    b
}
fn reader(b: &[u8], max: usize) -> Result<Reader<'_>, StoreError> {
    if b.len() < 32 || b.len() > max {
        return Err(StoreError::Corrupt);
    }
    let (p, h) = b.split_at(b.len() - 32);
    if Sha256::digest(p).as_slice() != h {
        return Err(StoreError::Corrupt);
    }
    Ok(Reader(p))
}
fn valid_pos(p: LogPos) -> Result<LogPos, StoreError> {
    if p.index > i64::MAX as u64 {
        Err(StoreError::Corrupt)
    } else {
        Ok(p)
    }
}
fn encode_row(p: LogPos, e: &Entry) -> Vec<u8> {
    let mut b = b"ARXLE1\0\0".to_vec();
    put_pos(&mut b, p);
    match e {
        Entry::Blank => b.push(0),
        Entry::Enrollment(c) => codec::encode_command(c, &mut b),
        Entry::Membership(m) => {
            b.push(3);
            b.extend((m.bytes().len() as u16).to_be_bytes());
            b.extend(m.bytes());
        }
    }
    seal(b)
}
fn decode_row(b: &[u8]) -> Result<(LogPos, Entry), StoreError> {
    let mut r = reader(b, MAX_ROW)?;
    if r.bytes(8)? != b"ARXLE1\0\0" {
        return Err(StoreError::Corrupt);
    }
    let p = valid_pos(get_pos(&mut r)?)?;
    let e = match r.bytes(1)?[0] {
        0 => Entry::Blank,
        t @ (1 | 2) => Entry::Enrollment(codec::decode_command(t, &mut r)?),
        3 => {
            let n = r.u16()? as usize;
            Entry::Membership(MembershipBlob::new(r.bytes(n)?.to_vec())?)
        }
        _ => return Err(StoreError::Corrupt),
    };
    if !r.0.is_empty() || encode_row(p, &e) != b {
        return Err(StoreError::Corrupt);
    }
    Ok((p, e))
}
fn encode_meta(m: &Meta) -> Vec<u8> {
    let mut b = b"ARXLM1\0\0".to_vec();
    b.extend(1u16.to_be_bytes());
    b.extend(m.cluster);
    b.push(u8::from(m.vote.is_some()));
    if let Some(v) = m.vote {
        b.extend(v.term.to_be_bytes());
        b.extend(v.node.to_be_bytes());
        b.push(u8::from(v.committed));
    }
    b.push(u8::from(m.purged.is_some()));
    if let Some(p) = m.purged {
        put_pos(&mut b, p);
    }
    b.push(u8::from(m.tail.is_some()));
    if let Some((first, last)) = m.tail {
        b.extend(first.to_be_bytes());
        put_pos(&mut b, last);
    }
    seal(b)
}
fn decode_meta(b: &[u8]) -> Result<Meta, StoreError> {
    let mut r = reader(b, 256)?;
    if r.bytes(8)? != b"ARXLM1\0\0" || r.u16()? != 1 {
        return Err(StoreError::Corrupt);
    }
    let cluster = r.array()?;
    let vote = if r.flag()? {
        Some(StoredVote {
            term: r.u64()?,
            node: r.u64()?,
            committed: r.flag()?,
        })
    } else {
        None
    };
    let purged = if r.flag()? {
        Some(valid_pos(get_pos(&mut r)?)?)
    } else {
        None
    };
    let tail = if r.flag()? {
        let first = r.u64()?;
        let last = valid_pos(get_pos(&mut r)?)?;
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
    let m = Meta {
        cluster,
        vote,
        purged,
        tail,
    };
    if !r.0.is_empty() || encode_meta(&m) != b {
        return Err(StoreError::Corrupt);
    }
    Ok(m)
}
pub struct VoteLogStore {
    db: Connection,
    meta: Meta,
    poisoned: bool,
    _lock: File,
    #[cfg(test)]
    hook: Option<(std::path::PathBuf, bool)>,
}
impl std::fmt::Debug for VoteLogStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoteLogStore")
            .field("meta", &self.meta)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}
impl VoteLogStore {
    pub fn open(dir: &Path, cluster: ClusterId) -> Result<Self, StoreError> {
        let (lock, mut db) = open_db(dir, "cluster-log", 2048)?;
        let tx = db.transaction()?;
        let version: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 {
            tx.execute_batch("CREATE TABLE meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1), blob BLOB NOT NULL CHECK(length(blob)<=256)); CREATE TABLE log(idx INTEGER PRIMARY KEY CHECK(idx>=0), body BLOB NOT NULL CHECK(length(body)<=4163)); PRAGMA user_version=1;")?;
            tx.execute(
                "INSERT INTO meta VALUES(1,?1)",
                [encode_meta(&Meta {
                    cluster,
                    vote: None,
                    purged: None,
                    tail: None,
                })],
            )?;
        } else if version != 1 {
            return Err(StoreError::Unsupported);
        }
        let mut stmt = tx.prepare("SELECT singleton,blob FROM meta LIMIT 2")?;
        let mut rows = stmt.query([])?;
        let row = rows.next()?.ok_or(StoreError::Corrupt)?;
        if row.get::<_, i64>(0)? != 1 {
            return Err(StoreError::Corrupt);
        }
        let raw = row.get_ref(1)?;
        let raw = raw.as_blob().map_err(|_| StoreError::Corrupt)?;
        if raw.len() > 256 {
            return Err(StoreError::Corrupt);
        }
        let blob = raw.to_vec();
        let meta = decode_meta(&blob)?;
        if rows.next()?.is_some() {
            return Err(StoreError::Corrupt);
        }
        drop(rows);
        drop(stmt);
        if meta.cluster != cluster {
            return Err(StoreError::WrongCluster);
        }
        // Bounded scan also catches extra rows, including outside the declared tail.
        let negative: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM log WHERE idx<0)", [], |r| {
                r.get(0)
            })?;
        if negative {
            return Err(StoreError::Corrupt);
        }
        let all = Self::select(&tx, 0, i64::MAX as u64 + 1, MAX_LIVE + 1)?;
        Self::verify(&meta, &all)?;
        tx.commit()?;
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let p = dir.join(format!("cluster-log.sqlite3{suffix}"));
            if p.exists() {
                crate::config::checked_path(&p).map_err(|_| StoreError::Io)?;
                crate::security::private(&p).map_err(|_| StoreError::Io)?;
            }
        }
        Ok(Self {
            db,
            meta,
            poisoned: false,
            _lock: lock,
            #[cfg(test)]
            hook: None,
        })
    }
    fn healthy(&self) -> Result<(), StoreError> {
        if self.poisoned {
            Err(StoreError::Poisoned)
        } else {
            Ok(())
        }
    }
    fn checked<T>(&mut self, r: Result<T, StoreError>) -> Result<T, StoreError> {
        if r.is_err() {
            self.poisoned = true;
        }
        r
    }
    pub fn vote(&self) -> Result<Option<StoredVote>, StoreError> {
        self.healthy()?;
        Ok(self.meta.vote)
    }
    pub fn log_state(&self) -> Result<LogState, StoreError> {
        self.healthy()?;
        Ok(LogState {
            purged: self.meta.purged,
            last: self.meta.tail.map(|(_, p)| p).or(self.meta.purged),
        })
    }
    pub fn live(&self) -> Result<usize, StoreError> {
        self.healthy()?;
        Ok(self
            .meta
            .tail
            .map_or(0, |(f, l)| (l.index - f + 1) as usize))
    }
    fn select(
        db: &Connection,
        start: u64,
        end: u64,
        limit: usize,
    ) -> Result<Vec<(LogPos, Entry)>, StoreError> {
        // SQLite INTEGER cannot represent the exclusive upper bound MAX+1.
        let mut stmt =
            db.prepare("SELECT idx,body FROM log WHERE idx>=?1 AND idx<=?2 ORDER BY idx LIMIT ?3")?;
        let mut rows = stmt.query(params![start as i64, (end - 1) as i64, limit as i64])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let idx: i64 = r.get(0)?;
            let raw = r.get_ref(1)?;
            let raw = raw.as_blob().map_err(|_| StoreError::Corrupt)?;
            if raw.len() > MAX_ROW {
                return Err(StoreError::Corrupt);
            }
            let b = raw.to_vec();
            let (p, e) = decode_row(&b)?;
            if idx < 0 || idx as u64 != p.index {
                return Err(StoreError::Corrupt);
            }
            if out.last().is_some_and(|(prev, _): &(LogPos, Entry)| {
                prev.index.checked_add(1) != Some(p.index) || *prev >= p
            }) {
                return Err(StoreError::Corrupt);
            }
            out.push((p, e));
        }
        Ok(out)
    }
    fn verify(m: &Meta, rows: &[(LogPos, Entry)]) -> Result<(), StoreError> {
        match (m.tail, rows.first(), rows.last()) {
            (None, None, None) => Ok(()),
            (Some((f, l)), Some((a, _)), Some((b, _)))
                if a.index == f
                    && *b == l
                    && rows.len() == (l.index - f + 1) as usize
                    && m.purged.is_none_or(|p| *a > p) =>
            {
                Ok(())
            }
            _ => Err(StoreError::Corrupt),
        }
    }
    pub fn entries(
        &mut self,
        start: u64,
        end_excl: u64,
        limit: usize,
    ) -> Result<Vec<(LogPos, Entry)>, StoreError> {
        self.healthy()?;
        if !(1..=MAX_LIVE).contains(&limit) {
            return Err(StoreError::TooLarge);
        }
        let Some((first, last)) = self.meta.tail else {
            return Ok(Vec::new());
        };
        let start = start.max(first);
        let end = end_excl.min(last.index + 1);
        if start >= end {
            return Ok(Vec::new());
        }
        let r = (|| {
            let rows = Self::select(&self.db, start, end, limit)?;
            if rows.len() != ((end - start) as usize).min(limit)
                || rows.first().is_none_or(|(p, _)| p.index != start)
                || rows
                    .iter()
                    .any(|(p, _)| *p > last || self.meta.purged.is_some_and(|q| *p <= q))
                || rows
                    .last()
                    .is_some_and(|(p, _)| p.index == last.index && *p != last)
            {
                return Err(StoreError::Corrupt);
            }
            Ok(rows)
        })();
        self.checked(r)
    }
    fn persist(
        &mut self,
        m: Meta,
        insert: &[(LogPos, Vec<u8>)],
        delete: Option<(u64, u64)>,
    ) -> Result<(), StoreError> {
        let result = (|| {
            let tx = self.db.transaction()?;
            if let Some((lo, hi)) = delete {
                tx.execute(
                    "DELETE FROM log WHERE idx>=?1 AND idx<=?2",
                    params![lo as i64, hi as i64],
                )?;
            }
            for (p, b) in insert {
                tx.execute("INSERT INTO log VALUES(?1,?2)", params![p.index as i64, b])?;
            }
            if tx.execute(
                "UPDATE meta SET blob=?1 WHERE singleton=1",
                [encode_meta(&m)],
            )? != 1
            {
                return Err(StoreError::Sql);
            }
            #[cfg(test)]
            if let Some((p, false)) = &self.hook {
                tx.cache_flush()?;
                super::state_store::tests::pause(p);
            }
            tx.commit()?;
            #[cfg(test)]
            if let Some((p, true)) = &self.hook {
                super::state_store::tests::pause(p);
            }
            Ok(())
        })();
        self.checked(result)?;
        self.meta = m;
        Ok(())
    }
    pub fn save_vote(&mut self, v: StoredVote) -> Result<(), StoreError> {
        self.healthy()?;
        if self.meta.vote.is_some_and(|old| v < old) {
            return Err(StoreError::VoteRegressed);
        }
        if self.meta.vote == Some(v) {
            return Ok(());
        }
        let mut m = self.meta.clone();
        m.vote = Some(v);
        self.persist(m, &[], None)
    }
    pub fn append(&mut self, entries: &[(LogPos, Entry)]) -> Result<(), StoreError> {
        self.healthy()?;
        if entries.len() > MAX_APPEND {
            return Err(StoreError::TooLarge);
        }
        if entries.is_empty() {
            return Ok(());
        }
        let mut prev = self.meta.tail.map(|(_, p)| p).or(self.meta.purged);
        let mut encoded = Vec::new();
        for (p, e) in entries {
            if p.index > i64::MAX as u64 {
                return Err(StoreError::TooLarge);
            }
            if prev.is_some_and(|q| q.index.checked_add(1) != Some(p.index) || *p <= q) {
                return Err(StoreError::OutOfOrder);
            }
            let b = encode_row(*p, e);
            if b.len() > MAX_ROW {
                return Err(StoreError::TooLarge);
            }
            encoded.push((*p, b));
            prev = Some(*p);
        }
        if self.live()? + entries.len() > MAX_LIVE {
            return Err(StoreError::Full);
        }
        let mut m = self.meta.clone();
        m.tail = Some((m.tail.map_or(entries[0].0.index, |(f, _)| f), prev.unwrap()));
        self.persist(m, &encoded, None)
    }
    pub fn truncate_after(&mut self, id: Option<LogPos>) -> Result<(), StoreError> {
        self.healthy()?;
        let mut m = self.meta.clone();
        let Some(a) = id else {
            if m.purged.is_some() {
                return Err(StoreError::OutOfOrder);
            }
            m.tail = None;
            return self.persist(m, &[], Some((0, i64::MAX as u64)));
        };
        if a.index > i64::MAX as u64 {
            return Err(StoreError::TooLarge);
        }
        if let Some(p) = m.purged {
            if a.index < p.index {
                return Err(StoreError::OutOfOrder);
            }
            if a.index == p.index && a != p {
                return Err(StoreError::Conflict);
            }
        }
        let Some((first, last)) = m.tail else {
            return Ok(());
        };
        if a.index > last.index {
            return Ok(());
        }
        if a.index < first {
            if a.index + 1 != first {
                return Err(StoreError::Conflict);
            }
            m.tail = None;
        } else {
            if self.entries(a.index, a.index + 1, 1)?[0].0 != a {
                return Err(StoreError::Conflict);
            }
            if a == last {
                return Ok(());
            }
            m.tail = Some((first, a));
        }
        self.persist(m, &[], Some((a.index + 1, i64::MAX as u64)))
    }
    /// Caller must durably apply/install through id before purging (future adapter duty).
    pub fn purge(&mut self, id: LogPos) -> Result<(), StoreError> {
        self.healthy()?;
        if id.index > i64::MAX as u64 {
            return Err(StoreError::TooLarge);
        }
        let mut m = self.meta.clone();
        if let Some(p) = m.purged {
            if id.index < p.index {
                return Err(StoreError::OutOfOrder);
            }
            if id.index == p.index {
                return if id == p {
                    Ok(())
                } else {
                    Err(StoreError::Conflict)
                };
            }
        }
        if let Some((first, last)) = m.tail {
            if id.index < first {
                if id.index + 1 != first || self.entries(first, first + 1, 1)?[0].0 <= id {
                    return Err(StoreError::Conflict);
                }
            } else if id.index <= last.index {
                if self.entries(id.index, id.index + 1, 1)?[0].0 != id {
                    return Err(StoreError::Conflict);
                }
            } else if id <= last {
                return Err(StoreError::OutOfOrder);
            }
            m.tail = if id.index < last.index {
                Some((id.index + 1, last))
            } else {
                None
            };
        } else if m.purged.is_some_and(|p| id <= p) {
            return Err(StoreError::OutOfOrder);
        }
        m.purged = Some(id);
        self.persist(m, &[], Some((0, id.index)))
    }
}
#[cfg(test)]
mod tests;

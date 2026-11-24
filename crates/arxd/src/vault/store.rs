use super::{Action, Id, Refusal, Reply, ScriptOs, Write, files, new_id, valid_script};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, sync::Mutex, time::Duration};

const MAX_SCRIPTS: i64 = 128;
const MAX_VERSIONS: i64 = 64;
const MAX_BYTES: i64 = 1024 * 1024;
// Fewer versions than host-wide script slots also bounds named scripts per owner.
const MAX_OWNER_VERSIONS: i64 = 96;
const MAX_OWNER_BYTES: i64 = 256 * 1024;
const MAX_REPLAY: i64 = 512;
const MAX_AUDIT: i64 = 256;
const MAX_PAGES: u64 = 1024;
const APPLICATION_ID: u32 = 0x41525856;

const SCHEMA: &str = "
CREATE TABLE scripts (
    seq INTEGER PRIMARY KEY,
    id BLOB NOT NULL UNIQUE CHECK(length(id)=16),
    owner TEXT NOT NULL CHECK(length(owner) BETWEEN 1 AND 64
        AND owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    os TEXT NOT NULL CHECK(os IN ('windows','ubuntu')),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 48
        AND name NOT GLOB '*[^a-z0-9_-]*'),
    UNIQUE(owner,os,name)
) STRICT;
CREATE TABLE versions (
    seq INTEGER PRIMARY KEY,
    id BLOB NOT NULL UNIQUE CHECK(length(id)=16),
    script INTEGER NOT NULL REFERENCES scripts(seq),
    body BLOB NOT NULL CHECK(length(body) BETWEEN 1 AND 4096 AND instr(body,x'00')=0),
    hash BLOB NOT NULL CHECK(length(hash)=32),
    created_ms INTEGER NOT NULL CHECK(created_ms BETWEEN 0 AND 253402300799999),
    state TEXT NOT NULL DEFAULT 'draft'
        CHECK(state IN ('draft','testing','active','rejected','superseded')),
    test_op BLOB UNIQUE CHECK(length(test_op)=16),
    test_status TEXT CHECK(test_status IN
        ('exited','unavailable','timed_out','launch_failed','terminated','interrupted')),
    exit_code INTEGER CHECK(exit_code BETWEEN 0 AND 4294967295),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    CHECK((state='draft')=(test_op IS NULL)),
    CHECK((test_status IS 'exited')=(exit_code IS NOT NULL)),
    CHECK((test_status IS NULL)=(cleanup_confirmed IS NULL)),
    CHECK(state NOT IN ('active','superseded') OR
        (test_status IS 'exited' AND exit_code IS 0 AND cleanup_confirmed IS 1)),
    CHECK(state NOT IN ('draft','rejected') OR (state='draft')=(test_status IS NULL))
) STRICT;
CREATE INDEX versions_script ON versions(script,seq);
CREATE UNIQUE INDEX active_version ON versions(script) WHERE state='active';
CREATE UNIQUE INDEX running_test ON versions((1)) WHERE state='testing' AND test_status IS NULL;
CREATE TABLE replay_clock(singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    high_water INTEGER NOT NULL CHECK(high_water BETWEEN 0 AND 253402300799999)) STRICT;
INSERT INTO replay_clock VALUES(1,0);
CREATE TABLE replay (
    owner TEXT NOT NULL,
    request BLOB NOT NULL CHECK(length(request)=16),
    binding BLOB NOT NULL CHECK(length(binding)=32),
    expires_ms INTEGER NOT NULL,
    operation BLOB NOT NULL UNIQUE CHECK(length(operation)=16),
    version BLOB NOT NULL REFERENCES versions(id),
    PRIMARY KEY(owner,request)
) STRICT, WITHOUT ROWID;
CREATE TABLE audit (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    owner TEXT NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('vault_add_version','vault_test_version','vault_activate')),
    at_ms INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK(outcome IN ('succeeded','started','failed','refused')),
    reason TEXT CHECK(length(reason) BETWEEN 1 AND 40 AND reason NOT GLOB '*[^a-z_]*'),
    version BLOB REFERENCES versions(id),
    operation BLOB CHECK(length(operation)=16)
) STRICT;
CREATE INDEX audit_owner ON audit(owner,seq);
";

pub(super) struct Store {
    db: Mutex<Connection>,
    _file: fs::File,
    _lock: fs::File,
}

impl Store {
    pub(super) fn open(dir: &Path, now: i64) -> Result<Self> {
        let (mut db, file, lock) = files::open(dir)?;
        db.busy_timeout(Duration::from_millis(250))?;
        ensure!(
            db.pragma_query_value(None, "page_size", |r| r.get::<_, u64>(0))? == 4096
                && db.pragma_query_value(None, "page_count", |r| r.get::<_, u64>(0))? <= MAX_PAGES,
            "invalid vault size"
        );
        let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let application: u32 = db.pragma_query_value(None, "application_id", |r| r.get(0))?;
        ensure!(
            matches!((version, application), (0, 0) | (1 | 2, APPLICATION_ID)),
            "unsupported vault state"
        );
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY;
             PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA max_page_count=1024;",
        )?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if version == 0 {
            ensure!(
                scalar(&tx, "SELECT count(*) FROM sqlite_schema", [])? == 0,
                "vault state must be empty"
            );
            tx.execute_batch(SCHEMA)?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        }
        if version < 2 {
            tx.execute_batch(
                "ALTER TABLE versions ADD COLUMN inconclusive INTEGER NOT NULL DEFAULT 0
                 CHECK(inconclusive IN (0,1) AND (inconclusive=0 OR
                       (state='rejected' AND test_status IS 'interrupted' AND cleanup_confirmed IS 0)));",
            )?;
            tx.pragma_update(None, "user_version", 2)?;
        }
        tx.execute(
            "INSERT INTO audit(owner,action,at_ms,outcome,reason,version,operation)
             SELECT s.owner,'vault_test_version',?1,'failed','interrupted',v.id,v.test_op
             FROM versions v JOIN scripts s ON s.seq=v.script
             WHERE v.state='testing' AND v.test_status IS NULL",
            [now],
        )?;
        tx.execute(
            "UPDATE versions SET state='rejected',test_status='interrupted',cleanup_confirmed=0
             WHERE state='testing' AND test_status IS NULL",
            [],
        )?;
        prune_audit(&tx)?;
        tx.commit()?;
        Ok(Self {
            db: Mutex::new(db),
            _file: file,
            _lock: lock,
        })
    }

    pub(super) fn read<T>(&self, run: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("vault state unavailable"))?;
        run(&db)
    }

    pub(super) fn write<T>(&self, run: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("vault state unavailable"))?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = run(&tx)?;
        tx.commit()?;
        Ok(result)
    }
}

fn scalar(db: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<i64> {
    Ok(db.query_row(sql, args, |r| r.get(0))?)
}

pub(super) fn prune_audit(db: &Connection) -> Result<()> {
    db.execute(
        "DELETE FROM audit WHERE seq <= (SELECT seq FROM audit ORDER BY seq DESC LIMIT 1 OFFSET ?1)",
        [MAX_AUDIT],
    )?;
    Ok(())
}

pub(super) struct WriteScope<'a> {
    pub owner: &'a str,
    pub action: Action,
    pub at_ms: i64,
}

impl WriteScope<'_> {
    pub(super) fn audit(
        &self,
        db: &Connection,
        outcome: &str,
        reason: Option<&str>,
        version: Option<Id>,
        operation: Option<Id>,
    ) -> Result<()> {
        db.execute(
            "INSERT INTO audit(owner,action,at_ms,outcome,reason,version,operation)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                self.owner,
                self.action.name(),
                self.at_ms,
                outcome,
                reason,
                version,
                operation
            ],
        )?;
        prune_audit(db)
    }

    pub(super) fn execute(
        &self,
        db: &Connection,
        write: Write,
        tester_available: bool,
        test_refusal: Option<Refusal>,
    ) -> Result<(Reply, Option<JobData>)> {
        let request = write.request_id();
        let now = scalar(
            db,
            "UPDATE replay_clock SET high_water=max(high_water,?1) RETURNING high_water",
            [self.at_ms],
        )?;
        let binding = write.binding();
        let existing: Option<(Id, Id, [u8; 32], i64)> = db.query_row(
            "SELECT operation,version,binding,expires_ms FROM replay WHERE owner=?1 AND request=?2",
            params![self.owner, request.key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        if let Some((operation, version, prior, expires)) = existing
            && now <= expires
        {
            if binding == prior {
                return Ok((Ok(receipt(operation, version)), None));
            }
            return self.refuse(db, Refusal::KeyCollision);
        }
        let expires = request.issued_at_ms + crate::vm::RETRY_WINDOW_MS;
        if request.issued_at_ms > now || expires < now {
            return self.refuse(db, Refusal::StaleKey);
        }
        db.execute("DELETE FROM replay WHERE expires_ms<?1", [now])?;
        if scalar(db, "SELECT count(*) FROM replay", [])? >= MAX_REPLAY {
            return self.refuse(db, Refusal::ReplayFull);
        }
        let operation = new_id()?;
        let (version, job) = match write {
            Write::Add {
                os, name, script, ..
            } => match self.add(db, os, &name, &script)? {
                Ok(version) => (version, None),
                Err(reason) => return self.refuse(db, reason),
            },
            Write::Test { version_id, .. } => {
                match self.test(
                    db,
                    operation,
                    version_id,
                    test_refusal
                        .or_else(|| (!tester_available).then_some(Refusal::TesterUnavailable)),
                )? {
                    Ok(job) => (version_id, Some(job)),
                    Err(reason) => return self.refuse(db, reason),
                }
            }
            Write::Activate { version_id, .. } => {
                if let Err(reason) = self.activate(db, version_id, tester_available)? {
                    return self.refuse(db, reason);
                }
                (version_id, None)
            }
        };
        db.execute(
            "INSERT INTO replay(owner,request,binding,expires_ms,operation,version)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                self.owner,
                request.key,
                binding,
                expires,
                operation,
                version
            ],
        )?;
        self.audit(
            db,
            if job.is_some() {
                "started"
            } else {
                "succeeded"
            },
            None,
            Some(version),
            Some(operation),
        )?;
        Ok((Ok(receipt(operation, version)), job))
    }

    fn refuse(&self, db: &Connection, reason: Refusal) -> Result<(Reply, Option<JobData>)> {
        self.audit(db, "refused", Some(reason.as_str()), None, None)?;
        Ok((Err(reason), None))
    }

    fn add(
        &self,
        db: &Connection,
        os: ScriptOs,
        name: &str,
        script: &str,
    ) -> Result<Result<Id, Refusal>> {
        let existing: Option<i64> = db
            .query_row(
                "SELECT seq FROM scripts WHERE owner=?1 AND os=?2 AND name=?3",
                params![self.owner, os.as_str(), name],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(seq) = existing {
            if scalar(db, "SELECT count(*) FROM versions WHERE script=?1", [seq])? >= MAX_VERSIONS {
                return Ok(Err(Refusal::VersionLimit));
            }
        } else if scalar(db, "SELECT count(*) FROM scripts", [])? >= MAX_SCRIPTS {
            return Ok(Err(Refusal::ScriptLimit));
        }
        let (total_bytes, owner_versions, owner_bytes): (i64, i64, i64) = db.query_row(
            "SELECT coalesce(sum(length(v.body)),0),
                    count(*) FILTER (WHERE s.owner=?1),
                    coalesce(sum(length(v.body)) FILTER (WHERE s.owner=?1),0)
             FROM versions v JOIN scripts s ON s.seq=v.script",
            [self.owner],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let bytes = script.len() as i64;
        if total_bytes + bytes > MAX_BYTES {
            return Ok(Err(Refusal::ByteLimit));
        }
        if owner_versions >= MAX_OWNER_VERSIONS {
            return Ok(Err(Refusal::PrincipalVersionLimit));
        }
        if owner_bytes + bytes > MAX_OWNER_BYTES {
            return Ok(Err(Refusal::PrincipalByteLimit));
        }
        let seq = match existing {
            Some(seq) => seq,
            None => {
                db.execute(
                    "INSERT INTO scripts(id,owner,os,name) VALUES(?1,?2,?3,?4)",
                    params![new_id()?, self.owner, os.as_str(), name],
                )?;
                db.last_insert_rowid()
            }
        };
        let id = new_id()?;
        let hash: [u8; 32] = Sha256::digest(script.as_bytes()).into();
        db.execute(
            "INSERT INTO versions(id,script,body,hash,created_ms) VALUES(?1,?2,?3,?4,?5)",
            params![id, seq, script.as_bytes(), hash, self.at_ms],
        )?;
        Ok(Ok(id))
    }

    fn test(
        &self,
        db: &Connection,
        operation: Id,
        version: Id,
        refusal: Option<Refusal>,
    ) -> Result<Result<JobData, Refusal>> {
        let subject: Option<(bool, ScriptOs)> = db
            .query_row(
                "SELECT v.state='draft' OR v.inconclusive=1,s.os
                 FROM versions v JOIN scripts s ON s.seq=v.script
                 WHERE v.id=?1 AND s.owner=?2",
                params![version, self.owner],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((retryable, os)) = subject else {
            return Ok(Err(Refusal::NotFound));
        };
        if !retryable {
            return Ok(Err(Refusal::NotDraft));
        }
        if let Some(reason) = refusal {
            return Ok(Err(reason));
        }
        if scalar(
            db,
            "SELECT count(*) FROM versions WHERE state='testing' AND test_status IS NULL",
            [],
        )? != 0
        {
            return Ok(Err(Refusal::Busy));
        }
        let (content, hash) = content(db, version)?;
        db.execute(
            "UPDATE versions SET state='testing',test_op=?1,test_status=NULL,
                 exit_code=NULL,cleanup_confirmed=NULL,inconclusive=0 WHERE id=?2",
            params![operation, version],
        )?;
        Ok(Ok(JobData {
            operation,
            version,
            content,
            hash,
            os,
        }))
    }

    fn activate(
        &self,
        db: &Connection,
        version: Id,
        available: bool,
    ) -> Result<Result<(), Refusal>> {
        let subject: Option<(i64, i64, bool)> = db
            .query_row(
                "SELECT v.seq,v.script,v.state='testing' AND v.test_status IS 'exited'
                AND v.exit_code IS 0 AND v.cleanup_confirmed IS 1
             FROM versions v JOIN scripts s ON s.seq=v.script WHERE v.id=?1 AND s.owner=?2",
                params![version, self.owner],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((seq, script, passed)) = subject else {
            return Ok(Err(Refusal::NotFound));
        };
        if !passed {
            return Ok(Err(Refusal::PassRequired));
        }
        if !available {
            return Ok(Err(Refusal::TesterUnavailable));
        }
        if scalar(
            db,
            "SELECT count(*) FROM versions WHERE script=?1 AND state='active' AND seq>?2",
            params![script, seq],
        )? != 0
        {
            return Ok(Err(Refusal::VersionSuperseded));
        }
        db.execute(
            "UPDATE versions SET state='superseded' WHERE script=?1 AND state='active'",
            [script],
        )?;
        db.execute("UPDATE versions SET state='active' WHERE seq=?1", [seq])?;
        Ok(Ok(()))
    }
}

pub(super) struct JobData {
    pub operation: Id,
    pub version: Id,
    pub content: String,
    pub hash: [u8; 32],
    pub os: ScriptOs,
}

pub(super) fn content(db: &Connection, version: Id) -> Result<(String, [u8; 32])> {
    let (bytes, hash) = db.query_row(
        "SELECT CASE WHEN length(body) BETWEEN 1 AND 4096 THEN body END,hash FROM versions WHERE id=?1",
        [version], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, [u8; 32]>(1)?)),
    )?;
    let content = String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("invalid vault content"))?;
    ensure!(
        valid_script(&content) && <[u8; 32]>::from(Sha256::digest(content.as_bytes())) == hash,
        "invalid vault content"
    );
    Ok((content, hash))
}

fn receipt(operation: Id, version: Id) -> Value {
    json!({"operation_id": hex::encode(operation), "version_id": hex::encode(version)})
}

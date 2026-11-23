//! Private publication intent and independent DNS evidence.
use super::*;

pub(super) const MIGRATE_V10: &str = "CREATE TABLE activity_v10 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 17),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 48),
    finished_ms INTEGER CHECK(finished_ms BETWEEN 0 AND 253402300799999),
    terminal_order INTEGER UNIQUE CHECK(terminal_order>0),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    publication_failure INTEGER CHECK(publication_failure BETWEEN 1 AND 17)
        CHECK(publication_failure IS NULL OR (action IN (15,16,17) AND status IN (1,2,3))),
    CHECK(vm IS NOT NULL OR action=11 OR status=4),
    CHECK(action NOT IN (13,14) OR status=4),
    CHECK((status=0)=(finished_ms IS NULL)),
    CHECK((status=0)=(terminal_order IS NULL)),
    CHECK((status=4)=(reason IS NOT NULL)),
    CHECK((actor='daemon')=(action IN (7,9,10,17)) AND (actor='agent' OR principal='arxd')),
    CHECK((vm_owner IS NOT NULL)=(action IN (7,10,17)))
) STRICT;
INSERT INTO activity_v10(id,principal,actor,vm_owner,vm,action,started_ms,status,reason,
    finished_ms,terminal_order,cleanup_confirmed)
    SELECT * FROM activity;
INSERT INTO sqlite_sequence(name,seq) SELECT 'activity_v10',seq FROM sqlite_sequence
    WHERE name='activity' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='activity_v10');
UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='activity'),0))
    WHERE name='activity_v10';
DROP TABLE activity;
ALTER TABLE activity_v10 RENAME TO activity;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND (action<=6 OR action=12);

CREATE TABLE vm_operations_v10 (
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    request_key BLOB NOT NULL CHECK(length(request_key)=16),
    operation_id BLOB NOT NULL UNIQUE DEFAULT (randomblob(16)) CHECK(length(operation_id)=16),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 6 OR action IN (8,11,12,15,16)),
    activity_id INTEGER NOT NULL UNIQUE CHECK(activity_id>0),
    valid_through_ms INTEGER NOT NULL CHECK(valid_through_ms>=0),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 3),
    lease_end_ms INTEGER CHECK(lease_end_ms BETWEEN 0 AND 253402300799999),
    binding_kind TEXT NOT NULL DEFAULT 'legacy_vm'
        CHECK(binding_kind IN ('legacy_vm','vm','template','recipe')),
    subject_id BLOB CHECK(length(subject_id)=16),
    recipe_id TEXT CHECK(length(recipe_id) BETWEEN 1 AND 64
        AND length(recipe_id)=length(CAST(recipe_id AS BLOB))
        AND recipe_id NOT GLOB '*[^a-z0-9-]*'),
    recipe_digest BLOB CHECK(length(recipe_digest)=32),
    request_digest BLOB CHECK(length(request_digest)=32),
    script_result TEXT CHECK(script_result IN
        ('legacy','exited','unavailable','timed_out','launch_failed','terminated','interrupted')),
    script_exit_code INTEGER CHECK(script_exit_code BETWEEN 0 AND 4294967295),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1))
        CHECK(cleanup_confirmed IS NULL OR status BETWEEN 1 AND 3),
    failure_reason INTEGER CHECK(failure_reason BETWEEN 1 AND 17)
        CHECK(failure_reason IS NULL OR status IN (2,3)),
    CHECK(action<>8 OR lease_end_ms IS NOT NULL),
    CHECK(lease_end_ms IS NULL OR action IN (1,8)),
    CHECK((action=11)=(vm IS NULL)),
    CHECK((action=11)=(binding_kind='recipe')),
    CHECK((binding_kind IN ('vm','template'))=(subject_id IS NOT NULL)),
    CHECK((binding_kind='recipe')=(recipe_id IS NOT NULL)
        AND (binding_kind='recipe')=(recipe_digest IS NOT NULL)),
    CHECK(binding_kind NOT IN ('vm','template') OR (action IN (1,12))=(binding_kind='template')),
    CHECK(binding_kind<>'vm' OR subject_id=vm),
    CHECK((binding_kind='template')=(request_digest IS NOT NULL)),
    CHECK(action=2 OR script_result IS NULL),
    CHECK(action<>2 OR status=0 OR script_result IS NOT NULL),
    CHECK(script_result IS NULL OR status<>0 OR script_result='legacy'),
    CHECK((script_result IS 'exited')=(script_exit_code IS NOT NULL)),
    PRIMARY KEY(principal,request_key)
) STRICT, WITHOUT ROWID;
INSERT INTO vm_operations_v10 SELECT * FROM vm_operations;
DROP TABLE vm_operations;
ALTER TABLE vm_operations_v10 RENAME TO vm_operations;

ALTER TABLE vm_static_instances ADD COLUMN publication_failure INTEGER
    CHECK(publication_failure BETWEEN 1 AND 17);

CREATE TABLE vm_publication (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    vm BLOB NOT NULL REFERENCES vm_static_instances(id) CHECK(length(vm)=16),
    generation BLOB NOT NULL REFERENCES vm_generations(id) CHECK(length(generation)=16),
    boot BLOB NOT NULL CHECK(length(boot)=16),
    revision BLOB NOT NULL CHECK(length(revision)=32),
    hostname TEXT NOT NULL CHECK(length(hostname) BETWEEN 3 AND 253),
    tunnel TEXT NOT NULL CHECK(length(tunnel)=36),
    zone TEXT NOT NULL CHECK(length(zone)=32),
    credential_ref TEXT NOT NULL CHECK(length(credential_ref) BETWEEN 1 AND 240),
    nonce TEXT NOT NULL CHECK(length(nonce)=32),
    phase TEXT NOT NULL CHECK(phase IN ('publishing','published','revoking','cleanup_pending')),
    post_possible INTEGER NOT NULL DEFAULT 0 CHECK(post_possible IN (0,1)),
    record TEXT CHECK(record IS NULL OR (length(record)=32 AND post_possible=1)),
    cleanup_reason INTEGER CHECK(cleanup_reason BETWEEN 1 AND 17),
    CHECK(phase<>'published' OR record IS NOT NULL)
) STRICT;
";

pub(crate) struct Intent {
    pub vm: Id,
    pub generation: Id,
    pub boot: Id,
    pub revision: [u8; 32],
    pub hostname: String,
    pub tunnel: String,
    pub zone: String,
    pub credential_ref: String,
    pub nonce: String,
    pub phase: Phase,
    pub post_possible: bool,
    pub record: Option<String>,
    pub reason: Option<FailureReason>,
}

pub(crate) struct SiteStatus {
    pub phase: Option<Phase>,
    pub reason: Option<FailureReason>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Phase {
    Publishing,
    Published,
    Revoking,
    CleanupPending,
}

impl Phase {
    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::Publishing => "publishing",
            Self::Published => "published",
            Self::Revoking => "revoking",
            Self::CleanupPending => "cleanup_pending",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "publishing" => Ok(Self::Publishing),
            "published" => Ok(Self::Published),
            "revoking" => Ok(Self::Revoking),
            "cleanup_pending" => Ok(Self::CleanupPending),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

pub(crate) fn current(db: &Connection) -> Result<Option<Intent>> {
    Ok(db
        .query_row(
            "SELECT vm,generation,boot,revision,hostname,tunnel,zone,credential_ref,nonce,
                phase,post_possible,record,cleanup_reason FROM vm_publication",
            [],
            |row| {
                Ok(Intent {
                    vm: row.get(0)?,
                    generation: row.get(1)?,
                    boot: row.get(2)?,
                    revision: row.get(3)?,
                    hostname: row.get(4)?,
                    tunnel: row.get(5)?,
                    zone: row.get(6)?,
                    credential_ref: row.get(7)?,
                    nonce: row.get(8)?,
                    phase: Phase::parse(&row.get::<_, String>(9)?)?,
                    post_possible: row.get(10)?,
                    record: row.get(11)?,
                    reason: row
                        .get::<_, Option<i64>>(12)?
                        .map(FailureReason::from_code)
                        .transpose()?,
                })
            },
        )
        .optional()?)
}

pub(crate) fn reserve(db: &Connection, intent: &Intent) -> Result<()> {
    ensure!(
        db.execute(
            "INSERT INTO vm_publication(singleton,vm,generation,boot,revision,hostname,tunnel,zone,
                credential_ref,nonce,phase)
             SELECT 1,?1,?2,?3,?4,?5,?6,?7,?8,?9,'publishing'
             FROM vm_generations WHERE id=?2 AND static_id=?1 AND active=1",
            params![
                intent.vm,
                intent.generation,
                intent.boot,
                intent.revision,
                intent.hostname,
                intent.tunnel,
                intent.zone,
                intent.credential_ref,
                intent.nonce
            ],
        )? == 1,
        "publication generation changed"
    );
    Ok(())
}

pub(crate) enum Update<'a> {
    PostPossible,
    /// The worker exited before submitting the POST it had marked.
    PostWithdrawn,
    Record(&'a str),
    Phase(Phase, Option<FailureReason>),
    Released(Option<FailureReason>),
}

pub(crate) fn update(db: &Connection, nonce: &str, change: Update<'_>) -> Result<()> {
    let changed = match change {
        Update::PostPossible => db.execute(
            "UPDATE vm_publication SET post_possible=1 WHERE nonce=?1 AND phase='publishing' AND post_possible=0",
            [nonce],
        )?,
        Update::PostWithdrawn => db.execute(
            "UPDATE vm_publication SET post_possible=0 WHERE nonce=?1 AND phase='publishing' AND post_possible=1 AND record IS NULL",
            [nonce],
        )?,
        Update::Record(id) => db.execute(
            "UPDATE vm_publication SET record=?2 WHERE nonce=?1 AND post_possible=1 AND (record IS NULL OR record=?2)",
            params![nonce,id],
        )?,
        Update::Phase(phase, reason) => db.execute(
            "UPDATE vm_publication SET phase=?2,cleanup_reason=?3 WHERE nonce=?1",
            params![nonce,phase.wire(),reason.map(|value| value as i64)],
        )?,
        Update::Released(reason) => {
            db.execute(
                "UPDATE vm_static_instances SET publication_failure=?2
                 WHERE id=(SELECT vm FROM vm_publication WHERE nonce=?1)",
                params![nonce, reason.map(|value| value as i64)],
            )?;
            db.execute("DELETE FROM vm_publication WHERE nonce=?1", [nonce])?
        }
    };
    ensure!(changed == 1, "publication intent changed");
    Ok(())
}

pub(crate) fn finish(
    db: &Connection,
    operation: i64,
    now: i64,
    failure: Option<FailureReason>,
    cleanup: Option<bool>,
) -> Result<()> {
    super::finish_with_script(
        db,
        operation,
        if failure.is_some() {
            Status::Failed
        } else {
            Status::Succeeded
        },
        now,
        cleanup,
        None,
        failure,
    )?;
    db.execute(
        "UPDATE activity SET publication_failure=?2 WHERE id=?1",
        params![operation, failure.map(|value| value as i64)],
    )?;
    Ok(())
}

pub(super) fn recover(db: &Connection, now: i64) -> Result<()> {
    db.execute(
        "UPDATE vm_publication SET phase='cleanup_pending',cleanup_reason=coalesce(cleanup_reason,11)",
        [],
    )?;
    // A publish and an owner unpublish may both be pending: the unpublish
    // waits for the publish worker's effects slot.
    let operations = db
        .prepare("SELECT id FROM activity WHERE status=0 AND action IN (15,16)")?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for operation in operations {
        super::finish(db, operation, Status::Interrupted, now, Some(false))?;
    }
    Ok(())
}

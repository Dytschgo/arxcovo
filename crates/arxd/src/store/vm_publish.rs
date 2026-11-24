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

pub(super) const MIGRATE_V13: &str = "CREATE TABLE vm_publication_v13 (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    vm BLOB NOT NULL REFERENCES vm_static_instances(id) CHECK(length(vm)=16),
    generation BLOB NOT NULL REFERENCES vm_generations(id) CHECK(length(generation)=16),
    boot BLOB NOT NULL CHECK(length(boot)=16),
    revision BLOB NOT NULL CHECK(length(revision)=32),
    tunnel TEXT NOT NULL CHECK(length(tunnel)=36),
    zone TEXT NOT NULL CHECK(length(zone)=32),
    credential_ref TEXT NOT NULL CHECK(length(credential_ref) BETWEEN 1 AND 240),
    nonce TEXT NOT NULL CHECK(length(nonce)=32),
    phase TEXT NOT NULL CHECK(phase IN ('publishing','published','revoking','cleanup_pending')),
    cleanup_reason INTEGER CHECK(cleanup_reason BETWEEN 1 AND 17)
) STRICT;
INSERT INTO vm_publication_v13(singleton,vm,generation,boot,revision,tunnel,zone,
    credential_ref,nonce,phase,cleanup_reason)
    SELECT singleton,vm,generation,boot,revision,tunnel,zone,
        credential_ref,nonce,phase,cleanup_reason
    FROM vm_publication;
CREATE TABLE vm_publication_hosts (
    publication INTEGER NOT NULL DEFAULT 1 REFERENCES vm_publication_v13(singleton) ON DELETE CASCADE
        CHECK(publication=1),
    position INTEGER NOT NULL CHECK(position BETWEEN 0 AND 7),
    hostname TEXT NOT NULL UNIQUE CHECK(length(hostname) BETWEEN 3 AND 253),
    project_id TEXT CHECK(project_id IS NULL OR (length(project_id) BETWEEN 1 AND 24
        AND project_id NOT GLOB '*[^a-z0-9-]*')),
    service TEXT CHECK(service IS NULL OR (length(service) BETWEEN 1 AND 16
        AND service NOT GLOB '*[^a-z0-9-]*')),
    project_slot INTEGER CHECK(project_slot IS NULL OR project_slot BETWEEN 0 AND 7),
    service_index INTEGER CHECK(service_index IS NULL OR service_index BETWEEN 0 AND 3),
    route_slot INTEGER UNIQUE CHECK(route_slot IS NULL OR route_slot BETWEEN 0 AND 7),
    marker TEXT NOT NULL UNIQUE CHECK(length(marker)=32),
    post_possible INTEGER NOT NULL DEFAULT 0 CHECK(post_possible IN (0,1)),
    record TEXT CHECK(record IS NULL OR (length(record)=32 AND post_possible=1)),
    ambiguous INTEGER NOT NULL DEFAULT 0 CHECK(ambiguous IN (0,1)),
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK(state IN ('pending','verified','cleaned','cleanup_pending')),
    failure_reason INTEGER CHECK(failure_reason BETWEEN 1 AND 17),
    CHECK((project_id IS NULL)=(service IS NULL)
        AND (project_id IS NULL)=(project_slot IS NULL)
        AND (project_id IS NULL)=(service_index IS NULL)
        AND (project_id IS NULL)=(route_slot IS NULL)),
    CHECK(state<>'verified' OR (record IS NOT NULL AND ambiguous=0)),
    CHECK(ambiguous=0 OR post_possible=1),
    PRIMARY KEY(publication,position)
) STRICT, WITHOUT ROWID;
INSERT INTO vm_publication_hosts(publication,position,hostname,marker,post_possible,
    record,ambiguous,state,failure_reason)
    SELECT singleton,0,hostname,nonce,post_possible,record,
        CASE WHEN post_possible=1 AND record IS NULL THEN 1 ELSE 0 END,
        CASE WHEN phase='published' THEN 'verified'
             WHEN phase='cleanup_pending' THEN 'cleanup_pending'
             ELSE 'pending' END,
        cleanup_reason
    FROM vm_publication;
DROP TABLE vm_publication;
ALTER TABLE vm_publication_v13 RENAME TO vm_publication;
";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostState {
    Pending,
    Verified,
    Cleaned,
    CleanupPending,
}

impl HostState {
    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::Cleaned => "cleaned",
            Self::CleanupPending => "cleanup_pending",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "verified" => Ok(Self::Verified),
            "cleaned" => Ok(Self::Cleaned),
            "cleanup_pending" => Ok(Self::CleanupPending),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostIntent {
    pub position: u8,
    pub hostname: crate::vm::publish::Hostname,
    pub route: Option<crate::vm::publish::RouteSlot>,
    pub marker: String,
    pub post_possible: bool,
    pub record: Option<String>,
    pub ambiguous: bool,
    pub state: HostState,
    pub reason: Option<FailureReason>,
}

pub(crate) struct Intent {
    pub vm: Id,
    pub generation: Id,
    pub boot: Id,
    pub revision: [u8; 32],
    pub tunnel: String,
    pub zone: String,
    pub credential_ref: String,
    pub nonce: String,
    pub phase: Phase,
    pub reason: Option<FailureReason>,
    pub hosts: Vec<HostIntent>,
}

pub(crate) struct HostStatusView {
    pub position: u8,
    pub state: HostState,
    pub ambiguous: bool,
    pub route_slot: Option<u8>,
    pub project_id: Option<String>,
    pub service: Option<String>,
    pub reason: Option<FailureReason>,
}

pub(crate) struct SiteStatus {
    pub phase: Option<Phase>,
    pub reason: Option<FailureReason>,
    pub hosts: Vec<HostStatusView>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

/// The release cause after owner-cause precedence: a durable owner
/// cancellation on the intent wins over the reason a cleanup supplies.
pub(crate) fn effective_cause(
    db: &Connection,
    nonce: &str,
    reason: Option<FailureReason>,
) -> Result<Option<FailureReason>> {
    let durable: Option<i64> = db
        .query_row(
            "SELECT cleanup_reason FROM vm_publication WHERE nonce=?1",
            [nonce],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(if durable == Some(FailureReason::Cancelled as i64) {
        Some(FailureReason::Cancelled)
    } else {
        reason
    })
}

pub(crate) fn current(db: &Connection) -> Result<Option<Intent>> {
    let Some(mut intent) = db
        .query_row(
            "SELECT vm,generation,boot,revision,tunnel,zone,credential_ref,nonce,
                phase,cleanup_reason FROM vm_publication",
            [],
            |row| {
                Ok(Intent {
                    vm: row.get(0)?,
                    generation: row.get(1)?,
                    boot: row.get(2)?,
                    revision: row.get(3)?,
                    tunnel: row.get(4)?,
                    zone: row.get(5)?,
                    credential_ref: row.get(6)?,
                    nonce: row.get(7)?,
                    phase: Phase::parse(&row.get::<_, String>(8)?)?,
                    reason: row
                        .get::<_, Option<i64>>(9)?
                        .map(FailureReason::from_code)
                        .transpose()?,
                    hosts: Vec::new(),
                })
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut stmt = db.prepare(
        "SELECT position,hostname,project_id,service,project_slot,service_index,route_slot,
            marker,post_possible,record,ambiguous,state,failure_reason
         FROM vm_publication_hosts WHERE publication=1 ORDER BY position ASC",
    )?;
    let hosts = stmt
        .query_map([], |row| {
            let project_id: Option<String> = row.get(2)?;
            let service: Option<String> = row.get(3)?;
            let project_slot: Option<u8> = row.get(4)?;
            let service_index: Option<u8> = row.get(5)?;
            let route_slot: Option<u8> = row.get(6)?;
            let route = match (project_id, service, project_slot, service_index, route_slot) {
                (
                    Some(project_id),
                    Some(service),
                    Some(project_slot),
                    Some(service_index),
                    Some(route_slot),
                ) => {
                    let slot = crate::vm::publish::RouteSlot {
                        project_id,
                        service,
                        project_slot,
                        service_index,
                        route_slot,
                    };
                    slot.validate().map_err(|_| rusqlite::Error::InvalidQuery)?;
                    Some(slot)
                }
                (None, None, None, None, None) => None,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(HostIntent {
                position: row.get(0)?,
                hostname: crate::vm::publish::Hostname::parse(&row.get::<_, String>(1)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                route,
                marker: row.get(7)?,
                post_possible: row.get(8)?,
                record: row.get(9)?,
                ambiguous: row.get(10)?,
                state: HostState::parse(&row.get::<_, String>(11)?)?,
                reason: row
                    .get::<_, Option<i64>>(12)?
                    .map(FailureReason::from_code)
                    .transpose()?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        !hosts.is_empty() && hosts.len() <= crate::vm::publish::MAX_HOSTNAMES,
        "publication hosts missing or out of bounds"
    );
    intent.hosts = hosts;
    Ok(Some(intent))
}

pub(crate) fn reserve(db: &Connection, intent: &Intent) -> Result<()> {
    ensure!(
        !intent.hosts.is_empty() && intent.hosts.len() <= crate::vm::publish::MAX_HOSTNAMES,
        "publication hosts out of bounds"
    );
    ensure!(
        db.execute(
            "INSERT INTO vm_publication(singleton,vm,generation,boot,revision,tunnel,zone,
                credential_ref,nonce,phase)
             SELECT 1,?1,?2,?3,?4,?5,?6,?7,?8,'publishing'
             FROM vm_generations WHERE id=?2 AND static_id=?1 AND active=1",
            params![
                intent.vm,
                intent.generation,
                intent.boot,
                intent.revision,
                intent.tunnel,
                intent.zone,
                intent.credential_ref,
                intent.nonce
            ],
        )? == 1,
        "publication generation changed"
    );
    for (index, host) in intent.hosts.iter().enumerate() {
        ensure!(
            usize::from(host.position) == index,
            "publication host position mismatch"
        );
        let (project_id, service, project_slot, service_index, route_slot) = match &host.route {
            Some(route) => (
                Some(route.project_id.as_str()),
                Some(route.service.as_str()),
                Some(route.project_slot),
                Some(route.service_index),
                Some(route.route_slot),
            ),
            None => (None, None, None, None, None),
        };
        ensure!(
            db.execute(
                "INSERT INTO vm_publication_hosts(publication,position,hostname,project_id,
                    service,project_slot,service_index,route_slot,marker,post_possible,
                    record,ambiguous,state)
                 VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8,0,NULL,0,'pending')",
                params![
                    host.position,
                    host.hostname.as_str(),
                    project_id,
                    service,
                    project_slot,
                    service_index,
                    route_slot,
                    host.marker,
                ],
            )? == 1,
            "publication host insert failed"
        );
    }
    Ok(())
}

pub(crate) enum Update<'a> {
    HostPostPossible {
        position: u8,
    },
    /// The worker exited before submitting the POST it had marked for this row.
    /// An owner unpublish may already have moved the intent to `revoking`.
    HostPostWithdrawn {
        position: u8,
    },
    HostRecord {
        position: u8,
        record: &'a str,
    },
    HostVerified {
        position: u8,
    },
    HostCleaned {
        position: u8,
    },
    HostCleanupPending {
        position: u8,
        ambiguous: bool,
        reason: Option<FailureReason>,
    },
    Phase(Phase, Option<FailureReason>),
    Released(Option<FailureReason>),
}

pub(crate) fn update(db: &Connection, nonce: &str, change: Update<'_>) -> Result<()> {
    let changed = match change {
        Update::HostPostPossible { position } => db.execute(
            "UPDATE vm_publication_hosts SET post_possible=1,ambiguous=1
             WHERE publication=1 AND position=?2 AND post_possible=0
               AND EXISTS(SELECT 1 FROM vm_publication WHERE singleton=1 AND nonce=?1 AND phase='publishing')",
            params![nonce, position],
        )?,
        Update::HostPostWithdrawn { position } => db.execute(
            "UPDATE vm_publication_hosts SET post_possible=0,ambiguous=0
             WHERE publication=1 AND position=?2 AND post_possible=1 AND record IS NULL
               AND EXISTS(SELECT 1 FROM vm_publication WHERE singleton=1 AND nonce=?1 AND phase IN ('publishing','revoking'))",
            params![nonce, position],
        )?,
        Update::HostRecord { position, record } => db.execute(
            "UPDATE vm_publication_hosts SET record=?3,ambiguous=0
             WHERE publication=1 AND position=?2 AND post_possible=1 AND (record IS NULL OR record=?3)
               AND EXISTS(SELECT 1 FROM vm_publication WHERE singleton=1 AND nonce=?1)",
            params![nonce, position, record],
        )?,
        Update::HostVerified { position } => db.execute(
            "UPDATE vm_publication_hosts SET state='verified',failure_reason=NULL
             WHERE publication=1 AND position=?2 AND post_possible=1 AND record IS NOT NULL AND ambiguous=0
               AND EXISTS(SELECT 1 FROM vm_publication WHERE singleton=1 AND nonce=?1 AND phase='publishing')",
            params![nonce, position],
        )?,
        Update::HostCleaned { position } => db.execute(
            "UPDATE vm_publication_hosts
             SET state='cleaned',record=NULL,ambiguous=0,failure_reason=NULL
             WHERE publication=1 AND position=?2
               AND EXISTS(SELECT 1 FROM vm_publication WHERE singleton=1 AND nonce=?1)",
            params![nonce, position],
        )?,
        Update::HostCleanupPending {
            position,
            ambiguous,
            reason,
        } => db.execute(
            "UPDATE vm_publication_hosts
             SET state='cleanup_pending',
                 ambiguous=CASE WHEN ?3=1 THEN 1 ELSE ambiguous END,
                 failure_reason=coalesce(?4,failure_reason)
             WHERE publication=1 AND position=?2 AND state<>'cleaned'
               AND EXISTS(SELECT 1 FROM vm_publication WHERE singleton=1 AND nonce=?1)",
            params![
                nonce,
                position,
                i64::from(ambiguous),
                reason.map(|value| value as i64)
            ],
        )?,
        // A recorded owner cause has precedence: a later null or generic
        // cleanup reason never erases it, and a null keeps any earlier reason.
        Update::Phase(phase, reason) => db.execute(
            "UPDATE vm_publication SET phase=?2,cleanup_reason=coalesce(
                 CASE WHEN cleanup_reason=?4 THEN cleanup_reason END,?3,cleanup_reason)
             WHERE nonce=?1",
            params![
                nonce,
                phase.wire(),
                reason.map(|value| value as i64),
                FailureReason::Cancelled as i64
            ],
        )?,
        Update::Released(reason) => {
            db.execute(
                "UPDATE vm_static_instances SET publication_failure=coalesce(
                     (SELECT CASE WHEN cleanup_reason=?3 THEN cleanup_reason END
                      FROM vm_publication WHERE nonce=?1),?2)
                 WHERE id=(SELECT vm FROM vm_publication WHERE nonce=?1)",
                params![
                    nonce,
                    reason.map(|value| value as i64),
                    FailureReason::Cancelled as i64
                ],
            )?;
            db.execute("DELETE FROM vm_publication WHERE nonce=?1", [nonce])?
        }
    };
    ensure!(changed == 1, "publication intent changed");
    Ok(())
}

/// The publish worker's final transition. It changes only a still
/// `publishing` intent where every child row has durable verified record
/// evidence and no ambiguity, so a `revoking` or `cleanup_pending` intent
/// committed by an owner unpublish, the reaper or recovery is never
/// overwritten. Returns whether it committed.
pub(crate) fn commit(db: &Connection, nonce: &str) -> Result<bool> {
    Ok(db.execute(
        "UPDATE vm_publication SET phase='published',cleanup_reason=NULL
         WHERE nonce=?1 AND phase='publishing'
           AND EXISTS(SELECT 1 FROM vm_publication_hosts WHERE publication=1)
           AND NOT EXISTS(
               SELECT 1 FROM vm_publication_hosts
               WHERE publication=1
                 AND (post_possible<>1 OR record IS NULL OR ambiguous<>0 OR state<>'verified')
           )",
        [nonce],
    )? == 1)
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

/// Settles publication receipts whose worker exited without its terminal
/// write, once their VM has no publication intent left. `live` holds the
/// receipts whose worker is still running. A lost publish failed: its intent
/// was released without committing `published`. A lost unpublish succeeded:
/// the released intent was its effect. A receipt in `unconfirmed` keeps its
/// cleanup unconfirmed, such as a publish whose failed ingress-file write left
/// a residue. Returns the settled receipts.
pub(crate) fn settle_orphans(
    db: &Connection,
    live: &std::collections::HashSet<i64>,
    unconfirmed: &std::collections::HashSet<i64>,
    now: i64,
) -> Result<Vec<i64>> {
    let pending = db
        .prepare(
            "SELECT a.id,a.action,s.publication_failure FROM activity a
             JOIN vm_static_instances s ON s.id=a.vm
             WHERE a.status=0 AND a.action IN (15,16)
               AND NOT EXISTS(SELECT 1 FROM vm_publication p WHERE p.vm=a.vm)",
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut settled = Vec::new();
    for (operation, action, failure) in pending {
        if live.contains(&operation) {
            continue;
        }
        let failure = if action == 15 {
            Some(
                failure
                    .map(FailureReason::from_code)
                    .transpose()?
                    .unwrap_or(FailureReason::Cancelled),
            )
        } else {
            None
        };
        finish(
            db,
            operation,
            now,
            failure,
            Some(!unconfirmed.contains(&operation)),
        )?;
        settled.push(operation);
    }
    Ok(settled)
}

/// Publication receipts still pending.
pub(crate) fn pending_receipts(db: &Connection) -> Result<usize> {
    let count: i64 = db.query_row(
        "SELECT count(*) FROM activity WHERE status=0 AND action IN (15,16)",
        [],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count)?)
}

pub(super) fn recover(db: &Connection, now: i64) -> Result<()> {
    db.execute(
        "UPDATE vm_publication SET phase='cleanup_pending',cleanup_reason=coalesce(cleanup_reason,11)",
        [],
    )?;
    db.execute(
        "UPDATE vm_publication_hosts
         SET ambiguous=CASE WHEN post_possible=1 AND record IS NULL THEN 1 ELSE ambiguous END,
             state='cleanup_pending',
             failure_reason=coalesce(failure_reason,11)
         WHERE publication=1 AND state<>'cleaned'",
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

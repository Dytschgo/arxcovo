//! The VM profile has its own activity schema and database version. The
//! shared and service openers have no route into this module.
//! Each VM opener rejects later versions.
//! Upgrades are forward-only. Upgraded state needs a matching or newer opener.
use super::{check_size, validate_principal};
use crate::vm::{
    Action, ArtifactEvidence, ArtifactSlot, BuildRequest, BuildSlots, Completion, ExpiredVm,
    ExpiryClaim, FailureReason, Guest, Id, OperationRef, OperationStatus, PairSlots,
    RETRY_WINDOW_MS, Receipt, RecoveryClaim, Refusal, RequestId, ScriptResult, Status, Subject,
    VmStatus,
    paths::FileIdentity,
    reaper::{CleanupArtifact, CleanupKind, CleanupWork},
};
use anyhow::{Result, ensure};
use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, ffi,
    params,
};
use std::{fs, path::Path, sync::Mutex, time::Duration};

const SCHEMA_V5: &str = "
CREATE TABLE activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB NOT NULL CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 10),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 18),
    finished_ms INTEGER CHECK(finished_ms BETWEEN 0 AND 253402300799999),
    terminal_order INTEGER UNIQUE CHECK(terminal_order>0),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    CHECK((status=0)=(finished_ms IS NULL)),
    CHECK((status=0)=(terminal_order IS NULL)),
    CHECK((status=4)=(reason IS NOT NULL)),
    CHECK((actor='daemon')=(action IN (7,9,10)) AND (actor='agent' OR principal='arxd')),
    CHECK((vm_owner IS NOT NULL)=(action IN (7,10)))
) STRICT;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND action<=6;
CREATE TABLE vm_retry_clock (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    high_water_ms INTEGER NOT NULL CHECK(high_water_ms BETWEEN 0 AND 253402300799999)
) STRICT;
INSERT INTO vm_retry_clock VALUES(1,0);
CREATE TABLE vm_operations (
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    request_key BLOB NOT NULL CHECK(length(request_key)=16),
    vm BLOB NOT NULL CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 6 OR action=8),
    activity_id INTEGER NOT NULL UNIQUE CHECK(activity_id>0),
    valid_through_ms INTEGER NOT NULL CHECK(valid_through_ms>=0),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 3),
    lease_end_ms INTEGER CHECK(lease_end_ms BETWEEN 0 AND 253402300799999),
    CHECK((action=8)=(lease_end_ms IS NOT NULL)),
    PRIMARY KEY(principal,request_key)
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_templates (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    guest INTEGER NOT NULL CHECK(guest IN (1,2)),
    recipe_digest BLOB NOT NULL UNIQUE CHECK(length(recipe_digest)=32),
    disk_digest BLOB NOT NULL CHECK(length(disk_digest)=32),
    vars_digest BLOB NOT NULL CHECK(length(vars_digest)=32),
    bytes INTEGER NOT NULL CHECK(bytes>0)
) STRICT, WITHOUT ROWID;
-- 0 creating; 1 stopped; 2 running; 3 recovery; 4 teardown required.
-- Every row is a managed TEST instance. Shared/static assets have no row here.
CREATE TABLE vm_instances (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    template BLOB NOT NULL REFERENCES vm_templates(id),
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    expires_ms INTEGER NOT NULL CHECK(expires_ms BETWEEN 0 AND 253402300799999),
    state INTEGER NOT NULL DEFAULT 0 CHECK(state BETWEEN 0 AND 4),
    cleanup_id INTEGER UNIQUE REFERENCES activity(id),
    CHECK(cleanup_id IS NULL OR state IN (3,4)),
    CHECK(state<>3 OR cleanup_id IS NOT NULL)
) STRICT, WITHOUT ROWID;
CREATE INDEX vm_expiry ON vm_instances(expires_ms,id);
CREATE INDEX vm_owner ON vm_instances(principal,id);
CREATE TABLE vm_tombstones (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    activity_id INTEGER NOT NULL UNIQUE REFERENCES activity(id),
    expired_at_ms INTEGER NOT NULL CHECK(expired_at_ms BETWEEN 0 AND 253402300799999)
) STRICT, WITHOUT ROWID;
CREATE TABLE vm_snapshots (
    vm BLOB NOT NULL REFERENCES vm_instances(id) ON DELETE CASCADE,
    id BLOB NOT NULL CHECK(length(id)=16),
    ready INTEGER NOT NULL DEFAULT 0 CHECK(ready IN (0,1)),
    PRIMARY KEY(vm,id)
) STRICT, WITHOUT ROWID;
-- Keep byte claims independent of audit retention. A claim with a snapshot
-- belongs to that snapshot. A claim without one belongs to the VM.
CREATE TABLE vm_disk_claims (
    activity_id INTEGER PRIMARY KEY,
    vm BLOB NOT NULL REFERENCES vm_instances(id) ON DELETE CASCADE,
    snapshot BLOB,
    persistent INTEGER NOT NULL CHECK(persistent>=0),
    staging INTEGER NOT NULL CHECK(staging>=0),
    CHECK(persistent>0 OR staging>0),
    FOREIGN KEY(vm,snapshot) REFERENCES vm_snapshots(vm,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=5;";

const VM_SCHEMA_VERSION: u32 = 12;

// Activity IDs also serve as audit order and as references from cleanup and
// tombstone rows. Copy them verbatim and carry the AUTOINCREMENT high-water.
// Action 11 is private build storage. The public Action codes stay as they were.
const MIGRATE_V6: &str = "
CREATE TABLE activity_v6 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 11),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 18),
    finished_ms INTEGER CHECK(finished_ms BETWEEN 0 AND 253402300799999),
    terminal_order INTEGER UNIQUE CHECK(terminal_order>0),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    CHECK(vm IS NOT NULL OR action=11),
    CHECK((status=0)=(finished_ms IS NULL)),
    CHECK((status=0)=(terminal_order IS NULL)),
    CHECK((status=4)=(reason IS NOT NULL)),
    CHECK((actor='daemon')=(action IN (7,9,10)) AND (actor='agent' OR principal='arxd')),
    CHECK((vm_owner IS NOT NULL)=(action IN (7,10)))
) STRICT;
INSERT INTO activity_v6 SELECT * FROM activity;
INSERT INTO sqlite_sequence(name,seq) SELECT 'activity_v6',seq FROM sqlite_sequence
    WHERE name='activity' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='activity_v6');
UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='activity'),0))
    WHERE name='activity_v6';
DROP TABLE activity;
ALTER TABLE activity_v6 RENAME TO activity;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND action<=6;

CREATE TABLE vm_operations_v6 (
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    request_key BLOB NOT NULL CHECK(length(request_key)=16),
    operation_id BLOB NOT NULL UNIQUE DEFAULT (randomblob(16)) CHECK(length(operation_id)=16),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 6 OR action IN (8,11)),
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
    CHECK((action=8)=(lease_end_ms IS NOT NULL)),
    CHECK((action=11)=(vm IS NULL)),
    CHECK((action=11)=(binding_kind='recipe')),
    CHECK((binding_kind IN ('vm','template'))=(subject_id IS NOT NULL)),
    CHECK((binding_kind='recipe')=(recipe_id IS NOT NULL)
        AND (binding_kind='recipe')=(recipe_digest IS NOT NULL)),
    CHECK(binding_kind NOT IN ('vm','template') OR (action=1)=(binding_kind='template')),
    CHECK(binding_kind<>'vm' OR subject_id=vm),
    CHECK((binding_kind='template')=(request_digest IS NOT NULL)),
    CHECK(action=2 OR script_result IS NULL),
    CHECK(action<>2 OR status=0 OR script_result IS NOT NULL),
    CHECK(script_result IS NULL OR status<>0 OR script_result='legacy'),
    CHECK((script_result IS 'exited')=(script_exit_code IS NOT NULL)),
    PRIMARY KEY(principal,request_key)
) STRICT, WITHOUT ROWID;
-- Ledger rows can outlive their activity projection. Copy directly from the ledger.
INSERT INTO vm_operations_v6(principal,request_key,vm,action,activity_id,valid_through_ms,
    status,lease_end_ms,script_result)
    SELECT principal,request_key,vm,action,activity_id,valid_through_ms,status,lease_end_ms,
        CASE WHEN action=2 THEN 'legacy' END FROM vm_operations;
DROP TABLE vm_operations;
ALTER TABLE vm_operations_v6 RENAME TO vm_operations;

-- Build charges live independently of instances. Completed build evidence can
-- outlive its retry ledger. The sealed charge lives in the referenced template.
CREATE TABLE vm_build_intents (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    operation_id BLOB UNIQUE REFERENCES vm_operations(operation_id) ON DELETE SET NULL,
    recipe_id TEXT NOT NULL CHECK(length(recipe_id) BETWEEN 1 AND 64
        AND length(recipe_id)=length(CAST(recipe_id AS BLOB))
        AND recipe_id NOT GLOB '*[^a-z0-9-]*'),
    recipe_digest BLOB NOT NULL CHECK(length(recipe_digest)=32),
    staging_bytes INTEGER NOT NULL CHECK(staging_bytes>=0),
    template_id BLOB UNIQUE REFERENCES vm_templates(id),
    health_verified INTEGER NOT NULL DEFAULT 0 CHECK(health_verified IN (0,1)),
    state TEXT NOT NULL CHECK(state IN ('pending','sealed','pending_disposal','quarantined')),
    CHECK((state='sealed')=(template_id IS NOT NULL)),
    CHECK((state='sealed')=(staging_bytes=0)),
    CHECK(state<>'sealed' OR health_verified=1),
    CHECK(state<>'pending' OR operation_id IS NOT NULL)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX vm_pending_build ON vm_build_intents(recipe_digest) WHERE state='pending';

CREATE TABLE vm_generations (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    build_id BLOB REFERENCES vm_build_intents(id),
    vm_id BLOB REFERENCES vm_instances(id),
    generation INTEGER NOT NULL CHECK(generation>0),
    active INTEGER NOT NULL DEFAULT 0 CHECK(active IN (0,1)),
    CHECK((build_id IS NULL)<>(vm_id IS NULL)),
    UNIQUE(build_id,generation),
    UNIQUE(vm_id,generation)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX vm_active_build_generation ON vm_generations(build_id) WHERE active=1;
CREATE UNIQUE INDEX vm_active_instance_generation ON vm_generations(vm_id) WHERE active=1;

-- object_key is a generated ID used with the role to derive a relative name.
-- File evidence uses opaque binary identities and bounded content digests.
CREATE TABLE vm_artifacts (
    generation_id BLOB NOT NULL REFERENCES vm_generations(id),
    role TEXT NOT NULL CHECK(role IN ('disk','vars','seed','verification_overlay')),
    object_key BLOB NOT NULL UNIQUE CHECK(length(object_key)=16),
    state TEXT NOT NULL CHECK(state IN
        ('planned','created','identified','sealed','active','pending_disposal','quarantined')),
    volume_id BLOB CHECK(length(volume_id)=8),
    directory_id BLOB CHECK(length(directory_id) IN (8,16)),
    file_id BLOB CHECK(length(file_id) IN (8,16)),
    content_digest BLOB CHECK(length(content_digest)=32),
    bytes INTEGER CHECK(bytes>0),
    CHECK((volume_id IS NULL)=(directory_id IS NULL)
        AND (volume_id IS NULL)=(file_id IS NULL)),
    CHECK(state NOT IN ('identified','sealed','active') OR file_id IS NOT NULL),
    CHECK(state<>'sealed' OR (content_digest IS NOT NULL AND bytes IS NOT NULL)),
    PRIMARY KEY(generation_id,role)
) STRICT, WITHOUT ROWID;

ALTER TABLE vm_instances ADD COLUMN cleanup_route TEXT
    CHECK(cleanup_route IN ('staging','lease','legacy_unknown'))
    CHECK(cleanup_route IS NULL OR state=4);
-- The first disk claim is the originating clone: its persistent charge stays
-- until instance deletion. An old claim taken before the deadline also proves
-- staging cleanup. Absent origin evidence, an older claim may be ambiguous.
WITH origin AS (
    SELECT v.id,v.expires_ms,v.cleanup_id,a.started_ms,
        coalesce(o.status,c.status) AS clone_status
    FROM vm_instances v
    LEFT JOIN activity a ON a.id=v.cleanup_id
    LEFT JOIN vm_disk_claims d ON d.activity_id=(SELECT min(activity_id) FROM vm_disk_claims WHERE vm=v.id)
    LEFT JOIN vm_operations o ON o.activity_id=d.activity_id AND o.action=1
    LEFT JOIN activity c ON c.id=d.activity_id AND c.action=1
    WHERE v.state=4
)
UPDATE vm_instances SET cleanup_route=CASE
    WHEN origin.cleanup_id IS NULL OR origin.started_ms<origin.expires_ms
        OR origin.clone_status IN (0,2,3) THEN 'staging'
    WHEN origin.clone_status=1 THEN 'lease'
    ELSE 'legacy_unknown' END
    FROM origin WHERE vm_instances.id=origin.id;
CREATE TRIGGER vm_cleanup_route AFTER UPDATE OF state ON vm_instances
    WHEN NEW.state=4 AND OLD.state<>4 AND NEW.cleanup_route IS NULL
    BEGIN
        UPDATE vm_instances SET cleanup_route=CASE WHEN OLD.state=0 THEN 'staging' ELSE 'lease' END
            WHERE id=NEW.id;
    END;
";

// Caller receipts keep their own cleanup proof because audit rows are pruned.
// Copy still-retained terminal agent proof before recovery can prune more.
// NULL means the proof is unavailable, and the receipt omits cleanup.
const MIGRATE_V7: &str = "
ALTER TABLE vm_operations ADD COLUMN cleanup_confirmed INTEGER
    CHECK(cleanup_confirmed IN (0,1))
    CHECK(cleanup_confirmed IS NULL OR status BETWEEN 1 AND 3);
UPDATE vm_operations AS o SET cleanup_confirmed=a.cleanup_confirmed
    FROM activity AS a
    WHERE a.id=o.activity_id AND a.actor='agent' AND a.principal=o.principal
        AND a.vm IS o.vm AND a.action=o.action AND a.status=o.status
        AND o.status BETWEEN 1 AND 3 AND a.cleanup_confirmed IS NOT NULL;
";

pub(crate) const MAX_CALLER_PENDING: i64 = 8;
// One cleanup slot per live instance, shared by expiry and recovery. Cleanup
// stays available at caller saturation. At most 16 pending rows are visible.
pub(crate) const MAX_INSTANCES: i64 = 8;
const MAX_RECEIPTS: i64 = 128;
const MAX_LEDGER: i64 = 512;
const MAX_TOMBSTONES: i64 = 32;
const TOMBSTONE_LIFETIME_MS: i64 = 24 * 60 * 60 * 1000;
// Leave space for finishing every pending operation, all eight expiry claims
// and their cleanup updates. New admission checks this after all its inserts.
const HEADROOM_PAGES: u64 = 64;

const MIGRATE_V8: &str = "
ALTER TABLE vm_instances ADD COLUMN dashboard_cpus INTEGER CHECK(dashboard_cpus BETWEEN 1 AND 1024);
ALTER TABLE vm_instances ADD COLUMN dashboard_memory INTEGER CHECK(dashboard_memory BETWEEN 1 AND 9007199254740991);
ALTER TABLE vm_snapshots ADD COLUMN dashboard_created_ms INTEGER CHECK(dashboard_created_ms BETWEEN 0 AND 253402300799999);
";

const MIGRATE_V9: &str = "CREATE TABLE activity_v9 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor TEXT NOT NULL CHECK(actor IN ('agent','daemon')),
    vm_owner TEXT CHECK(length(CAST(vm_owner AS BLOB)) BETWEEN 1 AND 64
        AND vm_owner NOT GLOB '*[^a-zA-Z0-9_-]*'),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 14),
    started_ms INTEGER NOT NULL CHECK(started_ms BETWEEN 0 AND 253402300799999),
    status INTEGER NOT NULL DEFAULT 0 CHECK(status BETWEEN 0 AND 4),
    reason INTEGER CHECK(reason BETWEEN 1 AND 44),
    finished_ms INTEGER CHECK(finished_ms BETWEEN 0 AND 253402300799999),
    terminal_order INTEGER UNIQUE CHECK(terminal_order>0),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IN (0,1)),
    CHECK(vm IS NOT NULL OR action=11 OR status=4),
    CHECK(action NOT IN (13,14) OR status=4),
    CHECK((status=0)=(finished_ms IS NULL)),
    CHECK((status=0)=(terminal_order IS NULL)),
    CHECK((status=4)=(reason IS NOT NULL)),
    CHECK((actor='daemon')=(action IN (7,9,10)) AND (actor='agent' OR principal='arxd')),
    CHECK((vm_owner IS NOT NULL)=(action IN (7,10)))
) STRICT;
INSERT INTO activity_v9
    SELECT id,principal,actor,vm_owner,CASE WHEN status=4 AND NOT EXISTS(
        SELECT 1 FROM vm_instances v WHERE v.id=a.vm AND v.principal=a.principal
        UNION ALL SELECT 1 FROM vm_tombstones t WHERE t.id=a.vm AND t.principal=a.principal
    ) THEN NULL ELSE vm END,action,started_ms,status,reason,finished_ms,terminal_order,cleanup_confirmed
    FROM activity a;
INSERT INTO sqlite_sequence(name,seq) SELECT 'activity_v9',seq FROM sqlite_sequence
    WHERE name='activity' AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='activity_v9');
UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='activity'),0))
    WHERE name='activity_v9';
DROP TABLE activity;
ALTER TABLE activity_v9 RENAME TO activity;
CREATE INDEX activity_principal ON activity(principal,id);
CREATE UNIQUE INDEX pending_vm ON activity(vm) WHERE status=0 AND (action<=6 OR action=12);

CREATE TABLE vm_operations_v9 (
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    request_key BLOB NOT NULL CHECK(length(request_key)=16),
    operation_id BLOB NOT NULL UNIQUE DEFAULT (randomblob(16)) CHECK(length(operation_id)=16),
    vm BLOB CHECK(length(vm)=16),
    action INTEGER NOT NULL CHECK(action BETWEEN 1 AND 6 OR action IN (8,11,12)),
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
    failure_reason INTEGER CHECK(failure_reason BETWEEN 1 AND 11)
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
INSERT INTO vm_operations_v9
    SELECT principal,request_key,operation_id,vm,action,activity_id,valid_through_ms,status,
        lease_end_ms,binding_kind,subject_id,recipe_id,recipe_digest,request_digest,
        script_result,script_exit_code,cleanup_confirmed,CASE
    WHEN status=3 AND cleanup_confirmed=0 THEN 11
    WHEN status=3 AND action=2 THEN 9 END FROM vm_operations;
DROP TABLE vm_operations;
ALTER TABLE vm_operations_v9 RENAME TO vm_operations;

CREATE TABLE vm_state_host (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    id BLOB NOT NULL UNIQUE CHECK(length(id)=16)
) STRICT;
INSERT INTO vm_state_host VALUES(1,randomblob(16));

CREATE TABLE vm_static_instances (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    name TEXT NOT NULL UNIQUE CHECK(length(name) BETWEEN 1 AND 63
        AND length(name)=length(CAST(name AS BLOB))
        AND name NOT GLOB '*[^a-z0-9-]*' AND name NOT GLOB '-*' AND name NOT GLOB '*-'),
    principal TEXT NOT NULL CHECK(length(CAST(principal AS BLOB)) BETWEEN 1 AND 64
        AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    host BLOB NOT NULL REFERENCES vm_state_host(id) CHECK(length(host)=16),
    template BLOB NOT NULL CHECK(length(template)=16),
    source_generation BLOB NOT NULL CHECK(length(source_generation)=16),
    recipe_digest BLOB NOT NULL CHECK(length(recipe_digest)=32),
    disk_digest BLOB NOT NULL CHECK(length(disk_digest)=32),
    vars_digest BLOB NOT NULL CHECK(length(vars_digest)=32),
    state INTEGER NOT NULL CHECK(state BETWEEN 0 AND 4),
    persistent INTEGER NOT NULL CHECK(persistent>0),
    staging INTEGER NOT NULL CHECK(staging>=0),
    CHECK(state NOT IN (1,2) OR staging=0)
) STRICT, WITHOUT ROWID;
CREATE INDEX vm_static_owner ON vm_static_instances(principal,id);
-- The exclusive store pins these rows to this host. Provenance is deliberately
-- not an FK. Pending copies hold their existing source through admission.
CREATE TABLE vm_generations_v9 (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    build_id BLOB REFERENCES vm_build_intents(id),
    vm_id BLOB REFERENCES vm_instances(id),
    static_id BLOB REFERENCES vm_static_instances(id),
    generation INTEGER NOT NULL CHECK(generation>0),
    active INTEGER NOT NULL DEFAULT 0 CHECK(active IN (0,1)),
    CHECK((build_id IS NOT NULL)+(vm_id IS NOT NULL)+(static_id IS NOT NULL)=1),
    UNIQUE(build_id,generation),
    UNIQUE(vm_id,generation),
    UNIQUE(static_id,generation)
) STRICT, WITHOUT ROWID;
INSERT INTO vm_generations_v9(id,build_id,vm_id,generation,active)
    SELECT id,build_id,vm_id,generation,active FROM vm_generations;
DROP TABLE vm_generations;
ALTER TABLE vm_generations_v9 RENAME TO vm_generations;
CREATE UNIQUE INDEX vm_active_build_generation ON vm_generations(build_id) WHERE active=1;
CREATE UNIQUE INDEX vm_active_instance_generation ON vm_generations(vm_id) WHERE active=1;
CREATE UNIQUE INDEX vm_active_static_generation ON vm_generations(static_id) WHERE active=1;

CREATE TABLE vm_artifacts_v9 (
    generation_id BLOB NOT NULL REFERENCES vm_generations(id),
    role TEXT NOT NULL CHECK(role IN ('disk','vars','seed','verification_overlay','static_disk','static_vars')),
    object_key BLOB NOT NULL UNIQUE CHECK(length(object_key)=16),
    state TEXT NOT NULL CHECK(state IN
        ('planned','created','identified','sealed','active','pending_disposal','quarantined')),
    volume_id BLOB CHECK(length(volume_id)=8),
    directory_id BLOB CHECK(length(directory_id) IN (8,16)),
    file_id BLOB CHECK(length(file_id) IN (8,16)),
    content_digest BLOB CHECK(length(content_digest)=32),
    bytes INTEGER CHECK(bytes>0),
    CHECK((volume_id IS NULL)=(directory_id IS NULL)
        AND (volume_id IS NULL)=(file_id IS NULL)),
    CHECK(state NOT IN ('identified','sealed','active') OR file_id IS NOT NULL),
    CHECK(state<>'sealed' OR (content_digest IS NOT NULL AND bytes IS NOT NULL)),
    PRIMARY KEY(generation_id,role)
) STRICT, WITHOUT ROWID;

INSERT INTO vm_artifacts_v9 SELECT * FROM vm_artifacts;
DROP TABLE vm_artifacts;
ALTER TABLE vm_artifacts_v9 RENAME TO vm_artifacts;

CREATE TRIGGER vm_test_disjoint BEFORE INSERT ON vm_instances
    WHEN EXISTS(SELECT 1 FROM vm_static_instances WHERE id=NEW.id)
    BEGIN SELECT RAISE(ABORT,'occupied VM identity'); END;
CREATE TRIGGER vm_static_disjoint BEFORE INSERT ON vm_static_instances
    WHEN EXISTS(SELECT 1 FROM vm_instances WHERE id=NEW.id)
      OR EXISTS(SELECT 1 FROM vm_tombstones WHERE id=NEW.id)
    BEGIN SELECT RAISE(ABORT,'occupied VM identity'); END;";

#[path = "vm_publish.rs"]
pub(crate) mod publish;

#[path = "vm_snapshot.rs"]
pub(crate) mod snapshot;

#[path = "vm_compose.rs"]
pub(crate) mod compose;

#[path = "vm_static.rs"]
pub(crate) mod static_vm;

pub(crate) struct VmStore {
    db: Mutex<Connection>,
    _database: fs::File,
    _lock: fs::File,
}

impl VmStore {
    pub(crate) fn open(dir: &Path, now: i64) -> Result<Self> {
        Self::open_inner(dir, now, open_sqlite)
    }

    fn open_inner(
        dir: &Path,
        now: i64,
        connect: impl FnOnce(&Path) -> Result<Connection>,
    ) -> Result<Self> {
        crate::config::checked_path(dir)?;
        crate::security::private(dir)?;
        ensure!(dir.is_dir(), "VM state directory required");

        let lock_path = dir.join("activity.lock");
        check_existing_file(&lock_path)?;
        let lock = state_options().write(true).create(true).open(&lock_path)?;
        lock.try_lock()?;
        verify_state_identity(&lock, &lock_path)?;

        let path = dir.join("activity.sqlite3");
        let exists = check_existing_file(&path)?;
        for suffix in ["-journal", "-wal", "-shm"] {
            check_existing_file(&dir.join(format!("activity.sqlite3{suffix}")))?;
        }
        let database = if exists {
            state_options().open(&path)?
        } else {
            state_options().write(true).create_new(true).open(&path)?
        };
        // The guard stays unlocked because SQLite owns the database locks.
        verify_state_identity(&database, &path)?;
        let expected = state_identity(&database)?;
        let mut db = connect(&path)?;
        ensure!(
            sqlite_main_identity(&mut db)? == expected && state_identity(&database)? == expected,
            "SQLite VM main database identity changed"
        );
        verify_state_identity(&database, &path)?;
        db.busy_timeout(Duration::from_millis(250))?;
        check_size(&db)?;
        let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(
            matches!(version, 0 | 5..=VM_SCHEMA_VERSION),
            "unsupported VM state version"
        );
        let upgrade = version != VM_SCHEMA_VERSION;
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY;
            PRAGMA foreign_keys=ON; PRAGMA max_page_count=1024;",
        )?;
        // SQLite's table replacement procedure suspends FK enforcement on this
        // connection so the original activity name can retain its dependents.
        // Validate every FK before publishing the replacement and restore
        // enforcement before handing the connection to any caller.
        if upgrade {
            db.pragma_update(None, "foreign_keys", false)?;
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            tx.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))? == version,
            "VM state version changed"
        );
        if version == 0 {
            ensure!(
                count(&tx, "SELECT count(*) FROM sqlite_schema", [])? == 0,
                "VM state must start with an empty database"
            );
            tx.execute_batch(SCHEMA_V5)?;
        }
        if version < 6 {
            tx.execute_batch(MIGRATE_V6)?;
        }
        if version < 7 {
            tx.execute_batch(MIGRATE_V7)?;
        }
        if version < 8 {
            tx.execute_batch(MIGRATE_V8)?;
        }
        if version < 9 {
            tx.execute_batch(MIGRATE_V9)?;
        }
        if version < 10 {
            tx.execute_batch(publish::MIGRATE_V10)?;
        }
        if version < 11 {
            tx.execute_batch(snapshot::MIGRATE_V11)?;
        }
        if version < 12 {
            tx.execute_batch(compose::MIGRATE_V12)?;
        }
        recover(&tx, now)?;
        check_size(&tx)?;
        if upgrade {
            require_headroom(&tx)?;
            ensure!(
                count(
                    &tx,
                    "SELECT NOT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
                    []
                )? == 1,
                "invalid VM state foreign keys"
            );
            tx.pragma_update(None, "user_version", VM_SCHEMA_VERSION)?;
        }
        tx.commit()?;
        if upgrade {
            db.pragma_update(None, "foreign_keys", true)?;
        }
        Ok(Self {
            db: Mutex::new(db),
            _database: database,
            _lock: lock,
        })
    }

    pub(crate) fn transact<T>(&self, run: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("VM state unavailable"))?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = run(&tx)?;
        tx.commit()?;
        Ok(result)
    }
}

fn open_sqlite(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?)
}

pub(crate) struct DashboardCatalog {
    db: Mutex<Connection>,
    _database: fs::File,
}

pub(crate) struct DashboardVm {
    pub id: Id,
    pub template: Id,
    pub guest: Guest,
    pub state: &'static str,
    pub expires_at_ms: i64,
    pub cpus: Option<u32>,
    pub memory: Option<u64>,
    pub disk: u64,
    pub snapshots: Vec<DashboardSnapshot>,
}

pub(crate) struct DashboardSnapshot {
    pub id: Id,
    pub created_at_ms: Option<i64>,
    pub size: Option<u64>,
}

impl DashboardCatalog {
    /// Acquire a read-only connection to the controller's catalog without
    /// running recovery or taking ownership from the controller.
    pub(crate) fn open(dir: &Path) -> Result<Self> {
        crate::config::checked_path(dir)?;
        crate::security::private(dir)?;
        let path = dir.join("activity.sqlite3");
        ensure!(check_existing_file(&path)?, "VM catalog not found");
        for suffix in ["-journal", "-wal", "-shm"] {
            check_existing_file(&dir.join(format!("activity.sqlite3{suffix}")))?;
        }
        let database = state_options().open(&path)?;
        let mut db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NOFOLLOW
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        verify_state_identity(&database, &path)?;
        ensure!(
            sqlite_main_identity(&mut db)? == state_identity(&database)?,
            "VM catalog changed"
        );
        db.busy_timeout(Duration::from_millis(250))?;
        db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
        check_size(&db)?;
        let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(
            version == VM_SCHEMA_VERSION,
            "VM catalog must be opened by the current controller"
        );
        Ok(Self {
            db: Mutex::new(db),
            _database: database,
        })
    }

    pub(crate) fn list(&self, id: Option<Id>, now: i64) -> Result<Vec<DashboardVm>> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("VM catalog unavailable"))?;
        let tx = db.transaction()?;
        check_size(&tx)?;
        let mut stmt = tx.prepare(
            "SELECT v.id,v.template,t.guest,v.state,v.expires_ms,
                    v.dashboard_cpus,v.dashboard_memory,
                    (SELECT coalesce(sum(d.persistent),0) FROM vm_disk_claims d
                     WHERE d.vm=v.id AND d.snapshot IS NULL),
                    EXISTS(SELECT 1 FROM activity a WHERE a.vm=v.id AND a.status=0 AND a.action=5)
             FROM vm_instances v JOIN vm_templates t ON t.id=v.template
             WHERE (?1 IS NULL OR v.id=?1) ORDER BY v.id LIMIT 1024",
        )?;
        let rows = stmt.query_map([id], |row| {
            let expires = row.get::<_, i64>(4)?;
            let state = match (
                row.get::<_, i64>(3)?,
                expires <= now,
                row.get::<_, bool>(8)?,
            ) {
                (0..=4, true, _) | (4, _, _) => "destroying",
                (0, _, _) => "creating",
                (1 | 2, _, true) => "snapshotting",
                (1, _, _) => "stopped",
                (2, _, _) => "running",
                (3, _, _) => "failed",
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(DashboardVm {
                id: row.get(0)?,
                template: row.get(1)?,
                guest: match row.get::<_, i64>(2)? {
                    1 => Guest::UbuntuX86_64,
                    2 => Guest::WindowsX86_64,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                },
                state,
                expires_at_ms: expires,
                cpus: row.get(5)?,
                memory: row.get(6)?,
                disk: row.get(7)?,
                snapshots: Vec::new(),
            })
        })?;
        let mut vms = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut snapshots = tx.prepare(
            "SELECT s.id,s.dashboard_created_ms,
                    (SELECT sum(d.persistent) FROM vm_disk_claims d WHERE d.vm=s.vm AND d.snapshot=s.id)
             FROM vm_snapshots s WHERE s.vm=?1 AND s.ready=1
             UNION ALL SELECT id,captured_ms,charge FROM vm_restore_points WHERE vm=?1 AND state='ready'
             ORDER BY 1 LIMIT 256",
        )?;
        for vm in &mut vms {
            vm.snapshots = snapshots
                .query_map([vm.id], |row| {
                    Ok(DashboardSnapshot {
                        id: row.get(0)?,
                        created_at_ms: row.get(1)?,
                        size: row.get(2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        Ok(vms)
    }
}

fn check_existing_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            crate::config::checked_path(path)?;
            ensure!(meta.is_file(), "regular VM state file required");
            crate::security::private(path)?;
            state_identity(&state_options().open(path)?)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn state_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        // Deny deletion/replacement while an identity guard is held.
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}

fn verify_state_identity(file: &fs::File, path: &Path) -> Result<()> {
    ensure!(check_existing_file(path)?, "VM state file disappeared");
    ensure!(
        state_identity(file)? == state_identity(&state_options().open(path)?)?,
        "VM state file identity changed"
    );
    Ok(())
}

#[cfg(unix)]
fn state_identity(file: &fs::File) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    ensure!(
        m.is_file() && m.nlink() == 1,
        "single-link regular VM state file required"
    );
    Ok((m.dev(), m.ino()))
}

#[cfg(windows)]
fn state_identity(file: &fs::File) -> Result<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    windows_identity(file.as_raw_handle())
}

#[cfg(windows)]
fn windows_identity(handle: windows_sys::Win32::Foundation::HANDLE) -> Result<(u64, u64)> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
    };
    ensure!(
        !handle.is_null() && handle != INVALID_HANDLE_VALUE,
        "invalid VM state handle"
    );
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // Both the retained Rust file and SQLite's borrowed handle outlive this query.
    ensure!(
        unsafe {
            GetFileType(handle) == FILE_TYPE_DISK
                && GetFileInformationByHandle(handle, &mut info) != 0
        },
        "unavailable VM state file identity"
    );
    ensure!(
        info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0
            && info.nNumberOfLinks == 1,
        "single-link regular VM state file required"
    );
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

fn sqlite_main_identity(db: &mut Connection) -> Result<(u64, u64)> {
    use std::ffi::CStr;

    // The bundled extension qualifies both the native VFS and the main file's
    // I/O methods before we use either platform's file-control contract.
    // The exclusive connection borrow spans qualification and measurement.
    let handle = unsafe { db.handle() };
    ensure!(
        unsafe {
            ffi::sqlite3_libversion_number() == 3050002
                && CStr::from_ptr(ffi::sqlite3_sourceid()).to_bytes() == b"2025-06-28 14:00:48 2af157d77fb1304a74176eaee7fbc7c7e932d946bf25325e9c26c91db19e3079"
                && ffi::sqlite3_arxcovo_bound_abi_v1() == 1
                && ffi::sqlite3_arxcovo_qualify_main_v1(handle) == ffi::SQLITE_OK
        },
        "unqualified SQLite VM main database"
    );
    #[cfg(unix)]
    {
        const MAGIC: u32 = 0x41525849;
        const _: () = assert!(size_of::<ffi::ArxIdentityV1>() == 80);
        let mut identity = ffi::ArxIdentityV1 {
            magic: MAGIC,
            abi_version: 1,
            struct_bytes: 80,
            status: u32::MAX,
            os_errno: 0,
            vfs_kind: 0,
            dev_lo: 0,
            dev_hi: 0,
            ino_lo: 0,
            ino_hi: 0,
            nlink_lo: 0,
            nlink_hi: 0,
            mode: 0,
            flags: 0,
            reserved: [0; 6],
        };
        // The pinned Unix operation fstats the actual descriptor and writes
        // exactly the versioned result structure supplied here.
        let rc = unsafe {
            ffi::sqlite3_file_control(
                handle,
                c"main".as_ptr(),
                ffi::ARX_FCNTL_IDENTITY_V1,
                std::ptr::from_mut(&mut identity).cast(),
            )
        };
        let vfs_kind = if cfg!(target_os = "macos") { 2 } else { 1 };
        ensure!(
            rc == ffi::SQLITE_OK
                && identity.magic == MAGIC
                && identity.abi_version == 1
                && identity.struct_bytes == 80
                && identity.status == 0
                && identity.os_errno == 0
                && identity.vfs_kind == vfs_kind
                && identity.nlink_lo == 1
                && identity.nlink_hi == 0
                && identity.mode & 0o170077 == 0o100000
                && identity.flags == 0
                && identity.reserved == [0; 6],
            "unavailable SQLite VM main database identity"
        );
        Ok((
            (u64::from(identity.dev_hi) << 32) | u64::from(identity.dev_lo),
            (u64::from(identity.ino_hi) << 32) | u64::from(identity.ino_lo),
        ))
    }
    #[cfg(windows)]
    {
        let mut borrowed: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        let rc = unsafe {
            ffi::sqlite3_file_control(
                handle,
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                std::ptr::from_mut(&mut borrowed).cast(),
            )
        };
        ensure!(
            rc == ffi::SQLITE_OK,
            "unavailable SQLite VM main database handle"
        );
        // SQLite retains ownership. Inspect the handle without adopting or closing it.
        windows_identity(borrowed)
    }
}

pub(crate) fn count(db: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<i64> {
    Ok(db.query_row(sql, args, |row| row.get(0))?)
}

pub(crate) fn headroom(db: &Connection) -> Result<bool> {
    let pages: u64 = db.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let free: u64 = db.pragma_query_value(None, "freelist_count", |row| row.get(0))?;
    let max: u64 = db.pragma_query_value(None, "max_page_count", |row| row.get(0))?;
    Ok(pages
        .checked_sub(free)
        .and_then(|used| max.checked_sub(used))
        .is_some_and(|remaining| remaining >= HEADROOM_PAGES))
}

#[derive(Debug)]
pub(crate) struct StateHeadroom;

impl std::fmt::Display for StateHeadroom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VM state headroom exhausted")
    }
}

impl std::error::Error for StateHeadroom {}

pub(crate) fn require_headroom(db: &Connection) -> Result<()> {
    ensure!(headroom(db)?, StateHeadroom);
    Ok(())
}

pub(crate) fn full(error: &anyhow::Error) -> bool {
    error.is::<StateHeadroom>()
        || matches!(
            error.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(err, _)) if err.code == ErrorCode::DiskFull
        )
}

pub(crate) fn activity(
    db: &Connection,
    principal: &str,
    vm: Id,
    action: Action,
    now: i64,
) -> Result<i64> {
    validate_principal(principal)?;
    db.execute(
        "INSERT INTO activity(principal,vm,action,started_ms,actor,vm_owner)
         VALUES(?1,?2,?3,?4,?5,CASE WHEN ?3 IN (7,10)
         THEN coalesce((SELECT principal FROM vm_instances WHERE id=?2),
            (SELECT principal FROM vm_static_instances WHERE id=?2)) END)",
        params![
            principal,
            vm,
            action as i64,
            now,
            if matches!(
                action,
                Action::Expire | Action::SealTemplate | Action::Recover
            ) {
                "daemon"
            } else {
                "agent"
            }
        ],
    )?;
    Ok(db.last_insert_rowid())
}

pub(crate) fn refusal(
    db: &Connection,
    principal: &str,
    vm: Id,
    action: Action,
    now: i64,
    reason: Refusal,
) -> Result<()> {
    refusal_for(db, principal, Some(vm), action as i64, now, reason)
}

pub(crate) fn refusal_for(
    db: &Connection,
    principal: &str,
    vm: Option<Id>,
    action: i64,
    now: i64,
    reason: Refusal,
) -> Result<()> {
    if headroom(db)? {
        validate_principal(principal)?;
        let vm = match vm {
            Some(id)
                if count(
                    db,
                    "SELECT EXISTS(
                SELECT 1 FROM vm_instances WHERE id=?1 AND principal=?2
                UNION ALL SELECT 1 FROM vm_static_instances WHERE id=?1 AND principal=?2
                UNION ALL SELECT 1 FROM vm_tombstones WHERE id=?1 AND principal=?2
            )",
                    params![id, principal],
                )? != 0 =>
            {
                Some(id)
            }
            _ => None,
        };
        db.execute(
            "INSERT INTO activity(principal,vm,action,started_ms,status,reason,finished_ms,actor,terminal_order)
             VALUES(?1,?2,?3,?4,4,?5,?4,'agent',
               (SELECT coalesce(max(terminal_order),0)+1 FROM activity))",
            params![principal, vm, action, now, reason as i64],
        )?;
        prune(db)?;
        require_headroom(db)?;
    }
    Ok(())
}

pub(crate) fn finish(
    db: &Connection,
    id: i64,
    status: Status,
    now: i64,
    cleanup: Option<bool>,
) -> Result<()> {
    finish_with_script(db, id, status, now, cleanup, None, None)
}

fn finish_with_script(
    db: &Connection,
    id: i64,
    status: Status,
    now: i64,
    cleanup: Option<bool>,
    script: Option<ScriptResult>,
    failure: Option<FailureReason>,
) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE activity SET status=?2,finished_ms=?3,cleanup_confirmed=?4,
           terminal_order=(SELECT coalesce(max(terminal_order),0)+1 FROM activity)
         WHERE id=?1 AND status=0",
            params![id, status.code(), now, cleanup],
        )? == 1,
        "VM activity already terminal"
    );
    let (result, code) = match script {
        Some(ScriptResult::Exited(code)) => (Some("exited"), Some(code)),
        Some(ScriptResult::Unavailable) => (Some("unavailable"), None),
        Some(ScriptResult::TimedOut) => (Some("timed_out"), None),
        Some(ScriptResult::LaunchFailed) => (Some("launch_failed"), None),
        Some(ScriptResult::Terminated) => (Some("terminated"), None),
        None => (None, None),
    };
    db.execute(
        "UPDATE vm_operations SET status=?2,script_result=CASE
            WHEN action=2 AND binding_kind='vm' AND ?2=3 THEN 'interrupted'
            ELSE coalesce(?3,script_result) END,script_exit_code=?4,
            cleanup_confirmed=?5,failure_reason=coalesce(?6,CASE
                WHEN ?2=3 AND ?5=0 THEN 11
                WHEN ?2=3 AND action=2 THEN 9 END)
         WHERE activity_id=?1",
        params![
            id,
            status.code(),
            result,
            code,
            cleanup,
            failure.map(|reason| reason as i64)
        ],
    )?;
    prune(db)
}

pub(crate) fn prune(db: &Connection) -> Result<()> {
    db.execute(
        "DELETE FROM activity WHERE id IN
         (SELECT id FROM activity WHERE status<>0
          ORDER BY (EXISTS(SELECT 1 FROM vm_tombstones WHERE activity_id=activity.id)
                 OR EXISTS(SELECT 1 FROM vm_instances WHERE cleanup_id=activity.id)) DESC,
                   terminal_order DESC LIMIT -1 OFFSET ?1)",
        [MAX_RECEIPTS],
    )?;
    Ok(())
}

fn retry_time(db: &Connection, now: i64) -> Result<i64> {
    Ok(db.query_row(
        "UPDATE vm_retry_clock SET high_water_ms=max(high_water_ms,?1) RETURNING high_water_ms",
        [now],
        |row| row.get(0),
    )?)
}

pub(crate) struct Binding<'a> {
    pub principal: &'a str,
    pub key: RequestId,
    pub subject: Subject<'a>,
    pub action: i64,
}

pub(crate) struct Replay {
    pub operation: OperationRef,
    pub lease_end: Option<i64>,
}

impl Binding<'_> {
    pub(crate) fn replay(
        &self,
        db: &Connection,
        now: i64,
        closed: bool,
    ) -> Result<Result<Option<Replay>, Refusal>> {
        let retry_now = retry_time(db, now)?;
        if self.valid_through() < retry_now {
            return Ok(Err(Refusal::StaleKey));
        }
        let previous = db
            .query_row(
                "SELECT action,vm,activity_id,status,lease_end_ms,operation_id,binding_kind,
                    subject_id,request_digest,recipe_id,recipe_digest FROM vm_operations
             WHERE principal=?1 AND request_key=?2",
                params![self.principal, self.key.key],
                |row| {
                    let matches = row.get::<_, i64>(0)? == self.action
                        && match (self.subject, row.get::<_, String>(6)?.as_str()) {
                            (Subject::Vm(id), "legacy_vm" | "vm") => {
                                row.get::<_, Option<Id>>(1)? == Some(id)
                            }
                            (Subject::Snapshot(id, digest), "snapshot")
                            | (Subject::Template(id, digest), "template") => {
                                row.get::<_, Option<Id>>(7)? == Some(id)
                                    && row.get::<_, Option<[u8; 32]>>(8)? == Some(digest)
                            }
                            (Subject::Recipe(id, digest), "recipe") => {
                                row.get::<_, Option<String>>(9)?.as_deref() == Some(id)
                                    && (digest.is_none()
                                        || row.get::<_, Option<[u8; 32]>>(10)? == digest)
                            }
                            _ => false,
                        };
                    Ok((
                        matches,
                        Replay {
                            operation: OperationRef {
                                id: row.get(5)?,
                                receipt: Receipt {
                                    activity_id: row.get(2)?,
                                    status: Status::from_code(row.get(3)?)?,
                                },
                            },
                            lease_end: row.get(4)?,
                        },
                    ))
                },
            )
            .optional()?;
        if let Some((matches, prior)) = previous {
            return Ok(if matches {
                Ok(Some(prior))
            } else {
                Err(Refusal::KeyCollision)
            });
        }
        if compose::uses_key(db, self.principal, self.key.key)? {
            return Ok(Err(Refusal::KeyCollision));
        }
        if self.key.issued_at_ms > now || compose::compacted_key(db, self.principal, self.key.key)?
        {
            return Ok(Err(Refusal::StaleKey));
        }
        if closed {
            return Ok(Err(Refusal::Shutdown));
        }
        if !headroom(db)? {
            return Ok(Err(Refusal::StateHeadroom));
        }
        db.execute(
            "DELETE FROM vm_operations WHERE status<>0 AND valid_through_ms<?1",
            [retry_now],
        )?;
        if count(db, "SELECT count(*) FROM vm_operations", [])? >= MAX_LEDGER {
            return Ok(Err(Refusal::ReplayFull));
        }
        Ok(Ok(None))
    }

    fn valid_through(&self) -> i64 {
        self.key.issued_at_ms + RETRY_WINDOW_MS
    }

    pub(crate) fn record(
        &self,
        db: &Connection,
        now: i64,
        vm: Option<Id>,
        lease_end: Option<i64>,
    ) -> Result<OperationRef> {
        db.execute(
            "INSERT INTO activity(principal,vm,action,started_ms,actor) VALUES(?1,?2,?3,?4,'agent')",
            params![self.principal, vm, self.action, now],
        )?;
        let activity_id = db.last_insert_rowid();
        let (kind, subject, request, recipe, digest) = match self.subject {
            Subject::Vm(id) => {
                // The existing public clone API still binds its supplied VM ID.
                let kind = if self.action == Action::Clone as i64 {
                    "legacy_vm"
                } else {
                    "vm"
                };
                (kind, (kind == "vm").then_some(id), None, None, None)
            }
            Subject::Snapshot(id, digest) => ("snapshot", Some(id), Some(digest), None, None),
            Subject::Template(id, digest) => ("template", Some(id), Some(digest), None, None),
            Subject::Recipe(id, Some(digest)) => ("recipe", None, None, Some(id), Some(digest)),
            Subject::Recipe(_, None) => anyhow::bail!("missing recipe evidence for new operation"),
        };
        let id = db.query_row(
            "INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,valid_through_ms,
                lease_end_ms,binding_kind,subject_id,request_digest,recipe_id,recipe_digest,script_result)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,CASE WHEN ?4=2 THEN 'legacy' END)
             RETURNING operation_id",
            params![self.principal, self.key.key, vm, self.action, activity_id, self.valid_through(),
                lease_end, kind, subject, request, recipe, digest],
            |row| row.get(0),
        )?;
        Ok(OperationRef {
            id,
            receipt: Receipt {
                activity_id,
                status: Status::Pending,
            },
        })
    }
}

#[allow(dead_code, reason = "foreground MCP projection is wired in 4d")]
pub(crate) fn operation_status(
    db: &Connection,
    principal: &str,
    operation: Id,
    now: i64,
) -> Result<Option<OperationStatus>> {
    let now = retry_time(db, now)?;
    Ok(db
        .query_row(
            // Only immutable ledger evidence names a template. A legacy clone
            // bound to a reusable VM ID has none, so its receipt omits it.
            "SELECT o.action,o.status,o.vm,o.recipe_id,
                CASE WHEN o.action IN (1,12) AND o.binding_kind='template' THEN o.subject_id
                    WHEN o.action=11 AND o.status=1 THEN (SELECT b.template_id FROM vm_build_intents b
                        WHERE b.state='sealed' AND b.recipe_id=o.recipe_id
                            AND b.recipe_digest=o.recipe_digest) END,
                o.lease_end_ms,o.script_result,o.script_exit_code,o.cleanup_confirmed,o.failure_reason,o.snapshot_result
         FROM vm_operations o
         WHERE o.principal=?1 AND o.operation_id=?2 AND (o.status=0 OR o.valid_through_ms>=?3)",
            params![principal, operation, now],
            |row| {
                let action: i64 = row.get(0)?;
                let state = Status::from_code(row.get(1)?)?;
                let (action, stage) = match action {
                    1 => ("create", Some("cloning")),
                    2 => ("run_script", Some("executing")),
                    3 => ("start", Some("booting")),
                    4 => ("stop", Some("stopping")),
                    5 => ("snapshot", Some("queued")),
                    6 => ("restore", Some("queued")),
                    8 => ("extend", None),
                    11 => ("template_build", Some("staging")),
                    12 => ("static_create", Some("cloning")),
                    15 => ("publish", Some("publishing")),
                    16 => ("unpublish", Some("revoking")),
                    18 => ("snapshot_delete", Some("deleting")),
                    _ => return Err(rusqlite::Error::InvalidQuery),
                };
                let script = match row.get::<_, Option<String>>(6)?.as_deref() {
                    Some("exited") => Some(ScriptResult::Exited(row.get(7)?)),
                    Some("unavailable") => Some(ScriptResult::Unavailable),
                    Some("timed_out") => Some(ScriptResult::TimedOut),
                    Some("launch_failed") => Some(ScriptResult::LaunchFailed),
                    Some("terminated") => Some(ScriptResult::Terminated),
                    _ => None,
                };
                // Only the proof persisted with the terminal ledger update counts.
                let cleanup = row
                    .get::<_, Option<bool>>(8)?
                    .map(|confirmed| if confirmed { "confirmed" } else { "unconfirmed" });
                Ok(OperationStatus {
                    id: operation,
                    action,
                    state,
                    stage: stage.filter(|_| state == Status::Pending),
                    cleanup,
                    vm: row.get(2)?,
                    recipe: row.get(3)?,
                    template: if matches!(action, "create" | "template_build" | "static_create") {
                        row.get(4)?
                    } else {
                        None
                    },
                    expires_at_ms: row.get(5)?,
                    snapshot: row.get(10)?,
                    script,
                    failure: row.get::<_, Option<i64>>(9)?.map(FailureReason::from_code).transpose()?,
                })
            },
        )
        .optional()?)
}

#[allow(dead_code, reason = "foreground MCP projection is wired in 4d")]
pub(crate) fn vm_status_rows(
    db: &Connection,
    principal: &str,
    exact: Option<Id>,
    after: Option<Id>,
    limit: u32,
    now: i64,
) -> Result<Vec<VmStatus>> {
    let mut stmt = db.prepare(
        "SELECT v.id,v.template,t.guest,v.state,v.expires_ms,a.cleanup_confirmed
         FROM vm_instances v JOIN vm_templates t ON t.id=v.template
         LEFT JOIN activity a ON a.id=v.cleanup_id
         WHERE v.principal=?1 AND (?2 IS NULL OR v.id=?2) AND (?3 IS NULL OR v.id>?3)
         ORDER BY v.id LIMIT ?4",
    )?;
    let items = stmt.query_map(params![principal, exact, after, limit], |row| {
        let expires: i64 = row.get(4)?;
        let state = match (row.get::<_, i64>(3)?, expires <= now) {
            (0..=4, true) | (4, _) => "teardown",
            (0, _) => "creating",
            (1, _) => "stopped",
            (2, _) => "running",
            (3, _) => "recovering",
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        let cleanup = match (row.get::<_, Option<bool>>(5)?, state) {
            (Some(false), _) => Some("unconfirmed"),
            (_, "teardown" | "recovering") => Some("pending"),
            _ => None,
        };
        Ok(VmStatus {
            id: row.get(0)?,
            template: Some(row.get(1)?),
            guest: Some(match row.get::<_, i64>(2)? {
                1 => Guest::UbuntuX86_64,
                2 => Guest::WindowsX86_64,
                _ => return Err(rusqlite::Error::InvalidQuery),
            }),
            state,
            ready: false,
            expires_at_ms: Some(expires),
            expired_at_ms: None,
            cleanup,
            snapshots: [],
            cleanup_activity: None,
        })
    })?;
    Ok(items.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn disk_bytes(db: &Connection) -> Result<Option<u64>> {
    let mut stmt = db.prepare(
        "SELECT bytes FROM vm_templates
        UNION ALL SELECT persistent FROM vm_disk_claims
        UNION ALL SELECT staging FROM vm_disk_claims
        UNION ALL SELECT staging_bytes FROM vm_build_intents
        UNION ALL SELECT persistent FROM vm_static_instances
        UNION ALL SELECT staging FROM vm_static_instances
        UNION ALL SELECT charge FROM vm_restore_points
        UNION ALL SELECT persistent FROM vm_snapshot_work
        UNION ALL SELECT staging FROM vm_snapshot_work",
    )?;
    let mut rows = stmt.query([])?;
    let mut total = 0u64;
    while let Some(row) = rows.next()? {
        let Some(sum) = total.checked_add(row.get::<_, u64>(0)?) else {
            return Ok(None);
        };
        total = sum;
    }
    Ok(Some(total))
}

pub(crate) fn new_id() -> Result<Id> {
    let mut id = [0; 16];
    getrandom::fill(&mut id).map_err(|_| anyhow::anyhow!("VM identity unavailable"))?;
    Ok(id)
}

fn new_generation(db: &Connection, build: Option<Id>, vm: Option<Id>, ordinal: i64) -> Result<Id> {
    let id = new_id()?;
    db.execute(
        "INSERT INTO vm_generations(id,build_id,vm_id,generation) VALUES(?1,?2,?3,?4)",
        params![id, build, vm, ordinal],
    )?;
    Ok(id)
}

fn plan(db: &Connection, generation: Id, role: &'static str) -> Result<ArtifactSlot> {
    let key = new_id()?;
    db.execute(
        "INSERT INTO vm_artifacts(generation_id,role,object_key,state) VALUES(?1,?2,?3,'planned')",
        params![generation, role, key],
    )?;
    Ok(ArtifactSlot {
        generation,
        role,
        key,
    })
}

fn plan_pair(db: &Connection, generation: Id, disk_role: &'static str) -> Result<PairSlots> {
    Ok(PairSlots {
        disk: plan(db, generation, disk_role)?,
        vars: plan(db, generation, "vars")?,
    })
}

pub(crate) fn reserve_build(
    db: &Connection,
    request: BuildRequest<'_>,
    operation: OperationRef,
    peak: u64,
) -> Result<BuildSlots> {
    let id = new_id()?;
    ensure!(
        db.execute(
            "INSERT INTO vm_build_intents(id,operation_id,recipe_id,recipe_digest,staging_bytes,state)
             SELECT ?1,operation_id,recipe_id,recipe_digest,?3,'pending' FROM vm_operations
             WHERE operation_id=?2 AND principal=?4 AND request_key=?5 AND action=11 AND status=0
               AND activity_id=?6 AND recipe_id=?7 AND recipe_digest=?8",
            params![id, operation.id, peak, request.principal, request.key.key,
                operation.receipt.activity_id, request.recipe, request.digest],
        )? == 1,
        "invalid build reservation binding"
    );
    let golden = new_generation(db, Some(id), None, 1)?;
    let verification = new_generation(db, Some(id), None, 2)?;
    Ok(BuildSlots {
        id,
        pair: plan_pair(db, golden, "disk")?,
        seed: plan(db, golden, "seed")?,
        verification: plan_pair(db, verification, "verification_overlay")?,
    })
}

pub(crate) fn plan_clone(db: &Connection, vm: Id) -> Result<PairSlots> {
    plan_pair(db, new_generation(db, None, Some(vm), 1)?, "disk")
}

pub(crate) fn created(db: &Connection, slot: ArtifactSlot) -> Result<()> {
    ensure!(
        db.execute(
            "UPDATE vm_artifacts SET state='created'
             WHERE generation_id=?1 AND role=?2 AND object_key=?3 AND state IN ('planned','created')",
            params![slot.generation, slot.role, slot.key],
        )? == 1,
        "invalid created artifact"
    );
    Ok(())
}

pub(crate) fn identify(
    db: &Connection,
    slot: ArtifactSlot,
    evidence: ArtifactEvidence,
) -> Result<()> {
    let volume = evidence.identity.volume.to_be_bytes();
    ensure!(
        db.execute(
            "UPDATE vm_artifacts SET state='identified',volume_id=?4,directory_id=?5,file_id=?6,
                content_digest=?7,bytes=?8
             WHERE generation_id=?1 AND role=?2 AND object_key=?3
               AND (state='created' OR (state='identified' AND volume_id=?4 AND directory_id=?5
                    AND file_id=?6 AND content_digest=?7 AND bytes=?8))",
            params![
                slot.generation,
                slot.role,
                slot.key,
                volume,
                evidence.directory,
                evidence.identity.file,
                evidence.pin.sha256,
                evidence.pin.size
            ],
        )? == 1,
        "invalid artifact identity"
    );
    Ok(())
}

pub(crate) fn sealed_pair(
    db: &Connection,
    recipe: &str,
    digest: [u8; 32],
) -> Result<Option<(Id, PairSlots)>> {
    Ok(db.query_row(
        "SELECT t.id,g.id,d.object_key,v.object_key FROM vm_templates t
         JOIN vm_build_intents b ON b.template_id=t.id
         JOIN vm_generations g ON g.build_id=b.id AND g.active=1
         JOIN vm_artifacts d ON d.generation_id=g.id AND d.role='disk' AND d.state='sealed'
         JOIN vm_artifacts v ON v.generation_id=g.id AND v.role='vars' AND v.state='sealed'
         WHERE b.state='sealed' AND b.recipe_id=?1 AND b.recipe_digest=?2
           AND t.recipe_digest=?2 AND t.disk_digest=d.content_digest AND t.vars_digest=v.content_digest",
        params![recipe, digest],
        |row| {
            let generation = row.get(1)?;
            Ok((row.get(0)?, PairSlots {
                disk: ArtifactSlot { generation, role: "disk", key: row.get(2)? },
                vars: ArtifactSlot { generation, role: "vars", key: row.get(3)? },
            }))
        },
    ).optional()?)
}

pub(crate) fn evidence_matches(
    db: &Connection,
    slot: ArtifactSlot,
    proof: ArtifactEvidence,
    state: &str,
) -> Result<bool> {
    Ok(count(db,
        "SELECT EXISTS(SELECT 1 FROM vm_artifacts WHERE generation_id=?1 AND role=?2 AND object_key=?3
         AND volume_id=?4 AND directory_id=?5 AND file_id=?6 AND content_digest=?7 AND bytes=?8 AND state=?9)",
        params![slot.generation, slot.role, slot.key, proof.identity.volume.to_be_bytes(),
            proof.directory, proof.identity.file, proof.pin.sha256, proof.pin.size, state],
    )? != 0)
}

pub(crate) fn publish_build(
    db: &Connection,
    activity_id: i64,
    slots: BuildSlots,
    request: BuildRequest<'_>,
    guest: Guest,
    proof: [ArtifactEvidence; 2],
    now: i64,
) -> Result<()> {
    let [disk, vars] = proof;
    let retained = request
        .retained
        .persistent()
        .ok_or_else(|| anyhow::anyhow!("invalid retained charge"))?;
    let peak = retained
        .checked_add(
            request
                .transient
                .total()
                .ok_or_else(|| anyhow::anyhow!("invalid transient charge"))?,
        )
        .ok_or_else(|| anyhow::anyhow!("invalid build peak"))?;
    ensure!(
        slots.pair.disk.role == "disk"
            && slots.pair.vars.role == "vars"
            && slots.pair.disk.generation == slots.pair.vars.generation
            && disk.identity != vars.identity
            && disk
                .pin
                .size
                .checked_add(vars.pin.size)
                .is_some_and(|bytes| bytes <= request.retained.images)
            && evidence_matches(db, slots.pair.disk, disk, "identified")?
            && evidence_matches(db, slots.pair.vars, vars, "identified")?,
        "invalid sealed artifact evidence"
    );
    let template = slots.id;
    ensure!(db.execute(
        "INSERT INTO vm_templates
         SELECT ?1,?2,b.recipe_digest,?3,?4,?5 FROM vm_build_intents b
         JOIN vm_operations o USING(operation_id)
         JOIN vm_generations g ON g.build_id=b.id AND g.generation=1
         WHERE b.id=?6 AND g.id=?7 AND b.state='pending' AND b.staging_bytes=?13
           AND o.activity_id=?8 AND o.action=11 AND o.status=0 AND b.recipe_id=?9 AND b.recipe_digest=?10
           AND o.recipe_id=b.recipe_id AND o.recipe_digest=b.recipe_digest
           AND o.principal=?11 AND o.request_key=?12",
        params![template, guest as i64, disk.pin.sha256, vars.pin.sha256,
            retained, slots.id, slots.pair.disk.generation, activity_id, request.recipe,
            request.digest, request.principal, request.key.key, peak],
    )? == 1, "invalid build publication binding");
    // The caller has proved release and removed the transient files.
    ensure!(db.execute(
        "DELETE FROM vm_artifacts WHERE generation_id=?1 AND role='seed' AND object_key=?2 AND state='identified'",
        params![slots.pair.disk.generation, slots.seed.key],
    )? == 1, "invalid released seed");
    ensure!(
        count(
            db,
            "SELECT count(*) FROM vm_generations g JOIN vm_artifacts a ON a.generation_id=g.id
         WHERE g.id=?1 AND g.build_id=?2 AND g.generation=2 AND g.active=0
           AND a.object_key IN (?3,?4) AND a.state='identified'",
            params![
                slots.verification.disk.generation,
                slots.id,
                slots.verification.disk.key,
                slots.verification.vars.key
            ],
        )? == 2,
        "invalid released verification pair"
    );
    forget_generation(db, slots.verification.disk.generation)?;
    activate(db, slots.pair, "sealed")?;
    db.execute(
        "UPDATE vm_build_intents SET state='sealed',template_id=?2,staging_bytes=0,health_verified=1 WHERE id=?1",
        params![slots.id, template],
    )?;
    finish(db, activity_id, Status::Succeeded, now, Some(true))?;
    require_headroom(db)
}

fn activate(db: &Connection, pair: PairSlots, state: &str) -> Result<()> {
    ensure!(
        pair.disk.generation == pair.vars.generation
            && db.execute(
                "UPDATE vm_artifacts SET state=?4 WHERE generation_id=?1 AND state='identified'
                 AND object_key IN (?2,?3)",
                params![pair.disk.generation, pair.disk.key, pair.vars.key, state],
            )? == 2,
        "invalid generation pair"
    );
    ensure!(
        db.execute(
            "UPDATE vm_generations SET active=1 WHERE id=?1 AND active=0",
            [pair.disk.generation],
        )? == 1,
        "invalid generation activation"
    );
    Ok(())
}

fn forget_generation(db: &Connection, generation: Id) -> Result<()> {
    db.execute(
        "DELETE FROM vm_artifacts WHERE generation_id=?1",
        [generation],
    )?;
    db.execute("DELETE FROM vm_generations WHERE id=?1", [generation])?;
    Ok(())
}

pub(crate) fn cancel_untouched_build(
    db: &Connection,
    slots: BuildSlots,
    activity_id: i64,
    now: i64,
) -> Result<()> {
    ensure!(
        count(
            db,
            "SELECT count(*) FROM vm_build_intents b JOIN vm_operations o USING(operation_id)
         WHERE b.id=?1 AND b.state='pending' AND o.activity_id=?2
           AND NOT EXISTS(SELECT 1 FROM vm_artifacts a JOIN vm_generations g ON g.id=a.generation_id
               WHERE g.build_id=b.id AND a.state<>'planned')",
            params![slots.id, activity_id],
        )? == 1,
        "invalid untouched build"
    );
    let mut stmt = db.prepare("SELECT id FROM vm_generations WHERE build_id=?1")?;
    for generation in stmt
        .query_map([slots.id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
    {
        forget_generation(db, generation)?;
    }
    db.execute("DELETE FROM vm_build_intents WHERE id=?1", [slots.id])?;
    finish_with_script(
        db,
        activity_id,
        Status::Failed,
        now,
        Some(true),
        None,
        Some(FailureReason::Cancelled),
    )
}

pub(crate) fn abandon_build(db: &Connection, id: Id, activity_id: i64, now: i64) -> Result<()> {
    abandon_build_with(db, id, activity_id, now, None)
}

/// As [`abandon_build`], recording the worker's proven cause.
pub(crate) fn abandon_build_with(
    db: &Connection,
    id: Id,
    activity_id: i64,
    now: i64,
    cause: Option<FailureReason>,
) -> Result<()> {
    ensure!(count(db,
        "SELECT count(*) FROM vm_build_intents WHERE id=?1 AND state='pending'
         AND operation_id=(SELECT operation_id FROM vm_operations WHERE activity_id=?2 AND status=0)",
        params![id, activity_id],
    )? == 1, "invalid interrupted build");
    let mut stmt = db.prepare("SELECT id FROM vm_generations WHERE build_id=?1")?;
    for generation in stmt.query_map([id], |row| row.get(0))? {
        abandon_inactive(db, Some(generation?))?;
    }
    classify_abandoned_builds(db, Some(id))?;
    finish_with_script(
        db,
        activity_id,
        Status::Interrupted,
        now,
        Some(false),
        None,
        cause,
    )
}

/// Settle a linked clone, recording the worker's proven cause when it
/// supplies no publishable pair.
pub(crate) fn settle_clone(
    db: &Connection,
    activity_id: i64,
    pair: PairSlots,
    proof: Option<[ArtifactEvidence; 2]>,
    before_worker_deadline: bool,
    now: i64,
    cause: Option<FailureReason>,
) -> Result<Status> {
    ensure!(
        pair.disk.generation == pair.vars.generation
            && pair.disk.role == "disk"
            && pair.vars.role == "vars",
        "invalid clone settlement pair"
    );
    let (state, expires, receipt) = db.query_row(
        "SELECT v.state,v.expires_ms,o.status FROM vm_operations o JOIN vm_instances v ON v.id=o.vm
         JOIN vm_generations g ON g.vm_id=v.id
         WHERE o.activity_id=?1 AND o.action=1 AND o.binding_kind='template'
           AND o.subject_id=v.template AND o.principal=v.principal AND g.id=?2 AND g.active=0
           AND (v.state=0 OR v.cleanup_route='staging')",
        params![activity_id, pair.disk.generation],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                Status::from_code(row.get(2)?)?,
            ))
        },
    )?;
    ensure!(
        receipt == Status::Pending || (state == 4 && receipt == Status::Interrupted),
        "invalid clone settlement receipt"
    );

    if let Some([disk, vars]) = proof {
        ensure!(disk.identity != vars.identity, "shared clone artifact");
        for (slot, evidence) in [(pair.disk, disk), (pair.vars, vars)] {
            // A worker that lost the claim can still hand off identity for
            // its own unpublished slots. Its publication and receipt stay fenced.
            if state == 4 && evidence_matches(db, slot, evidence, "pending_disposal")? {
                continue;
            }
            if state == 4 {
                db.execute(
                    "UPDATE vm_artifacts SET state='created'
                     WHERE generation_id=?1 AND role=?2 AND object_key=?3
                       AND state='quarantined' AND file_id IS NULL",
                    params![slot.generation, slot.role, slot.key],
                )?;
            }
            if !evidence_matches(db, slot, evidence, "identified")? {
                created(db, slot)?;
                identify(db, slot, evidence)?;
            }
        }
    }

    let status = if state == 0 && expires > now && before_worker_deadline && proof.is_some() {
        activate(db, pair, "active")?;
        snapshot::initial_release(db, pair.disk.generation)?;
        settle_operation(db, activity_id, now, Completion::Succeeded)?
    } else {
        abandon_inactive(db, Some(pair.disk.generation))?;
        if receipt == Status::Pending {
            let cause = cause.filter(|_| proof.is_none());
            settle_with_script(
                db,
                activity_id,
                now,
                Completion::CleanupUnconfirmed,
                None,
                cause,
            )?;
        }
        Status::Interrupted
    };
    require_headroom(db)?;
    Ok(status)
}

fn abandon_inactive(db: &Connection, generation: Option<Id>) -> Result<()> {
    // Identity is custody evidence, never evidence that a process exited.
    // Keep every charge and never publish an incomplete generation.
    // Typed snapshot candidates keep their own durable leaf classification.
    db.execute(
        "UPDATE vm_artifacts SET state=CASE WHEN state='identified' THEN 'pending_disposal' ELSE 'quarantined' END
         WHERE state IN ('planned','created','identified') AND generation_id IN
            (SELECT id FROM vm_generations WHERE active=0 AND static_id IS NULL AND (?1 IS NULL OR id=?1)
             AND id NOT IN (SELECT candidate FROM vm_snapshot_work WHERE candidate IS NOT NULL))",
        [generation],
    )?;
    Ok(())
}

/// Preserve all charges on uncertainty, including on restart.
pub(crate) fn recover(db: &Connection, now: i64) -> Result<()> {
    purge_tombstones(db, now)?;
    snapshot::recover(db, now)?;
    abandon_inactive(db, None)?;
    classify_abandoned_builds(db, None)?;
    static_vm::recover(db, now)?;
    publish::recover(db, now)?;
    db.execute("UPDATE vm_instances SET state=4 WHERE state=0", [])?;
    let mut stmt = db.prepare(
        "SELECT id FROM vm_instances WHERE state IN (1,2)
        AND (state=2 OR id IN (SELECT vm FROM activity WHERE status=0 AND action<=6)) LIMIT 8",
    )?;
    let interrupted = stmt
        .query_map([], |row| row.get::<_, Id>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for vm in interrupted {
        begin_recovery(db, vm, now)?;
    }
    let mut stmt = db.prepare(
        "SELECT id FROM activity WHERE status=0 AND (action<=6 OR action IN (11,12,18)) ORDER BY id LIMIT ?1",
    )?;
    let pending = stmt
        .query_map([MAX_CALLER_PENDING], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in pending {
        finish(db, id, Status::Interrupted, now, Some(false))?;
    }
    Ok(())
}

fn classify_abandoned_builds(db: &Connection, id: Option<Id>) -> Result<()> {
    db.execute(
        "UPDATE vm_build_intents SET state=CASE WHEN EXISTS(
            SELECT 1 FROM vm_artifacts a JOIN vm_generations g ON g.id=a.generation_id
            WHERE g.build_id=vm_build_intents.id AND a.state='quarantined')
         THEN 'quarantined' ELSE 'pending_disposal' END
         WHERE state='pending' AND (?1 IS NULL OR id=?1)",
        [id],
    )?;
    Ok(())
}

fn begin_recovery(db: &Connection, vm: Id, now: i64) -> Result<()> {
    let id = activity(db, "arxd", vm, Action::Recover, now)?;
    db.execute(
        "UPDATE vm_instances SET state=3,cleanup_id=?2 WHERE id=?1",
        params![vm, id],
    )?;
    Ok(())
}

pub(crate) fn settle_operation(
    db: &Connection,
    id: i64,
    now: i64,
    completion: Completion,
) -> Result<Status> {
    settle_with_script(db, id, now, completion, None, None)
}

/// Settle a VM worker. `cause` is the worker's proven, closed failure cause;
/// it is recorded independently of whether release is proven.
pub(crate) fn settle_with_script(
    db: &Connection,
    id: i64,
    now: i64,
    completion: Completion,
    script: Option<ScriptResult>,
    cause: Option<FailureReason>,
) -> Result<Status> {
    if let Some(status) = static_vm::settle_operation(db, id, now, completion, script, cause)? {
        return Ok(status);
    }
    let (vm, action, state, expires_ms): (Id, i64, i64, i64) = db.query_row(
        "SELECT a.vm,a.action,v.state,v.expires_ms FROM activity a JOIN vm_instances v ON a.vm=v.id
         WHERE a.id=?1 AND a.status=0 AND a.action<=4",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    ensure!(
        script.is_none() || action == Action::Script as i64,
        "invalid script worker"
    );
    let clone_due = action == Action::Clone as i64 && now >= expires_ms;
    if completion == Completion::CleanupUnconfirmed || state == 4 || clone_due {
        if state == 0 {
            db.execute("UPDATE vm_instances SET state=4 WHERE id=?1", [vm])?;
        } else if state != 4 {
            begin_recovery(db, vm, now)?;
        }
        finish_with_script(db, id, Status::Interrupted, now, Some(false), None, cause)?;
        return Ok(Status::Interrupted);
    }

    let status = if completion == Completion::Succeeded {
        Status::Succeeded
    } else {
        Status::Failed
    };
    let state = match (completion, action) {
        (Completion::Succeeded, x) if x == Action::Start as i64 => 2,
        (Completion::Succeeded, x) if x == Action::Stop as i64 || x == Action::Clone as i64 => 1,
        // Failure still requires teardown proof for partially created instances.
        (Completion::Failed, x) if x == Action::Clone as i64 => 4,
        _ => state,
    };
    db.execute(
        "UPDATE vm_instances SET state=?2 WHERE id=?1",
        params![vm, state],
    )?;
    release_staging(db, id)?;
    let failure = match (status, action) {
        (Status::Failed, 1) => Some(cause.unwrap_or(FailureReason::CloneFailed)),
        (Status::Failed, 3) => Some(cause.unwrap_or(FailureReason::BootFailed)),
        (Status::Failed, 4) => Some(cause.unwrap_or(FailureReason::DirtyStop)),
        (Status::Failed, _) => cause,
        _ => None,
    };
    // A failed stop has not proven the boot released.
    let cleanup = !(status == Status::Failed && action == Action::Stop as i64);
    finish_with_script(db, id, status, now, Some(cleanup), script, failure)?;
    Ok(status)
}

fn release_staging(db: &Connection, id: i64) -> Result<()> {
    db.execute(
        "DELETE FROM vm_disk_claims WHERE activity_id=?1 AND persistent=0",
        [id],
    )?;
    db.execute(
        "UPDATE vm_disk_claims SET staging=0 WHERE activity_id=?1",
        [id],
    )?;
    Ok(())
}

pub(crate) fn expiry_claims(db: &Connection, now: i64) -> Result<Vec<ExpiryClaim>> {
    purge_tombstones(db, now)?;
    claim_staging(db, now)?;
    let mut stmt = db.prepare(
        "SELECT id,cleanup_id FROM vm_instances v WHERE state<>4 AND expires_ms<=?1
        ORDER BY expires_ms,id LIMIT 8",
    )?;
    let due = stmt
        .query_map([now], |row| {
            Ok((row.get::<_, Id>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    due.into_iter()
        .map(|(vm, previous)| begin_expiry(db, vm, previous, now))
        .collect()
}

/// Select only a committed daemon-owned TEST clone. Unpublished clone
/// generations still belong to the normal staging recovery route.
pub(crate) fn vault_test_expiry(db: &Connection, vm: Id, now: i64) -> Result<Option<ExpiryClaim>> {
    let row: Option<(i64, Option<i64>, Option<String>)> = db
        .query_row(
            "SELECT state,cleanup_id,cleanup_route FROM vm_instances
             WHERE id=?1 AND principal='arxd' AND expires_ms<=?2",
            params![vm, now],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    match row {
        Some((1..=3, previous, _)) => begin_expiry(db, vm, previous, now).map(Some),
        Some((4, Some(activity_id), Some(route))) if route == "lease" => {
            Ok(Some(ExpiryClaim { vm, activity_id }))
        }
        _ => Ok(None),
    }
}

fn begin_expiry(db: &Connection, vm: Id, previous: Option<i64>, now: i64) -> Result<ExpiryClaim> {
    if let Some(previous) = previous {
        db.execute(
            "UPDATE vm_instances SET state=4,cleanup_route='lease',cleanup_id=NULL WHERE id=?1",
            [vm],
        )?;
        finish(db, previous, Status::Interrupted, now, Some(false))?;
    }
    let activity_id = activity(db, "arxd", vm, Action::Expire, now)?;
    db.execute(
        "UPDATE vm_instances SET state=4,cleanup_route='lease',cleanup_id=?2 WHERE id=?1",
        params![vm, activity_id],
    )?;
    snapshot::claim_expiry(db, vm, activity_id, now)?;
    Ok(ExpiryClaim { vm, activity_id })
}

pub(crate) fn purge_tombstones(db: &Connection, now: i64) -> Result<()> {
    db.execute(
        "DELETE FROM vm_tombstones WHERE expired_at_ms<=?1",
        [now - TOMBSTONE_LIFETIME_MS],
    )?;
    Ok(())
}

pub(crate) fn record_expired(db: &Connection, claim: ExpiryClaim, now: i64) -> Result<()> {
    ensure!(
        !snapshot::expiry_waits(db, claim.vm)?,
        "snapshot expiry work remains"
    );
    purge_tombstones(db, now)?;
    db.execute(
        "INSERT INTO vm_tombstones(id,principal,activity_id,expired_at_ms)
        SELECT id,principal,cleanup_id,?2 FROM vm_instances WHERE id=?1",
        params![claim.vm, now],
    )?;
    db.execute(
        "DELETE FROM vm_tombstones WHERE id IN
        (SELECT id FROM vm_tombstones ORDER BY (id=?2) DESC, expired_at_ms DESC, activity_id DESC
         LIMIT -1 OFFSET ?1)",
        params![MAX_TOMBSTONES, claim.vm],
    )?;
    forget_generations(db, claim.vm, false)?;
    db.execute("DELETE FROM vm_instances WHERE id=?1", [claim.vm])?;
    finish(db, claim.activity_id, Status::Succeeded, now, Some(true))
}

pub(crate) fn expired_status(
    db: &Connection,
    principal: &str,
    vm: Id,
) -> Result<Option<ExpiredVm>> {
    Ok(db
        .query_row(
            "SELECT expired_at_ms,activity_id FROM vm_tombstones WHERE id=?1 AND principal=?2",
            params![vm, principal],
            |row| {
                Ok(ExpiredVm {
                    expired_at_ms: row.get(0)?,
                    activity_id: row.get(1)?,
                })
            },
        )
        .optional()?)
}

/// The tombstone's expiry receipt and its cleanup proof, only when the same
/// join attributes that receipt to this VM, owner and daemon expiry. An
/// attributed legacy receipt can still carry no proof.
pub(crate) fn tombstone_proof(
    db: &Connection,
    principal: &str,
    vm: Id,
) -> Result<Option<(i64, Option<bool>)>> {
    Ok(db
        .query_row(
            "SELECT a.id,a.cleanup_confirmed FROM vm_tombstones t JOIN activity a ON a.id=t.activity_id
         WHERE t.id=?1 AND t.principal=?2 AND a.vm=t.id AND a.vm_owner=t.principal
           AND a.actor='daemon' AND a.action=7",
            params![vm, principal],
            |row| Ok((row.get(0)?, row.get::<_, Option<bool>>(1)?)),
        )
        .optional()?)
}

pub(crate) fn expiry_intents(db: &Connection) -> Result<Vec<ExpiryClaim>> {
    let mut stmt = db.prepare(
        "SELECT id,cleanup_id FROM vm_instances WHERE cleanup_route='lease' AND cleanup_id IS NOT NULL
         ORDER BY id LIMIT 8",
    )?;
    Ok(stmt
        .query_map([], |row| {
            Ok(ExpiryClaim {
                vm: row.get(0)?,
                activity_id: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn recovery_intents(db: &Connection) -> Result<Vec<RecoveryClaim>> {
    let mut stmt =
        db.prepare("SELECT id,cleanup_id FROM vm_instances WHERE state=3 ORDER BY id LIMIT 8")?;
    Ok(stmt
        .query_map([], |row| {
            Ok(RecoveryClaim {
                vm: row.get(0)?,
                activity_id: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn has_failed_clone_cleanup(db: &Connection) -> Result<bool> {
    Ok(count(
        db,
        "SELECT EXISTS(SELECT 1 FROM vm_instances v
        JOIN vm_generations g ON g.vm_id=v.id WHERE v.cleanup_route='staging')",
        [],
    )? != 0)
}

fn claim_staging(db: &Connection, now: i64) -> Result<()> {
    // Use the staging route for an unpublished durable clone, including
    // when its lease became due while the clone worker was still live.
    db.execute(
        "UPDATE vm_instances AS v SET state=4,cleanup_route='staging'
         WHERE state=0 AND expires_ms<=?1
           AND EXISTS(SELECT 1 FROM vm_generations WHERE vm_id=v.id)",
        [now],
    )?;
    let mut stmt = db.prepare(
        "SELECT v.id,v.cleanup_id,coalesce(a.status=0,0)
         FROM vm_instances v LEFT JOIN activity a ON a.id=v.cleanup_id
         WHERE v.cleanup_route='staging' AND (v.cleanup_id IS NULL OR a.action=7)
         ORDER BY v.id LIMIT 8",
    )?;
    let staging = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, Id>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (vm, old_expiry, pending) in staging {
        let mut stmt =
            db.prepare("SELECT id FROM vm_generations WHERE vm_id=?1 AND active=0 LIMIT 2")?;
        let generations = stmt
            .query_map([vm], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<Id>>>()?;
        for generation in generations {
            abandon_inactive(db, Some(generation))?;
        }
        let id = activity(db, "arxd", vm, Action::Recover, now)?;
        db.execute(
            "UPDATE vm_instances SET cleanup_id=?2 WHERE id=?1",
            params![vm, id],
        )?;
        if let Some(old) = old_expiry.filter(|_| pending) {
            finish(db, old, Status::Interrupted, now, Some(false))?;
        }
    }
    Ok(())
}

pub(crate) fn cleanup_kinds(db: &Connection) -> Result<Vec<CleanupKind>> {
    let mut builds = db.prepare(
        "SELECT id FROM vm_build_intents WHERE state IN ('pending_disposal','quarantined')
         ORDER BY id LIMIT 4",
    )?;
    let mut items = builds
        .query_map([], |row| Ok(CleanupKind::Build(row.get(0)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut staging = db.prepare(
        "SELECT id,cleanup_id FROM vm_instances WHERE cleanup_route='staging' AND cleanup_id IS NOT NULL
         ORDER BY id LIMIT 8",
    )?;
    items.extend(
        staging
            .query_map([], |row| {
                Ok(CleanupKind::Staging {
                    vm: row.get(0)?,
                    activity_id: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    items.extend(expiry_intents(db)?.into_iter().map(CleanupKind::from));
    items.extend(recovery_intents(db)?.into_iter().map(CleanupKind::from));
    items.extend(snapshot::cleanup_kinds(db)?);
    Ok(items)
}

pub(crate) fn legacy_cleanup_uncertain(db: &Connection) -> Result<bool> {
    Ok(count(
        db,
        "SELECT count(*) FROM vm_instances WHERE cleanup_route='legacy_unknown'",
        [],
    )? > 0)
}

pub(crate) fn cleanup_work(db: &Connection, kind: CleanupKind) -> Result<Option<CleanupWork>> {
    if let CleanupKind::Snapshot { vm, generation } = kind {
        return snapshot::cleanup_work(db, kind, vm, generation);
    }
    let id = kind.subject();
    let build = matches!(kind, CleanupKind::Build(_));
    let retain = matches!(kind, CleanupKind::Recovery { .. });
    let (activity_id, state, route) = match kind {
        CleanupKind::Build(_) => (None, None, None),
        CleanupKind::Staging { activity_id, .. } => (Some(activity_id), Some(4), Some("staging")),
        CleanupKind::Expiry { activity_id, .. } => (Some(activity_id), Some(4), Some("lease")),
        CleanupKind::Recovery { activity_id, .. } => (Some(activity_id), Some(3), None),
        CleanupKind::Snapshot { .. } => unreachable!("snapshot authority checked separately"),
    };
    let owned = if build {
        count(
            db,
            "SELECT count(*) FROM vm_build_intents WHERE id=?1 AND state IN
            ('pending_disposal','quarantined') AND template_id IS NULL
            AND NOT EXISTS(SELECT 1 FROM vm_templates WHERE id=?1)",
            [id],
        )?
    } else {
        count(
            db,
            "SELECT count(*) FROM vm_instances WHERE id=?1 AND cleanup_id=?2
            AND state=?3 AND cleanup_route IS ?4",
            params![id, activity_id, state, route],
        )?
    };
    if owned != 1 {
        return Ok(None);
    }
    if route == Some("lease") && snapshot::expiry_waits(db, id)? {
        // Split expiry's release proof from active media disposal: let the
        // reaper quiesce or force-reap the owned TEST job without disposing
        // active media while snapshot pairs remain.
        let pending_worker = db
            .query_row(
                "SELECT id FROM activity WHERE vm=?1 AND status=0 AND (action<=6 OR action=18)",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        return Ok(Some(CleanupWork {
            kind,
            pending_worker,
            snapshot_phase: None,
            artifacts: Vec::new(),
            accounted: false,
            release_only: true,
        }));
    }
    let mut stmt = db.prepare(
        "SELECT g.id,g.generation,g.active,a.role,a.object_key,a.state,a.volume_id,a.directory_id,a.file_id
         FROM vm_generations g LEFT JOIN vm_artifacts a ON a.generation_id=g.id
         WHERE (?2 AND g.build_id=?1) OR (NOT ?2 AND g.vm_id=?1
            AND (NOT ?3 OR g.active=1))
         ORDER BY g.generation,a.role LIMIT 6",
    )?;
    let mut rows = stmt.query(params![id, build, route != Some("staging")])?;
    let mut layout = Vec::new();
    let mut artifacts = Vec::new();
    let mut complete = true;
    while let Some(row) = rows.next()? {
        let ordinal: i64 = row.get(1)?;
        let active: bool = row.get(2)?;
        let (Some(role), Some(state)): (Option<String>, Option<String>) =
            (row.get(3)?, row.get(5)?)
        else {
            complete = false;
            break;
        };
        let directory = row.get::<_, Option<Vec<u8>>>(7)?.and_then(identity_bytes);
        let file = row.get::<_, Option<Vec<u8>>>(8)?.and_then(identity_bytes);
        let volume = row.get::<_, Option<[u8; 8]>>(6)?.map(u64::from_be_bytes);
        layout.push((ordinal, active, role.clone(), state));
        artifacts.push(CleanupArtifact {
            generation: row.get(0)?,
            role,
            key: row.get(4)?,
            directory,
            identity: volume
                .zip(file)
                .map(|(volume, file)| FileIdentity { volume, file }),
            retain: active && retain,
        });
    }
    let expected: &[(i64, &str)] = if build {
        &[
            (1, "disk"),
            (1, "seed"),
            (1, "vars"),
            (2, "vars"),
            (2, "verification_overlay"),
        ]
    } else {
        &[(1, "disk"), (1, "vars")]
    };
    let active = !build && route != Some("staging");
    let accounted = complete
        && layout.len() == expected.len()
        && layout
            .iter()
            .zip(expected)
            .all(|((ordinal, live, role, state), expected)| {
                (!build || *ordinal == expected.0)
                    && role.as_str() == expected.1
                    && *live == active
                    && if active {
                        state == "active"
                    } else {
                        matches!(state.as_str(), "pending_disposal" | "quarantined")
                    }
            })
        && (route != Some("lease")
            || count(
                db,
                "SELECT count(*) FROM vm_generations WHERE vm_id=?1",
                [id],
            )? == 1)
        && (build
            || count(
                db,
                "SELECT count(*) FROM vm_disk_claims WHERE vm=?1 AND snapshot IS NULL
            AND persistent>0 AND (?2 OR staging=0)",
                params![id, !active],
            )? == 1
                && count(db, "SELECT count(*) FROM vm_disk_claims WHERE vm=?1", [id])? == 1
                && count(db, "SELECT count(*) FROM vm_snapshots WHERE vm=?1", [id])? == 0);
    let pending_worker = if build {
        None
    } else {
        db.query_row(
            "SELECT id FROM activity WHERE vm=?1 AND status=0 AND (action<=6 OR action=18)",
            [id],
            |row| row.get(0),
        )
        .optional()?
    };
    Ok(Some(CleanupWork {
        kind,
        pending_worker,
        snapshot_phase: None,
        artifacts,
        accounted,
        release_only: false,
    }))
}

/// Claim exactly the selected objects after independent runtime quiescence.
/// Terminal cleanup phases already exclude publication. A live-copy candidate
/// additionally needs its pending receipt fenced durably before filesystem I/O:
/// a failed disposal or settlement must never revive the publishing worker.
pub(crate) fn claim_disposal(db: &Connection, work: &CleanupWork, now: i64) -> Result<bool> {
    let Some(mut current) = cleanup_work(db, work.kind)? else {
        return Ok(false);
    };
    // Sweeps retains the joined worker ID across receipt settlement. It is
    // a runtime hint; an outstanding durable worker must still match exactly.
    if current.pending_worker.is_none() {
        current.pending_worker = work.pending_worker;
    }
    if &current != work {
        return Ok(false);
    }
    if current.snapshot_phase.as_deref() == Some("copy") {
        let activity = current
            .pending_worker
            .ok_or_else(|| anyhow::anyhow!("missing snapshot copy receipt"))?;
        if count(
            db,
            "SELECT count(*) FROM activity WHERE id=?1 AND status=0",
            [activity],
        )? == 1
        {
            finish(db, activity, Status::Interrupted, now, Some(false))?;
        }
    }
    Ok(true)
}

fn identity_bytes(bytes: Vec<u8>) -> Option<Id> {
    match bytes.len() {
        16 => bytes.try_into().ok(),
        8 => {
            let mut id = [0; 16];
            id[8..].copy_from_slice(&bytes);
            Some(id)
        }
        _ => None,
    }
}

pub(crate) fn settle_cleanup(
    db: &Connection,
    work: &CleanupWork,
    now: i64,
    outcome: crate::vm::reaper::ReconcileOutcome,
) -> Result<bool> {
    if let CleanupKind::Snapshot { vm, generation } = work.kind {
        return snapshot::settle_cleanup(db, work, vm, generation, now, outcome);
    }
    let Some(current) = cleanup_work(db, work.kind)? else {
        return Ok(true);
    };
    if work.release_only {
        if let Some(vm) = work.kind.vm() {
            interrupt_pending(db, vm, now, outcome.released)?;
        }
        if !outcome.released
            && let CleanupKind::Expiry { activity_id, .. } = work.kind
        {
            db.execute(
                "UPDATE activity SET cleanup_confirmed=0 WHERE id=?1",
                [activity_id],
            )?;
        }
        return Ok(false);
    }
    let confirmed = outcome.released
        && outcome.disposed
        && current.accounted
        && current.artifacts == work.artifacts;
    if let Some(vm) = work.kind.vm() {
        interrupt_pending(db, vm, now, confirmed)?;
    }
    match work.kind {
        CleanupKind::Snapshot { .. } => unreachable!("snapshot settlement handled separately"),
        CleanupKind::Build(id) if confirmed => {
            forget_generations(db, id, true)?;
            db.execute("DELETE FROM vm_build_intents WHERE id=?1", [id])?;
        }
        CleanupKind::Build(id) => {
            db.execute(
                "UPDATE vm_build_intents SET state='quarantined' WHERE id=?1",
                [id],
            )?;
        }
        CleanupKind::Staging { vm, activity_id } if confirmed => {
            forget_generations(db, vm, false)?;
            db.execute("DELETE FROM vm_instances WHERE id=?1", [vm])?;
            finish(db, activity_id, Status::Succeeded, now, Some(true))?;
        }
        CleanupKind::Expiry { vm, activity_id } if confirmed => {
            record_expired(db, ExpiryClaim { vm, activity_id }, now)?;
        }
        CleanupKind::Recovery { vm, activity_id } if confirmed => {
            record_recovered(db, RecoveryClaim { vm, activity_id }, now)?;
        }
        CleanupKind::Staging { activity_id, .. }
        | CleanupKind::Expiry { activity_id, .. }
        | CleanupKind::Recovery { activity_id, .. } => {
            db.execute(
                "UPDATE activity SET cleanup_confirmed=0 WHERE id=?1",
                [activity_id],
            )?;
        }
    }
    Ok(confirmed)
}

pub(crate) fn interrupt_pending(db: &Connection, vm: Id, now: i64, released: bool) -> Result<()> {
    let pending = db
        .query_row(
            "SELECT id FROM activity WHERE vm=?1 AND status=0 AND (action<=6 OR action=18)",
            [vm],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = pending {
        finish(db, id, Status::Interrupted, now, Some(released))?;
    }
    Ok(())
}

/// Whether an owned TEST VM has durable generation custody rows.
#[cfg(test)]
pub(crate) fn has_test_generation(db: &Connection, principal: &str, vm: Id) -> Result<bool> {
    Ok(count(
        db,
        "SELECT EXISTS(
            SELECT 1 FROM vm_instances v JOIN vm_generations g ON g.vm_id=v.id
            WHERE v.id=?1 AND v.principal=?2
         )",
        params![vm, principal],
    )? != 0)
}

/// Rehydrate the committed pair of a stopped TEST VM after reopen or commit.
/// The caller re-verifies identity and content before any boot.
#[cfg(test)]
pub(crate) fn active_test_pair(
    db: &Connection,
    principal: &str,
    vm: Id,
) -> Result<Option<(PairSlots, [ArtifactEvidence; 2])>> {
    let mut stmt = db.prepare(
        "SELECT g.id,a.role,a.object_key,a.volume_id,a.directory_id,a.file_id,a.content_digest,a.bytes
         FROM vm_instances v JOIN vm_generations g ON g.vm_id=v.id AND g.active=1
         JOIN vm_artifacts a ON a.generation_id=g.id AND a.state='active'
         WHERE v.id=?1 AND v.principal=?2 AND v.state=1 AND a.role IN ('disk','vars')
         ORDER BY a.role",
    )?;
    let rows = stmt
        .query_map(params![vm, principal], |row| {
            let role = match row.get::<_, String>(1)?.as_str() {
                "disk" => "disk",
                "vars" => "vars",
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            let slot = ArtifactSlot {
                generation: row.get(0)?,
                role,
                key: row.get(2)?,
            };
            let volume = row.get::<_, Option<[u8; 8]>>(3)?.map(u64::from_be_bytes);
            let directory = row.get::<_, Option<Vec<u8>>>(4)?.and_then(identity_bytes);
            let file = row.get::<_, Option<Vec<u8>>>(5)?.and_then(identity_bytes);
            let digest = row.get::<_, Option<[u8; 32]>>(6)?;
            let size = row.get::<_, Option<u64>>(7)?;
            let evidence = match (volume, directory, file, digest, size) {
                (Some(volume), Some(directory), Some(file), Some(sha256), Some(size)) => {
                    Some(ArtifactEvidence {
                        identity: FileIdentity { volume, file },
                        directory,
                        pin: crate::vm::paths::ArtifactPin { size, sha256 },
                    })
                }
                _ => None,
            };
            Ok((slot, evidence))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(match rows[..] {
        [(disk, Some(disk_proof)), (vars, Some(vars_proof))]
            if disk.role == "disk" && vars.role == "vars" =>
        {
            Some((PairSlots { disk, vars }, [disk_proof, vars_proof]))
        }
        _ => None,
    })
}

fn forget_generations(db: &Connection, id: Id, build: bool) -> Result<()> {
    let mut stmt = db.prepare(
        "SELECT id FROM vm_generations
        WHERE (?2 AND build_id=?1) OR (NOT ?2 AND vm_id=?1)",
    )?;
    let generations = stmt
        .query_map(params![id, build], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for generation in generations {
        forget_generation(db, generation)?;
    }
    Ok(())
}

pub(crate) fn record_recovered(db: &Connection, claim: RecoveryClaim, now: i64) -> Result<()> {
    let vm = claim.vm;
    ensure!(db.execute(
        "UPDATE vm_instances SET state=1,cleanup_id=NULL WHERE id=?1 AND state=3 AND cleanup_id=?2",
        params![vm, claim.activity_id],
    )? == 1, "invalid recovery intent");
    db.execute(
        "DELETE FROM vm_disk_claims WHERE vm=?1 AND persistent=0",
        [vm],
    )?;
    db.execute("UPDATE vm_disk_claims SET staging=0 WHERE vm=?1", [vm])?;
    finish(db, claim.activity_id, Status::Succeeded, now, Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::{BUILD_ACTION, static_web::StaticState};

    fn legacy_binding(nonce: u64, vm: Id, action: Action) -> Binding<'static> {
        Binding {
            principal: "alice",
            key: RequestId::parse(&format!("{:016x}{nonce:016x}", 0_u64)).unwrap(),
            subject: Subject::Vm(vm),
            action: action as i64,
        }
    }

    #[test]
    fn legacy_clone_receipt_survives_vm_id_reuse() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store.transact(|db| {
            assert_eq!(count(db, "PRAGMA user_version", [])?, i64::from(VM_SCHEMA_VERSION));
            let vm = [1; 16];
            let [original_template, replacement_template]: [Id; 2] = [[2; 16], [3; 16]];
            for template in [original_template, replacement_template] {
                db.execute(
                    "INSERT INTO vm_templates VALUES(?1,1,?2,zeroblob(32),zeroblob(32),40)",
                    params![template, [template[0]; 32]],
                )?;
            }
            // Publish strictly before expiry; settlement at the deadline is
            // refused, matching the reaper's inclusive `expires_ms <= now`.
            let create = |nonce, template, published_at: i64| -> Result<OperationRef> {
                let operation = legacy_binding(nonce, vm, Action::Clone)
                    .record(db, published_at, Some(vm), None)?;
                db.execute(
                    "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',?3)",
                    params![vm, template, published_at + 60_000],
                )?;
                let settled = settle_operation(
                    db,
                    operation.receipt.activity_id,
                    published_at,
                    Completion::Succeeded,
                )?;
                assert_eq!(settled, Status::Succeeded);
                Ok(operation)
            };
            let original = create(1, original_template, NOW)?;
            let status = |id, now| -> Result<OperationStatus> {
                Ok(operation_status(db, "alice", id, now)?.unwrap())
            };
            let before = status(original.id, NOW)?;
            // The original is claimed at equality with its expiry.
            let expiry = NOW + 60_000;
            for id in 1..=MAX_TOMBSTONES + 1 {
                let instance = [id as u8; 16];
                if instance != vm {
                    db.execute(
                        "INSERT INTO vm_instances(id,template,principal,expires_ms,state) VALUES(?1,?2,'alice',?3,1)",
                        params![instance, original_template, expiry],
                    )?;
                }
                let [claim] = expiry_claims(db, expiry)?.try_into().unwrap();
                assert_eq!(claim.vm, instance);
                record_expired(db, claim, expiry)?;
            }
            assert!(expired_status(db, "alice", vm)?.is_none());
            let reused_at = expiry + 1;
            let replacement = create(2, replacement_template, reused_at)?;
            assert_eq!(status(original.id, reused_at)?, before);
            assert_eq!(before.template, None);
            let replacement = status(replacement.id, reused_at)?;
            assert_ne!(replacement.id, original.id);
            assert_eq!(replacement.vm, Some(vm));
            assert_eq!(replacement.template, None);
            assert_eq!(replacement.state, Status::Succeeded);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn interrupted_operation_keeps_cleanup_proof_after_audit_pruning() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        let settled = store
            .transact(|db| {
                let vm = [1; 16];
                let operation =
                    legacy_binding(1, vm, Action::Start).record(db, NOW, Some(vm), None)?;
                finish(
                    db,
                    operation.receipt.activity_id,
                    Status::Interrupted,
                    NOW,
                    Some(true),
                )?;
                let before = operation_status(db, "alice", operation.id, NOW)?.unwrap();
                assert_eq!(before.cleanup, Some("confirmed"));
                for _ in 0..MAX_RECEIPTS {
                    refusal(db, "alice", vm, Action::Start, NOW, Refusal::Busy)?;
                }
                assert_eq!(
                    count(
                        db,
                        "SELECT count(*) FROM activity WHERE id=?1",
                        [operation.receipt.activity_id]
                    )?,
                    0
                );
                assert_eq!(
                    operation_status(db, "alice", operation.id, NOW)?,
                    Some(before)
                );
                Ok(operation)
            })
            .unwrap();
        drop(store);
        VmStore::open(dir.path(), NOW)
            .unwrap()
            .transact(|db| {
                let after = operation_status(db, "alice", settled.id, NOW)?.unwrap();
                assert_eq!(after.state, Status::Interrupted);
                assert_eq!(after.cleanup, Some("confirmed"));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn reservation_refuses_an_operation_bound_to_another_recipe() {
        let dir = crate::service::application::tests::private_temp();
        let db = VmStore::open(dir.path(), 0).unwrap();
        db.transact(|db| {
            let request = BuildRequest {
                principal: "fixture",
                key: RequestId::parse("00000000000000000000000000000001").unwrap(),
                recipe: "fece98c2030f405fb6402fc92a79935d",
                digest: [9; 32],
                retained: crate::vm::DiskClaim::default(),
                transient: crate::vm::DiskClaim::default(),
            };
            for subject in [
                Subject::Recipe("e03a3012c18d20b8691490b580ab41e9", Some(request.digest)),
                Subject::Recipe(request.recipe, Some([8; 32])),
            ] {
                let binding = Binding {
                    principal: request.principal,
                    key: request.key,
                    subject,
                    action: 11,
                };
                let operation = binding.record(db, 0, None, None)?;
                assert!(reserve_build(db, request, operation, 8).is_err());
                assert_eq!(count(db, "SELECT count(*) FROM vm_build_intents", [])?, 0);
                db.execute("DELETE FROM vm_operations", [])?;
            }
            Ok(())
        })
        .unwrap();
    }
    use crate::service::application::tests::private_temp;

    const NOW: i64 = 100;

    fn fixture_db(dir: &Path) -> Connection {
        let db = Connection::open(dir.join("activity.sqlite3")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                dir.join("activity.sqlite3"),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
        db
    }

    /// Exact v6 state as the merged v6 opener wrote it. Proof for 20 and 21
    /// is retained, 22 has none and 23 was already pruned. Twenty is the oldest
    /// of a full audit window, so settling the pending 300 at startup prunes it.
    fn v6_fixture(dir: &Path) -> Connection {
        let db = fixture_db(dir);
        db.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")
            .unwrap();
        db.execute_batch(SCHEMA_V5).unwrap();
        db.execute_batch(MIGRATE_V6).unwrap();
        db.execute_batch(
            "PRAGMA user_version=6; COMMIT; PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;
             INSERT INTO activity(id,principal,actor,vm,action,started_ms,status,reason,
                 finished_ms,terminal_order,cleanup_confirmed) VALUES
                 (20,'alice','agent',unhex(printf('%032x',1)),3,10,3,NULL,11,1,1),
                 (21,'alice','agent',unhex(printf('%032x',2)),3,12,3,NULL,13,2,0),
                 (22,'alice','agent',unhex(printf('%032x',1)),8,14,1,NULL,15,3,NULL),
                 (300,'alice','agent',unhex(printf('%032x',4)),3,20,0,NULL,NULL,NULL,NULL);
             WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i+1 FROM n WHERE i<124)
             INSERT INTO activity(id,principal,actor,vm,action,started_ms,status,reason,
                 finished_ms,terminal_order)
                 SELECT 100+i,'alice','agent',unhex(printf('%032x',5)),3,16,4,1,16,4+i FROM n;
             INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,
                 valid_through_ms,status,lease_end_ms) VALUES
                 ('alice',unhex(printf('%032x',20)),unhex(printf('%032x',1)),3,20,86400000,3,NULL),
                 ('alice',unhex(printf('%032x',21)),unhex(printf('%032x',2)),3,21,86400000,3,NULL),
                 ('alice',unhex(printf('%032x',22)),unhex(printf('%032x',1)),8,22,86400000,1,400),
                 ('alice',unhex(printf('%032x',23)),unhex(printf('%032x',3)),3,23,86400000,1,NULL),
                 ('alice',unhex(printf('%032x',300)),unhex(printf('%032x',4)),3,300,86400000,0,NULL);
             COMMIT;",
        )
        .unwrap();
        assert_eq!(
            count(&db, "SELECT count(*) FROM activity WHERE status<>0", []).unwrap(),
            MAX_RECEIPTS
        );
        db
    }

    fn ledger_state(db: &Connection) -> Result<Vec<(i64, Id, i64)>> {
        let mut stmt = db.prepare(
            "SELECT activity_id,operation_id,status FROM vm_operations ORDER BY activity_id",
        )?;
        Ok(stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn receipts(db: &Connection, ids: &[(i64, Id, i64)]) -> Result<Vec<OperationStatus>> {
        ids.iter()
            .map(|(_, id, _)| Ok(operation_status(db, "alice", *id, NOW)?.unwrap()))
            .collect()
    }

    fn proof_column(db: &Connection) -> i64 {
        count(
            db,
            "SELECT count(*) FROM pragma_table_info('vm_operations') WHERE name='cleanup_confirmed'",
            [],
        )
        .unwrap()
    }

    #[test]
    fn v6_upgrade_backfills_cleanup_proof_before_recovery_prunes_it() {
        let dir = private_temp();
        let before = ledger_state(&v6_fixture(dir.path())).unwrap();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        let settled = store
            .transact(|db| {
                assert_eq!(
                    count(db, "PRAGMA user_version", [])?,
                    i64::from(VM_SCHEMA_VERSION)
                );
                assert_eq!(count(db, "PRAGMA foreign_keys", [])?, 1);
                assert_eq!(
                    count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?,
                    0
                );
                // Startup recovery settled 300 and pruned the oldest proof row.
                assert_eq!(
                    count(db, "SELECT count(*) FROM activity WHERE id=20", [])?,
                    0
                );
                assert_eq!(
                    count(db, "SELECT count(*) FROM activity WHERE id IN (21,22)", [])?,
                    2
                );
                let after = ledger_state(db)?;
                let expected: Vec<_> = before
                    .iter()
                    .map(|&(activity, id, status)| {
                        (activity, id, if activity == 300 { 3 } else { status })
                    })
                    .collect();
                assert_eq!(after, expected);
                let receipts = receipts(db, &after)?;
                let summary: Vec<_> = receipts
                    .iter()
                    .map(|r| (r.action, r.state, r.cleanup))
                    .collect();
                assert_eq!(
                    summary,
                    [
                        ("start", Status::Interrupted, Some("confirmed")),
                        ("start", Status::Interrupted, Some("unconfirmed")),
                        ("extend", Status::Succeeded, None),
                        ("start", Status::Succeeded, None),
                        ("start", Status::Interrupted, Some("unconfirmed")),
                    ]
                );
                assert_eq!(receipts[2].expires_at_ms, Some(400));
                Ok(receipts)
            })
            .unwrap();
        drop(store);

        let reopened = VmStore::open(dir.path(), NOW).unwrap();
        reopened
            .transact(|db| {
                assert_eq!(
                    count(db, "PRAGMA user_version", [])?,
                    i64::from(VM_SCHEMA_VERSION)
                );
                let state = ledger_state(db)?;
                assert_eq!(receipts(db, &state)?, settled);
                // Proof is limited to 0 and 1 on terminal ledger rows.
                assert!(
                    db.execute(
                        "UPDATE vm_operations SET cleanup_confirmed=2 WHERE activity_id=23",
                        []
                    )
                    .is_err()
                );
                let pending = legacy_binding(9, [9; 16], Action::Start).record(
                    db,
                    NOW,
                    Some([9; 16]),
                    None,
                )?;
                assert!(
                    db.execute(
                        "UPDATE vm_operations SET cleanup_confirmed=1 WHERE activity_id=?1",
                        [pending.receipt.activity_id]
                    )
                    .is_err()
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn v6_upgrade_rolls_back_proof_column_on_recovery_error_and_can_retry() {
        let dir = private_temp();
        let db = v6_fixture(dir.path());
        db.execute_batch(
            "INSERT INTO vm_tombstones(id,principal,activity_id,expired_at_ms)
             VALUES(randomblob(16),'alice',20,1);
             CREATE TRIGGER fixture_error BEFORE DELETE ON vm_tombstones
             BEGIN SELECT raise(ABORT,'fixture migration fault'); END;",
        )
        .unwrap();
        drop(db);
        let path = dir.path().join("activity.sqlite3");
        let before = fs::read(&path).unwrap();
        let error = VmStore::open(dir.path(), NOW + TOMBSTONE_LIFETIME_MS)
            .err()
            .unwrap();
        assert!(error.to_string().contains("fixture migration fault"));
        assert_eq!(fs::read(&path).unwrap(), before);
        let db = open_sqlite(&path).unwrap();
        assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 6);
        assert_eq!(proof_column(&db), 0);
        db.execute_batch("DROP TRIGGER fixture_error").unwrap();
        drop(db);
        VmStore::open(dir.path(), NOW + TOMBSTONE_LIFETIME_MS)
            .unwrap()
            .transact(|db| {
                assert_eq!(proof_column(db), 1);
                assert_eq!(
                    count(
                        db,
                        "SELECT cleanup_confirmed FROM vm_operations WHERE activity_id=20",
                        []
                    )?,
                    1
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn template_clone_receipt_keeps_its_bound_template_after_vm_id_reuse() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store.transact(|db| {
            let vm = [1; 16];
            let [bound, other]: [Id; 2] = [[2; 16], [3; 16]];
            for template in [bound, other] {
                db.execute(
                    "INSERT INTO vm_templates VALUES(?1,1,?2,zeroblob(32),zeroblob(32),40)",
                    params![template, [template[0]; 32]],
                )?;
            }
            let operation = Binding {
                principal: "alice",
                key: RequestId::parse("00000000000000000000000000000001").unwrap(),
                subject: Subject::Template(bound, [7; 32]),
                action: Action::Clone as i64,
            }
            .record(db, NOW, Some(vm), None)?;
            db.execute(
                "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',?3)",
                params![vm, bound, NOW + 1],
            )?;
            settle_operation(db, operation.receipt.activity_id, NOW, Completion::Succeeded)?;
            let receipt = operation_status(db, "alice", operation.id, NOW)?.unwrap();
            assert_eq!(receipt.template, Some(bound));
            assert_eq!(receipt.cleanup, Some("confirmed"));
            db.execute("DELETE FROM vm_disk_claims", [])?;
            db.execute("DELETE FROM vm_instances", [])?;
            db.execute(
                "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',?3)",
                params![vm, other, NOW + 1],
            )?;
            assert_eq!(operation_status(db, "alice", operation.id, NOW)?, Some(receipt));
            Ok(())
        }).unwrap();
    }

    fn v5_fixture(dir: &Path) -> Connection {
        let db = fixture_db(dir);
        db.execute_batch("PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;")
            .unwrap();
        db.execute_batch(SCHEMA_V5).unwrap();
        db.execute_batch(
            "INSERT INTO activity(id,principal,actor,vm_owner,vm,action,started_ms,status,
                  finished_ms,terminal_order,cleanup_confirmed) VALUES
                  (10,'alice','agent',NULL,unhex(printf('%032x',1)),1,10,1,11,1,1),
                  (11,'alice','agent',NULL,unhex(printf('%032x',1)),2,21,0,NULL,NULL,NULL),
                  (12,'bob','agent',NULL,unhex(printf('%032x',2)),1,12,2,13,2,1),
                  (13,'arxd','daemon','bob',unhex(printf('%032x',2)),7,15,0,NULL,NULL,NULL),
                  (14,'arxd','daemon','bob',unhex(printf('%032x',3)),7,16,1,17,3,1),
                  (15,'alice','agent',NULL,unhex(printf('%032x',1)),8,17,1,17,4,NULL),
                  (16,'bob','agent',NULL,unhex(printf('%032x',4)),1,18,0,NULL,NULL,NULL),
                  (17,'bob','agent',NULL,unhex(printf('%032x',5)),1,19,1,20,5,1),
                  (18,'arxd','daemon','bob',unhex(printf('%032x',5)),7,99,0,NULL,NULL,NULL),
                  (19,'arxd','daemon','bob',unhex(printf('%032x',6)),7,99,0,NULL,NULL,NULL);
             INSERT INTO vm_operations(principal,request_key,vm,action,activity_id,valid_through_ms,
                 status,lease_end_ms)
                 SELECT principal,unhex(printf('%032x',id)),vm,action,id,86400000+started_ms,status,
                     CASE WHEN action=8 THEN 400 END FROM activity WHERE actor='agent';
             INSERT INTO vm_operations VALUES
                 ('bob',unhex(printf('%032x',11)),unhex(printf('%032x',3)),2,9,86400009,3,NULL);
             INSERT INTO vm_templates VALUES(zeroblob(16),1,zeroblob(32),zeroblob(32),zeroblob(32),40);
             INSERT INTO vm_instances(id,template,principal,expires_ms,state,cleanup_id) VALUES
                 (unhex(printf('%032x',1)),zeroblob(16),'alice',400,2,NULL),
                 (unhex(printf('%032x',2)),zeroblob(16),'bob',200,4,13),
                 (unhex(printf('%032x',4)),zeroblob(16),'bob',300,0,NULL),
                 (unhex(printf('%032x',5)),zeroblob(16),'bob',99,4,18),
                 (unhex(printf('%032x',6)),zeroblob(16),'bob',99,4,19);
             INSERT INTO vm_tombstones VALUES(unhex(printf('%032x',3)),'bob',14,17);
             INSERT INTO vm_snapshots(vm,id,ready) VALUES(unhex(printf('%032x',1)),zeroblob(16),1);
             INSERT INTO vm_disk_claims VALUES
                 (8,unhex(printf('%032x',6)),NULL,17,0),
                 (10,unhex(printf('%032x',1)),NULL,5,0),
                 (12,unhex(printf('%032x',2)),NULL,7,2),
                 (16,unhex(printf('%032x',4)),NULL,11,3),
                 (17,unhex(printf('%032x',5)),NULL,13,0);
             UPDATE sqlite_sequence SET seq=80 WHERE name='activity';
             UPDATE vm_retry_clock SET high_water_ms=90;
             COMMIT;",
        )
        .unwrap();
        db
    }

    fn ledger_ids(db: &Connection) -> Result<Vec<(i64, Id)>> {
        let mut stmt =
            db.prepare("SELECT activity_id,operation_id FROM vm_operations ORDER BY activity_id")?;
        Ok(stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    #[test]
    fn v9_backfills_only_known_failure_and_scrubs_foreign_refusal() {
        let dir = private_temp();
        let db = v6_fixture(dir.path());
        db.execute_batch(MIGRATE_V7).unwrap();
        db.execute_batch(MIGRATE_V8).unwrap();
        db.execute_batch(
            "INSERT INTO activity(id,principal,actor,vm,action,started_ms,status,reason,finished_ms,terminal_order)
             VALUES(400,'beta','agent',unhex(printf('%032x',2)),3,20,4,6,20,400);
             UPDATE sqlite_sequence SET seq=900 WHERE name='activity';
             PRAGMA user_version=8;"
        ).unwrap();
        let existing = ledger_state(&db).unwrap();
        drop(db);
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                let actual = ledger_state(db)?;
                assert_eq!(
                    actual.iter().map(|row| row.1).collect::<Vec<_>>(),
                    existing.iter().map(|row| row.1).collect::<Vec<_>>()
                );
                let projected = receipts(db, &actual)?;
                assert_eq!(projected[0].failure, None);
                assert_eq!(
                    projected[1].failure,
                    Some(FailureReason::CleanupUnconfirmed)
                );
                assert_eq!(projected[2].expires_at_ms, Some(400));
                assert_eq!(projected[3].failure, None);
                assert_eq!(
                    projected[4].failure,
                    Some(FailureReason::CleanupUnconfirmed)
                );
                assert_eq!(
                    count(
                        db,
                        "SELECT count(*) FROM activity WHERE id=400 AND vm IS NULL",
                        []
                    )?,
                    1
                );
                assert_eq!(
                    count(
                        db,
                        "SELECT seq FROM sqlite_sequence WHERE name='activity'",
                        []
                    )?,
                    900
                );
                assert_eq!(
                    count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?,
                    0
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn v12_keeps_v9_through_v11_activity_replay_and_static_generation() {
        for version in [9, 10, 11] {
            verify_compose_upgrade(version);
        }
    }

    #[test]
    fn v11_direct_v10_rollback_retry_nonempty_v0_and_v10_opener_refusal() {
        // 1. Nonempty version-0 database is refused without altering bytes.
        let dir0 = private_temp();
        let path0 = dir0.path().join("activity.sqlite3");
        {
            let db = Connection::open(&path0).unwrap();
            db.execute_batch("CREATE TABLE stray(x INTEGER);").unwrap();
        }
        let before0 = fs::read(&path0).unwrap();
        assert!(VmStore::open(dir0.path(), NOW).is_err());
        assert_eq!(fs::read(&path0).unwrap(), before0);

        // 2. Direct populated v10 -> v11 rolls back on failure and retries cleanly.
        let dir = private_temp();
        let db = v6_fixture(dir.path());
        db.execute_batch(MIGRATE_V7).unwrap();
        db.execute_batch(MIGRATE_V8).unwrap();
        db.execute_batch(MIGRATE_V9).unwrap();
        let vm = [0x73; 16];
        let slots = static_fixture(&db, vm, "rollback-site").unwrap();
        commit_static(&db, vm, slots, 1).unwrap();
        let generation: Id = db
            .query_row(
                "SELECT id FROM vm_generations WHERE static_id=?1 AND active=1",
                [vm],
                |row| row.get(0),
            )
            .unwrap();
        let binding = Binding {
            principal: "alice",
            key: RequestId {
                key: [0x19; 16],
                issued_at_ms: NOW,
            },
            subject: Subject::Vm(vm),
            action: Action::Start as i64,
        };
        let recorded = binding.record(&db, NOW, Some(vm), None).unwrap();
        finish(
            &db,
            recorded.receipt.activity_id,
            Status::Failed,
            NOW,
            Some(true),
        )
        .unwrap();
        db.execute_batch(
            "UPDATE sqlite_sequence SET seq=900 WHERE name='activity'; PRAGMA user_version=9;",
        )
        .unwrap();
        db.execute_batch(publish::MIGRATE_V10).unwrap();
        db.pragma_update(None, "user_version", 10).unwrap();
        db.execute(
            "INSERT INTO vm_publication(singleton,vm,generation,boot,revision,hostname,
                 tunnel,zone,credential_ref,nonce,phase,post_possible)
             VALUES(1,?1,?2,zeroblob(16),zeroblob(32),'rollback.example.invalid',
                 '00000000-0000-0000-0000-000000000001',?3,'fixture.dpapi',?4,'publishing',1)",
            params![vm, generation, "a".repeat(32), "b".repeat(32)],
        )
        .unwrap();
        db.execute_batch(
            "CREATE TRIGGER fail_v11_upgrade BEFORE UPDATE ON vm_static_instances
             BEGIN SELECT RAISE(ABORT,'injected v11 upgrade failure'); END;",
        )
        .unwrap();
        let expected_ledger = ledger_ids(&db).unwrap();
        drop(db);

        assert!(VmStore::open(dir.path(), NOW).is_err());
        {
            let db = Connection::open(dir.path().join("activity.sqlite3")).unwrap();
            assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 10);
            assert_eq!(
                count(
                    &db,
                    "SELECT count(*) FROM sqlite_schema WHERE name='vm_restore_points'",
                    []
                )
                .unwrap(),
                0
            );
            assert_eq!(
                count(&db, "SELECT count(*) FROM vm_publication", []).unwrap(),
                1
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT seq FROM sqlite_sequence WHERE name='activity'",
                    []
                )
                .unwrap(),
                900
            );
            db.execute_batch("DROP TRIGGER fail_v11_upgrade;").unwrap();
        }

        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                assert_eq!(
                    count(db, "PRAGMA user_version", [])?,
                    i64::from(VM_SCHEMA_VERSION)
                );
                assert_eq!(
                    db.query_row(
                        "SELECT origin_generation FROM vm_static_instances WHERE id=?1",
                        [vm],
                        |row| row.get::<_, Id>(0)
                    )?,
                    generation
                );
                assert_eq!(ledger_ids(db)?, expected_ledger);
                assert_eq!(
                    count(
                        db,
                        "SELECT seq FROM sqlite_sequence WHERE name='activity'",
                        []
                    )?,
                    900
                );
                assert!(
                    publish::current(db)?.expect("v10 route retained").phase
                        == publish::Phase::CleanupPending
                );
                assert_eq!(
                    count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?,
                    0
                );
                Ok(())
            })
            .unwrap();
        drop(store);

        // 3. A v10 opener refuses a copy of the migrated v11 database without altering bytes.
        let copy_dir = private_temp();
        let copy_path = copy_dir.path().join("activity.sqlite3");
        fs::copy(dir.path().join("activity.sqlite3"), &copy_path).unwrap();
        let before_copy = fs::read(&copy_path).unwrap();
        {
            let db =
                Connection::open_with_flags(&copy_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let version: u32 = db
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, VM_SCHEMA_VERSION);
            // Exact v10 VmStore::open and DashboardCatalog::open version gates.
            assert!(!matches!(version, 0 | 5..=10));
            assert_ne!(version, 10);
            // Exact v11 writer and read-only dashboard gates.
            assert!(!matches!(version, 0 | 5..=11));
            assert_ne!(version, 11);
        }
        assert_eq!(fs::read(&copy_path).unwrap(), before_copy);
    }

    #[test]
    fn legacy_snapshot_5_6_keys_collide_with_typed_bindings_without_promotion() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                let template = [0x41; 16];
                let vm = [0x42; 16];
                let legacy_snap = [0x43; 16];
                db.execute(
                    "INSERT INTO vm_templates VALUES(?1,1,zeroblob(32),zeroblob(32),zeroblob(32),1024)",
                    [template],
                )?;
                db.execute(
                    "INSERT INTO vm_instances(id,template,principal,expires_ms,state)
                     VALUES(?1,?2,'alice',?3,1)",
                    params![vm, template, NOW + 60_000],
                )?;
                db.execute(
                    "INSERT INTO vm_snapshots(vm,id,ready,dashboard_created_ms) VALUES(?1,?2,1,?3)",
                    params![vm, legacy_snap, NOW],
                )?;
                let key5 = RequestId {
                    key: [0x55; 16],
                    issued_at_ms: NOW,
                };
                let key6 = RequestId {
                    key: [0x56; 16],
                    issued_at_ms: NOW,
                };
                let legacy5 = Binding {
                    principal: "alice",
                    key: key5,
                    subject: Subject::Vm(vm),
                    action: Action::Snapshot as i64,
                };
                let op5 = legacy5.record(db, NOW, Some(vm), None)?;
                db.execute(
                    "INSERT INTO vm_disk_claims(activity_id,vm,snapshot,persistent,staging)
                     VALUES(?1,?2,?3,4096,0)",
                    params![op5.receipt.activity_id, vm, legacy_snap],
                )?;
                finish(db, op5.receipt.activity_id, Status::Succeeded, NOW, Some(true))?;
                let legacy6 = Binding {
                    principal: "alice",
                    key: key6,
                    subject: Subject::Vm(vm),
                    action: Action::Restore as i64,
                };
                let op6 = legacy6.record(db, NOW, Some(vm), None)?;
                finish(db, op6.receipt.activity_id, Status::Succeeded, NOW, Some(true))?;

                let bytes_before = disk_bytes(db)?;
                let typed5 = Binding {
                    principal: "alice",
                    key: key5,
                    subject: crate::vm::snapshot::Request::Create(None).binding(vm),
                    action: Action::Snapshot as i64,
                };
                assert!(matches!(
                    typed5.replay(db, NOW, false)?,
                    Err(Refusal::KeyCollision)
                ));
                let typed6 = Binding {
                    principal: "alice",
                    key: key6,
                    subject: crate::vm::snapshot::Request::Restore(legacy_snap).binding(vm),
                    action: Action::Restore as i64,
                };
                assert!(matches!(
                    typed6.replay(db, NOW, false)?,
                    Err(Refusal::KeyCollision)
                ));
                assert!(snapshot::list(db, "alice", vm, None, 4)?.unwrap().is_empty());
                assert_eq!(disk_bytes(db)?, bytes_before);
                Ok(())
            })
            .unwrap();
    }

    fn verify_compose_upgrade(version: u32) {
        let dir = private_temp();
        let db = v6_fixture(dir.path());
        db.execute_batch(MIGRATE_V7).unwrap();
        db.execute_batch(MIGRATE_V8).unwrap();
        db.execute_batch(MIGRATE_V9).unwrap();
        let vm = [0x72; 16];
        let slots = static_fixture(&db, vm, "upgrade-site").unwrap();
        commit_static(&db, vm, slots, 1).unwrap();
        // A publication-independent v9 binding and the active generation.
        let binding = Binding {
            principal: "alice",
            key: RequestId {
                key: [9; 16],
                issued_at_ms: NOW,
            },
            subject: Subject::Vm(vm),
            action: Action::Start as i64,
        };
        let recorded = binding.record(&db, NOW, Some(vm), None).unwrap();
        // A settled receipt: a pending start would make the reopen recover.
        finish(
            &db,
            recorded.receipt.activity_id,
            Status::Failed,
            NOW,
            Some(true),
        )
        .unwrap();
        let active = |db: &Connection| -> Result<Vec<Id>> {
            let mut stmt =
                db.prepare("SELECT id FROM vm_generations WHERE static_id=?1 AND active=1")?;
            Ok(stmt
                .query_map([vm], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        };
        let generation = active(&db).unwrap();
        assert_eq!(generation.len(), 1);
        db.execute_batch(
            "UPDATE sqlite_sequence SET seq=900 WHERE name='activity'; PRAGMA user_version=9;",
        )
        .unwrap();
        if version >= 10 {
            db.execute_batch(publish::MIGRATE_V10).unwrap();
            db.pragma_update(None, "user_version", 10).unwrap();
            db.execute(
                "INSERT INTO vm_publication(singleton,vm,generation,boot,revision,hostname,
                     tunnel,zone,credential_ref,nonce,phase,post_possible)
                 VALUES(1,?1,?2,zeroblob(16),zeroblob(32),'site.example.invalid',
                     '00000000-0000-0000-0000-000000000001',?3,'fixture.dpapi',?4,'publishing',1)",
                params![vm, generation[0], "a".repeat(32), "b".repeat(32)],
            )
            .unwrap();
        }
        if version == 11 {
            db.execute_batch(snapshot::MIGRATE_V11).unwrap();
            db.pragma_update(None, "user_version", 11).unwrap();
            let snapshot = Binding {
                principal: "alice",
                key: RequestId {
                    key: [10; 16],
                    issued_at_ms: NOW,
                },
                subject: Subject::Snapshot(vm, [6; 32]),
                action: Action::Snapshot as i64,
            }
            .record(&db, NOW, Some(vm), None)
            .unwrap();
            let point = [0x56_u8; 16];
            db.execute(
                "INSERT INTO vm_generations(id,static_id,generation) VALUES(?1,?2,2)",
                params![point, vm],
            )
            .unwrap();
            db.execute("INSERT INTO vm_restore_points(id,static_vm,host,name,captured_ms,state,provenance,generation,lineage,charge)
                SELECT ?1,id,host,'kept',?3,'ready',origin_generation,?1,zeroblob(32),500 FROM vm_static_instances WHERE id=?2",
                params![point,vm,NOW]).unwrap();
            db.execute(
                "UPDATE vm_operations SET snapshot_result=?1 WHERE activity_id=?2",
                params![point, snapshot.receipt.activity_id],
            )
            .unwrap();
            finish(
                &db,
                snapshot.receipt.activity_id,
                Status::Succeeded,
                NOW,
                Some(true),
            )
            .unwrap();
            // Force a failure halfway through v12 DDL; retry from the populated v11 store.
            db.execute_batch("CREATE TABLE vm_compose_operations(injected INTEGER)")
                .unwrap();
        }
        let ledger = ledger_ids(&db).unwrap();
        let sequence = count(
            &db,
            "SELECT seq FROM sqlite_sequence WHERE name='activity'",
            [],
        )
        .unwrap();
        drop(db);
        if version == 11 {
            assert!(VmStore::open(dir.path(), NOW).is_err());
            let db = Connection::open(dir.path().join("activity.sqlite3")).unwrap();
            assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 11);
            assert_eq!(
                count(
                    &db,
                    "SELECT count(*) FROM sqlite_schema WHERE name='vm_compose_projects'",
                    []
                )
                .unwrap(),
                0
            );
            assert_eq!(ledger_ids(&db).unwrap(), ledger);
            db.execute_batch("DROP TABLE vm_compose_operations")
                .unwrap();
        }
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store.transact(|db| {
            assert_eq!(count(db, "PRAGMA user_version", [])?, i64::from(VM_SCHEMA_VERSION));
            for table in ["projects","releases","volumes","secrets","release_epochs","boots","operations","compacted_keys"] {
                assert_eq!(count(db,&format!("SELECT count(*) FROM vm_compose_{table}"),[])?,0);
            }
            if version==11 {
                assert_eq!(count(db,"SELECT charge FROM vm_restore_points WHERE name='kept' AND state='ready'",[])?,500);
                assert_eq!(count(db,"SELECT count(*) FROM vm_operations o JOIN vm_restore_points p ON o.snapshot_result=p.id
                    JOIN activity a ON a.id=o.activity_id WHERE o.binding_kind='snapshot' AND a.status=1 AND p.static_vm=o.vm",[])?,1);
            }
            assert_eq!(
                db.query_row("SELECT origin_generation FROM vm_static_instances WHERE id=?1",
                    [vm], |row| row.get::<_, Id>(0))?,
                generation[0]
            );
            assert_eq!(ledger_ids(db)?, ledger);
            let replay = binding.replay(db, NOW, false)?.unwrap().unwrap();
            assert_eq!(replay.operation.id, recorded.id);
            assert_eq!(active(db)?, generation);
            assert_eq!(static_vm::owned_state(db, "alice", vm)?, Some(StaticState::Stopped));
            if version >= 10 {
                let route = publish::current(db)?.expect("v10 route retained");
                assert_eq!(route.generation, generation[0]);
                assert_eq!(route.nonce, "b".repeat(32));
                assert!(route.post_possible);
                assert!(route.phase == publish::Phase::CleanupPending);
                assert!(!snapshot::route_quiescent(db, vm)?);
            } else {
                assert!(publish::current(db)?.is_none());
            }
            assert_eq!(count(db, "SELECT count(*) FROM vm_static_instances WHERE publication_failure IS NULL", [])?, 1);
            assert_eq!(count(db, "SELECT seq FROM sqlite_sequence WHERE name='activity'", [])?, sequence);
            assert_eq!(count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?, 0);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn recipe_removed_replays_but_cannot_bind_fresh_build() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                let recipe = "e03a3012c18d20b8691490b580ab41e9";
                let binding = Binding {
                    principal: "alice",
                    key: RequestId {
                        key: [2; 16],
                        issued_at_ms: NOW,
                    },
                    subject: Subject::Recipe(recipe, Some([4; 32])),
                    action: BUILD_ACTION,
                };
                let operation = binding.record(db, NOW, None, None)?;
                let removed = Binding {
                    subject: Subject::Recipe(recipe, None),
                    ..binding
                };
                assert_eq!(
                    removed.replay(db, NOW, false)?.unwrap().unwrap().operation,
                    operation
                );
                let changed = Binding {
                    subject: Subject::Recipe(recipe, Some([5; 32])),
                    ..binding
                };
                assert!(matches!(
                    changed.replay(db, NOW, false)?,
                    Err(Refusal::KeyCollision)
                ));
                let other = Binding {
                    subject: Subject::Recipe("other", None),
                    ..binding
                };
                assert!(matches!(
                    other.replay(db, NOW, false)?,
                    Err(Refusal::KeyCollision)
                ));
                Ok(())
            })
            .unwrap();
        // Failed recording rolls back the audit insert as well as the receipt.
        assert!(
            store
                .transact(|db| Binding {
                    principal: "alice",
                    key: RequestId {
                        key: [3; 16],
                        issued_at_ms: NOW
                    },
                    subject: Subject::Recipe("removed", None),
                    action: BUILD_ACTION,
                }
                .record(db, NOW, None, None))
                .is_err()
        );
        store
            .transact(|db| {
                assert_eq!(count(db, "SELECT count(*) FROM activity", [])?, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn v7_upgrade_preserves_rows_with_null_dashboard_columns() {
        let dir = private_temp();
        let db = v5_fixture(dir.path());
        db.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")
            .unwrap();
        db.execute_batch(MIGRATE_V6).unwrap();
        db.execute_batch(MIGRATE_V7).unwrap();
        // Keep the catalog and audit row counts after recovery stable. This
        // case is about dashboard columns and uses a stopped v7 catalog.
        db.execute_batch(
            "UPDATE vm_instances SET state=1 WHERE state=2;
            UPDATE activity SET status=3,finished_ms=5000,terminal_order=id+100
                WHERE status=0 AND action<=6;
            UPDATE vm_operations SET status=3 WHERE status=0;",
        )
        .unwrap();
        db.execute_batch("PRAGMA user_version=7; COMMIT;").unwrap();
        let rows = |db: &Connection| -> Result<[i64; 4]> {
            Ok([
                count(db, "SELECT count(*) FROM vm_instances", [])?,
                count(db, "SELECT count(*) FROM vm_snapshots", [])?,
                count(db, "SELECT count(*) FROM activity", [])?,
                count(db, "SELECT count(*) FROM vm_operations", [])?,
            ])
        };
        let before = rows(&db).unwrap();
        assert!(before[0] > 0 && before[1] > 0);
        drop(db);
        assert!(DashboardCatalog::open(dir.path()).is_err());
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                assert_eq!(
                    count(db, "PRAGMA user_version", [])?,
                    i64::from(VM_SCHEMA_VERSION)
                );
                assert_eq!(rows(db)?, before);
                assert_eq!(
                    count(
                        db,
                        "SELECT count(*) FROM vm_instances
                         WHERE dashboard_cpus IS NULL AND dashboard_memory IS NULL",
                        []
                    )?,
                    before[0]
                );
                assert_eq!(
                    count(
                        db,
                        "SELECT count(*) FROM vm_snapshots WHERE dashboard_created_ms IS NULL",
                        []
                    )?,
                    before[1]
                );
                Ok(())
            })
            .unwrap();
        drop(store);
        let catalog = DashboardCatalog::open(dir.path()).unwrap();
        let vms = catalog.list(None, NOW).unwrap();
        assert_eq!(vms.len() as i64, before[0]);
        assert!(
            vms.iter()
                .all(|vm| vm.cpus.is_none() && vm.memory.is_none())
        );
        assert!(
            vms.iter()
                .flat_map(|vm| &vm.snapshots)
                .all(|snapshot| snapshot.created_at_ms.is_none())
        );
    }

    #[test]
    fn v5_upgrade_preserves_rows_and_recovers_with_stable_operation_ids() {
        let dir = private_temp();
        drop(v5_fixture(dir.path()));
        let store = VmStore::open(dir.path(), NOW).unwrap();
        let ids = store.transact(|db| {
            assert_eq!(count(db, "PRAGMA user_version", [])?, i64::from(VM_SCHEMA_VERSION));
            // The v5 and v6 VM openers' gates were exactly 0 | 5 and 0 | 5 | 6.
            assert!(!matches!(count(db, "PRAGMA user_version", [])?, 0 | 5 | 6));
            assert_eq!(count(db, "PRAGMA foreign_keys", [])?, 1);
            assert_eq!(count(db, "SELECT count(*) FROM pragma_foreign_key_check", [])?, 0);
            assert_eq!(count(db, "SELECT seq FROM sqlite_sequence WHERE name='activity'", [])?, 81);
            assert_eq!(count(db, "SELECT high_water_ms FROM vm_retry_clock", [])?, 90);
            assert_eq!(disk_bytes(db)?, Some(98));
            assert_eq!(count(db, "SELECT count(*) FROM vm_snapshots WHERE ready=1", [])?, 1);
            assert_eq!(count(db, "SELECT activity_id FROM vm_tombstones WHERE principal='bob' AND expired_at_ms=17", [])?, 14);
            assert_eq!(count(db, "SELECT count(*) FROM vm_instances WHERE (principal='alice' AND
                 expires_ms=400 AND state=3 AND cleanup_id=81) OR (principal='bob' AND state=4 AND
                 cleanup_route='staging' AND ((expires_ms=200 AND cleanup_id=13) OR
                 (expires_ms=300 AND cleanup_id IS NULL)))", [])?, 3);
            assert_eq!(count(db, "SELECT count(*) FROM vm_instances WHERE expires_ms=99 AND state=4
                 AND ((cleanup_id=18 AND cleanup_route='lease')
                 OR (cleanup_id=19 AND cleanup_route='legacy_unknown'))", [])?, 2);
            assert_eq!(count(db, "SELECT count(*) FROM activity WHERE id IN (11,16) AND
                 status=3 AND finished_ms=?1 AND cleanup_confirmed=0", [NOW])?, 2);
            assert_eq!(count(db, "SELECT count(*) FROM vm_operations WHERE action=2 AND
                 script_result='legacy' AND script_exit_code IS NULL AND status=3", [])?, 2);
            assert_eq!(count(db, "SELECT count(*) FROM vm_operations WHERE binding_kind='legacy_vm'
                 AND subject_id IS NULL AND request_digest IS NULL", [])?, 7);
            assert_eq!(count(db, "SELECT count(*) FROM vm_operations WHERE
                 (activity_id IN (10,12,17) AND cleanup_confirmed=1)
                 OR (activity_id IN (11,16) AND cleanup_confirmed=0)
                 OR (activity_id IN (9,15) AND cleanup_confirmed IS NULL)", [])?, 7);

            let key = RequestId::parse("0000000000000000000000000000000b").unwrap();
            for (principal, vm, id) in [("alice", 1, 11), ("bob", 3, 9)] {
                let mut subject = [0; 16];
                subject[15] = vm;
                let replay = Binding { principal, key, subject: Subject::Vm(subject), action: Action::Script as i64 }
                    .replay(db, NOW, false)?.unwrap().unwrap();
                assert_eq!(replay.operation.receipt, Receipt { activity_id: id, status: Status::Interrupted });
            }
            let mut vm = [0; 16];
            vm[15] = 1;
            let extend = Binding {
                principal: "alice", subject: Subject::Vm(vm), action: Action::Extend as i64,
                key: RequestId::parse("0000000000000000000000000000000f").unwrap(),
            }.replay(db, NOW, false)?.unwrap().unwrap();
            assert_eq!(extend.operation.receipt, Receipt { activity_id: 15, status: Status::Succeeded });
            assert_eq!(extend.lease_end, Some(400));

            assert_eq!(count(db, "SELECT count(DISTINCT operation_id) FROM vm_operations", [])?, 7);
            assert!(headroom(db)?);
            ledger_ids(db)
        }).unwrap();
        assert_eq!(
            ids.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [9, 10, 11, 12, 15, 16, 17]
        );
        drop(store);

        let reopened = VmStore::open(dir.path(), NOW).unwrap();
        reopened
            .transact(|db| {
                assert_eq!(ledger_ids(db)?, ids);
                assert_eq!(
                    count(
                        db,
                        "SELECT seq FROM sqlite_sequence WHERE name='activity'",
                        []
                    )?,
                    81
                );
                assert_eq!(
                    count(db, "SELECT count(*) FROM activity WHERE id>80", [])?,
                    1
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn v5_upgrade_rolls_back_on_recovery_error_and_can_retry() {
        let dir = private_temp();
        let db = v5_fixture(dir.path());
        db.execute_batch(
            "CREATE TRIGGER fixture_error BEFORE UPDATE OF state ON vm_instances
             BEGIN SELECT raise(ABORT,'fixture migration fault'); END;",
        )
        .unwrap();
        drop(db);
        let path = dir.path().join("activity.sqlite3");
        let before = fs::read(&path).unwrap();
        let error = VmStore::open(dir.path(), NOW).err().unwrap();
        assert!(error.to_string().contains("fixture migration fault"));
        assert_eq!(fs::read(&path).unwrap(), before);
        let db = open_sqlite(&path).unwrap();
        assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 5);
        db.execute_batch("DROP TRIGGER fixture_error").unwrap();
        drop(db);
        assert!(VmStore::open(dir.path(), NOW).is_ok());
    }

    #[test]
    fn v5_upgrade_rolls_back_when_state_has_no_headroom() {
        let dir = private_temp();
        let db = v5_fixture(dir.path());
        db.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE fixture_padding(bytes BLOB);
             INSERT INTO fixture_padding VALUES(zeroblob(3940000));
             COMMIT;",
        )
        .unwrap();
        check_size(&db).unwrap();
        drop(db);
        let path = dir.path().join("activity.sqlite3");
        let before = fs::read(&path).unwrap();
        let error = VmStore::open(dir.path(), NOW).err().unwrap();
        assert!(full(&error), "{error}");
        assert!(fs::read(&path).unwrap() == before);
        let db = open_sqlite(&path).unwrap();
        assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 5);
        db.execute_batch("DROP TABLE fixture_padding").unwrap();
        drop(db);
        assert!(VmStore::open(dir.path(), NOW).is_ok());
    }

    fn static_fixture(db: &Connection, vm: Id, name: &str) -> Result<PairSlots> {
        use crate::vm::static_web::{StaticName, StaticProvenance};
        static_vm::reserve(
            db,
            vm,
            &StaticName::parse(name).unwrap(),
            "alice",
            [8; 16],
            StaticProvenance {
                source_generation: [9; 16],
                recipe_digest: [1; 32],
                disk_digest: [2; 32],
                vars_digest: [3; 32],
            },
            crate::vm::DiskClaim {
                images: 500,
                staging: 100,
                metadata: 0,
            },
        )
    }

    /// Commit a reserved static generation as if its copy had settled.
    fn commit_static(db: &Connection, vm: Id, slots: PairSlots, state: i64) -> Result<()> {
        db.execute(
            "UPDATE vm_artifacts SET state='active',volume_id=zeroblob(8),directory_id=zeroblob(8),
                file_id=randomblob(16),content_digest=zeroblob(32),bytes=4 WHERE generation_id=?1",
            [slots.disk.generation],
        )?;
        db.execute(
            "UPDATE vm_generations SET active=1 WHERE id=?1",
            [slots.disk.generation],
        )?;
        db.execute(
            "UPDATE vm_static_instances SET state=?2,staging=0 WHERE id=?1",
            params![vm, state],
        )?;
        Ok(())
    }

    fn evidence(byte: u8) -> ArtifactEvidence {
        ArtifactEvidence {
            identity: FileIdentity {
                volume: 0,
                file: [byte; 16],
            },
            directory: [0xdd; 16],
            pin: crate::vm::paths::ArtifactPin {
                size: 4,
                sha256: [byte; 32],
            },
        }
    }

    #[test]
    fn known_failure_causes_survive_without_release_proof() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                let status = |id| -> Result<OperationStatus> {
                    Ok(operation_status(db, "alice", id, NOW)?.unwrap())
                };
                db.execute(
                    "INSERT INTO vm_templates VALUES(?1,1,zeroblob(32),zeroblob(32),zeroblob(32),40)",
                    [[2u8; 16]],
                )?;
                let vm = |byte: u8, state: i64| -> Result<Id> {
                    let id = [byte; 16];
                    db.execute(
                        "INSERT INTO vm_instances(id,template,principal,expires_ms,state)
                         VALUES(?1,?2,'alice',?3,?4)",
                        params![id, [2u8; 16], NOW + 60_000, state],
                    )?;
                    Ok(id)
                };
                // A boot whose release is unproven keeps its known cause.
                let boot = vm(1, 1)?;
                let start = legacy_binding(1, boot, Action::Start).record(db, NOW, Some(boot), None)?;
                let cause = Some(FailureReason::BootFailed);
                settle_with_script(db, start.receipt.activity_id, NOW, Completion::CleanupUnconfirmed, None, cause)?;
                let start = status(start.id)?;
                assert_eq!(start.state, Status::Interrupted);
                assert_eq!((start.failure, start.cleanup), (cause, Some("unconfirmed")));
                // A failed stop is a known dirty stop without release proof.
                let dirty = vm(2, 2)?;
                let stop = legacy_binding(2, dirty, Action::Stop).record(db, NOW, Some(dirty), None)?;
                settle_with_script(db, stop.receipt.activity_id, NOW, Completion::Failed, None, None)?;
                let stop = status(stop.id)?;
                assert_eq!(
                    (stop.state, stop.failure, stop.cleanup),
                    (Status::Failed, Some(FailureReason::DirtyStop), Some("unconfirmed"))
                );
                // A lost guest result: execution unknown, no script status.
                let lost = vm(3, 2)?;
                let script = legacy_binding(3, lost, Action::Script).record(db, NOW, Some(lost), None)?;
                let cause = Some(FailureReason::ExecutionUnknown);
                settle_with_script(db, script.receipt.activity_id, NOW, Completion::CleanupUnconfirmed, None, cause)?;
                let script = status(script.id)?;
                assert_eq!((script.failure, script.script), (cause, None));
                // A trustworthy guest response keeps its script status.
                let guest = vm(4, 2)?;
                let exited = legacy_binding(4, guest, Action::Script).record(db, NOW, Some(guest), None)?;
                settle_with_script(db, exited.receipt.activity_id, NOW, Completion::Failed,
                    Some(ScriptResult::Exited(7)), None)?;
                let exited = status(exited.id)?;
                assert_eq!((exited.state, exited.script, exited.failure, exited.cleanup),
                    (Status::Failed, Some(ScriptResult::Exited(7)), None, Some("confirmed")));
                // Without a known cause the unproven release stays the reason.
                let unknown = vm(5, 2)?;
                let stop = legacy_binding(5, unknown, Action::Stop).record(db, NOW, Some(unknown), None)?;
                settle_with_script(db, stop.receipt.activity_id, NOW, Completion::CleanupUnconfirmed, None, None)?;
                assert_eq!(status(stop.id)?.failure, Some(FailureReason::CleanupUnconfirmed));
                // A clone worker with no publishable pair reports its cause.
                let clone_vm = [6; 16];
                let clone = Binding {
                    principal: "alice",
                    key: RequestId::parse("00000000000000000000000000000006").unwrap(),
                    subject: Subject::Template([2; 16], [6; 32]),
                    action: 1,
                }
                .record(db, NOW, Some(clone_vm), None)?;
                db.execute(
                    "INSERT INTO vm_instances(id,template,principal,expires_ms) VALUES(?1,?2,'alice',?3)",
                    params![clone_vm, [2u8; 16], NOW + 60_000],
                )?;
                let pair = plan_clone(db, clone_vm)?;
                let settled = settle_clone(db, clone.receipt.activity_id, pair, None, true, NOW,
                    Some(FailureReason::CloneFailed))?;
                assert_eq!(settled, Status::Interrupted);
                assert_eq!(status(clone.id)?.failure, Some(FailureReason::CloneFailed));
                // A staged build failure records the stage's cause.
                let request = BuildRequest {
                    principal: "alice",
                    key: RequestId::parse("00000000000000000000000000000007").unwrap(),
                    recipe: "fece98c2030f405fb6402fc92a79935d",
                    digest: [9; 32],
                    retained: crate::vm::DiskClaim::default(),
                    transient: crate::vm::DiskClaim::default(),
                };
                let build = Binding {
                    principal: request.principal,
                    key: request.key,
                    subject: Subject::Recipe(request.recipe, Some(request.digest)),
                    action: 11,
                }
                .record(db, NOW, None, None)?;
                let slots = reserve_build(db, request, build, 8)?;
                abandon_build_with(db, slots.id, build.receipt.activity_id, NOW,
                    Some(FailureReason::InstallTimedOut))?;
                let build = status(build.id)?;
                assert_eq!((build.state, build.failure), (Status::Interrupted, Some(FailureReason::InstallTimedOut)));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn static_recovery_settles_only_the_same_generation_after_release_proof() {
        let dir = private_temp();
        let vm = [7; 16];
        let store = VmStore::open(dir.path(), NOW).unwrap();
        let claim = store
            .transact(|db| {
                let slots = static_fixture(db, vm, "recovering-site")?;
                commit_static(db, vm, slots, 2)?;
                // A stop that failed to prove release keeps its cause and
                // fences the slot behind one daemon recovery receipt.
                let stop = legacy_binding(1, vm, Action::Stop).record(db, NOW, Some(vm), None)?;
                settle_with_script(
                    db,
                    stop.receipt.activity_id,
                    NOW,
                    Completion::CleanupUnconfirmed,
                    None,
                    Some(FailureReason::DirtyStop),
                )?;
                assert_eq!(
                    operation_status(db, "alice", stop.id, NOW)?
                        .unwrap()
                        .failure,
                    Some(FailureReason::DirtyStop)
                );
                assert_eq!(
                    static_vm::owned_state(db, "alice", vm)?,
                    Some(StaticState::Recovering)
                );
                assert!(static_vm::launch_busy(db)?);
                let [claim] = static_vm::recovery_claims(db)?.try_into().unwrap();
                assert_eq!((claim.vm, claim.generation), (vm, slots.disk.generation));
                assert_eq!(
                    db.query_row(
                        "SELECT vm_owner,actor FROM activity WHERE id=?1",
                        [claim.activity_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    )?,
                    ("alice".into(), "daemon".into())
                );
                // Ambiguous release keeps it recovering and records no proof.
                assert!(!static_vm::settle_recovery(db, claim, false, NOW)?);
                assert!(static_vm::recovering(db)?);
                assert_eq!(
                    db.query_row(
                        "SELECT status,cleanup_confirmed FROM activity WHERE id=?1",
                        [claim.activity_id],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<bool>>(1)?))
                    )?,
                    (0, Some(false))
                );
                Ok(claim)
            })
            .unwrap();
        // Reopen: the pending receipt is reused, not duplicated.
        drop(store);
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                assert_eq!(static_vm::recovery_claims(db)?, [claim]);
                let stale = static_vm::StaticRecovery { generation: [0x55; 16], ..claim };
                assert!(!static_vm::settle_recovery(db, stale, true, NOW)?);
                assert!(static_vm::recovering(db)?);
                assert!(static_vm::settle_recovery(db, claim, true, NOW)?);
                assert_eq!(static_vm::owned_state(db, "alice", vm)?, Some(StaticState::Stopped));
                assert!(!static_vm::launch_busy(db)?);
                assert_eq!(
                    db.query_row("SELECT status,cleanup_confirmed FROM activity WHERE id=?1",
                        [claim.activity_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<bool>>(1)?)))?,
                    (Status::Succeeded.code(), Some(true))
                );
                // Settlement never touched the committed generation.
                assert_eq!(
                    count(db, "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1 AND state='active'",
                        [claim.generation])?,
                    2
                );
                assert!(static_vm::active_pair(db, "alice", vm)?.is_some());
                assert!(static_vm::active_pair(db, "bob", vm)?.is_none());
                Ok(())
            })
            .unwrap();
        // A running static VM found at open enters recovery once.
        drop(store);
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                db.execute("UPDATE vm_static_instances SET state=2 WHERE id=?1", [vm])?;
                recover(db, NOW)?;
                recover(db, NOW)?;
                assert_eq!(
                    static_vm::owned_state(db, "alice", vm)?,
                    Some(StaticState::Recovering)
                );
                assert_eq!(
                    count(
                        db,
                        "SELECT count(*) FROM activity WHERE vm=?1 AND action=10 AND status=0",
                        [vm]
                    )?,
                    1
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn failed_static_copy_releases_only_a_proven_untouched_candidate() {
        use crate::vm::{CandidateSlot, CopyFailure};
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                let create = |byte: u8, name: &str| -> Result<(Id, PairSlots, OperationRef)> {
                    let vm = [byte; 16];
                    let operation = Binding {
                        principal: "alice",
                        key: RequestId::parse(&format!("{:032x}", byte)).unwrap(),
                        subject: Subject::Template([8; 16], [byte; 32]),
                        action: 12,
                    }
                    .record(db, NOW, Some(vm), None)?;
                    Ok((vm, static_fixture(db, vm, name)?, operation))
                };
                let settle = |(_, slots, operation): (Id, PairSlots, OperationRef), failure| {
                    static_vm::settle_create(db, operation.receipt.activity_id, slots, Err(failure), NOW)
                };
                let base = disk_bytes(db)?;
                // Proven untouched: the name, the slot and both charges are free.
                let untouched = create(1, "reused-name")?;
                assert_eq!(disk_bytes(db)?, base.map(|bytes| bytes + 600));
                assert_eq!(settle(untouched, CopyFailure::untouched(FailureReason::Cancelled))?, Status::Failed);
                assert_eq!(disk_bytes(db)?, base);
                assert_eq!(static_vm::instance_count(db)?, 0);
                assert_eq!(count(db, "SELECT count(*) FROM vm_generations WHERE static_id IS NOT NULL", [])?, 0);
                let receipt = operation_status(db, "alice", untouched.2.id, NOW)?.unwrap();
                assert_eq!((receipt.failure, receipt.cleanup), (Some(FailureReason::Cancelled), Some("confirmed")));
                // The released name is reusable.
                let identified = create(2, "reused-name")?;
                // One identified leaf: retained, quarantined and charged.
                let failure = CopyFailure {
                    reason: FailureReason::CloneFailed,
                    slots: [CandidateSlot::Retained(evidence(0x21)), CandidateSlot::Absent],
                };
                assert_eq!(settle(identified, failure)?, Status::Failed);
                assert_eq!(disk_bytes(db)?, base.map(|bytes| bytes + 600));
                let mut stmt = db.prepare(
                    "SELECT role,state,file_id IS NOT NULL FROM vm_artifacts WHERE generation_id=?1 ORDER BY role",
                )?;
                let rows = stmt
                    .query_map([identified.1.disk.generation], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert_eq!(rows, [
                    ("static_disk".into(), "quarantined".into(), true),
                    ("static_vars".into(), "quarantined".into(), false),
                ]);
                assert_eq!(static_vm::owned_state(db, "alice", identified.0)?, Some(StaticState::Failed));
                // Unknown effects, including a late worker, are never released.
                let unknown = create(3, "unknown-site")?;
                assert_eq!(settle(unknown, CopyFailure::unknown(FailureReason::CleanupUnconfirmed))?, Status::Failed);
                assert_eq!(static_vm::instance_count(db)?, 2);
                assert_eq!(
                    operation_status(db, "alice", unknown.2.id, NOW)?.unwrap().cleanup,
                    Some("unconfirmed")
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn tombstone_correlation_requires_the_attributed_expiry_receipt() {
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                let receipt = |vm: Id, owner: &str, action: i64, actor: &str, proof: Option<bool>| -> Result<i64> {
                    db.execute(
                        "INSERT INTO activity(principal,vm,action,started_ms,status,finished_ms,terminal_order,
                            cleanup_confirmed,actor,vm_owner)
                         VALUES(?1,?2,?3,?4,1,?4,(SELECT coalesce(max(terminal_order),0)+1 FROM activity),?5,?6,
                            CASE WHEN ?3 IN (7,10) THEN ?7 END)",
                        params![if actor == "daemon" { "arxd" } else { owner }, vm, action, NOW, proof, actor, owner],
                    )?;
                    Ok(db.last_insert_rowid())
                };
                let tombstone = |vm: Id, activity: i64| -> Result<()> {
                    db.execute(
                        "INSERT INTO vm_tombstones VALUES(?1,'alice',?2,?3)",
                        params![vm, activity, NOW],
                    )?;
                    Ok(())
                };
                type Case<'a> = (u8, Id, &'a str, i64, &'a str, Option<bool>);
                // Attributed receipts keep their correlation, including a
                // legacy receipt without proof. Mismatched VM, owner or
                // action yields no correlation and no label.
                let cases: [(Case, Option<Option<bool>>); 6] = [
                    ((1, [1; 16], "alice", 7, "daemon", Some(true)), Some(Some(true))),
                    ((2, [2; 16], "alice", 7, "daemon", Some(false)), Some(Some(false))),
                    ((3, [3; 16], "alice", 7, "daemon", None), Some(None)),
                    ((4, [0x44; 16], "alice", 7, "daemon", Some(true)), None),
                    ((5, [5; 16], "bob", 7, "daemon", Some(true)), None),
                    ((6, [6; 16], "alice", 3, "agent", Some(true)), None),
                ];
                for ((byte, receipt_vm, owner, action, actor, proof), expected) in cases {
                    let vm = [byte; 16];
                    let id = receipt(receipt_vm, owner, action, actor, proof)?;
                    tombstone(vm, id)?;
                    assert_eq!(
                        tombstone_proof(db, "alice", vm)?,
                        expected.map(|confirmed| (id, confirmed)),
                        "case {byte}"
                    );
                    assert!(tombstone_proof(db, "bob", vm)?.is_none());
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_cleanup_authority_cannot_resolve_a_static_generation() {
        use crate::vm::static_web::{StaticName, StaticProvenance};
        let dir = private_temp();
        let store = VmStore::open(dir.path(), NOW).unwrap();
        store
            .transact(|db| {
                let vm = [7; 16];
                let template = [8; 16];
                let slots = static_vm::reserve(
                    db,
                    vm,
                    &StaticName::parse("private-site").unwrap(),
                    "alice",
                    template,
                    StaticProvenance {
                        source_generation: [9; 16],
                        recipe_digest: [1; 32],
                        disk_digest: [2; 32],
                        vars_digest: [3; 32],
                    },
                    crate::vm::DiskClaim {
                        images: 500,
                        staging: 100,
                        metadata: 0,
                    },
                )?;
                for kind in [
                    CleanupKind::Build(vm),
                    CleanupKind::Expiry { vm, activity_id: 1 },
                    CleanupKind::Staging { vm, activity_id: 1 },
                    CleanupKind::Recovery { vm, activity_id: 1 },
                ] {
                    assert!(cleanup_work(db, kind)?.is_none());
                }
                assert!(cleanup_kinds(db)?.is_empty());
                assert_eq!(disk_bytes(db)?, Some(600));
                assert_eq!(
                    count(
                        db,
                        "SELECT count(*) FROM vm_artifacts WHERE generation_id=?1
                AND role IN ('static_disk','static_vars')",
                        [slots.disk.generation]
                    )?,
                    2
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn future_vm_version_leaves_state_unchanged() {
        let dir = private_temp();
        let db = v5_fixture(dir.path());
        db.pragma_update(None, "user_version", VM_SCHEMA_VERSION + 1)
            .unwrap();
        drop(db);
        let path = dir.path().join("activity.sqlite3");
        let before = fs::read(&path).unwrap();
        let error = VmStore::open(dir.path(), NOW).err().unwrap();
        assert_eq!(error.to_string(), "unsupported VM state version");
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn general_and_service_profiles_keep_activity_schema() {
        for open in [
            super::super::Activity::open,
            super::super::Activity::open_service_fixture,
        ] {
            let dir = private_temp();
            drop(open(dir.path()).unwrap());
            let path = dir.path().join("activity.sqlite3");
            let before = fs::read(&path).unwrap();
            assert!(VmStore::open(dir.path(), NOW).is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
            drop(open(dir.path()).unwrap());
            let db = open_sqlite(&path).unwrap();
            assert_eq!(count(&db, "PRAGMA user_version", []).unwrap(), 5);
            assert_eq!(
                count(
                    &db,
                    "SELECT count(*) FROM sqlite_schema WHERE name GLOB 'vm_*'",
                    []
                )
                .unwrap(),
                0
            );
        }
    }

    #[test]
    fn v6_checks_bindings_evidence_and_script_result_in_db_and_journal() {
        let dir = private_temp();
        let path = dir.path().join("activity.sqlite3");
        let journal = dir.path().join("activity.sqlite3-journal");
        let sentinels = [
            "fixture-script-contents-9cc20e7f",
            "fixture-token-23deea7d903170f8",
            "C:/fixture-absolute-path-48fd070a",
            "S-1-5-21-11112222-33334444-55556666-7777",
        ];
        let scan = |path: &Path| {
            let bytes = fs::read(path).unwrap();
            for sentinel in sentinels {
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|w| w == sentinel.as_bytes())
                );
            }
        };
        let old = v5_fixture(dir.path());
        old.execute_batch("BEGIN IMMEDIATE; UPDATE vm_retry_clock SET high_water_ms=91;")
            .unwrap();
        scan(&path);
        scan(&journal);
        old.execute_batch("ROLLBACK;").unwrap();
        drop(old);
        let store = VmStore::open(dir.path(), NOW).unwrap();
        scan(&path);
        store.transact(|db| {
            for sentinel in sentinels {
                assert!(db.execute("UPDATE vm_operations SET script_result=?1 WHERE activity_id=9", [sentinel]).is_err());
            }
            assert!(db.execute("UPDATE vm_operations SET script_result='exited' WHERE activity_id=9", []).is_err());
            assert!(db.execute("UPDATE vm_operations SET script_result='timed_out',script_exit_code=7 WHERE activity_id=9", []).is_err());
            db.execute("UPDATE vm_operations SET script_result='exited',script_exit_code=7 WHERE activity_id=9", [])?;
            assert!(db.execute("UPDATE vm_operations SET binding_kind='template',subject_id=zeroblob(16) WHERE activity_id=10", []).is_err());
            db.execute("UPDATE vm_operations SET binding_kind='template',subject_id=zeroblob(16),
                request_digest=zeroblob(32) WHERE activity_id=10", [])?;

            db.execute_batch(
                "INSERT INTO activity(principal,actor,action,started_ms) VALUES('alice','agent',11,100);
                 INSERT INTO vm_operations(principal,request_key,operation_id,action,activity_id,
                    valid_through_ms,binding_kind,recipe_id,recipe_digest)
                    VALUES('alice',zeroblob(16),zeroblob(16),11,last_insert_rowid(),86400100,
                        'recipe','ubuntu-fixture',zeroblob(32));
                 INSERT INTO vm_build_intents(id,operation_id,recipe_id,recipe_digest,staging_bytes,state)
                    VALUES(zeroblob(16),zeroblob(16),'ubuntu-fixture',zeroblob(32),100,'pending');
                 INSERT INTO vm_generations(id,build_id,generation) VALUES(zeroblob(16),zeroblob(16),1);
                 INSERT INTO vm_artifacts(generation_id,role,object_key,state)
                    VALUES(zeroblob(16),'disk',zeroblob(16),'planned');",
            )?;
            for sentinel in sentinels {
                assert!(db.execute("UPDATE vm_artifacts SET object_key=CAST(?1 AS BLOB)", [sentinel]).is_err());
            }
            assert!(db.execute("UPDATE vm_artifacts SET state='identified'", []).is_err());
            assert!(db.execute("UPDATE vm_build_intents SET staging_bytes=-1", []).is_err());
            assert!(db.execute("UPDATE vm_generations SET build_id=randomblob(16)", []).is_err());
            assert!(db.execute("INSERT INTO vm_generations(id,build_id,generation) VALUES(randomblob(16),zeroblob(16),1)", []).is_err());
            db.execute("UPDATE vm_artifacts SET state='identified',volume_id=zeroblob(8),
                directory_id=zeroblob(8),file_id=zeroblob(8)", [])?;
            db.execute("INSERT INTO vm_operations(principal,request_key,operation_id,action,
                activity_id,valid_through_ms,binding_kind,recipe_id,recipe_digest)
                VALUES('bob',zeroblob(16),randomblob(16),11,200,86400100,'recipe',
                    'ubuntu-fixture',zeroblob(32))", [])?;
            assert!(db.execute("INSERT INTO vm_build_intents(id,operation_id,recipe_id,recipe_digest,staging_bytes,state)
                SELECT randomblob(16),operation_id,'ubuntu-fixture',zeroblob(32),100,'pending'
                FROM vm_operations WHERE activity_id=200", []).is_err());
            scan(&journal);
            Ok(())
        }).unwrap();
        scan(&path);
    }

    #[test]
    fn substituted_sqlite_main_refuses_before_reconciliation() {
        let original = private_temp();
        let replacement = private_temp();
        let paths = [&original, &replacement].map(|dir| {
            drop(v5_fixture(dir.path()));
            let path = dir.path().join("activity.sqlite3");
            let before = fs::read(&path).unwrap();
            (path, before)
        });

        let other_path = replacement.path().join("activity.sqlite3");
        let parked = original.path().join("parked.sqlite3");
        let result = VmStore::open_inner(original.path(), NOW, |path| {
            #[cfg(unix)]
            {
                // Put both names back before returning, so a before/after
                // pathname-only check would accept the substituted connection.
                fs::rename(path, &parked)?;
                fs::rename(&other_path, path)?;
                let opened = open_sqlite(path)?;
                fs::rename(path, &other_path)?;
                fs::rename(&parked, path)?;
                Ok(opened)
            }
            #[cfg(windows)]
            {
                // The retained guard denies rename here. Present a connection
                // to the other database to exercise the independent handle check.
                let error = fs::rename(path, &parked).unwrap_err();
                assert_eq!(
                    error.raw_os_error(),
                    Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32)
                );
                open_sqlite(&other_path)
            }
        });
        let Err(error) = result else {
            panic!("SQLite opened an unexpected main database");
        };
        assert_eq!(
            error.to_string(),
            "SQLite VM main database identity changed"
        );
        for (path, before) in paths {
            assert_eq!(fs::read(&path).unwrap(), before);
            let raw = open_sqlite(&path).unwrap();
            assert_eq!(count(&raw, "PRAGMA user_version", []).unwrap(), 5);
            assert_eq!(
                count(
                    &raw,
                    "SELECT status FROM vm_operations WHERE activity_id=11",
                    []
                )
                .unwrap(),
                0
            );
        }
    }
}

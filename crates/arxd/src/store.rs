use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::{path::Path, sync::Mutex, time::Duration};

/// Private local identity and bounded activity; no credentials or host paths.
pub struct Activity {
    db: Mutex<Connection>,
    service_reason: Mutex<Option<&'static str>>,
    _lock: std::fs::File,
    _service_pins: Vec<crate::service::native::Observed>,
}
impl Activity {
    pub fn open(dir: &Path) -> Result<Self> {
        Self::open_inner(dir, Vec::new(), false)
    }
    pub(crate) fn open_service(layout: &crate::service::layout::Layout) -> Result<Self> {
        let pins = crate::service::native::state_guard(layout)?;
        Self::open_inner(layout.state(), pins, true)
    }
    #[cfg(test)]
    pub(crate) fn open_service_fixture(dir: &Path) -> Result<Self> {
        // Explicit current-user fixture policy; unreachable from Native hooks.
        Self::open_inner(dir, Vec::new(), true)
    }
    fn open_inner(
        dir: &Path,
        pins: Vec<crate::service::native::Observed>,
        service: bool,
    ) -> Result<Self> {
        crate::config::checked_path(dir)?;
        crate::security::private(dir)?;
        let lock_path = dir.join("activity.lock");
        if std::fs::symlink_metadata(&lock_path).is_ok() {
            crate::config::checked_path(&lock_path)?;
            crate::security::private(&lock_path)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(&lock_path)?;
        lock.try_lock()?;
        let path = dir.join("activity.sqlite3");
        if std::fs::symlink_metadata(&path).is_ok() {
            crate::config::checked_path(&path)?;
            crate::security::private(&path)?;
        }
        #[cfg(unix)]
        if !path.exists() {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
        }
        for suffix in ["-journal", "-wal", "-shm"] {
            let companion = dir.join(format!("activity.sqlite3{suffix}"));
            if std::fs::symlink_metadata(&companion).is_ok() {
                crate::config::checked_path(&companion)?;
                crate::security::private(&companion)?;
            }
        }
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(250))?;
        let page_size: u64 = db.pragma_query_value(None, "page_size", |r| r.get(0))?;
        let page_count: u64 = db.pragma_query_value(None, "page_count", |r| r.get(0))?;
        if page_size > 4096 || page_count > 1024 {
            anyhow::bail!("activity store exceeds size bound");
        }
        db.execute_batch(if service {
            "PRAGMA journal_mode=TRUNCATE; PRAGMA synchronous=FULL; PRAGMA max_page_count=1024;"
        } else {
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA max_page_count=1024;"
        })?;
        let tx = db.transaction()?;
        let version: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                tx.execute_batch("CREATE TABLE local_host_identity(singleton INTEGER PRIMARY KEY CHECK(singleton=1), host_id TEXT NOT NULL);")?;
                let mut random = [0u8; 32];
                getrandom::fill(&mut random)
                    .map_err(|_| anyhow::anyhow!("host identity randomness unavailable"))?;
                tx.execute(
                    "INSERT INTO local_host_identity VALUES(1, ?1)",
                    [hex::encode(random)],
                )?;
            }
            1..=3 => {}
            _ => anyhow::bail!("unsupported local state version"),
        }
        read_host_id(&tx)?;
        if version < 2 {
            tx.execute_batch("CREATE TABLE IF NOT EXISTS activity(id INTEGER PRIMARY KEY AUTOINCREMENT, principal TEXT NOT NULL, tool TEXT NOT NULL, outcome TEXT NOT NULL);")?;
            // Reconcile first so any recovery failure rolls back identity and schema too.
            tx.execute(
                "UPDATE activity SET outcome='interrupted' WHERE outcome='started'",
                [],
            )?;
            tx.execute_batch("ALTER TABLE activity RENAME TO activity_v1;")?;
            tx.execute_batch(&schema())?;
            tx.execute_batch("INSERT INTO activity(id,principal,tool,outcome,actor_kind,class) SELECT id,principal,tool,outcome,'agent','read' FROM activity_v1; UPDATE sqlite_sequence SET seq=max(seq,coalesce((SELECT seq FROM sqlite_sequence WHERE name='activity_v1'),0)) WHERE name='activity'; DROP TABLE activity_v1; PRAGMA user_version=2;")?;
        }
        if version < 3 {
            tx.execute_batch("ALTER TABLE activity ADD COLUMN identity_mode TEXT CHECK(identity_mode IS NULL OR (typeof(identity_mode)='text' AND identity_mode IN ('dedicated_account','development_current_user'))) CHECK(identity_mode IS NOT 'development_current_user' OR (observed_kind IS NULL AND observed_id IS NULL AND observed_gid IS NULL AND rights IS NULL AND (elevation IS NULL OR elevation=0) AND (integrity_rid IS NULL OR integrity_rid=8192) AND (observation_point IS NULL OR observation_point='pre_exec') AND (tree_boundary IS NULL OR tree_boundary='windows_job'))); PRAGMA user_version=3;")?;
        }
        tx.execute(
            "UPDATE activity SET outcome='interrupted',finished_at_ms=?1 WHERE outcome='started'",
            [now_ms()?],
        )?;
        validate_columns(&tx)?;
        check_size(&tx)?;
        tx.commit()?;
        Ok(Self {
            db: Mutex::new(db),
            service_reason: Mutex::new(service.then_some("mechanism_unavailable")),
            _lock: lock,
            _service_pins: pins,
        })
    }
    pub(crate) fn service_reason(&self) -> Option<&'static str> {
        self.service_reason.lock().ok().and_then(|r| *r)
    }
    pub(crate) fn set_service_reason(&self, reason: &'static str) {
        if let Ok(mut r) = self.service_reason.lock() {
            *r = Some(reason);
        }
    }
    /// Called only after service connection tasks have stopped; no executor exists.
    pub(crate) fn shutdown_pending(&self) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?;
        db.execute("UPDATE activity SET outcome='failed',reason='shutdown',stop_reason='shutdown',finished_at_ms=?1 WHERE outcome='started' AND actor_kind='agent'", [now_ms()?])?;
        Ok(())
    }
    /// Application host identity only, unrelated to Raft node IDs or certificates.
    pub fn host_id(&self) -> Result<String> {
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?;
        read_host_id(&db)
    }
    pub fn start(&self, principal: &str, tool: &str) -> Result<i64> {
        let tool = Tool::parse(tool)?;
        if matches!(tool, Tool::ServiceStart | Tool::ServiceStop) {
            anyhow::bail!("invalid agent tool");
        }
        self.start_typed(principal, tool, false, None)
    }
    pub fn start_development(&self, principal: &str, tool: &str) -> Result<i64> {
        self.start_typed(
            principal,
            Tool::parse(tool)?,
            false,
            Some(IdentityMode::DevelopmentCurrentUser),
        )
    }
    /// Lifecycle records are daemon-only, with a fixed principal.
    pub fn start_lifecycle(&self, event: Lifecycle) -> Result<i64> {
        self.start_typed(
            "arxd",
            match event {
                Lifecycle::Start => Tool::ServiceStart,
                Lifecycle::Stop => Tool::ServiceStop,
            },
            true,
            None,
        )
    }
    fn start_typed(
        &self,
        principal: &str,
        tool: Tool,
        daemon: bool,
        mode: Option<IdentityMode>,
    ) -> Result<i64> {
        validate_principal(principal)?;
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?;
        let tx = db.transaction()?;
        tx.execute("INSERT INTO activity(principal,tool,outcome,actor_kind,class,started_at_ms,identity_mode) VALUES (?1,?2,'started',?3,?4,?5,?6)", params![principal,tool.as_str(),if daemon {"daemon"} else {"agent"},tool.class(),now_ms()?,mode.map(IdentityMode::as_str)])?;
        let id = tx.last_insert_rowid();
        tx.execute_batch("DELETE FROM activity WHERE class='read' AND id NOT IN (SELECT id FROM activity WHERE class='read' ORDER BY id DESC LIMIT 200); DELETE FROM activity WHERE class<>'read' AND id NOT IN (SELECT id FROM activity WHERE class<>'read' ORDER BY id DESC LIMIT 1000);")?;
        check_size(&tx)?;
        tx.commit()?;
        Ok(id)
    }
    pub fn finish(&self, id: i64, outcome: &str) -> Result<()> {
        self.finish_typed(id, Outcome::parse(outcome)?, &Metadata::default())
    }
    /// Cancellation, timeout and shutdown complete as `Outcome::Failed` with
    /// `StopReason::Cancelled`, `Timeout` or `Shutdown`; never as outcome strings.
    pub fn finish_typed(&self, id: i64, outcome: Outcome, metadata: &Metadata) -> Result<()> {
        if outcome == Outcome::Started {
            anyhow::bail!("invalid completion");
        }
        metadata.validate()?;
        let mut db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?;
        let tx = db.transaction()?;
        // The mode recorded at start is authoritative; completion cannot relabel it.
        if let Some(mode) = metadata.identity_mode {
            let current: Option<String> = tx
                .query_row(
                    "SELECT identity_mode FROM activity WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if current.is_some_and(|c| c != mode.as_str()) {
                anyhow::bail!("activity completion refused");
            }
        }
        let value = serde_json::to_value(metadata)?;
        let mut values = vec![
            rusqlite::types::Value::Text(outcome.as_str().into()),
            now_ms()?.into(),
        ];
        for (name, _) in FIELDS {
            values.push(sql_value(&value[name])?);
        }
        values.push(id.into());
        let assignments = FIELDS
            .iter()
            .map(|(n, _)| {
                if *n == "identity_mode" {
                    "identity_mode=coalesce(identity_mode,?)".into()
                } else {
                    format!("{n}=?")
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let changed = tx.execute(&format!("UPDATE activity SET outcome=?,finished_at_ms=?,class=CASE WHEN ?1='rejected' THEN 'read' ELSE class END,{assignments} WHERE id=? AND outcome='started'"), rusqlite::params_from_iter(values))?;
        if changed != 1 {
            anyhow::bail!("activity completion refused");
        }
        tx.execute("DELETE FROM activity WHERE class='read' AND id NOT IN (SELECT id FROM activity WHERE class='read' ORDER BY id DESC LIMIT 200)", [])?;
        check_size(&tx)?;
        tx.commit()?;
        Ok(())
    }
    /// Caller must use its own committed read id; SQL bounds both rows and values.
    pub fn list(&self, principal: &str, read_id: i64, args: &ListActivity) -> Result<ActivityPage> {
        validate_principal(principal)?;
        args.validate()?;
        let db = self
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("activity unavailable"))?;
        validate_columns(&db)?;
        let own: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM activity WHERE id=?1 AND principal=?2 AND actor_kind='agent' AND tool='list_activity' AND outcome='started')", params![read_id,principal], |r| r.get(0))?;
        if !own {
            anyhow::bail!("invalid activity cursor");
        }
        let names = [
            vec![
                ("id", "integer"),
                ("tool", "text"),
                ("outcome", "text"),
                ("started_at_ms", "integer"),
                ("finished_at_ms", "integer"),
            ],
            FIELDS.to_vec(),
        ]
        .concat();
        // No arbitrary text/blob is ever loaded into Rust, even from a tampered DB.
        let columns = names.iter().map(|(n,_)| format!("CASE WHEN typeof({n})='text' AND length(CAST({n} AS BLOB))<=184 THEN {n} WHEN typeof({n}) IN ('null','integer') THEN {n} ELSE NULL END, typeof({n}), length(CAST({n} AS BLOB))")).collect::<Vec<_>>().join(",");
        let mut stmt = db.prepare(&format!("SELECT {columns}, class IN ('read','action','lifecycle') FROM activity WHERE actor_kind='agent' AND principal=?1 AND id<?2 AND (?3=1 OR class<>'read') ORDER BY id DESC LIMIT ?4"))?;
        let mut rows = stmt.query(params![
            principal,
            args.before_id.unwrap_or(read_id).min(read_id),
            args.include_reads,
            args.limit + 1
        ])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            if !row.get::<_, bool>(names.len() * 3)? {
                anyhow::bail!("invalid activity record");
            }
            let mut object = serde_json::Map::new();
            for (i, (name, kind)) in names.iter().enumerate() {
                let ty: String = row.get(i * 3 + 1)?;
                if ty != "null" && ty != *kind {
                    anyhow::bail!("invalid activity record");
                }
                let v = match ty.as_str() {
                    "null" => serde_json::Value::Null,
                    "integer" => {
                        let n: i64 = row.get(i * 3)?;
                        if ["cleanup_confirmed", "identity_matches_pin", "elevation"].contains(name)
                        {
                            if !(0..=1).contains(&n) {
                                anyhow::bail!("invalid activity record");
                            }
                            serde_json::json!(n == 1)
                        } else {
                            serde_json::json!(n)
                        }
                    }
                    "text" => {
                        let length: i64 = row.get(i * 3 + 2)?;
                        if length > 184 {
                            anyhow::bail!("invalid activity record");
                        }
                        serde_json::json!(row.get::<_, String>(i * 3)?)
                    }
                    _ => anyhow::bail!("invalid activity record"),
                };
                object.insert((*name).into(), v);
            }
            let mut entry: ActivityRow = serde_json::from_value(object.into())
                .map_err(|_| anyhow::anyhow!("invalid activity record"))?;
            entry.validate()?;
            entry.execution_mode = entry.metadata.identity_mode;
            if entry.execution_mode == Some(IdentityMode::DevelopmentCurrentUser) {
                entry.claim = Some(crate::development::CLAIM.into());
            }
            result.push(entry);
        }
        let more = result.len() > args.limit as usize;
        result.truncate(args.limit as usize);
        let next_before_id = if more {
            result.last().map(|r| r.id)
        } else {
            None
        };
        Ok(ActivityPage {
            activities: result,
            next_before_id,
        })
    }
}

fn read_host_id(db: &Connection) -> Result<String> {
    // Bound even malformed values and reject missing/extra rows and non-text data.
    let mut stmt = db.prepare(
        "SELECT singleton, length(host_id), substr(host_id,1,65) FROM local_host_identity LIMIT 2",
    )?;
    let mut rows = stmt.query([])?;
    let row = rows
        .next()?
        .ok_or_else(|| anyhow::anyhow!("missing host identity"))?;
    let singleton: i64 = row.get(0)?;
    let length: i64 = row.get(1)?;
    let id: String = row.get(2)?;
    if singleton != 1
        || length != 64
        || id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || rows.next()?.is_some()
    {
        anyhow::bail!("invalid host identity");
    }
    Ok(id)
}

use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    #[serde(rename = "list_hosts")]
    ListHosts,
    #[serde(rename = "list_activity")]
    ListActivity,
    #[serde(rename = "unknown_tool")]
    UnknownTool,
    #[serde(rename = "run_command")]
    RunCommand,
    #[serde(rename = "discover_tools")]
    DiscoverTools,
    #[serde(rename = "service_start")]
    ServiceStart,
    #[serde(rename = "service_stop")]
    ServiceStop,
}
impl Tool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ListHosts => "list_hosts",
            Self::ListActivity => "list_activity",
            Self::UnknownTool => "unknown_tool",
            Self::RunCommand => "run_command",
            Self::DiscoverTools => "discover_tools",
            Self::ServiceStart => "service_start",
            Self::ServiceStop => "service_stop",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "list_hosts" => Ok(Self::ListHosts),
            "list_activity" => Ok(Self::ListActivity),
            "unknown_tool" => Ok(Self::UnknownTool),
            "run_command" => Ok(Self::RunCommand),
            "discover_tools" => Ok(Self::DiscoverTools),
            "service_start" => Ok(Self::ServiceStart),
            "service_stop" => Ok(Self::ServiceStop),
            _ => anyhow::bail!("invalid activity metadata"),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    #[serde(rename = "started")]
    Started,
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "refused")]
    Refused,
    #[serde(rename = "interrupted")]
    Interrupted,
}
impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Rejected => "rejected",
            Self::Refused => "refused",
            Self::Interrupted => "interrupted",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "started" => Ok(Self::Started),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "rejected" => Ok(Self::Rejected),
            "refused" => Ok(Self::Refused),
            "interrupted" => Ok(Self::Interrupted),
            _ => anyhow::bail!("invalid activity metadata"),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reason {
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "busy")]
    Busy,
    #[serde(rename = "quarantined")]
    Quarantined,
    #[serde(rename = "invalid_arguments")]
    InvalidArguments,
    #[serde(rename = "preflight_failed")]
    PreflightFailed,
    #[serde(rename = "storage_failed")]
    StorageFailed,
    #[serde(rename = "execution_failed")]
    ExecutionFailed,
    #[serde(rename = "identity_mismatch")]
    IdentityMismatch,
    #[serde(rename = "cleanup_unconfirmed")]
    CleanupUnconfirmed,
    #[serde(rename = "shutdown")]
    Shutdown,
}
impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Busy => "busy",
            Self::Quarantined => "quarantined",
            Self::InvalidArguments => "invalid_arguments",
            Self::PreflightFailed => "preflight_failed",
            Self::StorageFailed => "storage_failed",
            Self::ExecutionFailed => "execution_failed",
            Self::IdentityMismatch => "identity_mismatch",
            Self::CleanupUnconfirmed => "cleanup_unconfirmed",
            Self::Shutdown => "shutdown",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Program {
    #[serde(rename = "cargo")]
    Cargo,
    #[serde(rename = "rustc")]
    Rustc,
    #[serde(rename = "rustup")]
    Rustup,
    #[serde(rename = "git")]
    Git,
    #[serde(rename = "python3")]
    Python3,
    #[serde(rename = "python")]
    Python,
    #[serde(rename = "node")]
    Node,
    #[serde(rename = "qemu-system-x86_64")]
    QemuSystemX8664,
    #[serde(rename = "qemu-img")]
    QemuImg,
    #[serde(rename = "path")]
    Path,
}
impl Program {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Rustc => "rustc",
            Self::Rustup => "rustup",
            Self::Git => "git",
            Self::Python3 => "python3",
            Self::Python => "python",
            Self::Node => "node",
            Self::QemuSystemX8664 => "qemu-system-x86_64",
            Self::QemuImg => "qemu-img",
            Self::Path => "path",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rights {
    #[serde(rename = "standard")]
    Standard,
    #[serde(rename = "admin")]
    Admin,
}
impl Rights {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Admin => "admin",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservedKind {
    #[serde(rename = "windows_sid")]
    WindowsSid,
    #[serde(rename = "unix_uid")]
    UnixUid,
}
impl ObservedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WindowsSid => "windows_sid",
            Self::UnixUid => "unix_uid",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationPoint {
    #[serde(rename = "post_switch")]
    PostSwitch,
    #[serde(rename = "pre_exec")]
    PreExec,
    #[serde(rename = "child")]
    Child,
}
impl ObservationPoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PostSwitch => "post_switch",
            Self::PreExec => "pre_exec",
            Self::Child => "child",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    #[serde(rename = "exited")]
    Exited,
    #[serde(rename = "timeout")]
    Timeout,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "output_limit")]
    OutputLimit,
    #[serde(rename = "shutdown")]
    Shutdown,
    #[serde(rename = "failed")]
    Failed,
}
impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exited => "exited",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::OutputLimit => "output_limit",
            Self::Shutdown => "shutdown",
            Self::Failed => "failed",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeBoundary {
    #[serde(rename = "windows_job")]
    WindowsJob,
    #[serde(rename = "linux_cgroup")]
    LinuxCgroup,
    #[serde(rename = "mac_uid_sweep")]
    MacUidSweep,
    #[serde(rename = "process_group")]
    ProcessGroup,
}
impl TreeBoundary {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WindowsJob => "windows_job",
            Self::LinuxCgroup => "linux_cgroup",
            Self::MacUidSweep => "mac_uid_sweep",
            Self::ProcessGroup => "process_group",
        }
    }
}
impl Tool {
    fn class(self) -> &'static str {
        match self {
            Self::RunCommand | Self::DiscoverTools => "action",
            Self::ServiceStart | Self::ServiceStop => "lifecycle",
            _ => "read",
        }
    }
}
#[derive(Clone, Copy)]
pub enum Lifecycle {
    Start,
    Stop,
}
#[derive(Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub identity_mode: Option<IdentityMode>,
    pub reason: Option<Reason>,
    pub program: Option<Program>,
    pub argc: Option<u16>,
    pub rights: Option<Rights>,
    pub observed_kind: Option<ObservedKind>,
    pub observed_id: Option<String>,
    pub observed_gid: Option<u32>,
    pub integrity_rid: Option<u32>,
    pub elevation: Option<bool>,
    pub observation_point: Option<ObservationPoint>,
    pub identity_matches_pin: Option<bool>,
    pub stop_reason: Option<StopReason>,
    pub exit_code: Option<i32>,
    pub tree_boundary: Option<TreeBoundary>,
    pub cleanup_confirmed: Option<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityMode {
    DedicatedAccount,
    DevelopmentCurrentUser,
}
impl IdentityMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DedicatedAccount => "dedicated_account",
            Self::DevelopmentCurrentUser => "development_current_user",
        }
    }
}
impl Metadata {
    fn validate(&self) -> Result<()> {
        if self.identity_mode == Some(IdentityMode::DevelopmentCurrentUser)
            && (self.observed_id.is_some()
                || self.observed_kind.is_some()
                || self.observed_gid.is_some()
                || self.rights.is_some())
        {
            anyhow::bail!("invalid development metadata");
        }
        if self.argc.is_some_and(|n| n > 128) {
            anyhow::bail!("invalid activity metadata");
        }
        if let Some(id) = &self.observed_id {
            let valid = match self.observed_kind {
                Some(ObservedKind::UnixUid) => decimal(id) && id.parse::<u32>().is_ok(),
                Some(ObservedKind::WindowsSid) => id.strip_prefix("S-1-").is_some_and(|s| {
                    let parts: Vec<_> = s.split('-').collect();
                    (2..=16).contains(&parts.len())
                        && decimal(parts[0])
                        && parts[0].parse::<u64>().is_ok_and(|n| n <= 281474976710655)
                        && parts[1..]
                            .iter()
                            .all(|p| decimal(p) && p.parse::<u32>().is_ok())
                }),
                None => false,
            };
            if id.len() > 184 || !valid {
                anyhow::bail!("invalid observed identity");
            }
        }
        Ok(())
    }
}
fn decimal(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ActivityRow {
    pub id: i64,
    pub tool: Tool,
    pub outcome: Outcome,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    #[serde(flatten)]
    pub metadata: Metadata,
    #[serde(default)]
    pub execution_mode: Option<IdentityMode>,
    #[serde(default)]
    pub claim: Option<String>,
}
impl ActivityRow {
    fn validate(&self) -> Result<()> {
        if self.id <= 0
            || [self.started_at_ms, self.finished_at_ms]
                .into_iter()
                .flatten()
                .any(|n| !(0..=253402300799999).contains(&n))
        {
            anyhow::bail!("invalid activity record");
        }
        self.metadata.validate()
    }
}
#[derive(Debug, Serialize)]
pub struct ActivityPage {
    pub activities: Vec<ActivityRow>,
    pub next_before_id: Option<i64>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListActivity {
    #[serde(default = "default_limit")]
    pub limit: u8,
    pub before_id: Option<i64>,
    #[serde(default)]
    pub include_reads: bool,
}
fn default_limit() -> u8 {
    20
}
impl ListActivity {
    pub fn validate(&self) -> Result<()> {
        if !(1..=50).contains(&self.limit) || self.before_id.is_some_and(|n| n <= 0) {
            anyhow::bail!("invalid activity arguments");
        }
        Ok(())
    }
}
fn validate_principal(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 64
        || !s
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        anyhow::bail!("invalid principal");
    }
    Ok(())
}
fn now_ms() -> Result<i64> {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    if n > 253402300799999 {
        anyhow::bail!("invalid clock");
    }
    Ok(n as i64)
}
fn check_size(db: &Connection) -> Result<()> {
    let pages: u64 = db.pragma_query_value(None, "page_count", |r| r.get(0))?;
    let size: u64 = db.pragma_query_value(None, "page_size", |r| r.get(0))?;
    if pages > 1024 || size > 4096 {
        anyhow::bail!("activity store exceeds size bound");
    }
    Ok(())
}
fn sql_value(v: &serde_json::Value) -> Result<rusqlite::types::Value> {
    Ok(match v {
        serde_json::Value::Null => rusqlite::types::Value::Null,
        serde_json::Value::Bool(b) => (*b as i64).into(),
        serde_json::Value::Number(n) => n
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("invalid metadata"))?
            .into(),
        serde_json::Value::String(s) => s.clone().into(),
        _ => anyhow::bail!("invalid metadata"),
    })
}
const FIELDS: &[(&str, &str)] = &[
    ("identity_mode", "text"),
    ("reason", "text"),
    ("program", "text"),
    ("argc", "integer"),
    ("rights", "text"),
    ("observed_kind", "text"),
    ("observed_id", "text"),
    ("observed_gid", "integer"),
    ("integrity_rid", "integer"),
    ("elevation", "integer"),
    ("observation_point", "text"),
    ("identity_matches_pin", "integer"),
    ("stop_reason", "text"),
    ("exit_code", "integer"),
    ("tree_boundary", "text"),
    ("cleanup_confirmed", "integer"),
];
fn schema() -> String {
    r#"CREATE TABLE activity(
    id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id>0),
    principal TEXT NOT NULL CHECK(typeof(principal)='text' AND length(CAST(principal AS BLOB)) BETWEEN 1 AND 64 AND principal NOT GLOB '*[^a-zA-Z0-9_-]*'),
    actor_kind TEXT NOT NULL CHECK(actor_kind IN ('agent','daemon') AND (actor_kind<>'daemon' OR principal='arxd')),
    class TEXT NOT NULL CHECK(class IN ('read','action','lifecycle')),
    tool TEXT NOT NULL CHECK(tool IS NULL OR (typeof(tool)='text' AND tool IN ('list_hosts','list_activity','unknown_tool','run_command','discover_tools','service_start','service_stop'))),
    outcome TEXT NOT NULL CHECK(outcome IS NULL OR (typeof(outcome)='text' AND outcome IN ('started','succeeded','failed','rejected','refused','interrupted'))),
    reason TEXT CHECK(reason IS NULL OR (typeof(reason)='text' AND reason IN ('unavailable','busy','quarantined','invalid_arguments','preflight_failed','storage_failed','execution_failed','identity_mismatch','cleanup_unconfirmed','shutdown'))),
    program TEXT CHECK(program IS NULL OR (typeof(program)='text' AND program IN ('cargo','rustc','rustup','git','python3','python','node','qemu-system-x86_64','qemu-img','path'))),
    argc INTEGER CHECK(argc IS NULL OR (typeof(argc)='integer' AND argc BETWEEN 0 AND 128)),
    rights TEXT CHECK(rights IS NULL OR (typeof(rights)='text' AND rights IN ('standard','admin'))),
    observed_kind TEXT CHECK(observed_kind IS NULL OR (typeof(observed_kind)='text' AND observed_kind IN ('windows_sid','unix_uid'))),
    observed_id TEXT CHECK(observed_id IS NULL OR (typeof(observed_id)='text' AND length(CAST(observed_id AS BLOB)) BETWEEN 1 AND 184 AND observed_id NOT GLOB '*[^S0-9-]*')),
    observed_gid INTEGER CHECK(observed_gid IS NULL OR (typeof(observed_gid)='integer' AND observed_gid BETWEEN 0 AND 4294967295)),
    integrity_rid INTEGER CHECK(integrity_rid IS NULL OR (typeof(integrity_rid)='integer' AND integrity_rid BETWEEN 0 AND 4294967295)),
    elevation INTEGER CHECK(elevation IS NULL OR (typeof(elevation)='integer' AND elevation IN (0,1))),
    observation_point TEXT CHECK(observation_point IS NULL OR (typeof(observation_point)='text' AND observation_point IN ('post_switch','pre_exec','child'))),
    identity_matches_pin INTEGER CHECK(identity_matches_pin IS NULL OR (typeof(identity_matches_pin)='integer' AND identity_matches_pin IN (0,1))),
    stop_reason TEXT CHECK(stop_reason IS NULL OR (typeof(stop_reason)='text' AND stop_reason IN ('exited','timeout','cancelled','output_limit','shutdown','failed'))),
    exit_code INTEGER CHECK(exit_code IS NULL OR (typeof(exit_code)='integer' AND exit_code BETWEEN -2147483648 AND 2147483647)),
    tree_boundary TEXT CHECK(tree_boundary IS NULL OR (typeof(tree_boundary)='text' AND tree_boundary IN ('windows_job','linux_cgroup','mac_uid_sweep','process_group'))),
    cleanup_confirmed INTEGER CHECK(cleanup_confirmed IS NULL OR (typeof(cleanup_confirmed)='integer' AND cleanup_confirmed IN (0,1))),
    started_at_ms INTEGER CHECK(started_at_ms IS NULL OR (typeof(started_at_ms)='integer' AND started_at_ms BETWEEN 0 AND 253402300799999)),
    finished_at_ms INTEGER CHECK(finished_at_ms IS NULL OR (typeof(finished_at_ms)='integer' AND finished_at_ms BETWEEN 0 AND 253402300799999)),CHECK(observed_id IS NULL OR (observed_kind IS NOT NULL AND ((observed_kind='unix_uid' AND observed_id NOT GLOB '*[^0-9]*' AND CAST(observed_id AS INTEGER)<=4294967295) OR (observed_kind='windows_sid' AND observed_id GLOB 'S-1-*-*' AND json_valid('['||replace(substr(observed_id,5),'-',',')||']'))))));
CREATE INDEX activity_visibility ON activity(principal,
    actor_kind,id);"#.into()
}

fn validate_columns(db: &Connection) -> Result<()> {
    let mut expected = vec![
        "id",
        "principal",
        "actor_kind",
        "class",
        "tool",
        "outcome",
        "started_at_ms",
        "finished_at_ms",
    ];
    expected.extend(FIELDS.iter().map(|(name, _)| *name));
    let mut stmt = db.prepare("SELECT CASE WHEN length(CAST(name AS BLOB))<=64 THEN name ELSE NULL END FROM pragma_table_info('activity') LIMIT 25")?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if names.len() != expected.len() || names.iter().any(|n| !expected.contains(&n.as_str())) {
        anyhow::bail!("invalid activity schema");
    }
    Ok(())
}

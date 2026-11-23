use super::{ApiError, Dashboard, instant_string, json_response};
use crate::{
    store::{Activity, ListActivity, Outcome},
    vm::{DashboardVm, Guest},
};
use axum::{extract::Query, http::Uri, response::Response};
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_INTEGER: u64 = 9_007_199_254_740_991;

pub(super) fn validate_id(id: &str) -> Result<(), ApiError> {
    if (1..=128).contains(&id.len())
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        Ok(())
    } else {
        Err(ApiError::INVALID)
    }
}

pub(super) async fn route(state: &Dashboard, uri: &Uri) -> Result<Response, ApiError> {
    let path = uri
        .path()
        .strip_prefix("/api/v1/")
        .ok_or(ApiError::NOT_FOUND)?;
    let (collection, target) = path
        .split_once('/')
        .map_or((path, None), |(collection, target)| {
            (collection, Some(target))
        });
    if let Some(id) = target {
        validate_id(id)?;
    }
    let mut query = None;
    if collection == "activity" && target.is_none() {
        if uri.query().is_some_and(|query| query.len() > 1024) {
            return Err(ApiError::INVALID);
        }
        query = Some(
            Query::<ActivityQuery>::try_from_uri(uri)
                .map_err(|_| ApiError::INVALID)?
                .0,
        );
    }
    let state = state.clone();
    let collection = collection.to_owned();
    let target = target.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let now = state.clock.now().utc_ms;
        let host_id = state
            .activity
            .host_id()
            .map_err(|_| ApiError::UNAVAILABLE)?;
        match (collection.as_str(), target.as_deref()) {
            ("hosts", Some(id)) if id != host_id => Err(ApiError::NOT_FOUND),
            ("hosts", target) => {
                let host = host(&state.activity, &host_id, now)?;
                json_response(if target.is_some() {
                    host
                } else {
                    json!({"hosts": [host]})
                })
            }
            ("activity", None) => {
                activity(&state.activity, &host_id, query.expect("activity query"))
            }
            ("vms", target) => {
                let exact = target
                    .map(|id| {
                        let mut decoded = [0; 16];
                        hex::decode_to_slice(id, &mut decoded).map_err(|_| ApiError::NOT_FOUND)?;
                        if hex::encode(decoded) != id {
                            return Err(ApiError::NOT_FOUND);
                        }
                        Ok(decoded)
                    })
                    .transpose()?;
                let mut vms = state
                    .vm_catalog
                    .as_ref()
                    .map(|catalog| catalog.list(exact, now))
                    .transpose()
                    .map_err(|_| ApiError::UNAVAILABLE)?
                    .unwrap_or_default()
                    .into_iter()
                    .map(|row| vm(row, &host_id))
                    .collect::<Result<Vec<_>, _>>()?;
                if target.is_some() {
                    json_response(vms.pop().ok_or(ApiError::NOT_FOUND)?)
                } else {
                    json_response(json!({"vms": vms}))
                }
            }
            _ => Err(ApiError::NOT_FOUND),
        }
    })
    .await
    .map_err(|_| ApiError::UNAVAILABLE)?
}

fn text(value: &str, limit: usize) -> String {
    // A Rust scalar can use two UTF-16 code units in the client.
    let mut budget = limit;
    value
        .chars()
        .take_while(|ch| {
            let len = ch.len_utf16();
            if len > budget {
                return false;
            }
            budget -= len;
            true
        })
        .collect()
}

fn host(activity: &Activity, id: &str, now: i64) -> Result<Value, ApiError> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    if !matches!(os, "windows" | "linux") || !matches!(arch, "x86_64" | "aarch64") {
        return Err(ApiError::UNAVAILABLE);
    }
    let arch = if arch == "aarch64" { "arm64" } else { arch };
    let mut system = sysinfo::System::new();
    system.refresh_cpu_all();
    system.refresh_memory();
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let (disk_total, disk_free) = disks.iter().fold((0u64, 0u64), |(total, free), disk| {
        (
            total.saturating_add(disk.total_space()),
            free.saturating_add(disk.available_space()),
        )
    });
    let tools = activity
        .tool_cache()
        .and_then(|cache| {
            Some(
                cache
                    .tools
                    .as_array()?
                    .iter()
                    .filter_map(|tool| {
                        if tool.get("found").and_then(Value::as_bool) == Some(false) {
                            return None;
                        }
                        let name = text(tool.get("name")?.as_str()?, 64);
                        let version = tool
                            .get("version")
                            .and_then(Value::as_str)
                            .map(|v| text(v.trim(), 64));
                        Some(json!({"name": name, "version": version}))
                    })
                    .take(128)
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    let name = sysinfo::System::host_name().unwrap_or_else(|| id.to_owned());
    let rights = activity
        .dedicated
        .as_ref()
        .map(|execution| execution.capability());
    let admin = rights
        .as_ref()
        .is_some_and(|value| value.get("rights").and_then(Value::as_str) == Some("admin"));
    Ok(json!({
        "id": id, "name": text(&name, 128), "os": os, "arch": arch,
        "osVersion": text(&sysinfo::System::long_os_version().unwrap_or_default(), 128),
        "cpu": {
            "model": text(system.cpus().first().map_or("", sysinfo::Cpu::brand), 128),
            "cores": system.cpus().len().clamp(1, 4096), "usedPercent": null
        },
        "memory": {"totalBytes": system.total_memory().min(MAX_INTEGER), "usedBytes": system.used_memory().min(MAX_INTEGER)},
        "disk": {"totalBytes": disk_total.min(MAX_INTEGER), "freeBytes": disk_free.min(MAX_INTEGER), "capBytes": null},
        "tools": tools, "reachability": "online", "reachabilityReason": null,
        "lastSeenAt": instant_string(now)?, "mode": "always_on",
        "agentRights": if admin {"admin"} else {"standard"},
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityQuery {
    #[serde(default = "default_limit")]
    limit: u8,
    before_id: Option<i64>,
    #[serde(default)]
    include_reads: bool,
    outcome: Option<String>,
    tool: Option<String>,
    host_id: Option<String>,
}

fn default_limit() -> u8 {
    20
}

fn activity(store: &Activity, host_id: &str, query: ActivityQuery) -> Result<Response, ApiError> {
    let args = ListActivity {
        limit: query.limit,
        before_id: query.before_id,
        include_reads: query.include_reads,
    };
    args.validate().map_err(|_| ApiError::INVALID)?;
    if query
        .before_id
        .is_some_and(|id| id.unsigned_abs() > MAX_INTEGER)
    {
        return Err(ApiError::INVALID);
    }
    let outcome = query
        .outcome
        .as_deref()
        .map(Outcome::parse)
        .transpose()
        .map_err(|_| ApiError::INVALID)?;
    if let Some(tool) = &query.tool
        && (tool.is_empty()
            || tool.len() > 64
            || !tool.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
    {
        return Err(ApiError::INVALID);
    }
    if let Some(host) = &query.host_id {
        validate_id(host)?;
        if host != host_id {
            return json_response(json!({"entries": [], "nextBeforeId": null}));
        }
    }
    let mut rows = store
        .dashboard_list(&args, outcome, query.tool.as_deref())
        .map_err(|_| ApiError::UNAVAILABLE)?;
    let more = rows.len() > usize::from(args.limit);
    rows.truncate(usize::from(args.limit));
    let before = rows.last().filter(|_| more).map(|row| row.id);
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        if row.id <= 0 || row.id.unsigned_abs() > MAX_INTEGER {
            return Err(ApiError::UNAVAILABLE);
        }
        entries.push(json!({
            "id": row.id, "tool": row.tool.as_str(), "outcome": row.outcome.as_str(),
            "actor": {"kind": row.actor, "name": row.name},
            "hostId": host_id, "vmId": null, "summary": row.tool.as_str(),
            "exitCode": row.exit_code, "startedAt": row.started.map(instant_string).transpose()?,
            "finishedAt": row.finished.map(instant_string).transpose()?,
        }));
    }
    json_response(json!({"entries": entries, "nextBeforeId": before}))
}

fn vm(row: DashboardVm, host_id: &str) -> Result<Value, ApiError> {
    let id = hex::encode(row.id);
    let (os, version) = match row.guest {
        Guest::UbuntuX86_64 => ("linux", "Ubuntu"),
        Guest::WindowsX86_64 => ("windows", "Windows"),
    };
    let mut snapshots = Vec::with_capacity(row.snapshots.len());
    for snapshot in row.snapshots {
        let id = hex::encode(snapshot.id);
        snapshots.push(json!({
            "id": id, "name": id, "createdAt": snapshot.created_at_ms.map(instant_string).transpose()?,
            "sizeBytes": snapshot.size.filter(|size| *size <= MAX_INTEGER)
        }));
    }
    Ok(json!({
        "id": id, "name": id, "kind": "test", "template": {"id": hex::encode(row.template), "version": 1},
        "guestOs": os, "guestOsVersion": version, "arch": "x86_64", "hostId": host_id,
        "state": row.state, "ready": false, "expiresAt": instant_string(row.expires_at_ms)?,
        "cpus": row.cpus, "memoryBytes": row.memory, "diskBytes": row.disk.min(MAX_INTEGER),
        "snapshots": snapshots
    }))
}

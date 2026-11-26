use super::{ApiError, Dashboard, instant_string, json_response};
use crate::{
    store::{Activity, ListActivity, Outcome},
    vm::{DashboardVm, Guest},
};
use axum::{extract::Query, http::Uri, response::Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

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
                let host = host(&state.activity, &host_id, now, &state.host)?;
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

/// One decimal place, half away from zero (`f64::round`).
/// Non-finite values and values outside 0..=100 are `None` (JSON null).
fn cpu_used_percent(raw: f64) -> Option<f64> {
    if !raw.is_finite() || !(0.0..=100.0).contains(&raw) {
        return None;
    }
    let scaled = (raw * 10.0).round();
    if !scaled.is_finite() {
        return None;
    }
    let rounded = scaled / 10.0;
    (0.0..=100.0).contains(&rounded).then_some(rounded)
}

fn used_percent_value(used: Option<f64>) -> Value {
    used.and_then(serde_json::Number::from_f64)
        .map_or(Value::Null, Value::Number)
}

fn cpu_object(model: &str, cores: usize, used: Option<f64>) -> Value {
    json!({
        "model": text(model, 128),
        "cores": cores.clamp(1, 4096),
        "usedPercent": used_percent_value(used),
    })
}

/// Usage is published only after two accepted samples. `None` means the
/// sysinfo interval has not elapsed: it is not a sample, and the last
/// published value is left unchanged. A non-finite or out-of-range sample clears it.
#[derive(Default)]
struct CpuWindow {
    accepted: u8,
    published: Option<f64>,
}

impl CpuWindow {
    fn observe(&mut self, raw: Option<f32>) -> Option<f64> {
        if let Some(raw) = raw {
            self.accepted = self.accepted.saturating_add(1);
            if self.accepted >= 2 {
                self.published = cpu_used_percent(f64::from(raw));
            }
        }
        if self.accepted >= 2 {
            self.published
        } else {
            None
        }
    }
}

/// A refresh closer than this does not produce a new sysinfo usage sample.
/// The sampler records the gap and moves on; it does not sleep.
fn cpu_sample_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|last| {
        now.saturating_duration_since(last) >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL
    })
}

/// Background refresh period of the host sampler.
const SAMPLE_PERIOD: Duration = Duration::from_secs(1);
/// Lower bound for any sampler period, also where sysinfo reports a zero minimum.
const SAMPLE_FLOOR: Duration = Duration::from_millis(100);

/// Never below sysinfo's minimum CPU interval, so each tick can take a sample.
fn sample_period(requested: Duration) -> Duration {
    requested
        .max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)
        .max(SAMPLE_FLOOR)
}

/// One native refresh of host facts. Tests replace this boundary.
pub(super) trait HostSource: Send + 'static {
    /// Refreshes memory, and CPU usage when `sample_cpu` is true. `cpu` is
    /// `None` when no usage sample was taken.
    fn refresh(&mut self, sample_cpu: bool) -> HostReading;
}

pub(super) struct HostReading {
    pub(super) model: String,
    pub(super) cores: usize,
    pub(super) total_memory: u64,
    pub(super) used_memory: u64,
    pub(super) cpu: Option<f32>,
}

/// The production source: one long-lived `sysinfo::System`, created by the
/// first refresh on the blocking pool rather than at daemon startup.
struct SysinfoSource(Option<sysinfo::System>);

impl HostSource for SysinfoSource {
    fn refresh(&mut self, sample_cpu: bool) -> HostReading {
        let system = self.0.get_or_insert_with(sysinfo::System::new);
        let cpu = if sample_cpu {
            if system.cpus().is_empty() {
                system.refresh_cpu_all();
            } else {
                system.refresh_cpu_usage();
            }
            // An empty list is not a sample, so the next tick can try again.
            accepted_cpu_sample(!system.cpus().is_empty(), system.global_cpu_usage())
        } else {
            None
        };
        system.refresh_memory();
        HostReading {
            model: system
                .cpus()
                .first()
                .map(|cpu| cpu.brand().to_owned())
                .unwrap_or_default(),
            cores: system.cpus().len(),
            total_memory: system.total_memory(),
            used_memory: system.used_memory(),
            cpu,
        }
    }
}

fn accepted_cpu_sample(has_cpus: bool, usage: f32) -> Option<f32> {
    has_cpus.then_some(usage)
}

/// Sampling state around a source. The first accepted sample only opens
/// the window; `usedPercent` stays null until the next one.
struct HostProbe<S> {
    source: S,
    last_cpu: Option<Instant>,
    window: CpuWindow,
}

#[derive(Clone)]
pub(super) struct HostFacts {
    pub(super) model: String,
    pub(super) cores: usize,
    pub(super) total_memory: u64,
    pub(super) used_memory: u64,
    pub(super) used_percent: Option<f64>,
}

impl HostFacts {
    fn inventory(&self) -> HostInventory {
        HostInventory {
            model: self.model.clone(),
            cores: self.cores,
            total_memory: self.total_memory,
        }
    }
}

/// Static host facts. They do not change while the daemon runs, so they stay
/// valid after the sampler fails.
#[derive(Clone)]
pub(super) struct HostInventory {
    pub(super) model: String,
    pub(super) cores: usize,
    pub(super) total_memory: u64,
}

/// What a host read reports. Telemetry is `None` unless a live sample is
/// published; `inventory` is `None` until any sample has been taken.
pub(super) struct HostView {
    pub(super) inventory: Option<HostInventory>,
    pub(super) used_memory: Option<u64>,
    pub(super) used_percent: Option<f64>,
}

impl<S: HostSource> HostProbe<S> {
    fn new(source: S) -> Self {
        Self {
            source,
            last_cpu: None,
            window: CpuWindow::default(),
        }
    }

    /// `now` is read before the refresh (is a CPU sample due?) and after it
    /// (the stamp), so the next gap starts at or after sysinfo's own.
    fn sample(&mut self, mut now: impl FnMut() -> Instant) -> HostFacts {
        let reading = self.source.refresh(cpu_sample_due(self.last_cpu, now()));
        if reading.cpu.is_some() {
            self.last_cpu = Some(now());
        }
        HostFacts {
            model: reading.model,
            cores: reading.cores,
            total_memory: reading.total_memory,
            used_memory: reading.used_memory,
            used_percent: self.window.observe(reading.cpu),
        }
    }
}

/// Published sampler state. `Failed` keeps the inventory of the last
/// sample, if there was one.
#[derive(Clone)]
enum HostState {
    Pending,
    Ready(HostFacts),
    Failed(Option<HostInventory>),
}

/// Latest host figures, refreshed by one background task. Host reads copy
/// the published state and never refresh, so a read never waits for a
/// native CPU or memory refresh, its own or another request's. Every
/// refresh, the first included, runs on the blocking pool, so creating the
/// sampler never blocks daemon startup. Telemetry problems never fail a host
/// read: before the first sample, or after a refresh panic, the read reports
/// null telemetry. The task stops when the last clone (held by the dashboard
/// state) is dropped.
#[derive(Clone)]
pub(super) struct HostSampler {
    latest: tokio::sync::watch::Receiver<HostState>,
    _task: Arc<SamplerTask>,
}

struct SamplerTask(tokio::task::JoinHandle<()>);

impl Drop for SamplerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl HostSampler {
    pub(super) fn native() -> Self {
        Self::start(SysinfoSource(None), SAMPLE_PERIOD)
    }

    /// Spawns the sampler: the first refresh starts at once, later ones every
    /// `period` (see [`sample_period`]). Must be called inside a Tokio runtime.
    pub(super) fn start(source: impl HostSource, period: Duration) -> Self {
        let period = sample_period(period);
        let (slot, latest) = tokio::sync::watch::channel(HostState::Pending);
        let task = tokio::spawn(async move {
            let slot = Arc::new(slot);
            let mut probe = HostProbe::new(source);
            let mut ticks = tokio::time::interval(period);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                // The first tick completes immediately.
                ticks.tick().await;
                let target = Arc::clone(&slot);
                match tokio::task::spawn_blocking(move || {
                    publish(&mut probe, &target);
                    probe
                })
                .await
                {
                    Ok(next) => probe = next,
                    Err(_) => {
                        // Tokio caught the panic on the blocking pool. Keep the
                        // static inventory; stop publishing telemetry.
                        slot.send_modify(|state| {
                            let inventory = match state {
                                HostState::Ready(facts) => Some(facts.inventory()),
                                HostState::Failed(inventory) => inventory.clone(),
                                HostState::Pending => None,
                            };
                            *state = HostState::Failed(inventory);
                        });
                        eprintln!("arxd: dashboard host sampler stopped after a refresh failure");
                        return;
                    }
                }
            }
        });
        Self {
            latest,
            _task: Arc::new(SamplerTask(task)),
        }
    }

    /// Copies the last published figures. The channel lock is only ever held
    /// for a copy or a swap, never across a refresh.
    pub(super) fn latest(&self) -> HostView {
        match &*self.latest.borrow() {
            HostState::Ready(facts) => HostView {
                inventory: Some(facts.inventory()),
                used_memory: Some(facts.used_memory),
                used_percent: facts.used_percent,
            },
            HostState::Failed(inventory) => HostView {
                inventory: inventory.clone(),
                used_memory: None,
                used_percent: None,
            },
            HostState::Pending => HostView {
                inventory: None,
                used_memory: None,
                used_percent: None,
            },
        }
    }

    /// Waits until the first sample (or a failure) is published.
    #[cfg(test)]
    pub(super) async fn settled(&self) {
        self.wait(|state| !matches!(state, HostState::Pending))
            .await;
    }

    /// Waits until a refresh failure is published.
    #[cfg(test)]
    pub(super) async fn failed(&self) {
        self.wait(|state| matches!(state, HostState::Failed(_)))
            .await;
    }

    /// Waits until a published sample satisfies `done`.
    #[cfg(test)]
    pub(super) async fn sampled(&self, mut done: impl FnMut(&HostFacts) -> bool) {
        self.wait(|state| matches!(state, HostState::Ready(facts) if done(facts)))
            .await;
    }

    #[cfg(test)]
    async fn wait(&self, done: impl FnMut(&HostState) -> bool) {
        let mut latest = self.latest.clone();
        let _ = latest.wait_for(done).await;
    }
}

/// Refreshes outside the channel lock, then swaps the result in.
fn publish<S: HostSource>(probe: &mut HostProbe<S>, slot: &tokio::sync::watch::Sender<HostState>) {
    let facts = probe.sample(Instant::now);
    slot.send_replace(HostState::Ready(facts));
}

/// Inventory for a read before any sample exists (pending, or the first
/// refresh failed): a memory-only native read local to this request, with no
/// CPU counters and no shared state; the available parallelism; no model.
fn fallback_inventory() -> HostInventory {
    let mut system = sysinfo::System::new();
    system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
    HostInventory {
        model: String::new(),
        cores: std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get),
        total_memory: system.total_memory(),
    }
}

fn host(activity: &Activity, id: &str, now: i64, sampler: &HostSampler) -> Result<Value, ApiError> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    if !matches!(os, "windows" | "linux") || !matches!(arch, "x86_64" | "aarch64") {
        return Err(ApiError::UNAVAILABLE);
    }
    let arch = if arch == "aarch64" { "arm64" } else { arch };
    let view = sampler.latest();
    let inventory = view.inventory.unwrap_or_else(fallback_inventory);
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
        "cpu": cpu_object(&inventory.model, inventory.cores, view.used_percent),
        "memory": {
            "totalBytes": inventory.total_memory.min(MAX_INTEGER),
            "usedBytes": view.used_memory.map(|used| used.min(MAX_INTEGER)),
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Scripted native boundary. Each requested CPU sample takes the next
    /// scripted value; `None` (or an exhausted script) is not a sample.
    struct ScriptedSource {
        cpu: std::collections::VecDeque<Option<f32>>,
        requested: Vec<bool>,
    }

    impl ScriptedSource {
        fn new(cpu: impl IntoIterator<Item = Option<f32>>) -> Self {
            Self {
                cpu: cpu.into_iter().collect(),
                requested: Vec::new(),
            }
        }
    }

    impl HostSource for ScriptedSource {
        fn refresh(&mut self, sample_cpu: bool) -> HostReading {
            self.requested.push(sample_cpu);
            let cpu = if sample_cpu {
                self.cpu.pop_front().flatten()
            } else {
                None
            };
            HostReading {
                model: "Test CPU".into(),
                cores: 4,
                total_memory: 8 << 30,
                used_memory: 2 << 30,
                cpu,
            }
        }
    }

    /// A synthetic clock fixed at `at`.
    fn at(at: Instant) -> impl FnMut() -> Instant {
        move || at
    }

    #[test]
    fn cpu_used_percent_rounds_one_decimal_and_drops_out_of_range() {
        // 37.4 is already one decimal and stays 37.4. Nearby values use
        // half-away-from-zero rounding at the first decimal.
        assert_eq!(cpu_used_percent(37.4), Some(37.4));
        assert_eq!(cpu_used_percent(37.44), Some(37.4));
        assert_eq!(cpu_used_percent(37.46), Some(37.5));
        assert_eq!(cpu_used_percent(0.0), Some(0.0));
        assert_eq!(cpu_used_percent(100.0), Some(100.0));
        assert_eq!(cpu_used_percent(f64::NAN), None);
        assert_eq!(cpu_used_percent(f64::INFINITY), None);
        assert_eq!(cpu_used_percent(f64::NEG_INFINITY), None);
        assert_eq!(cpu_used_percent(-0.1), None);
        assert_eq!(cpu_used_percent(-1.0), None);
        assert_eq!(cpu_used_percent(100.1), None);
        assert_eq!(cpu_used_percent(100.04), None);
        assert_eq!(cpu_used_percent(-0.04), None);
        assert_eq!(cpu_used_percent(140.0), None);
    }

    #[test]
    fn host_probe_publishes_usage_only_after_two_samples_with_synthetic_timestamps() {
        let min = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL;
        let start = Instant::now();
        let tick = |n: u32| start + min * n;
        let mut probe = HostProbe::new(ScriptedSource::new([
            None,
            Some(4.0),
            Some(37.44),
            Some(140.0),
            Some(12.0),
            Some(f32::NAN),
            Some(-1.0),
        ]));
        // A due poll without a sample (empty CPU list) is not a sample and
        // is not stamped, so the next poll is due again.
        let gap = probe.sample(at(start));
        assert_eq!(gap.used_percent, None);
        assert_eq!(gap.model, "Test CPU");
        assert_eq!(
            (gap.cores, gap.total_memory, gap.used_memory),
            (4, 8 << 30, 2 << 30)
        );
        // The first real sample opens the window: still null.
        assert_eq!(probe.sample(at(start)).used_percent, None);
        // Inside the minimum interval no CPU sample is requested.
        if !min.is_zero() {
            assert_eq!(probe.sample(at(start)).used_percent, None);
        }
        // The second sample publishes, rounded to one decimal.
        assert_eq!(probe.sample(at(tick(1))).used_percent, Some(37.4));
        // An early read keeps the published value.
        if !min.is_zero() {
            assert_eq!(probe.sample(at(tick(1))).used_percent, Some(37.4));
        }
        // Out-of-range and non-finite samples clear it; a valid one restores it.
        assert_eq!(probe.sample(at(tick(2))).used_percent, None);
        assert_eq!(probe.sample(at(tick(3))).used_percent, Some(12.0));
        assert_eq!(probe.sample(at(tick(4))).used_percent, None);
        assert_eq!(probe.sample(at(tick(5))).used_percent, None);
        let expected = if min.is_zero() {
            vec![true; 7]
        } else {
            vec![true, true, false, true, false, true, true, true, true]
        };
        assert_eq!(probe.source.requested, expected);
    }

    #[test]
    fn sampler_period_never_drops_below_the_cpu_minimum() {
        let floor = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(SAMPLE_FLOOR);
        assert_eq!(sample_period(Duration::ZERO), floor);
        assert_eq!(sample_period(SAMPLE_PERIOD), SAMPLE_PERIOD.max(floor));
        assert!(SAMPLE_PERIOD >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    }

    #[test]
    fn cpu_sample_is_due_only_after_the_minimum_interval() {
        let start = Instant::now();
        let interval = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL;
        assert!(cpu_sample_due(None, start));
        assert_eq!(cpu_sample_due(Some(start), start), interval.is_zero());
        if let Some(early) = interval.checked_sub(Duration::from_nanos(1)) {
            assert!(!cpu_sample_due(Some(start), start + early));
        }
        assert!(cpu_sample_due(Some(start), start + interval));
    }

    #[test]
    fn empty_cpu_list_is_not_a_sample() {
        assert_eq!(accepted_cpu_sample(false, 0.0), None);
        assert_eq!(accepted_cpu_sample(true, 12.5), Some(12.5));
    }
}

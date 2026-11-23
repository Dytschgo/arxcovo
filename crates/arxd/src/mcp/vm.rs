//! Foreground-only VM tools. This module is selected after local authentication.
use crate::{
    config::{TestVms, VmRuntimeProfile},
    service::Shutdown,
    store::ListActivity,
    vm::{runtime::NativeRefusal, *},
};
use ring::hmac;
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
struct ForegroundClock {
    #[cfg(test)]
    utc: Option<Arc<std::sync::atomic::AtomicI64>>,
}

impl Clock for ForegroundClock {
    fn now(&self) -> ClockReading {
        #[cfg(test)]
        if let Some(now) = &self.utc {
            return ClockReading {
                utc_ms: now.load(Ordering::Relaxed),
                monotonic: Instant::now(),
            };
        }
        ClockReading {
            utc_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
                .unwrap_or(-1),
            monotonic: Instant::now(),
        }
    }
}

type Controller = VmController<ForegroundClock>;
type Outcome<T> = std::result::Result<T, Fault>;

#[derive(Clone, Copy, Debug)]
struct Fault(&'static str);

const INVALID: Fault = Fault("invalid");
const INVALID_CURSOR: Fault = Fault("invalid_cursor");
const SHUTDOWN: Fault = Fault("shutdown");
const NOT_FOUND: Fault = Fault("not_found");
const STATE_UNAVAILABLE: Fault = Fault("state_unavailable");
const NOT_READY: Fault = Fault("not_ready");
const HOST_EXECUTION_DISABLED: Fault = Fault("host_execution_disabled");

impl From<anyhow::Error> for Fault {
    fn from(_: anyhow::Error) -> Self {
        STATE_UNAVAILABLE
    }
}

/// The single checked controller-to-wire refusal projection.
fn refusal_wire(reason: Refusal) -> &'static str {
    match reason {
        Refusal::Invalid => "invalid",
        Refusal::StaleKey => "stale_key",
        Refusal::KeyCollision => "key_collision",
        Refusal::ReplayFull => "replay_full",
        Refusal::PendingFull => "pending_full",
        Refusal::NotFound => "not_found",
        Refusal::Busy => "busy",
        Refusal::Expired => "expired",
        Refusal::TemplateLimit => "template_limit",
        Refusal::InstanceLimit => "instance_limit",
        Refusal::SnapshotLimit => "snapshot_limit",
        Refusal::Disk => "disk",
        Refusal::StateHeadroom => "state_headroom",
        Refusal::UnknownHost => "unknown_host",
        Refusal::OwnerActive => "owner_active",
        Refusal::Cpu => "cpu",
        Refusal::Ram => "ram",
        Refusal::Shutdown => "shutdown",
        Refusal::CleanupUnconfirmed => "cleanup_unconfirmed",
    }
}

/// Stored audit reason codes; an unknown future code is never guessed.
fn refusal_code(code: i64) -> Option<Refusal> {
    REFUSAL_CODES
        .into_iter()
        .find(|reason| *reason as i64 == code)
}

/// The stored code for a wire reason, only where the codes match exactly.
fn refusal_from_wire(reason: &str) -> Option<Refusal> {
    REFUSAL_CODES
        .into_iter()
        .find(|code| refusal_wire(*code) == reason)
}

/// The codes the v7 audit stores exactly (1..=18). `cleanup_unconfirmed` has
/// no stored code yet, so the foreground never files it as another reason.
const REFUSAL_CODES: [Refusal; 18] = {
    use Refusal::*;
    [
        Invalid,
        StaleKey,
        KeyCollision,
        ReplayFull,
        PendingFull,
        NotFound,
        Busy,
        Expired,
        TemplateLimit,
        InstanceLimit,
        SnapshotLimit,
        Disk,
        StateHeadroom,
        UnknownHost,
        OwnerActive,
        Cpu,
        Ram,
        Shutdown,
    ]
};

impl From<Refusal> for Fault {
    fn from(reason: Refusal) -> Self {
        Self(refusal_wire(reason))
    }
}

fn native_reason(reason: NativeRefusal) -> &'static str {
    use NativeRefusal::*;
    match reason {
        Disabled => "disabled",
        UnsupportedPlatform => "unsupported_platform",
        QualificationMissing => "qualification_missing",
        IdentityUnverified => "identity_unverified",
        DistributionUnverified => "distribution_unverified",
        PathCustodyUnverified => "path_custody_unverified",
        JobUnverified => "job_unverified",
        WhpxUnverified => "whpx_unverified",
        QmpUnverified => "qmp_unverified",
        GuestEndpointUnverified => "guest_endpoint_unverified",
        DiskCeilingUnverified => "disk_ceiling_unverified",
        OwnerActivityUnverified => "owner_activity_unverified",
        ReservesUnverified => "reserves_unverified",
        NetworkFenceUnverified => "network_fence_unverified",
        InstallerPhaseUnavailable => "installer_phase_unavailable",
    }
}

struct Gate {
    closed: bool,
    #[cfg(test)]
    fixture: Option<tests::Fixture>,
    #[cfg(test)]
    drained: Option<tests::FixtureDrain>,
}

pub(crate) struct Supervisor {
    controller: Arc<Controller>,
    gate: Mutex<Gate>,
    cancel: Shutdown,
    cursor_key: hmac::Key,
    host_id: Id,
    recipe: Option<(Id, [u8; 32])>,
    native_refusal: NativeRefusal,
    /// The validated native runtime profile, if configured. It only selects
    /// the closed candidate runtime; it never authorizes a launch here.
    runtime: Option<VmRuntimeProfile>,
    #[cfg(test)]
    ticks: Mutex<Option<tests::TestTicks>>,
}

impl Supervisor {
    pub(crate) fn open(root: &Path, mut profile: TestVms) -> anyhow::Result<Arc<Self>> {
        let reason = if !profile.enabled {
            NativeRefusal::Disabled
        } else if cfg!(all(windows, target_arch = "x86_64")) {
            NativeRefusal::QualificationMissing
        } else {
            NativeRefusal::UnsupportedPlatform
        };
        let policy = profile.policy();
        let recipe = profile.recipe()?;
        Self::new(
            root,
            policy,
            ForegroundClock {
                #[cfg(test)]
                utc: None,
            },
            recipe,
            reason,
            profile.runtime.take(),
        )
    }

    fn new(
        root: &Path,
        policy: VmPolicy,
        clock: ForegroundClock,
        recipe: Option<(Id, [u8; 32])>,
        native_refusal: NativeRefusal,
        runtime: Option<VmRuntimeProfile>,
    ) -> anyhow::Result<Arc<Self>> {
        let mut secret = [0; 32];
        let mut host_id = [0; 16];
        getrandom::fill(&mut secret).map_err(|_| anyhow::anyhow!("cursor key unavailable"))?;
        getrandom::fill(&mut host_id).map_err(|_| anyhow::anyhow!("host ID unavailable"))?;
        Ok(Arc::new(Self {
            controller: Arc::new(VmController::open(root, policy, clock)?),
            gate: Mutex::new(Gate {
                closed: false,
                #[cfg(test)]
                fixture: None,
                #[cfg(test)]
                drained: None,
            }),
            cancel: Shutdown::default(),
            cursor_key: hmac::Key::new(hmac::HMAC_SHA256, &secret),
            host_id,
            recipe,
            native_refusal,
            runtime,
            #[cfg(test)]
            ticks: Mutex::new(None),
        }))
    }

    pub(crate) fn connection(self: &Arc<Self>, principal: String) -> Connection {
        Connection(VmInventory {
            supervisor: self.clone(),
            closed: Arc::new(AtomicBool::new(false)),
            principal,
        })
    }

    fn lock(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Gate>> {
        self.gate
            .lock()
            .map_err(|_| anyhow::anyhow!("VM supervisor unavailable"))
    }

    /// Close foreground admission, then controller admission. The same lock
    /// covers connection close, request cancellation sampling and the
    /// complete SQLite commit and worker registration.
    pub(crate) fn close(&self) {
        if let Ok(mut gate) = self.gate.lock() {
            gate.closed = true;
        }
        // A failed close leaves the controller gate poisoned, which already
        // refuses every admission; teardown continues either way.
        let _ = self.controller.close_admission();
        self.cancel.cancel();
    }

    fn candidate(&self) -> runtime::CandidateRuntime<'_> {
        runtime::CandidateRuntime::new(self.runtime.as_ref())
    }

    /// One bounded reaper turn before the listener is published.
    pub(crate) fn startup_sweep(&self) -> anyhow::Result<()> {
        self.sweep()
    }

    fn sweep(&self) -> anyhow::Result<()> {
        #[cfg(test)]
        if let Some(reaper) = self.lock()?.fixture.as_ref().map(tests::Fixture::reaper) {
            self.controller.sweep_with(&reaper, &*reaper.custody)?;
            return Ok(());
        }
        self.controller.sweep_once(&self.candidate())?;
        Ok(())
    }

    /// The supervised periodic reaper; the caller cancels and joins it.
    pub(crate) async fn run_reaper(&self, stop: &Shutdown) -> anyhow::Result<()> {
        #[cfg(test)]
        if let Some(mut ticks) = self.ticks.lock().ok().and_then(|mut ticks| ticks.take()) {
            let reaper = self.lock()?.fixture.as_ref().map(tests::Fixture::reaper);
            return match reaper {
                Some(reaper) => {
                    self.controller
                        .run_reaper_with(&reaper, &*reaper.custody, &mut ticks, stop)
                        .await
                }
                None => {
                    self.controller
                        .run_reaper(&self.candidate(), &mut ticks, stop)
                        .await
                }
            };
        }
        self.controller
            .run_reaper(
                &self.candidate(),
                &mut MonotonicReaperTicks::default(),
                stop,
            )
            .await
    }

    /// Cancel and join every admitted worker and its terminal audit write.
    /// Admission is closed first, so no new worker can be registered.
    pub(crate) async fn drain_workers(&self, bound: Duration) -> anyhow::Result<()> {
        self.close();
        #[cfg(test)]
        {
            let jobs = self.lock()?.fixture.as_mut().map(tests::Fixture::take_jobs);
            if let Some(jobs) = jobs {
                let drained = tokio::time::timeout(bound, jobs.join()).await??;
                self.lock()?.drained = Some(drained);
            }
        }
        let _ = bound;
        Ok(())
    }

    /// Final controller recovery, after the reaper and workers are joined,
    /// then one more custody turn. Unresolved custody, including build or
    /// staging quarantine and legacy uncertain rows, is an unconfirmed drain.
    /// The blocking task stays owned until it returns, even past `bound`, so
    /// it never writes after this call reports.
    pub(crate) async fn recover(self: &Arc<Self>, bound: Duration) -> anyhow::Result<()> {
        #[cfg(test)]
        {
            let drained = self.lock()?.drained.take();
            if let Some(drained) = drained {
                drained.release().await?;
            }
        }
        let supervisor = self.clone();
        let mut task = tokio::task::spawn_blocking(move || {
            supervisor.controller.recover_after_drain()?;
            supervisor.sweep()?;
            anyhow::ensure!(
                !supervisor.controller.foreground_custody_uncertain()?,
                "VM release unconfirmed"
            );
            Ok(())
        });
        match tokio::time::timeout(bound, &mut task).await {
            Ok(result) => result?,
            Err(_) => {
                let _ = task.await;
                anyhow::bail!("VM recovery unconfirmed")
            }
        }
    }

    fn capability(&self, gate: &Gate) -> Value {
        #[cfg(test)]
        if gate.fixture.is_some() {
            return json!({"available":true});
        }
        let _ = gate;
        json!({"available":false,"reason":native_reason(self.native_refusal)})
    }
}

pub(crate) struct Connection(VmInventory);

impl Connection {
    pub(crate) fn inventory(&self) -> VmInventory {
        self.0.clone()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _gate = self.0.supervisor.gate.lock();
        self.0.closed.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub(crate) struct VmInventory {
    supervisor: Arc<Supervisor>,
    closed: Arc<AtomicBool>,
    principal: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolName {
    Hosts,
    Activity,
    Catalog,
    List,
    Build,
    Create,
    Get,
    Operation,
    Start,
    Stop,
    Script,
    Extend,
}

impl ToolName {
    const ALL: [Self; 12] = [
        Self::Hosts,
        Self::Activity,
        Self::Catalog,
        Self::List,
        Self::Build,
        Self::Create,
        Self::Get,
        Self::Operation,
        Self::Start,
        Self::Stop,
        Self::Script,
        Self::Extend,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Hosts => "list_hosts",
            Self::Activity => "list_activity",
            Self::Catalog => "vm_catalog",
            Self::List => "vm_list",
            Self::Build => "vm_template_build",
            Self::Create => "vm_create",
            Self::Get => "vm_get",
            Self::Operation => "vm_operation",
            Self::Start => "vm_start",
            Self::Stop => "vm_stop",
            Self::Script => "vm_run_script",
            Self::Extend => "vm_extend",
        }
    }

    fn action(self) -> Option<i64> {
        Some(match self {
            Self::Build => BUILD_ACTION,
            Self::Create => Action::Clone as i64,
            Self::Start => Action::Start as i64,
            Self::Stop => Action::Stop as i64,
            Self::Script => Action::Script as i64,
            Self::Extend => Action::Extend as i64,
            _ => return None,
        })
    }

    fn schema(self) -> Tool {
        let id = json!({"type":"string","pattern":"^[0-9a-f]{32}$"});
        let ttl = json!({"type":"integer","minimum":60,"maximum":2592000});
        let (description, properties, required): (_, _, &[&str]) = match self {
            Self::Hosts => ("List safe host facts", json!({}), &[]),
            Self::Activity => (
                "List caller-scoped VM receipts",
                json!({
                    "limit":{"type":"integer","minimum":1,"maximum":50,"default":20},
                    "before_id":{"type":"integer","minimum":1,"maximum":i64::MAX},
                    "activity_id":{"type":"integer","minimum":1,"maximum":i64::MAX},
                    "include_reads":{"type":"boolean","default":false}
                }),
                &[],
            ),
            Self::Catalog | Self::List => (
                if self == Self::Catalog {
                    "List VM recipes, templates and caller builds"
                } else {
                    "List caller-owned VMs"
                },
                json!({"limit":{"type":"integer","minimum":1,"maximum":20,"default":20},
                    "cursor":{"type":"string","maxLength":96,"pattern":"^[A-Za-z0-9_-]+$"}}),
                &[],
            ),
            Self::Build => (
                "Build a pinned template",
                json!({"recipe_id":id,"request_id":id}),
                &["recipe_id", "request_id"],
            ),
            Self::Create => (
                "Create a stopped clone",
                json!({"template_id":id,"request_id":id,"ttl_s":ttl}),
                &["template_id", "request_id"],
            ),
            Self::Get => ("Read a caller-owned VM", json!({"vm_id":id}), &["vm_id"]),
            Self::Operation => (
                "Read a caller-owned operation",
                json!({"operation_id":id}),
                &["operation_id"],
            ),
            Self::Start => (
                "Start a caller-owned VM",
                json!({"vm_id":id,"request_id":id}),
                &["vm_id", "request_id"],
            ),
            Self::Stop => (
                "Stop a caller-owned VM",
                json!({"vm_id":id,"request_id":id}),
                &["vm_id", "request_id"],
            ),
            Self::Script => (
                "Run a bounded guest script",
                json!({"vm_id":id,"request_id":id,
                "script":{"type":"string","minLength":1,"maxLength":4096},
                "timeout_s":{"type":"integer","minimum":1,"maximum":120,"default":30}}),
                &["vm_id", "request_id", "script"],
            ),
            Self::Extend => (
                "Extend a caller-owned VM lease",
                json!({"vm_id":id,"request_id":id,"ttl_s":ttl}),
                &["vm_id", "request_id", "ttl_s"],
            ),
        };
        let mut schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        if self == Self::Activity {
            schema["not"] = json!({"required":["activity_id","before_id"]});
        }
        Tool::new(
            self.name(),
            description,
            Arc::new(schema.as_object().expect("tool schema").clone()),
        )
    }
}

impl ServerHandler for VmInventory {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: ToolName::ALL.map(ToolName::schema).into(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let result = self.dispatch(request, || context.ct.is_cancelled());
        let (value, error) = match result {
            Ok(mut object) => {
                object["ok"] = Value::Bool(true);
                (object, false)
            }
            Err(reason) => (json!({"ok":false,"reason":reason.0}), true),
        };
        let mut result = CallToolResult::structured(value);
        result.content.clear();
        result.is_error = Some(error);
        Ok(result.into())
    }
}

struct Args(Map<String, Value>);

impl Args {
    /// The authenticated frame pump replaces a present non-object argument
    /// value with this reserved key, so it fails as `invalid` here instead of
    /// the SDK erasing `null` into an empty object.
    fn new(arguments: Map<String, Value>) -> Self {
        Self(arguments)
    }

    fn malformed(&self) -> bool {
        self.0.contains_key(crate::local::INVALID_TOOL_ARGUMENTS)
    }

    fn optional<T: DeserializeOwned>(&mut self, name: &str) -> Outcome<Option<T>> {
        self.0
            .remove(name)
            .map(|value| {
                if value.is_null() {
                    return Err(INVALID);
                }
                serde_json::from_value(value).map_err(|_| INVALID)
            })
            .transpose()
    }

    fn get<T: DeserializeOwned>(&mut self, name: &str) -> Outcome<T> {
        self.optional(name)?.ok_or(INVALID)
    }

    fn id(&mut self, name: &str) -> Outcome<Id> {
        parse_id(&self.get::<String>(name)?)
    }

    fn end(&self) -> Outcome<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(INVALID)
        }
    }
}

fn parse_id(value: &str) -> Outcome<Id> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(INVALID);
    }
    let mut id = [0; 16];
    hex::decode_to_slice(value, &mut id).map_err(|_| INVALID)?;
    Ok(id)
}

enum Work {
    Build,
    Create(u64),
    Start,
    Stop,
    Script(
        #[cfg_attr(
            not(test),
            allow(dead_code, reason = "native channel issuance remains closed")
        )]
        String,
        #[cfg_attr(
            not(test),
            allow(dead_code, reason = "native channel issuance remains closed")
        )]
        u32,
    ),
    Extend(
        #[cfg_attr(
            not(test),
            allow(dead_code, reason = "unqualified fresh extensions remain closed")
        )]
        u64,
    ),
}

struct Mutation {
    key: RequestId,
    id: Id,
    work: Work,
}

impl Mutation {
    fn parse(tool: ToolName, args: &mut Args) -> Outcome<Self> {
        let id = args.id(match tool {
            ToolName::Build => "recipe_id",
            ToolName::Create => "template_id",
            _ => "vm_id",
        })?;
        let key = RequestId::parse(&args.get::<String>("request_id")?)?;
        let work = match tool {
            ToolName::Build => Work::Build,
            ToolName::Create | ToolName::Extend => {
                let ttl = args
                    .optional("ttl_s")?
                    .or((tool == ToolName::Create).then_some(DEFAULT_LEASE_SECONDS))
                    .filter(|ttl| (60..=2_592_000).contains(ttl))
                    .ok_or(INVALID)?;
                if tool == ToolName::Create {
                    Work::Create(ttl)
                } else {
                    Work::Extend(ttl)
                }
            }
            ToolName::Start => Work::Start,
            ToolName::Stop => Work::Stop,
            ToolName::Script => {
                let script: String = args.get("script")?;
                let timeout = args.optional("timeout_s")?.unwrap_or(30);
                if script.is_empty()
                    || script.len() > 4096
                    || script.contains('\0')
                    || !(1..=120).contains(&timeout)
                {
                    return Err(INVALID);
                }
                Work::Script(script, timeout)
            }
            _ => return Err(INVALID),
        };
        args.end()?;
        Ok(Self { key, id, work })
    }

    fn vm(&self) -> Option<Id> {
        (!matches!(self.work, Work::Build | Work::Create(_))).then_some(self.id)
    }
}

impl VmInventory {
    fn dispatch(
        &self,
        request: CallToolRequestParams,
        cancelled: impl FnOnce() -> bool,
    ) -> Outcome<Value> {
        let mut gate = self.supervisor.lock()?;
        if gate.closed || self.closed.load(Ordering::Relaxed) || cancelled() {
            return Err(SHUTDOWN);
        }
        let name = request.name.as_ref();
        if matches!(name, "run_command" | "discover_tools") {
            return Err(HOST_EXECUTION_DISABLED);
        }
        let tool = ToolName::ALL
            .into_iter()
            .find(|tool| tool.name() == name)
            .ok_or(Fault("unknown_tool"))?;
        let mut args = Args::new(request.arguments.unwrap_or_default());
        if args.malformed() {
            return match tool.action() {
                Some(action) => self.refuse(action, None, INVALID),
                None => Err(INVALID),
            };
        }
        if let Some(action) = tool.action() {
            // The attempted VM ID is only a candidate correlation. The audit
            // transaction keeps it only when it is owned by this caller.
            let vm = args
                .0
                .get("vm_id")
                .and_then(Value::as_str)
                .and_then(|id| parse_id(id).ok());
            let request = match Mutation::parse(tool, &mut args) {
                Ok(request) => request,
                Err(reason) => return self.refuse(action, vm, reason),
            };
            return self.mutate(&mut gate, action, request);
        }
        self.read(&gate, tool, args)
    }

    /// The one bounded refusal policy for trusted mutation attempts. A
    /// refusal is filed only under its exact stored code; reasons the v6
    /// vocabulary cannot express are not misfiled as another code. When the
    /// audit write cannot commit the caller gets the headroom refusal instead.
    fn refuse(&self, action: i64, vm: Option<Id>, reason: Fault) -> Outcome<Value> {
        if let Some(code) = refusal_from_wire(reason.0) {
            self.supervisor
                .controller
                .foreground_refusal(&self.principal, action, vm, code)??;
        }
        Err(reason)
    }

    fn mutate(&self, gate: &mut Gate, action: i64, request: Mutation) -> Outcome<Value> {
        let controller = &self.supervisor.controller;
        let recipe = hex::encode(request.id);
        let digest = self
            .supervisor
            .recipe
            .filter(|(id, _)| *id == request.id)
            .map(|(_, digest)| digest);
        let subject = match request.work {
            Work::Build => Subject::Recipe(&recipe, digest.unwrap_or([0; 32])),
            Work::Create(ttl) => Subject::Template(
                request.id,
                Sha256::new()
                    .chain_update(b"arxcovo.clone.ttl.v1\0")
                    .chain_update(ttl.to_be_bytes())
                    .finalize()
                    .into(),
            ),
            _ => Subject::Vm(request.id),
        };
        match controller.foreground_replay(&self.principal, request.key, subject, action)? {
            Ok(Some(replay)) => return Ok(receipt(replay.operation.id, replay.lease_end)),
            Ok(None) => {}
            Err(reason) => return self.refuse(action, request.vm(), reason.into()),
        }
        // Scope and subject state precede native or fixture eligibility.
        let subject = match request.work {
            Work::Build if digest.is_none() => {
                return self.refuse(action, None, NOT_FOUND);
            }
            Work::Build => None,
            Work::Create(_) => Some(ForegroundSubject::Template(request.id)),
            Work::Start => Some(ForegroundSubject::Start(request.id)),
            Work::Stop => Some(ForegroundSubject::Stop(request.id)),
            Work::Script(..) => Some(ForegroundSubject::Script(request.id)),
            Work::Extend(_) => Some(ForegroundSubject::Extend(request.id)),
        };
        if let Some(subject) = subject
            && let Err(reason) = controller.foreground_precheck(&self.principal, subject)?
        {
            let reason = match reason {
                Precheck::Refused(reason) => reason.into(),
                Precheck::NotReady => NOT_READY,
            };
            return self.refuse(action, request.vm(), reason);
        }
        #[cfg(test)]
        if let Some(fixture) = &mut gate.fixture {
            return fixture.admit(self, action, request, digest);
        }
        let _ = gate;
        self.refuse(
            action,
            request.vm(),
            Fault(native_reason(self.supervisor.native_refusal)),
        )
    }

    fn read(&self, gate: &Gate, tool: ToolName, mut args: Args) -> Outcome<Value> {
        let controller = &self.supervisor.controller;
        match tool {
            ToolName::Hosts => {
                args.end()?;
                let mut host = sysinfo::System::new();
                host.refresh_cpu_all();
                host.refresh_memory();
                Ok(
                    json!({"hosts":[{"host_id":hex::encode(self.supervisor.host_id),
                    "os":std::env::consts::OS,"arch":sysinfo::System::cpu_arch(),
                    "logical_cpus":host.cpus().len(),"ram_bytes":host.total_memory(),
                    "execution_available":false,"execution_unavailable_reason":"host_execution_disabled",
                    "execution":{"available":false,"reason":"host_execution_disabled"},
                    "tool_discovery":"not_configured"}]}),
                )
            }
            ToolName::Activity => self.activity(args),
            ToolName::Catalog | ToolName::List => self.page(gate, tool, args),
            ToolName::Get => {
                let id = args.id("vm_id")?;
                args.end()?;
                let status = controller.get_status(&self.principal, id)??;
                Ok(vm_row(status, gate, true))
            }
            ToolName::Operation => {
                let id = args.id("operation_id")?;
                args.end()?;
                Ok(operation_row(
                    controller.operation_status(&self.principal, id)??,
                ))
            }
            _ => Err(INVALID),
        }
    }

    fn activity(&self, mut args: Args) -> Outcome<Value> {
        let exact: Option<i64> = args.optional("activity_id")?;
        if args.0.values().any(Value::is_null) {
            return Err(INVALID);
        }
        let query: ListActivity =
            serde_json::from_value(Value::Object(args.0)).map_err(|_| INVALID)?;
        if query.validate().is_err() || exact.is_some_and(|id| id <= 0 || query.before_id.is_some())
        {
            return Err(INVALID);
        }
        let mut rows = self.supervisor.controller.foreground_activity(
            &self.principal,
            query.before_id,
            exact,
            query.limit + 1,
        )?;
        let mut value = json!({"activities":[]});
        if rows.len() > usize::from(query.limit) {
            rows.pop();
            value["next_before_id"] = json!(rows.last().expect("positive limit").id);
        }
        value["activities"] =
            Value::Array(rows.into_iter().map(activity_row).collect::<Outcome<_>>()?);
        Ok(value)
    }

    fn page(&self, gate: &Gate, tool: ToolName, mut args: Args) -> Outcome<Value> {
        let limit: u32 = args.optional("limit")?.unwrap_or(20);
        if !(1..=20).contains(&limit) {
            return Err(INVALID);
        }
        let cursor: Option<String> = args.optional("cursor")?;
        args.end()?;
        let after = cursor
            .as_deref()
            .map(|value| self.decode_cursor(tool, value))
            .transpose()?;
        let capability = self.supervisor.capability(gate);
        let controller = &self.supervisor.controller;
        let buildable = capability["available"] == true;
        // Both pages read `limit + 1` rows from one controller snapshot; the
        // extra row only decides whether a cursor is returned.
        let mut rows: Vec<((u8, Id), Value)> = if tool == ToolName::Catalog {
            controller
                .foreground_catalog(
                    &self.principal,
                    self.supervisor.recipe.map(|(id, _)| id),
                    after.unwrap_or((0, [0; 16])),
                    limit + 1,
                )?
                .into_iter()
                .map(|entry| Ok(((entry.kind, entry.id), catalog_row(&entry, buildable)?)))
                .collect::<Outcome<_>>()?
        } else {
            controller
                .foreground_list(&self.principal, after.map(|(_, id)| id), limit + 1)??
                .into_iter()
                .map(|item| ((0, item.id), vm_row(item, gate, false)))
                .collect()
        };
        let mut value = json!({"arch":"x86_64","capability":capability});
        if rows.len() > limit as usize {
            rows.pop();
            value["next_cursor"] = json!(self.cursor(tool, rows.last().expect("positive limit").0));
        }
        value["entries"] = Value::Array(rows.into_iter().map(|(_, value)| value).collect());
        Ok(value)
    }

    fn cursor(&self, tool: ToolName, (kind, id): (u8, Id)) -> String {
        let mut bytes = [0; 51];
        bytes[0] = 1;
        bytes[1] = kind;
        bytes[2..18].copy_from_slice(&id);
        let tag = self.cursor_mac(tool, &bytes[..19]);
        bytes[19..].copy_from_slice(tag.as_ref());
        encode_cursor(bytes)
    }

    fn cursor_mac(&self, tool: ToolName, bytes: &[u8]) -> hmac::Tag {
        let mut mac = hmac::Context::with_key(&self.supervisor.cursor_key);
        mac.update(b"arxcovo.vm.cursor.v1\0");
        mac.update(tool.name().as_bytes());
        mac.update(&[0]);
        mac.update(self.principal.as_bytes());
        mac.update(&[0]);
        mac.update(bytes);
        mac.sign()
    }

    fn decode_cursor(&self, tool: ToolName, value: &str) -> Outcome<(u8, Id)> {
        use subtle::ConstantTimeEq;
        let bytes = decode_cursor(value)?;
        let valid_kind = if tool == ToolName::Catalog {
            (1..=3).contains(&bytes[1])
        } else {
            bytes[1] == 0
        };
        let expected = self.cursor_mac(tool, &bytes[..19]);
        if bytes[0] != 1
            || !valid_kind
            || bytes[18] != 0
            || !bool::from(expected.as_ref().ct_eq(&bytes[19..]))
        {
            return Err(INVALID_CURSOR);
        }
        Ok((
            bytes[1],
            bytes[2..18].try_into().expect("fixed cursor key width"),
        ))
    }
}

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_cursor(bytes: [u8; 51]) -> String {
    let mut output = String::with_capacity(68);
    for chunk in bytes.chunks_exact(3) {
        let [a, b, c] = [chunk[0], chunk[1], chunk[2]];
        for index in [
            a >> 2,
            (a << 4 | b >> 4) & 63,
            (b << 2 | c >> 6) & 63,
            c & 63,
        ] {
            output.push(char::from(BASE64URL[index as usize]));
        }
    }
    output
}

fn decode_cursor(value: &str) -> Outcome<[u8; 51]> {
    if value.len() != 68 {
        return Err(INVALID_CURSOR);
    }
    let mut output = [0; 51];
    for (packed, chunk) in output
        .chunks_exact_mut(3)
        .zip(value.as_bytes().chunks_exact(4))
    {
        let mut digits = [0u8; 4];
        for (digit, byte) in digits.iter_mut().zip(chunk) {
            *digit = BASE64URL
                .iter()
                .position(|candidate| candidate == byte)
                .ok_or(INVALID_CURSOR)? as u8;
        }
        let [a, b, c, d] = digits;
        packed.copy_from_slice(&[a << 2 | b >> 4, b << 4 | c >> 2, c << 6 | d]);
    }
    Ok(output)
}

/// Catalog kinds are the keyset's first ordering column. Optional recipe
/// correlations are omitted; a sealed or building row always has one in v6,
/// so an absent value is a store invariant failure, not a wire `null`.
fn catalog_row(entry: &crate::vm::CatalogEntry, buildable: bool) -> Outcome<Value> {
    let id = hex::encode(entry.id);
    Ok(match (entry.kind, entry.recipe.as_deref()) {
        (1, _) if buildable => json!({"recipe_id":id,"guest":"ubuntu","state":"buildable"}),
        (1, _) => {
            json!({"recipe_id":id,"guest":"ubuntu","state":"unqualified","reason":"recipe_unqualified"})
        }
        (2, Some(recipe)) => {
            json!({"template_id":id,"recipe_id":recipe,"guest":"ubuntu","state":"sealed"})
        }
        (3, Some(recipe)) => json!({"operation_id":id,"recipe_id":recipe,"state":"building"}),
        _ => return Err(STATE_UNAVAILABLE),
    })
}

fn receipt(id: Id, expires: Option<i64>) -> Value {
    let mut value = json!({"operation_id":hex::encode(id)});
    if let Some(expires) = expires {
        value["expires_at_ms"] = json!(expires);
    }
    value
}

fn vm_row(status: VmStatus, gate: &Gate, get: bool) -> Value {
    let ready = status.ready;
    #[cfg(test)]
    let ready = ready
        || (status.state == "running" && gate.fixture.as_ref().is_some_and(|f| f.ready(status.id)));
    let _ = gate;
    let mut value = json!({"vm_id":hex::encode(status.id),"state":status.state,"ready":ready});
    if let Some(template) = status.template {
        value["template_id"] = json!(hex::encode(template));
    }
    if let Some(guest) = status.guest {
        value["guest"] = json!(match guest {
            Guest::UbuntuX86_64 => "ubuntu",
            Guest::WindowsX86_64 => "windows",
        });
    }
    if let Some(expires) = status.expires_at_ms {
        value["expires_at_ms"] = json!(expires);
    }
    if let Some(expired) = status.expired_at_ms {
        value["expired_at_ms"] = json!(expired);
    }
    if get {
        value["snapshots"] = json!([]);
        if let Some(cleanup) = status.cleanup.and_then(cleanup_wire) {
            value["cleanup"] = json!(cleanup);
        }
    }
    value
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Pending => "pending",
        Status::Succeeded => "succeeded",
        Status::Failed => "failed",
        Status::Interrupted => "interrupted",
    }
}

fn operation_row(operation: OperationStatus) -> Value {
    let mut value = receipt(operation.id, operation.expires_at_ms);
    value["action"] = json!(operation.action);
    value["state"] = json!(status_label(operation.state));
    if let Some(stage) = operation.stage {
        value["stage"] = json!(stage);
    }
    if let Some(cleanup) = operation.cleanup.and_then(cleanup_wire) {
        value["cleanup"] = json!(cleanup);
    }
    if let Some(vm) = operation.vm {
        value["vm_id"] = json!(hex::encode(vm));
    }
    if let Some(recipe) = operation.recipe {
        value["recipe_id"] = json!(recipe);
    }
    if let Some(template) = operation.template {
        value["template_id"] = json!(hex::encode(template));
    }
    if let Some(result) = operation.script {
        value["script_status"] = json!(match result {
            ScriptResult::Exited(code) => {
                value["exit_code"] = json!(code);
                "exited"
            }
            ScriptResult::Unavailable => "unavailable",
            ScriptResult::TimedOut => "timed_out",
            ScriptResult::LaunchFailed => "launch_failed",
            ScriptResult::Terminated => "terminated",
        });
    } else if operation.state == Status::Interrupted {
        // Only evidence-backed reasons are projected. The v6 ledger has no
        // typed failure reason, so other failed or interrupted rows omit it.
        let reason = match (operation.cleanup, operation.action) {
            (Some("unconfirmed"), _) => Some("cleanup_unconfirmed"),
            (_, "run_script") => Some("execution_unknown"),
            _ => None,
        };
        if let Some(reason) = reason {
            value["reason"] = json!(reason);
        }
    }
    value
}

fn action_wire(code: i64) -> Option<&'static str> {
    Some(match code {
        1 => "create",
        2 => "run_script",
        3 => "start",
        4 => "stop",
        5 => "snapshot",
        6 => "restore",
        7 => "expire",
        8 => "extend",
        9 => "seal_template",
        10 => "recover",
        BUILD_ACTION => "template_build",
        _ => return None,
    })
}

fn activity_state_wire(code: i64) -> Option<&'static str> {
    Some(match code {
        0 => "pending",
        1 => "succeeded",
        2 => "failed",
        3 => "interrupted",
        4 => "refused",
        _ => return None,
    })
}

fn activity_row(row: crate::vm::ActivityReceipt) -> Outcome<Value> {
    let action = action_wire(row.action).ok_or(STATE_UNAVAILABLE)?;
    let state = activity_state_wire(row.state).ok_or(STATE_UNAVAILABLE)?;
    let mut value = json!({"activity_id":row.id,"action":action,"state":state,
        "identity_mode":"local_vm_principal","actor":"agent","started_at_ms":row.started_ms});
    if let Some(finished) = row.finished_ms {
        value["finished_at_ms"] = json!(finished);
    }
    if let Some(vm) = row.vm {
        value["vm_id"] = json!(hex::encode(vm));
    }
    if let Some(id) = row.operation {
        value["operation_id"] = json!(hex::encode(id));
    }
    if let Some(reason) = row.reason {
        value["reason"] = json!(refusal_wire(refusal_code(reason).ok_or(STATE_UNAVAILABLE)?));
    }
    if let Some(cleanup) = row.cleanup {
        value["cleanup"] = json!(proof_wire(cleanup));
    }
    Ok(value)
}

/// Coordinator steering (2026-11-23): explicit proof maps `true` to
/// `confirmed` and `false` to `cleanup_unconfirmed`; absent proof is omitted.
fn proof_wire(confirmed: bool) -> &'static str {
    if confirmed {
        "confirmed"
    } else {
        "cleanup_unconfirmed"
    }
}

/// Closed projection of a store cleanup label. `pending` is the current VM
/// teardown state, never operation proof; any other label is omitted.
fn cleanup_wire(label: &str) -> Option<&'static str> {
    match label {
        "confirmed" => Some(proof_wire(true)),
        "unconfirmed" => Some(proof_wire(false)),
        "pending" => Some("pending"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Agent, Config},
        local::{self, Endpoint, Listener},
        vm::{
            guest::GuestChannel,
            qmp::Qmp,
            reaper::{CleanupWork, fixture::FixtureCustody},
        },
    };
    use anyhow::{Result, ensure};
    use rmcp::{
        RoleClient, ServiceExt, service::RunningService, transport::async_rw::AsyncRwTransport,
    };
    use std::{
        collections::{HashMap, HashSet},
        fs,
        sync::atomic::AtomicI64,
    };
    use tokio::{
        io::{
            AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf,
            WriteHalf, split,
        },
        sync::Notify,
        task::{JoinHandle, JoinSet},
    };

    const HOST: HostObservation = HostObservation {
        owner_idle: Some(true),
        logical_cpus: Some(4),
        available_ram: Some(u64::MAX),
        capacity: Some(Capacity {
            free_bytes: u64::MAX,
            managed_bytes: 0,
        }),
    };
    const RESOURCES: Resources = Resources {
        vcpus: 1,
        guest_ram: 1024,
        overhead_ram: 1024,
    };
    const RETAINED: DiskClaim = DiskClaim {
        staging: 0,
        images: 128,
        metadata: 0,
    };
    const TRANSIENT: DiskClaim = DiskClaim {
        staging: 128,
        images: 128,
        metadata: 0,
    };
    const UTC: i64 = 1_800_000_000_000;
    const RECIPE: (Id, [u8; 32]) = ([3; 16], [4; 32]);

    /// Staged fixture files with real file identities, written through the
    /// reaper's own test custody so expiry and recovery proofs are the 4c2b
    /// reaper's, not this module's. Pathname custody never qualifies native.
    #[derive(Clone)]
    struct Artifacts(Arc<FixtureCustody>);

    impl Artifacts {
        fn write(&self, slot: ArtifactSlot) -> Result<ArtifactEvidence> {
            self.0.stage(slot, slot.role.as_bytes())
        }

        fn verify(&self, slot: ArtifactSlot, proof: ArtifactEvidence) -> Result<()> {
            let path = self.0.path(slot.role, slot.key);
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.file_type().is_file() && proof.pin.verify(fs::File::open(path)?, 128)?,
                "fixture artifact changed"
            );
            Ok(())
        }

        fn remove(&self, slot: ArtifactSlot, proof: ArtifactEvidence) -> Result<()> {
            self.verify(slot, proof)?;
            Ok(fs::remove_file(self.0.path(slot.role, slot.key))?)
        }
    }

    /// The fixture's teardown authority for the 4c2b reaper. Release is
    /// offline only when no fixture boot or worker holds the subject.
    #[derive(Clone)]
    pub(super) struct FixtureReaper {
        live: Arc<Mutex<Live>>,
        pub(super) custody: Arc<FixtureCustody>,
    }

    impl runtime::ReaperRuntime for FixtureReaper {
        fn quiesce(&self, work: &CleanupWork, _: Instant) -> runtime::Quiesced {
            let live = self.live.lock().unwrap();
            match work.kind.vm() {
                Some(vm) if live.boots.contains_key(&vm) || live.working.contains(&vm) => {
                    runtime::Quiesced::Unconfirmed
                }
                _ => runtime::Quiesced::Offline,
            }
        }

        fn graceful_stop(&self, _: &CleanupWork, _: Instant) -> runtime::StopDisposition {
            runtime::StopDisposition::ChargedQuarantine
        }

        fn force_reap(&self, _: &CleanupWork, _: Instant) -> runtime::StopDisposition {
            runtime::StopDisposition::ChargedQuarantine
        }
    }

    /// Injected reaper turns for the supervised runner.
    pub(super) struct TestTicks(tokio::sync::mpsc::UnboundedReceiver<()>);

    impl ReaperTick for TestTicks {
        async fn next_tick(&mut self) -> bool {
            self.0.recv().await.is_some()
        }
    }

    type Pair = (PairSlots, [ArtifactEvidence; 2]);

    /// A published healthy boot. Channel health (the slot exists) is distinct
    /// from its one exchange slot (idle, or held by a script worker).
    enum Slot {
        Idle(Box<Boot>),
        Exchanging,
    }

    #[derive(Default)]
    struct Live {
        templates: HashMap<Id, Pair>,
        instances: HashMap<Id, Pair>,
        boots: HashMap<Id, Slot>,
        /// VMs with a registered, not yet joined fixture worker.
        working: HashSet<Id>,
    }

    pub(super) struct Fixture {
        jobs: JoinSet<Result<()>>,
        live: Arc<Mutex<Live>>,
        artifacts: Artifacts,
        build_hold: Option<Arc<Notify>>,
        script_hold: Option<Arc<Notify>>,
        /// A cancelled build worker reports failure, to witness drain errors.
        fail_cancelled: bool,
        failed: bool,
    }

    pub(super) struct FixtureDrain {
        jobs: JoinSet<Result<()>>,
        live: Arc<Mutex<Live>>,
        failed: bool,
    }

    impl FixtureDrain {
        pub(super) async fn join(mut self) -> Result<Self> {
            while let Some(result) = self.jobs.join_next().await {
                self.failed |= !matches!(result, Ok(Ok(())));
            }
            ensure!(!self.failed, "fixture job failed");
            Ok(self)
        }

        /// Release idle boots once every worker holding a slot is joined.
        pub(super) async fn release(self) -> Result<()> {
            let boots = std::mem::take(&mut self.live.lock().unwrap().boots);
            for slot in boots.into_values() {
                if let Slot::Idle(boot) = slot {
                    (*boot).release().await?;
                }
            }
            Ok(())
        }
    }

    impl Fixture {
        fn new(root: &Path) -> Self {
            Self {
                jobs: JoinSet::new(),
                live: Arc::new(Mutex::new(Live::default())),
                artifacts: Artifacts(Arc::new(FixtureCustody::open(root).unwrap())),
                build_hold: None,
                script_hold: None,
                fail_cancelled: false,
                failed: false,
            }
        }

        pub(super) fn ready(&self, vm: Id) -> bool {
            self.live.lock().unwrap().boots.contains_key(&vm)
        }

        pub(super) fn reaper(&self) -> FixtureReaper {
            FixtureReaper {
                live: self.live.clone(),
                custody: self.artifacts.0.clone(),
            }
        }

        fn reap(&mut self) {
            while let Some(result) = self.jobs.try_join_next() {
                self.failed |= !matches!(result, Ok(Ok(())));
            }
        }

        pub(super) fn take_jobs(&mut self) -> FixtureDrain {
            FixtureDrain {
                jobs: std::mem::take(&mut self.jobs),
                live: self.live.clone(),
                failed: self.failed,
            }
        }

        pub(super) fn admit(
            &mut self,
            inventory: &VmInventory,
            action: i64,
            mutation: Mutation,
            digest: Option<[u8; 32]>,
        ) -> Outcome<Value> {
            self.reap();
            if self.failed {
                return Err(STATE_UNAVAILABLE);
            }
            if self.jobs.len() >= 8 {
                return inventory.refuse(action, mutation.vm(), Refusal::PendingFull.into());
            }
            if let Work::Script(..) = mutation.work {
                // The controller already proved a running owned generation
                // with no pending operation; this checks the live channel.
                match self.live.lock().unwrap().boots.get(&mutation.id) {
                    Some(Slot::Idle(_)) => {}
                    Some(Slot::Exchanging) => {
                        return inventory.refuse(action, mutation.vm(), Refusal::Busy.into());
                    }
                    None => return inventory.refuse(action, mutation.vm(), NOT_READY),
                }
            }
            let controller = inventory.supervisor.controller.clone();
            let cancel = inventory.supervisor.cancel.clone();
            let principal = &inventory.principal;
            let Mutation { key, id, work } = mutation;
            let live = self.live.clone();
            let artifacts = self.artifacts.clone();
            let mut registry = self.live.lock().unwrap();
            let (operation, vm) = match work {
                Work::Build => {
                    let recipe = hex::encode(id);
                    let spec = BuildRequest {
                        principal,
                        key,
                        recipe: &recipe,
                        digest: digest.ok_or(NOT_FOUND)?,
                        retained: RETAINED,
                        transient: TRANSIENT,
                    };
                    let (slots, worker, reference) = match controller.admit_build(
                        spec,
                        HOST,
                        RESOURCES,
                        |template, slots| {
                            let (saved, [disk, vars]) = *registry.templates.get(&template)?;
                            if saved != slots {
                                return None;
                            }
                            artifacts.verify(slots.disk, disk).ok()?;
                            artifacts.verify(slots.vars, vars).ok()?;
                            Some([disk, vars])
                        },
                    )?? {
                        BuildAdmission::Fresh(slots, worker, reference) => {
                            (*slots, worker, reference)
                        }
                        BuildAdmission::Existing(reference) => {
                            return Ok(receipt(reference.id, None));
                        }
                    };
                    let principal = principal.clone();
                    let digest = spec.digest;
                    let hold = self.build_hold.take();
                    let fail_cancelled = self.fail_cancelled;
                    self.jobs.spawn(async move {
                        if !released(hold, &cancel).await {
                            controller.fail_build(&worker, slots, true)?;
                            ensure!(!fail_cancelled, "fixture worker failed");
                            return Ok(());
                        }
                        let spec = BuildRequest {
                            principal: &principal,
                            key,
                            recipe: &recipe,
                            digest,
                            retained: RETAINED,
                            transient: TRANSIENT,
                        };
                        let [disk, vars, seed, overlay, verify_vars] = [
                            slots.pair.disk,
                            slots.pair.vars,
                            slots.seed,
                            slots.verification.disk,
                            slots.verification.vars,
                        ];
                        let mut proof = Vec::new();
                        for slot in [disk, vars, seed, overlay, verify_vars] {
                            let file = artifacts.write(slot)?;
                            controller.build_evidence(
                                &worker,
                                slots.id,
                                &[(slot, None), (slot, Some(file))],
                            )?;
                            proof.push(file);
                        }
                        for (slot, file) in
                            [seed, overlay, verify_vars].into_iter().zip(&proof[2..])
                        {
                            artifacts.remove(slot, *file)?;
                        }
                        let pair = [proof[0], proof[1]];
                        let mut registry = live.lock().unwrap();
                        ensure!(
                            controller.finish_build(&worker, slots, spec, pair)?
                                == Status::Succeeded,
                            "fixture build interrupted"
                        );
                        registry.templates.insert(slots.id, (slots.pair, pair));
                        Ok(())
                    });
                    return Ok(receipt(reference.id, None));
                }
                Work::Create(ttl) => {
                    // The controller precheck proved the sealed template.
                    let (golden, proof) = registry
                        .templates
                        .get(&id)
                        .copied()
                        .ok_or(STATE_UNAVAILABLE)?;
                    artifacts.verify(golden.disk, proof[0])?;
                    artifacts.verify(golden.vars, proof[1])?;
                    let request = Request {
                        principal,
                        request_id: key,
                        vm: [0; 16],
                        disk: RETAINED,
                        operation: Operation::Clone {
                            template: id,
                            lease_seconds: Some(ttl),
                        },
                    };
                    let (vm, slots, worker, reference) = match controller
                        .admit_linked_clone(request, HOST, RESOURCES, golden, proof)??
                    {
                        CloneAdmission::Fresh(vm, slots, worker, reference) => {
                            (vm, slots, worker, reference)
                        }
                        CloneAdmission::Existing(reference) => {
                            return Ok(receipt(reference.id, None));
                        }
                    };
                    self.jobs.spawn(async move {
                        let pair = [artifacts.write(slots.disk)?, artifacts.write(slots.vars)?];
                        let mut registry = live.lock().unwrap();
                        ensure!(
                            controller.finish_linked_clone(&worker, slots, Some(pair))?
                                == Status::Succeeded,
                            "fixture clone interrupted"
                        );
                        registry.instances.insert(vm, (slots, pair));
                        Ok(())
                    });
                    return Ok(receipt(reference.id, None));
                }
                Work::Extend(ttl) => {
                    let end = controller.extend_lease(principal, id, key, ttl)??;
                    let replay = controller
                        .foreground_replay(principal, key, Subject::Vm(id), Action::Extend as i64)??
                        .ok_or(STATE_UNAVAILABLE)?;
                    return Ok(receipt(replay.operation.id, Some(end)));
                }
                Work::Start => (Operation::Start, id),
                Work::Stop => (Operation::Stop, id),
                Work::Script(ref script, seconds) => (
                    Operation::Script {
                        script,
                        timeout_seconds: u64::from(seconds),
                    },
                    id,
                ),
            };
            let request = Request {
                principal,
                request_id: key,
                vm,
                operation,
                disk: DiskClaim::default(),
            };
            let mut worker = match controller.admit(request, HOST, RESOURCES)? {
                Admission::Admitted(worker) => worker,
                Admission::Existing(prior) => {
                    return Ok(receipt(
                        controller.foreground_operation_ref(principal, prior.activity_id)?,
                        None,
                    ));
                }
                Admission::Refused(reason) => return Err(reason.into()),
            };
            let activity = worker.activity_id();
            // Registered synchronously with the committed admission; the
            // guard leaves the reaper's offline proof closed until joined.
            registry.working.insert(vm);
            let working = Working(live.clone(), vm);
            match work {
                Work::Start => {
                    let controller = controller.clone();
                    self.jobs.spawn(async move {
                        let _working = working;
                        let boot = Boot::start(&cancel).await;
                        match boot {
                            Ok(boot) => {
                                // Publish ready only if this worker's settlement
                                // won; a worker that lost to expiry or shutdown
                                // is reaped without a ready boot.
                                let settled = {
                                    let mut registry = live.lock().unwrap();
                                    let status =
                                        controller.finish(&worker, Completion::Succeeded)?;
                                    if status == Status::Succeeded {
                                        registry.boots.insert(vm, Slot::Idle(Box::new(boot)));
                                        None
                                    } else {
                                        Some(boot)
                                    }
                                };
                                if let Some(boot) = settled {
                                    boot.release().await?;
                                }
                            }
                            Err(_) => {
                                controller.finish(&worker, Completion::CleanupUnconfirmed)?;
                            }
                        }
                        Ok(())
                    });
                }
                Work::Stop => {
                    let boot = match registry.boots.remove(&vm) {
                        Some(Slot::Idle(boot)) => Some(*boot),
                        Some(Slot::Exchanging) | None => None,
                    };
                    let controller = controller.clone();
                    self.jobs.spawn(async move {
                        let _working = working;
                        let complete = match boot {
                            Some(boot) => boot.stop().await.is_ok(),
                            None => false,
                        };
                        controller.finish(
                            &worker,
                            if complete {
                                Completion::Succeeded
                            } else {
                                Completion::CleanupUnconfirmed
                            },
                        )?;
                        Ok(())
                    });
                }
                Work::Script(script, seconds) => {
                    let Some(Slot::Idle(boot)) = registry.boots.insert(vm, Slot::Exchanging) else {
                        unreachable!("idle channel checked under the fixture gate");
                    };
                    let mut boot = *boot;
                    let hold = self.script_hold.take();
                    worker.deadline += Duration::from_secs(6);
                    let controller = controller.clone();
                    self.jobs.spawn(async move {
                        let _working = working;
                        let result = if released(hold, &cancel).await {
                            boot.channel.run_script(&cancel, script, seconds).await.ok()
                        } else {
                            None
                        };
                        let result = result.and_then(|(status, code)| guest_result(status, code));
                        // A trustworthy result that won settlement returns the
                        // exchange slot; anything else retires the boot.
                        let retired = match result {
                            Some(result) => {
                                let mut registry = live.lock().unwrap();
                                let status = controller.finish_script(&worker, result)?;
                                if matches!(status, Status::Succeeded | Status::Failed) {
                                    registry.boots.insert(vm, Slot::Idle(Box::new(boot)));
                                    None
                                } else {
                                    registry.boots.remove(&vm);
                                    Some(boot)
                                }
                            }
                            None => {
                                live.lock().unwrap().boots.remove(&vm);
                                controller.finish(&worker, Completion::CleanupUnconfirmed)?;
                                Some(boot)
                            }
                        };
                        if let Some(boot) = retired {
                            boot.release().await?;
                        }
                        Ok(())
                    });
                }
                _ => unreachable!("handled before VM admission"),
            }
            Ok(receipt(
                controller.foreground_operation_ref(principal, activity)?,
                None,
            ))
        }
    }

    /// Marks a VM as held by a fixture worker until the worker finishes.
    struct Working(Arc<Mutex<Live>>, Id);

    impl Drop for Working {
        fn drop(&mut self) {
            if let Ok(mut live) = self.0.lock() {
                live.working.remove(&self.1);
            }
        }
    }

    async fn released(hold: Option<Arc<Notify>>, cancel: &Shutdown) -> bool {
        if let Some(hold) = hold {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return false,
                _ = hold.notified() => {}
            }
        }
        !cancel.is_cancelled()
    }

    fn guest_result(status: arx_guest::ScriptStatus, code: Option<u32>) -> Option<ScriptResult> {
        use arx_guest::ScriptStatus::*;
        Some(match status {
            Exited => ScriptResult::Exited(code?),
            Unavailable => ScriptResult::Unavailable,
            TimedOut => ScriptResult::TimedOut,
            LaunchFailed => ScriptResult::LaunchFailed,
            Terminated => ScriptResult::Terminated,
        })
    }

    type Monitor = Qmp<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>;
    struct Boot {
        channel: GuestChannel<DuplexStream>,
        qmp: Monitor,
        peer: JoinHandle<Result<()>>,
        guest: JoinHandle<Result<()>>,
    }

    impl Boot {
        async fn start(cancel: &Shutdown) -> Result<Self> {
            let (host, mut peer) = tokio::io::duplex(4096);
            let (serial, guest) = tokio::io::duplex(8192);
            let peer = tokio::spawn(async move {
                let [major, minor, micro] = crate::vm::qemu_args::QEMU_VERSION
                    .split('.')
                    .map(|part| part.parse::<u32>().unwrap())
                    .collect::<Vec<_>>()[..]
                else {
                    unreachable!("pinned QEMU version is major.minor.micro");
                };
                let greeting = json!({"QMP":{"version":{"qemu":{"major":major,"minor":minor,"micro":micro},"package":""},"capabilities":[]}});
                peer.write_all(format!("{greeting}\n").as_bytes()).await?;
                let mut peer = BufReader::new(peer);
                for (id, command, result) in [
                    (1, "qmp_capabilities", json!({})),
                    (
                        2,
                        "query-status",
                        json!({"status":"prelaunch","running":false}),
                    ),
                    (3, "cont", json!({})),
                    (4, "system_powerdown", json!({})),
                ] {
                    let mut line = String::new();
                    if peer.read_line(&mut line).await? == 0 {
                        return Ok(());
                    }
                    ensure!(
                        serde_json::from_str::<Value>(&line)? == json!({"execute":command,"id":id}),
                        "unexpected fixture QMP command"
                    );
                    send(&mut peer, json!({"return":result,"id":id})).await?;
                    if id == 3 {
                        send(&mut peer, json!({"event":"VSERPORT_CHANGE","data":{"id":crate::vm::qemu_args::GUEST_PORT_ID,"open":true}})).await?;
                    }
                }
                send(
                    &mut peer,
                    json!({"event":"SHUTDOWN","data":{"guest":true,"reason":"guest-shutdown"}}),
                )
                .await
            });
            let guest = tokio::spawn(guest_peer(guest));
            let (read, write) = split(host);
            let started = async {
                let mut qmp = Qmp::handshake(read, write, cancel).await?;
                qmp.release_startup_hold(cancel, || true).await?;
                let channel = GuestChannel::ready(serial, &mut qmp, cancel).await?;
                Ok::<_, crate::vm::qmp::ChannelError>((qmp, channel))
            }
            .await;
            match started {
                Ok((qmp, channel)) => Ok(Self {
                    channel,
                    qmp,
                    peer,
                    guest,
                }),
                Err(_) => {
                    peer.await??;
                    guest.await??;
                    anyhow::bail!("fixture boot unconfirmed")
                }
            }
        }

        async fn release(self) -> Result<()> {
            drop(self.qmp);
            drop(self.channel);
            self.peer.await??;
            self.guest.await??;
            Ok(())
        }

        async fn stop(self) -> Result<()> {
            let stopped = self.qmp.powerdown(&Shutdown::default()).await;
            drop(self.channel);
            self.peer.await??;
            self.guest.await??;
            ensure!(stopped.is_ok(), "fixture stop unconfirmed");
            Ok(())
        }
    }

    async fn send(peer: &mut BufReader<DuplexStream>, value: Value) -> Result<()> {
        Ok(peer
            .get_mut()
            .write_all(format!("{value}\n").as_bytes())
            .await?)
    }

    async fn guest_peer(mut stream: DuplexStream) -> Result<()> {
        use arx_guest::{Request, Response, ScriptExecution, ScriptStatus};
        while let Ok(length) = stream.read_u32().await {
            ensure!(
                length as usize <= arx_guest::MAX_FRAME,
                "fixture frame length"
            );
            let mut payload = vec![0; length as usize + 4];
            payload[..4].copy_from_slice(&length.to_be_bytes());
            stream.read_exact(&mut payload[4..]).await?;
            let request = arx_guest::read_frame::<Request>(&mut payload.as_slice())?
                .expect("fixture request");
            let response = match request {
                Request::Hello { version, challenge } => Response::HelloAck {
                    version,
                    challenge,
                    script_execution: ScriptExecution::Available,
                },
                Request::RunScript { id, script, .. }
                    if script == "exit 7" || script == "exit 0" =>
                {
                    Response::ScriptResult {
                        id,
                        status: ScriptStatus::Exited,
                        exit_code: Some(if script == "exit 7" { 7 } else { 0 }),
                    }
                }
                _ => anyhow::bail!("unexpected fixture guest request"),
            };
            let mut frame = Vec::new();
            arx_guest::write_frame(&mut frame, &response)?;
            stream.write_all(&frame).await?;
        }
        Ok(())
    }

    fn key(number: u64) -> String {
        format!("{UTC:016x}{number:016x}")
    }

    fn request(name: &str, arguments: Value) -> CallToolRequestParams {
        CallToolRequestParams::new(name.to_owned())
            .with_arguments(arguments.as_object().unwrap().clone())
    }

    type Client = RunningService<RoleClient, ()>;

    async fn client(endpoint: &Endpoint, token: &str) -> Client {
        let stream = local::connect_vm_fixture(&endpoint.address, &endpoint.client_identity, token)
            .await
            .unwrap();
        let (read, write) = split(stream);
        ().serve(AsyncRwTransport::new_client(read, write))
            .await
            .unwrap()
    }

    async fn call(client: &Client, name: &str, args: Value) -> Value {
        let result = client.call_tool(request(name, args)).await.unwrap();
        let row = result.structured_content.expect("one structured result");
        assert_eq!(result.is_error, Some(row["ok"] == false));
        row
    }

    async fn terminal(client: &Client, reference: &Value) -> Value {
        assert_eq!(reference["ok"], true, "{reference}");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let result = call(
                    client,
                    "vm_operation",
                    json!({"operation_id":reference["operation_id"]}),
                )
                .await;
                assert_eq!(result["ok"], true, "{result}");
                if result["state"] != "pending" {
                    return result;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("operation did not settle")
    }

    async fn raw_initialize<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
        stream: T,
    ) -> BufReader<T> {
        let mut stream = BufReader::new(stream);
        stream.get_mut().write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"initialize\",\"id\":0,\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n").await.unwrap();
        let mut reply = String::new();
        stream.read_line(&mut reply).await.unwrap();
        assert!(
            serde_json::from_str::<Value>(&reply).unwrap()["result"]["capabilities"]["tools"]
                .is_object()
        );
        stream
            .get_mut()
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        stream
    }

    #[tokio::test]
    async fn real_local_sdk_sqlite_journey() {
        tokio::time::timeout(Duration::from_secs(8), journey())
            .await
            .unwrap();
    }

    async fn journey() {
        let temp = crate::service::application::tests::private_temp();
        let media = temp.path().join("fixture-media");
        fs::create_dir(&media).unwrap();
        let now = Arc::new(AtomicI64::new(UTC));
        let supervisor = Supervisor::new(
            temp.path(),
            VmPolicy::default(),
            ForegroundClock {
                utc: Some(now.clone()),
            },
            Some(RECIPE),
            NativeRefusal::QualificationMissing,
            None,
        )
        .unwrap();
        let hold_build = Arc::new(Notify::new());
        let hold_script = Arc::new(Notify::new());
        let mut fixture = Fixture::new(&media);
        fixture.build_hold = Some(hold_build.clone());
        fixture.script_hold = Some(hold_script.clone());
        supervisor.gate.lock().unwrap().fixture = Some(fixture);
        let endpoint = Endpoint {
            #[cfg(unix)]
            address: temp.path().join("vm.sock").to_str().unwrap().into(),
            #[cfg(windows)]
            address: format!(
                r"\\.\pipe\arxcovo-mcp-{}",
                hex::encode(crate::store::vm::new_id().unwrap())
            ),
            client_identity: local::vm_supervisor_identity().unwrap(),
        };
        let token_a = "a".repeat(64);
        let token_b = "b".repeat(64);
        let config = Arc::new(Config {
            agents: [("alpha", &token_a), ("beta", &token_b)]
                .into_iter()
                .map(|(principal, token)| Agent {
                    principal: principal.into(),
                    token_sha256: hex::encode(Sha256::digest(token.as_bytes())),
                })
                .collect(),
            ..Config::default()
        });
        let listener = Listener::bind(&endpoint).unwrap();
        let stop = Shutdown::default();
        let server = tokio::spawn(local::run_vm_listener(
            listener,
            config,
            supervisor.clone(),
            stop.clone(),
        ));
        let a = client(&endpoint, &token_a).await;
        let b = client(&endpoint, &token_b).await;
        let tools = a.list_tools(None).await.unwrap();
        let names = tools
            .tools
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(names, ToolName::ALL.map(ToolName::name));
        for name in ["run_command", "discover_tools"] {
            assert_eq!(
                call(&a, name, json!({})).await,
                json!({"ok":false,"reason":"host_execution_disabled"})
            );
        }
        assert_eq!(
            call(&a, "list_hosts", json!({})).await["hosts"][0]["execution"]["available"],
            false
        );
        let rejected = local::connect_vm_fixture(
            &endpoint.address,
            &endpoint.client_identity,
            &"c".repeat(64),
        )
        .await;
        assert_eq!(rejected.err().unwrap().to_string(), "peer-auth-rejected");
        #[cfg(windows)]
        let wrong = String::from("S-1-5-18");
        #[cfg(unix)]
        let wrong = (unsafe { libc::geteuid() } ^ 1).to_string();
        let rejected = local::connect_vm_fixture(&endpoint.address, &wrong, &token_a).await;
        assert_eq!(
            rejected.err().unwrap().to_string(),
            "peer-connect-or-identity"
        );

        let recipe = hex::encode(RECIPE.0);
        let first = call(&a, "vm_catalog", json!({})).await;
        assert_eq!(
            first["entries"],
            json!([{"recipe_id":recipe,"guest":"ubuntu","state":"buildable"}])
        );
        let build_args = json!({"recipe_id":recipe,"request_id":key(1)});
        let closed = supervisor.connection("alpha".into());
        let detached = closed.inventory();
        drop(closed);
        assert_eq!(
            detached
                .dispatch(request("vm_template_build", build_args.clone()), || false)
                .unwrap_err()
                .0,
            "shutdown"
        );
        let active = supervisor.connection("alpha".into());
        assert_eq!(
            active
                .inventory()
                .dispatch(request("vm_template_build", build_args.clone()), || true)
                .unwrap_err()
                .0,
            "shutdown"
        );
        assert_eq!(
            call(&a, "list_activity", json!({})).await["activities"],
            json!([])
        );

        let build = call(&a, "vm_template_build", build_args.clone()).await;
        let pending = call(&a, "vm_catalog", json!({"limit":1})).await;
        let cursor = pending["next_cursor"].as_str().unwrap();
        let second = call(&a, "vm_catalog", json!({"cursor":cursor})).await;
        assert_eq!(
            second["entries"],
            json!([{"operation_id":build["operation_id"],"recipe_id":recipe,"state":"building"}])
        );
        assert_eq!(
            call(&b, "vm_catalog", json!({})).await["entries"],
            first["entries"]
        );
        let mut altered = cursor.to_string();
        altered.replace_range(0..1, "Z");
        for (peer, tool, item) in [
            (&b, "vm_catalog", cursor),
            (&a, "vm_list", cursor),
            (&a, "vm_catalog", altered.as_str()),
        ] {
            assert_eq!(
                call(peer, tool, json!({"cursor":item})).await["reason"],
                "invalid_cursor"
            );
        }
        let operation = json!({"operation_id":build["operation_id"]});
        assert_eq!(
            call(&b, "vm_operation", operation.clone()).await["reason"],
            "not_found"
        );
        assert_eq!(
            call(&a, "vm_operation", operation).await["stage"],
            "staging"
        );
        hold_build.notify_one();
        let template = terminal(&a, &build).await["template_id"].clone();
        assert!(template.is_string());
        let reused = call(&b, "vm_template_build", build_args).await;
        assert_ne!(reused["operation_id"], build["operation_id"]);
        assert_eq!(terminal(&b, &reused).await["template_id"], template);

        let create = json!({"template_id":template,"request_id":key(2),"ttl_s":60});
        let clone = call(&a, "vm_create", create.clone()).await;
        let clone_result = terminal(&a, &clone).await;
        assert_eq!(clone_result["state"], "succeeded", "{clone_result}");
        let vm = clone_result["vm_id"].clone();
        let clone_again = call(&a, "vm_create", create).await;
        assert_eq!(clone, clone_again);
        assert_eq!(
            call(
                &a,
                "vm_create",
                json!({"template_id":template,"request_id":key(2),"ttl_s":61})
            )
            .await["reason"],
            "key_collision"
        );
        let get = json!({"vm_id":vm});
        assert_eq!(call(&a, "vm_get", get.clone()).await["state"], "stopped");
        assert_eq!(
            call(&a, "vm_list", json!({})).await["entries"][0]["vm_id"],
            vm
        );
        assert_eq!(call(&b, "vm_get", get.clone()).await["reason"], "not_found");
        assert_eq!(call(&b, "vm_list", json!({})).await["entries"], json!([]));
        assert_eq!(
            call(&b, "vm_stop", json!({"vm_id":vm,"request_id":key(3)})).await["reason"],
            "not_found"
        );
        assert_eq!(
            call(
                &a,
                "vm_run_script",
                json!({"vm_id":vm,"request_id":key(3),"script":"exit 0"})
            )
            .await["reason"],
            "not_ready"
        );
        let start = call(&a, "vm_start", json!({"vm_id":vm,"request_id":key(4)})).await;
        assert_eq!(terminal(&a, &start).await["state"], "succeeded");
        assert_eq!(call(&a, "vm_get", get.clone()).await["ready"], true);

        let raw = local::connect_vm_fixture(&endpoint.address, &endpoint.client_identity, &token_a)
            .await
            .unwrap();
        let mut raw = raw_initialize(raw).await;
        let script_args = json!({"vm_id":vm,"request_id":key(5),"script":"exit 7"});
        let wire = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"vm_run_script","arguments":script_args}});
        raw.get_mut()
            .write_all(format!("{wire}\n").as_bytes())
            .await
            .unwrap();
        // Await durable admission through a separate session. Discard the raw
        // caller's reply and disconnect while the admitted worker is held.
        let pending = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let row =
                    call(&a, "list_activity", json!({"limit":1})).await["activities"][0].clone();
                if row["action"] == "run_script" && row["state"] == "pending" {
                    break row;
                }
            }
        })
        .await
        .unwrap();
        drop(raw);
        let script = call(
            &a,
            "vm_run_script",
            json!({"vm_id":vm,"request_id":key(5),"script":"exit 0","timeout_s":1}),
        )
        .await;
        assert_eq!(script["operation_id"], pending["operation_id"]);
        for args in [
            json!({"vm_id":vm,"request_id":key(5),"script":null}),
            json!({"vm_id":vm,"request_id":key(5),"script":"\0"}),
            json!({"vm_id":vm,"request_id":key(5),"script":"é".repeat(2049)}),
            json!({"vm_id":vm,"request_id":key(5),"script":"exit 0","timeout_s":1.5}),
            json!({"vm_id":vm,"request_id":key(5),"script":"exit 0","timeout_s":"30"}),
            json!({"vm_id":"F".repeat(32),"request_id":key(5),"script":"exit 0"}),
            json!({"vm_id":vm,"request_id":key(5),"script":"exit 0","interpreter":"sh"}),
        ] {
            assert_eq!(call(&a, "vm_run_script", args).await["reason"], "invalid");
        }
        hold_script.notify_one();
        let done = terminal(&a, &script).await;
        assert_eq!(
            (
                done["state"].as_str(),
                done["script_status"].as_str(),
                done["exit_code"].as_u64()
            ),
            (Some("failed"), Some("exited"), Some(7))
        );
        let stop_vm = call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(6)})).await;
        assert_eq!(terminal(&a, &stop_vm).await["state"], "succeeded");
        assert_eq!(
            call(&a, "vm_stop", json!({"vm_id":vm,"request_id":key(7)})).await["reason"],
            "busy"
        );
        let extend = call(
            &a,
            "vm_extend",
            json!({"vm_id":vm,"request_id":key(8),"ttl_s":120}),
        )
        .await;
        assert_eq!(extend["expires_at_ms"], UTC + 120_000);
        assert_eq!(
            call(
                &a,
                "vm_extend",
                json!({"vm_id":vm,"request_id":key(8),"ttl_s":180})
            )
            .await,
            extend
        );
        now.store(UTC + 120_000, Ordering::Relaxed);
        assert_eq!(call(&a, "vm_get", get.clone()).await["state"], "teardown");
        supervisor.sweep().unwrap();
        assert_eq!(
            call(&a, "vm_get", get.clone()).await,
            json!({
                "ok":true,"vm_id":vm,"state":"expired","ready":false,"expired_at_ms":UTC+120_000,"cleanup":"confirmed","snapshots":[]
            })
        );
        assert_eq!(call(&b, "vm_get", get).await["reason"], "not_found");
        assert_eq!(call(&a, "vm_list", json!({})).await["entries"], json!([]));
        assert_eq!(fs::read_dir(&media).unwrap().count(), 2);
        let activity = call(
            &a,
            "list_activity",
            json!({"limit":50,"include_reads":true}),
        )
        .await;
        assert!(
            activity["activities"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["identity_mode"] == "local_vm_principal")
        );
        let own = activity["activities"][0]["activity_id"].clone();
        assert_eq!(
            call(&b, "list_activity", json!({"activity_id":own})).await["activities"],
            json!([])
        );
        for sentinel in [
            "exit 7",
            "exit 0",
            &token_a,
            &token_b,
            media.to_str().unwrap(),
        ] {
            assert!(!activity.to_string().contains(sentinel));
            let db = fs::read(temp.path().join("activity.sqlite3")).unwrap();
            assert!(
                !db.windows(sentinel.len())
                    .any(|bytes| bytes == sentinel.as_bytes())
            );
        }

        frame_cases(&endpoint, &token_a).await;
        // A detached SDK handler can still hold the inventory after the
        // listener leaves, but neither admission nor a refusal write survives.
        supervisor.close();
        stop.cancel();
        server.await.unwrap().unwrap();
        supervisor
            .drain_workers(Duration::from_secs(5))
            .await
            .unwrap();
        supervisor.recover(Duration::from_secs(5)).await.unwrap();
        assert_eq!(
            active
                .inventory()
                .dispatch(request("vm_template_build", json!({})), || false)
                .unwrap_err()
                .0,
            "shutdown"
        );
    }

    async fn frame_cases(endpoint: &Endpoint, token: &str) {
        const PREFIX: &[u8] = b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":1,\"params\":{\"name\":\"vm_list\",\"arguments\":{}}}";
        for (frame, valid) in [
            ([PREFIX, &vec![b' '; local::FRAME_LIMIT - PREFIX.len() - 1], b"\n"].concat(), true),
            ([PREFIX, &vec![b' '; local::FRAME_LIMIT - PREFIX.len()], b"\n"].concat(), false),
            (b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"id\":1,\"params\":{\"name\":\"vm_list\",\"arguments\":{\"limit\":1,\"l\\u0069mit\":2}}}\n".to_vec(), false),
            ([b"{\"text\":\"".as_slice(), &[255], b"\"}\n"].concat(), false),
            ([vec![b'['; 17], vec![b']'; 17], vec![b'\n']].concat(), false),
            ([PREFIX, b" {}\n"].concat(), false),
        ] {
            let raw = local::connect_vm_fixture(&endpoint.address, &endpoint.client_identity, token).await.unwrap();
            let mut raw = raw_initialize(raw).await;
            let _ = raw.get_mut().write_all(&frame).await;
            let mut reply = String::new();
            let read = raw.read_line(&mut reply).await;
            if valid {
                assert!(read.unwrap() > 0);
                assert_eq!(serde_json::from_str::<Value>(&reply).unwrap()["result"]["structuredContent"]["ok"], true);
            } else {
                assert!(matches!(read, Ok(0) | Err(_)), "rejected frame had a reply");
            }
        }
    }
}

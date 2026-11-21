use crate::store::{Activity, ListActivity};
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde_json::json;
use std::sync::Arc;
#[cfg(test)]
mod activity_tests;
#[cfg(test)]
mod discovery_tests;
#[derive(Clone)]
pub struct Inventory {
    pub execution: Option<Arc<crate::development::Execution>>,
    pub activity: Arc<Activity>,
    pub principal: Option<Principal>,
}
#[derive(Clone)]
pub struct Principal(pub String);
#[cfg(target_os = "linux")]
pub(crate) struct DelegatedInventory {
    pub(crate) local: Inventory,
    pub(crate) build: Arc<crate::service::cluster_build::BuildService>,
    pub(crate) closed: crate::service::Shutdown,
}

#[cfg(target_os = "linux")]
impl ServerHandler for DelegatedInventory {
    fn get_info(&self) -> ServerConfig {
        self.local.get_info()
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut result = self.local.list_tools(request, context).await?;
        if let Some(run) = result.tools.iter_mut().find(|t| t.name == "run_command") {
            let schema = Arc::make_mut(&mut run.input_schema);
            schema.get_mut("properties").unwrap()["target"] = json!({"type":"integer","minimum":1});
            schema.get_mut("properties").unwrap()["workspace"] =
                json!({"type":"string","maxLength":48});
        }
        for (name, description, schema) in [
            (
                "discover_build_targets",
                "Show enrolled delegated Standard build targets",
                json!({"type":"object","additionalProperties":false}),
            ),
            (
                "delegated_build_status",
                "Find your forwarded operations or resolve one by ID",
                json!({"type":"object","required":["target"],"additionalProperties":false,
                    "properties":{"target":{"type":"integer","minimum":1},
                                  "operation_id":{"type":"string","pattern":"^[a-f0-9]{64}$"}}}),
            ),
        ] {
            result.tools.push(Tool::new(
                name,
                description,
                Arc::new(schema.as_object().unwrap().clone()),
            ));
        }
        Ok(result)
    }

    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        use crate::cluster::{delegated_build::wire::Descriptor, enrollment::NodeId};
        let invalid = || ErrorData::invalid_params("invalid delegated build arguments", None);
        let principal = &self.local.principal.as_ref().ok_or_else(invalid)?.0;
        let args = request.arguments.get_or_insert_with(Default::default);
        let value = match request.name.as_ref() {
            "run_command" if args.contains_key("target") => {
                let target = args
                    .remove("target")
                    .and_then(|t| NodeId::new(t.as_u64()?))
                    .ok_or_else(invalid)?;
                let workspace = args
                    .remove("workspace")
                    .and_then(|w| w.as_str().map(str::to_owned))
                    .ok_or_else(invalid)?;
                let command: crate::service::executor::Request =
                    serde_json::from_value(serde_json::Value::Object(std::mem::take(args)))
                        .map_err(|_| invalid())?;
                let tool = command
                    .tool
                    .filter(|_| command.executable.is_none() && command.cwd.is_empty())
                    .ok_or_else(invalid)?;
                self.build
                    .forward(
                        principal,
                        target,
                        Descriptor {
                            workspace,
                            tool,
                            args: command.args,
                            timeout_s: command.timeout_s,
                            output_bytes: command.output_bytes,
                        },
                        &self.closed,
                    )
                    .await
            }
            "discover_build_targets" if args.is_empty() => {
                json!({"targets": self.build.targets(principal)})
            }
            "delegated_build_status" => {
                let target = args
                    .remove("target")
                    .and_then(|t| NodeId::new(t.as_u64()?))
                    .ok_or_else(invalid)?;
                let operation = args
                    .remove("operation_id")
                    .map(|id| {
                        let mut bytes = [0; 32];
                        let id = id.as_str().ok_or_else(invalid)?;
                        hex::decode_to_slice(id, &mut bytes).map_err(|_| invalid())?;
                        Ok::<_, ErrorData>(bytes)
                    })
                    .transpose()?;
                if !args.is_empty() {
                    return Err(invalid());
                }
                self.build
                    .forwarded_status(principal, target, operation)
                    .await
            }
            _ => return self.local.call_tool(request, context).await,
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(value.to_string())]).into())
    }
}

fn unavailable() -> ErrorData {
    ErrorData::internal_error("activity unavailable", None)
}
/// Records that this handler's terminal activity row committed.
fn completed(admission: &mut Option<crate::development::Admission>) {
    if let Some(admission) = admission {
        admission.completed();
    }
}
impl ServerHandler for Inventory {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: vec![
                Tool::new(
                    "list_hosts",
                    "Observed local facts, current execution capability and prior released tool inventory; cached discovery does not imply execution availability",
                    Arc::new(
                        json!({"type":"object","properties":{},"additionalProperties":false})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                ),
                Tool::new(
                    "list_activity",
                    "Durable activity visible to the authenticated agent; activity_id selects zero or one row and is mutually exclusive with before_id. Read rows require include_reads",
                    Arc::new(
                        json!({
                            "type":"object",
                            "properties":{
                                "limit":{"type":"integer","minimum":1,"maximum":50,"default":20},
                                "before_id":{"type":"integer","minimum":1},
                                "activity_id":{"type":"integer","minimum":1,"maximum":9223372036854775807_i64},
                                "include_reads":{"type":"boolean","default":false}
                            },
                            "additionalProperties":false,
                            "not":{"required":["activity_id","before_id"]}
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                ),
                Tool::new("run_command", if self.activity.dedicated.is_some() { "Run a bounded command as the configured dedicated account" } else { "Run a bounded tool using the configured Windows execution account" }, Arc::new(if self.activity.dedicated.is_some() { crate::service::executor::schema() } else { json!({"type":"object","required":["tool"],"properties":{"tool":{"type":"string","enum":crate::development::TOOLS},"args":{"type":"array","maxItems":128,"items":{"type":"string","maxLength":8192}},"cwd":{"type":"string","maxLength":4096},"timeout_s":{"type":"integer","minimum":1,"maximum":3600},"output_bytes":{"type":"integer","minimum":2,"maximum":8192}},"additionalProperties":false}) }.as_object().unwrap().clone())),
                Tool::new("discover_tools", "Probe fixed tool names using the configured executor", Arc::new(json!({"type":"object","properties":{},"additionalProperties":false}).as_object().unwrap().clone())),
            ],
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        #[cfg(test)]
        let _entry = match &self.execution {
            Some(e) => e.checkpoint(crate::development::Checkpoint::Entry).await,
            None => None,
        };
        let principal = self
            .principal
            .as_ref()
            .or_else(|| {
                context
                    .extensions
                    .get::<axum::http::request::Parts>()
                    .and_then(|p| p.extensions.get::<Principal>())
            })
            .ok_or_else(|| ErrorData::internal_error("authentication context unavailable", None))?;
        // Never persist attacker-provided tool names or arguments.
        let label = match request.name.as_ref() {
            "run_command" => "run_command",
            "discover_tools" => "discover_tools",
            "list_hosts" => "list_hosts",
            "list_activity" => "list_activity",
            _ => "unknown_tool",
        };
        // A foreground executor admits each handler before any activity access
        // and holds the admission through its terminal commit. The SDK detaches
        // handlers, so this, not connection shutdown, is what drain observes; a
        // handler first polled after drain closed admission writes nothing.
        let mut admission = match &self.execution {
            Some(execution) => Some(execution.admit().ok_or_else(|| {
                ErrorData::internal_error(
                    "execution unavailable",
                    Some(json!({"execution_reason":"shutdown"})),
                )
            })?),
            None => None,
        };
        let observation = if label == "discover_tools"
            && (self.activity.dedicated.is_some() || self.execution.is_some())
        {
            Some(
                self.activity
                    .discovery_observation()
                    .map_err(|_| unavailable())?,
            )
        } else {
            None
        };
        let dedicated_row =
            self.activity.dedicated.is_some() && matches!(label, "run_command" | "discover_tools");
        let id = if dedicated_row {
            self.activity.start_dedicated(&principal.0, label)
        } else if let Some(execution) = &self.execution {
            self.activity
                .start_execution(&principal.0, label, execution.identity_mode())
        } else {
            self.activity.start(&principal.0, label)
        }
        .map_err(|_| unavailable())?;
        if !dedicated_row && let Some(admission) = &mut admission {
            admission.started();
        }
        #[cfg(test)]
        let _started = match &self.execution {
            Some(e) => e.checkpoint(crate::development::Checkpoint::Started).await,
            None => None,
        };
        let args = request.arguments.unwrap_or_default();
        if let Some(executor) = &self.activity.dedicated
            && (label == "run_command" || label == "discover_tools")
        {
            // Service authority requires an immutable local connection principal.
            let parsed = if label == "run_command" && !args.values().any(serde_json::Value::is_null)
            {
                serde_json::from_value::<crate::service::executor::Request>(args.clone().into())
                    .ok()
                    .filter(|r| r.validate())
            } else {
                None
            };
            if self.principal.is_none()
                || (label == "run_command" && parsed.is_none())
                || (label == "discover_tools" && !args.is_empty())
            {
                self.activity
                    .finish_typed(
                        id,
                        crate::store::Outcome::Rejected,
                        &crate::store::Metadata {
                            reason: Some(crate::store::Reason::InvalidArguments),
                            ..Default::default()
                        },
                    )
                    .map_err(|_| unavailable())?;
                return Err(ErrorData::invalid_params(
                    "invalid execution arguments",
                    None,
                ));
            }
            let work = executor.dispatch(parsed, self.activity.clone(), id);
            let result = tokio::select! {
                    result = work => result,
                    _ = context.ct.cancelled() => return Err(ErrorData::internal_error("cancelled", None)),
                }.map_err(|e| ErrorData::internal_error(e.reason.as_str(), e.execution_reason.map(|r| json!({"activity_id":id,"outcome":"refused","reason":e.reason.as_str(),"execution_reason":r.as_str()}))))?;
            if let Some(observation) = observation {
                self.activity.cache_discovery(observation, &result);
            }
            return Ok(
                CallToolResult::success(vec![ContentBlock::text(result.to_string())]).into(),
            );
        }
        if label == "run_command" || label == "discover_tools" {
            use crate::{
                development as dev,
                store::{Outcome, Reason},
            };
            let parsed = if label == "run_command" && !args.values().any(serde_json::Value::is_null)
            {
                serde_json::from_value::<dev::Request>(args.clone().into())
                    .ok()
                    .filter(|r| r.validate())
            } else {
                None
            };
            if (self.principal.is_none() && self.execution.is_some())
                || (label == "run_command" && parsed.is_none())
                || (label == "discover_tools" && !args.is_empty())
            {
                let m = if let Some(execution) = &self.execution {
                    execution.metadata(None, None, Some(Reason::InvalidArguments))
                } else {
                    crate::store::Metadata {
                        reason: Some(Reason::InvalidArguments),
                        ..Default::default()
                    }
                };
                self.activity
                    .finish_typed(id, Outcome::Rejected, &m)
                    .map_err(|_| unavailable())?;
                completed(&mut admission);
                return Err(ErrorData::invalid_params(
                    "invalid execution arguments",
                    None,
                ));
            }
            let (Some(executor), Some(admission)) = (&self.execution, admission.take()) else {
                self.activity
                    .finish_typed(
                        id,
                        Outcome::Refused,
                        &crate::store::Metadata {
                            reason: Some(
                                self.activity
                                    .service_reason()
                                    .map(|r| r.stored_reason())
                                    .unwrap_or(Reason::Unavailable),
                            ),
                            ..Default::default()
                        },
                    )
                    .map_err(|_| unavailable())?;
                let reason = self
                    .activity
                    .service_reason()
                    .unwrap_or(crate::service::preflight::ExecutionUnavailable::NotConfigured);
                return Err(ErrorData::internal_error(
                    "execution unavailable",
                    Some(json!({"execution_reason":reason.as_str()})),
                ));
            };
            // Spawned here, before any await: the worker completes the audit row
            // even if cancellation wins the race below or the future is dropped.
            let work = executor.dispatch_admitted(admission, parsed, self.activity.clone(), id);
            let result = tokio::select! {
                result = work => result,
                _ = context.ct.cancelled() => return Err(ErrorData::internal_error("cancelled", None)),
            }
            .map_err(|r| ErrorData::internal_error(r.as_str(),
                (executor.identity_mode() == crate::store::IdentityMode::DedicatedAccount).then(||
                    json!({"execution_mode":"dedicated_account","execution_reason":"dedicated_account_setup_required"}))))?;
            if let Some(observation) = observation {
                self.activity.cache_discovery(observation, &result);
            }
            return Ok(
                CallToolResult::success(vec![ContentBlock::text(result.to_string())]).into(),
            );
        }
        if label == "list_activity" {
            let mut args = args;
            let activity_id = args.remove("activity_id");
            let activity_id = activity_id.map(serde_json::from_value::<i64>).transpose();
            let parsed = if args.values().any(serde_json::Value::is_null) {
                None
            } else {
                serde_json::from_value::<ListActivity>(args.into()).ok()
            }
            .filter(|a| {
                a.validate().is_ok()
                    && matches!(&activity_id, Ok(None) | Ok(Some(1..)))
                    && !(matches!(&activity_id, Ok(Some(_))) && a.before_id.is_some())
            });
            let Some(args) = parsed else {
                self.activity
                    .finish(id, "rejected")
                    .map_err(|_| unavailable())?;
                completed(&mut admission);
                return Err(ErrorData::invalid_params(
                    "invalid activity arguments",
                    None,
                ));
            };
            let page = self
                .activity
                .list_filtered(&principal.0, id, &args, activity_id.unwrap())
                .map_err(|_| unavailable())?;
            self.activity
                .finish(id, "succeeded")
                .map_err(|_| unavailable())?;
            completed(&mut admission);
            let value = serde_json::to_string(&page).map_err(|_| unavailable())?;
            return Ok(CallToolResult::success(vec![ContentBlock::text(value)]).into());
        }
        if label != "list_hosts" || !args.is_empty() {
            self.activity
                .finish(id, "rejected")
                .map_err(|_| unavailable())?;
            completed(&mut admission);
            return Err(ErrorData::invalid_params("unknown tool or arguments", None));
        }
        let host_id = self.activity.host_id().map_err(|_| unavailable())?;
        let mut system = sysinfo::System::new();
        system.refresh_cpu_all();
        system.refresh_memory();
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let mut value = json!({"hosts":[{"host_id":host_id,"os":std::env::consts::OS,"arch":sysinfo::System::cpu_arch(),"logical_cpus":system.cpus().len(),"ram_bytes":system.total_memory(),"disk_total_bytes":disks.iter().fold(0u64, |sum,d| sum.saturating_add(d.total_space())),"disk_available_bytes":disks.iter().fold(0u64, |sum,d| sum.saturating_add(d.available_space())),"execution_available":false,"execution_unavailable_reason":"dedicated account backend not implemented","tool_discovery":"unavailable until dedicated account backend exists"}]});
        let execution = self
            .activity
            .dedicated
            .as_ref()
            .map(|e| e.capability())
            .or_else(|| self.execution.as_ref().map(|e| e.capability()))
            .unwrap_or_else(|| json!({"available":false,"reason":"not_configured"}));
        value["hosts"][0]["execution_available"] = execution["available"].clone();
        value["hosts"][0]["execution_unavailable_reason"] = execution["reason"].clone();
        value["hosts"][0]["execution"] = execution;
        if self.activity.dedicated.is_some() {
            value["hosts"][0]["last_observed_identity"] = serde_json::Value::Null;
            value["hosts"][0]["tools"] = json!([]);
            value["hosts"][0]["tools_observed_at"] = serde_json::Value::Null;
        }
        value["hosts"][0]["tool_discovery"] = json!(if self.activity.dedicated.is_some() {
            "unavailable"
        } else if self
            .execution
            .as_ref()
            .is_some_and(|e| e.capability()["available"] == true)
        {
            "discover_tools"
        } else {
            "not_configured"
        });
        if let Some(cache) = self.activity.tool_cache() {
            // A released prior observation is available even while execution is
            // unavailable. Preserve the no-cache rendering and execution facts.
            value["hosts"][0]["tool_discovery"] = json!("cached");
            value["hosts"][0]["tools"] = cache.tools;
            value["hosts"][0]["tools_observed_at"] = json!(cache.observed_at_ms);
            value["hosts"][0]["last_observed_identity"] = cache.observed_identity;
        }
        self.activity
            .finish(id, "succeeded")
            .map_err(|_| unavailable())?;
        completed(&mut admission);
        Ok(CallToolResult::success(vec![ContentBlock::text(value.to_string())]).into())
    }
}

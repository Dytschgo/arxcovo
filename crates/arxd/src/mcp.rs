use crate::store::{Activity, ListActivity};
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde_json::json;
use std::sync::Arc;
#[derive(Clone)]
pub struct Inventory {
    pub activity: Arc<Activity>,
    pub principal: Option<Principal>,
}
#[derive(Clone)]
pub struct Principal(pub String);
fn unavailable() -> ErrorData {
    ErrorData::internal_error("activity unavailable", None)
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
                    "Observed local facts; execution unavailable until dedicated account backend exists",
                    Arc::new(
                        json!({"type":"object","properties":{},"additionalProperties":false})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                ),
                Tool::new(
                    "list_activity",
                    "Durable activity visible to the authenticated agent",
                    Arc::new(
                        json!({
                            "type":"object",
                            "properties":{
                                "limit":{"type":"integer","minimum":1,"maximum":50,"default":20},
                                "before_id":{"type":"integer","minimum":1},
                                "include_reads":{"type":"boolean","default":false}
                            },
                            "additionalProperties":false
                        })
                        .as_object()
                        .unwrap()
                        .clone(),
                    ),
                ),
            ],
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
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
            "list_hosts" => "list_hosts",
            "list_activity" => "list_activity",
            _ => "unknown_tool",
        };
        let id = self
            .activity
            .start(&principal.0, label)
            .map_err(|_| unavailable())?;
        let args = request.arguments.unwrap_or_default();
        if label == "list_activity" {
            let parsed = if args.values().any(serde_json::Value::is_null) {
                None
            } else {
                serde_json::from_value::<ListActivity>(args.into()).ok()
            }
            .filter(|a| a.validate().is_ok());
            let Some(args) = parsed else {
                self.activity
                    .finish(id, "rejected")
                    .map_err(|_| unavailable())?;
                return Err(ErrorData::invalid_params(
                    "invalid activity arguments",
                    None,
                ));
            };
            let page = self
                .activity
                .list(&principal.0, id, &args)
                .map_err(|_| unavailable())?;
            self.activity
                .finish(id, "succeeded")
                .map_err(|_| unavailable())?;
            let value = serde_json::to_string(&page).map_err(|_| unavailable())?;
            return Ok(CallToolResult::success(vec![ContentBlock::text(value)]).into());
        }
        if label != "list_hosts" || !args.is_empty() {
            self.activity
                .finish(id, "rejected")
                .map_err(|_| unavailable())?;
            return Err(ErrorData::invalid_params("unknown tool or arguments", None));
        }
        let host_id = self.activity.host_id().map_err(|_| unavailable())?;
        let mut system = sysinfo::System::new();
        system.refresh_cpu_all();
        system.refresh_memory();
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let value = json!({"hosts":[{"host_id":host_id,"os":std::env::consts::OS,"arch":sysinfo::System::cpu_arch(),"logical_cpus":system.cpus().len(),"ram_bytes":system.total_memory(),"disk_total_bytes":disks.iter().fold(0u64, |sum,d| sum.saturating_add(d.total_space())),"disk_available_bytes":disks.iter().fold(0u64, |sum,d| sum.saturating_add(d.available_space())),"execution_available":false,"execution_unavailable_reason":"dedicated account backend not implemented","tool_discovery":"unavailable until dedicated account backend exists"}]});
        self.activity
            .finish(id, "succeeded")
            .map_err(|_| unavailable())?;
        Ok(CallToolResult::success(vec![ContentBlock::text(value.to_string())]).into())
    }
}

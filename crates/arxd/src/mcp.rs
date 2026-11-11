use crate::store::Activity;
use rmcp::{ErrorData, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde_json::json;
use std::sync::Arc;
#[derive(Clone)]
pub struct Inventory {
    pub activity: Arc<Activity>,
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
            tools: vec![Tool::new(
                "list_hosts",
                "Observed local facts; execution unavailable until dedicated account backend exists",
                Arc::new(
                    json!({"type":"object","properties":{},"additionalProperties":false})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )],
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let principal = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|p| p.extensions.get::<Principal>())
            .ok_or_else(|| ErrorData::internal_error("authentication context unavailable", None))?;
        // Never persist attacker-provided tool names or arguments.
        let tool = if request.name == "list_hosts" {
            "list_hosts"
        } else {
            "unknown_tool"
        };
        let id = self
            .activity
            .start(&principal.0, tool)
            .map_err(|_| unavailable())?;
        if request.name != "list_hosts" || request.arguments.as_ref().is_some_and(|a| !a.is_empty())
        {
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

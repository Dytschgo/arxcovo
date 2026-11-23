use super::{
    Action, Refusal, Reply, ScriptOs, Vault, Write, parse_id, read, utc_ms, valid_name,
    valid_script,
};
use crate::vm::{RequestId, UbuntuRecipe};
use rmcp::model::{CallToolResult, Tool};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use std::sync::Arc;

const ID_PATTERN: &str = "^[a-f0-9]{32}$";
const PAGE_CURSOR_PATTERN: &str = "^[a-f0-9]{82}$";

pub(crate) fn tools() -> impl Iterator<Item = Tool> {
    let page = json!({
        "limit":{"type":"integer","minimum":1,"maximum":20,"default":20},
        "cursor":{"type":"string","pattern":PAGE_CURSOR_PATTERN}
    });
    let id = json!({"type":"string","pattern":ID_PATTERN});
    let mut history = page.clone();
    history["script_id"] = id.clone();
    let version_write = json!({"version_id":id,"request_id":id});
    [
        (
            "vault_list",
            "List scripts owned by this local principal",
            page,
            &[][..],
        ),
        (
            "vault_get",
            "Read a version and optionally its content",
            json!({
                "version_id":id,"include_content":{"type":"boolean","default":false}
            }),
            &["version_id"][..],
        ),
        (
            "vault_add_version",
            "Store an immutable draft for this OS and script name",
            json!({
                "os":{"type":"string","enum":["windows","ubuntu"]},
                "name":{"type":"string","pattern":"^[a-z0-9_-]{1,48}$"},
                "script":{"type":"string","minLength":1,"maxLength":arx_guest::MAX_SCRIPT,
                    "description":"At most 4096 UTF-8 bytes, with no NUL"},
                "request_id":id
            }),
            &["os", "name", "script", "request_id"][..],
        ),
        (
            "vault_test_version",
            "Queue a draft with a qualified throwaway VM tester",
            version_write.clone(),
            &["version_id", "request_id"][..],
        ),
        (
            "vault_activate",
            "Activate a version with a recorded passing test",
            version_write,
            &["version_id", "request_id"][..],
        ),
        (
            "vault_history",
            "List immutable versions of one owned script",
            history,
            &["script_id"][..],
        ),
        (
            "template_tools",
            "Show declared tool inventory from available template recipes",
            json!({}),
            &[][..],
        ),
    ]
    .into_iter()
    .map(|(name, description, properties, required)| {
        Tool::new(name, description, Arc::new(schema(properties, required)))
    })
}

fn schema(properties: Value, required: &[&str]) -> Map<String, Value> {
    let mut result = Map::from_iter([
        ("type".into(), json!("object")),
        ("properties".into(), properties),
        ("additionalProperties".into(), json!(false)),
    ]);
    if !required.is_empty() {
        result.insert("required".into(), json!(required));
    }
    result
}

pub(crate) fn extend_activity_schema(tool: &mut Tool) {
    let schema = Arc::make_mut(&mut tool.input_schema);
    let legacy = Value::Object(std::mem::take(schema));
    let audit = schema_for_audit();
    *schema = Map::from_iter([
        ("type".into(), json!("object")),
        ("oneOf".into(), json!([legacy, audit])),
    ]);
    tool.description = Some(
        "Read caller-scoped activity. Use source vault for vault mutations and test results."
            .into(),
    );
}

fn schema_for_audit() -> Map<String, Value> {
    schema(
        json!({
            "source":{"const":"vault"},
            "limit":{"type":"integer","minimum":1,"maximum":20,"default":20},
            "cursor":{"type":"string","pattern":PAGE_CURSOR_PATTERN}
        }),
        &["source"],
    )
}

pub(crate) fn handles(name: &str, args: Option<&Map<String, Value>>) -> bool {
    matches!(
        name,
        "vault_list"
            | "vault_get"
            | "vault_add_version"
            | "vault_test_version"
            | "vault_activate"
            | "vault_history"
            | "template_tools"
    ) || (name == "list_activity" && args.is_some_and(|a| a.contains_key("source")))
}

pub(crate) fn call(
    vault: &Arc<Vault>,
    principal: &str,
    name: &str,
    arguments: Map<String, Value>,
) -> CallToolResult {
    let reply = match crate::store::validate_principal(principal) {
        Ok(()) => dispatch(vault, principal, name, arguments),
        Err(_) => Err(Refusal::Invalid),
    };
    match reply {
        Ok(Value::Object(mut fields)) => {
            fields.insert("ok".into(), json!(true));
            CallToolResult::structured(Value::Object(fields))
        }
        Ok(_) => unreachable!("vault replies are objects"),
        Err(reason) => {
            CallToolResult::structured_error(json!({"ok": false, "reason": reason.as_str()}))
        }
    }
}

fn dispatch(vault: &Arc<Vault>, owner: &str, name: &str, mut args: Map<String, Value>) -> Reply {
    let action = match name {
        "vault_add_version" => Some(Action::Add),
        "vault_test_version" => Some(Action::Test),
        "vault_activate" => Some(Action::Activate),
        _ => None,
    };
    if let Some(action) = action {
        let now = utc_ms().map_err(|_| Refusal::StateUnavailable)?;
        return vault.mutate(owner, action, parse_write(action, args), now);
    }
    match name {
        "vault_list" => vault.page(owner, read::List::Scripts, parse(args)?),
        "vault_get" => {
            let args: Get = parse(args)?;
            vault.get(owner, parse_id(&args.version_id)?, args.include_content)
        }
        "vault_history" => {
            let id = args
                .remove("script_id")
                .and_then(|v| v.as_str().map(parse_id))
                .ok_or(Refusal::Invalid)??;
            vault.page(owner, read::List::Versions(id), parse(args)?)
        }
        "list_activity"
            if args.remove("source").as_ref().and_then(Value::as_str) == Some("vault") =>
        {
            vault.page(owner, read::List::Audit, parse(args)?)
        }
        "template_tools" if args.is_empty() => Ok(json!({"entries":[
            {
                "os":"ubuntu","arch":"x86_64","recipe":UbuntuRecipe::ID,
                "inventory":"declared","tools":["arx-guest"]
            },
            {"os":"windows","inventory":"unavailable","reason":"recipe_unavailable"}
        ]})),
        _ => Err(Refusal::Invalid),
    }
}

fn parse<T: DeserializeOwned>(args: Map<String, Value>) -> Result<T, Refusal> {
    if args.values().any(Value::is_null) {
        return Err(Refusal::Invalid);
    }
    serde_json::from_value(Value::Object(args)).map_err(|_| Refusal::Invalid)
}

fn parse_write(action: Action, args: Map<String, Value>) -> Result<Write, Refusal> {
    if matches!(action, Action::Add) {
        let add: Add = parse(args)?;
        if !valid_name(&add.name) || !valid_script(&add.script) {
            return Err(Refusal::Invalid);
        }
        return Ok(Write::Add {
            request_id: request(&add.request_id)?,
            os: add.os,
            name: add.name,
            script: add.script,
        });
    }
    let args: VersionWrite = parse(args)?;
    let request_id = request(&args.request_id)?;
    let version_id = parse_id(&args.version_id)?;
    Ok(match action {
        Action::Test => Write::Test {
            request_id,
            version_id,
        },
        Action::Activate => Write::Activate {
            request_id,
            version_id,
        },
        Action::Add => unreachable!("handled above"),
    })
}

fn request(value: &str) -> Result<RequestId, Refusal> {
    RequestId::parse(value).map_err(|_| Refusal::Invalid)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Add {
    request_id: String,
    os: ScriptOs,
    name: String,
    script: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionWrite {
    request_id: String,
    version_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Get {
    version_id: String,
    #[serde(default)]
    include_content: bool,
}

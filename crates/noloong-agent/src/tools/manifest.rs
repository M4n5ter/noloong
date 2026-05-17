use crate::{BuiltInToolName, Catalog, ManifestPatch, ManifestProposalStore, MessageKey};
use noloong_agent_core::{
    BoxFuture, CancellationToken, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde_json::{Value, json};
use std::sync::LazyLock;

use super::{json_tool_output, sequential_tool_spec};

#[derive(Clone)]
pub struct ManifestPatchProposalTool {
    store: ManifestProposalStore,
    catalog: Catalog,
}

impl ManifestPatchProposalTool {
    pub fn new(store: ManifestProposalStore, catalog: Catalog) -> Self {
        Self { store, catalog }
    }
}

impl ToolProvider for ManifestPatchProposalTool {
    fn spec(&self) -> ToolSpec {
        sequential_tool_spec(
            BuiltInToolName::ManifestProposePatch.as_str(),
            self.catalog.message(MessageKey::ManifestPatchDescription),
            manifest_patch_input_schema(),
            "agent.manifest.patch",
            self.catalog
                .message(MessageKey::ManifestPatchPermissionDescription),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let patch_value = request.arguments.get("patch").cloned().ok_or_else(|| {
                noloong_agent_core::AgentCoreError::InvalidEffect(
                    self.catalog.missing_manifest_patch_argument().into(),
                )
            })?;
            let patch = serde_json::from_value::<ManifestPatch>(patch_value).map_err(|error| {
                noloong_agent_core::AgentCoreError::InvalidEffect(
                    self.catalog.render_tool_input_error(error),
                )
            })?;
            let summary = self.catalog.render_manifest_patch_summary(&patch);
            let proposal = self
                .store
                .record_pending_proposal_with_summary(patch, Some(summary))
                .map_err(|error| {
                    noloong_agent_core::AgentCoreError::Provider(
                        self.catalog.render_manifest_error(&error),
                    )
                })?;
            let value = json!(proposal);
            Ok(json_tool_output(value))
        })
    }
}

fn manifest_patch_input_schema() -> Value {
    static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
        json!({
            "type": "object",
            "required": ["patch"],
            "properties": {
                "patch": {
                    "type": "object",
                    "oneOf": manifest_patch_variants(),
                }
            }
        })
    });
    SCHEMA.clone()
}

fn manifest_patch_variants() -> Vec<Value> {
    vec![
        patch_variant(
            &["op", "prompt"],
            json!({
                "op": {"const": "replace_system_prompt"},
                "prompt": {"type": "string"},
            }),
        ),
        patch_variant(
            &["op"],
            json!({"op": {"const": "use_built_in_system_prompt"}}),
        ),
        patch_variant(
            &["op", "profile"],
            json!({
                "op": {"const": "set_built_in_system_prompt_profile"},
                "profile": {"enum": ["auto", "general", "openai"]},
            }),
        ),
        patch_variant(
            &["op", "addition"],
            json!({
                "op": {"const": "upsert_system_prompt_addition"},
                "addition": {
                    "type": "object",
                    "required": ["id", "text"],
                    "properties": {
                        "id": {"type": "string"},
                        "text": {"type": "string"},
                        "enabled": {"type": "boolean"},
                    },
                },
            }),
        ),
        patch_variant(
            &["op", "id"],
            json!({
                "op": {"const": "remove_system_prompt_addition"},
                "id": {"type": "string"},
            }),
        ),
        patch_variant(
            &["op", "id", "enabled"],
            json!({
                "op": {"const": "set_system_prompt_addition_enabled"},
                "id": {"type": "string"},
                "enabled": {"type": "boolean"},
            }),
        ),
        patch_variant(
            &["op", "ids"],
            json!({
                "op": {"const": "reorder_system_prompt_additions"},
                "ids": {"type": "array", "items": {"type": "string"}},
            }),
        ),
        patch_variant(
            &["op"],
            json!({"op": {"const": "clear_system_prompt_additions"}}),
        ),
        patch_variant(
            &["op", "locale"],
            json!({
                "op": {"const": "set_locale"},
                "locale": {"enum": ["en", "zh"]},
            }),
        ),
        patch_variant(
            &["op", "toolName"],
            json!({
                "op": {"enum": ["enable_tool", "disable_tool"]},
                "toolName": {"type": "string"},
            }),
        ),
        patch_variant(
            &["op", "policy"],
            json!({
                "op": {"const": "update_file_edit_tool_policy"},
                "policy": {"enum": ["auto_by_model", "apply_patch", "write_file", "disabled"]},
            }),
        ),
        register_plugin_patch_variant(),
        patch_variant(
            &["op", "pluginId", "enabled"],
            json!({
                "op": {"const": "set_plugin_enabled"},
                "pluginId": {"type": "string"},
                "enabled": {"type": "boolean"},
            }),
        ),
        patch_variant(
            &["op", "pluginId"],
            json!({
                "op": {"const": "remove_plugin"},
                "pluginId": {"type": "string"},
            }),
        ),
    ]
}

fn patch_variant(required: &[&str], properties: Value) -> Value {
    json!({"required": required, "properties": properties})
}

fn register_plugin_patch_variant() -> Value {
    patch_variant(
        &["op", "plugin"],
        json!({
            "op": {"const": "register_plugin"},
            "plugin": plugin_schema(),
        }),
    )
}

fn plugin_schema() -> Value {
    json!({
        "type": "object",
        "required": ["pluginId", "displayName", "components"],
        "properties": {
            "pluginId": {"type": "string"},
            "displayName": {"type": "string"},
            "description": {"type": "string"},
            "enabled": {"type": "boolean"},
            "onLoadFailure": {"enum": ["disable_for_run", "fail_run"]},
            "components": {
                "type": "array",
                "minItems": 1,
                "items": {"oneOf": plugin_component_variants()},
            },
        },
    })
}

fn plugin_component_variants() -> Vec<Value> {
    vec![
        patch_variant(
            &["type", "roots"],
            json!({
                "type": {"const": "skills"},
                "roots": {"type": "array", "minItems": 1, "items": {"type": "string"}},
            }),
        ),
        patch_variant(
            &["type", "serverId", "transport"],
            json!({
                "type": {"const": "mcp"},
                "serverId": {"type": "string"},
                "transport": {"oneOf": mcp_transport_variants()},
                "enabledTools": {"type": "array", "items": {"type": "string"}},
                "disabledTools": {"type": "array", "items": {"type": "string"}},
                "toolNamePrefix": {"type": "string"},
                "requestTimeoutSecs": {"type": "integer", "minimum": 1},
            }),
        ),
        patch_variant(
            &["type", "transport"],
            json!({
                "type": {"const": "noloong_extension"},
                "transport": stdio_transport_schema(),
                "allowedCapabilities": {
                    "type": "array",
                    "items": {"oneOf": capability_selector_variants()},
                },
            }),
        ),
    ]
}

fn mcp_transport_variants() -> Vec<Value> {
    vec![
        stdio_transport_variant(),
        streamable_http_transport_variant(),
    ]
}

fn stdio_transport_variant() -> Value {
    patch_variant(&["type", "command"], mcp_stdio_transport_properties())
}

fn stdio_transport_schema() -> Value {
    json!({
        "type": "object",
        "required": ["type", "command"],
        "properties": extension_stdio_transport_properties(),
    })
}

fn mcp_stdio_transport_properties() -> Value {
    json!({
        "type": {"const": "stdio"},
        "command": {"type": "string"},
        "args": {"type": "array", "items": {"type": "string"}},
        "cwd": {"type": "string"},
        "env": plugin_env_map_schema(),
        "requestTimeoutSecs": {"type": "integer", "minimum": 1},
    })
}

fn extension_stdio_transport_properties() -> Value {
    json!({
        "type": {"const": "stdio"},
        "command": {"type": "string"},
        "args": {"type": "array", "items": {"type": "string"}},
        "cwd": {"type": "string"},
        "env": plugin_env_map_schema(),
        "requestTimeoutSecs": {"type": "integer", "minimum": 1},
        "streamTimeoutSecs": {"type": "integer", "minimum": 1},
    })
}

fn streamable_http_transport_variant() -> Value {
    patch_variant(
        &["type", "url"],
        json!({
            "type": {"const": "streamable_http"},
            "url": {"type": "string"},
            "headers": mcp_header_map_schema(),
            "connectTimeoutSecs": {"type": "integer", "minimum": 1},
            "requestTimeoutSecs": {"type": "integer", "minimum": 1},
        }),
    )
}

fn plugin_env_map_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": plugin_env_source_schema(),
    })
}

fn plugin_env_source_schema() -> Value {
    json!({
        "oneOf": [
            patch_variant(
                &["type", "name"],
                json!({
                    "type": {"const": "host_env"},
                    "name": {"type": "string"},
                }),
            ),
        ],
    })
}

fn mcp_header_map_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": mcp_header_source_schema(),
    })
}

fn mcp_header_source_schema() -> Value {
    json!({
        "oneOf": [
            patch_variant(
                &["type", "value"],
                json!({
                    "type": {"const": "static"},
                    "value": {"type": "string"},
                }),
            ),
            patch_variant(
                &["type", "name"],
                json!({
                    "type": {"const": "host_env"},
                    "name": {"type": "string"},
                    "prefix": {"type": "string"},
                }),
            ),
        ],
    })
}

fn capability_selector_variants() -> Vec<Value> {
    [
        ("model_provider", "id"),
        ("tool", "name"),
        ("context_provider", "id"),
        ("phase_node", "id"),
        ("phase_hook", "id"),
        ("tool_call_hook", "id"),
        ("compaction_summarizer", "id"),
        ("context_compactor", "id"),
        ("http_auth_provider", "id"),
    ]
    .into_iter()
    .map(|(selector_type, field)| capability_selector_variant(selector_type, field))
    .collect()
}

fn capability_selector_variant(selector_type: &str, field: &str) -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert("type".into(), json!({"const": selector_type}));
    properties.insert(field.into(), json!({"type": "string"}));
    json!({
        "required": ["type", field],
        "properties": properties,
    })
}

#[cfg(test)]
mod tests {
    use super::manifest_patch_input_schema;
    use serde_json::Value;
    use std::collections::BTreeMap;

    #[test]
    fn manifest_patch_schema_matches_capability_selector_shapes() {
        let schema = manifest_patch_input_schema();
        let plugin_variant = schema["properties"]["patch"]["oneOf"]
            .as_array()
            .expect("manifest patch schema should use oneOf")
            .iter()
            .find(|variant| variant["properties"]["op"]["const"] == "register_plugin")
            .expect("register plugin schema should exist");
        let component_variants = plugin_variant["properties"]["plugin"]["properties"]["components"]
            ["items"]["oneOf"]
            .as_array()
            .expect("plugin components schema should use oneOf");
        let extension_component = component_variants
            .iter()
            .find(|variant| variant["properties"]["type"]["const"] == "noloong_extension")
            .expect("noloong extension component schema should exist");
        let mcp_component = component_variants
            .iter()
            .find(|variant| variant["properties"]["type"]["const"] == "mcp")
            .expect("mcp component schema should exist");
        let mcp_stdio_transport = mcp_component["properties"]["transport"]["oneOf"]
            .as_array()
            .expect("mcp transport schema should use oneOf")
            .iter()
            .find(|variant| variant["properties"]["type"]["const"] == "stdio")
            .expect("mcp stdio transport schema should exist");
        let extension_stdio_transport = &extension_component["properties"]["transport"];

        assert!(mcp_stdio_transport["properties"]["streamTimeoutSecs"].is_null());
        assert_eq!(
            extension_stdio_transport["properties"]["streamTimeoutSecs"]["type"],
            "integer"
        );
        assert_eq!(
            mcp_stdio_transport["properties"]["env"]["additionalProperties"]["oneOf"][0]["properties"]
                ["type"]["const"],
            "host_env"
        );
        assert_eq!(
            mcp_component["properties"]["transport"]["oneOf"][1]["properties"]["headers"]["additionalProperties"]
                ["oneOf"][0]["properties"]["type"]["const"],
            "static"
        );

        let variants = extension_component["properties"]["allowedCapabilities"]["items"]["oneOf"]
            .as_array()
            .expect("capability selector schema should use oneOf");

        let required_by_type = variants
            .iter()
            .map(|variant| {
                let selector_type = variant["properties"]["type"]["const"]
                    .as_str()
                    .expect("selector type should be const");
                let required = required_fields(variant);
                (selector_type, required)
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(required_by_type.len(), 9);
        assert_eq!(required_by_type["tool"], vec!["type", "name"]);
        for selector_type in [
            "model_provider",
            "context_provider",
            "phase_node",
            "phase_hook",
            "tool_call_hook",
            "compaction_summarizer",
            "context_compactor",
            "http_auth_provider",
        ] {
            assert_eq!(required_by_type[selector_type], vec!["type", "id"]);
        }
    }

    fn required_fields(value: &Value) -> Vec<&str> {
        value["required"]
            .as_array()
            .expect("schema variant should declare required fields")
            .iter()
            .map(|value| value.as_str().expect("required field should be string"))
            .collect::<Vec<_>>()
    }
}

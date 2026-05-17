use crate::{BuiltInToolName, Catalog, ManifestPatch, ManifestProposalStore, MessageKey};
use noloong_agent_core::{
    BoxFuture, CancellationToken, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde_json::{Value, json};

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
    serde_json::from_str(
        r#"{
            "type": "object",
            "required": ["patch"],
            "properties": {
                "patch": {
                    "type": "object",
                    "oneOf": [
                        {"required": ["op", "prompt"], "properties": {"op": {"const": "replace_system_prompt"}, "prompt": {"type": "string"}}},
                        {"required": ["op"], "properties": {"op": {"const": "use_built_in_system_prompt"}}},
                        {"required": ["op", "profile"], "properties": {"op": {"const": "set_built_in_system_prompt_profile"}, "profile": {"enum": ["auto", "general", "openai"]}}},
                        {"required": ["op", "addition"], "properties": {"op": {"const": "upsert_system_prompt_addition"}, "addition": {"type": "object", "required": ["id", "text"], "properties": {"id": {"type": "string"}, "text": {"type": "string"}, "enabled": {"type": "boolean"}}}}},
                        {"required": ["op", "id"], "properties": {"op": {"const": "remove_system_prompt_addition"}, "id": {"type": "string"}}},
                        {"required": ["op", "id", "enabled"], "properties": {"op": {"const": "set_system_prompt_addition_enabled"}, "id": {"type": "string"}, "enabled": {"type": "boolean"}}},
                        {"required": ["op", "ids"], "properties": {"op": {"const": "reorder_system_prompt_additions"}, "ids": {"type": "array", "items": {"type": "string"}}}},
                        {"required": ["op"], "properties": {"op": {"const": "clear_system_prompt_additions"}}},
                        {"required": ["op", "locale"], "properties": {"op": {"const": "set_locale"}, "locale": {"enum": ["en", "zh"]}}},
                        {"required": ["op", "toolName"], "properties": {"op": {"enum": ["enable_tool", "disable_tool"]}, "toolName": {"type": "string"}}},
                        {"required": ["op", "policy"], "properties": {"op": {"const": "update_file_edit_tool_policy"}, "policy": {"enum": ["auto_by_model", "apply_patch", "write_file", "disabled"]}}},
                        {
                            "required": ["op", "plugin"],
                            "properties": {
                                "op": {"const": "register_plugin"},
                                "plugin": {
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
                                            "items": {
                                                "oneOf": [
                                                    {"required": ["type", "roots"], "properties": {"type": {"const": "skills"}, "roots": {"type": "array", "minItems": 1, "items": {"type": "string"}}}},
                                                    {"required": ["type", "serverId", "transport"], "properties": {"type": {"const": "mcp"}, "serverId": {"type": "string"}, "transport": {"oneOf": [
                                                        {"required": ["type", "command"], "properties": {"type": {"const": "stdio"}, "command": {"type": "string"}, "args": {"type": "array", "items": {"type": "string"}}, "cwd": {"type": "string"}, "env": {"type": "object"}, "requestTimeoutSecs": {"type": "integer", "minimum": 1}, "streamTimeoutSecs": {"type": "integer", "minimum": 1}}},
                                                        {"required": ["type", "url"], "properties": {"type": {"const": "streamable_http"}, "url": {"type": "string"}, "headers": {"type": "object"}, "connectTimeoutSecs": {"type": "integer", "minimum": 1}, "requestTimeoutSecs": {"type": "integer", "minimum": 1}}}
                                                    ]}, "enabledTools": {"type": "array", "items": {"type": "string"}}, "disabledTools": {"type": "array", "items": {"type": "string"}}, "toolNamePrefix": {"type": "string"}, "requestTimeoutSecs": {"type": "integer", "minimum": 1}}},
                                                    {
                                                        "required": ["type", "transport"],
                                                        "properties": {
                                                            "type": {"const": "noloong_extension"},
                                                            "transport": {"type": "object", "required": ["type", "command"], "properties": {"type": {"const": "stdio"}, "command": {"type": "string"}, "args": {"type": "array", "items": {"type": "string"}}, "cwd": {"type": "string"}, "env": {"type": "object"}, "requestTimeoutSecs": {"type": "integer", "minimum": 1}, "streamTimeoutSecs": {"type": "integer", "minimum": 1}}},
                                                            "allowedCapabilities": {
                                                                "type": "array",
                                                                "items": {
                                                                    "oneOf": [
                                                                        {"required": ["type", "id"], "properties": {"type": {"const": "model_provider"}, "id": {"type": "string"}}},
                                                                        {"required": ["type", "name"], "properties": {"type": {"const": "tool"}, "name": {"type": "string"}}},
                                                                        {"required": ["type", "id"], "properties": {"type": {"const": "context_provider"}, "id": {"type": "string"}}},
                                                                        {"required": ["type", "id"], "properties": {"type": {"const": "phase_node"}, "id": {"type": "string"}}},
                                                                        {"required": ["type", "id"], "properties": {"type": {"const": "phase_hook"}, "id": {"type": "string"}}},
                                                                        {"required": ["type", "id"], "properties": {"type": {"const": "tool_call_hook"}, "id": {"type": "string"}}},
                                                                        {"required": ["type", "id"], "properties": {"type": {"const": "compaction_summarizer"}, "id": {"type": "string"}}},
                                                                        {"required": ["type", "id"], "properties": {"type": {"const": "context_compactor"}, "id": {"type": "string"}}},
                                                                        {"required": ["type", "id"], "properties": {"type": {"const": "http_auth_provider"}, "id": {"type": "string"}}}
                                                                    ]
                                                                }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        {"required": ["op", "pluginId", "enabled"], "properties": {"op": {"const": "set_plugin_enabled"}, "pluginId": {"type": "string"}, "enabled": {"type": "boolean"}}},
                        {"required": ["op", "pluginId"], "properties": {"op": {"const": "remove_plugin"}, "pluginId": {"type": "string"}}}
                    ]
                }
            }
        }"#,
    )
    .expect("manifest patch tool input schema is valid JSON")
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

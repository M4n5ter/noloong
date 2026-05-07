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
                        {
                            "required": ["op", "prompt"],
                            "properties": {
                                "op": {"const": "replace_system_prompt"},
                                "prompt": {"type": "string"}
                            }
                        },
                        {
                            "required": ["op", "locale"],
                            "properties": {
                                "op": {"const": "set_locale"},
                                "locale": {"enum": ["en", "zh"]}
                            }
                        },
                        {
                            "required": ["op", "toolName"],
                            "properties": {
                                "op": {"enum": ["enable_tool", "disable_tool"]},
                                "toolName": {"type": "string"}
                            }
                        },
                        {
                            "required": ["op", "policy"],
                            "properties": {
                                "op": {"const": "update_file_edit_tool_policy"},
                                "policy": {
                                    "enum": [
                                        "auto_by_model",
                                        "apply_patch",
                                        "write_file",
                                        "disabled"
                                    ]
                                }
                            }
                        },
                        {
                            "required": ["op", "plugin"],
                            "properties": {
                                "op": {"const": "register_plugin"},
                                "plugin": {
                                    "type": "object",
                                    "required": ["pluginId", "displayName", "transport"],
                                    "properties": {
                                        "pluginId": {"type": "string"},
                                        "displayName": {"type": "string"},
                                        "description": {"type": "string"},
                                        "enabled": {"type": "boolean"},
                                        "onLoadFailure": {
                                            "enum": ["disable_for_run", "fail_run"]
                                        },
                                        "allowedCapabilities": {
                                            "type": "array",
                                            "items": {
                                                "type": "object",
                                                "required": ["type"],
                                                "properties": {
                                                    "type": {
                                                        "enum": [
                                                            "model_provider",
                                                            "tool",
                                                            "context_provider",
                                                            "phase_node",
                                                            "phase_hook",
                                                            "tool_call_hook",
                                                            "compaction_summarizer",
                                                            "context_compactor",
                                                            "http_auth_provider"
                                                        ]
                                                    },
                                                    "id": {"type": "string"},
                                                    "name": {"type": "string"}
                                                }
                                            }
                                        },
                                        "transport": {
                                            "type": "object",
                                            "required": ["type", "command"],
                                            "properties": {
                                                "type": {"const": "stdio"},
                                                "command": {"type": "string"},
                                                "args": {
                                                    "type": "array",
                                                    "items": {"type": "string"}
                                                },
                                                "cwd": {"type": "string"},
                                                "env": {
                                                    "type": "object",
                                                    "additionalProperties": {
                                                        "type": "object",
                                                        "required": ["type", "name"],
                                                        "properties": {
                                                            "type": {"const": "host_env"},
                                                            "name": {"type": "string"}
                                                        }
                                                    }
                                                },
                                                "requestTimeoutSecs": {
                                                    "type": "integer",
                                                    "minimum": 1
                                                },
                                                "streamTimeoutSecs": {
                                                    "type": "integer",
                                                    "minimum": 1
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        {
                            "required": ["op", "pluginId", "enabled"],
                            "properties": {
                                "op": {"const": "set_plugin_enabled"},
                                "pluginId": {"type": "string"},
                                "enabled": {"type": "boolean"}
                            }
                        },
                        {
                            "required": ["op", "pluginId"],
                            "properties": {
                                "op": {"const": "remove_plugin"},
                                "pluginId": {"type": "string"}
                            }
                        }
                    ]
                }
            }
        }"#,
    )
    .expect("manifest patch tool input schema is valid JSON")
}

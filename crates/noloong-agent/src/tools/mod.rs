mod file_edit;
mod host_exec;
mod manifest;
mod output_overflow;
mod subagent;

use noloong_agent_core::{
    ContentBlock, ToolExecutionMode, ToolOutput, ToolPermissionRequirement, ToolSpec,
};
use serde_json::{Value, json};

pub(crate) use file_edit::apply_patch_target_paths;
pub use file_edit::{
    APPLY_PATCH_TOOL_NAME, ApplyPatchTool, FILE_EDIT_PERMISSION_CAPABILITY, FileEditManager,
    WRITE_FILE_TOOL_NAME, WriteFileTool,
};
pub use host_exec::{
    HostExecListTool, HostExecReadTool, HostExecStartTool, HostExecTerminateTool, HostExecWaitTool,
    HostExecWriteTool,
};
pub use manifest::ManifestPatchProposalTool;
pub use output_overflow::{
    BuiltInToolOutputOverflowHook, DEFAULT_MAX_INLINE_TOOL_OUTPUT_BYTES,
    DEFAULT_TOOL_OUTPUT_PREVIEW_EDGE_BYTES, ToolOutputOverflowConfig,
};
pub use subagent::{
    DEFAULT_SUBAGENT_WAIT_TIMEOUT_MS, MAX_SUBAGENT_WAIT_TIMEOUT_MS, MIN_SUBAGENT_WAIT_TIMEOUT_MS,
    SubagentController, SubagentFinalOutput, SubagentListTool, SubagentResult, SubagentResultTool,
    SubagentSpawnRequest, SubagentSpawnTool, SubagentSummary, SubagentWaitOutcome,
    SubagentWaitTool, final_assistant_output,
};

pub(crate) fn json_tool_output(value: Value) -> ToolOutput {
    ToolOutput {
        content: vec![ContentBlock::Json {
            value: value.clone(),
        }],
        details: value,
        is_error: false,
        updates: Vec::new(),
    }
}

pub(crate) fn json_tool_error(
    code: &str,
    message: impl Into<String>,
    details: Value,
) -> ToolOutput {
    let message = message.into();
    let value = json!({
        "error": {
            "code": code,
            "message": message,
            "details": details,
        },
    });
    ToolOutput {
        content: vec![ContentBlock::Json {
            value: value.clone(),
        }],
        details: value,
        is_error: true,
        updates: Vec::new(),
    }
}

pub(crate) fn sequential_tool_spec(
    name: &str,
    description: &str,
    input_schema: Value,
    capability: &str,
    permission_description: &str,
) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: description.into(),
        input_schema,
        execution_mode: Some(ToolExecutionMode::Sequential),
        permissions: vec![ToolPermissionRequirement {
            capability: capability.into(),
            description: Some(permission_description.into()),
            metadata: json!({
                "builtIn": true,
                "capability": capability,
                "tool": name,
            }),
        }],
    }
}

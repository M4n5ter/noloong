mod host_exec;
mod manifest;
mod output_overflow;

use noloong_agent_core::{
    ContentBlock, ToolExecutionMode, ToolOutput, ToolPermissionRequirement, ToolSpec,
};
use serde_json::{Value, json};

pub use host_exec::{
    HostExecListTool, HostExecReadTool, HostExecStartTool, HostExecTerminateTool, HostExecWaitTool,
    HostExecWriteTool,
};
pub use manifest::ManifestPatchProposalTool;
pub use output_overflow::{
    BuiltInToolOutputOverflowHook, DEFAULT_MAX_INLINE_TOOL_OUTPUT_BYTES,
    DEFAULT_TOOL_OUTPUT_PREVIEW_EDGE_BYTES, ToolOutputOverflowConfig,
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
            metadata: json!({"tool": name}),
        }],
    }
}

use noloong_agent_core::ToolCall;
use serde_json::{Map, Value};

use super::constants::{
    REVIEWER_HUMAN, REVIEWER_METADATA, TOOL_CALL_ID_METADATA, TOOL_NAME_METADATA,
};

pub(super) fn tool_metadata(tool_call: &ToolCall) -> Value {
    let mut metadata = Map::new();
    metadata.insert(
        TOOL_NAME_METADATA.into(),
        Value::String(tool_call.name.clone()),
    );
    metadata.insert(
        TOOL_CALL_ID_METADATA.into(),
        Value::String(tool_call.id.clone()),
    );
    Value::Object(metadata)
}

pub(super) fn human_reviewer_tool_metadata(tool_call: &ToolCall) -> Value {
    let mut metadata = tool_metadata(tool_call);
    if let Value::Object(map) = &mut metadata {
        map.insert(
            REVIEWER_METADATA.into(),
            Value::String(REVIEWER_HUMAN.into()),
        );
    }
    metadata
}

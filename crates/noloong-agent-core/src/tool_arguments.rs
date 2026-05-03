use serde_json::{Map, Value};

pub(crate) fn parse_tool_arguments(arguments_json: &str) -> Value {
    if arguments_json.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(arguments_json)
            .unwrap_or_else(|_| Value::String(arguments_json.to_string()))
    }
}

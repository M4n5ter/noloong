use super::AgentSessionRecord;
use crate::interaction::InteractionError;

pub(super) fn encode_record_json(record: &AgentSessionRecord) -> Result<String, InteractionError> {
    serde_json::to_string(record).map_err(|error| {
        InteractionError::internal(format!(
            "failed to encode session record {}: {error}",
            record.session_id
        ))
    })
}

pub(super) fn decode_record_json(
    label: &str,
    bytes: &[u8],
) -> Result<AgentSessionRecord, InteractionError> {
    serde_json::from_slice(bytes).map_err(|error| {
        InteractionError::internal(format!("failed to decode session record {label}: {error}"))
    })
}

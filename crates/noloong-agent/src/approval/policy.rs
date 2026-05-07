#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ApprovalPolicy {
    AllowAll,
    #[default]
    RequireApproval,
    AutoReview {
        fallback_to_human: bool,
    },
}

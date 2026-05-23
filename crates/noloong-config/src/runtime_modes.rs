use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesStateMode {
    #[default]
    Stateless,
    Stateful,
}

impl ResponsesStateMode {
    pub const fn is_stateless(self) -> bool {
        matches!(self, Self::Stateless)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompactionMode {
    #[default]
    PersistentState,
    RequestOnly,
}

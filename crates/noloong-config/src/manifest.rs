use crate::{AgentPluginDeclaration, Locale};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ApprovalPolicy {
    AllowAll,
    #[default]
    RequireApproval,
    AutoReview {
        fallback_to_human: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuiltInSystemPromptProfile {
    #[default]
    Auto,
    General,
    OpenAi,
}

impl BuiltInSystemPromptProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::General => "general",
            Self::OpenAi => "openai",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemPromptAddition {
    pub id: String,
    pub text: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl SystemPromptAddition {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ManifestPatch {
    ReplaceSystemPrompt {
        prompt: String,
    },
    UseBuiltInSystemPrompt,
    SetBuiltInSystemPromptProfile {
        profile: BuiltInSystemPromptProfile,
    },
    UpsertSystemPromptAddition {
        addition: SystemPromptAddition,
    },
    RemoveSystemPromptAddition {
        id: String,
    },
    SetSystemPromptAdditionEnabled {
        id: String,
        enabled: bool,
    },
    ReorderSystemPromptAdditions {
        ids: Vec<String>,
    },
    ClearSystemPromptAdditions,
    SetLocale {
        locale: Locale,
    },
    EnableTool {
        tool_name: BuiltInToolName,
    },
    DisableTool {
        tool_name: BuiltInToolName,
    },
    UpdateApprovalPolicy {
        policy: ApprovalPolicy,
    },
    UpdateFileEditToolPolicy {
        policy: FileEditToolPolicy,
    },
    RegisterPlugin {
        plugin: AgentPluginDeclaration,
    },
    SetPluginEnabled {
        plugin_id: String,
        enabled: bool,
    },
    RemovePlugin {
        plugin_id: String,
    },
    ReservedPhaseProfile {
        description: String,
        #[serde(default)]
        metadata: serde_json::Value,
    },
}

impl ManifestPatch {
    pub fn validate(&self) -> Result<(), ManifestConfigError> {
        match self {
            Self::ReplaceSystemPrompt { prompt } if prompt.trim().is_empty() => Err(
                ManifestConfigError::Invalid("system prompt must not be empty".into()),
            ),
            Self::UpsertSystemPromptAddition { addition } => {
                validate_system_prompt_addition(addition)
            }
            Self::RemoveSystemPromptAddition { id }
            | Self::SetSystemPromptAdditionEnabled { id, .. } => {
                validate_non_empty("system prompt addition id", id)
            }
            Self::ReorderSystemPromptAdditions { ids } => validate_system_prompt_addition_ids(ids),
            Self::ReservedPhaseProfile { .. } => Err(ManifestConfigError::Unsupported(
                "phase profile patches are reserved for a later version".into(),
            )),
            Self::RegisterPlugin { plugin } => plugin
                .validate()
                .map_err(|error| ManifestConfigError::Invalid(error.to_string())),
            Self::SetPluginEnabled { plugin_id, .. } | Self::RemovePlugin { plugin_id } => {
                validate_non_empty("pluginId", plugin_id)
            }
            _ => Ok(()),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::ReplaceSystemPrompt { .. } => "replace system prompt".into(),
            Self::UseBuiltInSystemPrompt => "use built-in system prompt".into(),
            Self::SetBuiltInSystemPromptProfile { profile } => {
                format!("set built-in system prompt profile to {}", profile.as_str())
            }
            Self::UpsertSystemPromptAddition { addition } => {
                format!("upsert system prompt addition {}", addition.id)
            }
            Self::RemoveSystemPromptAddition { id } => {
                format!("remove system prompt addition {id}")
            }
            Self::SetSystemPromptAdditionEnabled { id, enabled } => {
                format!("set system prompt addition {id} enabled={enabled}")
            }
            Self::ReorderSystemPromptAdditions { .. } => "reorder system prompt additions".into(),
            Self::ClearSystemPromptAdditions => "clear system prompt additions".into(),
            Self::SetLocale { locale } => format!("set locale to {}", locale.code()),
            Self::EnableTool { tool_name } => format!("enable tool {}", tool_name.as_str()),
            Self::DisableTool { tool_name } => format!("disable tool {}", tool_name.as_str()),
            Self::UpdateApprovalPolicy { .. } => "update approval policy".into(),
            Self::UpdateFileEditToolPolicy { policy } => {
                format!("update file edit tool policy to {}", policy.as_str())
            }
            Self::RegisterPlugin { plugin } => format!("register {}", plugin.summary()),
            Self::SetPluginEnabled { plugin_id, enabled } => {
                format!("set plugin {plugin_id} enabled={enabled}")
            }
            Self::RemovePlugin { plugin_id } => format!("remove plugin {plugin_id}"),
            Self::ReservedPhaseProfile { description, .. } => {
                format!("reserved phase profile patch: {description}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileEditToolPolicy {
    #[default]
    AutoByModel,
    ApplyPatch,
    WriteFile,
    Disabled,
}

impl FileEditToolPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutoByModel => "auto_by_model",
            Self::ApplyPatch => "apply_patch",
            Self::WriteFile => "write_file",
            Self::Disabled => "disabled",
        }
    }
}

macro_rules! define_built_in_tool_names {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum BuiltInToolName {
            $($variant,)+
        }

        impl BuiltInToolName {
            pub const ALL: &'static [Self] = &[
                $(Self::$variant,)+
            ];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            pub fn parse(value: &str) -> Result<Self, ManifestConfigError> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|tool_name| tool_name.as_str() == value)
                    .ok_or_else(|| ManifestConfigError::UnknownTool(value.into()))
            }
        }
    };
}

define_built_in_tool_names! {
    HostExecStart => "host.exec.start",
    HostExecRead => "host.exec.read",
    HostExecWait => "host.exec.wait",
    HostExecWrite => "host.exec.write",
    HostExecTerminate => "host.exec.terminate",
    HostExecList => "host.exec.list",
    SubagentSpawn => "agent.subagent.spawn",
    SubagentWait => "agent.subagent.wait",
    SubagentResult => "agent.subagent.result",
    SubagentList => "agent.subagent.list",
    GoalUpdate => "agent.goal.update",
    ManifestProposePatch => "agent.manifest.propose_patch",
}

impl BuiltInToolName {
    pub fn default_enabled() -> BTreeSet<Self> {
        Self::ALL.iter().copied().collect()
    }
}

impl std::fmt::Display for BuiltInToolName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for BuiltInToolName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BuiltInToolName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for BuiltInToolName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BuiltInToolName".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::BuiltInToolName").into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        let values = BuiltInToolName::ALL
            .iter()
            .map(|tool_name| tool_name.as_str())
            .collect::<Vec<_>>();
        serde_json::json!({
            "type": "string",
            "enum": values,
        })
        .try_into()
        .expect("built-in tool name schema is valid")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestConfigError {
    Invalid(String),
    UnknownTool(String),
    Unsupported(String),
}

impl std::fmt::Display for ManifestConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) | Self::Unsupported(message) => formatter.write_str(message),
            Self::UnknownTool(tool) => write!(formatter, "unknown built-in tool: {tool}"),
        }
    }
}

impl std::error::Error for ManifestConfigError {}

fn default_enabled() -> bool {
    true
}

fn validate_system_prompt_addition(
    addition: &SystemPromptAddition,
) -> Result<(), ManifestConfigError> {
    validate_non_empty("system prompt addition id", &addition.id)?;
    validate_non_empty("system prompt addition text", &addition.text)
}

fn validate_system_prompt_addition_ids(ids: &[String]) -> Result<(), ManifestConfigError> {
    if ids.is_empty() {
        return Err(ManifestConfigError::Invalid(
            "system prompt addition order must not be empty".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    for id in ids {
        validate_non_empty("system prompt addition id", id)?;
        if !seen.insert(id) {
            return Err(ManifestConfigError::Invalid(format!(
                "duplicate system prompt addition id: {id}"
            )));
        }
    }
    Ok(())
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), ManifestConfigError> {
    if value.trim().is_empty() {
        return Err(ManifestConfigError::Invalid(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

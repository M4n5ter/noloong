use crate::{AgentPluginDeclaration, ApprovalPolicy, Locale};
#[cfg(feature = "json-schema")]
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentManifest {
    pub locale: Locale,
    #[serde(default)]
    pub system_prompt: AgentSystemPrompt,
    #[serde(default = "BuiltInToolName::default_enabled")]
    pub enabled_tools: BTreeSet<BuiltInToolName>,
    #[serde(default)]
    pub file_edit_tool_policy: FileEditToolPolicy,
    pub approval_policy: ApprovalPolicy,
    #[serde(default)]
    pub plugins: BTreeMap<String, AgentPluginDeclaration>,
    #[serde(default)]
    pub reserved_phase_profile: BTreeMap<String, serde_json::Value>,
}

impl AgentManifest {
    pub fn new(system_prompt: AgentSystemPrompt) -> Self {
        Self {
            locale: Locale::En,
            system_prompt,
            enabled_tools: BuiltInToolName::default_enabled(),
            file_edit_tool_policy: FileEditToolPolicy::default(),
            approval_policy: ApprovalPolicy::RequireApproval,
            plugins: BTreeMap::new(),
            reserved_phase_profile: BTreeMap::new(),
        }
    }

    pub fn with_enabled_tool(mut self, tool_name: BuiltInToolName) -> Self {
        self.enabled_tools.insert(tool_name);
        self
    }

    pub fn with_file_edit_tool_policy(mut self, policy: FileEditToolPolicy) -> Self {
        self.file_edit_tool_policy = policy;
        self
    }

    pub fn effective_system_prompt(&self) -> String {
        self.system_prompt.effective_text(self.locale, None)
    }

    pub fn with_plugin(mut self, plugin: AgentPluginDeclaration) -> Result<Self, ManifestError> {
        self.register_plugin(plugin)?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        self.system_prompt.validate()?;
        for (plugin_id, plugin) in &self.plugins {
            if plugin_id != &plugin.plugin_id {
                return Err(ManifestError::Invalid(format!(
                    "plugin map key `{plugin_id}` does not match pluginId `{}`",
                    plugin.plugin_id
                )));
            }
            validate_plugin(plugin)?;
        }
        Ok(())
    }

    pub fn apply_patch(&mut self, patch: ManifestPatch) -> Result<(), ManifestError> {
        patch.validate()?;
        match patch {
            ManifestPatch::ReplaceSystemPrompt { prompt } => {
                let additions = self.system_prompt.additions().to_vec();
                self.system_prompt = AgentSystemPrompt::Custom { prompt, additions };
            }
            ManifestPatch::UseBuiltInSystemPrompt => {
                let additions = self.system_prompt.additions().to_vec();
                self.system_prompt = AgentSystemPrompt::BuiltIn {
                    profile: BuiltInSystemPromptProfile::Auto,
                    additions,
                };
            }
            ManifestPatch::SetBuiltInSystemPromptProfile { profile } => {
                let additions = self.system_prompt.additions().to_vec();
                self.system_prompt = AgentSystemPrompt::BuiltIn { profile, additions };
            }
            ManifestPatch::UpsertSystemPromptAddition { addition } => {
                let additions = self.system_prompt.additions_mut();
                match additions.iter_mut().find(|item| item.id == addition.id) {
                    Some(existing) => *existing = addition,
                    None => additions.push(addition),
                }
            }
            ManifestPatch::RemoveSystemPromptAddition { id } => {
                remove_system_prompt_addition(self.system_prompt.additions_mut(), &id)?;
            }
            ManifestPatch::SetSystemPromptAdditionEnabled { id, enabled } => {
                let addition = self
                    .system_prompt
                    .additions_mut()
                    .iter_mut()
                    .find(|addition| addition.id == id)
                    .ok_or_else(|| ManifestError::UnknownSystemPromptAddition(id.clone()))?;
                addition.enabled = enabled;
            }
            ManifestPatch::ReorderSystemPromptAdditions { ids } => {
                reorder_system_prompt_additions(self.system_prompt.additions_mut(), ids)?;
            }
            ManifestPatch::ClearSystemPromptAdditions => {
                self.system_prompt.additions_mut().clear();
            }
            ManifestPatch::SetLocale { locale } => {
                self.locale = locale;
            }
            ManifestPatch::EnableTool { tool_name } => {
                self.enabled_tools.insert(tool_name);
            }
            ManifestPatch::DisableTool { tool_name } => {
                self.enabled_tools.remove(&tool_name);
            }
            ManifestPatch::UpdateApprovalPolicy { policy } => {
                self.approval_policy = policy;
            }
            ManifestPatch::UpdateFileEditToolPolicy { policy } => {
                self.file_edit_tool_policy = policy;
            }
            ManifestPatch::RegisterPlugin { plugin } => {
                self.register_plugin(plugin)?;
            }
            ManifestPatch::SetPluginEnabled { plugin_id, enabled } => {
                let plugin = self
                    .plugins
                    .get_mut(&plugin_id)
                    .ok_or_else(|| ManifestError::UnknownPlugin(plugin_id.clone()))?;
                plugin.enabled = enabled;
            }
            ManifestPatch::RemovePlugin { plugin_id } => {
                self.plugins
                    .remove(&plugin_id)
                    .ok_or_else(|| ManifestError::UnknownPlugin(plugin_id.clone()))?;
            }
            ManifestPatch::ReservedPhaseProfile { .. } => {
                return Err(ManifestError::Unsupported(
                    "phase profile patches are reserved for a later version".into(),
                ));
            }
        }
        Ok(())
    }

    fn register_plugin(&mut self, plugin: AgentPluginDeclaration) -> Result<(), ManifestError> {
        validate_plugin(&plugin)?;
        if self.plugins.contains_key(&plugin.plugin_id) {
            return Err(ManifestError::PluginAlreadyExists(plugin.plugin_id));
        }
        self.plugins.insert(plugin.plugin_id.clone(), plugin);
        Ok(())
    }
}

impl Default for AgentManifest {
    fn default() -> Self {
        Self::new(AgentSystemPrompt::BuiltIn {
            profile: BuiltInSystemPromptProfile::Auto,
            additions: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(
    tag = "source",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentSystemPrompt {
    BuiltIn {
        #[serde(default, skip_serializing_if = "BuiltInSystemPromptProfile::is_auto")]
        profile: BuiltInSystemPromptProfile,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        additions: Vec<SystemPromptAddition>,
    },
    Custom {
        prompt: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        additions: Vec<SystemPromptAddition>,
    },
}

impl Default for AgentSystemPrompt {
    fn default() -> Self {
        Self::BuiltIn {
            profile: BuiltInSystemPromptProfile::Auto,
            additions: Vec::new(),
        }
    }
}

impl AgentSystemPrompt {
    pub fn custom(prompt: impl Into<String>) -> Self {
        Self::Custom {
            prompt: prompt.into(),
            additions: Vec::new(),
        }
    }

    pub fn effective_text(
        &self,
        locale: Locale,
        model: Option<&crate::system_prompt::SystemPromptModelContext>,
    ) -> String {
        crate::system_prompt::resolve_system_prompt(locale, self, model).effective_text
    }

    pub const fn source(&self) -> SystemPromptSource {
        match self {
            Self::BuiltIn { .. } => SystemPromptSource::BuiltIn,
            Self::Custom { .. } => SystemPromptSource::Custom,
        }
    }

    pub fn additions(&self) -> &[SystemPromptAddition] {
        match self {
            Self::BuiltIn { additions, .. } | Self::Custom { additions, .. } => additions,
        }
    }

    fn additions_mut(&mut self) -> &mut Vec<SystemPromptAddition> {
        match self {
            Self::BuiltIn { additions, .. } | Self::Custom { additions, .. } => additions,
        }
    }

    fn validate(&self) -> Result<(), ManifestError> {
        match self {
            Self::Custom { prompt, additions } => {
                if prompt.trim().is_empty() {
                    return Err(ManifestError::Invalid(
                        "system prompt must not be empty".into(),
                    ));
                }
                validate_system_prompt_additions(additions)
            }
            Self::BuiltIn { additions, .. } => validate_system_prompt_additions(additions),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SystemPromptSource {
    BuiltIn,
    Custom,
}

impl SystemPromptSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built_in",
            Self::Custom => "custom",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BuiltInSystemPromptProfile {
    #[default]
    Auto,
    General,
    #[serde(rename = "gpt_5_5")]
    Gpt55,
}

impl BuiltInSystemPromptProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::General => "general",
            Self::Gpt55 => "gpt_5_5",
        }
    }

    const fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
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
    pub fn validate(&self) -> Result<(), ManifestError> {
        match self {
            Self::ReplaceSystemPrompt { prompt } if prompt.trim().is_empty() => Err(
                ManifestError::Invalid("system prompt must not be empty".into()),
            ),
            Self::UpsertSystemPromptAddition { addition } => {
                validate_system_prompt_addition(addition)
            }
            Self::RemoveSystemPromptAddition { id }
            | Self::SetSystemPromptAdditionEnabled { id, .. } => {
                validate_non_empty("system prompt addition id", id)
            }
            Self::ReorderSystemPromptAdditions { ids } => validate_system_prompt_addition_ids(ids),
            Self::ReservedPhaseProfile { .. } => Err(ManifestError::Unsupported(
                "phase profile patches are reserved for a later version".into(),
            )),
            Self::RegisterPlugin { plugin } => validate_plugin(plugin),
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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
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

            pub fn parse(value: &str) -> Result<Self, ManifestError> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|tool_name| tool_name.as_str() == value)
                    .ok_or_else(|| ManifestError::UnknownTool(value.into()))
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
    ManifestProposePatch => "agent.manifest.propose_patch",
}

impl std::fmt::Display for BuiltInToolName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl BuiltInToolName {
    pub fn default_enabled() -> BTreeSet<Self> {
        Self::ALL.iter().copied().collect()
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

#[cfg(feature = "json-schema")]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestPatchProposal {
    pub proposal_id: String,
    pub patch: ManifestPatch,
    pub summary: String,
}

#[derive(Clone, Debug, Default)]
pub struct ManifestProposalStore {
    inner: Arc<ManifestProposalStoreInner>,
}

#[derive(Debug, Default)]
struct ManifestProposalStoreInner {
    counter: AtomicU64,
    pending: Mutex<BTreeMap<String, ManifestPatchProposal>>,
    approved: Mutex<Vec<ManifestPatchProposal>>,
}

impl ManifestProposalStore {
    pub fn record_pending_proposal(
        &self,
        patch: ManifestPatch,
    ) -> Result<ManifestPatchProposal, ManifestError> {
        self.record_pending_proposal_with_summary(patch, None)
    }

    pub fn record_pending_proposal_with_summary(
        &self,
        patch: ManifestPatch,
        summary: Option<String>,
    ) -> Result<ManifestPatchProposal, ManifestError> {
        patch.validate()?;
        let proposal_id = format!(
            "manifest-proposal-{}",
            self.inner.counter.fetch_add(1, Ordering::SeqCst) + 1
        );
        let proposal = ManifestPatchProposal {
            proposal_id,
            summary: summary.unwrap_or_else(|| patch.summary()),
            patch,
        };
        self.inner
            .pending
            .lock()
            .expect("manifest proposal store lock poisoned")
            .insert(proposal.proposal_id.clone(), proposal.clone());
        Ok(proposal)
    }

    pub fn approve_proposal(
        &self,
        proposal_id: &str,
    ) -> Result<ManifestPatchProposal, ManifestError> {
        let proposal = self
            .inner
            .pending
            .lock()
            .expect("manifest proposal store lock poisoned")
            .remove(proposal_id)
            .ok_or_else(|| ManifestError::UnknownProposal(proposal_id.into()))?;
        self.inner
            .approved
            .lock()
            .expect("manifest proposal store lock poisoned")
            .push(proposal.clone());
        Ok(proposal)
    }

    pub fn pending_len(&self) -> usize {
        self.inner
            .pending
            .lock()
            .expect("manifest proposal store lock poisoned")
            .len()
    }

    pub fn pending_proposals(&self) -> Vec<ManifestPatchProposal> {
        self.inner
            .pending
            .lock()
            .expect("manifest proposal store lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn drain_approved(&self) -> Vec<ManifestPatchProposal> {
        self.inner
            .approved
            .lock()
            .expect("manifest proposal store lock poisoned")
            .drain(..)
            .collect()
    }

    pub fn approved_len(&self) -> usize {
        self.inner
            .approved
            .lock()
            .expect("manifest proposal store lock poisoned")
            .len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    Invalid(String),
    UnknownTool(String),
    UnknownSystemPromptAddition(String),
    UnknownPlugin(String),
    PluginAlreadyExists(String),
    UnknownProposal(String),
    Unsupported(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid manifest patch: {message}"),
            Self::UnknownTool(tool_name) => write!(formatter, "unknown built-in tool: {tool_name}"),
            Self::UnknownSystemPromptAddition(id) => {
                write!(formatter, "unknown system prompt addition: {id}")
            }
            Self::UnknownPlugin(plugin_id) => write!(formatter, "unknown plugin: {plugin_id}"),
            Self::PluginAlreadyExists(plugin_id) => {
                write!(formatter, "plugin already exists: {plugin_id}")
            }
            Self::UnknownProposal(proposal_id) => {
                write!(formatter, "unknown manifest proposal: {proposal_id}")
            }
            Self::Unsupported(message) => {
                write!(formatter, "unsupported manifest patch: {message}")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

fn validate_plugin(plugin: &AgentPluginDeclaration) -> Result<(), ManifestError> {
    plugin
        .validate()
        .map_err(|error| ManifestError::Invalid(error.to_string()))
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), ManifestError> {
    if value.trim().is_empty() {
        return Err(ManifestError::Invalid(format!("{field} must not be empty")));
    }
    Ok(())
}

fn default_enabled() -> bool {
    true
}

fn validate_system_prompt_additions(
    additions: &[SystemPromptAddition],
) -> Result<(), ManifestError> {
    let ids = additions
        .iter()
        .map(|addition| {
            validate_system_prompt_addition(addition)?;
            Ok(addition.id.as_str())
        })
        .collect::<Result<Vec<_>, ManifestError>>()?;
    validate_unique_system_prompt_addition_ids(ids)
}

fn validate_system_prompt_addition(addition: &SystemPromptAddition) -> Result<(), ManifestError> {
    validate_non_empty("system prompt addition id", &addition.id)?;
    validate_non_empty("system prompt addition text", &addition.text)
}

fn validate_system_prompt_addition_ids(ids: &[String]) -> Result<(), ManifestError> {
    let ids = ids
        .iter()
        .map(|id| {
            validate_non_empty("system prompt addition id", id)?;
            Ok(id.as_str())
        })
        .collect::<Result<Vec<_>, ManifestError>>()?;
    validate_unique_system_prompt_addition_ids(ids)
}

fn validate_unique_system_prompt_addition_ids(ids: Vec<&str>) -> Result<(), ManifestError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(ManifestError::Invalid(format!(
                "duplicate system prompt addition id: {id}"
            )));
        }
    }
    Ok(())
}

fn remove_system_prompt_addition(
    additions: &mut Vec<SystemPromptAddition>,
    id: &str,
) -> Result<(), ManifestError> {
    let index = additions
        .iter()
        .position(|addition| addition.id == id)
        .ok_or_else(|| ManifestError::UnknownSystemPromptAddition(id.into()))?;
    additions.remove(index);
    Ok(())
}

fn reorder_system_prompt_additions(
    additions: &mut Vec<SystemPromptAddition>,
    ids: Vec<String>,
) -> Result<(), ManifestError> {
    let current_ids = additions
        .iter()
        .map(|addition| addition.id.as_str())
        .collect::<BTreeSet<_>>();
    let requested_ids = ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    if current_ids != requested_ids {
        return Err(ManifestError::Invalid(
            "system prompt addition reorder ids must match current addition ids".into(),
        ));
    }

    let mut by_id = additions
        .drain(..)
        .map(|addition| (addition.id.clone(), addition))
        .collect::<BTreeMap<_, _>>();
    for id in ids {
        let addition = by_id
            .remove(&id)
            .ok_or_else(|| ManifestError::UnknownSystemPromptAddition(id.clone()))?;
        additions.push(addition);
    }
    Ok(())
}

use crate::{ApprovalPolicy, Locale};
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
    pub system_prompt: String,
    #[serde(default)]
    pub enabled_tools: BTreeSet<BuiltInToolName>,
    #[serde(default)]
    pub file_edit_tool_policy: FileEditToolPolicy,
    pub approval_policy: ApprovalPolicy,
    #[serde(default)]
    pub reserved_phase_profile: BTreeMap<String, serde_json::Value>,
}

impl AgentManifest {
    pub fn new(system_prompt: impl Into<String>) -> Self {
        Self {
            locale: Locale::En,
            system_prompt: system_prompt.into(),
            enabled_tools: BTreeSet::new(),
            file_edit_tool_policy: FileEditToolPolicy::default(),
            approval_policy: ApprovalPolicy::RequireApproval,
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

    pub fn apply_patch(&mut self, patch: ManifestPatch) -> Result<(), ManifestError> {
        patch.validate()?;
        match patch {
            ManifestPatch::ReplaceSystemPrompt { prompt } => {
                self.system_prompt = prompt;
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
            ManifestPatch::ReservedPhaseProfile { .. } => {
                return Err(ManifestError::Unsupported(
                    "phase profile patches are reserved for a later version".into(),
                ));
            }
        }
        Ok(())
    }
}

impl Default for AgentManifest {
    fn default() -> Self {
        Self::new("You are Noloong, a host-first evolvable AI agent.")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ManifestPatch {
    ReplaceSystemPrompt {
        prompt: String,
    },
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
            Self::ReservedPhaseProfile { .. } => Err(ManifestError::Unsupported(
                "phase profile patches are reserved for a later version".into(),
            )),
            _ => Ok(()),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::ReplaceSystemPrompt { .. } => "replace system prompt".into(),
            Self::SetLocale { locale } => format!("set locale to {}", locale.code()),
            Self::EnableTool { tool_name } => format!("enable tool {}", tool_name.as_str()),
            Self::DisableTool { tool_name } => format!("disable tool {}", tool_name.as_str()),
            Self::UpdateApprovalPolicy { .. } => "update approval policy".into(),
            Self::UpdateFileEditToolPolicy { policy } => {
                format!("update file edit tool policy to {}", policy.as_str())
            }
            Self::ReservedPhaseProfile { description, .. } => {
                format!("reserved phase profile patch: {description}")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BuiltInToolName {
    HostExecStart,
    HostExecRead,
    HostExecWait,
    HostExecWrite,
    HostExecTerminate,
    HostExecList,
    ManifestProposePatch,
}

impl BuiltInToolName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HostExecStart => "host.exec.start",
            Self::HostExecRead => "host.exec.read",
            Self::HostExecWait => "host.exec.wait",
            Self::HostExecWrite => "host.exec.write",
            Self::HostExecTerminate => "host.exec.terminate",
            Self::HostExecList => "host.exec.list",
            Self::ManifestProposePatch => "agent.manifest.propose_patch",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ManifestError> {
        match value {
            "host.exec.start" => Ok(Self::HostExecStart),
            "host.exec.read" => Ok(Self::HostExecRead),
            "host.exec.wait" => Ok(Self::HostExecWait),
            "host.exec.write" => Ok(Self::HostExecWrite),
            "host.exec.terminate" => Ok(Self::HostExecTerminate),
            "host.exec.list" => Ok(Self::HostExecList),
            "agent.manifest.propose_patch" => Ok(Self::ManifestProposePatch),
            other => Err(ManifestError::UnknownTool(other.into())),
        }
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
    UnknownProposal(String),
    Unsupported(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid manifest patch: {message}"),
            Self::UnknownTool(tool_name) => write!(formatter, "unknown built-in tool: {tool_name}"),
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

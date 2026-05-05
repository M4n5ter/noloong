use crate::{
    APPLY_PATCH_TOOL_NAME, BuiltInToolName, FILE_EDIT_PERMISSION_CAPABILITY, StartCommandRequest,
    WRITE_FILE_TOOL_NAME, tools::apply_patch_target_paths,
};
use noloong_agent_core::{ToolCall, ToolPermissionDecision};
use serde_json::{Map, Value, json};

use super::{
    cache::ApprovalCacheKey,
    command_safety::{CommandSafety, classify_host_command},
    constants::{
        APPROVAL_CACHE_KEY_METADATA, CLASSIFICATION_DECISION_METADATA,
        CLASSIFICATION_REASON_METADATA, CLASSIFICATION_SOURCE_METADATA,
    },
    decisions::{allow_decision, deny_decision},
    metadata::tool_metadata,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ApprovalClass {
    Allow,
    NeedsApproval,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ApprovalClassification {
    decision: ApprovalClass,
    source: &'static str,
    reason: &'static str,
    cache_key: Option<ApprovalCacheKey>,
    extra_metadata: Map<String, Value>,
}

impl ApprovalClassification {
    pub(super) fn allow(
        source: &'static str,
        reason: &'static str,
        cache_key: Option<ApprovalCacheKey>,
    ) -> Self {
        Self {
            decision: ApprovalClass::Allow,
            source,
            reason,
            cache_key,
            extra_metadata: Map::new(),
        }
    }

    pub(super) fn needs_approval(
        source: &'static str,
        reason: &'static str,
        cache_key: Option<ApprovalCacheKey>,
    ) -> Self {
        Self {
            decision: ApprovalClass::NeedsApproval,
            source,
            reason,
            cache_key,
            extra_metadata: Map::new(),
        }
    }

    pub(super) fn deny(
        source: &'static str,
        reason: &'static str,
        cache_key: Option<ApprovalCacheKey>,
    ) -> Self {
        Self {
            decision: ApprovalClass::Deny,
            source,
            reason,
            cache_key,
            extra_metadata: Map::new(),
        }
    }

    pub(super) fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra_metadata.insert(key.into(), value);
        self
    }

    pub(super) fn terminal_decision(
        &self,
        default_reason: &'static str,
        tool_call: &ToolCall,
    ) -> Option<ToolPermissionDecision> {
        match self.decision {
            ApprovalClass::Allow => Some(self.allow_decision(default_reason, tool_call)),
            ApprovalClass::Deny => Some(self.deny_decision(tool_call)),
            ApprovalClass::NeedsApproval => None,
        }
    }

    fn allow_decision(
        &self,
        default_reason: &'static str,
        tool_call: &ToolCall,
    ) -> ToolPermissionDecision {
        allow_decision(
            default_reason,
            "policy",
            self.metadata(tool_metadata(tool_call)),
        )
    }

    fn deny_decision(&self, tool_call: &ToolCall) -> ToolPermissionDecision {
        deny_decision(
            self.reason,
            "policy",
            self.metadata(tool_metadata(tool_call)),
        )
    }

    pub(super) fn metadata(&self, mut base: Value) -> Value {
        let Value::Object(ref mut map) = base else {
            return base;
        };
        map.insert(CLASSIFICATION_SOURCE_METADATA.into(), json!(self.source));
        map.insert(CLASSIFICATION_REASON_METADATA.into(), json!(self.reason));
        map.insert(
            CLASSIFICATION_DECISION_METADATA.into(),
            json!(self.decision.as_str()),
        );
        if let Some(cache_key) = &self.cache_key {
            map.insert(
                APPROVAL_CACHE_KEY_METADATA.into(),
                json!(cache_key.as_str()),
            );
        }
        for (key, value) in &self.extra_metadata {
            map.insert(key.clone(), value.clone());
        }
        base
    }
}

impl ApprovalClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::NeedsApproval => "needs_approval",
            Self::Deny => "deny",
        }
    }
}

pub(super) fn classify_built_in_tool(
    tool_name: BuiltInToolName,
    cache_key: Option<ApprovalCacheKey>,
) -> ApprovalClassification {
    match tool_name {
        BuiltInToolName::HostExecRead
        | BuiltInToolName::HostExecWait
        | BuiltInToolName::HostExecList => ApprovalClassification::allow(
            "built_in_tool",
            "read-only host command lifecycle operation",
            cache_key,
        ),
        BuiltInToolName::HostExecWrite | BuiltInToolName::HostExecTerminate => {
            ApprovalClassification::needs_approval(
                "built_in_tool",
                "host command control operation requires approval",
                cache_key,
            )
        }
        BuiltInToolName::ManifestProposePatch => ApprovalClassification::needs_approval(
            "built_in_tool",
            "manifest changes require approval",
            cache_key,
        ),
        BuiltInToolName::HostExecStart => {
            unreachable!("host.exec.start is classified before generic built-in tools")
        }
    }
}

pub(super) fn classify_file_edit_tool(tool_call: &ToolCall) -> Option<ApprovalClassification> {
    if tool_call.name != WRITE_FILE_TOOL_NAME && tool_call.name != APPLY_PATCH_TOOL_NAME {
        return None;
    }
    let mut classification =
        ApprovalClassification::needs_approval("file_edit", "file edits require approval", None)
            .with_metadata("capability", json!(FILE_EDIT_PERMISSION_CAPABILITY))
            .with_metadata("builtIn", json!(true))
            .with_metadata("tool", json!(tool_call.name));
    if let Some(paths) = file_edit_target_paths(tool_call) {
        classification = classification.with_metadata("targetPaths", json!(paths));
    }
    Some(classification)
}

pub(super) fn classify_host_exec_start(
    input: Option<&StartCommandRequest>,
    cache_key: Option<ApprovalCacheKey>,
) -> ApprovalClassification {
    let Some(input) = input else {
        return ApprovalClassification::needs_approval(
            "host_command",
            "host command arguments could not be classified",
            cache_key,
        );
    };
    if input.command.trim().is_empty() {
        return ApprovalClassification::deny("host_command", "host command is empty", cache_key);
    }
    match classify_host_command(&input.command, input.shell.as_deref()) {
        CommandSafety::Safe => ApprovalClassification::allow(
            "host_command",
            "known safe read-only host command",
            cache_key,
        ),
        CommandSafety::Dangerous => ApprovalClassification::needs_approval(
            "host_command",
            "dangerous host command requires approval",
            cache_key,
        ),
        CommandSafety::Unknown => ApprovalClassification::needs_approval(
            "host_command",
            "unknown host command requires approval",
            cache_key,
        ),
    }
}

fn file_edit_target_paths(tool_call: &ToolCall) -> Option<Vec<String>> {
    match tool_call.name.as_str() {
        WRITE_FILE_TOOL_NAME => tool_call
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .map(|path| vec![path.to_owned()]),
        APPLY_PATCH_TOOL_NAME => tool_call
            .arguments
            .get("patch")
            .and_then(Value::as_str)
            .and_then(apply_patch_target_paths)
            .filter(|paths| !paths.is_empty()),
        _ => None,
    }
}

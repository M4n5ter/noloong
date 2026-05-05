use crate::{BuiltInToolName, StartCommandRequest};
use noloong_agent_core::{
    ToolApprovalRequest, ToolCall, ToolPermissionDecision, ToolPermissionOutcome,
};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

use super::constants::{APPROVAL_CACHE_KEY_METADATA, BUILT_IN_APPROVAL_HOOK_ID};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ApprovalCacheKey(String);

impl ApprovalCacheKey {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn from_json(value: Value) -> Option<Self> {
        serde_json::to_string(&canonical_json(&value))
            .ok()
            .map(Self)
    }

    fn from_metadata(value: &Value) -> Option<Self> {
        value.as_str().map(|value| Self(value.to_owned()))
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ApprovalCache {
    keys: Arc<Mutex<HashSet<ApprovalCacheKey>>>,
}

impl ApprovalCache {
    pub(crate) fn contains(&self, key: &ApprovalCacheKey) -> bool {
        self.keys
            .lock()
            .expect("approval cache lock poisoned")
            .contains(key)
    }

    pub(crate) fn insert(&self, key: ApprovalCacheKey) -> bool {
        self.keys
            .lock()
            .expect("approval cache lock poisoned")
            .insert(key)
    }
}

pub(super) struct HostExecStartApprovalInput {
    pub(super) input: StartCommandRequest,
    pub(super) cache_key: Option<ApprovalCacheKey>,
}

pub(super) fn host_exec_start_approval_input(
    tool_call: &ToolCall,
) -> Option<HostExecStartApprovalInput> {
    let input = parse_start_command_request(&tool_call.arguments)?;
    let cache_key = host_exec_start_cache_key(&input);
    Some(HostExecStartApprovalInput { input, cache_key })
}

pub(super) fn approval_cache_key_for_tool_call(tool_call: &ToolCall) -> Option<ApprovalCacheKey> {
    let tool_name = BuiltInToolName::parse(&tool_call.name).ok()?;
    match tool_name {
        BuiltInToolName::HostExecStart => {
            host_exec_start_approval_input(tool_call).and_then(|parsed| parsed.cache_key)
        }
        BuiltInToolName::HostExecWrite | BuiltInToolName::HostExecTerminate => {
            ApprovalCacheKey::from_json(json!({
            "tool": tool_name.as_str(),
            "jobId": tool_call.arguments.get("jobId").and_then(Value::as_str)?,
            }))
        }
        BuiltInToolName::ManifestProposePatch => None,
        BuiltInToolName::HostExecRead
        | BuiltInToolName::HostExecWait
        | BuiltInToolName::HostExecList => None,
    }
}

fn host_exec_start_cache_key(input: &StartCommandRequest) -> Option<ApprovalCacheKey> {
    ApprovalCacheKey::from_json(json!({
        "tool": BuiltInToolName::HostExecStart.as_str(),
        "command": &input.command,
        "shell": &input.shell,
        "cwd": &input.cwd,
        "env": &input.env,
        "pipeStdin": input.pipe_stdin,
        "foregroundWaitMs": input.foreground_wait_ms,
        "maxSpoolBytes": input.max_spool_bytes,
    }))
}

pub(crate) fn cache_key_from_approval_resolution(
    approval: &ToolApprovalRequest,
    decision: &ToolPermissionDecision,
) -> Option<ApprovalCacheKey> {
    if approval.hook_id.as_deref() != Some(BUILT_IN_APPROVAL_HOOK_ID)
        || decision.outcome != ToolPermissionOutcome::Allow
    {
        return None;
    }
    approval
        .request
        .metadata
        .get(APPROVAL_CACHE_KEY_METADATA)
        .and_then(ApprovalCacheKey::from_metadata)
}

fn parse_start_command_request(value: &Value) -> Option<StartCommandRequest> {
    serde_json::from_value(value.clone()).ok()
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        Value::Object(map) => {
            let sorted = map
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect::<BTreeMap<_, _>>();
            let mut canonical = Map::new();
            for (key, value) in sorted {
                canonical.insert(key, value);
            }
            Value::Object(canonical)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use noloong_agent_core::ToolApprovalRequestSpec;

    #[test]
    fn approval_cache_key_is_stable_and_sensitive_to_command_shape() {
        let first = start_call(json!({
            "command": "python -c 'print(1)'",
            "shell": "sh",
            "cwd": ".",
            "env": {
                "B": null,
                "A": "1"
            },
            "pipeStdin": false,
            "foregroundWaitMs": 1000,
            "maxSpoolBytes": 4096
        }));
        let reordered = start_call(json!({
            "maxSpoolBytes": 4096,
            "foregroundWaitMs": 1000,
            "pipeStdin": false,
            "env": {
                "A": "1",
                "B": null
            },
            "cwd": ".",
            "shell": "sh",
            "command": "python -c 'print(1)'"
        }));
        let changed = start_call(json!({
            "command": "python -c 'print(2)'",
            "shell": "sh",
            "cwd": ".",
            "env": {
                "A": "1",
                "B": null
            },
            "pipeStdin": false,
            "foregroundWaitMs": 1000,
            "maxSpoolBytes": 4096
        }));

        assert_eq!(
            approval_cache_key_for_tool_call(&first)
                .as_ref()
                .map(ApprovalCacheKey::as_str),
            approval_cache_key_for_tool_call(&reordered)
                .as_ref()
                .map(ApprovalCacheKey::as_str)
        );
        assert_ne!(
            approval_cache_key_for_tool_call(&first)
                .as_ref()
                .map(ApprovalCacheKey::as_str),
            approval_cache_key_for_tool_call(&changed)
                .as_ref()
                .map(ApprovalCacheKey::as_str)
        );
    }

    #[test]
    fn cache_key_from_resolution_requires_built_in_allow_with_metadata() {
        let tool_call = start_call(json!({
            "command": "python -c 'print(1)'",
            "shell": "sh"
        }));
        let cache_key = approval_cache_key_for_tool_call(&tool_call).expect("cache key exists");
        let approval = approval_request(
            tool_call,
            Some(BUILT_IN_APPROVAL_HOOK_ID),
            cache_key.as_str(),
        );
        let allow = ToolPermissionDecision {
            outcome: ToolPermissionOutcome::Allow,
            reason: Some("approved".into()),
            approver: Some("human".into()),
            metadata: json!({}),
        };
        let deny = ToolPermissionDecision {
            outcome: ToolPermissionOutcome::Deny,
            reason: Some("denied".into()),
            approver: Some("human".into()),
            metadata: json!({}),
        };

        assert_eq!(
            cache_key_from_approval_resolution(&approval, &allow)
                .as_ref()
                .map(ApprovalCacheKey::as_str),
            Some(cache_key.as_str())
        );
        assert!(cache_key_from_approval_resolution(&approval, &deny).is_none());

        let external = ToolApprovalRequest {
            hook_id: Some("other.hook".into()),
            ..approval.clone()
        };
        assert!(cache_key_from_approval_resolution(&external, &allow).is_none());

        let missing_metadata = ToolApprovalRequest {
            request: ToolApprovalRequestSpec {
                metadata: json!({}),
                ..approval.request.clone()
            },
            ..approval
        };
        assert!(cache_key_from_approval_resolution(&missing_metadata, &allow).is_none());
    }

    fn start_call(arguments: Value) -> ToolCall {
        ToolCall {
            id: "tool-call-test".into(),
            name: BuiltInToolName::HostExecStart.as_str().into(),
            arguments,
        }
    }

    fn approval_request(
        tool_call: ToolCall,
        hook_id: Option<&str>,
        cache_key: &str,
    ) -> ToolApprovalRequest {
        let mut metadata = Map::new();
        metadata.insert(APPROVAL_CACHE_KEY_METADATA.into(), json!(cache_key));
        ToolApprovalRequest {
            approval_id: "approval-test".into(),
            tool_call,
            permissions: Vec::new(),
            hook_id: hook_id.map(str::to_owned),
            request: ToolApprovalRequestSpec {
                prompt: None,
                reason: None,
                expires_at_ms: None,
                metadata: Value::Object(metadata),
            },
        }
    }
}

use noloong_config::ManifestPatch;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

mod http_client;
mod media;
mod ws;

pub use http_client::{
    AppInteractionClient, AppInteractionError, AppInteractionHttpClient,
    initialize_interaction_status, interaction_http_url,
};
pub use media::{AppMediaBlock, AppMediaKind, AppMediaSource};
pub use ws::{AppInteractionWsClient, AppInteractionWsNotification};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInteractionEndpoint {
    pub ws_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AppInteractionStatus {
    Unavailable,
    Pending,
    Ready {
        server_name: String,
        protocol_version: String,
        profiles: Vec<InteractionProfileDescriptor>,
    },
    Failed {
        error: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionInitializeRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub requested_authority: BTreeSet<InteractionAuthorityCapability>,
    #[serde(default)]
    pub requested_ux: InteractionUxCapabilities,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl InteractionInitializeRequest {
    pub fn noloong_app() -> Self {
        Self {
            name: "noloong-app".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            requested_authority: [
                InteractionAuthorityCapability::AgentRun,
                InteractionAuthorityCapability::ApprovalResolve,
                InteractionAuthorityCapability::SessionDelete,
            ]
            .into_iter()
            .collect(),
            requested_ux: InteractionUxCapabilities {
                display_events: true,
                stream_text: true,
                edit_message: true,
                markdown: true,
                max_message_bytes: None,
            },
            metadata: Map::new(),
        }
    }
}

#[derive(
    Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
pub enum InteractionAuthorityCapability {
    #[serde(rename = "agent.run")]
    AgentRun,
    #[serde(rename = "approval.resolve")]
    ApprovalResolve,
    #[serde(rename = "session.delete")]
    SessionDelete,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionUxCapabilities {
    #[serde(default)]
    pub display_events: bool,
    #[serde(default)]
    pub stream_text: bool,
    #[serde(default)]
    pub edit_message: bool,
    #[serde(default)]
    pub markdown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_message_bytes: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionInitializeResult {
    pub server: InteractionServerInfo,
    #[serde(default)]
    pub profiles: Vec<InteractionProfileDescriptor>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionServerInfo {
    pub name: String,
    pub protocol_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionProfileDescriptor {
    pub profile_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub default_manifest_patches: Vec<ManifestPatch>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionMetadataUpdateRequest {
    pub session_id: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppApprovalResolveRequest {
    pub session_id: String,
    pub approval_id: String,
    pub decision: AppToolPermissionDecision,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppPromptRequest {
    pub session_id: String,
    pub input: AppPromptInput,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AppPromptInput {
    Text { text: String },
    Message { message: AppMessage },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInteractionSessionDescriptor {
    pub session_id: String,
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub status: AppInteractionSessionStatus,
    pub state: AppInteractionSessionState,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppInteractionSessionStatus {
    Idle,
    Running,
    Completed,
    Aborted,
    Failed,
    Paused,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInteractionSessionState {
    #[serde(default)]
    pub messages: Vec<AppMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppMessage {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub content: Vec<AppContentBlock>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppContentBlock {
    Thinking {
        thinking: AppThinkingBlock,
    },
    Media {
        media: AppMediaBlock,
    },
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppThinkingBlock {
    #[serde(default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AppDisplayEvent {
    RunStarted {
        run_id: String,
    },
    RunCompleted {
        run_id: String,
    },
    RunAborted {
        run_id: String,
    },
    RunFailed {
        run_id: String,
        error: String,
    },
    RunPaused {
        run_id: String,
        reason: Value,
    },
    ThoughtStarted {
        run_id: String,
        thought_id: String,
    },
    ThoughtDelta {
        run_id: String,
        thought_id: String,
        kind: String,
        text: String,
    },
    ThoughtCompleted {
        run_id: String,
        thought_id: String,
        elapsed_ms: u64,
    },
    AssistantMessageDelta {
        run_id: String,
        display_message_id: String,
        text: String,
    },
    AssistantMessageFinal {
        run_id: String,
        display_message_id: String,
        message: AppMessage,
        #[serde(default)]
        truncated: bool,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
    },
    ToolUpdated {
        tool_call_id: String,
        update: AppToolUpdate,
    },
    ToolCompleted {
        tool_call_id: String,
        output: AppToolOutput,
    },
    ApprovalRequested {
        approval: AppToolApprovalRequest,
    },
    ApprovalResolved {
        approval_id: String,
        decision: AppToolPermissionDecision,
    },
    ApprovalExpired {
        approval_id: String,
        decision: AppToolPermissionDecision,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppToolUpdate {
    #[serde(default)]
    pub content: Vec<AppContentBlock>,
    #[serde(default)]
    pub details: Value,
}

impl AppToolUpdate {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![AppContentBlock::Text { text: text.into() }],
            details: Value::Null,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppToolOutput {
    #[serde(default)]
    pub content: Vec<AppContentBlock>,
    #[serde(default)]
    pub details: Value,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub updates: Vec<AppToolUpdate>,
}

impl AppToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![AppContentBlock::Text { text: text.into() }],
            details: Value::Null,
            is_error: false,
            updates: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![AppContentBlock::Text { text: text.into() }],
            details: Value::Null,
            is_error: true,
            updates: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppToolPermissionRequirement {
    pub capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppToolApprovalRequestSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppToolApprovalRequest {
    pub approval_id: String,
    pub tool_call: AppToolCall,
    #[serde(default)]
    pub permissions: Vec<AppToolPermissionRequirement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_id: Option<String>,
    pub request: AppToolApprovalRequestSpec,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppToolPermissionOutcome {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppToolPermissionDecision {
    pub outcome: AppToolPermissionOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approver: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

impl AppToolPermissionDecision {
    pub fn from_outcome(outcome: AppToolPermissionOutcome) -> Self {
        Self {
            outcome,
            reason: Some("Resolved from noloong app".into()),
            approver: Some("noloong-app".into()),
            metadata: Value::Null,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppDisplaySubscribeRequest {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ux: Option<InteractionUxCapabilities>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSubscriptionResult {
    pub subscription_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInteractionDisplayNotification {
    pub session_id: String,
    pub subscription_id: String,
    pub event: AppDisplayEvent,
}

#[cfg(test)]
mod tests {
    use super::{
        AppApprovalResolveRequest, AppDisplayEvent, AppDisplaySubscribeRequest,
        AppInteractionDisplayNotification, AppToolPermissionDecision, AppToolPermissionOutcome,
        InteractionAuthorityCapability, InteractionInitializeRequest, InteractionUxCapabilities,
        interaction_http_url,
    };
    use serde_json::json;

    #[test]
    fn interaction_http_url_derives_jsonrpc_post_endpoint_from_ws_endpoint() {
        assert_eq!(
            interaction_http_url("ws://127.0.0.1:8787/jsonrpc/ws").unwrap(),
            "http://127.0.0.1:8787/jsonrpc"
        );
        assert_eq!(
            interaction_http_url("wss://noloong.example/jsonrpc/ws").unwrap(),
            "https://noloong.example/jsonrpc"
        );
    }

    #[test]
    fn noloong_app_initialize_request_asks_for_chat_authority_and_display_ux() {
        let request = InteractionInitializeRequest::noloong_app();

        assert!(
            request
                .requested_authority
                .contains(&InteractionAuthorityCapability::AgentRun)
        );
        assert!(
            request
                .requested_authority
                .contains(&InteractionAuthorityCapability::ApprovalResolve)
        );
        assert!(request.requested_ux.display_events);
        assert!(request.requested_ux.stream_text);
        assert!(request.requested_ux.markdown);
    }

    #[test]
    fn display_notification_decodes_assistant_delta() {
        let notification = serde_json::from_value::<AppInteractionDisplayNotification>(json!({
            "sessionId": "session-1",
            "subscriptionId": "subscription-1",
            "event": {
                "type": "assistant_message_delta",
                "runId": "run-1",
                "displayMessageId": "run-1:assistant",
                "text": "hello"
            }
        }))
        .unwrap();

        assert_eq!(notification.session_id, "session-1");
        assert_eq!(
            notification.event,
            AppDisplayEvent::AssistantMessageDelta {
                run_id: "run-1".into(),
                display_message_id: "run-1:assistant".into(),
                text: "hello".into(),
            }
        );
    }

    #[test]
    fn display_notification_decodes_thought_delta() {
        let notification = serde_json::from_value::<AppInteractionDisplayNotification>(json!({
            "sessionId": "session-1",
            "subscriptionId": "subscription-1",
            "event": {
                "type": "thought_delta",
                "runId": "run-1",
                "thoughtId": "run-1:thought",
                "kind": "summary",
                "text": "summary"
            }
        }))
        .unwrap();

        assert_eq!(
            notification.event,
            AppDisplayEvent::ThoughtDelta {
                run_id: "run-1".into(),
                thought_id: "run-1:thought".into(),
                kind: "summary".into(),
                text: "summary".into(),
            }
        );
    }

    #[test]
    fn display_notification_decodes_run_aborted() {
        let notification = serde_json::from_value::<AppInteractionDisplayNotification>(json!({
            "sessionId": "session-1",
            "subscriptionId": "subscription-1",
            "event": {
                "type": "run_aborted",
                "runId": "run-1"
            }
        }))
        .unwrap();

        assert_eq!(
            notification.event,
            AppDisplayEvent::RunAborted {
                run_id: "run-1".into(),
            }
        );
    }

    #[test]
    fn display_notification_decodes_tool_and_approval_events() {
        let tool = serde_json::from_value::<AppInteractionDisplayNotification>(json!({
            "sessionId": "session-1",
            "subscriptionId": "subscription-1",
            "event": {
                "type": "tool_completed",
                "toolCallId": "call-1",
                "output": {
                    "content": [{"type": "text", "text": "done"}],
                    "isError": false
                }
            }
        }))
        .unwrap();
        assert!(matches!(
            tool.event,
            AppDisplayEvent::ToolCompleted {
                ref tool_call_id,
                ..
            } if tool_call_id == "call-1"
        ));

        let approval = serde_json::from_value::<AppInteractionDisplayNotification>(json!({
            "sessionId": "session-1",
            "subscriptionId": "subscription-1",
            "event": {
                "type": "approval_requested",
                "approval": {
                    "approvalId": "approval-1",
                    "toolCall": {
                        "id": "call-1",
                        "name": "host.exec.start",
                        "arguments": {"command": "echo ok"}
                    },
                    "permissions": [],
                    "request": {
                        "prompt": "Approve command?",
                        "metadata": {}
                    }
                }
            }
        }))
        .unwrap();
        assert!(matches!(
            approval.event,
            AppDisplayEvent::ApprovalRequested { ref approval }
                if approval.approval_id == "approval-1"
                    && approval.tool_call.name == "host.exec.start"
        ));
    }

    #[test]
    fn approval_resolve_request_uses_interaction_protocol_shape() {
        let request = AppApprovalResolveRequest {
            session_id: "session-1".into(),
            approval_id: "approval-1".into(),
            decision: AppToolPermissionDecision::from_outcome(AppToolPermissionOutcome::Deny),
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "sessionId": "session-1",
                "approvalId": "approval-1",
                "decision": {
                    "outcome": "deny",
                    "reason": "Resolved from noloong app",
                    "approver": "noloong-app",
                    "metadata": null
                }
            })
        );
    }

    #[test]
    fn display_subscribe_request_requests_streaming_display_ux() {
        let request = AppDisplaySubscribeRequest {
            session_id: "session-1".into(),
            ux: Some(InteractionUxCapabilities {
                display_events: true,
                stream_text: true,
                edit_message: true,
                markdown: true,
                max_message_bytes: Some(65_536),
            }),
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "sessionId": "session-1",
                "ux": {
                    "displayEvents": true,
                    "streamText": true,
                    "editMessage": true,
                    "markdown": true,
                    "maxMessageBytes": 65536
                }
            })
        );
    }
}

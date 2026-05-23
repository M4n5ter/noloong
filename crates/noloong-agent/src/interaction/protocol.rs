use super::{
    DisplayEvent, InteractionCapabilityGrant, InteractionProfileDescriptor,
    InteractionUxCapabilities,
};
use crate::ReadOutputRequest;
use noloong_agent_core::{
    AgentEvent, AgentInput, AgentMessage, QueueMode, QueuedAgentMessage, QueuedMessageIntent,
    ToolApprovalId, ToolPermissionDecision,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SERVER_NAME: &str = "noloong-agent";
pub const PROTOCOL_VERSION: &str = "2026-05-05";

pub fn request_params<T>(params: T) -> Value
where
    T: Serialize,
{
    serde_json::to_value(params).expect("interaction request params serialize")
}

pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const SHUTDOWN: &str = "shutdown";
    pub const PROFILE_LIST: &str = "profile/list";
    pub const SESSION_CREATE: &str = "session/create";
    pub const SESSION_LIST: &str = "session/list";
    pub const SESSION_GET: &str = "session/get";
    pub const SESSION_DELETE: &str = "session/delete";
    pub const SUBAGENT_SPAWN: &str = "subagent/spawn";
    pub const GOAL_SET: &str = "goal/set";
    pub const GOAL_GET: &str = "goal/get";
    pub const GOAL_PAUSE: &str = "goal/pause";
    pub const GOAL_RESUME: &str = "goal/resume";
    pub const GOAL_CLEAR: &str = "goal/clear";
    pub const GOAL_UPDATE: &str = "goal/update";
    pub const AUTOMATION_CREATE: &str = "automation/create";
    pub const AUTOMATION_GET: &str = "automation/get";
    pub const AUTOMATION_LIST: &str = "automation/list";
    pub const AUTOMATION_UPDATE: &str = "automation/update";
    pub const AUTOMATION_DELETE: &str = "automation/delete";
    pub const AUTOMATION_FIRE: &str = "automation/fire";
    pub const AGENT_PROMPT: &str = "agent/prompt";
    pub const AGENT_CONTINUE: &str = "agent/continue";
    pub const AGENT_ABORT: &str = "agent/abort";
    pub const AGENT_WAIT_IDLE: &str = "agent/wait_idle";
    pub const AGENT_STATE: &str = "agent/state";
    pub const AGENT_STEER: &str = "agent/steer";
    pub const AGENT_FOLLOW_UP: &str = "agent/follow_up";
    pub const QUEUE_LIST: &str = "queue/list";
    pub const QUEUE_EDIT: &str = "queue/edit";
    pub const QUEUE_CLEAR: &str = "queue/clear";
    pub const QUEUE_SET_MODE: &str = "queue/set_mode";
    pub const EVENT_SUBSCRIBE: &str = "event/subscribe";
    pub const EVENT_UNSUBSCRIBE: &str = "event/unsubscribe";
    pub const DISPLAY_SUBSCRIBE: &str = "display/subscribe";
    pub const APPROVAL_LIST: &str = "approval/list";
    pub const APPROVAL_RESOLVE: &str = "approval/resolve";
    pub const APPROVAL_RESUME_TIMEOUTS: &str = "approval/resume_timeouts";
    pub const MANIFEST_GET: &str = "manifest/get";
    pub const MANIFEST_SYSTEM_PROMPT_GET: &str = "manifest/system_prompt/get";
    pub const MANIFEST_PROPOSALS_LIST: &str = "manifest/proposals/list";
    pub const MANIFEST_PROPOSALS_APPROVE: &str = "manifest/proposals/approve";
    pub const MANIFEST_APPLY_APPROVED: &str = "manifest/apply_approved";
    pub const PROCESS_LIST: &str = "process/list";
    pub const PROCESS_READ: &str = "process/read";
    pub const PROCESS_WAIT: &str = "process/wait";
    pub const PROCESS_WRITE: &str = "process/write";
    pub const PROCESS_TERMINATE: &str = "process/terminate";
}

pub mod notification {
    pub const RAW_EVENT: &str = "agent/event";
    pub const DISPLAY_EVENT: &str = "display/event";
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionInitializeResult {
    pub server: InteractionServerInfo,
    pub grant: InteractionCapabilityGrant,
    pub profiles: Vec<InteractionProfileDescriptor>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionServerInfo {
    pub name: String,
    pub protocol_version: String,
}

impl InteractionServerInfo {
    pub fn current() -> Self {
        Self {
            name: SERVER_NAME.into(),
            protocol_version: PROTOCOL_VERSION.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionDeleteRequest {
    pub session_id: String,
    #[serde(default)]
    pub force_abort: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptRequest {
    pub session_id: String,
    pub input: AgentPromptInput,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentPromptInput {
    Text { text: String },
    Message { message: AgentMessage },
}

impl AgentPromptInput {
    pub fn into_agent_input(self) -> AgentInput {
        match self {
            Self::Text { text } => AgentInput::Text(text),
            Self::Message { message } => AgentInput::Message(message),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSteerRequest {
    pub session_id: String,
    pub message: AgentMessage,
    #[serde(default)]
    pub intent: Option<AgentSessionQueuedMessageIntent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentFollowUpRequest {
    pub session_id: String,
    pub message: AgentMessage,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InteractionQueueKind {
    Steering,
    FollowUp,
}

impl InteractionQueueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Steering => "steering",
            Self::FollowUp => "follow_up",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueRequest {
    pub session_id: String,
    pub queue: InteractionQueueKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueEditRequest {
    pub session_id: String,
    pub queue: InteractionQueueKind,
    pub messages: Vec<AgentSessionQueuedMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueSetModeRequest {
    pub session_id: String,
    pub queue: InteractionQueueKind,
    pub mode: QueueMode,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventSubscribeRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DisplaySubscribeRequest {
    pub session_id: String,
    #[serde(default)]
    pub ux: Option<InteractionUxCapabilities>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventUnsubscribeRequest {
    pub subscription_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionResult {
    pub subscription_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeResult {
    pub unsubscribed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RawEventNotification {
    pub session_id: String,
    pub subscription_id: String,
    pub event: AgentEvent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionDisplayNotification {
    pub session_id: String,
    pub subscription_id: String,
    pub event: DisplayEvent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalResolveRequest {
    pub session_id: String,
    pub approval_id: ToolApprovalId,
    pub decision: ToolPermissionDecision,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestProposalRequest {
    pub session_id: String,
    pub proposal_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestApplyResult {
    pub applied_proposal_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessJobRequest {
    pub session_id: String,
    pub job_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessReadRequest {
    pub session_id: String,
    pub job_id: String,
    #[serde(flatten)]
    pub output: ReadOutputRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessWaitRequest {
    pub session_id: String,
    pub job_id: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessWriteRequest {
    pub session_id: String,
    pub job_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionQueuedMessage {
    pub message: AgentMessage,
    #[serde(default)]
    pub intent: AgentSessionQueuedMessageIntent,
}

impl AgentSessionQueuedMessage {
    pub fn from_core(message: QueuedAgentMessage) -> Self {
        Self {
            message: message.message,
            intent: AgentSessionQueuedMessageIntent::from(message.intent),
        }
    }

    pub fn into_core(self) -> QueuedAgentMessage {
        match self.intent {
            AgentSessionQueuedMessageIntent::Observation => {
                QueuedAgentMessage::observation(self.message)
            }
            AgentSessionQueuedMessageIntent::UserInput => {
                QueuedAgentMessage::user_input(self.message)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionQueuedMessageIntent {
    #[default]
    Observation,
    UserInput,
}

impl From<QueuedMessageIntent> for AgentSessionQueuedMessageIntent {
    fn from(value: QueuedMessageIntent) -> Self {
        match value {
            QueuedMessageIntent::Observation => Self::Observation,
            QueuedMessageIntent::UserInput => Self::UserInput,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::{InteractionCapabilityGrant, InteractionProfileDescriptor};
    use noloong_agent_core::AgentMessage;
    use serde_json::json;

    #[test]
    fn agent_prompt_input_round_trips_text_and_message() {
        let text = serde_json::from_value::<AgentPromptInput>(json!({
            "type": "text",
            "text": "hello"
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(text).unwrap(),
            json!({"type": "text", "text": "hello"})
        );

        let message = AgentMessage::user("m1", "hi");
        let value = serde_json::to_value(AgentPromptInput::Message {
            message: message.clone(),
        })
        .unwrap();
        let decoded = serde_json::from_value::<AgentPromptInput>(value).unwrap();
        assert_eq!(decoded, AgentPromptInput::Message { message });
    }

    #[test]
    fn queue_kind_round_trips() {
        let value = serde_json::to_value(InteractionQueueKind::FollowUp).unwrap();
        assert_eq!(value, json!("follow_up"));
        assert_eq!(
            serde_json::from_value::<InteractionQueueKind>(value).unwrap(),
            InteractionQueueKind::FollowUp
        );
    }

    #[test]
    fn process_read_request_round_trips() {
        let request = ProcessReadRequest {
            session_id: "s1".into(),
            job_id: "j1".into(),
            output: ReadOutputRequest {
                after_seq: Some(2),
                max_bytes: Some(1024),
                wait_ms: Some(50),
            },
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["afterSeq"], 2);
        assert_eq!(
            serde_json::from_value::<ProcessReadRequest>(value).unwrap(),
            request
        );
    }

    #[test]
    fn display_subscribe_request_round_trips() {
        let request = DisplaySubscribeRequest {
            session_id: "s1".into(),
            ux: Some(InteractionUxCapabilities {
                display_events: true,
                stream_text: true,
                edit_message: true,
                markdown: true,
                max_message_bytes: Some(4096),
                raw_events: false,
            }),
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["ux"]["displayEvents"], true);
        assert_eq!(
            serde_json::from_value::<DisplaySubscribeRequest>(value).unwrap(),
            request
        );
    }

    #[test]
    fn display_thought_events_round_trip_reasoning_summary_and_completion() {
        let delta = serde_json::from_value::<DisplayEvent>(json!({
            "type": "thought_delta",
            "runId": "run-1",
            "thoughtId": "run-1:thought",
            "kind": "summary",
            "text": "checked the files"
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(delta).unwrap(),
            json!({
                "type": "thought_delta",
                "runId": "run-1",
                "thoughtId": "run-1:thought",
                "kind": "summary",
                "text": "checked the files"
            })
        );

        let completed = serde_json::from_value::<DisplayEvent>(json!({
            "type": "thought_completed",
            "runId": "run-1",
            "thoughtId": "run-1:thought",
            "elapsedMs": 2_000
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(completed).unwrap()["elapsedMs"],
            json!(2_000)
        );
    }

    #[test]
    fn display_run_aborted_round_trips_separately_from_failed() {
        let event = serde_json::from_value::<DisplayEvent>(json!({
            "type": "run_aborted",
            "runId": "run-1"
        }))
        .unwrap();

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "type": "run_aborted",
                "runId": "run-1"
            })
        );
    }

    #[test]
    fn initialize_result_round_trips() {
        let result = InteractionInitializeResult {
            server: InteractionServerInfo {
                name: SERVER_NAME.into(),
                protocol_version: PROTOCOL_VERSION.into(),
            },
            grant: InteractionCapabilityGrant::default(),
            profiles: vec![InteractionProfileDescriptor {
                profile_id: "default".into(),
                display_name: "Default".into(),
                description: None,
                default_manifest_patches: Vec::new(),
                metadata: Default::default(),
            }],
        };
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["server"]["name"], SERVER_NAME);
        assert_eq!(
            serde_json::from_value::<InteractionInitializeResult>(value).unwrap(),
            result
        );
    }
}

mod automation;
#[cfg(feature = "interaction-client")]
mod client;
mod control;
mod error;
mod goal;
#[cfg(feature = "interaction-http")]
mod http;
mod jsonrpc;
mod profile;
pub mod protocol;
mod registry;
mod store;
mod wire;

pub(crate) use automation::AutomationScheduleScanBuilder;
pub use automation::{
    AUTOMATION_SESSION_METADATA_KEY, AUTOMATION_SOURCE_TYPE, AUTOMATION_SYSTEM_PROMPT_ADDITION_ID,
    AutomationPromptInput, AutomationRecord, AutomationScheduleScan, AutomationStatus,
    AutomationTarget, AutomationTimeSchedule, AutomationTrigger, automation_identity_prompt,
    automation_message, automation_session_metadata, existing_session_automation_message,
    session_ready_for_direct_prompt,
};
#[cfg(feature = "interaction-client")]
pub use client::{
    InteractionClientError, InteractionClientResult, InteractionWsClient,
    InteractionWsClientConfig, InteractionWsNotification,
};
pub use control::InteractionControlHandler;
pub use error::{
    INTERACTION_ERROR_BUSY, INTERACTION_ERROR_INTERNAL, INTERACTION_ERROR_INVALID_PARAMS,
    INTERACTION_ERROR_METHOD_NOT_FOUND, INTERACTION_ERROR_NOT_FOUND,
    INTERACTION_ERROR_UNAUTHORIZED, InteractionError,
};
pub use goal::{
    GOAL_AUDIT_MESSAGE_ID_PREFIX, GOAL_AUDIT_REASON_TOOL_UPDATE, GOAL_AUDIT_REASON_TURN_END,
    GOAL_AUDIT_SOURCE_TYPE, GOAL_UPDATE_ALLOWED_STATUS_VALUES, GOAL_UPDATE_STATUS_ERROR,
    GoalAuditRecord, GoalRecord, GoalStatus, goal_audit_message, trim_non_empty,
};
#[cfg(feature = "interaction-http")]
pub use http::{
    InteractionHttpTransportConfig, InteractionTransportAuth, interaction_http_router,
    serve_interaction_http,
};
pub use jsonrpc::{
    InteractionFuture, InteractionNotifier, JsonRpcHandler, JsonRpcHandlerOutput, serve_jsonrpc,
};
pub use profile::AgentRuntimeProfile;
pub use protocol::{AgentSessionQueuedMessage, AgentSessionQueuedMessageIntent};
pub use registry::{
    AgentSessionCreateRequest, AgentSessionDeleteOptions, AgentSessionListFilter,
    AgentSessionRegistry, AgentSessionRegistryOptions, AutomationCreateRequest,
    AutomationListRequest, AutomationRequest, AutomationUpdateRequest, GoalSetRequest,
    GoalStatusUpdateRequest, RegisteredAgentSession, SubagentSpawnRequest,
};
pub(crate) use store::current_unix_ms;
pub use store::{
    AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentSessionQueueSnapshot, AgentSessionQueueState,
    AgentSessionRecord, AgentSessionRegistryStore, InMemoryAgentSessionRegistryStore,
};
#[cfg(feature = "registry-store-object")]
pub use store::{OpenDalAgentSessionRegistryStore, OpenDalAgentSessionRegistryStoreConfig};
#[cfg(any(feature = "registry-store-sqlite", feature = "registry-store-postgres"))]
pub use store::{SqlAgentSessionRegistryStore, SqlAgentSessionRegistryStoreConfig};
pub use wire::{
    DisplayEvent, InteractionAuthorityCapability, InteractionCapabilityGrant,
    InteractionCapabilityPolicy, InteractionClientInfo, InteractionProfileDescriptor,
    InteractionSessionDescriptor, InteractionSessionStatus, InteractionUxCapabilities,
    JsonRpcErrorObject, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    JsonRpcResponsePayload,
};

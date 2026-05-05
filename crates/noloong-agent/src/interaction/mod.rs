mod control;
mod error;
mod jsonrpc;
mod profile;
mod registry;
mod wire;

pub use control::{DISPLAY_EVENT_NOTIFICATION, InteractionControlHandler, RAW_EVENT_NOTIFICATION};
pub use error::{
    INTERACTION_ERROR_BUSY, INTERACTION_ERROR_INTERNAL, INTERACTION_ERROR_INVALID_PARAMS,
    INTERACTION_ERROR_METHOD_NOT_FOUND, INTERACTION_ERROR_NOT_FOUND,
    INTERACTION_ERROR_UNAUTHORIZED, InteractionError,
};
pub use jsonrpc::{
    InteractionFuture, InteractionNotifier, JsonRpcHandler, JsonRpcHandlerOutput, serve_jsonrpc,
};
pub use profile::AgentRuntimeProfile;
pub use registry::{
    AgentSessionCreateRequest, AgentSessionDeleteOptions, AgentSessionListFilter,
    AgentSessionRecord, AgentSessionRegistry, AgentSessionRegistryStore,
    InMemoryAgentSessionRegistryStore, RegisteredAgentSession, SubagentSpawnRequest,
};
pub use wire::{
    DisplayEvent, InteractionAuthorityCapability, InteractionCapabilityGrant,
    InteractionCapabilityPolicy, InteractionClientInfo, InteractionProfileDescriptor,
    InteractionSessionDescriptor, InteractionSessionStatus, InteractionUxCapabilities,
    JsonRpcErrorObject, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    JsonRpcResponsePayload,
};

mod control;
mod error;
mod jsonrpc;
mod profile;
mod registry;
mod store;
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
    AgentSessionRegistry, RegisteredAgentSession, SubagentSpawnRequest,
};
pub use store::{
    AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentSessionQueueSnapshot, AgentSessionQueueState,
    AgentSessionQueuedMessage, AgentSessionQueuedMessageIntent, AgentSessionRecord,
    AgentSessionRegistryStore, InMemoryAgentSessionRegistryStore,
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

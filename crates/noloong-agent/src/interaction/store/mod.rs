#[cfg(any(
    feature = "registry-store-object",
    feature = "registry-store-sqlite",
    feature = "registry-store-postgres"
))]
mod codec;
mod memory;
#[cfg(feature = "registry-store-object")]
mod object;
mod snapshot;
#[cfg(any(feature = "registry-store-sqlite", feature = "registry-store-postgres"))]
mod sql;
mod traits;

pub use memory::InMemoryAgentSessionRegistryStore;
#[cfg(feature = "registry-store-object")]
pub use object::{OpenDalAgentSessionRegistryStore, OpenDalAgentSessionRegistryStoreConfig};
pub use snapshot::{
    AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentSessionQueueSnapshot, AgentSessionQueueState,
    AgentSessionQueuedMessage, AgentSessionQueuedMessageIntent, AgentSessionRecord,
    current_unix_ms,
};
#[cfg(any(feature = "registry-store-sqlite", feature = "registry-store-postgres"))]
pub use sql::{SqlAgentSessionRegistryStore, SqlAgentSessionRegistryStoreConfig};
pub use traits::AgentSessionRegistryStore;
pub(super) use traits::{duplicate_session_error, missing_session_error};

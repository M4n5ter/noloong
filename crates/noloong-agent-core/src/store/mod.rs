mod memory;

#[cfg(feature = "sqlite-store")]
mod sqlite;

use crate::AgentEvent;

pub type StoreFuture<'a, T> = crate::BoxFuture<'a, T>;

pub trait EventStore: Send + Sync {
    fn append<'a>(&'a self, event: AgentEvent) -> StoreFuture<'a, ()>;
    fn load<'a>(&'a self, run_id: &'a str) -> StoreFuture<'a, Vec<AgentEvent>>;
}

pub use memory::InMemoryEventStore;

#[cfg(feature = "sqlite-store")]
pub use sqlite::{SqliteEventStore, SqliteEventStoreConfig};

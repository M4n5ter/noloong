use crate::{AgentEvent, Result, RunId};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};
use tokio::sync::Mutex;

pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait EventStore: Send + Sync {
    fn append<'a>(&'a self, event: AgentEvent) -> StoreFuture<'a, ()>;
    fn load<'a>(&'a self, run_id: &'a str) -> StoreFuture<'a, Vec<AgentEvent>>;
}

#[derive(Clone, Default)]
pub struct InMemoryEventStore {
    events: Arc<Mutex<BTreeMap<RunId, Vec<AgentEvent>>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EventStore for InMemoryEventStore {
    fn append<'a>(&'a self, event: AgentEvent) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let mut events = self.events.lock().await;
            events.entry(event.run_id.clone()).or_default().push(event);
            Ok(())
        })
    }

    fn load<'a>(&'a self, run_id: &'a str) -> StoreFuture<'a, Vec<AgentEvent>> {
        Box::pin(async move {
            let events = self.events.lock().await;
            Ok(events.get(run_id).cloned().unwrap_or_default())
        })
    }
}

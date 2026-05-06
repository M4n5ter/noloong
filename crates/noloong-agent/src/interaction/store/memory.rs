use super::{
    AgentSessionRecord, AgentSessionRegistryStore, duplicate_session_error, missing_session_error,
};
use crate::interaction::InteractionFuture;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub struct InMemoryAgentSessionRegistryStore {
    records: Arc<Mutex<BTreeMap<String, AgentSessionRecord>>>,
}

impl AgentSessionRegistryStore for InMemoryAgentSessionRegistryStore {
    fn insert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let mut records = self
                .records
                .lock()
                .expect("interaction session store lock poisoned");
            if records.contains_key(&record.session_id) {
                return Err(duplicate_session_error(&record.session_id));
            }
            records.insert(record.session_id.clone(), record);
            Ok(())
        })
    }

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let mut records = self
                .records
                .lock()
                .expect("interaction session store lock poisoned");
            if !records.contains_key(&record.session_id) {
                return Err(missing_session_error(&record.session_id));
            }
            records.insert(record.session_id.clone(), record);
            Ok(())
        })
    }

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            self.records
                .lock()
                .expect("interaction session store lock poisoned")
                .remove(session_id);
            Ok(())
        })
    }

    fn get<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<AgentSessionRecord>> {
        Box::pin(async move {
            Ok(self
                .records
                .lock()
                .expect("interaction session store lock poisoned")
                .get(session_id)
                .cloned())
        })
    }

    fn list<'a>(&'a self) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        Box::pin(async move {
            Ok(self
                .records
                .lock()
                .expect("interaction session store lock poisoned")
                .values()
                .cloned()
                .collect())
        })
    }
}

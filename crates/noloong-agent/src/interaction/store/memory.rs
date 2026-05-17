use super::{
    AgentSessionRecord, AgentSessionRegistryStore, AutomationRecord, AutomationScheduleScan,
    AutomationScheduleScanBuilder, GoalRecord, duplicate_automation_error, duplicate_session_error,
    missing_automation_error, missing_session_error, record_matches_session_list_filter,
};
use crate::interaction::{AgentSessionListFilter, InteractionFuture};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub struct InMemoryAgentSessionRegistryStore {
    records: Arc<Mutex<BTreeMap<String, AgentSessionRecord>>>,
    goals: Arc<Mutex<BTreeMap<String, GoalRecord>>>,
    automations: Arc<Mutex<BTreeMap<String, AutomationRecord>>>,
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

    fn list<'a>(
        &'a self,
        filter: &'a AgentSessionListFilter,
    ) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        Box::pin(async move {
            Ok(self
                .records
                .lock()
                .expect("interaction session store lock poisoned")
                .values()
                .filter(|record| record_matches_session_list_filter(record, filter))
                .cloned()
                .collect())
        })
    }

    fn save_goal<'a>(&'a self, goal: GoalRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            self.goals
                .lock()
                .expect("interaction goal store lock poisoned")
                .insert(goal.session_id.clone(), goal);
            Ok(())
        })
    }

    fn get_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<GoalRecord>> {
        Box::pin(async move {
            Ok(self
                .goals
                .lock()
                .expect("interaction goal store lock poisoned")
                .get(session_id)
                .cloned())
        })
    }

    fn list_goals<'a>(&'a self) -> InteractionFuture<'a, Vec<GoalRecord>> {
        Box::pin(async move {
            Ok(self
                .goals
                .lock()
                .expect("interaction goal store lock poisoned")
                .values()
                .cloned()
                .collect())
        })
    }

    fn remove_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            self.goals
                .lock()
                .expect("interaction goal store lock poisoned")
                .remove(session_id);
            Ok(())
        })
    }

    fn insert_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let mut automations = self
                .automations
                .lock()
                .expect("interaction automation store lock poisoned");
            if automations.contains_key(&automation.automation_id) {
                return Err(duplicate_automation_error(&automation.automation_id));
            }
            automations.insert(automation.automation_id.clone(), automation);
            Ok(())
        })
    }

    fn save_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let mut automations = self
                .automations
                .lock()
                .expect("interaction automation store lock poisoned");
            if !automations.contains_key(&automation.automation_id) {
                return Err(missing_automation_error(&automation.automation_id));
            }
            automations.insert(automation.automation_id.clone(), automation);
            Ok(())
        })
    }

    fn get_automation<'a>(
        &'a self,
        automation_id: &'a str,
    ) -> InteractionFuture<'a, Option<AutomationRecord>> {
        Box::pin(async move {
            Ok(self
                .automations
                .lock()
                .expect("interaction automation store lock poisoned")
                .get(automation_id)
                .cloned())
        })
    }

    fn list_automations<'a>(&'a self) -> InteractionFuture<'a, Vec<AutomationRecord>> {
        Box::pin(async move {
            Ok(self
                .automations
                .lock()
                .expect("interaction automation store lock poisoned")
                .values()
                .cloned()
                .collect())
        })
    }

    fn scan_automation_schedule<'a>(
        &'a self,
        now_ms: u64,
    ) -> InteractionFuture<'a, AutomationScheduleScan> {
        Box::pin(async move {
            let automations = self
                .automations
                .lock()
                .expect("interaction automation store lock poisoned");
            let mut scan = AutomationScheduleScanBuilder::default();
            for automation in automations.values() {
                scan.include(
                    automation.automation_id.clone(),
                    automation.is_active(),
                    automation.next_fire_at_ms,
                    now_ms,
                );
            }
            Ok(scan.finish())
        })
    }

    fn remove_automation<'a>(&'a self, automation_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            self.automations
                .lock()
                .expect("interaction automation store lock poisoned")
                .remove(automation_id);
            Ok(())
        })
    }
}

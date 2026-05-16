use super::codec::{decode_record_json, encode_record_json};
use super::{
    AgentSessionRecord, AgentSessionRegistryStore, AutomationRecord, AutomationScheduleScan,
    AutomationScheduleScanBuilder, GoalRecord, duplicate_automation_error, duplicate_session_error,
    missing_automation_error, missing_session_error,
};
use crate::interaction::{AutomationStatus, InteractionError, InteractionFuture};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use opendal::{ErrorKind, Operator};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct OpenDalAgentSessionRegistryStoreConfig {
    pub prefix: String,
}

impl OpenDalAgentSessionRegistryStoreConfig {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }
}

#[derive(Clone)]
pub struct OpenDalAgentSessionRegistryStore {
    operator: Operator,
    prefix: String,
}

impl OpenDalAgentSessionRegistryStore {
    pub fn new(operator: Operator, config: OpenDalAgentSessionRegistryStoreConfig) -> Self {
        Self {
            operator,
            prefix: normalize_prefix(&config.prefix),
        }
    }

    fn session_path(&self, session_id: &str) -> String {
        let encoded = URL_SAFE_NO_PAD.encode(session_id.as_bytes());
        format!("{}{encoded}.json", self.prefix)
    }

    fn goal_path(&self, session_id: &str) -> String {
        let encoded = URL_SAFE_NO_PAD.encode(session_id.as_bytes());
        format!("{}goals/{encoded}.json", self.prefix)
    }

    fn automation_path(&self, automation_id: &str) -> String {
        let encoded = URL_SAFE_NO_PAD.encode(automation_id.as_bytes());
        format!("{}automations/{encoded}.json", self.prefix)
    }

    fn automation_schedule_path(&self, automation_id: &str) -> String {
        let encoded = URL_SAFE_NO_PAD.encode(automation_id.as_bytes());
        format!("{}automation-schedule/{encoded}.json", self.prefix)
    }

    async fn save_automation_schedule_index(
        &self,
        automation: &AutomationRecord,
    ) -> Result<(), InteractionError> {
        let path = self.automation_schedule_path(&automation.automation_id);
        if automation.is_active() && automation.next_fire_at_ms.is_some() {
            let bytes = serde_json::to_vec(&ObjectAutomationScheduleEntry::from(automation))
                .map_err(to_store_error)?;
            self.operator
                .write(&path, bytes)
                .await
                .map_err(to_store_error)?;
        } else {
            self.operator.delete(&path).await.map_err(to_store_error)?;
        }
        Ok(())
    }

    async fn remove_automation_schedule_index(
        &self,
        automation_id: &str,
    ) -> Result<(), InteractionError> {
        self.operator
            .delete(&self.automation_schedule_path(automation_id))
            .await
            .map_err(to_store_error)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectAutomationScheduleEntry {
    automation_id: String,
    status: AutomationStatus,
    next_fire_at_ms: Option<u64>,
    updated_at_ms: u64,
}

impl From<&AutomationRecord> for ObjectAutomationScheduleEntry {
    fn from(automation: &AutomationRecord) -> Self {
        Self {
            automation_id: automation.automation_id.clone(),
            status: automation.status.clone(),
            next_fire_at_ms: automation.next_fire_at_ms,
            updated_at_ms: automation.updated_at_ms,
        }
    }
}

impl AgentSessionRegistryStore for OpenDalAgentSessionRegistryStore {
    fn insert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.session_path(&record.session_id);
            if self.operator.exists(&path).await.map_err(to_store_error)? {
                return Err(duplicate_session_error(&record.session_id));
            }
            let bytes = encode_record_json(&record)?.into_bytes();
            self.operator
                .write(&path, bytes)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.session_path(&record.session_id);
            if !self.operator.exists(&path).await.map_err(to_store_error)? {
                return Err(missing_session_error(&record.session_id));
            }
            let bytes = encode_record_json(&record)?.into_bytes();
            self.operator
                .write(&path, bytes)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.session_path(session_id);
            self.operator.delete(&path).await.map_err(to_store_error)?;
            Ok(())
        })
    }

    fn get<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<AgentSessionRecord>> {
        Box::pin(async move {
            let path = self.session_path(session_id);
            let bytes = match self.operator.read(&path).await {
                Ok(bytes) => bytes.to_bytes(),
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(to_store_error(error)),
            };
            Ok(Some(decode_record_json(session_id, bytes.as_ref())?))
        })
    }

    fn list<'a>(&'a self) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        Box::pin(async move {
            let entries = self
                .operator
                .list(&self.prefix)
                .await
                .map_err(to_store_error)?;
            let mut records = Vec::new();
            for entry in entries {
                if !entry.path().ends_with(".json")
                    || entry.path().starts_with(&format!("{}goals/", self.prefix))
                    || entry
                        .path()
                        .starts_with(&format!("{}automations/", self.prefix))
                    || entry
                        .path()
                        .starts_with(&format!("{}automation-schedule/", self.prefix))
                {
                    continue;
                }
                let bytes = self
                    .operator
                    .read(entry.path())
                    .await
                    .map_err(to_store_error)?
                    .to_bytes();
                records.push(decode_record_json(entry.path(), bytes.as_ref())?);
            }
            records.sort_by(|left, right| left.session_id.cmp(&right.session_id));
            Ok(records)
        })
    }

    fn save_goal<'a>(&'a self, goal: GoalRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.goal_path(&goal.session_id);
            let bytes = serde_json::to_vec(&goal).map_err(to_store_error)?;
            self.operator
                .write(&path, bytes)
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn get_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<GoalRecord>> {
        Box::pin(async move {
            let path = self.goal_path(session_id);
            let bytes = match self.operator.read(&path).await {
                Ok(bytes) => bytes.to_bytes(),
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(to_store_error(error)),
            };
            serde_json::from_slice(bytes.as_ref())
                .map(Some)
                .map_err(to_store_error)
        })
    }

    fn list_goals<'a>(&'a self) -> InteractionFuture<'a, Vec<GoalRecord>> {
        Box::pin(async move {
            let prefix = format!("{}goals/", self.prefix);
            let entries = self.operator.list(&prefix).await.map_err(to_store_error)?;
            let mut records = Vec::new();
            for entry in entries {
                if !entry.path().ends_with(".json") {
                    continue;
                }
                let bytes = self
                    .operator
                    .read(entry.path())
                    .await
                    .map_err(to_store_error)?
                    .to_bytes();
                records.push(serde_json::from_slice(bytes.as_ref()).map_err(to_store_error)?);
            }
            records.sort_by(|left: &GoalRecord, right| left.session_id.cmp(&right.session_id));
            Ok(records)
        })
    }

    fn remove_goal<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            self.operator
                .delete(&self.goal_path(session_id))
                .await
                .map_err(to_store_error)?;
            Ok(())
        })
    }

    fn insert_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.automation_path(&automation.automation_id);
            if self.operator.exists(&path).await.map_err(to_store_error)? {
                return Err(duplicate_automation_error(&automation.automation_id));
            }
            let bytes = serde_json::to_vec(&automation).map_err(to_store_error)?;
            self.save_automation_schedule_index(&automation).await?;
            match self.operator.write(&path, bytes).await {
                Ok(_) => Ok(()),
                Err(error) => {
                    let _ = self
                        .remove_automation_schedule_index(&automation.automation_id)
                        .await;
                    Err(to_store_error(error))
                }
            }
        })
    }

    fn save_automation<'a>(&'a self, automation: AutomationRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.automation_path(&automation.automation_id);
            let previous = self
                .get_automation(&automation.automation_id)
                .await?
                .ok_or_else(|| missing_automation_error(&automation.automation_id))?;
            if !self.operator.exists(&path).await.map_err(to_store_error)? {
                return Err(missing_automation_error(&automation.automation_id));
            }
            let bytes = serde_json::to_vec(&automation).map_err(to_store_error)?;
            self.save_automation_schedule_index(&automation).await?;
            match self.operator.write(&path, bytes).await {
                Ok(_) => Ok(()),
                Err(error) => {
                    let _ = self.save_automation_schedule_index(&previous).await;
                    Err(to_store_error(error))
                }
            }
        })
    }

    fn get_automation<'a>(
        &'a self,
        automation_id: &'a str,
    ) -> InteractionFuture<'a, Option<AutomationRecord>> {
        Box::pin(async move {
            let path = self.automation_path(automation_id);
            let bytes = match self.operator.read(&path).await {
                Ok(bytes) => bytes.to_bytes(),
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(to_store_error(error)),
            };
            serde_json::from_slice(bytes.as_ref())
                .map(Some)
                .map_err(to_store_error)
        })
    }

    fn list_automations<'a>(&'a self) -> InteractionFuture<'a, Vec<AutomationRecord>> {
        Box::pin(async move {
            let prefix = format!("{}automations/", self.prefix);
            let entries = self.operator.list(&prefix).await.map_err(to_store_error)?;
            let mut records = Vec::new();
            for entry in entries {
                if !entry.path().ends_with(".json") {
                    continue;
                }
                let bytes = self
                    .operator
                    .read(entry.path())
                    .await
                    .map_err(to_store_error)?
                    .to_bytes();
                records.push(serde_json::from_slice(bytes.as_ref()).map_err(to_store_error)?);
            }
            records.sort_by(|left: &AutomationRecord, right| {
                left.automation_id.cmp(&right.automation_id)
            });
            Ok(records)
        })
    }

    fn scan_automation_schedule<'a>(
        &'a self,
        now_ms: u64,
    ) -> InteractionFuture<'a, AutomationScheduleScan> {
        Box::pin(async move {
            let prefix = format!("{}automation-schedule/", self.prefix);
            let entries = self.operator.list(&prefix).await.map_err(to_store_error)?;
            let mut scan = AutomationScheduleScanBuilder::default();
            for entry in entries {
                if !entry.path().ends_with(".json") {
                    continue;
                }
                let bytes = self
                    .operator
                    .read(entry.path())
                    .await
                    .map_err(to_store_error)?
                    .to_bytes();
                let index: ObjectAutomationScheduleEntry =
                    serde_json::from_slice(bytes.as_ref()).map_err(to_store_error)?;
                let Some(automation) = self.get_automation(&index.automation_id).await? else {
                    self.remove_automation_schedule_index(&index.automation_id)
                        .await?;
                    continue;
                };
                if !automation.is_active() || automation.next_fire_at_ms.is_none() {
                    self.remove_automation_schedule_index(&index.automation_id)
                        .await?;
                    continue;
                }
                if index.status != automation.status
                    || index.next_fire_at_ms != automation.next_fire_at_ms
                    || index.updated_at_ms != automation.updated_at_ms
                {
                    self.save_automation_schedule_index(&automation).await?;
                }
                scan.include(
                    automation.automation_id,
                    true,
                    automation.next_fire_at_ms,
                    now_ms,
                );
            }
            Ok(scan.finish())
        })
    }

    fn remove_automation<'a>(&'a self, automation_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            self.operator
                .delete(&self.automation_path(automation_id))
                .await
                .map_err(to_store_error)?;
            self.remove_automation_schedule_index(automation_id).await?;
            Ok(())
        })
    }
}

fn normalize_prefix(prefix: &str) -> String {
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    }
}

fn to_store_error(error: impl std::fmt::Display) -> InteractionError {
    InteractionError::internal(format!(
        "opendal agent session registry store error: {error}"
    ))
}

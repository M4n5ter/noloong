use super::codec::{decode_record_json, encode_record_json};
use super::{
    AgentSessionRecord, AgentSessionRegistryStore, AutomationRecord, AutomationScheduleScan,
    AutomationScheduleScanBuilder, GoalRecord, duplicate_automation_error, duplicate_session_error,
    missing_automation_error, missing_session_error, record_matches_session_list_filter,
    session_metadata_index_value,
};
use crate::interaction::{
    AgentSessionListFilter, AutomationStatus, InteractionError, InteractionFuture,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use opendal::{ErrorKind, Operator};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::{sync::Semaphore, task::JoinSet};

const OBJECT_STORE_LIST_READ_CONCURRENCY: usize = 16;
const OBJECT_METADATA_INDEX_MARKER: &[u8] = b"{}";

type ObjectMetadataSessionCandidates = BTreeMap<String, BTreeSet<String>>;

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

    fn session_index_path(&self, session_id: &str) -> String {
        let encoded = URL_SAFE_NO_PAD.encode(session_id.as_bytes());
        format!("{}session-index/{encoded}.json", self.prefix)
    }

    fn session_metadata_index_prefix(&self, key: &str, value: &str) -> String {
        let encoded_key = URL_SAFE_NO_PAD.encode(key.as_bytes());
        let encoded_value = URL_SAFE_NO_PAD.encode(value.as_bytes());
        format!(
            "{}session-metadata/{encoded_key}/{encoded_value}/",
            self.prefix
        )
    }

    fn session_metadata_index_path(&self, key: &str, value: &str, session_id: &str) -> String {
        let encoded_session_id = URL_SAFE_NO_PAD.encode(session_id.as_bytes());
        format!(
            "{}{encoded_session_id}.json",
            self.session_metadata_index_prefix(key, value)
        )
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

    async fn save_session_index(
        &self,
        record: &AgentSessionRecord,
    ) -> Result<(), InteractionError> {
        let bytes =
            serde_json::to_vec(&ObjectSessionIndexEntry::from(record)).map_err(to_store_error)?;
        self.operator
            .write(&self.session_index_path(&record.session_id), bytes)
            .await
            .map_err(to_store_error)?;
        Ok(())
    }

    async fn save_session_indexes(
        &self,
        record: &AgentSessionRecord,
    ) -> Result<(), InteractionError> {
        self.save_session_index(record).await?;
        if let Err(error) = self.save_session_metadata_index(record).await {
            let _ = self.remove_session_index(&record.session_id).await;
            let _ = self.remove_session_metadata_index(record).await;
            return Err(error);
        }
        Ok(())
    }

    async fn save_session_metadata_index(
        &self,
        record: &AgentSessionRecord,
    ) -> Result<(), InteractionError> {
        for (key, value) in &record.metadata {
            let Some(value) = session_metadata_index_value(value) else {
                continue;
            };
            self.operator
                .write(
                    &self.session_metadata_index_path(key, &value, &record.session_id),
                    OBJECT_METADATA_INDEX_MARKER.to_vec(),
                )
                .await
                .map_err(to_store_error)?;
        }
        Ok(())
    }

    async fn remove_session_index(&self, session_id: &str) -> Result<(), InteractionError> {
        self.operator
            .delete(&self.session_index_path(session_id))
            .await
            .map_err(to_store_error)?;
        Ok(())
    }

    async fn remove_session_metadata_index(
        &self,
        record: &AgentSessionRecord,
    ) -> Result<(), InteractionError> {
        self.remove_session_metadata_paths(self.session_metadata_index_paths(record))
            .await
    }

    async fn remove_session_metadata_index_difference(
        &self,
        stale: &AgentSessionRecord,
        keep: &AgentSessionRecord,
    ) -> Result<(), InteractionError> {
        let keep_paths = self.session_metadata_index_paths(keep);
        let stale_paths = self
            .session_metadata_index_paths(stale)
            .difference(&keep_paths)
            .cloned()
            .collect();
        self.remove_session_metadata_paths(stale_paths).await
    }

    async fn remove_session_metadata_index_entry(
        &self,
        index: &ObjectSessionIndexEntry,
    ) -> Result<(), InteractionError> {
        self.remove_session_metadata_paths(self.session_metadata_index_paths_for_index(index))
            .await
    }

    async fn remove_session_metadata_index_entry_difference(
        &self,
        stale: &ObjectSessionIndexEntry,
        keep: &ObjectSessionIndexEntry,
    ) -> Result<(), InteractionError> {
        let keep_paths = self.session_metadata_index_paths_for_index(keep);
        let stale_paths = self
            .session_metadata_index_paths_for_index(stale)
            .difference(&keep_paths)
            .cloned()
            .collect();
        self.remove_session_metadata_paths(stale_paths).await
    }

    fn session_metadata_index_paths(&self, record: &AgentSessionRecord) -> BTreeSet<String> {
        record
            .metadata
            .iter()
            .filter_map(|(key, value)| {
                session_metadata_index_value(value)
                    .map(|value| self.session_metadata_index_path(key, &value, &record.session_id))
            })
            .collect()
    }

    fn session_metadata_index_paths_for_index(
        &self,
        index: &ObjectSessionIndexEntry,
    ) -> BTreeSet<String> {
        index
            .metadata
            .iter()
            .map(|(key, value)| self.session_metadata_index_path(key, value, &index.session_id))
            .collect()
    }

    async fn remove_session_metadata_paths(
        &self,
        paths: BTreeSet<String>,
    ) -> Result<(), InteractionError> {
        for path in paths {
            self.operator.delete(&path).await.map_err(to_store_error)?;
        }
        Ok(())
    }

    async fn remove_stale_filter_metadata_index_entries(
        &self,
        index: &ObjectSessionIndexEntry,
        filter: &AgentSessionListFilter,
    ) -> Result<(), InteractionError> {
        let mut stale_paths = BTreeSet::new();
        for (key, value) in &filter.metadata_equals {
            let Some(value) = session_metadata_index_value(value) else {
                continue;
            };
            if index.metadata.get(key) == Some(&value) {
                continue;
            }
            stale_paths.insert(self.session_metadata_index_path(key, &value, &index.session_id));
        }
        self.remove_session_metadata_paths(stale_paths).await
    }

    async fn metadata_session_candidates(
        &self,
        filter: &AgentSessionListFilter,
    ) -> Result<Option<ObjectMetadataSessionCandidates>, InteractionError> {
        if filter.metadata_equals.is_empty() {
            return Ok(None);
        }
        let mut candidates: Option<ObjectMetadataSessionCandidates> = None;
        for (key, value) in &filter.metadata_equals {
            let Some(value) = session_metadata_index_value(value) else {
                return Ok(Some(BTreeMap::new()));
            };
            let prefix = self.session_metadata_index_prefix(key, &value);
            let entries = self.operator.list(&prefix).await.map_err(to_store_error)?;
            let mut matching = BTreeMap::new();
            for entry in entries {
                let Some(session_id) = decode_metadata_index_session_id(&prefix, entry.path())
                else {
                    continue;
                };
                matching
                    .entry(session_id)
                    .or_insert_with(BTreeSet::new)
                    .insert(entry.path().to_owned());
            }
            candidates = Some(match candidates {
                Some(existing) => existing
                    .into_iter()
                    .filter_map(|(session_id, mut paths)| {
                        matching.get(&session_id).map(|matching_paths| {
                            paths.extend(matching_paths.iter().cloned());
                            (session_id, paths)
                        })
                    })
                    .collect(),
                None => matching,
            });
            if candidates.as_ref().is_some_and(BTreeMap::is_empty) {
                break;
            }
        }
        Ok(candidates)
    }

    async fn load_candidate_session_indexes(
        &self,
        candidates: ObjectMetadataSessionCandidates,
    ) -> Result<Vec<ObjectSessionIndexEntry>, InteractionError> {
        let semaphore = Arc::new(Semaphore::new(OBJECT_STORE_LIST_READ_CONCURRENCY));
        let mut tasks = JoinSet::new();
        for (session_id, metadata_paths) in candidates {
            let operator = self.operator.clone();
            let index_path = self.session_index_path(&session_id);
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(to_store_error)?;
            tasks.spawn(async move {
                let _permit = permit;
                let index = read_session_index_entry(operator, index_path).await?;
                Ok::<_, InteractionError>((session_id, metadata_paths, index))
            });
        }

        let mut indexes = Vec::new();
        while let Some(result) = tasks.join_next().await {
            let (session_id, metadata_paths, index) = result.map_err(|error| {
                InteractionError::internal(format!(
                    "object store session index read task failed: {error}"
                ))
            })??;
            if let Some(index) = index {
                indexes.push(index);
            } else {
                let _ = self.remove_session_index(&session_id).await;
                self.remove_session_metadata_paths(metadata_paths).await?;
            }
        }
        Ok(indexes)
    }

    async fn load_all_session_indexes(
        &self,
    ) -> Result<Vec<ObjectSessionIndexEntry>, InteractionError> {
        let prefix = format!("{}session-index/", self.prefix);
        let paths = self
            .operator
            .list(&prefix)
            .await
            .map_err(to_store_error)?
            .into_iter()
            .filter_map(|entry| {
                entry
                    .path()
                    .ends_with(".json")
                    .then(|| entry.path().to_owned())
            })
            .collect::<Vec<_>>();
        self.load_session_indexes_by_path(paths).await
    }

    async fn load_session_indexes_by_path(
        &self,
        paths: Vec<String>,
    ) -> Result<Vec<ObjectSessionIndexEntry>, InteractionError> {
        let semaphore = Arc::new(Semaphore::new(OBJECT_STORE_LIST_READ_CONCURRENCY));
        let mut tasks = JoinSet::new();
        for path in paths {
            let operator = self.operator.clone();
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(to_store_error)?;
            tasks.spawn(async move {
                let _permit = permit;
                read_session_index_entry(operator, path).await
            });
        }

        let mut indexes = Vec::new();
        while let Some(result) = tasks.join_next().await {
            if let Some(index) = result.map_err(|error| {
                InteractionError::internal(format!(
                    "object store session index read task failed: {error}"
                ))
            })?? {
                indexes.push(index);
            }
        }
        Ok(indexes)
    }

    async fn load_records_for_indexes(
        &self,
        indexes: Vec<ObjectSessionIndexEntry>,
    ) -> Result<Vec<(ObjectSessionIndexEntry, Option<AgentSessionRecord>)>, InteractionError> {
        let semaphore = Arc::new(Semaphore::new(OBJECT_STORE_LIST_READ_CONCURRENCY));
        let mut tasks = JoinSet::new();
        for index in indexes {
            let operator = self.operator.clone();
            let path = self.session_path(&index.session_id);
            let session_id = index.session_id.clone();
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(to_store_error)?;
            tasks.spawn(async move {
                let _permit = permit;
                let record = match operator.read(&path).await {
                    Ok(bytes) => Some(decode_record_json(&session_id, bytes.to_bytes().as_ref())?),
                    Err(error) if error.kind() == ErrorKind::NotFound => None,
                    Err(error) => return Err(to_store_error(error)),
                };
                Ok::<_, InteractionError>((index, record))
            });
        }

        let mut records = Vec::new();
        while let Some(result) = tasks.join_next().await {
            records.push(result.map_err(|error| {
                InteractionError::internal(format!(
                    "object store session record read task failed: {error}"
                ))
            })??);
        }
        Ok(records)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectSessionIndexEntry {
    session_id: String,
    profile_id: String,
    parent_session_id: Option<String>,
    status: String,
    metadata: BTreeMap<String, String>,
    updated_at_ms: u64,
}

impl ObjectSessionIndexEntry {
    fn matches_filter_prefilter(&self, filter: &AgentSessionListFilter) -> bool {
        if filter
            .parent_session_id
            .as_ref()
            .is_some_and(|parent| self.parent_session_id.as_ref() != Some(parent))
        {
            return false;
        }
        if filter
            .profile_id
            .as_ref()
            .is_some_and(|profile| &self.profile_id != profile)
        {
            return false;
        }
        if filter
            .status
            .as_ref()
            .is_some_and(|status| self.status != status.as_str())
        {
            return false;
        }
        filter.metadata_equals.iter().all(|(key, value)| {
            session_metadata_index_value(value)
                .as_ref()
                .is_some_and(|expected| self.metadata.get(key) == Some(expected))
        })
    }
}

impl From<&AgentSessionRecord> for ObjectSessionIndexEntry {
    fn from(record: &AgentSessionRecord) -> Self {
        Self {
            session_id: record.session_id.clone(),
            profile_id: record.profile_id.clone(),
            parent_session_id: record.parent_session_id.clone(),
            status: crate::interaction::InteractionSessionStatus::from(record.state.status.clone())
                .as_str()
                .to_owned(),
            metadata: record
                .metadata
                .iter()
                .filter_map(|(key, value)| {
                    session_metadata_index_value(value).map(|value| (key.clone(), value))
                })
                .collect(),
            updated_at_ms: record.updated_at_ms,
        }
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
            self.save_session_indexes(&record).await?;
            match self.operator.write(&path, bytes).await {
                Ok(_) => Ok(()),
                Err(error) => {
                    let _ = self.remove_session_index(&record.session_id).await;
                    let _ = self.remove_session_metadata_index(&record).await;
                    Err(to_store_error(error))
                }
            }
        })
    }

    fn save<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let path = self.session_path(&record.session_id);
            let previous = self
                .get(&record.session_id)
                .await?
                .ok_or_else(|| missing_session_error(&record.session_id))?;
            if !self.operator.exists(&path).await.map_err(to_store_error)? {
                return Err(missing_session_error(&record.session_id));
            }
            let bytes = encode_record_json(&record)?.into_bytes();
            if let Err(error) = self.save_session_indexes(&record).await {
                let _ = self.save_session_indexes(&previous).await;
                let _ = self
                    .remove_session_metadata_index_difference(&record, &previous)
                    .await;
                return Err(error);
            }
            match self.operator.write(&path, bytes).await {
                Ok(_) => {
                    self.remove_session_metadata_index_difference(&previous, &record)
                        .await?;
                    Ok(())
                }
                Err(error) => {
                    let _ = self.save_session_indexes(&previous).await;
                    let _ = self
                        .remove_session_metadata_index_difference(&record, &previous)
                        .await;
                    Err(to_store_error(error))
                }
            }
        })
    }

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            let previous = self.get(session_id).await?;
            let path = self.session_path(session_id);
            self.operator.delete(&path).await.map_err(to_store_error)?;
            self.remove_session_index(session_id).await?;
            if let Some(previous) = previous {
                self.remove_session_metadata_index(&previous).await?;
            }
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

    fn list<'a>(
        &'a self,
        filter: &'a AgentSessionListFilter,
    ) -> InteractionFuture<'a, Vec<AgentSessionRecord>> {
        Box::pin(async move {
            let indexes =
                if let Some(candidate_ids) = self.metadata_session_candidates(filter).await? {
                    self.load_candidate_session_indexes(candidate_ids).await?
                } else {
                    self.load_all_session_indexes().await?
                };
            let mut record_indexes = Vec::new();
            for index in indexes {
                if !index.matches_filter_prefilter(filter) {
                    self.remove_stale_filter_metadata_index_entries(&index, filter)
                        .await?;
                    continue;
                }
                record_indexes.push(index);
            }
            let mut records = Vec::new();
            for (index, record) in self.load_records_for_indexes(record_indexes).await? {
                let Some(record) = record else {
                    self.remove_session_index(&index.session_id).await?;
                    self.remove_session_metadata_index_entry(&index).await?;
                    continue;
                };
                let current_index = ObjectSessionIndexEntry::from(&record);
                if current_index.updated_at_ms != index.updated_at_ms
                    || current_index.metadata != index.metadata
                {
                    self.save_session_indexes(&record).await?;
                    self.remove_session_metadata_index_entry_difference(&index, &current_index)
                        .await?;
                }
                if record_matches_session_list_filter(&record, filter) {
                    records.push(record);
                }
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

fn decode_metadata_index_session_id(prefix: &str, path: &str) -> Option<String> {
    let encoded = path.strip_prefix(prefix)?.strip_suffix(".json")?;
    if encoded.is_empty() || encoded.contains('/') {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(encoded.as_bytes()).ok()?;
    String::from_utf8(bytes).ok()
}

async fn read_session_index_entry(
    operator: Operator,
    path: String,
) -> Result<Option<ObjectSessionIndexEntry>, InteractionError> {
    match operator.read(&path).await {
        Ok(bytes) => serde_json::from_slice::<ObjectSessionIndexEntry>(bytes.to_bytes().as_ref())
            .map(Some)
            .map_err(to_store_error),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(to_store_error(error)),
    }
}

fn to_store_error(error: impl std::fmt::Display) -> InteractionError {
    InteractionError::internal(format!(
        "opendal agent session registry store error: {error}"
    ))
}

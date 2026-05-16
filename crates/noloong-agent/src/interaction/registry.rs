use super::{
    AUTOMATION_SESSION_METADATA_KEY, AUTOMATION_SOURCE_TYPE, AUTOMATION_SYSTEM_PROMPT_ADDITION_ID,
    AgentRuntimeProfile, AutomationPromptInput, AutomationRecord, AutomationStatus,
    AutomationTarget, AutomationTrigger, GOAL_AUDIT_REASON_TURN_END, GOAL_UPDATE_STATUS_ERROR,
    GoalAuditRecord, GoalRecord, GoalStatus, InteractionError, InteractionProfileDescriptor,
    InteractionSessionDescriptor, InteractionSessionStatus, automation_identity_prompt,
    automation_session_metadata, existing_session_automation_message, goal_audit_message,
    session_ready_for_direct_prompt,
    store::{
        AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentSessionQueueSnapshot, AgentSessionQueueState,
        AgentSessionRecord, AgentSessionRegistryStore, InMemoryAgentSessionRegistryStore,
        current_unix_ms, duplicate_session_error, missing_automation_error, missing_session_error,
    },
    trim_non_empty,
};
use crate::tools::{
    GoalController, GoalUpdateRequest as ToolGoalUpdateRequest, SubagentController, SubagentResult,
    SubagentSpawnRequest as ToolSubagentSpawnRequest, SubagentSummary, SubagentWaitOutcome,
    final_assistant_output, update_goal_audit,
};
use crate::{AgentManifest, AgentSession, ManifestPatch, SystemPromptAddition};
use noloong_agent_core::{
    Agent, AgentCoreError, AgentEvent, AgentEventKind, AgentInput, AgentMessage, AgentState,
    BoxFuture, CancellationToken, RunStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::{Notify, RwLock};
use tokio::time::{Duration, Instant, sleep};

const INTERRUPTED_RUNNING_SESSION_ERROR: &str =
    "agent session was interrupted while running and cannot be resumed automatically";
const SUBAGENT_INHERIT_PROMPT_ADDITIONS_METADATA: &str = "inheritPromptAdditions";
const TRANSIENT_PROMPT_ADDITION_PREFIX: &str = "transient.";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<AgentManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manifest_patches: Vec<ManifestPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSpawnRequest {
    pub parent_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<AgentManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manifest_patches: Vec<ManifestPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<AgentMessage>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoalSetRequest {
    pub session_id: String,
    pub objective: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoalStatusUpdateRequest {
    pub session_id: String,
    pub status: GoalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    pub target: AutomationTarget,
    pub trigger: AutomationTrigger,
    pub prompt: AutomationPromptInput,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<AutomationStatus>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationUpdateRequest {
    pub automation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<AutomationStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<AutomationTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<AutomationTrigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<AutomationPromptInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRequest {
    pub automation_id: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionListFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<InteractionSessionStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRegistryOptions {
    #[serde(default = "default_automation_runner_enabled")]
    pub automation_runner_enabled: bool,
}

impl Default for AgentSessionRegistryOptions {
    fn default() -> Self {
        Self {
            automation_runner_enabled: true,
        }
    }
}

fn default_automation_runner_enabled() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionDeleteOptions {
    #[serde(default)]
    pub force_abort: bool,
}

pub struct RegisteredAgentSession {
    record: Mutex<AgentSessionRecord>,
    session: AgentSession,
    agent: Agent,
    store: Arc<dyn AgentSessionRegistryStore>,
}

impl RegisteredAgentSession {
    pub fn record(&self) -> AgentSessionRecord {
        self.record
            .lock()
            .expect("interaction session record lock poisoned")
            .clone()
    }

    pub fn session(&self) -> &AgentSession {
        &self.session
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    pub async fn descriptor(&self) -> InteractionSessionDescriptor {
        descriptor_from_record(&self.snapshot_record().await)
    }

    pub async fn save_snapshot(&self) -> Result<InteractionSessionDescriptor, InteractionError> {
        let record = self.snapshot_record().await;
        self.store.save(record.clone()).await?;
        *self
            .record
            .lock()
            .expect("interaction session record lock poisoned") = record.clone();
        Ok(descriptor_from_record(&record))
    }

    async fn snapshot_record(&self) -> AgentSessionRecord {
        let mut record = self.record();
        record.schema_version = AGENT_SESSION_RECORD_SCHEMA_VERSION;
        record.manifest = self.session.manifest();
        record.state = self.agent.state().await;
        record.queues = queue_snapshot_from_agent(&self.agent);
        record.updated_at_ms = current_unix_ms();
        record
    }
}

#[derive(Clone)]
pub struct AgentSessionRegistry {
    inner: Arc<AgentSessionRegistryInner>,
}

struct AgentSessionRegistryInner {
    profiles: BTreeMap<String, Arc<dyn AgentRuntimeProfile>>,
    default_profile_id: String,
    store: Arc<dyn AgentSessionRegistryStore>,
    sessions: RwLock<BTreeMap<String, Arc<RegisteredAgentSession>>>,
    creating_sessions: Mutex<BTreeSet<String>>,
    firing_automations: Mutex<BTreeSet<String>>,
    session_changes: Notify,
    automation_changes: Arc<Notify>,
    counter: AtomicU64,
}

struct CreateSessionReservation {
    inner: Arc<AgentSessionRegistryInner>,
    session_id: String,
}

struct AutomationFireReservation {
    inner: Arc<AgentSessionRegistryInner>,
    automation_id: String,
}

impl Drop for CreateSessionReservation {
    fn drop(&mut self) {
        self.inner
            .creating_sessions
            .lock()
            .expect("interaction creating sessions lock poisoned")
            .remove(&self.session_id);
        self.inner.session_changes.notify_waiters();
    }
}

impl Drop for AutomationFireReservation {
    fn drop(&mut self) {
        self.inner
            .firing_automations
            .lock()
            .expect("interaction firing automations lock poisoned")
            .remove(&self.automation_id);
        self.inner.automation_changes.notify_waiters();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AutomationFireMode {
    Manual,
    Triggered,
}

impl AgentSessionRegistry {
    pub fn new(default_profile: Arc<dyn AgentRuntimeProfile>) -> Result<Self, InteractionError> {
        Self::with_store(
            default_profile.descriptor().profile_id.clone(),
            vec![default_profile],
            Arc::new(InMemoryAgentSessionRegistryStore::default()),
        )
    }

    pub fn with_store(
        default_profile_id: impl Into<String>,
        profiles: impl IntoIterator<Item = Arc<dyn AgentRuntimeProfile>>,
        store: Arc<dyn AgentSessionRegistryStore>,
    ) -> Result<Self, InteractionError> {
        Self::with_store_and_options(
            default_profile_id,
            profiles,
            store,
            AgentSessionRegistryOptions::default(),
        )
    }

    pub fn with_store_and_options(
        default_profile_id: impl Into<String>,
        profiles: impl IntoIterator<Item = Arc<dyn AgentRuntimeProfile>>,
        store: Arc<dyn AgentSessionRegistryStore>,
        options: AgentSessionRegistryOptions,
    ) -> Result<Self, InteractionError> {
        let mut profiles_by_id = BTreeMap::new();
        for profile in profiles {
            let id = profile.descriptor().profile_id;
            if profiles_by_id.insert(id.clone(), profile).is_some() {
                return Err(InteractionError::invalid_params(format!(
                    "duplicate runtime profile id: {id}"
                )));
            }
        }
        let default_profile_id = default_profile_id.into();
        if !profiles_by_id.contains_key(&default_profile_id) {
            return Err(InteractionError::not_found(format!(
                "default runtime profile not found: {default_profile_id}"
            )));
        }
        let registry = Self {
            inner: Arc::new(AgentSessionRegistryInner {
                profiles: profiles_by_id,
                default_profile_id,
                store,
                sessions: RwLock::new(BTreeMap::new()),
                creating_sessions: Mutex::new(BTreeSet::new()),
                firing_automations: Mutex::new(BTreeSet::new()),
                session_changes: Notify::new(),
                automation_changes: Arc::new(Notify::new()),
                counter: AtomicU64::new(0),
            }),
        };
        if options.automation_runner_enabled {
            registry.spawn_automation_runner_if_possible();
        }
        Ok(registry)
    }

    pub fn profile_descriptors(&self) -> Vec<InteractionProfileDescriptor> {
        self.inner
            .profiles
            .values()
            .map(|profile| profile.descriptor())
            .collect()
    }

    pub async fn create_session(
        &self,
        request: AgentSessionCreateRequest,
    ) -> Result<InteractionSessionDescriptor, InteractionError> {
        let profile_id = request
            .profile_id
            .clone()
            .unwrap_or_else(|| self.inner.default_profile_id.clone());
        let profile = self
            .inner
            .profiles
            .get(&profile_id)
            .cloned()
            .ok_or_else(|| {
                InteractionError::not_found(format!("profile not found: {profile_id}"))
            })?;
        let profile_descriptor = profile.descriptor();
        let manifest = manifest_for_request(
            request.manifest.clone(),
            &profile_descriptor,
            &request.manifest_patches,
        )?;
        let session_id = request
            .session_id
            .clone()
            .unwrap_or_else(|| self.next_session_id());
        let _reservation = self.reserve_session_id(&session_id)?;
        if self.inner.sessions.read().await.contains_key(&session_id) {
            return Err(duplicate_session_error(&session_id));
        }

        let created_at_ms = current_unix_ms();
        let record = AgentSessionRecord {
            schema_version: AGENT_SESSION_RECORD_SCHEMA_VERSION,
            session_id: session_id.clone(),
            profile_id,
            parent_session_id: request.parent_session_id,
            role: request.role,
            manifest,
            state: AgentState::default(),
            queues: AgentSessionQueueSnapshot::default(),
            metadata: request.metadata,
            created_at_ms,
            updated_at_ms: created_at_ms,
        };
        let registered = self.registered_from_record(record.clone()).await?;
        self.inner.store.insert(record).await?;
        self.inner
            .sessions
            .write()
            .await
            .insert(session_id, Arc::clone(&registered));
        Ok(registered.descriptor().await)
    }

    pub async fn spawn_subagent(
        &self,
        request: SubagentSpawnRequest,
    ) -> Result<InteractionSessionDescriptor, InteractionError> {
        let parent = self
            .get_descriptor(&request.parent_session_id)
            .await?
            .ok_or_else(|| {
                InteractionError::not_found(format!(
                    "parent session not found: {}",
                    request.parent_session_id
                ))
            })?;
        let descriptor = self
            .create_session(AgentSessionCreateRequest {
                session_id: None,
                profile_id: request.profile_id.or(Some(parent.profile_id)),
                manifest: request.manifest,
                manifest_patches: request.manifest_patches,
                parent_session_id: Some(request.parent_session_id),
                role: request.role,
                metadata: request.metadata,
            })
            .await?;
        if let Some(initial_prompt) = request.initial_prompt
            && let Some(child) = self.get(&descriptor.session_id).await?
        {
            child.agent.prompt(initial_prompt).await?;
            return Ok(child.descriptor().await);
        }
        Ok(descriptor)
    }

    pub async fn list(
        &self,
        filter: AgentSessionListFilter,
    ) -> Result<Vec<InteractionSessionDescriptor>, InteractionError> {
        let mut descriptors = BTreeMap::new();
        for record in self.inner.store.list().await? {
            let record = self.normalize_record(record).await?;
            if !record_filter_matches(&record, &filter) {
                continue;
            }
            let descriptor = descriptor_from_record(&record);
            if status_filter_matches(&descriptor, &filter) {
                descriptors.insert(descriptor.session_id.clone(), descriptor);
            }
        }

        let sessions = self
            .inner
            .sessions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for session in sessions {
            if !record_filter_matches(&session.record(), &filter) {
                continue;
            }
            let descriptor = session.descriptor().await;
            if status_filter_matches(&descriptor, &filter) {
                descriptors.insert(descriptor.session_id.clone(), descriptor);
            }
        }
        Ok(descriptors.into_values().collect())
    }

    pub async fn get(
        &self,
        session_id: &str,
    ) -> Result<Option<Arc<RegisteredAgentSession>>, InteractionError> {
        loop {
            if let Some(session) = self.loaded_session(session_id).await {
                return Ok(Some(session));
            }
            let Some(_reservation) = self.try_reserve_session_id(session_id) else {
                self.inner.session_changes.notified().await;
                continue;
            };
            if let Some(session) = self.loaded_session(session_id).await {
                return Ok(Some(session));
            }
            let Some(record) = self.inner.store.get(session_id).await? else {
                return Ok(None);
            };
            let record = self.normalize_record(record).await?;
            let registered = self.registered_from_record(record).await?;
            self.inner
                .sessions
                .write()
                .await
                .insert(session_id.to_owned(), Arc::clone(&registered));
            return Ok(Some(registered));
        }
    }

    pub async fn get_descriptor(
        &self,
        session_id: &str,
    ) -> Result<Option<InteractionSessionDescriptor>, InteractionError> {
        if let Some(session) = self.loaded_session(session_id).await {
            return Ok(Some(session.descriptor().await));
        }
        let Some(record) = self.inner.store.get(session_id).await? else {
            return Ok(None);
        };
        let record = self.normalize_record(record).await?;
        Ok(Some(descriptor_from_record(&record)))
    }

    pub async fn set_goal(&self, request: GoalSetRequest) -> Result<GoalRecord, InteractionError> {
        if request.objective.trim().is_empty() {
            return Err(InteractionError::invalid_params(
                "goal objective must not be empty",
            ));
        }
        self.get_descriptor(&request.session_id)
            .await?
            .ok_or_else(|| missing_session_error(&request.session_id))?;
        let mut goal = GoalRecord::new(&request.session_id, request.objective.trim());
        goal.token_budget = request.token_budget;
        goal.metadata = request.metadata;
        self.inner.store.save_goal(goal.clone()).await?;
        self.update_loaded_goal(&request.session_id, Some(goal.clone()))
            .await;
        Ok(goal)
    }

    pub async fn get_goal(&self, session_id: &str) -> Result<Option<GoalRecord>, InteractionError> {
        self.get_descriptor(session_id)
            .await?
            .ok_or_else(|| missing_session_error(session_id))?;
        self.inner.store.get_goal(session_id).await
    }

    pub async fn pause_goal(&self, session_id: &str) -> Result<GoalRecord, InteractionError> {
        self.set_goal_status(session_id, GoalStatus::Paused).await
    }

    pub async fn resume_goal(&self, session_id: &str) -> Result<GoalRecord, InteractionError> {
        self.set_goal_status(session_id, GoalStatus::Pursuing).await
    }

    pub async fn clear_goal(&self, session_id: &str) -> Result<GoalRecord, InteractionError> {
        self.set_goal_status(session_id, GoalStatus::Cleared).await
    }

    pub async fn update_goal_status(
        &self,
        request: GoalStatusUpdateRequest,
        run_id: String,
        turn_id: u64,
    ) -> Result<GoalRecord, InteractionError> {
        if !request.status.is_goal_update_allowed() {
            return Err(InteractionError::invalid_params(GOAL_UPDATE_STATUS_ERROR));
        }
        let goal = self
            .inner
            .store
            .get_goal(&request.session_id)
            .await?
            .ok_or_else(|| missing_goal_error(&request.session_id))?;
        if !goal.is_pursuing() && request.status != GoalStatus::Pursuing {
            return Err(InteractionError::invalid_params(format!(
                "goal is not pursuing: {}",
                request.session_id
            )));
        }
        let tool_request = ToolGoalUpdateRequest {
            status: request.status,
            summary: request.summary.and_then(trim_non_empty),
            evidence: request.evidence.and_then(trim_non_empty),
        };
        let updated = update_goal_audit(goal, &tool_request, run_id, turn_id);
        self.inner.store.save_goal(updated.clone()).await?;
        self.update_loaded_goal(&updated.session_id, Some(updated.clone()))
            .await;
        Ok(updated)
    }

    pub async fn create_automation(
        &self,
        request: AutomationCreateRequest,
    ) -> Result<AutomationRecord, InteractionError> {
        request.trigger.validate()?;
        self.validate_automation_target(&request.target).await?;
        let automation_id = request
            .automation_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| self.next_automation_id());
        let prompt = request.prompt.into_message(&automation_id)?;
        let mut automation =
            AutomationRecord::new(automation_id, request.target, request.trigger, prompt);
        automation.metadata = request.metadata;
        automation.mark_updated();
        self.inner
            .store
            .insert_automation(automation.clone())
            .await?;
        self.inner.automation_changes.notify_waiters();
        Ok(automation)
    }

    pub async fn get_automation(
        &self,
        automation_id: &str,
    ) -> Result<Option<AutomationRecord>, InteractionError> {
        self.inner.store.get_automation(automation_id).await
    }

    pub async fn list_automations(
        &self,
        request: AutomationListRequest,
    ) -> Result<Vec<AutomationRecord>, InteractionError> {
        let mut automations = self.inner.store.list_automations().await?;
        if let Some(status) = request.status {
            automations.retain(|automation| automation.status == status);
        }
        automations.sort_by(|left, right| left.automation_id.cmp(&right.automation_id));
        Ok(automations)
    }

    pub async fn update_automation(
        &self,
        request: AutomationUpdateRequest,
    ) -> Result<AutomationRecord, InteractionError> {
        let mut automation = self
            .inner
            .store
            .get_automation(&request.automation_id)
            .await?
            .ok_or_else(|| missing_automation_error(&request.automation_id))?;
        if let Some(status) = request.status {
            automation.status = status;
        }
        if let Some(target) = request.target {
            self.validate_automation_target(&target).await?;
            automation.target = target;
        }
        if let Some(trigger) = request.trigger {
            trigger.validate()?;
            automation.next_fire_at_ms = trigger.next_fire_after_create(current_unix_ms());
            automation.trigger = trigger;
        }
        if let Some(prompt) = request.prompt {
            automation.prompt = prompt.into_message(&automation.automation_id)?;
        }
        if let Some(metadata) = request.metadata {
            automation.metadata = metadata;
        }
        if automation.status == AutomationStatus::Active && automation.next_fire_at_ms.is_none() {
            automation.next_fire_at_ms =
                automation.trigger.next_fire_after_create(current_unix_ms());
        }
        automation.last_error = None;
        automation.mark_updated();
        self.inner.store.save_automation(automation.clone()).await?;
        self.inner.automation_changes.notify_waiters();
        Ok(automation)
    }

    pub async fn delete_automation(
        &self,
        automation_id: &str,
    ) -> Result<AutomationRecord, InteractionError> {
        let automation = self
            .inner
            .store
            .get_automation(automation_id)
            .await?
            .ok_or_else(|| missing_automation_error(automation_id))?;
        self.inner.store.remove_automation(automation_id).await?;
        self.inner.automation_changes.notify_waiters();
        Ok(automation)
    }

    pub async fn fire_automation(
        &self,
        automation_id: &str,
    ) -> Result<AutomationRecord, InteractionError> {
        self.fire_automation_with_mode(automation_id, AutomationFireMode::Manual)
            .await
    }

    pub async fn delete_session(
        &self,
        session_id: &str,
        options: AgentSessionDeleteOptions,
    ) -> Result<InteractionSessionDescriptor, InteractionError> {
        let live_session = self.loaded_session(session_id).await;
        let descriptor = if let Some(session) = &live_session {
            session.descriptor().await
        } else {
            self.get_descriptor(session_id)
                .await?
                .ok_or_else(|| missing_session_error(session_id))?
        };
        if matches!(
            descriptor.status,
            InteractionSessionStatus::Running | InteractionSessionStatus::Paused
        ) {
            if !options.force_abort {
                return Err(InteractionError::busy(format!(
                    "session is not idle: {session_id}"
                )));
            }
            if let Some(session) = &live_session {
                session.agent.abort().await;
                session.agent.wait_for_idle().await;
            }
        }
        self.inner.sessions.write().await.remove(session_id);
        self.inner.store.remove(session_id).await?;
        self.inner.session_changes.notify_waiters();
        Ok(descriptor)
    }

    fn next_session_id(&self) -> String {
        let id = self.inner.counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("session-{id}")
    }

    fn next_automation_id(&self) -> String {
        let id = self.inner.counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("automation-{id}")
    }

    fn spawn_automation_runner_if_possible(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let registry = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            automation_runner_loop(registry).await;
        });
    }

    async fn validate_automation_target(
        &self,
        target: &AutomationTarget,
    ) -> Result<(), InteractionError> {
        match target {
            AutomationTarget::ExistingSession { session_id } => {
                self.get_descriptor(session_id)
                    .await?
                    .ok_or_else(|| missing_session_error(session_id))?;
            }
            AutomationTarget::NewSession {
                session_id: Some(session_id),
                profile_id,
            } => {
                if self.get_descriptor(session_id).await?.is_some() {
                    return Ok(());
                }
                let profile_id = profile_id
                    .as_deref()
                    .unwrap_or(self.inner.default_profile_id.as_str());
                if !self.inner.profiles.contains_key(profile_id) {
                    return Err(InteractionError::not_found(format!(
                        "profile not found: {profile_id}"
                    )));
                }
            }
            AutomationTarget::NewSession {
                session_id: None,
                profile_id,
            } => {
                let profile_id = profile_id
                    .as_deref()
                    .unwrap_or(self.inner.default_profile_id.as_str());
                if !self.inner.profiles.contains_key(profile_id) {
                    return Err(InteractionError::not_found(format!(
                        "profile not found: {profile_id}"
                    )));
                }
            }
        }
        Ok(())
    }

    async fn fire_automation_with_mode(
        &self,
        automation_id: &str,
        mode: AutomationFireMode,
    ) -> Result<AutomationRecord, InteractionError> {
        let _reservation = self.reserve_automation_fire(automation_id)?;
        let mut automation = self
            .inner
            .store
            .get_automation(automation_id)
            .await?
            .ok_or_else(|| missing_automation_error(automation_id))?;
        if automation.status != AutomationStatus::Active {
            return Err(InteractionError::invalid_params(format!(
                "automation is not active: {automation_id}"
            )));
        }
        let fired_at_ms = current_unix_ms();
        match self.deliver_automation(&mut automation, fired_at_ms).await {
            Ok(()) => {
                automation.last_fired_at_ms = Some(fired_at_ms);
                automation.last_error = None;
                if mode == AutomationFireMode::Triggered {
                    let after_fire = automation.trigger.after_fire(fired_at_ms);
                    automation.status = after_fire.status;
                    automation.next_fire_at_ms = after_fire.next_fire_at_ms;
                }
            }
            Err(error) => {
                automation.last_error = Some(error.to_string());
                if mode == AutomationFireMode::Triggered {
                    automation.next_fire_at_ms = retry_after_failure(&automation, fired_at_ms);
                }
            }
        }
        automation.mark_updated();
        self.inner.store.save_automation(automation.clone()).await?;
        self.inner.automation_changes.notify_waiters();
        Ok(automation)
    }

    fn reserve_automation_fire(
        &self,
        automation_id: &str,
    ) -> Result<AutomationFireReservation, InteractionError> {
        let mut firing = self
            .inner
            .firing_automations
            .lock()
            .expect("interaction firing automations lock poisoned");
        if !firing.insert(automation_id.to_owned()) {
            return Err(InteractionError::busy(format!(
                "automation is already firing: {automation_id}"
            )));
        }
        Ok(AutomationFireReservation {
            inner: Arc::clone(&self.inner),
            automation_id: automation_id.to_owned(),
        })
    }

    async fn deliver_automation(
        &self,
        automation: &mut AutomationRecord,
        fired_at_ms: u64,
    ) -> Result<(), InteractionError> {
        match &automation.target {
            AutomationTarget::ExistingSession { session_id } => {
                let registered = self
                    .get(session_id)
                    .await?
                    .ok_or_else(|| missing_session_error(session_id))?;
                let message = existing_session_automation_message(
                    automation,
                    fired_at_ms,
                    automation.prompt.clone(),
                );
                let status = registered.agent().state().await.status;
                if session_ready_for_direct_prompt(&status) {
                    registered
                        .agent()
                        .prompt(AgentInput::Message(message))
                        .await?;
                } else {
                    registered.agent().steer(message);
                    registered.save_snapshot().await?;
                }
                Ok(())
            }
            AutomationTarget::NewSession { .. } => {
                let registered = self.automation_target_session(automation).await?;
                let message =
                    super::automation_message(automation, fired_at_ms, automation.prompt.clone());
                let status = registered.agent().state().await.status;
                if session_ready_for_direct_prompt(&status) {
                    registered
                        .agent()
                        .prompt(AgentInput::Message(message))
                        .await?;
                } else {
                    registered.agent().steer(message);
                    registered.save_snapshot().await?;
                }
                Ok(())
            }
        }
    }

    async fn automation_target_session(
        &self,
        automation: &mut AutomationRecord,
    ) -> Result<Arc<RegisteredAgentSession>, InteractionError> {
        let (target_session_id, target_profile_id) = match &automation.target {
            AutomationTarget::NewSession {
                session_id,
                profile_id,
            } => (session_id.clone(), profile_id.clone()),
            AutomationTarget::ExistingSession { .. } => {
                unreachable!("automation_target_session only handles new-session targets")
            }
        };
        if let Some(session_id) = &target_session_id
            && let Some(registered) = self.get(session_id).await?
        {
            return Ok(registered);
        }
        let mut metadata = Map::new();
        metadata.insert(
            AUTOMATION_SESSION_METADATA_KEY.into(),
            automation_session_metadata(&automation.automation_id),
        );
        metadata.insert("type".into(), Value::String(AUTOMATION_SOURCE_TYPE.into()));
        let descriptor = self
            .create_session(AgentSessionCreateRequest {
                session_id: target_session_id,
                profile_id: target_profile_id,
                manifest: None,
                manifest_patches: vec![ManifestPatch::UpsertSystemPromptAddition {
                    addition: SystemPromptAddition::new(
                        AUTOMATION_SYSTEM_PROMPT_ADDITION_ID,
                        automation_identity_prompt(&automation.automation_id),
                    ),
                }],
                parent_session_id: None,
                role: Some("automation".into()),
                metadata,
            })
            .await?;
        automation.target = AutomationTarget::NewSession {
            session_id: Some(descriptor.session_id.clone()),
            profile_id: Some(descriptor.profile_id.clone()),
        };
        self.get(&descriptor.session_id)
            .await?
            .ok_or_else(|| missing_session_error(&descriptor.session_id))
    }

    async fn loaded_session(&self, session_id: &str) -> Option<Arc<RegisteredAgentSession>> {
        self.inner.sessions.read().await.get(session_id).cloned()
    }

    async fn registered_from_record(
        &self,
        record: AgentSessionRecord,
    ) -> Result<Arc<RegisteredAgentSession>, InteractionError> {
        record
            .validate_schema_version()
            .map_err(InteractionError::internal)?;
        let profile = self
            .inner
            .profiles
            .get(&record.profile_id)
            .cloned()
            .ok_or_else(|| {
                InteractionError::not_found(format!("profile not found: {}", record.profile_id))
            })?;
        let active_goal = self.inner.store.get_goal(&record.session_id).await?;
        let session = AgentSession::builder()
            .with_manifest(record.manifest.clone())
            .with_subagent_depth(subagent_depth_for_record(&record))
            .with_subagent_controller(Arc::new(RegistrySubagentController {
                registry: Arc::downgrade(&self.inner),
                parent_session_id: record.session_id.clone(),
            }))
            .with_goal_controller(Arc::new(RegistryGoalController {
                registry: Arc::downgrade(&self.inner),
                session_id: record.session_id.clone(),
            }))
            .with_active_goal(active_goal.filter(GoalRecord::is_pursuing))
            .build();
        let runtime = profile.build_runtime(&session, &record.manifest).await?;
        let agent = Agent::builder()
            .with_runtime(Arc::new(runtime))
            .with_initial_state(record.state.clone())
            .build()
            .map_err(InteractionError::from)?;
        restore_agent_queues(&agent, record.queues.clone());
        let registered = Arc::new(RegisteredAgentSession {
            record: Mutex::new(record),
            session,
            agent,
            store: Arc::clone(&self.inner.store),
        });
        attach_snapshot_listener(&registered);
        attach_goal_audit_listener(&registered);
        Ok(registered)
    }

    async fn normalize_record(
        &self,
        mut record: AgentSessionRecord,
    ) -> Result<AgentSessionRecord, InteractionError> {
        record
            .validate_schema_version()
            .map_err(InteractionError::internal)?;
        if !matches!(record.state.status, RunStatus::Running) {
            return Ok(record);
        }
        record.state.status = RunStatus::Failed;
        record.state.last_error = Some(INTERRUPTED_RUNNING_SESSION_ERROR.into());
        record.state.active_phase = None;
        record.updated_at_ms = current_unix_ms();
        self.inner.store.save(record.clone()).await?;
        Ok(record)
    }

    async fn set_goal_status(
        &self,
        session_id: &str,
        status: GoalStatus,
    ) -> Result<GoalRecord, InteractionError> {
        self.get_descriptor(session_id)
            .await?
            .ok_or_else(|| missing_session_error(session_id))?;
        let mut goal = self
            .inner
            .store
            .get_goal(session_id)
            .await?
            .ok_or_else(|| missing_goal_error(session_id))?;
        goal.status = status;
        if !goal.is_pursuing()
            && let Some(audit) = goal.last_audit.as_mut()
        {
            audit.pending = false;
        }
        goal.mark_updated();
        self.inner.store.save_goal(goal.clone()).await?;
        self.update_loaded_goal(session_id, Some(goal.clone()))
            .await;
        Ok(goal)
    }

    async fn update_loaded_goal(&self, session_id: &str, goal: Option<GoalRecord>) {
        if let Some(session) = self.loaded_session(session_id).await {
            session.session().set_active_goal_for_tools(goal);
        }
    }

    fn reserve_session_id(
        &self,
        session_id: &str,
    ) -> Result<CreateSessionReservation, InteractionError> {
        self.try_reserve_session_id(session_id)
            .ok_or_else(|| duplicate_session_error(session_id))
    }

    fn try_reserve_session_id(&self, session_id: &str) -> Option<CreateSessionReservation> {
        let mut creating = self
            .inner
            .creating_sessions
            .lock()
            .expect("interaction creating sessions lock poisoned");
        if !creating.insert(session_id.to_owned()) {
            return None;
        }
        Some(CreateSessionReservation {
            inner: Arc::clone(&self.inner),
            session_id: session_id.to_owned(),
        })
    }
}

#[derive(Clone)]
struct RegistryGoalController {
    registry: Weak<AgentSessionRegistryInner>,
    session_id: String,
}

impl GoalController for RegistryGoalController {
    fn update_goal<'a>(
        &'a self,
        request: ToolGoalUpdateRequest,
        run_id: String,
        turn_id: u64,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, GoalRecord> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let registry = self.registry()?;
            let request = GoalStatusUpdateRequest {
                session_id: self.session_id.clone(),
                status: request.status,
                summary: request.summary,
                evidence: request.evidence,
            };
            registry
                .update_goal_status(request, run_id, turn_id)
                .await
                .map_err(to_core_error)
        })
    }
}

impl RegistryGoalController {
    fn registry(&self) -> Result<AgentSessionRegistry, AgentCoreError> {
        let inner = self
            .registry
            .upgrade()
            .ok_or_else(|| AgentCoreError::Provider("goal registry is unavailable".into()))?;
        Ok(AgentSessionRegistry { inner })
    }
}

#[derive(Clone)]
struct RegistrySubagentController {
    registry: Weak<AgentSessionRegistryInner>,
    parent_session_id: String,
}

impl SubagentController for RegistrySubagentController {
    fn spawn_subagent<'a>(
        &'a self,
        request: ToolSubagentSpawnRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentSummary> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let registry = self.registry()?;
            let parent = registry
                .get_descriptor(&self.parent_session_id)
                .await
                .map_err(to_core_error)?
                .ok_or_else(|| {
                    AgentCoreError::Provider(format!(
                        "parent session not found: {}",
                        self.parent_session_id
                    ))
                })?;
            let ToolSubagentSpawnRequest {
                role,
                prompt,
                metadata,
            } = request;
            let manifest = subagent_manifest_from_parent(parent.manifest, &metadata);
            let prompt_id = format!("subagent-initial-{}", current_unix_ms());
            let descriptor = registry
                .spawn_subagent(SubagentSpawnRequest {
                    parent_session_id: self.parent_session_id.clone(),
                    profile_id: Some(parent.profile_id),
                    manifest: Some(manifest),
                    manifest_patches: Vec::new(),
                    role,
                    metadata,
                    initial_prompt: Some(AgentMessage::user(prompt_id, prompt)),
                })
                .await
                .map_err(to_core_error)?;
            Ok(summary_from_descriptor(&descriptor))
        })
    }

    fn wait_subagents<'a>(
        &'a self,
        session_ids: Vec<String>,
        timeout_ms: u64,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentWaitOutcome> {
        Box::pin(async move {
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            loop {
                cancellation.throw_if_cancelled()?;
                let results = self.subagent_results(session_ids.iter()).await?;
                if results.iter().all(|result| result.settled) {
                    return Ok(SubagentWaitOutcome {
                        timed_out: false,
                        results,
                    });
                }
                if Instant::now() >= deadline {
                    return Ok(SubagentWaitOutcome {
                        timed_out: true,
                        results,
                    });
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                let delay = remaining.min(Duration::from_millis(50));
                tokio::select! {
                    _ = sleep(delay) => {}
                    _ = cancellation.cancelled() => return Err(AgentCoreError::Aborted),
                }
            }
        })
    }

    fn subagent_result<'a>(
        &'a self,
        session_id: String,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, SubagentResult> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            self.subagent_result_for(&session_id).await
        })
    }

    fn list_subagents<'a>(
        &'a self,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<SubagentSummary>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let registry = self.registry()?;
            let descriptors = registry
                .list(AgentSessionListFilter {
                    parent_session_id: Some(self.parent_session_id.clone()),
                    ..AgentSessionListFilter::default()
                })
                .await
                .map_err(to_core_error)?;
            Ok(descriptors
                .iter()
                .map(summary_from_descriptor)
                .collect::<Vec<_>>())
        })
    }
}

impl RegistrySubagentController {
    fn registry(&self) -> Result<AgentSessionRegistry, AgentCoreError> {
        let inner = self
            .registry
            .upgrade()
            .ok_or_else(|| AgentCoreError::Provider("subagent registry is unavailable".into()))?;
        Ok(AgentSessionRegistry { inner })
    }

    async fn subagent_results<'a>(
        &self,
        session_ids: impl IntoIterator<Item = &'a String>,
    ) -> Result<Vec<SubagentResult>, AgentCoreError> {
        let mut results = Vec::new();
        for session_id in session_ids {
            results.push(self.subagent_result_for(session_id).await?);
        }
        Ok(results)
    }

    async fn subagent_result_for(
        &self,
        session_id: &str,
    ) -> Result<SubagentResult, AgentCoreError> {
        let registry = self.registry()?;
        let descriptor = registry
            .get_descriptor(session_id)
            .await
            .map_err(to_core_error)?
            .ok_or_else(|| subagent_access_error(session_id, &self.parent_session_id))?;
        if descriptor.parent_session_id.as_deref() != Some(self.parent_session_id.as_str()) {
            return Err(subagent_access_error(session_id, &self.parent_session_id));
        }
        let summary = summary_from_descriptor(&descriptor);
        let settled = descriptor.status.is_settled();
        let final_output = settled
            .then(|| final_assistant_output(&descriptor.state))
            .flatten();
        Ok(SubagentResult {
            summary,
            settled,
            final_output,
        })
    }
}

fn summary_from_descriptor(descriptor: &InteractionSessionDescriptor) -> SubagentSummary {
    SubagentSummary {
        session_id: descriptor.session_id.clone(),
        role: descriptor.role.clone(),
        status: descriptor.status.as_str().into(),
    }
}

fn subagent_depth_for_record(record: &AgentSessionRecord) -> usize {
    usize::from(record.parent_session_id.is_some())
}

fn subagent_manifest_from_parent(
    mut manifest: AgentManifest,
    metadata: &serde_json::Map<String, serde_json::Value>,
) -> AgentManifest {
    let inherit_prompt_additions = metadata
        .get(SUBAGENT_INHERIT_PROMPT_ADDITIONS_METADATA)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let additions = manifest.system_prompt.additions_mut();
    if inherit_prompt_additions {
        additions.retain(|addition| !addition.id.starts_with(TRANSIENT_PROMPT_ADDITION_PREFIX));
    } else {
        additions.clear();
    }
    manifest
}

fn subagent_access_error(session_id: &str, parent_session_id: &str) -> AgentCoreError {
    AgentCoreError::Provider(format!(
        "subagent `{session_id}` was not found as a direct child of `{parent_session_id}`"
    ))
}

fn to_core_error(error: InteractionError) -> AgentCoreError {
    AgentCoreError::Provider(error.to_string())
}

fn missing_goal_error(session_id: &str) -> InteractionError {
    InteractionError::not_found(format!("goal not found for session: {session_id}"))
}

fn retry_after_failure(automation: &AutomationRecord, fired_at_ms: u64) -> Option<u64> {
    match automation.trigger.after_fire(fired_at_ms).next_fire_at_ms {
        Some(next_fire_at_ms) => Some(next_fire_at_ms),
        None => Some(fired_at_ms.saturating_add(60_000)),
    }
}

async fn automation_runner_loop(registry: Weak<AgentSessionRegistryInner>) {
    loop {
        let Some(inner) = registry.upgrade() else {
            return;
        };
        let notify = Arc::clone(&inner.automation_changes);
        let now_ms = current_unix_ms();
        let automations = match inner.store.list_automations().await {
            Ok(automations) => automations,
            Err(_) => {
                drop(inner);
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let registry_handle = AgentSessionRegistry {
            inner: Arc::clone(&inner),
        };
        let mut nearest_fire_at_ms = None;
        for automation in automations {
            if automation.status != AutomationStatus::Active {
                continue;
            }
            let Some(next_fire_at_ms) = automation.next_fire_at_ms else {
                continue;
            };
            if next_fire_at_ms <= now_ms {
                let _ = registry_handle
                    .fire_automation_with_mode(
                        &automation.automation_id,
                        AutomationFireMode::Triggered,
                    )
                    .await;
            } else {
                nearest_fire_at_ms = Some(
                    nearest_fire_at_ms
                        .map(|current: u64| current.min(next_fire_at_ms))
                        .unwrap_or(next_fire_at_ms),
                );
            }
        }
        let now_ms = current_unix_ms();
        let delay_ms = nearest_fire_at_ms
            .map(|fire_at| fire_at.saturating_sub(now_ms).clamp(10, 60_000))
            .unwrap_or(60_000);
        drop(registry_handle);
        drop(inner);
        tokio::select! {
            _ = sleep(Duration::from_millis(delay_ms)) => {}
            _ = notify.notified() => {}
        }
    }
}

fn attach_snapshot_listener(registered: &Arc<RegisteredAgentSession>) {
    let weak = Arc::downgrade(registered);
    registered.agent.subscribe(move |event| {
        let weak = Weak::clone(&weak);
        async move {
            if !event_requires_snapshot(&event) {
                return Ok(());
            }
            if let Some(registered) = weak.upgrade() {
                registered
                    .save_snapshot()
                    .await
                    .map_err(|error| AgentCoreError::Provider(error.to_string()))?;
            }
            Ok(())
        }
    });
}

fn attach_goal_audit_listener(registered: &Arc<RegisteredAgentSession>) {
    let weak = Arc::downgrade(registered);
    registered.agent.subscribe(move |event| {
        let weak = Weak::clone(&weak);
        async move {
            let AgentEventKind::TurnCompleted { .. } = event.kind else {
                return Ok(());
            };
            let Some(turn_id) = event.turn_id else {
                return Ok(());
            };
            let Some(registered) = weak.upgrade() else {
                return Ok(());
            };
            let session_id = registered.record().session_id;
            let Some(mut goal) = registered
                .store
                .get_goal(&session_id)
                .await
                .map_err(|error| AgentCoreError::Provider(error.to_string()))?
            else {
                registered.session().set_active_goal_for_tools(None);
                return Ok(());
            };
            if !goal.is_pursuing() {
                registered.session().set_active_goal_for_tools(Some(goal));
                return Ok(());
            }
            if let Some(audit) = goal.last_audit.as_mut() {
                if audit.pending {
                    audit.pending = false;
                    goal.mark_updated();
                    registered
                        .store
                        .save_goal(goal.clone())
                        .await
                        .map_err(|error| AgentCoreError::Provider(error.to_string()))?;
                    registered.session().set_active_goal_for_tools(Some(goal));
                    return Ok(());
                }
                if audit.run_id == event.run_id && audit.turn_id == turn_id {
                    return Ok(());
                }
            }
            goal.last_audit = Some(GoalAuditRecord {
                reason: GOAL_AUDIT_REASON_TURN_END.into(),
                run_id: event.run_id.clone(),
                turn_id,
                pending: true,
                summary: None,
                evidence: None,
                audited_at_ms: current_unix_ms(),
            });
            goal.mark_updated();
            registered
                .store
                .save_goal(goal.clone())
                .await
                .map_err(|error| AgentCoreError::Provider(error.to_string()))?;
            registered
                .session()
                .set_active_goal_for_tools(Some(goal.clone()));
            registered
                .agent()
                .steer(goal_audit_message(&goal, &event.run_id, turn_id));
            registered
                .save_snapshot()
                .await
                .map_err(|error| AgentCoreError::Provider(error.to_string()))?;
            Ok(())
        }
    });
}

fn event_requires_snapshot(event: &AgentEvent) -> bool {
    matches!(
        event.kind,
        AgentEventKind::RunStarted
            | AgentEventKind::RunCompleted
            | AgentEventKind::RunAborted
            | AgentEventKind::RunFailed { .. }
            | AgentEventKind::RunPaused { .. }
            | AgentEventKind::RunResumed { .. }
            | AgentEventKind::TurnCompleted { .. }
            | AgentEventKind::PhaseStarted { .. }
            | AgentEventKind::PhaseCompleted { .. }
            | AgentEventKind::PhaseFailed { .. }
            | AgentEventKind::EffectCommitted { .. }
            | AgentEventKind::ToolApprovalRequested { .. }
            | AgentEventKind::ToolApprovalResolved { .. }
            | AgentEventKind::ToolApprovalExpired { .. }
    )
}

fn descriptor_from_record(record: &AgentSessionRecord) -> InteractionSessionDescriptor {
    InteractionSessionDescriptor {
        session_id: record.session_id.clone(),
        profile_id: record.profile_id.clone(),
        parent_session_id: record.parent_session_id.clone(),
        role: record.role.clone(),
        status: InteractionSessionStatus::from(record.state.status.clone()),
        manifest: record.manifest.clone(),
        state: record.state.clone(),
        metadata: record.metadata.clone(),
    }
}

fn queue_snapshot_from_agent(agent: &Agent) -> AgentSessionQueueSnapshot {
    AgentSessionQueueSnapshot {
        steering: AgentSessionQueueState::from_core(
            agent.steering_queue_mode(),
            agent.queued_steering_messages(),
        ),
        follow_up: AgentSessionQueueState::from_core(
            agent.follow_up_queue_mode(),
            agent.queued_follow_up_messages(),
        ),
    }
}

fn restore_agent_queues(agent: &Agent, queues: AgentSessionQueueSnapshot) {
    let steering = queues.steering;
    agent.set_steering_mode(steering.mode);
    agent.edit_steering_queue(|queue| {
        *queue = steering.into_core_messages();
    });

    let follow_up = queues.follow_up;
    agent.set_follow_up_mode(follow_up.mode);
    agent.edit_follow_up_queue(|queue| {
        *queue = follow_up.into_core_messages();
    });
}

fn manifest_for_request(
    manifest: Option<AgentManifest>,
    profile: &InteractionProfileDescriptor,
    manifest_patches: &[ManifestPatch],
) -> Result<AgentManifest, InteractionError> {
    let mut manifest = match manifest {
        Some(manifest) => manifest,
        None => {
            let mut manifest = AgentManifest::default();
            for patch in profile.default_manifest_patches.iter().cloned() {
                manifest.apply_patch(patch).map_err(|error| {
                    InteractionError::invalid_params(format!(
                        "profile {} default manifest patch failed: {error}",
                        profile.profile_id
                    ))
                })?;
            }
            manifest
        }
    };

    for patch in manifest_patches.iter().cloned() {
        manifest.apply_patch(patch).map_err(|error| {
            InteractionError::invalid_params(format!(
                "session manifest patch failed for profile {}: {error}",
                profile.profile_id,
            ))
        })?;
    }

    manifest
        .validate()
        .map_err(|error| InteractionError::invalid_params(error.to_string()))?;
    Ok(manifest)
}

fn record_filter_matches(record: &AgentSessionRecord, filter: &AgentSessionListFilter) -> bool {
    if filter
        .parent_session_id
        .as_ref()
        .is_some_and(|parent| record.parent_session_id.as_ref() != Some(parent))
    {
        return false;
    }
    if filter
        .profile_id
        .as_ref()
        .is_some_and(|profile| &record.profile_id != profile)
    {
        return false;
    }
    true
}

fn status_filter_matches(
    descriptor: &InteractionSessionDescriptor,
    filter: &AgentSessionListFilter,
) -> bool {
    if filter
        .status
        .as_ref()
        .is_some_and(|status| &descriptor.status != status)
    {
        return false;
    }
    true
}

impl From<RunStatus> for AgentSessionListFilter {
    fn from(status: RunStatus) -> Self {
        Self {
            status: Some(InteractionSessionStatus::from(status)),
            ..Self::default()
        }
    }
}

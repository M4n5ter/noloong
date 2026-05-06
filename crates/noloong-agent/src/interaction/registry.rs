use super::{
    AgentRuntimeProfile, InteractionError, InteractionProfileDescriptor,
    InteractionSessionDescriptor, InteractionSessionStatus,
    store::{
        AGENT_SESSION_RECORD_SCHEMA_VERSION, AgentSessionQueueSnapshot, AgentSessionQueueState,
        AgentSessionRecord, AgentSessionRegistryStore, InMemoryAgentSessionRegistryStore,
        current_unix_ms, duplicate_session_error, missing_session_error,
    },
};
use crate::{AgentManifest, AgentSession};
use noloong_agent_core::{
    Agent, AgentCoreError, AgentEvent, AgentEventKind, AgentMessage, AgentState, RunStatus,
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

const INTERRUPTED_RUNNING_SESSION_ERROR: &str =
    "agent session was interrupted while running and cannot be resumed automatically";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<AgentManifest>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<AgentMessage>,
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
    session_changes: Notify,
    counter: AtomicU64,
}

struct CreateSessionReservation {
    inner: Arc<AgentSessionRegistryInner>,
    session_id: String,
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
        Ok(Self {
            inner: Arc::new(AgentSessionRegistryInner {
                profiles: profiles_by_id,
                default_profile_id,
                store,
                sessions: RwLock::new(BTreeMap::new()),
                creating_sessions: Mutex::new(BTreeSet::new()),
                session_changes: Notify::new(),
                counter: AtomicU64::new(0),
            }),
        })
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
        let manifest = manifest_for_request(request.manifest.clone(), &profile_descriptor)?;
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
        let session = AgentSession::builder()
            .with_manifest(record.manifest.clone())
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
) -> Result<AgentManifest, InteractionError> {
    let Some(manifest) = manifest else {
        let mut manifest = AgentManifest::default();
        for patch in profile.default_manifest_patches.iter().cloned() {
            manifest.apply_patch(patch).map_err(|error| {
                InteractionError::invalid_params(format!(
                    "profile {} default manifest patch failed: {error}",
                    profile.profile_id
                ))
            })?;
        }
        return Ok(manifest);
    };
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

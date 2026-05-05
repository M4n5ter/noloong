use super::{
    AgentRuntimeProfile, InteractionError, InteractionFuture, InteractionProfileDescriptor,
    InteractionSessionDescriptor, InteractionSessionStatus,
};
use crate::{AgentManifest, AgentSession};
use noloong_agent_core::{Agent, AgentMessage, RunStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionRecord {
    pub session_id: String,
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub manifest: AgentManifest,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

pub trait AgentSessionRegistryStore: Send + Sync {
    fn upsert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()>;

    fn remove<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, ()>;

    fn get<'a>(&'a self, session_id: &'a str) -> InteractionFuture<'a, Option<AgentSessionRecord>>;

    fn list<'a>(&'a self) -> InteractionFuture<'a, Vec<AgentSessionRecord>>;
}

#[derive(Clone, Default)]
pub struct InMemoryAgentSessionRegistryStore {
    records: Arc<Mutex<BTreeMap<String, AgentSessionRecord>>>,
}

impl AgentSessionRegistryStore for InMemoryAgentSessionRegistryStore {
    fn upsert<'a>(&'a self, record: AgentSessionRecord) -> InteractionFuture<'a, ()> {
        Box::pin(async move {
            self.records
                .lock()
                .expect("interaction session store lock poisoned")
                .insert(record.session_id.clone(), record);
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

#[derive(Clone)]
pub struct RegisteredAgentSession {
    record: AgentSessionRecord,
    session: AgentSession,
    agent: Agent,
}

impl RegisteredAgentSession {
    pub fn record(&self) -> &AgentSessionRecord {
        &self.record
    }

    pub fn session(&self) -> &AgentSession {
        &self.session
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    pub async fn descriptor(&self) -> InteractionSessionDescriptor {
        let state = self.agent.state().await;
        let mut descriptor = descriptor_from_record(&self.record, state);
        descriptor.manifest = self.session.manifest();
        descriptor
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
                counter: AtomicU64::new(0),
            }),
        })
    }

    pub fn profile_descriptors(&self) -> Vec<super::InteractionProfileDescriptor> {
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
            return Err(InteractionError::invalid_params(format!(
                "session already exists: {session_id}"
            )));
        }
        if self.inner.store.get(&session_id).await?.is_some() {
            return Err(InteractionError::invalid_params(format!(
                "session already exists: {session_id}"
            )));
        }

        let session = AgentSession::builder()
            .with_manifest(manifest.clone())
            .build();
        let runtime = profile.build_runtime(&session, &manifest).await?;
        let agent = Agent::builder()
            .with_runtime(Arc::new(runtime))
            .build()
            .map_err(InteractionError::from)?;
        let record = AgentSessionRecord {
            session_id: session_id.clone(),
            profile_id,
            parent_session_id: request.parent_session_id,
            role: request.role,
            manifest,
            metadata: request.metadata,
        };
        let registered = Arc::new(RegisteredAgentSession {
            record: record.clone(),
            session,
            agent,
        });
        self.inner.store.upsert(record).await?;
        self.inner
            .sessions
            .write()
            .await
            .insert(session_id, registered.clone());
        Ok(registered.descriptor().await)
    }

    pub async fn spawn_subagent(
        &self,
        request: SubagentSpawnRequest,
    ) -> Result<InteractionSessionDescriptor, InteractionError> {
        let parent = self.get(&request.parent_session_id).await?.ok_or_else(|| {
            InteractionError::not_found(format!(
                "parent session not found: {}",
                request.parent_session_id
            ))
        })?;
        let descriptor = self
            .create_session(AgentSessionCreateRequest {
                session_id: None,
                profile_id: request
                    .profile_id
                    .or_else(|| Some(parent.record.profile_id.clone())),
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

    pub async fn list(&self, filter: AgentSessionListFilter) -> Vec<InteractionSessionDescriptor> {
        let sessions = self
            .inner
            .sessions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut descriptors = Vec::new();
        for session in sessions {
            if !record_filter_matches(session.record(), &filter) {
                continue;
            }
            let descriptor = session.descriptor().await;
            if !status_filter_matches(&descriptor, &filter) {
                continue;
            }
            descriptors.push(descriptor);
        }
        descriptors
    }

    pub async fn get(
        &self,
        session_id: &str,
    ) -> Result<Option<Arc<RegisteredAgentSession>>, InteractionError> {
        if let Some(session) = self.inner.sessions.read().await.get(session_id).cloned() {
            return Ok(Some(session));
        }
        let Some(record) = self.inner.store.get(session_id).await? else {
            return Ok(None);
        };
        Err(InteractionError::internal(format!(
            "session {} exists in store but is not loaded",
            record.session_id
        )))
    }

    pub async fn get_descriptor(
        &self,
        session_id: &str,
    ) -> Result<Option<InteractionSessionDescriptor>, InteractionError> {
        let Some(session) = self.get(session_id).await? else {
            return Ok(None);
        };
        Ok(Some(session.descriptor().await))
    }

    pub async fn delete_session(
        &self,
        session_id: &str,
        options: AgentSessionDeleteOptions,
    ) -> Result<InteractionSessionDescriptor, InteractionError> {
        let session = self.get(session_id).await?.ok_or_else(|| {
            InteractionError::not_found(format!("session not found: {session_id}"))
        })?;
        let descriptor = session.descriptor().await;
        if matches!(
            descriptor.status,
            InteractionSessionStatus::Running | InteractionSessionStatus::Paused
        ) {
            if !options.force_abort {
                return Err(InteractionError::busy(format!(
                    "session is not idle: {session_id}"
                )));
            }
            session.agent.abort().await;
            session.agent.wait_for_idle().await;
        }
        self.inner.sessions.write().await.remove(session_id);
        self.inner.store.remove(session_id).await?;
        Ok(descriptor)
    }

    fn next_session_id(&self) -> String {
        let id = self.inner.counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("session-{id}")
    }

    fn reserve_session_id(
        &self,
        session_id: &str,
    ) -> Result<CreateSessionReservation, InteractionError> {
        let mut creating = self
            .inner
            .creating_sessions
            .lock()
            .expect("interaction creating sessions lock poisoned");
        if !creating.insert(session_id.to_owned()) {
            return Err(InteractionError::invalid_params(format!(
                "session already exists: {session_id}"
            )));
        }
        Ok(CreateSessionReservation {
            inner: Arc::clone(&self.inner),
            session_id: session_id.to_owned(),
        })
    }
}

fn descriptor_from_record(
    record: &AgentSessionRecord,
    state: noloong_agent_core::AgentState,
) -> InteractionSessionDescriptor {
    InteractionSessionDescriptor {
        session_id: record.session_id.clone(),
        profile_id: record.profile_id.clone(),
        parent_session_id: record.parent_session_id.clone(),
        role: record.role.clone(),
        status: InteractionSessionStatus::from(state.status.clone()),
        manifest: record.manifest.clone(),
        state,
        metadata: record.metadata.clone(),
    }
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

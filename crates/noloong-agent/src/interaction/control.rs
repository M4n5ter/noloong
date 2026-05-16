use super::{
    AgentSessionDeleteOptions, AgentSessionListFilter, AgentSessionRegistry,
    AutomationCreateRequest, AutomationListRequest, AutomationRequest, AutomationUpdateRequest,
    DisplayEvent, GoalSetRequest, GoalStatusUpdateRequest, InteractionAuthorityCapability,
    InteractionCapabilityGrant, InteractionCapabilityPolicy, InteractionClientInfo,
    InteractionError, InteractionFuture, InteractionNotifier, InteractionProfileDescriptor,
    InteractionSessionDescriptor, InteractionUxCapabilities, JsonRpcHandler, JsonRpcHandlerOutput,
    SubagentSpawnRequest, store::missing_session_error,
};
use crate::{ReadOutputRequest, text};
use noloong_agent_core::{
    AgentEvent, AgentEventKind, AgentInput, AgentMessage, ContentBlock, ModelStreamEvent,
    QueueMode, QueuedAgentMessage, QueuedMessageIntent, ToolApprovalId, ToolApprovalResolution,
    ToolPermissionDecision,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub const RAW_EVENT_NOTIFICATION: &str = "agent/event";
pub const DISPLAY_EVENT_NOTIFICATION: &str = "display/event";
pub const EVENT_SUBSCRIBE_METHOD: &str = "event/subscribe";
pub const DISPLAY_SUBSCRIBE_METHOD: &str = "display/subscribe";

#[derive(Clone)]
pub struct InteractionControlHandler {
    inner: Arc<InteractionControlHandlerInner>,
    client: Arc<InteractionControlClientState>,
}

struct InteractionControlHandlerInner {
    registry: AgentSessionRegistry,
    policy: InteractionCapabilityPolicy,
    subscription_counter: AtomicU64,
}

struct InteractionControlClientState {
    grant: Mutex<Option<InteractionCapabilityGrant>>,
    subscriptions: Mutex<BTreeMap<String, InteractionSubscription>>,
}

struct InteractionSubscription {
    session_id: String,
    listener_id: u64,
}

impl InteractionControlHandler {
    pub fn new(registry: AgentSessionRegistry, policy: InteractionCapabilityPolicy) -> Self {
        Self {
            inner: Arc::new(InteractionControlHandlerInner {
                registry,
                policy,
                subscription_counter: AtomicU64::new(0),
            }),
            client: Arc::new(InteractionControlClientState {
                grant: Mutex::new(None),
                subscriptions: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub fn registry(&self) -> &AgentSessionRegistry {
        &self.inner.registry
    }

    fn new_connection(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            client: Arc::new(InteractionControlClientState {
                grant: Mutex::new(None),
                subscriptions: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    fn initialize(&self, params: Value) -> Result<Value, InteractionError> {
        let client = parse_params::<InteractionClientInfo>(params)?;
        let grant = self.inner.policy.grant(&client);
        *self
            .client
            .grant
            .lock()
            .expect("interaction capability grant lock poisoned") = Some(grant.clone());
        value(InitializeResult {
            server: InteractionServerInfo {
                name: "noloong-agent".into(),
                protocol_version: "2026-05-05".into(),
            },
            grant,
            profiles: self.inner.registry.profile_descriptors(),
        })
    }

    fn require(
        &self,
        method: &str,
        capability: InteractionAuthorityCapability,
    ) -> Result<(), InteractionError> {
        let grant = self
            .client
            .grant
            .lock()
            .expect("interaction capability grant lock poisoned")
            .clone()
            .unwrap_or_default();
        InteractionCapabilityPolicy::authorize(&grant, method, capability)
    }

    fn require_ux(
        &self,
        method: &str,
        capability: &str,
        allowed: impl FnOnce(&InteractionUxCapabilities) -> bool,
    ) -> Result<(), InteractionError> {
        let ux = self.granted_ux();
        if allowed(&ux) {
            return Ok(());
        }
        Err(InteractionError::unauthorized(
            method,
            capability,
            format!("method {method} requires {capability}"),
        ))
    }

    fn granted_ux(&self) -> InteractionUxCapabilities {
        self.client
            .grant
            .lock()
            .expect("interaction capability grant lock poisoned")
            .as_ref()
            .map(|grant| grant.ux.clone())
            .unwrap_or_default()
    }

    fn next_subscription_id(&self) -> String {
        let id = self
            .inner
            .subscription_counter
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        format!("subscription-{id}")
    }
}

impl JsonRpcHandler for InteractionControlHandler {
    fn connection_handler(&self) -> Self {
        self.new_connection()
    }

    fn handle<'a>(
        &'a self,
        method: &'a str,
        params: Value,
        notifier: InteractionNotifier,
    ) -> InteractionFuture<'a, JsonRpcHandlerOutput> {
        Box::pin(async move {
            let result = match method {
                "initialize" => self.initialize(params)?,
                "shutdown" => return Ok(JsonRpcHandlerOutput::shutdown(json!({"ok": true}))),
                "profile/list" => value(self.inner.registry.profile_descriptors())?,
                "session/create" => {
                    let request = parse_params(params)?;
                    value(self.inner.registry.create_session(request).await?)?
                }
                "session/list" => {
                    let request = parse_params::<AgentSessionListFilter>(params)?;
                    value(self.inner.registry.list(request).await?)?
                }
                "session/get" => {
                    let request = parse_params::<SessionRequest>(params)?;
                    let descriptor = self.session_descriptor(&request.session_id).await?;
                    value(descriptor)?
                }
                "session/delete" => {
                    self.require(method, InteractionAuthorityCapability::SessionDelete)?;
                    let request = parse_params::<SessionDeleteRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .delete_session(
                                &request.session_id,
                                AgentSessionDeleteOptions {
                                    force_abort: request.force_abort,
                                },
                            )
                            .await?,
                    )?
                }
                "subagent/spawn" => {
                    self.require(method, InteractionAuthorityCapability::SubagentSpawn)?;
                    let request = parse_params::<SubagentSpawnRequest>(params)?;
                    value(self.inner.registry.spawn_subagent(request).await?)?
                }
                "goal/set" => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<GoalSetRequest>(params)?;
                    value(self.inner.registry.set_goal(request).await?)?
                }
                "goal/get" => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(self.inner.registry.get_goal(&request.session_id).await?)?
                }
                "goal/pause" => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    value(self.inner.registry.pause_goal(&request.session_id).await?)?
                }
                "goal/resume" => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    value(self.inner.registry.resume_goal(&request.session_id).await?)?
                }
                "goal/clear" => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    value(self.inner.registry.clear_goal(&request.session_id).await?)?
                }
                "goal/update" => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<GoalStatusUpdateRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .update_goal_status(request, String::new(), 0)
                            .await?,
                    )?
                }
                "automation/create" => {
                    self.require(method, InteractionAuthorityCapability::AutomationManage)?;
                    let request = parse_params::<AutomationCreateRequest>(params)?;
                    value(self.inner.registry.create_automation(request).await?)?
                }
                "automation/get" => {
                    let request = parse_params::<AutomationRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .get_automation(&request.automation_id)
                            .await?,
                    )?
                }
                "automation/list" => {
                    let request = parse_params::<AutomationListRequest>(params)?;
                    value(self.inner.registry.list_automations(request).await?)?
                }
                "automation/update" => {
                    self.require(method, InteractionAuthorityCapability::AutomationManage)?;
                    let request = parse_params::<AutomationUpdateRequest>(params)?;
                    value(self.inner.registry.update_automation(request).await?)?
                }
                "automation/delete" => {
                    self.require(method, InteractionAuthorityCapability::AutomationManage)?;
                    let request = parse_params::<AutomationRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .delete_automation(&request.automation_id)
                            .await?,
                    )?
                }
                "automation/fire" => {
                    self.require(method, InteractionAuthorityCapability::AutomationManage)?;
                    let request = parse_params::<AutomationRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .fire_automation(&request.automation_id)
                            .await?,
                    )?
                }
                "agent/prompt" => {
                    self.require(method, InteractionAuthorityCapability::AgentRun)?;
                    let request = parse_params::<AgentPromptRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered
                        .agent()
                        .prompt(request.input.into_agent_input())
                        .await?;
                    value(registered.descriptor().await)?
                }
                "agent/continue" => {
                    self.require(method, InteractionAuthorityCapability::AgentRun)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered.agent().continue_run().await?;
                    value(registered.descriptor().await)?
                }
                "agent/abort" => {
                    self.require(method, InteractionAuthorityCapability::AgentRun)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered.agent().abort().await;
                    value(registered.descriptor().await)?
                }
                "agent/wait_idle" => {
                    self.require(method, InteractionAuthorityCapability::AgentRun)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered.agent().wait_for_idle().await;
                    value(registered.descriptor().await)?
                }
                "agent/state" => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .agent()
                            .state()
                            .await,
                    )?
                }
                "agent/steer" => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<AgentSteerRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    let message = request.message;
                    match request.intent.unwrap_or_default() {
                        InteractionQueuedMessageIntent::Observation => {
                            registered.agent().steer(message)
                        }
                        InteractionQueuedMessageIntent::UserInput => {
                            registered.agent().steer_user_input(message)
                        }
                    }
                    value(registered.save_snapshot().await?)?
                }
                "agent/follow_up" => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<AgentFollowUpRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered.agent().follow_up(request.message);
                    value(registered.save_snapshot().await?)?
                }
                "queue/list" => {
                    let request = parse_params::<QueueRequest>(params)?;
                    value(queue_messages(
                        self.session(&request.session_id).await?.agent(),
                        request.queue,
                    ))?
                }
                "queue/edit" => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<QueueEditRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    replace_queue(
                        registered.agent(),
                        request.queue,
                        request
                            .messages
                            .into_iter()
                            .map(InteractionQueuedMessage::into_core)
                            .collect(),
                    );
                    registered.save_snapshot().await?;
                    value(queue_messages(registered.agent(), request.queue))?
                }
                "queue/clear" => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<QueueRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    clear_queue(registered.agent(), request.queue);
                    registered.save_snapshot().await?;
                    value(queue_messages(registered.agent(), request.queue))?
                }
                "queue/set_mode" => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<QueueSetModeRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    set_queue_mode(registered.agent(), request.queue, request.mode);
                    registered.save_snapshot().await?;
                    value(queue_messages(registered.agent(), request.queue))?
                }
                EVENT_SUBSCRIBE_METHOD => {
                    self.require_ux(method, "rawEvents", |ux| ux.raw_events)?;
                    let request = parse_params::<EventSubscribeRequest>(params)?;
                    value(self.subscribe_raw(request, notifier).await?)?
                }
                "event/unsubscribe" => {
                    let request = parse_params::<EventUnsubscribeRequest>(params)?;
                    value(self.unsubscribe(request.subscription_id).await?)?
                }
                DISPLAY_SUBSCRIBE_METHOD => {
                    self.require_ux(method, "displayEvents", |ux| ux.display_events)?;
                    let request = parse_params::<DisplaySubscribeRequest>(params)?;
                    let granted_ux = self.granted_ux();
                    let ux = request
                        .ux
                        .map(|requested| granted_ux.grant(&requested))
                        .unwrap_or(granted_ux);
                    value(
                        self.subscribe_display(request.session_id, ux, notifier)
                            .await?,
                    )?
                }
                "approval/list" => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .agent()
                            .pending_tool_approvals()
                            .await,
                    )?
                }
                "approval/resolve" => {
                    self.require(method, InteractionAuthorityCapability::ApprovalResolve)?;
                    let request = parse_params::<ApprovalResolveRequest>(params)?;
                    value(self.resolve_approval(request).await?)?
                }
                "approval/resume_timeouts" => {
                    self.require(method, InteractionAuthorityCapability::ApprovalResolve)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered
                        .agent()
                        .resume_due_tool_approval_timeouts()
                        .await?;
                    value(registered.descriptor().await)?
                }
                "manifest/get" => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .manifest(),
                    )?
                }
                "manifest/system_prompt/get" => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .resolved_system_prompt(),
                    )?
                }
                "manifest/proposals/list" => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .proposal_store()
                            .pending_proposals(),
                    )?
                }
                "manifest/proposals/approve" => {
                    self.require(method, InteractionAuthorityCapability::ManifestApply)?;
                    let request = parse_params::<ManifestProposalRequest>(params)?;
                    let proposal = self
                        .session(&request.session_id)
                        .await?
                        .session()
                        .proposal_store()
                        .approve_proposal(&request.proposal_id)
                        .map_err(|error| InteractionError::not_found(error.to_string()))?;
                    value(proposal)?
                }
                "manifest/apply_approved" => {
                    self.require(method, InteractionAuthorityCapability::ManifestApply)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    let applied = registered.session().apply_approved_manifest_patches()?;
                    registered.save_snapshot().await?;
                    value(ManifestApplyResult {
                        applied_proposal_ids: applied,
                    })?
                }
                "process/list" => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .process_manager()
                            .list()
                            .await?,
                    )?
                }
                "process/read" => {
                    let request = parse_params::<ProcessReadRpcRequest>(params)?;
                    let output = self
                        .session(&request.session_id)
                        .await?
                        .session()
                        .process_manager()
                        .read(
                            &request.job_id,
                            ReadOutputRequest {
                                after_seq: request.after_seq,
                                max_bytes: request.max_bytes,
                                wait_ms: request.wait_ms,
                            },
                        )
                        .await?;
                    value(output)?
                }
                "process/wait" => {
                    self.require(method, InteractionAuthorityCapability::ProcessControl)?;
                    let request = parse_params::<ProcessWaitRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .process_manager()
                            .wait(&request.job_id, request.timeout_ms)
                            .await?,
                    )?
                }
                "process/write" => {
                    self.require(method, InteractionAuthorityCapability::ProcessControl)?;
                    let request = parse_params::<ProcessWriteRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .process_manager()
                            .write(&request.job_id, &request.text)
                            .await?,
                    )?
                }
                "process/terminate" => {
                    self.require(method, InteractionAuthorityCapability::ProcessControl)?;
                    let request = parse_params::<ProcessJobRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .process_manager()
                            .terminate(&request.job_id)
                            .await?,
                    )?
                }
                other => return Err(InteractionError::method_not_found(other)),
            };
            Ok(JsonRpcHandlerOutput::result(result))
        })
    }
}

impl InteractionControlHandler {
    async fn session(
        &self,
        session_id: &str,
    ) -> Result<Arc<super::RegisteredAgentSession>, InteractionError> {
        self.inner
            .registry
            .get(session_id)
            .await?
            .ok_or_else(|| missing_session_error(session_id))
    }

    async fn session_descriptor(
        &self,
        session_id: &str,
    ) -> Result<InteractionSessionDescriptor, InteractionError> {
        self.inner
            .registry
            .get_descriptor(session_id)
            .await?
            .ok_or_else(|| missing_session_error(session_id))
    }

    async fn subscribe_raw(
        &self,
        request: EventSubscribeRequest,
        notifier: InteractionNotifier,
    ) -> Result<SubscriptionResult, InteractionError> {
        let registered = self.session(&request.session_id).await?;
        let subscription_id = self.next_subscription_id();
        let session_id = request.session_id;
        let notification_subscription_id = subscription_id.clone();
        let listener_id = registered.agent().subscribe(move |event| {
            let notifier = notifier.clone();
            let session_id = session_id.clone();
            let subscription_id = notification_subscription_id.clone();
            async move {
                let _ = notifier.notify(
                    RAW_EVENT_NOTIFICATION,
                    &RawEventNotification {
                        session_id,
                        subscription_id,
                        event,
                    },
                );
                Ok(())
            }
        });
        self.client
            .subscriptions
            .lock()
            .expect("interaction subscription lock poisoned")
            .insert(
                subscription_id.clone(),
                InteractionSubscription {
                    session_id: registered.record().session_id.clone(),
                    listener_id,
                },
            );
        Ok(SubscriptionResult { subscription_id })
    }

    async fn subscribe_display(
        &self,
        session_id: String,
        ux: InteractionUxCapabilities,
        notifier: InteractionNotifier,
    ) -> Result<SubscriptionResult, InteractionError> {
        let registered = self.session(&session_id).await?;
        let subscription_id = self.next_subscription_id();
        let projector = Arc::new(Mutex::new(DisplayProjector {
            session_id,
            subscription_id: subscription_id.clone(),
            ux,
            notifier,
        }));
        let listener_projector = Arc::clone(&projector);
        let listener_id = registered.agent().subscribe(move |event| {
            let listener_projector = Arc::clone(&listener_projector);
            async move {
                listener_projector
                    .lock()
                    .expect("display projector lock poisoned")
                    .handle(event);
                Ok(())
            }
        });
        self.client
            .subscriptions
            .lock()
            .expect("interaction subscription lock poisoned")
            .insert(
                subscription_id.clone(),
                InteractionSubscription {
                    session_id: registered.record().session_id.clone(),
                    listener_id,
                },
            );
        Ok(SubscriptionResult { subscription_id })
    }

    async fn unsubscribe(
        &self,
        subscription_id: String,
    ) -> Result<UnsubscribeResult, InteractionError> {
        let subscription = self
            .client
            .subscriptions
            .lock()
            .expect("interaction subscription lock poisoned")
            .remove(&subscription_id)
            .ok_or_else(|| {
                InteractionError::not_found(format!("subscription not found: {subscription_id}"))
            })?;
        if let Some(session) = self.inner.registry.get(&subscription.session_id).await? {
            session.agent().unsubscribe(subscription.listener_id);
        }
        Ok(UnsubscribeResult { unsubscribed: true })
    }

    async fn resolve_approval(
        &self,
        request: ApprovalResolveRequest,
    ) -> Result<InteractionSessionDescriptor, InteractionError> {
        let registered = self.session(&request.session_id).await?;
        let pending = registered.agent().pending_tool_approvals().await;
        let approval = pending.get(&request.approval_id).cloned().ok_or_else(|| {
            InteractionError::not_found(format!("approval not found: {}", request.approval_id))
        })?;
        registered
            .session()
            .record_tool_approval_resolution(&approval, &request.decision);
        registered
            .agent()
            .resume_tool_approval(ToolApprovalResolution {
                approval_id: request.approval_id,
                decision: request.decision,
            })
            .await?;
        Ok(registered.descriptor().await)
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct InitializeResult {
    server: InteractionServerInfo,
    grant: InteractionCapabilityGrant,
    profiles: Vec<InteractionProfileDescriptor>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct InteractionServerInfo {
    name: String,
    protocol_version: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SessionRequest {
    session_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SessionDeleteRequest {
    session_id: String,
    #[serde(default)]
    force_abort: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AgentPromptRequest {
    session_id: String,
    input: AgentPromptInput,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum AgentPromptInput {
    Text { text: String },
    Message { message: AgentMessage },
}

impl AgentPromptInput {
    fn into_agent_input(self) -> AgentInput {
        match self {
            Self::Text { text } => AgentInput::Text(text),
            Self::Message { message } => AgentInput::Message(message),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AgentSteerRequest {
    session_id: String,
    message: AgentMessage,
    #[serde(default)]
    intent: Option<InteractionQueuedMessageIntent>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct AgentFollowUpRequest {
    session_id: String,
    message: AgentMessage,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum QueueKind {
    Steering,
    FollowUp,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct InteractionQueuedMessage {
    message: AgentMessage,
    intent: InteractionQueuedMessageIntent,
}

impl InteractionQueuedMessage {
    fn from_core(message: QueuedAgentMessage) -> Self {
        Self {
            message: message.message,
            intent: InteractionQueuedMessageIntent::from(message.intent),
        }
    }

    fn into_core(self) -> QueuedAgentMessage {
        match self.intent {
            InteractionQueuedMessageIntent::Observation => {
                QueuedAgentMessage::observation(self.message)
            }
            InteractionQueuedMessageIntent::UserInput => {
                QueuedAgentMessage::user_input(self.message)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum InteractionQueuedMessageIntent {
    #[default]
    Observation,
    UserInput,
}

impl From<QueuedMessageIntent> for InteractionQueuedMessageIntent {
    fn from(value: QueuedMessageIntent) -> Self {
        match value {
            QueuedMessageIntent::Observation => Self::Observation,
            QueuedMessageIntent::UserInput => Self::UserInput,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct QueueRequest {
    session_id: String,
    queue: QueueKind,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct QueueEditRequest {
    session_id: String,
    queue: QueueKind,
    messages: Vec<InteractionQueuedMessage>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct QueueSetModeRequest {
    session_id: String,
    queue: QueueKind,
    mode: QueueMode,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct EventSubscribeRequest {
    session_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DisplaySubscribeRequest {
    session_id: String,
    #[serde(default)]
    ux: Option<InteractionUxCapabilities>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct EventUnsubscribeRequest {
    subscription_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SubscriptionResult {
    subscription_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct UnsubscribeResult {
    unsubscribed: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RawEventNotification {
    session_id: String,
    subscription_id: String,
    event: AgentEvent,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DisplayEventNotification {
    session_id: String,
    subscription_id: String,
    event: DisplayEvent,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ApprovalResolveRequest {
    session_id: String,
    approval_id: ToolApprovalId,
    decision: ToolPermissionDecision,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManifestProposalRequest {
    session_id: String,
    proposal_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ManifestApplyResult {
    applied_proposal_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ProcessJobRequest {
    session_id: String,
    job_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ProcessReadRpcRequest {
    session_id: String,
    job_id: String,
    #[serde(default)]
    after_seq: Option<u64>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    wait_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ProcessWaitRequest {
    session_id: String,
    job_id: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ProcessWriteRequest {
    session_id: String,
    job_id: String,
    text: String,
}

struct DisplayProjector {
    session_id: String,
    subscription_id: String,
    ux: InteractionUxCapabilities,
    notifier: InteractionNotifier,
}

impl DisplayProjector {
    fn handle(&mut self, event: AgentEvent) {
        for display_event in self.project(event) {
            let _ = self.notifier.notify(
                DISPLAY_EVENT_NOTIFICATION,
                &DisplayEventNotification {
                    session_id: self.session_id.clone(),
                    subscription_id: self.subscription_id.clone(),
                    event: display_event,
                },
            );
        }
    }

    fn project(&mut self, event: AgentEvent) -> Vec<DisplayEvent> {
        match event.kind {
            AgentEventKind::RunStarted => vec![DisplayEvent::RunStarted {
                run_id: event.run_id,
            }],
            AgentEventKind::RunCompleted => vec![DisplayEvent::RunCompleted {
                run_id: event.run_id,
            }],
            AgentEventKind::RunAborted => vec![DisplayEvent::RunFailed {
                run_id: event.run_id,
                error: "run aborted".into(),
            }],
            AgentEventKind::RunFailed { error } => vec![DisplayEvent::RunFailed {
                run_id: event.run_id,
                error,
            }],
            AgentEventKind::RunPaused { reason } => vec![DisplayEvent::RunPaused {
                run_id: event.run_id,
                reason: serde_json::to_value(reason).unwrap_or(Value::Null),
            }],
            AgentEventKind::ModelStreamEvent {
                event: ModelStreamEvent::TextDelta { text },
                ..
            } => {
                if self.ux.stream_text {
                    vec![DisplayEvent::AssistantMessageDelta {
                        display_message_id: display_message_id(&event.run_id),
                        text: truncate_text_for_ux(&text, &self.ux).0,
                    }]
                } else {
                    Vec::new()
                }
            }
            AgentEventKind::EffectCommitted {
                effect: noloong_agent_core::AgentEffect::AppendMessage { message },
            } if matches!(message.role, noloong_agent_core::MessageRole::Assistant) => {
                let (message, truncated) = truncate_message_for_ux(message, &self.ux);
                vec![DisplayEvent::AssistantMessageFinal {
                    display_message_id: display_message_id(&event.run_id),
                    message,
                    truncated,
                }]
            }
            AgentEventKind::ToolExecutionStarted {
                tool_call_id,
                tool_name,
            } => vec![DisplayEvent::ToolStarted {
                tool_call_id,
                tool_name,
            }],
            AgentEventKind::ToolExecutionUpdate {
                tool_call_id,
                update,
            } => vec![DisplayEvent::ToolUpdated {
                tool_call_id,
                update,
            }],
            AgentEventKind::ToolExecutionCompleted {
                tool_call_id,
                output,
            } => vec![DisplayEvent::ToolCompleted {
                tool_call_id,
                output,
            }],
            AgentEventKind::ToolApprovalRequested { approval } => {
                vec![DisplayEvent::ApprovalRequested { approval }]
            }
            _ => Vec::new(),
        }
    }
}

fn queue_messages(
    agent: &noloong_agent_core::Agent,
    queue: QueueKind,
) -> Vec<InteractionQueuedMessage> {
    match queue {
        QueueKind::Steering => agent.queued_steering_messages(),
        QueueKind::FollowUp => agent.queued_follow_up_messages(),
    }
    .into_iter()
    .map(InteractionQueuedMessage::from_core)
    .collect()
}

fn replace_queue(
    agent: &noloong_agent_core::Agent,
    queue: QueueKind,
    messages: Vec<QueuedAgentMessage>,
) {
    match queue {
        QueueKind::Steering => agent.edit_steering_queue(|queue| {
            *queue = messages;
        }),
        QueueKind::FollowUp => agent.edit_follow_up_queue(|queue| {
            *queue = messages;
        }),
    }
}

fn clear_queue(agent: &noloong_agent_core::Agent, queue: QueueKind) {
    match queue {
        QueueKind::Steering => agent.clear_steering_queue(),
        QueueKind::FollowUp => agent.clear_follow_up_queue(),
    }
}

fn set_queue_mode(agent: &noloong_agent_core::Agent, queue: QueueKind, mode: QueueMode) {
    match queue {
        QueueKind::Steering => agent.set_steering_mode(mode),
        QueueKind::FollowUp => agent.set_follow_up_mode(mode),
    }
}

fn truncate_message_for_ux(
    mut message: AgentMessage,
    ux: &InteractionUxCapabilities,
) -> (AgentMessage, bool) {
    let Some(max_bytes) = ux.max_message_bytes else {
        return (message, false);
    };
    let mut remaining = max_bytes;
    let mut truncated = false;
    for block in &mut message.content {
        if let ContentBlock::Text { text } = block {
            let (next, block_truncated) = truncate_text_edges(text, remaining);
            *text = next;
            truncated |= block_truncated;
            remaining = remaining.saturating_sub(text.len());
        }
    }
    (message, truncated)
}

fn truncate_text_for_ux(text: &str, ux: &InteractionUxCapabilities) -> (String, bool) {
    ux.max_message_bytes
        .map(|max_bytes| truncate_text_edges(text, max_bytes))
        .unwrap_or_else(|| (text.into(), false))
}

fn truncate_text_edges(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.into(), false);
    }
    const MARKER: &str = "\n...[truncated]...\n";
    if max_bytes <= MARKER.len() + 2 {
        return (text::prefix_to_bytes(text, max_bytes), true);
    }
    let content_bytes = max_bytes - MARKER.len();
    let head_bytes = content_bytes / 2;
    let tail_bytes = content_bytes - head_bytes;
    (
        format!(
            "{}{}{}",
            text::prefix_to_bytes(text, head_bytes),
            MARKER,
            text::suffix_to_bytes(text, tail_bytes)
        ),
        true,
    )
}

fn display_message_id(run_id: &str) -> String {
    format!("{run_id}:assistant")
}

fn parse_params<T>(params: Value) -> Result<T, InteractionError>
where
    T: DeserializeOwned,
{
    serde_json::from_value(params).map_err(InteractionError::from)
}

fn value<T>(value: T) -> Result<Value, InteractionError>
where
    T: Serialize,
{
    serde_json::to_value(value)
        .map_err(|error| InteractionError::internal(format!("json encode failed: {error}")))
}

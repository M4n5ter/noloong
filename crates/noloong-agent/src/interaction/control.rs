use super::display_projector::DisplayProjector;
use super::{
    AgentSessionDeleteOptions, AgentSessionListFilter, AgentSessionRegistry,
    AutomationCreateRequest, AutomationListRequest, AutomationRequest, AutomationUpdateRequest,
    GoalSetRequest, GoalStatusUpdateRequest, InteractionAuthorityCapability,
    InteractionCapabilityGrant, InteractionCapabilityPolicy, InteractionClientInfo,
    InteractionError, InteractionFuture, InteractionNotifier, InteractionSessionDescriptor,
    InteractionUxCapabilities, JsonRpcHandler, JsonRpcHandlerOutput, SubagentSpawnRequest,
    protocol::{
        AgentFollowUpRequest, AgentPromptRequest, AgentSessionQueuedMessage,
        AgentSessionQueuedMessageIntent, AgentSteerRequest, ApprovalResolveRequest,
        DisplaySubscribeRequest, EventSubscribeRequest, EventUnsubscribeRequest,
        InteractionInitializeResult, InteractionQueueKind, InteractionServerInfo,
        ManifestApplyResult, ManifestProposalRequest, ProcessJobRequest, ProcessReadRequest,
        ProcessWaitRequest, ProcessWriteRequest, QueueEditRequest, QueueRequest,
        QueueSetModeRequest, RawEventNotification, SessionDeleteRequest,
        SessionMetadataUpdateRequest, SessionRequest, SubscriptionResult, UnsubscribeResult,
        method, notification,
    },
    store::missing_session_error,
};
use crate::tools::final_assistant_output;
use noloong_agent_core::{
    Agent, AgentCoreError, AgentMessage, QueueMode, QueuedAgentMessage, ToolApprovalResolution,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

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
    agent: Agent,
    listener_id: u64,
}

impl Drop for InteractionControlClientState {
    fn drop(&mut self) {
        let subscriptions = self
            .subscriptions
            .get_mut()
            .expect("interaction subscription lock poisoned");
        for subscription in std::mem::take(subscriptions).into_values() {
            subscription.agent.unsubscribe(subscription.listener_id);
        }
    }
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
        value(InteractionInitializeResult {
            server: InteractionServerInfo::current(),
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
                method::INITIALIZE => self.initialize(params)?,
                method::SHUTDOWN => return Ok(JsonRpcHandlerOutput::shutdown(json!({"ok": true}))),
                method::PROFILE_LIST => value(self.inner.registry.profile_descriptors())?,
                method::SESSION_CREATE => {
                    let request = parse_params(params)?;
                    value(self.inner.registry.create_session(request).await?)?
                }
                method::SESSION_LIST => {
                    let request = parse_params::<AgentSessionListFilter>(params)?;
                    value(self.inner.registry.list(request).await?)?
                }
                method::SESSION_GET => {
                    let request = parse_params::<SessionRequest>(params)?;
                    let descriptor = self.session_descriptor(&request.session_id).await?;
                    value(descriptor)?
                }
                method::SESSION_UPDATE_METADATA => {
                    let request = parse_params::<SessionMetadataUpdateRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .update_session_metadata(&request.session_id, request.metadata)
                            .await?,
                    )?
                }
                method::SESSION_DELETE => {
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
                method::SUBAGENT_SPAWN => {
                    self.require(method, InteractionAuthorityCapability::SubagentSpawn)?;
                    let request = parse_params::<SubagentSpawnRequest>(params)?;
                    let parent_session_id = request.parent_session_id.clone();
                    let observe_result = request.initial_prompt.is_some();
                    let descriptor = self.inner.registry.spawn_subagent(request).await?;
                    if observe_result {
                        self.observe_control_plane_subagent_result(
                            parent_session_id,
                            descriptor.session_id.clone(),
                        );
                    }
                    value(descriptor)?
                }
                method::GOAL_SET => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<GoalSetRequest>(params)?;
                    value(self.inner.registry.set_goal(request).await?)?
                }
                method::GOAL_GET => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(self.inner.registry.get_goal(&request.session_id).await?)?
                }
                method::GOAL_PAUSE => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    value(self.inner.registry.pause_goal(&request.session_id).await?)?
                }
                method::GOAL_RESUME => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    value(self.inner.registry.resume_goal(&request.session_id).await?)?
                }
                method::GOAL_CLEAR => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    value(self.inner.registry.clear_goal(&request.session_id).await?)?
                }
                method::GOAL_UPDATE => {
                    self.require(method, InteractionAuthorityCapability::GoalManage)?;
                    let request = parse_params::<GoalStatusUpdateRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .update_goal_status(request, String::new(), 0)
                            .await?,
                    )?
                }
                method::AUTOMATION_CREATE => {
                    self.require(method, InteractionAuthorityCapability::AutomationManage)?;
                    let request = parse_params::<AutomationCreateRequest>(params)?;
                    value(self.inner.registry.create_automation(request).await?)?
                }
                method::AUTOMATION_GET => {
                    let request = parse_params::<AutomationRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .get_automation(&request.automation_id)
                            .await?,
                    )?
                }
                method::AUTOMATION_LIST => {
                    let request = parse_params::<AutomationListRequest>(params)?;
                    value(self.inner.registry.list_automations(request).await?)?
                }
                method::AUTOMATION_UPDATE => {
                    self.require(method, InteractionAuthorityCapability::AutomationManage)?;
                    let request = parse_params::<AutomationUpdateRequest>(params)?;
                    value(self.inner.registry.update_automation(request).await?)?
                }
                method::AUTOMATION_DELETE => {
                    self.require(method, InteractionAuthorityCapability::AutomationManage)?;
                    let request = parse_params::<AutomationRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .delete_automation(&request.automation_id)
                            .await?,
                    )?
                }
                method::AUTOMATION_FIRE => {
                    self.require(method, InteractionAuthorityCapability::AutomationManage)?;
                    let request = parse_params::<AutomationRequest>(params)?;
                    value(
                        self.inner
                            .registry
                            .fire_automation(&request.automation_id)
                            .await?,
                    )?
                }
                method::AGENT_PROMPT => {
                    self.require(method, InteractionAuthorityCapability::AgentRun)?;
                    let request = parse_params::<AgentPromptRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered
                        .agent()
                        .prompt(request.input.into_agent_input())
                        .await?;
                    let descriptor = registered.descriptor().await;
                    if let Some(parent_session_id) = descriptor.parent_session_id.clone() {
                        self.observe_control_plane_subagent_result(
                            parent_session_id,
                            descriptor.session_id.clone(),
                        );
                    }
                    value(descriptor)?
                }
                method::AGENT_CONTINUE => {
                    self.require(method, InteractionAuthorityCapability::AgentRun)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered.agent().continue_run().await?;
                    value(registered.descriptor().await)?
                }
                method::AGENT_ABORT => {
                    self.require(method, InteractionAuthorityCapability::AgentRun)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered.agent().abort().await;
                    value(registered.descriptor().await)?
                }
                method::AGENT_WAIT_IDLE => {
                    self.require(method, InteractionAuthorityCapability::AgentRun)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered.agent().wait_for_idle().await;
                    value(registered.descriptor().await)?
                }
                method::AGENT_STATE => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .agent()
                            .state()
                            .await,
                    )?
                }
                method::AGENT_STEER => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<AgentSteerRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    let message = request.message;
                    match request.intent.unwrap_or_default() {
                        AgentSessionQueuedMessageIntent::Observation => {
                            registered.agent().steer(message)
                        }
                        AgentSessionQueuedMessageIntent::UserInput => {
                            registered.agent().steer_user_input(message)
                        }
                    }
                    value(registered.save_snapshot().await?)?
                }
                method::AGENT_FOLLOW_UP => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<AgentFollowUpRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered.agent().follow_up(request.message);
                    value(registered.save_snapshot().await?)?
                }
                method::QUEUE_LIST => {
                    let request = parse_params::<QueueRequest>(params)?;
                    value(queue_messages(
                        self.session(&request.session_id).await?.agent(),
                        request.queue,
                    ))?
                }
                method::QUEUE_EDIT => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<QueueEditRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    replace_queue(
                        registered.agent(),
                        request.queue,
                        request
                            .messages
                            .into_iter()
                            .map(AgentSessionQueuedMessage::into_core)
                            .collect(),
                    );
                    registered.save_snapshot().await?;
                    value(queue_messages(registered.agent(), request.queue))?
                }
                method::QUEUE_CLEAR => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<QueueRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    clear_queue(registered.agent(), request.queue);
                    registered.save_snapshot().await?;
                    value(queue_messages(registered.agent(), request.queue))?
                }
                method::QUEUE_SET_MODE => {
                    self.require(method, InteractionAuthorityCapability::AgentQueue)?;
                    let request = parse_params::<QueueSetModeRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    set_queue_mode(registered.agent(), request.queue, request.mode);
                    registered.save_snapshot().await?;
                    value(queue_messages(registered.agent(), request.queue))?
                }
                method::EVENT_SUBSCRIBE => {
                    self.require_ux(method, "rawEvents", |ux| ux.raw_events)?;
                    let request = parse_params::<EventSubscribeRequest>(params)?;
                    value(self.subscribe_raw(request, notifier).await?)?
                }
                method::EVENT_UNSUBSCRIBE => {
                    let request = parse_params::<EventUnsubscribeRequest>(params)?;
                    value(self.unsubscribe(request.subscription_id).await?)?
                }
                method::DISPLAY_SUBSCRIBE => {
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
                method::APPROVAL_LIST => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .agent()
                            .pending_tool_approvals()
                            .await,
                    )?
                }
                method::APPROVAL_RESOLVE => {
                    self.require(method, InteractionAuthorityCapability::ApprovalResolve)?;
                    let request = parse_params::<ApprovalResolveRequest>(params)?;
                    value(self.resolve_approval(request).await?)?
                }
                method::APPROVAL_RESUME_TIMEOUTS => {
                    self.require(method, InteractionAuthorityCapability::ApprovalResolve)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    registered
                        .agent()
                        .resume_due_tool_approval_timeouts()
                        .await?;
                    value(registered.descriptor().await)?
                }
                method::MANIFEST_GET => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .manifest(),
                    )?
                }
                method::MANIFEST_SYSTEM_PROMPT_GET => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .resolved_system_prompt(),
                    )?
                }
                method::MANIFEST_PROPOSALS_LIST => {
                    let request = parse_params::<SessionRequest>(params)?;
                    value(
                        self.session(&request.session_id)
                            .await?
                            .session()
                            .proposal_store()
                            .pending_proposals(),
                    )?
                }
                method::MANIFEST_PROPOSALS_APPROVE => {
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
                method::MANIFEST_APPLY_APPROVED => {
                    self.require(method, InteractionAuthorityCapability::ManifestApply)?;
                    let request = parse_params::<SessionRequest>(params)?;
                    let registered = self.session(&request.session_id).await?;
                    let applied = registered.session().apply_approved_manifest_patches()?;
                    registered.save_snapshot().await?;
                    value(ManifestApplyResult {
                        applied_proposal_ids: applied,
                    })?
                }
                method::PROCESS_LIST => {
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
                method::PROCESS_READ => {
                    let request = parse_params::<ProcessReadRequest>(params)?;
                    let output = self
                        .session(&request.session_id)
                        .await?
                        .session()
                        .process_manager()
                        .read(&request.job_id, request.output)
                        .await?;
                    value(output)?
                }
                method::PROCESS_WAIT => {
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
                method::PROCESS_WRITE => {
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
                method::PROCESS_TERMINATE => {
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
                notifier
                    .notify(
                        notification::RAW_EVENT,
                        &RawEventNotification {
                            session_id,
                            subscription_id,
                            event,
                        },
                    )
                    .await
                    .map_err(interaction_event_sink_error)?;
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
                    agent: registered.agent().clone(),
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
        let projector = Arc::new(Mutex::new(DisplayProjector::new(
            session_id,
            subscription_id.clone(),
            ux,
        )));
        let listener_projector = Arc::clone(&projector);
        let display_notifier = notifier.clone();
        let listener_id = registered.agent().subscribe(move |event| {
            let listener_projector = Arc::clone(&listener_projector);
            let display_notifier = display_notifier.clone();
            async move {
                let notifications = listener_projector
                    .lock()
                    .expect("display projector lock poisoned")
                    .handle(event);
                for notification_payload in notifications {
                    display_notifier
                        .notify(notification::DISPLAY_EVENT, &notification_payload)
                        .await
                        .map_err(interaction_event_sink_error)?;
                }
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
                    agent: registered.agent().clone(),
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
        subscription.agent.unsubscribe(subscription.listener_id);
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

    fn observe_control_plane_subagent_result(
        &self,
        parent_session_id: String,
        child_session_id: String,
    ) {
        let registry = self.inner.registry.clone();
        tokio::spawn(async move {
            let _ = observe_control_plane_subagent_result(
                registry,
                parent_session_id,
                child_session_id,
            )
            .await;
        });
    }
}

fn interaction_event_sink_error(error: InteractionError) -> AgentCoreError {
    AgentCoreError::JsonRpc(error.to_string())
}

async fn observe_control_plane_subagent_result(
    registry: AgentSessionRegistry,
    parent_session_id: String,
    child_session_id: String,
) -> Result<(), InteractionError> {
    if let Some(child) = registry.get(&child_session_id).await? {
        child.agent().wait_for_idle().await;
    }
    let Some(descriptor) = registry.get_descriptor(&child_session_id).await? else {
        return Ok(());
    };
    if descriptor.parent_session_id.as_deref() != Some(parent_session_id.as_str()) {
        return Ok(());
    }
    let Some(parent) = registry.get(&parent_session_id).await? else {
        return Ok(());
    };
    parent.agent().steer(subagent_result_observation_message(
        &parent_session_id,
        &descriptor,
    ));
    parent.save_snapshot().await?;
    Ok(())
}

fn subagent_result_observation_message(
    parent_session_id: &str,
    descriptor: &InteractionSessionDescriptor,
) -> AgentMessage {
    let final_output = final_assistant_output(&descriptor.state);
    let final_text = final_output
        .as_ref()
        .map(|output| output.final_text.trim())
        .filter(|text| !text.is_empty())
        .unwrap_or("(no assistant text)");
    let role_line = descriptor
        .role
        .as_deref()
        .map(|role| format!("\nrole: {role}"))
        .unwrap_or_default();
    let text = format!(
        "<subagent_result>\nsource: interaction_control\nparent_session_id: {parent_session_id}\nsession_id: {}\nstatus: {}{role_line}\nfinal_text:\n{final_text}\n</subagent_result>",
        descriptor.session_id,
        descriptor.status.as_str(),
    );
    let mut message =
        AgentMessage::user(format!("subagent-result-{}", descriptor.session_id), text);
    message.metadata.insert(
        "source".into(),
        Value::String("interaction_control.subagent_result".into()),
    );
    message.metadata.insert(
        "parentSessionId".into(),
        Value::String(parent_session_id.into()),
    );
    message.metadata.insert(
        "childSessionId".into(),
        Value::String(descriptor.session_id.clone()),
    );
    message.metadata.insert(
        "childStatus".into(),
        Value::String(descriptor.status.as_str().into()),
    );
    message
}

fn queue_messages(
    agent: &noloong_agent_core::Agent,
    queue: InteractionQueueKind,
) -> Vec<AgentSessionQueuedMessage> {
    match queue {
        InteractionQueueKind::Steering => agent.queued_steering_messages(),
        InteractionQueueKind::FollowUp => agent.queued_follow_up_messages(),
    }
    .into_iter()
    .map(AgentSessionQueuedMessage::from_core)
    .collect()
}

fn replace_queue(
    agent: &noloong_agent_core::Agent,
    queue: InteractionQueueKind,
    messages: Vec<QueuedAgentMessage>,
) {
    match queue {
        InteractionQueueKind::Steering => agent.edit_steering_queue(|queue| {
            *queue = messages;
        }),
        InteractionQueueKind::FollowUp => agent.edit_follow_up_queue(|queue| {
            *queue = messages;
        }),
    }
}

fn clear_queue(agent: &noloong_agent_core::Agent, queue: InteractionQueueKind) {
    match queue {
        InteractionQueueKind::Steering => agent.clear_steering_queue(),
        InteractionQueueKind::FollowUp => agent.clear_follow_up_queue(),
    }
}

fn set_queue_mode(agent: &noloong_agent_core::Agent, queue: InteractionQueueKind, mode: QueueMode) {
    match queue {
        InteractionQueueKind::Steering => agent.set_steering_mode(mode),
        InteractionQueueKind::FollowUp => agent.set_follow_up_mode(mode),
    }
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

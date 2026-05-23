use crate::{
    bridge::{WeixinBridge, WeixinBridgeError, WeixinQueueKind, WeixinQueueSnapshot},
    config::WeixinBridgeConfig,
    delivery::{WeixinDelivery, WeixinDeliveryError},
    display::{WeixinDisplayState, deliver_display_event},
    i18n::WeixinCatalog,
    ilink_api::{ReqwestWeixinApi, WeixinApi, WeixinMessage},
    input::{WeixinCommand, WeixinCommandKind, WeixinInboundUpdate},
    media::{WeixinAttachmentResolver, aes_padded_size},
    polling::{
        WeixinPoller, WeixinPollingError, WeixinUpdateHandler, WeixinUpdateHandlerFuture,
        run_polling_loop,
    },
    session::WeixinSessionKey,
    state::{SqliteWeixinStateStore, WeixinStateStore, account_fingerprint, current_unix_ms},
};
use noloong_agent::{
    JobSnapshot, Locale, ProcessOutput, WaitOutcome,
    approval::{allow_decision, deny_decision},
    interaction::{
        InteractionClientError, InteractionSessionStatus, InteractionWsClient,
        InteractionWsClientConfig,
    },
};
use noloong_agent_core::{ToolApprovalRequest, ToolPermissionOutcome};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::sync::Mutex;

type SharedDisplayState = Arc<Mutex<WeixinDisplayState>>;
type SharedDisplayStates = Arc<Mutex<BTreeMap<WeixinSessionKey, SharedDisplayState>>>;
type SharedControlCards = Arc<Mutex<BTreeMap<WeixinSessionKey, WeixinControlCardState>>>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WeixinControlCardState {
    session_ids: Vec<String>,
    process_ids: Vec<String>,
    approval_ids: Vec<String>,
}

pub async fn run_weixin_bridge_with_config(
    config: WeixinBridgeConfig,
    state_database_url: String,
) -> Result<(), WeixinRuntimeError> {
    let mut client_config = InteractionWsClientConfig::new(&config.interaction_ws_url)
        .request_timeout(Duration::from_secs(600));
    if let Some(token) = &config.interaction_bearer_token {
        client_config = client_config.bearer_token(token);
    }
    let account_fingerprint = account_fingerprint(&config.account_id);
    let state = Arc::new(SqliteWeixinStateStore::new(
        state_database_url,
        account_fingerprint,
    )?) as Arc<dyn WeixinStateStore>;
    let interaction = InteractionWsClient::connect(client_config).await?;
    let bridge = Arc::new(WeixinBridge::from_ws_client_with_state_store(
        config.clone(),
        interaction,
        Arc::clone(&state),
    )?);
    bridge.initialize().await?;
    let http_client = reqwest::Client::builder()
        .user_agent("noloong-weixin")
        .build()
        .map_err(|error| WeixinRuntimeError::HttpClient(error.to_string()))?;
    let api = Arc::new(
        ReqwestWeixinApi::new(http_client, Some(config.token.clone()))
            .with_base_url(config.base_url.clone())
            .with_cdn_base_url(config.cdn_base_url.clone())
            .with_max_download_bytes(aes_padded_size(config.file_policy.max_download_bytes)),
    ) as Arc<dyn WeixinApi>;
    let delivery = WeixinDelivery::new(
        Arc::clone(&api),
        Arc::clone(&state),
        config.cdn_base_url.clone(),
        config.max_outbound_chars,
        config.file_policy.max_upload_bytes,
    );
    let media_resolver = WeixinAttachmentResolver::new(
        Arc::clone(&api),
        config.file_policy.clone(),
        config.cdn_base_url.clone(),
    );
    let display_states = Arc::new(Mutex::new(BTreeMap::new()));
    let control_cards = Arc::new(Mutex::new(BTreeMap::new()));
    let display_task = tokio::spawn(run_display_delivery(
        Arc::clone(&bridge),
        delivery.clone(),
        Arc::clone(&display_states),
    ));
    let handler = Arc::new(BridgeUpdateHandler {
        bridge,
        state,
        delivery,
        media_resolver,
        account_id: config.account_id.clone(),
        locale: config.locale,
        catalog: WeixinCatalog::new(config.locale),
        display_states: Arc::clone(&display_states),
        control_cards,
    });
    let poller = WeixinPoller::new(api, handler.state.clone(), handler);
    log::info!("weixin bridge initialized; polling started");

    tokio::select! {
        result = run_polling_loop(poller) => result.map_err(WeixinRuntimeError::Polling),
        result = display_task => result.map_err(|error| WeixinRuntimeError::Task(error.to_string()))?,
    }
}

struct BridgeUpdateHandler {
    bridge: Arc<WeixinBridge>,
    state: Arc<dyn WeixinStateStore>,
    delivery: WeixinDelivery,
    media_resolver: WeixinAttachmentResolver,
    account_id: String,
    locale: Locale,
    catalog: WeixinCatalog,
    display_states: SharedDisplayStates,
    control_cards: SharedControlCards,
}

impl WeixinUpdateHandler for BridgeUpdateHandler {
    fn handle_message<'a>(&'a self, message: WeixinMessage) -> WeixinUpdateHandlerFuture<'a> {
        Box::pin(async move {
            match WeixinInboundUpdate::from_message(&self.account_id, message, self.bridge.access())
            {
                WeixinInboundUpdate::Ignored => {
                    log::debug!("weixin inbound update ignored");
                    Ok(())
                }
                WeixinInboundUpdate::Message(message) => {
                    log::info!(
                        "weixin inbound message accepted; peer={} sender={} message_id={} items={}",
                        safe_log_id(&message.context.peer_id),
                        safe_log_id(&message.context.sender_id),
                        safe_log_id(&message.context.message_id),
                        message.items.len()
                    );
                    if let Some(context_token) = message.context.context_token.as_deref() {
                        self.state
                            .save_context_token(&message.context.peer_id, context_token)
                            .await?;
                    }
                    let media = match self.media_resolver.resolve_all(&message.items).await {
                        Ok(media) => media,
                        Err(error) => {
                            self.report_media_error(&message.context.peer_id, error)
                                .await?;
                            return Ok(());
                        }
                    };
                    match self
                        .bridge
                        .handle_inbound_message(message.clone(), media)
                        .await
                    {
                        Ok(_) => Ok(()),
                        Err(error) => {
                            self.report_bridge_error(&message.context.peer_id, error)
                                .await
                        }
                    }
                }
                WeixinInboundUpdate::Command(command) => {
                    log::info!(
                        "weixin command accepted; kind={:?} peer={} sender={} message_id={}",
                        command.kind,
                        safe_log_id(&command.context.peer_id),
                        safe_log_id(&command.context.sender_id),
                        safe_log_id(&command.context.message_id)
                    );
                    if let Some(context_token) = command.context.context_token.as_deref() {
                        self.state
                            .save_context_token(&command.context.peer_id, context_token)
                            .await?;
                    }
                    let peer_id = command.context.peer_id.clone();
                    if let Err(error) = self.handle_command(command).await {
                        return self.report_command_error(&peer_id, error).await;
                    }
                    Ok(())
                }
            }
        })
    }
}

fn safe_log_id(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "?".into();
    }
    value.chars().take(12).collect()
}

impl BridgeUpdateHandler {
    async fn handle_command(&self, command: WeixinCommand) -> Result<(), WeixinPollingError> {
        match command.kind {
            WeixinCommandKind::Help => {
                self.delivery
                    .send_text(&command.context.peer_id, self.catalog.help())
                    .await?;
            }
            WeixinCommandKind::Status => self.send_status(&command).await?,
            WeixinCommandKind::New => self.create_new_session(&command).await?,
            WeixinCommandKind::Sessions => self.send_sessions(&command).await?,
            WeixinCommandKind::Switch => self.switch_session(&command).await?,
            WeixinCommandKind::Delete => self.delete_session(&command).await?,
            WeixinCommandKind::Approvals => self.send_approvals(&command).await?,
            WeixinCommandKind::Approve => {
                self.resolve_approval(&command, ToolPermissionOutcome::Allow)
                    .await?
            }
            WeixinCommandKind::Deny => {
                self.resolve_approval(&command, ToolPermissionOutcome::Deny)
                    .await?
            }
            WeixinCommandKind::RunConfig => self.send_run_config(&command).await?,
            WeixinCommandKind::Queue => self.send_or_update_queue(&command).await?,
            WeixinCommandKind::ClearQueue => self.clear_queues(&command).await?,
            WeixinCommandKind::Processes => self.send_processes(&command).await?,
            WeixinCommandKind::Process => self.send_process(&command).await?,
            WeixinCommandKind::Subagent => self.spawn_subagent(&command).await?,
        }
        Ok(())
    }

    async fn send_status(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self
            .bridge
            .current_session_for_chat(&command.context)
            .await?
        else {
            self.delivery
                .send_text(&command.context.peer_id, self.catalog.no_current_session())
                .await?;
            return Ok(());
        };
        self.delivery
            .send_text(
                &command.context.peer_id,
                &render_status(&descriptor, self.locale),
            )
            .await?;
        Ok(())
    }

    async fn current_session(
        &self,
        command: &WeixinCommand,
    ) -> Result<Option<noloong_agent::interaction::InteractionSessionDescriptor>, WeixinRuntimeError>
    {
        self.bridge
            .current_session_for_chat(&command.context)
            .await
            .map_err(Into::into)
    }

    async fn require_current_session(
        &self,
        command: &WeixinCommand,
    ) -> Result<Option<noloong_agent::interaction::InteractionSessionDescriptor>, WeixinRuntimeError>
    {
        let descriptor = self.current_session(command).await?;
        if descriptor.is_none() {
            self.delivery
                .send_text(&command.context.peer_id, self.catalog.no_current_session())
                .await?;
        }
        Ok(descriptor)
    }

    async fn send_run_config(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self.require_current_session(command).await? else {
            return Ok(());
        };
        self.delivery
            .send_text(
                &command.context.peer_id,
                &render_run_config(&descriptor, self.locale),
            )
            .await?;
        Ok(())
    }

    async fn create_new_session(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let key = command_key(command);
        let session_id = key.derived_session_id(current_unix_ms());
        let descriptor = self
            .bridge
            .create_chat_session(&command.context, session_id)
            .await?;
        self.delivery
            .send_text(
                &command.context.peer_id,
                &match self.locale {
                    Locale::Zh => format!("新会话已创建\n\nSession: {}", descriptor.session_id),
                    _ => format!("New session created\n\nSession: {}", descriptor.session_id),
                },
            )
            .await?;
        Ok(())
    }

    async fn send_sessions(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let key = command_key(command);
        let sessions = self.bridge.list_sessions_for_chat(&key).await?;
        self.remember_session_card(&key, &sessions).await;
        self.send_session_list(command, &sessions, None).await
    }

    async fn send_session_list(
        &self,
        command: &WeixinCommand,
        sessions: &[noloong_agent::interaction::InteractionSessionDescriptor],
        prefix: Option<&str>,
    ) -> Result<(), WeixinRuntimeError> {
        if sessions.is_empty() {
            self.delivery
                .send_text(
                    &command.context.peer_id,
                    match self.locale {
                        Locale::Zh => "没有可切换的会话。",
                        _ => "There are no sessions to switch to.",
                    },
                )
                .await?;
            return Ok(());
        }
        let lines = sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                format!(
                    "{}. {} · {:?} · {}",
                    index + 1,
                    session.session_id,
                    session.status,
                    session.profile_id
                )
            })
            .collect::<Vec<_>>();
        let prefix = prefix
            .map(|prefix| format!("{prefix}\n\n"))
            .unwrap_or_default();
        self.delivery
            .send_text(
                &command.context.peer_id,
                &match self.locale {
                    Locale::Zh => format!(
                        "{prefix}会话列表\n\n{}\n\n回复“/切换 1”或“/删除 1”。",
                        lines.join("\n\n")
                    ),
                    _ => format!(
                        "{prefix}Sessions\n\n{}\n\nReply with \"/switch 1\" or \"/delete 1\".",
                        lines.join("\n\n")
                    ),
                },
            )
            .await?;
        Ok(())
    }

    async fn switch_session(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some((key, session_id)) = self.selected_session(command).await? else {
            return Ok(());
        };
        let descriptor = self.bridge.switch_session(key, &session_id).await?;
        self.delivery
            .send_text(
                &command.context.peer_id,
                &match self.locale {
                    Locale::Zh => format!("已切换会话\n\nSession: {}", descriptor.session_id),
                    _ => format!("Session switched\n\nSession: {}", descriptor.session_id),
                },
            )
            .await?;
        Ok(())
    }

    async fn delete_session(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some((key, session_id)) = self.selected_session(command).await? else {
            return Ok(());
        };
        self.bridge.delete_session(key, &session_id, true).await?;
        self.delivery
            .send_text(
                &command.context.peer_id,
                &match self.locale {
                    Locale::Zh => format!("已删除会话\n\nSession: {session_id}"),
                    _ => format!("Session deleted\n\nSession: {session_id}"),
                },
            )
            .await?;
        Ok(())
    }

    async fn send_approvals(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self
            .bridge
            .current_session_for_chat(&command.context)
            .await?
        else {
            self.delivery
                .send_text(&command.context.peer_id, self.catalog.no_session())
                .await?;
            return Ok(());
        };
        let approvals = self.bridge.list_approvals(&descriptor.session_id).await?;
        self.remember_approval_card(&command_key(command), approval_ids(&approvals))
            .await;
        self.send_approval_list(command, &approvals, None).await
    }

    async fn send_approval_list(
        &self,
        command: &WeixinCommand,
        approvals: &BTreeMap<String, ToolApprovalRequest>,
        prefix: Option<&str>,
    ) -> Result<(), WeixinRuntimeError> {
        if approvals.is_empty() {
            self.delivery
                .send_text(&command.context.peer_id, self.catalog.no_approvals())
                .await?;
            return Ok(());
        }
        self.delivery
            .send_text(
                &command.context.peer_id,
                &render_approval_list(approvals, self.locale, prefix),
            )
            .await?;
        Ok(())
    }

    async fn send_or_update_queue(
        &self,
        command: &WeixinCommand,
    ) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self.require_current_session(command).await? else {
            return Ok(());
        };
        let args = command.args.trim();
        if !args.is_empty() {
            self.bridge
                .submit_follow_up_text(&command.context, &descriptor.session_id, args.to_owned())
                .await?;
            self.delivery
                .send_text(
                    &command.context.peer_id,
                    &match self.locale {
                        Locale::Zh => {
                            format!(
                                "已加入 follow-up 队列\n\nSession: {}",
                                descriptor.session_id
                            )
                        }
                        _ => format!(
                            "Added to follow-up queue\n\nSession: {}",
                            descriptor.session_id
                        ),
                    },
                )
                .await?;
            return Ok(());
        }
        let snapshot = self.bridge.list_queues(&descriptor.session_id).await?;
        self.delivery
            .send_text(
                &command.context.peer_id,
                &render_queue_snapshot(&snapshot, self.locale),
            )
            .await?;
        Ok(())
    }

    async fn clear_queues(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self.require_current_session(command).await? else {
            return Ok(());
        };
        let steering = self
            .bridge
            .clear_queue(&descriptor.session_id, WeixinQueueKind::Steering)
            .await?;
        let follow_up = self
            .bridge
            .clear_queue(&descriptor.session_id, WeixinQueueKind::FollowUp)
            .await?;
        self.delivery
            .send_text(
                &command.context.peer_id,
                &match self.locale {
                    Locale::Zh => format!(
                        "队列已清空\n\nSteering: {}\n\nFollow-up: {}",
                        steering.len(),
                        follow_up.len()
                    ),
                    _ => format!(
                        "Queues cleared\n\nSteering: {}\n\nFollow-up: {}",
                        steering.len(),
                        follow_up.len()
                    ),
                },
            )
            .await?;
        Ok(())
    }

    async fn send_processes(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self.require_current_session(command).await? else {
            return Ok(());
        };
        let processes = self.bridge.list_processes(&descriptor.session_id).await?;
        self.remember_process_card(&command_key(command), &processes)
            .await;
        self.send_process_list(command, &processes, None).await
    }

    async fn send_process_list(
        &self,
        command: &WeixinCommand,
        processes: &[JobSnapshot],
        prefix: Option<&str>,
    ) -> Result<(), WeixinRuntimeError> {
        self.delivery
            .send_text(
                &command.context.peer_id,
                &render_process_list(processes, self.locale, prefix),
            )
            .await?;
        Ok(())
    }

    async fn send_process(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self.require_current_session(command).await? else {
            return Ok(());
        };
        let Some((job_id, operation)) = self
            .resolve_process_target(&descriptor.session_id, command)
            .await?
        else {
            self.delivery
                .send_text(&command.context.peer_id, self.catalog.process_usage())
                .await?;
            return Ok(());
        };
        match operation {
            WeixinProcessOperation::Read => {
                let output = self
                    .bridge
                    .read_process(
                        &descriptor.session_id,
                        &job_id,
                        None,
                        Some(12_000),
                        Some(500),
                    )
                    .await?;
                self.delivery
                    .send_text(
                        &command.context.peer_id,
                        &render_process_output_text(&output, self.locale),
                    )
                    .await?;
            }
            WeixinProcessOperation::Wait => {
                let outcome = self
                    .bridge
                    .wait_process(&descriptor.session_id, &job_id, Some(5_000))
                    .await?;
                self.delivery
                    .send_text(
                        &command.context.peer_id,
                        &render_wait_outcome(&outcome, self.locale),
                    )
                    .await?;
            }
            WeixinProcessOperation::Terminate => {
                let snapshot = self
                    .bridge
                    .terminate_process(&descriptor.session_id, &job_id)
                    .await?;
                self.delivery
                    .send_text(
                        &command.context.peer_id,
                        &format_process_snapshot(1, &snapshot, self.locale),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn resolve_process_target(
        &self,
        session_id: &str,
        command: &WeixinCommand,
    ) -> Result<Option<(String, WeixinProcessOperation)>, WeixinRuntimeError> {
        let args = command.args.trim();
        if args.is_empty() {
            return Ok(None);
        }
        let mut parts = args.split_whitespace();
        let target = parts.next().unwrap_or_default();
        let operation = match parts.next().unwrap_or_default() {
            "wait" | "等待" => WeixinProcessOperation::Wait,
            "terminate" | "kill" | "终止" => WeixinProcessOperation::Terminate,
            _ => WeixinProcessOperation::Read,
        };
        let job_id = if let Ok(index) = target.parse::<usize>() {
            let processes = self.bridge.list_processes(session_id).await?;
            let selected = self
                .control_card(&command_key(command))
                .await
                .and_then(|card| select_numbered(&card.process_ids, index));
            match selected {
                Some(job_id) if processes.iter().any(|process| process.job_id == job_id) => {
                    Some(job_id)
                }
                _ => {
                    self.remember_process_card(&command_key(command), &processes)
                        .await;
                    self.send_process_list(
                        command,
                        &processes,
                        Some(self.catalog.stale_process_selector()),
                    )
                    .await?;
                    None
                }
            }
        } else {
            Some(target.to_owned())
        };
        Ok(job_id.map(|job_id| (job_id, operation)))
    }

    async fn spawn_subagent(&self, command: &WeixinCommand) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self.require_current_session(command).await? else {
            return Ok(());
        };
        let prompt = command.args.trim();
        if prompt.is_empty() {
            self.delivery
                .send_text(&command.context.peer_id, self.catalog.subagent_usage())
                .await?;
            return Ok(());
        }
        let child = self
            .bridge
            .spawn_subagent(&command.context, &descriptor.session_id, prompt.to_owned())
            .await?;
        self.delivery
            .send_text(
                &command.context.peer_id,
                &self
                    .catalog
                    .subagent_created(&child.session_id, session_status_label(&child.status)),
            )
            .await?;
        Ok(())
    }

    async fn resolve_approval(
        &self,
        command: &WeixinCommand,
        outcome: ToolPermissionOutcome,
    ) -> Result<(), WeixinRuntimeError> {
        let Some(descriptor) = self
            .bridge
            .current_session_for_chat(&command.context)
            .await?
        else {
            self.delivery
                .send_text(&command.context.peer_id, self.catalog.no_session())
                .await?;
            return Ok(());
        };
        let approvals = self.bridge.list_approvals(&descriptor.session_id).await?;
        let Some(approval_id) = self.selected_approval_id(command, &approvals).await else {
            self.remember_approval_card(&command_key(command), approval_ids(&approvals))
                .await;
            self.send_approval_list(
                command,
                &approvals,
                Some(self.catalog.stale_approval_selector()),
            )
            .await?;
            return Ok(());
        };
        let decision = weixin_approval_decision(
            outcome.clone(),
            match self.locale {
                Locale::Zh => "从微信处理",
                _ => "resolved from Weixin",
            },
            format!("weixin:{}", command.context.sender_id),
        );
        self.bridge
            .resolve_approval(&descriptor.session_id, &approval_id, decision)
            .await?;
        self.remove_display_approval(&command_key(command), &approval_id)
            .await;
        let selector = command.selector.unwrap_or(1);
        self.delivery
            .send_text(
                &command.context.peer_id,
                &match outcome {
                    ToolPermissionOutcome::Allow => match self.locale {
                        Locale::Zh => format!("已同意审批 {selector}，继续执行。"),
                        _ => format!("Approved {selector}; continuing."),
                    },
                    ToolPermissionOutcome::Deny => match self.locale {
                        Locale::Zh => format!("已拒绝审批 {selector}。"),
                        _ => format!("Denied {selector}."),
                    },
                },
            )
            .await?;
        Ok(())
    }

    async fn selected_approval_id(
        &self,
        command: &WeixinCommand,
        approvals: &BTreeMap<String, ToolApprovalRequest>,
    ) -> Option<String> {
        let selector = command.selector.unwrap_or(1);
        if let Some(state) = self.display_state(&command_key(command)).await {
            let state = state.lock().await;
            if let Some(approval_id) = state.approval_id_by_index(selector)
                && approvals.contains_key(&approval_id)
            {
                return Some(approval_id);
            }
        }
        if let Some(card) = self.control_card(&command_key(command)).await
            && let Some(approval_id) = select_numbered(&card.approval_ids, selector)
            && approvals.contains_key(&approval_id)
        {
            return Some(approval_id);
        }
        None
    }

    async fn remove_display_approval(&self, key: &WeixinSessionKey, approval_id: &str) {
        if let Some(state) = self.display_state(key).await {
            state.lock().await.remove_approval(approval_id);
        }
    }

    async fn display_state(&self, key: &WeixinSessionKey) -> Option<SharedDisplayState> {
        self.display_states.lock().await.get(key).cloned()
    }

    async fn selected_session(
        &self,
        command: &WeixinCommand,
    ) -> Result<Option<(WeixinSessionKey, String)>, WeixinRuntimeError> {
        let key = command_key(command);
        let sessions = self.bridge.list_sessions_for_chat(&key).await?;
        if let Some(selector) = command.selector {
            let selected = self
                .control_card(&key)
                .await
                .and_then(|card| select_numbered(&card.session_ids, selector));
            if let Some(session_id) = selected
                && sessions
                    .iter()
                    .any(|session| session.session_id == session_id)
            {
                return Ok(Some((key, session_id)));
            }
            self.remember_session_card(&key, &sessions).await;
            self.send_session_list(
                command,
                &sessions,
                Some(self.catalog.stale_session_selector()),
            )
            .await?;
            return Ok(None);
        }
        let explicit_session_id = command.args.trim();
        if explicit_session_id.is_empty() {
            self.remember_session_card(&key, &sessions).await;
            self.send_session_list(
                command,
                &sessions,
                Some(self.catalog.stale_session_selector()),
            )
            .await?;
            return Ok(None);
        }
        if sessions
            .iter()
            .any(|session| session.session_id == explicit_session_id)
        {
            return Ok(Some((key, explicit_session_id.to_owned())));
        }
        self.remember_session_card(&key, &sessions).await;
        self.send_session_list(
            command,
            &sessions,
            Some(self.catalog.stale_session_selector()),
        )
        .await?;
        Ok(None)
    }

    async fn control_card(&self, key: &WeixinSessionKey) -> Option<WeixinControlCardState> {
        self.control_cards.lock().await.get(key).cloned()
    }

    async fn remember_session_card(
        &self,
        key: &WeixinSessionKey,
        sessions: &[noloong_agent::interaction::InteractionSessionDescriptor],
    ) {
        let mut cards = self.control_cards.lock().await;
        cards.entry(key.clone()).or_default().session_ids = sessions
            .iter()
            .map(|session| session.session_id.clone())
            .collect();
    }

    async fn remember_process_card(&self, key: &WeixinSessionKey, processes: &[JobSnapshot]) {
        let mut cards = self.control_cards.lock().await;
        cards.entry(key.clone()).or_default().process_ids = processes
            .iter()
            .map(|process| process.job_id.clone())
            .collect();
    }

    async fn remember_approval_card(&self, key: &WeixinSessionKey, approval_ids: Vec<String>) {
        let mut cards = self.control_cards.lock().await;
        cards.entry(key.clone()).or_default().approval_ids = approval_ids;
    }

    async fn report_bridge_error(
        &self,
        peer_id: &str,
        error: WeixinBridgeError,
    ) -> Result<(), WeixinPollingError> {
        match error {
            WeixinBridgeError::EmptyMessage => Ok(()),
            WeixinBridgeError::Interaction(InteractionClientError::Timeout(_)) => {
                self.delivery
                    .send_text(peer_id, self.catalog.still_running())
                    .await?;
                Ok(())
            }
            error => {
                self.delivery
                    .send_text(
                        peer_id,
                        &match self.locale {
                            Locale::Zh => format!("提交失败：{error}"),
                            _ => format!("Submission failed: {error}"),
                        },
                    )
                    .await?;
                Ok(())
            }
        }
    }

    async fn report_command_error(
        &self,
        peer_id: &str,
        error: WeixinPollingError,
    ) -> Result<(), WeixinPollingError> {
        log::warn!(
            "weixin command failed; peer={} error={}",
            safe_log_id(peer_id),
            error
        );
        self.delivery
            .send_text(
                peer_id,
                &match self.locale {
                    Locale::Zh => format!("命令失败：{error}"),
                    _ => format!("Command failed: {error}"),
                },
            )
            .await?;
        Ok(())
    }

    async fn report_media_error(
        &self,
        peer_id: &str,
        error: crate::media::WeixinMediaError,
    ) -> Result<(), WeixinPollingError> {
        log::warn!(
            "weixin media processing failed; peer={} error={}",
            safe_log_id(peer_id),
            error
        );
        self.delivery
            .send_text(
                peer_id,
                &match self.locale {
                    Locale::Zh => format!("媒体处理失败：{error}"),
                    _ => format!("Media processing failed: {error}"),
                },
            )
            .await?;
        Ok(())
    }
}

async fn run_display_delivery(
    bridge: Arc<WeixinBridge>,
    delivery: WeixinDelivery,
    states: SharedDisplayStates,
) -> Result<(), WeixinRuntimeError> {
    let locale = bridge.config().locale;
    let mut receiver = bridge.subscribe_interaction_notifications();
    loop {
        let notification = receiver
            .recv()
            .await
            .map_err(|error| WeixinRuntimeError::Task(error.to_string()))?;
        let Some(display) = WeixinBridge::parse_display_notification(notification)? else {
            continue;
        };
        let Some(key) = bridge.session_key_for_display(&display.session_id) else {
            continue;
        };
        let state = {
            let mut states = states.lock().await;
            states
                .entry(key.clone())
                .or_insert_with(|| Arc::new(Mutex::new(WeixinDisplayState::default())))
                .clone()
        };
        let mut state = state.lock().await;
        if let Err(error) =
            deliver_display_event(&mut state, &delivery, &key.peer_id, locale, &display.event).await
        {
            log::warn!(
                "weixin display delivery failed; session_id={} peer={} error={}",
                display.session_id,
                safe_log_id(&key.peer_id),
                error
            );
        }
        if display_event_is_terminal(&display.event) {
            bridge
                .unsubscribe_inactive_display_route(&display.session_id)
                .await;
        }
    }
}

fn display_event_is_terminal(event: &noloong_agent::interaction::DisplayEvent) -> bool {
    matches!(
        event,
        noloong_agent::interaction::DisplayEvent::RunCompleted { .. }
            | noloong_agent::interaction::DisplayEvent::RunAborted { .. }
            | noloong_agent::interaction::DisplayEvent::RunFailed { .. }
    )
}

fn command_key(command: &WeixinCommand) -> WeixinSessionKey {
    WeixinSessionKey::new(
        command.context.account_id.clone(),
        command.context.peer_id.clone(),
        command.context.chat_kind,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WeixinProcessOperation {
    Read,
    Wait,
    Terminate,
}

fn render_status(
    descriptor: &noloong_agent::interaction::InteractionSessionDescriptor,
    locale: Locale,
) -> String {
    match locale {
        Locale::Zh => format!(
            "当前会话\n\nSession: {}\n\nProfile: {}\n\nStatus: {:?}\n\nMessages: {}",
            descriptor.session_id,
            descriptor.profile_id,
            descriptor.status,
            descriptor.state.messages.len()
        ),
        _ => format!(
            "Current session\n\nSession: {}\n\nProfile: {}\n\nStatus: {:?}\n\nMessages: {}",
            descriptor.session_id,
            descriptor.profile_id,
            descriptor.status,
            descriptor.state.messages.len()
        ),
    }
}

fn render_run_config(
    descriptor: &noloong_agent::interaction::InteractionSessionDescriptor,
    locale: Locale,
) -> String {
    match locale {
        Locale::Zh => format!(
            "运行配置\n\nSession: {}\n\nProfile: {}\n\nStatus: {:?}\n\nTools: {}\n\nPlugins: {}\n\nMessages: {}",
            descriptor.session_id,
            descriptor.profile_id,
            descriptor.status,
            descriptor.manifest.enabled_tools.len(),
            descriptor.manifest.plugins.len(),
            descriptor.state.messages.len()
        ),
        _ => format!(
            "Run config\n\nSession: {}\n\nProfile: {}\n\nStatus: {:?}\n\nTools: {}\n\nPlugins: {}\n\nMessages: {}",
            descriptor.session_id,
            descriptor.profile_id,
            descriptor.status,
            descriptor.manifest.enabled_tools.len(),
            descriptor.manifest.plugins.len(),
            descriptor.state.messages.len()
        ),
    }
}

fn render_queue_snapshot(snapshot: &WeixinQueueSnapshot, locale: Locale) -> String {
    match locale {
        Locale::Zh => format!(
            "队列\n\nSteering: {}\n\n{}\n\nFollow-up: {}\n\n{}\n\n回复“/队列 <文本>”加入 follow-up。\n\n回复“/清空队列”清空两类队列。",
            snapshot.steering.len(),
            render_queue_items(&snapshot.steering, locale),
            snapshot.follow_up.len(),
            render_queue_items(&snapshot.follow_up, locale)
        ),
        _ => format!(
            "Queues\n\nSteering: {}\n\n{}\n\nFollow-up: {}\n\n{}\n\nUse /queue <text> to add a follow-up.\n\nUse /clear_queue to clear both queues.",
            snapshot.steering.len(),
            render_queue_items(&snapshot.steering, locale),
            snapshot.follow_up.len(),
            render_queue_items(&snapshot.follow_up, locale)
        ),
    }
}

fn render_queue_items(items: &[crate::bridge::WeixinQueuedMessage], locale: Locale) -> String {
    if items.is_empty() {
        return match locale {
            Locale::Zh => "  空".into(),
            _ => "  empty".into(),
        };
    }
    items
        .iter()
        .take(5)
        .enumerate()
        .map(|(index, item)| {
            format!(
                "  {}. {:?}: {}",
                index + 1,
                item.intent,
                message_preview(&item.message)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn message_preview(message: &noloong_agent_core::AgentMessage) -> String {
    let text = message
        .content
        .iter()
        .filter_map(crate::render::render_content_block_text)
        .collect::<Vec<_>>()
        .join(" ");
    let text = text.trim();
    if text.is_empty() {
        return "<non-text>".into();
    }
    text.chars().take(120).collect()
}

fn session_status_label(status: &InteractionSessionStatus) -> &'static str {
    match status {
        InteractionSessionStatus::Idle => "idle",
        InteractionSessionStatus::Running => "running",
        InteractionSessionStatus::Completed => "completed",
        InteractionSessionStatus::Aborted => "aborted",
        InteractionSessionStatus::Failed => "failed",
        InteractionSessionStatus::Paused => "paused",
    }
}

fn render_process_list(processes: &[JobSnapshot], locale: Locale, prefix: Option<&str>) -> String {
    if processes.is_empty() {
        return match locale {
            Locale::Zh => "没有后台进程。".into(),
            _ => "No background processes.".into(),
        };
    }
    let mut lines = Vec::new();
    if let Some(prefix) = prefix {
        lines.push(prefix.to_owned());
    }
    lines.push(match locale {
        Locale::Zh => format!("后台进程：{}", processes.len()),
        _ => format!("Background processes: {}", processes.len()),
    });
    lines.extend(
        processes
            .iter()
            .enumerate()
            .map(|(index, process)| format_process_snapshot(index + 1, process, locale)),
    );
    lines.push(match locale {
        Locale::Zh => "回复“/进程 1”查看输出。\n\n回复“/进程 1 等待”等待。\n\n回复“/进程 1 终止”终止。".into(),
        _ => "Use /process 1 to read output.\n\nUse /process 1 wait to wait.\n\nUse /process 1 terminate to terminate.".into(),
    });
    lines.join("\n\n")
}

fn format_process_snapshot(index: usize, process: &JobSnapshot, locale: Locale) -> String {
    match locale {
        Locale::Zh => format!(
            "{}. {} · {:?}\n\n命令：{}",
            index, process.job_id, process.status, process.command
        ),
        _ => format!(
            "{}. {} · {:?}\n\nCommand: {}",
            index, process.job_id, process.status, process.command
        ),
    }
}

fn render_process_output_text(output: &ProcessOutput, locale: Locale) -> String {
    let text = output
        .chunks
        .iter()
        .map(|chunk| format!("[{:?} #{}]\n{}", chunk.stream, chunk.seq, chunk.text))
        .collect::<Vec<_>>()
        .join("\n");
    let text = if text.trim().is_empty() {
        match locale {
            Locale::Zh => "<无输出>".into(),
            _ => "<no output>".into(),
        }
    } else {
        text
    };
    match locale {
        Locale::Zh => format!(
            "进程输出\n\nJob: {}\n\nStatus: {:?}\n\n{}\n\nNext cursor: {}{}",
            output.job_id,
            output.status,
            text,
            output.next_cursor,
            if output.truncated {
                "\n(输出已截断)"
            } else {
                ""
            }
        ),
        _ => format!(
            "Process output\n\nJob: {}\n\nStatus: {:?}\n\n{}\n\nNext cursor: {}{}",
            output.job_id,
            output.status,
            text,
            output.next_cursor,
            if output.truncated {
                "\n(output truncated)"
            } else {
                ""
            }
        ),
    }
}

fn render_wait_outcome(outcome: &WaitOutcome, locale: Locale) -> String {
    match locale {
        Locale::Zh => format!(
            "进程等待结果\n\nJob: {}\n\nStatus: {:?}\n\nTimed out: {}",
            outcome.job_id, outcome.status, outcome.timed_out
        ),
        _ => format!(
            "Process wait result\n\nJob: {}\n\nStatus: {:?}\n\nTimed out: {}",
            outcome.job_id, outcome.status, outcome.timed_out
        ),
    }
}

fn select_numbered(ids: &[String], selector: usize) -> Option<String> {
    selector
        .checked_sub(1)
        .and_then(|index| ids.get(index))
        .cloned()
}

fn approval_ids(approvals: &BTreeMap<String, ToolApprovalRequest>) -> Vec<String> {
    approvals.keys().cloned().collect()
}

fn render_approval_list(
    approvals: &BTreeMap<String, ToolApprovalRequest>,
    locale: Locale,
    prefix: Option<&str>,
) -> String {
    let lines = approvals
        .iter()
        .enumerate()
        .map(|(index, (approval_id, approval))| {
            format!(
                "{}. {} · {}",
                index + 1,
                approval.tool_call.name,
                approval_id
            )
        })
        .collect::<Vec<_>>();
    let prefix = prefix
        .map(|prefix| format!("{prefix}\n\n"))
        .unwrap_or_default();
    match locale {
        Locale::Zh => format!(
            "{prefix}待处理审批\n\n{}\n\n回复“/同意 1”或“/拒绝 1”。",
            lines.join("\n\n")
        ),
        _ => format!(
            "{prefix}Pending approvals\n\n{}\n\nReply with \"/approve 1\" or \"/deny 1\".",
            lines.join("\n\n")
        ),
    }
}

fn weixin_approval_decision(
    outcome: ToolPermissionOutcome,
    reason: &str,
    approver: String,
) -> noloong_agent_core::ToolPermissionDecision {
    let metadata = serde_json::Value::Object(Default::default());
    match outcome {
        ToolPermissionOutcome::Allow => allow_decision(reason, approver, metadata),
        ToolPermissionOutcome::Deny => deny_decision(reason, approver, metadata),
    }
}

impl From<WeixinRuntimeError> for WeixinPollingError {
    fn from(value: WeixinRuntimeError) -> Self {
        WeixinPollingError::Handler(value.to_string())
    }
}

impl From<WeixinDeliveryError> for WeixinPollingError {
    fn from(value: WeixinDeliveryError) -> Self {
        WeixinPollingError::Handler(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{render_approval_list, render_process_list, select_numbered};
    use noloong_agent::{JobSnapshot, JobStatus, Locale};
    use noloong_agent_core::{ToolApprovalRequest, ToolApprovalRequestSpec, ToolCall};
    use serde_json::{Map, Value};
    use std::{collections::BTreeMap, path::PathBuf};

    #[test]
    fn numbered_selection_uses_stored_card_ids() {
        let ids = vec!["old-a".to_owned(), "old-b".to_owned()];

        assert_eq!(select_numbered(&ids, 2).as_deref(), Some("old-b"));
        assert_eq!(select_numbered(&ids, 3), None);
        assert_eq!(select_numbered(&ids, 0), None);
    }

    #[test]
    fn stale_process_selector_renders_current_list_with_hint() {
        let rendered = render_process_list(
            &[JobSnapshot {
                job_id: "job-current".into(),
                command: "echo ok".into(),
                shell: "/bin/zsh".into(),
                cwd: PathBuf::from("/tmp"),
                status: JobStatus::Running,
                started_at_ms: 1,
                ended_at_ms: None,
                next_cursor: 0,
                dropped_before_seq: 0,
            }],
            Locale::Zh,
            Some("进程编号已过期"),
        );

        assert!(rendered.starts_with("进程编号已过期\n\n后台进程：1"));
        assert!(rendered.contains("job-current"));
        assert!(rendered.contains("/进程 1"));
    }

    #[test]
    fn stale_approval_selector_renders_current_list_with_hint() {
        let mut approvals = BTreeMap::new();
        approvals.insert(
            "approval-current".into(),
            ToolApprovalRequest {
                approval_id: "approval-current".into(),
                tool_call: ToolCall {
                    id: "call-1".into(),
                    name: "host.exec.start".into(),
                    arguments: serde_json::json!({}),
                },
                permissions: Vec::new(),
                hook_id: None,
                request: ToolApprovalRequestSpec {
                    prompt: None,
                    reason: None,
                    expires_at_ms: None,
                    metadata: Value::Object(Map::new()),
                },
            },
        );

        let rendered = render_approval_list(&approvals, Locale::Zh, Some("审批编号已过期"));

        assert!(rendered.starts_with("审批编号已过期\n\n待处理审批"));
        assert!(rendered.contains("approval-current"));
        assert!(rendered.contains("/同意 1"));
    }
}

#[derive(Debug, Error)]
pub enum WeixinRuntimeError {
    #[error("{0}")]
    Bridge(#[from] WeixinBridgeError),
    #[error("{0}")]
    Delivery(#[from] WeixinDeliveryError),
    #[error("{0}")]
    Display(#[from] crate::display::WeixinDisplayError),
    #[error("{0}")]
    Polling(#[from] WeixinPollingError),
    #[error("interaction client failed: {0}")]
    InteractionClient(#[from] noloong_agent::interaction::InteractionClientError),
    #[error("interaction transport failed: {0}")]
    Interaction(#[from] noloong_agent::interaction::InteractionError),
    #[error("HTTP client failed: {0}")]
    HttpClient(String),
    #[error("{0}")]
    State(#[from] crate::state::WeixinStateError),
    #[error("Weixin command failed: {0}")]
    Command(String),
    #[error("background task failed: {0}")]
    Task(String),
}

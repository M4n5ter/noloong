use crate::{
    cli::{
        CliError, interaction_token, non_empty_option, parse_config_optional_u64,
        parse_config_usize, parse_csv_strings, parse_locale_arg, process_env, resolve_locale,
        stable_fingerprint, start_embedded_interaction,
    },
    config::{
        self, BuiltInProviderConfig, DEFAULT_INTERACTION_TOKEN_ENV, DEFAULT_INTERACTION_URL_ENV,
        DEFAULT_TELEGRAM_ALLOWED_CHATS_ENV, DEFAULT_TELEGRAM_ALLOWED_USERS_ENV,
        DEFAULT_TELEGRAM_BOT_TOKEN_ENV, DEFAULT_TELEGRAM_BOT_USERNAME_ENV,
        DEFAULT_TELEGRAM_DISABLE_ENV_PROXY_ENV, DEFAULT_TELEGRAM_DISABLE_FALLBACK_IPS_ENV,
        DEFAULT_TELEGRAM_FALLBACK_IPS_ENV, DEFAULT_TELEGRAM_FILE_DOWNLOAD_DIR_ENV,
        DEFAULT_TELEGRAM_FILE_INLINE_MAX_BYTES_ENV, DEFAULT_TELEGRAM_FILE_MAX_DOWNLOAD_BYTES_ENV,
        DEFAULT_TELEGRAM_FILE_RETENTION_SECONDS_ENV, DEFAULT_TELEGRAM_LOCALE_ENV,
        DEFAULT_TELEGRAM_PROXY_ENV, DEFAULT_TELEGRAM_REQUIRE_MENTION_ENV,
        DEFAULT_TELEGRAM_STARTUP_UPDATE_POLICY_ENV,
        DEFAULT_TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_ENV, HostProfileConfig,
        ensure_sqlite_database_parent, parse_bool_env, parse_csv_i64, parse_csv_u64,
        resolve_state_database_url,
    },
};
use clap::Args;
use noloong_agent::{
    Locale, SqliteClientStateStore,
    approval::{allow_decision, deny_decision},
    interaction::{InteractionClientError, InteractionWsClient, InteractionWsClientConfig},
};
use noloong_agent_core::{QueueMode, ToolApprovalRequest, ToolPermissionOutcome};
use noloong_agent_telegram::{
    access::{TelegramAccessPolicy, TelegramTextInput},
    approval::render_pending_approval_requests,
    bridge::{TelegramBridge, TelegramBridgeError},
    commands::{
        TelegramCockpitCommand, render_command_help, render_unknown_command_help,
        telegram_command_menu_request,
    },
    config::{
        TelegramFilePolicy, TelegramNativeMediaHandling, TelegramStartupUpdatePolicy,
        TelegramUnsupportedMediaFallbackPolicy,
    },
    delivery::{TelegramDelivery, TelegramMessageTarget},
    display::{
        TelegramDisplayDeliveryContext, TelegramDisplayState, cleanup_display_messages,
        deliver_display_event_with_reply,
    },
    i18n::{
        MANIFEST_PROPOSAL_DISPLAY_LIMIT, TelegramManifestCard, TelegramStatusCard,
        TelegramUiCatalog,
    },
    input::{TelegramCommand, TelegramInboundMessage, TelegramInboundUpdate},
    media::TelegramAttachmentResolver,
    network::{
        TelegramNetworkConfig, TelegramNetworkResolutionMode, build_telegram_http_client,
        discover_fallback_addrs, network_resolution_mode,
    },
    polling::{
        ClientStateTelegramOffsetStore, TelegramCallbackQuery, TelegramPollOutcome, TelegramPoller,
        TelegramPollingError, TelegramUpdate, TelegramUpdateHandler, TelegramUpdateHandlerFuture,
    },
    process::{
        PROCESS_OUTPUT_INLINE_CHAR_LIMIT, process_output_document_bytes, process_output_filename,
        process_output_read_max_bytes, process_output_wait_ms, process_snapshot_label,
        process_wait_timeout_ms, render_process_output,
    },
    queue::TelegramQueueKind,
    session::{
        TelegramSessionAction, TelegramSessionActionStore, TelegramSessionKey, single_button_markup,
    },
    telegram_api::{
        ReqwestTelegramApi, TelegramApi, TelegramInlineKeyboardMarkup, TelegramInputFile,
        TelegramMediaMessageOptions, TelegramMessageHandle, TelegramSendDocumentRequest,
    },
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;

type SharedDisplayState = Arc<Mutex<TelegramDisplayState>>;
type SharedDisplayStates = Arc<Mutex<BTreeMap<TelegramSessionKey, SharedDisplayState>>>;
type SharedSessionActions = Arc<Mutex<TelegramSessionActionStore>>;

const RESPONSES_FILE_DATA_MIME_TYPES: &[&str] = &[
    "application/msword",
    "application/json",
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/typescript",
    "application/x-sh",
    "text/css",
    "text/html",
    "text/javascript",
    "text/markdown",
    "text/plain",
    "text/x-c",
    "text/x-c++",
    "text/x-csharp",
    "text/x-golang",
    "text/x-java",
    "text/x-php",
    "text/x-python",
    "text/x-ruby",
    "text/x-script.python",
    "text/x-tex",
];
const CHAT_COMPLETIONS_NATIVE_AUDIO_MIME_TYPES: &[&str] =
    &["audio/wav", "audio/x-wav", "audio/mpeg", "audio/mp3"];

pub(crate) async fn run_telegram_bridge(options: TelegramBridgeOptions) -> Result<(), CliError> {
    let config = telegram_config_from_values(&options, process_env)?;
    run_telegram_bridge_with_config(config).await
}

pub(crate) async fn run_telegram(options: TelegramOptions) -> Result<(), CliError> {
    let embedded = start_embedded_interaction(options.profile_config).await?;
    let mut bridge_options = options.bridge;
    bridge_options.interaction_url = Some(embedded.interaction_ws_url().to_owned());
    bridge_options.interaction_token = Some(embedded.interaction_token().to_owned());
    let has_explicit_media_fallback = has_explicit_telegram_media_fallback(&bridge_options);
    let mut bridge_config = telegram_config_from_values(&bridge_options, process_env)?;
    if !has_explicit_media_fallback {
        apply_profile_media_fallback_policy(
            &mut bridge_config.file_policy,
            embedded.profile_config(),
            bridge_config.profile_id.as_deref(),
        );
    }
    embedded
        .run(run_telegram_bridge_with_config(bridge_config))
        .await
}

async fn run_telegram_bridge_with_config(
    mut config: noloong_agent_telegram::config::TelegramBridgeConfig,
) -> Result<(), CliError> {
    let mut client_config = InteractionWsClientConfig::new(&config.interaction_ws_url)
        .request_timeout(Duration::from_secs(600));
    if let Some(token) = &config.interaction_bearer_token {
        client_config = client_config.bearer_token(token);
    }
    let interaction = InteractionWsClient::connect(client_config).await?;
    let bridge = Arc::new(TelegramBridge::from_ws_client(config.clone(), interaction)?);
    bridge.initialize().await?;

    hydrate_telegram_fallback_addrs(&mut config.network).await?;
    log_telegram_network_mode(&config.network);
    let http_client = build_telegram_http_client(&config.network)?;
    let api = Arc::new(
        ReqwestTelegramApi::new(http_client, &config.bot_token, &config.network)
            .with_max_download_bytes(config.file_policy.max_download_bytes),
    ) as Arc<dyn TelegramApi>;
    let delivery = TelegramDelivery::new(Arc::clone(&api), config.max_outbound_chars);
    let media_resolver =
        TelegramAttachmentResolver::new(Arc::clone(&api), config.file_policy.clone());
    let catalog = TelegramUiCatalog::new(config.locale);
    register_telegram_commands(api.as_ref(), catalog).await?;
    let display_states = Arc::new(Mutex::new(
        BTreeMap::<TelegramSessionKey, SharedDisplayState>::new(),
    ));
    let session_actions = Arc::new(Mutex::new(TelegramSessionActionStore::default()));
    let edit_throttle = config.edit_throttle();
    let display_task = tokio::spawn(run_display_delivery(
        Arc::clone(&bridge),
        delivery.clone(),
        Arc::clone(&display_states),
        config.show_tool_status,
        edit_throttle,
        catalog,
    ));
    let handler = Arc::new(BridgeUpdateHandler {
        bridge,
        api,
        delivery,
        media_resolver,
        display_states,
        session_actions,
        catalog,
        bot_username: config.bot_username.clone(),
    });
    let state_database_url = resolve_state_database_url()?;
    ensure_sqlite_database_parent(&state_database_url)?;
    let client_state = Arc::new(SqliteClientStateStore::new(&state_database_url)?);
    let mut poller = TelegramPoller::new(Arc::clone(&handler.api), handler)
        .with_startup_update_policy(config.startup_update_policy)
        .with_offset_store(Arc::new(ClientStateTelegramOffsetStore::new(
            client_state,
            stable_fingerprint(&config.bot_token),
        )));
    poller.initialize().await.map_err(CliError::Polling)?;
    log::info!("telegram bridge initialized; polling started");

    tokio::select! {
        result = run_polling_loop(poller) => result.map_err(CliError::Polling),
        result = display_task => result.map_err(|error| CliError::Task(error.to_string()))?,
    }
}

async fn register_telegram_commands(
    api: &dyn TelegramApi,
    catalog: TelegramUiCatalog,
) -> Result<(), CliError> {
    api.set_my_commands(telegram_command_menu_request(catalog))
        .await?;
    Ok(())
}

async fn hydrate_telegram_fallback_addrs(
    config: &mut TelegramNetworkConfig,
) -> Result<(), CliError> {
    if config.proxy_url.is_some()
        || config.disable_fallback_ips
        || !config.resolved_addrs.is_empty()
    {
        return Ok(());
    }
    let discovery_client = build_telegram_http_client(config)?;
    config.resolved_addrs = discover_fallback_addrs(config, &discovery_client).await?;
    Ok(())
}

fn log_telegram_network_mode(config: &TelegramNetworkConfig) {
    match network_resolution_mode(config) {
        TelegramNetworkResolutionMode::Proxy => {
            log::info!("telegram network using TELEGRAM_PROXY");
        }
        TelegramNetworkResolutionMode::EnvProxy => {
            log::info!("telegram network using ambient proxy environment");
        }
        TelegramNetworkResolutionMode::StaticResolve => {
            log::info!(
                "telegram network fallback addresses configured: {}",
                config.resolved_addrs.len()
            );
        }
        TelegramNetworkResolutionMode::SystemDns => {
            log::info!("telegram network using direct system DNS");
        }
    }
}

async fn run_polling_loop(mut poller: TelegramPoller) -> Result<(), TelegramPollingError> {
    loop {
        match poller.poll_once().await? {
            TelegramPollOutcome::Polled => {}
            TelegramPollOutcome::RetryAfter {
                delay_seconds,
                reason,
            } => {
                log::warn!("telegram polling retrying after {delay_seconds}s: {reason}");
                tokio::time::sleep(Duration::from_secs(delay_seconds)).await;
            }
        }
    }
}

async fn run_display_delivery(
    bridge: Arc<TelegramBridge>,
    delivery: TelegramDelivery,
    display_states: SharedDisplayStates,
    show_tool_status: bool,
    edit_throttle: Duration,
    catalog: TelegramUiCatalog,
) -> Result<(), CliError> {
    let mut notifications = bridge.subscribe_interaction_notifications();
    loop {
        let notification = match notifications.recv().await {
            Ok(notification) => notification,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                log::warn!("telegram display notification receiver lagged by {skipped} events");
                continue;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                return Err(CliError::Task(
                    "telegram display notification channel closed".into(),
                ));
            }
        };
        let display = match TelegramBridge::parse_display_notification(notification) {
            Ok(Some(display)) => display,
            Ok(None) => continue,
            Err(error) => {
                log::warn!("telegram display notification decode failed: {error}");
                continue;
            }
        };
        let Some(key) = bridge.session_key_for_display(&display.session_id) else {
            continue;
        };
        let reply_to = bridge
            .observe_display_reply_target(key, &display.event)
            .map(noloong_agent_telegram::delivery::TelegramReplyTarget::new);
        let cleanup = {
            let state = display_state_for(&display_states, key).await;
            let mut state = state.lock().await;
            match deliver_display_event_with_reply(
                &mut state,
                &delivery,
                TelegramDisplayDeliveryContext {
                    target: TelegramMessageTarget::new(key.chat_id, key.thread_id),
                    notification: display,
                    reply_to,
                    show_tool_status,
                    edit_throttle,
                    catalog,
                },
            )
            .await
            {
                Ok(cleanup) => cleanup,
                Err(error) => {
                    log::warn!("telegram display delivery failed: {error}");
                    continue;
                }
            }
        };
        cleanup_display_messages(&delivery, cleanup).await;
    }
}

async fn display_state_for(
    display_states: &SharedDisplayStates,
    key: TelegramSessionKey,
) -> SharedDisplayState {
    display_states
        .lock()
        .await
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(TelegramDisplayState::default())))
        .clone()
}

struct BridgeUpdateHandler {
    bridge: Arc<TelegramBridge>,
    api: Arc<dyn TelegramApi>,
    delivery: TelegramDelivery,
    media_resolver: TelegramAttachmentResolver,
    display_states: SharedDisplayStates,
    session_actions: SharedSessionActions,
    catalog: TelegramUiCatalog,
    bot_username: Option<String>,
}

impl TelegramUpdateHandler for BridgeUpdateHandler {
    fn handle_update<'a>(&'a self, update: TelegramUpdate) -> TelegramUpdateHandlerFuture<'a> {
        Box::pin(async move {
            if let Some(message) = update.message
                && let Some(inbound) =
                    TelegramInboundUpdate::from_message(message, self.bot_username.as_deref())
            {
                match inbound {
                    TelegramInboundUpdate::Message(message) => {
                        if message.attachments.is_empty() {
                            let Some(input) = message.into_text_input() else {
                                return Ok(());
                            };
                            self.handle_text_input_message(input).await?;
                        } else {
                            self.handle_media_message(message).await?;
                        }
                    }
                    TelegramInboundUpdate::Command(command) => {
                        self.handle_command(command).await?;
                    }
                }
            }
            if let Some(callback) = update.callback_query {
                self.handle_callback(callback).await?;
            }
            Ok(())
        })
    }
}

impl BridgeUpdateHandler {
    async fn handle_text_input_message(
        &self,
        input: TelegramTextInput,
    ) -> Result<(), TelegramPollingError> {
        let target = TelegramMessageTarget::new(input.chat_id, input.thread_id);
        match self
            .bridge
            .handle_text_message(input, self.bot_username.as_deref())
            .await
        {
            Ok(_) => Ok(()),
            Err(error) => self.handle_agent_submission_error(target, error).await,
        }
    }

    async fn handle_agent_submission_error(
        &self,
        target: TelegramMessageTarget,
        error: TelegramBridgeError,
    ) -> Result<(), TelegramPollingError> {
        match error {
            TelegramBridgeError::NotAddressed | TelegramBridgeError::EmptyMessage => Ok(()),
            TelegramBridgeError::RunFailureDisplayed { source } => {
                log::debug!("agent run failure already rendered through display events: {source}");
                Ok(())
            }
            TelegramBridgeError::Unauthorized => Ok(()),
            TelegramBridgeError::Interaction(InteractionClientError::Timeout(_)) => {
                self.delivery
                    .send_text(target, self.catalog.input_submission_still_running(), None)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                Ok(())
            }
            error => {
                self.delivery
                    .send_text(
                        target,
                        &self.catalog.input_submission_failed(&error.to_string()),
                        None,
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                Ok(())
            }
        }
    }

    async fn handle_command(&self, command: TelegramCommand) -> Result<(), TelegramPollingError> {
        if !self
            .bridge
            .access()
            .allows(command.context.chat_id, command.context.user_id)
        {
            return Ok(());
        }
        match TelegramCockpitCommand::from_name(&command.name) {
            Some(TelegramCockpitCommand::Start | TelegramCockpitCommand::Help) => {
                self.send_command_help(command).await
            }
            Some(TelegramCockpitCommand::Status) => self.send_status(command).await,
            Some(TelegramCockpitCommand::New) => self.create_new_session(command).await,
            Some(TelegramCockpitCommand::Switch) => self.switch_or_list_sessions(command).await,
            Some(TelegramCockpitCommand::Sessions) => self.send_sessions(command).await,
            Some(TelegramCockpitCommand::Profiles) => self.send_profiles(command).await,
            Some(TelegramCockpitCommand::Continue) => self.continue_active_session(command).await,
            Some(TelegramCockpitCommand::Abort) => self.abort_active_session(command).await,
            Some(TelegramCockpitCommand::Queue) => self.send_or_update_queue(command).await,
            Some(TelegramCockpitCommand::Approvals) => self.send_pending_approvals(command).await,
            Some(TelegramCockpitCommand::Approve) => {
                self.resolve_approval_command(command, ToolPermissionOutcome::Allow)
                    .await
            }
            Some(TelegramCockpitCommand::Deny) => {
                self.resolve_approval_command(command, ToolPermissionOutcome::Deny)
                    .await
            }
            Some(TelegramCockpitCommand::Processes) => self.send_processes(command).await,
            Some(TelegramCockpitCommand::Process) => self.send_process(command).await,
            Some(TelegramCockpitCommand::Manifest) => self.send_manifest(command).await,
            Some(TelegramCockpitCommand::Subagent) => self.spawn_subagent(command).await,
            Some(command_id) => self.send_command_not_ready(command, command_id).await,
            None => self.send_unknown_command_help(command).await,
        }
    }

    async fn send_command_help(
        &self,
        command: TelegramCommand,
    ) -> Result<(), TelegramPollingError> {
        self.send_command_text(command, &render_command_help(self.catalog))
            .await
    }

    async fn send_unknown_command_help(
        &self,
        command: TelegramCommand,
    ) -> Result<(), TelegramPollingError> {
        let text = render_unknown_command_help(&command.name, self.catalog);
        self.send_command_text(command, &text).await
    }

    async fn send_command_not_ready(
        &self,
        command: TelegramCommand,
        command_id: TelegramCockpitCommand,
    ) -> Result<(), TelegramPollingError> {
        self.send_command_text(command, &self.catalog.command_not_ready(command_id))
            .await
    }

    async fn send_profiles(&self, command: TelegramCommand) -> Result<(), TelegramPollingError> {
        let profiles = self
            .bridge
            .list_profiles()
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        let mut text = self.catalog.profile_list_title(profiles.len());
        let mut keyboard = Vec::new();
        {
            let mut actions = self.session_actions.lock().await;
            for (index, profile) in profiles.iter().enumerate() {
                text.push('\n');
                text.push_str(&self.catalog.profile_item(
                    index + 1,
                    &profile.display_name,
                    &profile.profile_id,
                ));
                keyboard.push(vec![actions.button(
                    format!("{} {}", self.catalog.select_button(), profile.display_name),
                    TelegramSessionAction::SelectProfile {
                        profile_id: profile.profile_id.clone(),
                    },
                )]);
            }
        }
        self.send_command_text_with_markup(
            command,
            &text,
            Some(TelegramInlineKeyboardMarkup {
                inline_keyboard: keyboard,
            }),
        )
        .await
    }

    async fn create_new_session(
        &self,
        command: TelegramCommand,
    ) -> Result<(), TelegramPollingError> {
        let session_id = command_key(&command).derived_session_id(command.context.message_id);
        let descriptor = self
            .bridge
            .create_chat_session(&command.context, session_id)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        self.send_command_text(
            command,
            &self
                .catalog
                .session_created(&descriptor.session_id, &descriptor.profile_id),
        )
        .await
    }

    async fn switch_or_list_sessions(
        &self,
        command: TelegramCommand,
    ) -> Result<(), TelegramPollingError> {
        if command.args.trim().is_empty() {
            return self.send_sessions(command).await;
        }
        let key = command_key(&command);
        let descriptor = self
            .bridge
            .switch_session(key, command.args.trim())
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        self.send_command_text(
            command,
            &self.catalog.session_switched(&descriptor.session_id),
        )
        .await
    }

    async fn send_sessions(&self, command: TelegramCommand) -> Result<(), TelegramPollingError> {
        let key = command_key(&command);
        let sessions = self
            .bridge
            .list_sessions_for_chat(&key)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        if sessions.is_empty() {
            return self
                .send_command_text(command, self.catalog.no_sessions())
                .await;
        }
        let active_session_id = self.bridge.session_id(&key);
        let mut text = self.catalog.session_list_title(sessions.len());
        let mut keyboard = Vec::new();
        {
            let mut actions = self.session_actions.lock().await;
            for (index, session) in sessions.iter().enumerate() {
                text.push('\n');
                text.push_str(&self.catalog.session_item(
                    index + 1,
                    &session.session_id,
                    &session.profile_id,
                    &session.status,
                    active_session_id.as_deref() == Some(session.session_id.as_str()),
                ));
                let mut row = Vec::new();
                if active_session_id.as_deref() != Some(session.session_id.as_str()) {
                    row.push(actions.button(
                        self.catalog.switch_button(),
                        TelegramSessionAction::SwitchSession {
                            session_id: session.session_id.clone(),
                        },
                    ));
                }
                row.push(actions.button(
                    self.catalog.delete_button(),
                    TelegramSessionAction::RequestDelete {
                        session_id: session.session_id.clone(),
                    },
                ));
                keyboard.push(row);
            }
        }
        self.send_command_text_with_markup(
            command,
            &text,
            Some(TelegramInlineKeyboardMarkup {
                inline_keyboard: keyboard,
            }),
        )
        .await
    }

    async fn send_status(&self, command: TelegramCommand) -> Result<(), TelegramPollingError> {
        let key = command_key(&command);
        let Some(session_id) = self.bridge.session_id(&key) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let descriptor = self
            .bridge
            .get_session(&session_id)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        let text = self.catalog.status_card(TelegramStatusCard {
            session_id: &descriptor.session_id,
            profile_id: &descriptor.profile_id,
            status: &descriptor.status,
            messages: descriptor.state.messages.len(),
            tools: descriptor.manifest.enabled_tools.len(),
            pending_approvals: descriptor.state.pending_tool_approvals.len(),
            plugins: descriptor.manifest.plugins.len(),
        });
        self.send_command_text(command, &text).await
    }

    async fn send_pending_approvals(
        &self,
        command: TelegramCommand,
    ) -> Result<(), TelegramPollingError> {
        let key = TelegramSessionKey::new(command.context.chat_id, command.context.thread_id);
        let text = match self.bridge.session_id(&key) {
            Some(session_id) => {
                let approvals = self
                    .bridge
                    .list_approvals(&session_id)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                render_pending_approval_requests(&approvals, self.catalog)
            }
            None => self.catalog.pending_approvals_empty().into(),
        };
        self.send_command_text(command, &text).await
    }

    async fn resolve_approval_command(
        &self,
        command: TelegramCommand,
        outcome: ToolPermissionOutcome,
    ) -> Result<(), TelegramPollingError> {
        let Some(session_id) = self.active_session_id(&command) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let approvals = self
            .bridge
            .list_approvals(&session_id)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        let Some(approval_id) = select_approval_id(&approvals, &command.args) else {
            return self
                .send_command_text(
                    command,
                    &render_pending_approval_requests(&approvals, self.catalog),
                )
                .await;
        };
        let key = command_key(&command);
        let decision = telegram_approval_decision(
            outcome.clone(),
            self.catalog.approval_resolution_reason(),
            telegram_approver(&command.context),
        );
        self.bridge
            .resolve_approval(&session_id, &approval_id, decision)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        if self
            .resolve_visible_approval_card(key, &approval_id, outcome.clone())
            .await?
        {
            return Ok(());
        }
        self.send_command_text(command, &self.catalog.approval_resolved(&outcome))
            .await
    }

    async fn resolve_visible_approval_card(
        &self,
        key: TelegramSessionKey,
        approval_id: &str,
        outcome: ToolPermissionOutcome,
    ) -> Result<bool, TelegramPollingError> {
        let selection = {
            let state = self.display_states.lock().await.get(&key).cloned();
            match state {
                Some(state) => state
                    .lock()
                    .await
                    .lookup_approval_id(approval_id, outcome.clone()),
                None => None,
            }
        };
        let Some(selection) = selection else {
            return Ok(false);
        };
        self.clear_approval_card(selection.target.message, &outcome)
            .await?;
        if let Some(state) = self.display_states.lock().await.get(&key).cloned() {
            let _ = state
                .lock()
                .await
                .resolve_approval_id(approval_id, outcome.clone());
        }
        Ok(true)
    }

    async fn clear_approval_card(
        &self,
        message: TelegramMessageHandle,
        outcome: &ToolPermissionOutcome,
    ) -> Result<(), TelegramPollingError> {
        let target = TelegramMessageTarget::chat(message.chat_id);
        if self
            .delivery
            .delete_message(target, message.message_id)
            .await
            .is_ok()
        {
            return Ok(());
        }
        self.delivery
            .edit_text(
                target,
                message.message_id,
                &self.catalog.approval_resolved(outcome),
                Some(TelegramInlineKeyboardMarkup::empty()),
            )
            .await
            .map(|_| ())
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))
    }

    async fn continue_active_session(
        &self,
        command: TelegramCommand,
    ) -> Result<(), TelegramPollingError> {
        let Some(session_id) = self.active_session_id(&command) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let descriptor = self
            .bridge
            .continue_session(&session_id)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        self.send_command_text(command, &self.catalog.run_continued(&descriptor.session_id))
            .await
    }

    async fn abort_active_session(
        &self,
        command: TelegramCommand,
    ) -> Result<(), TelegramPollingError> {
        let Some(session_id) = self.active_session_id(&command) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let descriptor = self
            .bridge
            .get_session(&session_id)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        if matches!(
            descriptor.status,
            noloong_agent::interaction::InteractionSessionStatus::Running
                | noloong_agent::interaction::InteractionSessionStatus::Paused
        ) {
            let button = self.session_actions.lock().await.button(
                self.catalog.confirm_abort_button(),
                TelegramSessionAction::ConfirmAbort {
                    session_id: descriptor.session_id.clone(),
                },
            );
            return self
                .send_command_text_with_markup(
                    command,
                    &self.catalog.run_abort_confirm(&descriptor.session_id),
                    Some(single_button_markup(button)),
                )
                .await;
        }

        let descriptor = self
            .bridge
            .abort_session(&session_id)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        self.send_command_text(command, &self.catalog.run_aborted(&descriptor.session_id))
            .await
    }

    async fn send_or_update_queue(
        &self,
        command: TelegramCommand,
    ) -> Result<(), TelegramPollingError> {
        let Some(session_id) = self.active_session_id(&command) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let args = command.args.trim();
        if !args.is_empty() {
            let descriptor = self
                .bridge
                .submit_follow_up_text(&command.context, &session_id, args.to_owned())
                .await
                .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            return self
                .send_command_text(
                    command,
                    &self.catalog.queue_follow_up_added(&descriptor.session_id),
                )
                .await;
        }

        let snapshot = self
            .bridge
            .list_queues(&session_id)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        let reply_markup = self.queue_markup(session_id).await;
        self.send_command_text_with_markup(
            command,
            &self.catalog.queue_card(&snapshot),
            Some(reply_markup),
        )
        .await
    }

    async fn queue_markup(&self, session_id: String) -> TelegramInlineKeyboardMarkup {
        const QUEUE_KINDS: [TelegramQueueKind; 2] =
            [TelegramQueueKind::Steering, TelegramQueueKind::FollowUp];
        const QUEUE_MODES: [QueueMode; 2] = [QueueMode::All, QueueMode::OneAtATime];

        let mut actions = self.session_actions.lock().await;
        let mut inline_keyboard = Vec::new();
        inline_keyboard.push(
            QUEUE_KINDS
                .iter()
                .map(|queue| {
                    actions.button(
                        self.catalog.clear_queue_button(*queue),
                        TelegramSessionAction::ClearQueue {
                            session_id: session_id.clone(),
                            queue: *queue,
                        },
                    )
                })
                .collect(),
        );
        inline_keyboard.extend(QUEUE_KINDS.iter().map(|queue| {
            QUEUE_MODES
                .iter()
                .map(|mode| {
                    actions.button(
                        self.catalog.set_queue_mode_button(*queue, *mode),
                        TelegramSessionAction::SetQueueMode {
                            session_id: session_id.clone(),
                            queue: *queue,
                            mode: *mode,
                        },
                    )
                })
                .collect()
        }));
        TelegramInlineKeyboardMarkup { inline_keyboard }
    }

    fn active_session_id(&self, command: &TelegramCommand) -> Option<String> {
        self.bridge.session_id(&command_key(command))
    }

    async fn send_processes(&self, command: TelegramCommand) -> Result<(), TelegramPollingError> {
        let Some(session_id) = self.active_session_id(&command) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let processes = self
            .bridge
            .list_processes(&session_id)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        if processes.is_empty() {
            return self
                .send_command_text(command, self.catalog.no_processes())
                .await;
        }

        let mut text = self.catalog.process_list_title(processes.len());
        let mut keyboard = Vec::new();
        {
            let mut actions = self.session_actions.lock().await;
            for (index, process) in processes.iter().enumerate() {
                text.push('\n');
                text.push_str(&self.catalog.process_item(index + 1, process));
                keyboard.push(vec![actions.button(
                    format!(
                        "{} {}",
                        self.catalog.open_process_button(),
                        process_snapshot_label(process)
                    ),
                    TelegramSessionAction::OpenProcess {
                        session_id: session_id.clone(),
                        job_id: process.job_id.clone(),
                    },
                )]);
            }
        }
        self.send_command_text_with_markup(
            command,
            &text,
            Some(TelegramInlineKeyboardMarkup {
                inline_keyboard: keyboard,
            }),
        )
        .await
    }

    async fn send_process(&self, command: TelegramCommand) -> Result<(), TelegramPollingError> {
        let Some(session_id) = self.active_session_id(&command) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let Some((job_id, operation)) = parse_process_command_args(&command.args) else {
            return self
                .send_command_text(command, self.catalog.process_usage())
                .await;
        };
        match operation {
            ProcessCommandOperation::Inspect => {
                let output = self.read_process_output(&session_id, &job_id, None).await?;
                self.send_process_output(
                    TelegramMessageTarget::new(command.context.chat_id, command.context.thread_id),
                    None,
                    &session_id,
                    &output,
                )
                .await
            }
            ProcessCommandOperation::Write { text } => {
                let button = self.session_actions.lock().await.button(
                    self.catalog.confirm_write_button(),
                    TelegramSessionAction::ConfirmWriteProcess {
                        session_id,
                        job_id: job_id.clone(),
                        text: text.clone(),
                    },
                );
                self.send_command_text_with_markup(
                    command,
                    &self.catalog.process_write_confirm(&job_id, &text),
                    Some(single_button_markup(button)),
                )
                .await
            }
        }
    }

    async fn read_process_output(
        &self,
        session_id: &str,
        job_id: &str,
        after_seq: Option<u64>,
    ) -> Result<noloong_agent::ProcessOutput, TelegramPollingError> {
        self.bridge
            .read_process(
                session_id,
                job_id,
                after_seq,
                Some(process_output_read_max_bytes()),
                Some(process_output_wait_ms()),
            )
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))
    }

    async fn send_process_output(
        &self,
        target: TelegramMessageTarget,
        message_id: Option<i64>,
        session_id: &str,
        output: &noloong_agent::ProcessOutput,
    ) -> Result<(), TelegramPollingError> {
        let output_text = render_process_output(output);
        let buttons = self.process_output_markup(session_id, output).await;
        if output_text.chars().count() > PROCESS_OUTPUT_INLINE_CHAR_LIMIT {
            self.api
                .send_document(TelegramSendDocumentRequest {
                    chat_id: target.chat_id,
                    document: TelegramInputFile::bytes(
                        process_output_filename(&output.job_id),
                        process_output_document_bytes(output),
                    )
                    .with_mime_type("text/plain"),
                    options: TelegramMediaMessageOptions {
                        message_thread_id: target.message_thread_id,
                        caption: Some(self.catalog.process_output_attached(&output.job_id)),
                        parse_mode: None,
                        reply_parameters: None,
                        reply_markup: None,
                    },
                })
                .await
                .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            let text = self.catalog.process_output_attached(&output.job_id);
            return self
                .send_or_edit_process_card(target, message_id, &text, Some(buttons))
                .await;
        }

        let text = self.catalog.process_output_card(
            &output.job_id,
            &output.status,
            &output_text,
            output.truncated,
        );
        self.send_or_edit_process_card(target, message_id, &text, Some(buttons))
            .await
    }

    async fn send_or_edit_process_card(
        &self,
        target: TelegramMessageTarget,
        message_id: Option<i64>,
        text: &str,
        reply_markup: Option<TelegramInlineKeyboardMarkup>,
    ) -> Result<(), TelegramPollingError> {
        match message_id {
            Some(message_id) => {
                self.delivery
                    .edit_text(target, message_id, text, reply_markup)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            None => {
                self.delivery
                    .send_text(target, text, reply_markup)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
        }
        Ok(())
    }

    async fn process_output_markup(
        &self,
        session_id: &str,
        output: &noloong_agent::ProcessOutput,
    ) -> TelegramInlineKeyboardMarkup {
        let mut actions = self.session_actions.lock().await;
        TelegramInlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![
                    actions.button(
                        self.catalog.read_process_button(),
                        TelegramSessionAction::ReadProcess {
                            session_id: session_id.into(),
                            job_id: output.job_id.clone(),
                            after_seq: Some(output.next_cursor),
                        },
                    ),
                    actions.button(
                        self.catalog.wait_process_button(),
                        TelegramSessionAction::WaitProcess {
                            session_id: session_id.into(),
                            job_id: output.job_id.clone(),
                        },
                    ),
                ],
                vec![actions.button(
                    self.catalog.terminate_process_button(),
                    TelegramSessionAction::RequestTerminateProcess {
                        session_id: session_id.into(),
                        job_id: output.job_id.clone(),
                    },
                )],
            ],
        }
    }

    async fn send_manifest(&self, command: TelegramCommand) -> Result<(), TelegramPollingError> {
        let Some(session_id) = self.active_session_id(&command) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let (manifest, system_prompt, proposals) = tokio::try_join!(
            self.bridge.get_manifest(&session_id),
            self.bridge.get_system_prompt(&session_id),
            self.bridge.list_manifest_proposals(&session_id),
        )
        .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        let text = self.catalog.manifest_card(TelegramManifestCard {
            manifest: &manifest,
            system_prompt: &system_prompt,
            proposals: &proposals,
        });
        let reply_markup = self.manifest_markup(&session_id, &proposals).await;
        self.send_command_text_with_markup(command, &text, reply_markup)
            .await
    }

    async fn manifest_markup(
        &self,
        session_id: &str,
        proposals: &[noloong_agent::ManifestPatchProposal],
    ) -> Option<TelegramInlineKeyboardMarkup> {
        if proposals.is_empty() {
            return None;
        }
        let mut actions = self.session_actions.lock().await;
        Some(TelegramInlineKeyboardMarkup {
            inline_keyboard: proposals
                .iter()
                .take(MANIFEST_PROPOSAL_DISPLAY_LIMIT)
                .map(|proposal| {
                    vec![actions.button(
                        format!(
                            "{} {}",
                            self.catalog.approve_manifest_button(),
                            proposal.proposal_id
                        ),
                        TelegramSessionAction::ApproveManifestProposal {
                            session_id: session_id.into(),
                            proposal_id: proposal.proposal_id.clone(),
                        },
                    )]
                })
                .collect(),
        })
    }

    async fn spawn_subagent(&self, command: TelegramCommand) -> Result<(), TelegramPollingError> {
        let Some(parent_session_id) = self.active_session_id(&command) else {
            return self
                .send_command_text(command, self.catalog.no_active_session())
                .await;
        };
        let Some(args) = parse_subagent_command_args(&command.args) else {
            return self
                .send_command_text(command, self.catalog.subagent_usage())
                .await;
        };
        let descriptor = self
            .bridge
            .spawn_subagent(
                &command.context,
                &parent_session_id,
                Some(args.role),
                args.initial_prompt,
            )
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        self.send_command_text(command, &self.catalog.subagent_spawned(&descriptor))
            .await
    }

    async fn send_command_text(
        &self,
        command: TelegramCommand,
        text: &str,
    ) -> Result<(), TelegramPollingError> {
        self.send_command_text_with_markup(command, text, None)
            .await
    }

    async fn send_command_text_with_markup(
        &self,
        command: TelegramCommand,
        text: &str,
        reply_markup: Option<TelegramInlineKeyboardMarkup>,
    ) -> Result<(), TelegramPollingError> {
        self.delivery
            .send_text(
                TelegramMessageTarget::new(command.context.chat_id, command.context.thread_id),
                text,
                reply_markup,
            )
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        Ok(())
    }

    async fn handle_media_message(
        &self,
        message: TelegramInboundMessage,
    ) -> Result<(), TelegramPollingError> {
        let target = TelegramMessageTarget::new(message.context.chat_id, message.context.thread_id);
        self.bridge
            .preflight_inbound_message(&message, self.bot_username.as_deref())
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        let resolved = match self
            .media_resolver
            .resolve_all_with_notices(&message.attachments)
            .await
        {
            Ok(resolved) => resolved,
            Err(error) => {
                self.delivery
                    .send_text(target, &self.catalog.media_resolution_failed(&error), None)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                return Ok(());
            }
        };
        if !resolved.notices.is_empty() {
            self.delivery
                .send_text(
                    target,
                    &self
                        .catalog
                        .unsupported_media_fallback_notices(&resolved.notices),
                    None,
                )
                .await
                .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        }
        match self
            .bridge
            .handle_inbound_message(message, resolved.media, self.bot_username.as_deref())
            .await
        {
            Ok(_) => Ok(()),
            Err(error) => self.handle_agent_submission_error(target, error).await,
        }
    }

    async fn handle_callback(
        &self,
        callback: TelegramCallbackQuery,
    ) -> Result<(), TelegramPollingError> {
        let Some(message) = callback.message else {
            return Ok(());
        };
        let chat_id = message.chat.id;
        let key = TelegramSessionKey::new(chat_id, message.message_thread_id);
        if !self.bridge.access().allows(chat_id, Some(callback.from.id)) {
            self.api
                .answer_callback_query(&callback.id, Some(self.catalog.callback_not_allowed()))
                .await
                .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            return Ok(());
        }
        let Some(data) = callback.data else {
            return Ok(());
        };
        if TelegramSessionActionStore::is_session_action(&data) {
            return self
                .handle_session_callback(
                    callback.id,
                    message.message_id,
                    TelegramMessageTarget::new(chat_id, message.message_thread_id),
                    key,
                    data,
                )
                .await;
        }
        let selection = {
            let state = self.display_states.lock().await.get(&key).cloned();
            match state {
                Some(state) => state.lock().await.lookup_approval_callback(&data),
                None => None,
            }
        };
        let Some(selection) = selection else {
            self.api
                .answer_callback_query(&callback.id, Some(self.catalog.callback_approval_expired()))
                .await
                .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            return Ok(());
        };
        let outcome = selection.outcome.clone();
        let target = selection.target.clone();
        selection
            .apply(&self.bridge, callback.from.id, self.catalog)
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        self.clear_approval_card(target.message, &outcome).await?;
        if let Some(state) = self.display_states.lock().await.get(&key).cloned() {
            let _ = state.lock().await.resolve_approval_callback(&data);
        }
        self.api
            .answer_callback_query(&callback.id, Some(self.catalog.callback_recorded()))
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        Ok(())
    }

    async fn handle_session_callback(
        &self,
        callback_id: String,
        message_id: i64,
        target: TelegramMessageTarget,
        key: TelegramSessionKey,
        data: String,
    ) -> Result<(), TelegramPollingError> {
        let action = self.session_actions.lock().await.resolve(&data);
        let Some(action) = action else {
            self.api
                .answer_callback_query(&callback_id, Some(self.catalog.callback_action_expired()))
                .await
                .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            return Ok(());
        };
        match action {
            TelegramSessionAction::SelectProfile { profile_id } => {
                self.bridge.set_preferred_profile(key, profile_id.clone());
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.profile_selected(&profile_id),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::SwitchSession { session_id } => {
                let descriptor = self
                    .bridge
                    .switch_session(key, &session_id)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.session_switched(&descriptor.session_id),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::RequestDelete { session_id } => {
                let descriptor = self
                    .bridge
                    .get_session(&session_id)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                let force_abort = matches!(
                    descriptor.status,
                    noloong_agent::interaction::InteractionSessionStatus::Running
                        | noloong_agent::interaction::InteractionSessionStatus::Paused
                );
                let button = self.session_actions.lock().await.button(
                    self.catalog.confirm_delete_button(),
                    TelegramSessionAction::ConfirmDelete {
                        session_id,
                        force_abort,
                    },
                );
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self
                            .catalog
                            .session_delete_confirm(&descriptor.session_id, force_abort),
                        Some(single_button_markup(button)),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::ConfirmDelete {
                session_id,
                force_abort,
            } => {
                self.bridge
                    .delete_session(key, &session_id, force_abort)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.session_deleted(&session_id),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::ConfirmAbort { session_id } => {
                let descriptor = self
                    .bridge
                    .abort_session(&session_id)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.run_aborted(&descriptor.session_id),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::ClearQueue { session_id, queue } => {
                let messages = self
                    .bridge
                    .clear_queue(&session_id, queue)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.queue_cleared(queue, messages.len()),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::SetQueueMode {
                session_id,
                queue,
                mode,
            } => {
                let messages = self
                    .bridge
                    .set_queue_mode(&session_id, queue, mode)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.queue_mode_updated(queue, mode, messages.len()),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::OpenProcess { session_id, job_id } => {
                let output = self.read_process_output(&session_id, &job_id, None).await?;
                self.send_process_output(target, Some(message_id), &session_id, &output)
                    .await?;
            }
            TelegramSessionAction::ReadProcess {
                session_id,
                job_id,
                after_seq,
            } => {
                let output = self
                    .read_process_output(&session_id, &job_id, after_seq)
                    .await?;
                self.send_process_output(target, Some(message_id), &session_id, &output)
                    .await?;
            }
            TelegramSessionAction::WaitProcess { session_id, job_id } => {
                let outcome = self
                    .bridge
                    .wait_process(&session_id, &job_id, Some(process_wait_timeout_ms()))
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.process_wait_result(&outcome),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::RequestTerminateProcess { session_id, job_id } => {
                let button = self.session_actions.lock().await.button(
                    self.catalog.terminate_process_button(),
                    TelegramSessionAction::ConfirmTerminateProcess {
                        session_id,
                        job_id: job_id.clone(),
                    },
                );
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.process_terminate_confirm(&job_id),
                        Some(single_button_markup(button)),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::ConfirmTerminateProcess { session_id, job_id } => {
                let snapshot = self
                    .bridge
                    .terminate_process(&session_id, &job_id)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.process_terminated(&snapshot),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::RequestWriteProcess {
                session_id,
                job_id,
                text,
            } => {
                let button = self.session_actions.lock().await.button(
                    self.catalog.confirm_write_button(),
                    TelegramSessionAction::ConfirmWriteProcess {
                        session_id,
                        job_id: job_id.clone(),
                        text: text.clone(),
                    },
                );
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.process_write_confirm(&job_id, &text),
                        Some(single_button_markup(button)),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::ConfirmWriteProcess {
                session_id,
                job_id,
                text,
            } => {
                let snapshot = self
                    .bridge
                    .write_process(&session_id, &job_id, &text)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.process_written(&snapshot),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::ApproveManifestProposal {
                session_id,
                proposal_id,
            } => {
                let proposal = self
                    .bridge
                    .approve_manifest_proposal(&session_id, &proposal_id)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                let button = self.session_actions.lock().await.button(
                    self.catalog.apply_manifest_button(),
                    TelegramSessionAction::RequestApplyApprovedManifest { session_id },
                );
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.manifest_proposal_approved(&proposal),
                        Some(single_button_markup(button)),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::RequestApplyApprovedManifest { session_id } => {
                let button = self.session_actions.lock().await.button(
                    self.catalog.confirm_apply_manifest_button(),
                    TelegramSessionAction::ConfirmApplyApprovedManifest {
                        session_id: session_id.clone(),
                    },
                );
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.manifest_apply_confirm(&session_id),
                        Some(single_button_markup(button)),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            TelegramSessionAction::ConfirmApplyApprovedManifest { session_id } => {
                let result = self
                    .bridge
                    .apply_approved_manifest(&session_id)
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
                self.delivery
                    .edit_text(
                        target,
                        message_id,
                        &self.catalog.manifest_applied(&result.applied_proposal_ids),
                        Some(TelegramInlineKeyboardMarkup::empty()),
                    )
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
        }
        self.api
            .answer_callback_query(&callback_id, Some(self.catalog.callback_recorded()))
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        Ok(())
    }
}

fn command_key(command: &TelegramCommand) -> TelegramSessionKey {
    TelegramSessionKey::new(command.context.chat_id, command.context.thread_id)
}

fn telegram_approver(context: &noloong_agent_telegram::input::TelegramInboundContext) -> String {
    context
        .user_id
        .map(|user_id| format!("telegram:{user_id}"))
        .unwrap_or_else(|| format!("telegram:chat:{}", context.chat_id))
}

fn telegram_approval_decision(
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

fn select_approval_id(
    approvals: &BTreeMap<String, ToolApprovalRequest>,
    selector: &str,
) -> Option<String> {
    if approvals.is_empty() {
        return None;
    }
    let selector = selector.trim();
    if selector.is_empty() {
        return (approvals.len() == 1).then(|| approvals.keys().next().cloned())?;
    }
    if let Ok(index) = selector.parse::<usize>() {
        return index
            .checked_sub(1)
            .and_then(|index| approvals.keys().nth(index))
            .cloned();
    }
    approvals
        .contains_key(selector)
        .then(|| selector.to_owned())
}

enum ProcessCommandOperation {
    Inspect,
    Write { text: String },
}

fn parse_process_command_args(args: &str) -> Option<(String, ProcessCommandOperation)> {
    let args = args.trim();
    if args.is_empty() {
        return None;
    }
    let (job_id, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let rest = rest.trim();
    if rest.is_empty() {
        return Some((job_id.into(), ProcessCommandOperation::Inspect));
    }
    let Some(text) = rest.strip_prefix("write").map(str::trim) else {
        return Some((job_id.into(), ProcessCommandOperation::Inspect));
    };
    (!text.is_empty()).then_some((
        job_id.into(),
        ProcessCommandOperation::Write { text: text.into() },
    ))
}

struct SubagentCommandArgs {
    role: String,
    initial_prompt: Option<String>,
}

fn parse_subagent_command_args(args: &str) -> Option<SubagentCommandArgs> {
    let args = args.trim();
    if args.is_empty() {
        return None;
    }
    let (role, prompt) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let prompt = prompt.trim();
    let initial_prompt = (!prompt.is_empty()).then(|| prompt.to_owned());
    Some(SubagentCommandArgs {
        role: role.into(),
        initial_prompt,
    })
}

fn has_explicit_telegram_media_fallback(options: &TelegramBridgeOptions) -> bool {
    options
        .unsupported_media_fallback_to_file
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || process_env(DEFAULT_TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_ENV)
            .is_some_and(|value| !value.trim().is_empty())
}

fn apply_profile_media_fallback_policy(
    file_policy: &mut TelegramFilePolicy,
    profile_config: &HostProfileConfig,
    selected_profile_id: Option<&str>,
) {
    if !file_policy.unsupported_media_fallback.is_native() {
        return;
    }
    file_policy.unsupported_media_fallback =
        profile_media_fallback_policy(profile_config, selected_profile_id);
}

fn profile_media_fallback_policy(
    profile_config: &HostProfileConfig,
    selected_profile_id: Option<&str>,
) -> TelegramUnsupportedMediaFallbackPolicy {
    profile_config
        .selected_profile(selected_profile_id)
        .map(|profile| provider_media_fallback_policy(&profile.provider))
        .unwrap_or_default()
}

fn provider_media_fallback_policy(
    provider: &BuiltInProviderConfig,
) -> TelegramUnsupportedMediaFallbackPolicy {
    match provider {
        BuiltInProviderConfig::Responses {
            allow_file_data_url_input,
            ..
        }
        | BuiltInProviderConfig::ChatgptResponses {
            allow_file_data_url_input,
            ..
        } => {
            if *allow_file_data_url_input {
                let file_input = TelegramNativeMediaHandling::file_for_mime_types(
                    RESPONSES_FILE_DATA_MIME_TYPES,
                );
                TelegramUnsupportedMediaFallbackPolicy {
                    audio: file_input.clone(),
                    voice: file_input.clone(),
                    video: file_input,
                }
            } else {
                TelegramUnsupportedMediaFallbackPolicy {
                    audio: TelegramNativeMediaHandling::Unsupported,
                    voice: TelegramNativeMediaHandling::Unsupported,
                    video: TelegramNativeMediaHandling::Unsupported,
                }
            }
        }
        BuiltInProviderConfig::AnthropicMessages { .. } => TelegramUnsupportedMediaFallbackPolicy {
            audio: TelegramNativeMediaHandling::Unsupported,
            voice: TelegramNativeMediaHandling::Unsupported,
            video: TelegramNativeMediaHandling::Unsupported,
        },
        BuiltInProviderConfig::ChatCompletions { .. } => {
            let supported_audio = TelegramNativeMediaHandling::native_for_mime_types(
                CHAT_COMPLETIONS_NATIVE_AUDIO_MIME_TYPES,
            );
            TelegramUnsupportedMediaFallbackPolicy {
                audio: supported_audio.clone(),
                voice: supported_audio,
                video: TelegramNativeMediaHandling::Native,
            }
        }
    }
}

fn telegram_unsupported_media_fallback_policy(
    cli_value: Option<String>,
    env_value: Option<String>,
) -> Result<TelegramUnsupportedMediaFallbackPolicy, CliError> {
    let Some(value) = cli_value
        .or(env_value)
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(TelegramUnsupportedMediaFallbackPolicy::default());
    };
    let mut policy = TelegramUnsupportedMediaFallbackPolicy::default();
    let mut saw_none = false;
    let mut saw_media = false;
    for token in value
        .split(',')
        .map(|token| token.trim().to_ascii_lowercase().replace('-', "_"))
        .filter(|token| !token.is_empty())
    {
        match token.as_str() {
            "all" => {
                saw_media = true;
                policy = TelegramUnsupportedMediaFallbackPolicy::file_for_audio_voice_video();
            }
            "audio" => {
                saw_media = true;
                policy.audio = TelegramNativeMediaHandling::File;
            }
            "voice" => {
                saw_media = true;
                policy.voice = TelegramNativeMediaHandling::File;
            }
            "video" => {
                saw_media = true;
                policy.video = TelegramNativeMediaHandling::File;
            }
            "none" | "native" => saw_none = true,
            _ => {
                return Err(config::CliConfigError::ParseConfig(format!(
                    "invalid {DEFAULT_TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_ENV}: {token}"
                ))
                .into());
            }
        }
    }
    if saw_none && saw_media {
        return Err(config::CliConfigError::ParseConfig(format!(
            "{DEFAULT_TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_ENV} cannot combine none/native with media kinds"
        ))
        .into());
    }
    Ok(if saw_none {
        TelegramUnsupportedMediaFallbackPolicy::default()
    } else {
        policy
    })
}

fn telegram_config_from_values(
    options: &TelegramBridgeOptions,
    env_source: impl Fn(&str) -> Option<String>,
) -> Result<noloong_agent_telegram::config::TelegramBridgeConfig, CliError> {
    let interaction_ws_url = options
        .interaction_url
        .clone()
        .or_else(|| env_source(DEFAULT_INTERACTION_URL_ENV))
        .ok_or(config::CliConfigError::MissingEnv(
            DEFAULT_INTERACTION_URL_ENV.into(),
        ))?;
    let bot_token = options
        .bot_token
        .clone()
        .or_else(|| env_source(DEFAULT_TELEGRAM_BOT_TOKEN_ENV))
        .ok_or(config::CliConfigError::MissingEnv(
            DEFAULT_TELEGRAM_BOT_TOKEN_ENV.into(),
        ))?;
    let users = parse_csv_u64(
        options
            .allowed_users
            .clone()
            .or_else(|| env_source(DEFAULT_TELEGRAM_ALLOWED_USERS_ENV)),
    )?;
    let chats = parse_csv_i64(
        options
            .allowed_chats
            .clone()
            .or_else(|| env_source(DEFAULT_TELEGRAM_ALLOWED_CHATS_ENV)),
    )?;
    let mut access = if options.allow_all {
        TelegramAccessPolicy::allow_all()
    } else {
        TelegramAccessPolicy::new(chats, users)
    };
    access.require_mention_in_groups = parse_bool_env(
        env_source(DEFAULT_TELEGRAM_REQUIRE_MENTION_ENV),
        access.require_mention_in_groups,
    );
    let locale = resolve_locale(options.locale, env_source(DEFAULT_TELEGRAM_LOCALE_ENV))?;
    let default_file_policy = TelegramFilePolicy::default();
    let file_policy = TelegramFilePolicy {
        inline_max_bytes: parse_config_usize(
            options.file_inline_max_bytes,
            env_source(DEFAULT_TELEGRAM_FILE_INLINE_MAX_BYTES_ENV),
            default_file_policy.inline_max_bytes,
            DEFAULT_TELEGRAM_FILE_INLINE_MAX_BYTES_ENV,
        )?,
        max_download_bytes: parse_config_usize(
            options.file_max_download_bytes,
            env_source(DEFAULT_TELEGRAM_FILE_MAX_DOWNLOAD_BYTES_ENV),
            default_file_policy.max_download_bytes,
            DEFAULT_TELEGRAM_FILE_MAX_DOWNLOAD_BYTES_ENV,
        )?,
        download_dir: options.file_download_dir.clone().or_else(|| {
            non_empty_option(env_source(DEFAULT_TELEGRAM_FILE_DOWNLOAD_DIR_ENV)).map(PathBuf::from)
        }),
        retention_seconds: parse_config_optional_u64(
            options.file_retention_seconds,
            env_source(DEFAULT_TELEGRAM_FILE_RETENTION_SECONDS_ENV),
            DEFAULT_TELEGRAM_FILE_RETENTION_SECONDS_ENV,
        )?,
        unsupported_media_fallback: telegram_unsupported_media_fallback_policy(
            options.unsupported_media_fallback_to_file.clone(),
            env_source(DEFAULT_TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_ENV),
        )?,
    };
    let startup_update_policy = telegram_startup_update_policy(
        options.startup_update_policy,
        env_source(DEFAULT_TELEGRAM_STARTUP_UPDATE_POLICY_ENV),
    )?;
    let network = TelegramNetworkConfig {
        proxy_url: non_empty_option(env_source(DEFAULT_TELEGRAM_PROXY_ENV)),
        fallback_ips: parse_csv_strings(env_source(DEFAULT_TELEGRAM_FALLBACK_IPS_ENV)),
        disable_fallback_ips: parse_bool_env(
            env_source(DEFAULT_TELEGRAM_DISABLE_FALLBACK_IPS_ENV),
            false,
        ),
        disable_env_proxy: parse_bool_env(
            env_source(DEFAULT_TELEGRAM_DISABLE_ENV_PROXY_ENV),
            false,
        ),
        ..TelegramNetworkConfig::default()
    };
    let config = noloong_agent_telegram::config::TelegramBridgeConfig {
        bot_token,
        bot_username: options
            .bot_username
            .clone()
            .or_else(|| env_source(DEFAULT_TELEGRAM_BOT_USERNAME_ENV)),
        interaction_ws_url,
        interaction_bearer_token: options
            .interaction_token
            .clone()
            .or_else(|| {
                options
                    .interaction_token_env
                    .as_deref()
                    .and_then(|env_name| interaction_token(Some(env_name)))
            })
            .or_else(|| env_source(DEFAULT_INTERACTION_TOKEN_ENV)),
        profile_id: options.profile_id.clone(),
        message_window_ms: 600,
        long_split_window_ms: 2_000,
        edit_throttle_ms: 750,
        max_outbound_chars: 3900,
        access,
        network,
        file_policy,
        startup_update_policy,
        show_tool_status: true,
        locale,
    };
    config.validate()?;
    Ok(config)
}

fn parse_telegram_startup_update_policy_arg(
    value: &str,
) -> Result<TelegramStartupUpdatePolicy, String> {
    value
        .parse::<TelegramStartupUpdatePolicy>()
        .map_err(|error| error.to_string())
}

fn telegram_startup_update_policy(
    cli_policy: Option<TelegramStartupUpdatePolicy>,
    env_policy: Option<String>,
) -> Result<TelegramStartupUpdatePolicy, CliError> {
    if let Some(policy) = cli_policy {
        return Ok(policy);
    }
    let Some(value) = env_policy.filter(|value| !value.trim().is_empty()) else {
        return Ok(TelegramStartupUpdatePolicy::default());
    };
    value
        .parse::<TelegramStartupUpdatePolicy>()
        .map_err(|error| {
            config::CliConfigError::ParseConfig(format!(
                "invalid {DEFAULT_TELEGRAM_STARTUP_UPDATE_POLICY_ENV}: {error}"
            ))
            .into()
        })
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(crate) struct TelegramBridgeOptions {
    #[arg(long = "interaction-url")]
    pub(crate) interaction_url: Option<String>,
    #[arg(long = "interaction-token")]
    pub(crate) interaction_token: Option<String>,
    #[arg(long = "interaction-token-env")]
    pub(crate) interaction_token_env: Option<String>,
    #[arg(long = "telegram-bot-token")]
    pub(crate) bot_token: Option<String>,
    #[arg(long = "telegram-bot-username")]
    pub(crate) bot_username: Option<String>,
    #[arg(long = "telegram-allowed-users")]
    pub(crate) allowed_users: Option<String>,
    #[arg(long = "telegram-allowed-chats")]
    pub(crate) allowed_chats: Option<String>,
    #[arg(long = "telegram-allow-all")]
    pub(crate) allow_all: bool,
    #[arg(long = "telegram-locale", value_parser = parse_locale_arg)]
    pub(crate) locale: Option<Locale>,
    #[arg(long = "telegram-file-inline-max-bytes")]
    pub(crate) file_inline_max_bytes: Option<usize>,
    #[arg(long = "telegram-file-max-download-bytes")]
    pub(crate) file_max_download_bytes: Option<usize>,
    #[arg(long = "telegram-file-download-dir")]
    pub(crate) file_download_dir: Option<PathBuf>,
    #[arg(long = "telegram-file-retention-seconds")]
    pub(crate) file_retention_seconds: Option<u64>,
    #[arg(long = "telegram-unsupported-media-fallback-to-file")]
    pub(crate) unsupported_media_fallback_to_file: Option<String>,
    #[arg(long = "telegram-startup-update-policy", value_parser = parse_telegram_startup_update_policy_arg)]
    pub(crate) startup_update_policy: Option<TelegramStartupUpdatePolicy>,
    #[arg(long = "profile-id")]
    pub(crate) profile_id: Option<String>,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(crate) struct TelegramOptions {
    #[arg(long = "profile-config")]
    pub(crate) profile_config: Option<String>,
    #[command(flatten)]
    pub(crate) bridge: TelegramBridgeOptions,
}

#[cfg(test)]
#[path = "telegram_cli_tests.rs"]
mod tests;

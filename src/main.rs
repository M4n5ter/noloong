mod build_info;
mod chatgpt;
mod config;
mod host;
mod models_dev;
mod schema;
#[cfg(test)]
mod test_support;

use crate::{
    config::{
        BuiltInProviderConfig, DEFAULT_INTERACTION_TOKEN_ENV, DEFAULT_INTERACTION_URL_ENV,
        DEFAULT_PROFILE_CONFIG_ENV, DEFAULT_TELEGRAM_ALLOWED_CHATS_ENV,
        DEFAULT_TELEGRAM_ALLOWED_USERS_ENV, DEFAULT_TELEGRAM_BOT_TOKEN_ENV,
        DEFAULT_TELEGRAM_BOT_USERNAME_ENV, DEFAULT_TELEGRAM_DISABLE_ENV_PROXY_ENV,
        DEFAULT_TELEGRAM_DISABLE_FALLBACK_IPS_ENV, DEFAULT_TELEGRAM_FALLBACK_IPS_ENV,
        DEFAULT_TELEGRAM_FILE_DOWNLOAD_DIR_ENV, DEFAULT_TELEGRAM_FILE_INLINE_MAX_BYTES_ENV,
        DEFAULT_TELEGRAM_FILE_MAX_DOWNLOAD_BYTES_ENV, DEFAULT_TELEGRAM_FILE_RETENTION_SECONDS_ENV,
        DEFAULT_TELEGRAM_LOCALE_ENV, DEFAULT_TELEGRAM_PROXY_ENV,
        DEFAULT_TELEGRAM_REQUIRE_MENTION_ENV, DEFAULT_TELEGRAM_STARTUP_UPDATE_POLICY_ENV,
        DEFAULT_TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_ENV, DEFAULT_WEIXIN_ACCOUNT_ID_ENV,
        DEFAULT_WEIXIN_ALLOW_ALL_ENV, DEFAULT_WEIXIN_ALLOWED_USERS_ENV,
        DEFAULT_WEIXIN_BASE_URL_ENV, DEFAULT_WEIXIN_CDN_BASE_URL_ENV,
        DEFAULT_WEIXIN_FILE_DOWNLOAD_DIR_ENV, DEFAULT_WEIXIN_FILE_INLINE_MAX_BYTES_ENV,
        DEFAULT_WEIXIN_FILE_MAX_DOWNLOAD_BYTES_ENV, DEFAULT_WEIXIN_FILE_MAX_UPLOAD_BYTES_ENV,
        DEFAULT_WEIXIN_LOCALE_ENV, DEFAULT_WEIXIN_TOKEN_ENV, HostProfileConfig,
        ensure_sqlite_database_parent, env_or_value, parse_bool_env, parse_csv_i64, parse_csv_u64,
        resolve_state_database_url,
    },
    host::build_registry,
};
use clap::{Args, Parser, Subcommand};
use noloong_agent::{
    Locale, ManifestPatch, SqliteClientStateStore,
    approval::{allow_decision, deny_decision},
    interaction::{
        InteractionCapabilityPolicy, InteractionClientError, InteractionControlHandler,
        InteractionHttpTransportConfig, InteractionTransportAuth, InteractionWsClient,
        InteractionWsClientConfig, serve_interaction_http,
    },
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
        ReqwestTelegramApi, TelegramApi, TelegramApiError, TelegramInlineKeyboardMarkup,
        TelegramInputFile, TelegramMediaMessageOptions, TelegramMessageHandle,
        TelegramSendDocumentRequest,
    },
};
use noloong_agent_weixin::{
    config::{
        ILINK_BASE_URL, WEIXIN_CDN_BASE_URL, WeixinAccessPolicy, WeixinBridgeConfig,
        WeixinFilePolicy,
    },
    login::{WeixinLoginOptions, run_qr_login},
    runtime::run_weixin_bridge_with_config,
    state::WeixinAccountStore,
};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{net::TcpListener, sync::Mutex};

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

#[tokio::main]
async fn main() {
    init_process_diagnostics();
    if let Err(error) = run_cli(env::args().skip(1).collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn init_process_diagnostics() {
    human_panic::setup_panic!();
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}

async fn run_cli(args: Vec<String>) -> Result<(), CliError> {
    let cli = Cli::try_parse_from(std::iter::once("noloong".to_owned()).chain(args))
        .map_err(|error| CliError::Usage(error.to_string()))?;
    match cli.command {
        CliCommand::Serve(ServeCommand {
            command: ServeSubcommand::Interaction(options),
        }) => run_serve_interaction(options).await,
        CliCommand::ChatGpt(options) => chatgpt::run_chatgpt(options).await.map_err(Into::into),
        CliCommand::BuildInfo(command) => run_build_info(command),
        CliCommand::ProfileConfig(ProfileConfigCommand {
            command: ProfileConfigSubcommand::Schema(options),
        }) => run_profile_config_schema(options),
        CliCommand::TelegramBridge(options) => run_telegram_bridge(options).await,
        CliCommand::Telegram(options) => run_telegram(options).await,
        CliCommand::Weixin(command) => run_weixin(command).await,
    }
}

fn run_profile_config_schema(options: ProfileConfigSchemaOptions) -> Result<(), CliError> {
    if options.output.is_some() && options.check.is_some() {
        return Err(CliError::Schema(
            "--output cannot be used together with --check".into(),
        ));
    }
    if let Some(check_path) = options.check {
        return check_profile_config_schema(&check_path);
    }
    let schema = schema::profile_config_schema_json();
    if let Some(output_path) = options.output {
        return write_profile_config_schema(&output_path, &schema);
    }
    io::stdout().lock().write_all(schema.as_bytes())?;
    Ok(())
}

fn run_build_info(command: BuildInfoCommand) -> Result<(), CliError> {
    match command.command {
        BuildInfoSubcommand::Manifest => {
            io::stdout()
                .lock()
                .write_all(build_info::manifest_json().as_bytes())?;
        }
        BuildInfoSubcommand::Command => {
            writeln!(io::stdout().lock(), "{}", build_info::build_command()?)?;
        }
        BuildInfoSubcommand::Source(command) => run_build_info_source(command)?,
    }
    Ok(())
}

fn run_build_info_source(command: BuildInfoSourceCommand) -> Result<(), CliError> {
    match command.command {
        BuildInfoSourceSubcommand::List => {
            let mut stdout = io::stdout().lock();
            for path in build_info::source_paths()? {
                writeln!(stdout, "{path}")?;
            }
        }
        BuildInfoSourceSubcommand::Cat(options) => {
            io::stdout()
                .lock()
                .write_all(&build_info::source_file(&options.path)?)?;
        }
        BuildInfoSourceSubcommand::Extract(options) => {
            build_info::extract_source(&options.output_dir, options.force)?;
        }
        BuildInfoSourceSubcommand::Archive(options) => {
            build_info::write_archive(&options.output)?;
        }
    }
    Ok(())
}

fn check_profile_config_schema(path: &Path) -> Result<(), CliError> {
    let current = fs::read_to_string(path)?;
    let expected = schema::profile_config_schema_json();
    if current == expected {
        return Ok(());
    }
    Err(CliError::Schema(format!(
        "profile config schema is out of date: {}; regenerate it with `noloong profile-config schema --output {}`",
        path.display(),
        path.display()
    )))
}

fn write_profile_config_schema(path: &Path, schema: &str) -> Result<(), CliError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, schema)?;
    Ok(())
}

async fn run_serve_interaction(options: ServeInteractionOptions) -> Result<(), CliError> {
    let profile_config = load_profile_config(options.profile_config)?;
    let registry = build_registry(&profile_config).await?;
    let bind = options.bind.unwrap_or_else(default_interaction_bind);
    let token = interaction_token(options.interaction_token_env.as_deref());
    validate_interaction_bind(bind, token.as_deref())?;
    let listener = TcpListener::bind(bind).await?;
    log::info!("interaction server listening on {}", listener.local_addr()?);
    serve_interaction_http(
        listener,
        InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all()),
        InteractionHttpTransportConfig {
            auth: token
                .map(InteractionTransportAuth::BearerToken)
                .unwrap_or(InteractionTransportAuth::None),
            ..InteractionHttpTransportConfig::default()
        },
    )
    .await?;
    Ok(())
}

async fn run_telegram_bridge(options: TelegramBridgeOptions) -> Result<(), CliError> {
    let config = telegram_config_from_values(&options, process_env)?;
    run_telegram_bridge_with_config(config).await
}

async fn run_telegram(options: TelegramOptions) -> Result<(), CliError> {
    let profile_config = load_profile_config(options.profile_config)?;
    let registry = build_registry(&profile_config).await?;
    let token = generate_token()?;
    let listener = TcpListener::bind(default_embedded_interaction_bind()).await?;
    let address = listener.local_addr()?;
    let server_token = token.clone();
    let server = tokio::spawn(async move {
        serve_interaction_http(
            listener,
            InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all()),
            InteractionHttpTransportConfig::bearer_token(server_token),
        )
        .await
    });
    let mut bridge_options = options.bridge;
    bridge_options.interaction_url = Some(format!("ws://{address}/jsonrpc/ws"));
    bridge_options.interaction_token = Some(token);
    let has_explicit_media_fallback = has_explicit_telegram_media_fallback(&bridge_options);
    let mut bridge_config = telegram_config_from_values(&bridge_options, process_env)?;
    if !has_explicit_media_fallback {
        apply_profile_media_fallback_policy(
            &mut bridge_config.file_policy,
            &profile_config,
            bridge_config.profile_id.as_deref(),
        );
    }
    tokio::select! {
        result = run_telegram_bridge_with_config(bridge_config) => result,
        result = server => {
            result.map_err(|error| CliError::Task(error.to_string()))?
                .map_err(CliError::Interaction)
        }
    }
}

async fn run_weixin(command: WeixinCommand) -> Result<(), CliError> {
    match command.command {
        WeixinSubcommand::Login(options) => run_weixin_login(options).await,
        WeixinSubcommand::Bridge(options) => run_weixin_bridge(options).await,
        WeixinSubcommand::Run(options) => run_weixin_embedded(options).await,
    }
}

async fn run_weixin_login(options: WeixinLoginCliOptions) -> Result<(), CliError> {
    let client = reqwest::Client::builder()
        .user_agent("noloong-weixin-login")
        .build()
        .map_err(|error| {
            CliError::WeixinRuntime(
                noloong_agent_weixin::runtime::WeixinRuntimeError::HttpClient(error.to_string()),
            )
        })?;
    let store = WeixinAccountStore::default_root();
    run_qr_login(
        client,
        &store,
        WeixinLoginOptions {
            bot_type: options.bot_type,
            timeout_seconds: options.timeout_seconds,
            qr_png_path: options.qr_png_path,
        },
        io::stdout().lock(),
    )
    .await?;
    Ok(())
}

async fn run_weixin_bridge(options: WeixinBridgeOptions) -> Result<(), CliError> {
    let config = weixin_config_from_values(&options, process_env)?;
    run_weixin_bridge_config(config).await
}

async fn run_weixin_embedded(options: WeixinRunOptions) -> Result<(), CliError> {
    let profile_config = load_profile_config(options.profile_config)?;
    let registry = build_registry(&profile_config).await?;
    let token = generate_token()?;
    let listener = TcpListener::bind(default_embedded_interaction_bind()).await?;
    let address = listener.local_addr()?;
    let server_token = token.clone();
    let server = tokio::spawn(async move {
        serve_interaction_http(
            listener,
            InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all()),
            InteractionHttpTransportConfig::bearer_token(server_token),
        )
        .await
    });
    let mut bridge_options = options.bridge;
    if bridge_options.locale.is_none() && process_env(DEFAULT_WEIXIN_LOCALE_ENV).is_none() {
        bridge_options.locale =
            profile_locale(&profile_config, bridge_options.profile_id.as_deref());
    }
    bridge_options.interaction_url = Some(format!("ws://{address}/jsonrpc/ws"));
    bridge_options.interaction_token = Some(token);
    let bridge_config = weixin_config_from_values(&bridge_options, process_env)?;
    tokio::select! {
        result = run_weixin_bridge_config(bridge_config) => result,
        result = server => {
            result.map_err(|error| CliError::Task(error.to_string()))?
                .map_err(CliError::Interaction)
        }
    }
}

async fn run_weixin_bridge_config(config: WeixinBridgeConfig) -> Result<(), CliError> {
    let state_database_url = resolve_state_database_url()?;
    ensure_sqlite_database_parent(&state_database_url)?;
    run_weixin_bridge_with_config(config, state_database_url).await?;
    Ok(())
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

fn load_profile_config(path: Option<String>) -> Result<HostProfileConfig, CliError> {
    let path = env_or_value(path, DEFAULT_PROFILE_CONFIG_ENV)
        .ok_or(config::CliConfigError::MissingProfileConfig)?;
    let config = HostProfileConfig::load(path)?;
    config.validate()?;
    Ok(config)
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
    let profile_id = selected_profile_id
        .or(profile_config.default_profile_id.as_deref())
        .unwrap_or_else(|| profile_config.profiles[0].profile_id.as_str());
    let Some(profile) = profile_config
        .profiles
        .iter()
        .find(|profile| profile.profile_id == profile_id)
    else {
        return TelegramUnsupportedMediaFallbackPolicy::default();
    };
    provider_media_fallback_policy(&profile.provider)
}

fn profile_locale(
    profile_config: &HostProfileConfig,
    selected_profile_id: Option<&str>,
) -> Option<Locale> {
    let profile_id = selected_profile_id
        .or(profile_config.default_profile_id.as_deref())
        .unwrap_or_else(|| profile_config.profiles[0].profile_id.as_str());
    let profile = profile_config
        .profiles
        .iter()
        .find(|profile| profile.profile_id == profile_id)?;
    profile
        .manifest_patches
        .iter()
        .rev()
        .find_map(|patch| match patch {
            ManifestPatch::SetLocale { locale } => Some(*locale),
            _ => None,
        })
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
    let locale = telegram_locale(options.locale, env_source(DEFAULT_TELEGRAM_LOCALE_ENV))?;
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
        fallback_ips: env_source(DEFAULT_TELEGRAM_FALLBACK_IPS_ENV)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
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

fn weixin_config_from_values(
    options: &WeixinBridgeOptions,
    env_source: impl Fn(&str) -> Option<String>,
) -> Result<WeixinBridgeConfig, CliError> {
    let interaction_ws_url = options
        .interaction_url
        .clone()
        .or_else(|| env_source(DEFAULT_INTERACTION_URL_ENV))
        .ok_or(config::CliConfigError::MissingEnv(
            DEFAULT_INTERACTION_URL_ENV.into(),
        ))?;
    let account_id = options
        .account_id
        .clone()
        .or_else(|| env_source(DEFAULT_WEIXIN_ACCOUNT_ID_ENV))
        .ok_or(config::CliConfigError::MissingEnv(
            DEFAULT_WEIXIN_ACCOUNT_ID_ENV.into(),
        ))?;
    let store = WeixinAccountStore::default_root();
    let stored_account = store.load(&account_id)?;
    let token = options
        .token
        .clone()
        .or_else(|| env_source(DEFAULT_WEIXIN_TOKEN_ENV))
        .or_else(|| stored_account.as_ref().map(|account| account.token.clone()))
        .ok_or(config::CliConfigError::MissingEnv(
            DEFAULT_WEIXIN_TOKEN_ENV.into(),
        ))?;
    let base_url = options
        .base_url
        .clone()
        .or_else(|| env_source(DEFAULT_WEIXIN_BASE_URL_ENV))
        .or_else(|| {
            stored_account
                .as_ref()
                .map(|account| account.base_url.clone())
        })
        .unwrap_or_else(|| ILINK_BASE_URL.into());
    let cdn_base_url = options
        .cdn_base_url
        .clone()
        .or_else(|| env_source(DEFAULT_WEIXIN_CDN_BASE_URL_ENV))
        .unwrap_or_else(|| WEIXIN_CDN_BASE_URL.into());
    let allowed_users = parse_csv_strings(
        options
            .allowed_users
            .clone()
            .or_else(|| env_source(DEFAULT_WEIXIN_ALLOWED_USERS_ENV)),
    );
    let allow_all =
        options.allow_all || parse_bool_env(env_source(DEFAULT_WEIXIN_ALLOW_ALL_ENV), false);
    let access = if allow_all {
        WeixinAccessPolicy::allow_all()
    } else {
        WeixinAccessPolicy::new(allowed_users)
    };
    let locale = weixin_locale(options.locale, env_source(DEFAULT_WEIXIN_LOCALE_ENV))?;
    let default_file_policy = WeixinFilePolicy::default();
    let file_policy = WeixinFilePolicy {
        inline_max_bytes: parse_config_usize(
            options.file_inline_max_bytes,
            env_source(DEFAULT_WEIXIN_FILE_INLINE_MAX_BYTES_ENV),
            default_file_policy.inline_max_bytes,
            DEFAULT_WEIXIN_FILE_INLINE_MAX_BYTES_ENV,
        )?,
        max_download_bytes: parse_config_usize(
            options.file_max_download_bytes,
            env_source(DEFAULT_WEIXIN_FILE_MAX_DOWNLOAD_BYTES_ENV),
            default_file_policy.max_download_bytes,
            DEFAULT_WEIXIN_FILE_MAX_DOWNLOAD_BYTES_ENV,
        )?,
        max_upload_bytes: parse_config_usize(
            options.file_max_upload_bytes,
            env_source(DEFAULT_WEIXIN_FILE_MAX_UPLOAD_BYTES_ENV),
            default_file_policy.max_upload_bytes,
            DEFAULT_WEIXIN_FILE_MAX_UPLOAD_BYTES_ENV,
        )?,
        download_dir: options.file_download_dir.clone().or_else(|| {
            non_empty_option(env_source(DEFAULT_WEIXIN_FILE_DOWNLOAD_DIR_ENV)).map(PathBuf::from)
        }),
    };
    let config = WeixinBridgeConfig {
        account_id,
        token,
        base_url,
        cdn_base_url,
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
        max_outbound_chars: options.max_outbound_chars.unwrap_or(2000),
        access,
        file_policy,
        locale,
    };
    config.validate()?;
    Ok(config)
}

fn interaction_token(token_env: Option<&str>) -> Option<String> {
    token_env
        .and_then(|env_name| env_or_value(None, env_name))
        .or_else(|| env_or_value(None, DEFAULT_INTERACTION_TOKEN_ENV))
}

fn validate_interaction_bind(bind: SocketAddr, token: Option<&str>) -> Result<(), CliError> {
    if bind.ip().is_loopback() || token.is_some_and(|token| !token.trim().is_empty()) {
        return Ok(());
    }
    Err(CliError::PublicBindWithoutToken(bind))
}

fn default_interaction_bind() -> SocketAddr {
    "127.0.0.1:8787"
        .parse()
        .expect("default interaction bind address is valid")
}

fn default_embedded_interaction_bind() -> SocketAddr {
    "127.0.0.1:0"
        .parse()
        .expect("default embedded interaction bind address is valid")
}

fn process_env(name: &str) -> Option<String> {
    env::var(name).ok()
}

fn non_empty_option(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn telegram_locale(
    cli_locale: Option<Locale>,
    env_locale: Option<String>,
) -> Result<Locale, CliError> {
    if let Some(locale) = cli_locale {
        return Ok(locale);
    }
    parse_locale_option(env_locale)?.map_or_else(|| Ok(Locale::detect()), Ok)
}

fn weixin_locale(
    cli_locale: Option<Locale>,
    env_locale: Option<String>,
) -> Result<Locale, CliError> {
    if let Some(locale) = cli_locale {
        return Ok(locale);
    }
    parse_locale_option(env_locale)?.map_or_else(|| Ok(Locale::detect()), Ok)
}

fn parse_csv_strings(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_locale_option(value: Option<String>) -> Result<Option<Locale>, CliError> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    Locale::parse(&value).map(Some).ok_or_else(|| {
        config::CliConfigError::ParseConfig(format!("invalid locale: {value}")).into()
    })
}

fn parse_locale_arg(value: &str) -> Result<Locale, String> {
    Locale::parse(value).ok_or_else(|| format!("invalid locale: {value}"))
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

fn parse_config_usize(
    cli_value: Option<usize>,
    env_value: Option<String>,
    default_value: usize,
    env_name: &str,
) -> Result<usize, CliError> {
    if let Some(value) = cli_value {
        return Ok(value);
    }
    let Some(value) = env_value.filter(|value| !value.trim().is_empty()) else {
        return Ok(default_value);
    };
    value.trim().parse::<usize>().map_err(|error| {
        config::CliConfigError::ParseConfig(format!("invalid {env_name}: {error}")).into()
    })
}

fn parse_config_optional_u64(
    cli_value: Option<u64>,
    env_value: Option<String>,
    env_name: &str,
) -> Result<Option<u64>, CliError> {
    if cli_value.is_some() {
        return Ok(cli_value);
    }
    let Some(value) = env_value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    value.trim().parse::<u64>().map(Some).map_err(|error| {
        config::CliConfigError::ParseConfig(format!("invalid {env_name}: {error}")).into()
    })
}

fn stable_fingerprint(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn generate_token() -> Result<String, CliError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| CliError::Random(error.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
struct TelegramBridgeOptions {
    #[arg(long = "interaction-url")]
    interaction_url: Option<String>,
    #[arg(long = "interaction-token")]
    interaction_token: Option<String>,
    #[arg(long = "interaction-token-env")]
    interaction_token_env: Option<String>,
    #[arg(long = "telegram-bot-token")]
    bot_token: Option<String>,
    #[arg(long = "telegram-bot-username")]
    bot_username: Option<String>,
    #[arg(long = "telegram-allowed-users")]
    allowed_users: Option<String>,
    #[arg(long = "telegram-allowed-chats")]
    allowed_chats: Option<String>,
    #[arg(long = "telegram-allow-all")]
    allow_all: bool,
    #[arg(long = "telegram-locale", value_parser = parse_locale_arg)]
    locale: Option<Locale>,
    #[arg(long = "telegram-file-inline-max-bytes")]
    file_inline_max_bytes: Option<usize>,
    #[arg(long = "telegram-file-max-download-bytes")]
    file_max_download_bytes: Option<usize>,
    #[arg(long = "telegram-file-download-dir")]
    file_download_dir: Option<PathBuf>,
    #[arg(long = "telegram-file-retention-seconds")]
    file_retention_seconds: Option<u64>,
    #[arg(long = "telegram-unsupported-media-fallback-to-file")]
    unsupported_media_fallback_to_file: Option<String>,
    #[arg(long = "telegram-startup-update-policy", value_parser = parse_telegram_startup_update_policy_arg)]
    startup_update_policy: Option<TelegramStartupUpdatePolicy>,
    #[arg(long = "profile-id")]
    profile_id: Option<String>,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
struct TelegramOptions {
    #[arg(long = "profile-config")]
    profile_config: Option<String>,
    #[command(flatten)]
    bridge: TelegramBridgeOptions,
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct WeixinCommand {
    #[command(subcommand)]
    command: WeixinSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
enum WeixinSubcommand {
    Login(WeixinLoginCliOptions),
    Bridge(WeixinBridgeOptions),
    Run(WeixinRunOptions),
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct WeixinLoginCliOptions {
    #[arg(long = "bot-type", default_value = "3")]
    bot_type: String,
    #[arg(long = "timeout-seconds", default_value_t = 480)]
    timeout_seconds: u64,
    #[arg(long = "qr-png")]
    qr_png_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
struct WeixinBridgeOptions {
    #[arg(long = "interaction-url")]
    interaction_url: Option<String>,
    #[arg(long = "interaction-token")]
    interaction_token: Option<String>,
    #[arg(long = "interaction-token-env")]
    interaction_token_env: Option<String>,
    #[arg(long = "weixin-account-id")]
    account_id: Option<String>,
    #[arg(long = "weixin-token")]
    token: Option<String>,
    #[arg(long = "weixin-base-url")]
    base_url: Option<String>,
    #[arg(long = "weixin-cdn-base-url")]
    cdn_base_url: Option<String>,
    #[arg(long = "weixin-allowed-users")]
    allowed_users: Option<String>,
    #[arg(long = "weixin-allow-all")]
    allow_all: bool,
    #[arg(long = "weixin-locale", value_parser = parse_locale_arg)]
    locale: Option<Locale>,
    #[arg(long = "weixin-max-outbound-chars")]
    max_outbound_chars: Option<usize>,
    #[arg(long = "weixin-file-inline-max-bytes")]
    file_inline_max_bytes: Option<usize>,
    #[arg(long = "weixin-file-max-download-bytes")]
    file_max_download_bytes: Option<usize>,
    #[arg(long = "weixin-file-max-upload-bytes")]
    file_max_upload_bytes: Option<usize>,
    #[arg(long = "weixin-file-download-dir")]
    file_download_dir: Option<PathBuf>,
    #[arg(long = "profile-id")]
    profile_id: Option<String>,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
struct WeixinRunOptions {
    #[arg(long = "profile-config")]
    profile_config: Option<String>,
    #[command(flatten)]
    bridge: WeixinBridgeOptions,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
struct ServeInteractionOptions {
    #[arg(long = "profile-config")]
    profile_config: Option<String>,
    #[arg(long = "bind")]
    bind: Option<SocketAddr>,
    #[arg(long = "interaction-token-env")]
    interaction_token_env: Option<String>,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
struct ProfileConfigSchemaOptions {
    #[arg(long = "output", conflicts_with = "check")]
    output: Option<PathBuf>,
    #[arg(long = "check", conflicts_with = "output")]
    check: Option<PathBuf>,
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct BuildInfoCommand {
    #[command(subcommand)]
    command: BuildInfoSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
enum BuildInfoSubcommand {
    Manifest,
    Command,
    Source(BuildInfoSourceCommand),
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct BuildInfoSourceCommand {
    #[command(subcommand)]
    command: BuildInfoSourceSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
enum BuildInfoSourceSubcommand {
    List,
    Cat(BuildInfoSourceCatOptions),
    Extract(BuildInfoSourceExtractOptions),
    Archive(BuildInfoSourceArchiveOptions),
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct BuildInfoSourceCatOptions {
    path: String,
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct BuildInfoSourceExtractOptions {
    #[arg(long = "output-dir")]
    output_dir: PathBuf,
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct BuildInfoSourceArchiveOptions {
    #[arg(long)]
    output: PathBuf,
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct ProfileConfigCommand {
    #[command(subcommand)]
    command: ProfileConfigSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
enum ProfileConfigSubcommand {
    Schema(ProfileConfigSchemaOptions),
}

#[derive(Clone, Debug, Parser, PartialEq, Eq)]
#[command(name = "noloong", version, about = "Noloong agent runtime")]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
enum CliCommand {
    Serve(ServeCommand),
    #[command(name = "chatgpt")]
    ChatGpt(chatgpt::ChatGptOptions),
    #[command(name = "build-info")]
    BuildInfo(BuildInfoCommand),
    #[command(name = "profile-config")]
    ProfileConfig(ProfileConfigCommand),
    #[command(name = "telegram-bridge")]
    TelegramBridge(TelegramBridgeOptions),
    Telegram(TelegramOptions),
    Weixin(WeixinCommand),
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
struct ServeCommand {
    #[command(subcommand)]
    command: ServeSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
enum ServeSubcommand {
    Interaction(ServeInteractionOptions),
}

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Config(#[from] config::CliConfigError),
    #[error("{0}")]
    Host(#[from] host::HostBuildError),
    #[error("{0}")]
    ChatGpt(#[from] chatgpt::ChatGptCliError),
    #[error("{0}")]
    BuildInfo(#[from] build_info::BuildInfoError),
    #[error("interaction transport failed: {0}")]
    Interaction(#[from] noloong_agent::interaction::InteractionError),
    #[error("interaction client failed: {0}")]
    InteractionClient(#[from] noloong_agent::interaction::InteractionClientError),
    #[error("client state failed: {0}")]
    ClientState(#[from] noloong_agent::ClientStateError),
    #[error("Telegram bridge failed: {0}")]
    TelegramBridge(#[from] noloong_agent_telegram::bridge::TelegramBridgeError),
    #[error("Telegram config failed: {0}")]
    TelegramConfig(#[from] noloong_agent_telegram::config::TelegramConfigError),
    #[error("Telegram network failed: {0}")]
    TelegramNetwork(#[from] noloong_agent_telegram::network::TelegramNetworkError),
    #[error("Telegram API failed: {0}")]
    TelegramApi(#[from] TelegramApiError),
    #[error("Telegram delivery failed: {0}")]
    TelegramDelivery(#[from] noloong_agent_telegram::delivery::TelegramDeliveryError),
    #[error("Telegram polling failed: {0}")]
    Polling(TelegramPollingError),
    #[error("Weixin bridge failed: {0}")]
    WeixinConfig(#[from] noloong_agent_weixin::config::WeixinConfigError),
    #[error("Weixin runtime failed: {0}")]
    WeixinRuntime(#[from] noloong_agent_weixin::runtime::WeixinRuntimeError),
    #[error("Weixin login failed: {0}")]
    WeixinLogin(#[from] noloong_agent_weixin::login::WeixinLoginError),
    #[error("Weixin state failed: {0}")]
    WeixinState(#[from] noloong_agent_weixin::state::WeixinStateError),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("background task failed: {0}")]
    Task(String),
    #[error("cannot listen on public address {0} without an interaction bearer token")]
    PublicBindWithoutToken(SocketAddr),
    #[error("random token generation failed: {0}")]
    Random(String),
    #[error("{0}")]
    Schema(String),
    #[error("{0}")]
    Usage(String),
}

#[cfg(test)]
mod tests {
    use super::{
        BridgeUpdateHandler, BuildInfoSourceSubcommand, BuildInfoSubcommand, Cli, CliCommand,
        CliError, ProfileConfigSchemaOptions, ProfileConfigSubcommand, TelegramBridgeOptions,
        WeixinBridgeOptions, WeixinSubcommand, apply_profile_media_fallback_policy, profile_locale,
        profile_media_fallback_policy, register_telegram_commands, run_profile_config_schema,
        telegram_config_from_values, validate_interaction_bind, weixin_config_from_values,
    };
    use crate::config::HostProfileConfig;
    use crate::schema::profile_config_schema_json;
    use crate::test_support::{remove_temp_file, write_temp_file};
    use clap::Parser;
    use noloong_agent::{
        AgentManifest, JobSnapshot, JobStatus, Locale, ManifestPatch, ManifestPatchProposal,
        OutputChunk, ProcessOutput, ProcessOutputStream, SystemPromptAddition, WaitOutcome,
        interaction::{
            InteractionClientError, InteractionProfileDescriptor, InteractionSessionDescriptor,
            InteractionSessionStatus, InteractionWsNotification,
        },
    };
    use noloong_agent_core::{
        AgentMessage, AgentState, QueueMode, ToolApprovalRequest, ToolApprovalRequestSpec, ToolCall,
    };
    use noloong_agent_telegram::{
        access::{TelegramChatKind, TelegramTextInput},
        bridge::{TelegramBridgeError, TelegramInteractionClient, TelegramInteractionFuture},
        config::{TelegramFilePolicy, TelegramNativeMediaDecision, TelegramNativeMediaHandling},
        delivery::{TelegramDelivery, TelegramMessageTarget},
        display::{TelegramDisplayState, deliver_display_event},
        i18n::TelegramUiCatalog,
        input::{TelegramCommand, TelegramInboundContext, TelegramInboundUpdate},
        media::TelegramAttachmentResolver,
        polling::{
            TelegramCallbackQuery, TelegramChat, TelegramMessage, TelegramUpdate, TelegramUser,
        },
        process::{PROCESS_OUTPUT_INLINE_CHAR_LIMIT, process_output_read_max_bytes},
        queue::{TelegramQueueKind, TelegramQueuedMessage, TelegramQueuedMessageIntent},
        session::{TelegramSessionActionStore, telegram_session_metadata},
        telegram_api::{
            TelegramApi, TelegramApiError, TelegramDeleteMessageRequest,
            TelegramEditMessageTextRequest, TelegramMessageHandle, TelegramSendDocumentRequest,
            TelegramSendMessageRequest, TelegramSetMyCommandsRequest,
        },
    };
    use serde_json::{Value, json};
    use std::{
        collections::BTreeMap,
        future::Future,
        net::SocketAddr,
        path::PathBuf,
        pin::Pin,
        sync::{Arc, Mutex as StdMutex},
    };
    use tokio::sync::{Mutex, broadcast};

    #[test]
    fn cli_serve_rejects_public_bind_without_token() {
        let bind: SocketAddr = "0.0.0.0:8787".parse().unwrap();

        let error = validate_interaction_bind(bind, None).unwrap_err();

        assert!(matches!(error, CliError::PublicBindWithoutToken(_)));
    }

    #[test]
    fn cli_telegram_bridge_requires_interaction_url() {
        let options = TelegramBridgeOptions {
            bot_token: Some("token".into()),
            allowed_users: Some("123456789".into()),
            ..Default::default()
        };

        let error = telegram_config_from_values(&options, |_| None).unwrap_err();

        assert!(error.to_string().contains("NOLOONG_INTERACTION_URL"));
    }

    #[test]
    fn cli_telegram_bridge_requires_allowlist() {
        let options = TelegramBridgeOptions {
            interaction_url: Some("ws://127.0.0.1:8787/jsonrpc/ws".into()),
            bot_token: Some("token".into()),
            ..Default::default()
        };

        let error = telegram_config_from_values(&options, |_| None).unwrap_err();

        assert!(error.to_string().contains("allowlist"));
    }

    #[test]
    fn cli_telegram_embeds_loopback_interaction_options() {
        let cli = Cli::try_parse_from([
            "noloong",
            "telegram",
            "--profile-config",
            "profiles.json",
            "--telegram-bot-username",
            "noloong_bot",
            "--telegram-allowed-users",
            "123456789",
            "--telegram-locale",
            "zh",
        ])
        .unwrap();

        let CliCommand::Telegram(options) = cli.command else {
            panic!("expected telegram command");
        };
        assert_eq!(options.profile_config.as_deref(), Some("profiles.json"));
        assert_eq!(options.bridge.bot_username.as_deref(), Some("noloong_bot"));
        assert_eq!(options.bridge.allowed_users.as_deref(), Some("123456789"));
        assert_eq!(options.bridge.locale, Some(Locale::Zh));
    }

    #[test]
    fn cli_weixin_run_embeds_loopback_interaction_options() {
        let cli = Cli::try_parse_from([
            "noloong",
            "weixin",
            "run",
            "--profile-config",
            "profiles.json",
            "--weixin-account-id",
            "wx-bot",
            "--weixin-allowed-users",
            "user-1,user-2",
            "--weixin-locale",
            "zh",
        ])
        .unwrap();

        let CliCommand::Weixin(command) = cli.command else {
            panic!("expected weixin command");
        };
        let WeixinSubcommand::Run(options) = command.command else {
            panic!("expected weixin run");
        };
        assert_eq!(options.profile_config.as_deref(), Some("profiles.json"));
        assert_eq!(options.bridge.account_id.as_deref(), Some("wx-bot"));
        assert_eq!(
            options.bridge.allowed_users.as_deref(),
            Some("user-1,user-2")
        );
        assert_eq!(options.bridge.locale, Some(Locale::Zh));
    }

    #[test]
    fn cli_profile_config_schema_command_parses() {
        let cli = Cli::try_parse_from([
            "noloong",
            "profile-config",
            "schema",
            "--check",
            "schemas/profile-config.schema.json",
        ])
        .unwrap();

        let CliCommand::ProfileConfig(command) = cli.command else {
            panic!("expected profile-config command");
        };
        let ProfileConfigSubcommand::Schema(options) = command.command;
        assert_eq!(
            options.check,
            Some(PathBuf::from("schemas/profile-config.schema.json"))
        );
    }

    #[test]
    fn cli_build_info_commands_parse() {
        let manifest = Cli::try_parse_from(["noloong", "build-info", "manifest"]).unwrap();
        let CliCommand::BuildInfo(command) = manifest.command else {
            panic!("expected build-info command");
        };
        assert!(matches!(command.command, BuildInfoSubcommand::Manifest));

        let list = Cli::try_parse_from(["noloong", "build-info", "source", "list"]).unwrap();
        let CliCommand::BuildInfo(command) = list.command else {
            panic!("expected build-info command");
        };
        let BuildInfoSubcommand::Source(source) = command.command else {
            panic!("expected build-info source command");
        };
        assert!(matches!(source.command, BuildInfoSourceSubcommand::List));

        let cat =
            Cli::try_parse_from(["noloong", "build-info", "source", "cat", "Cargo.toml"]).unwrap();
        let CliCommand::BuildInfo(command) = cat.command else {
            panic!("expected build-info command");
        };
        let BuildInfoSubcommand::Source(source) = command.command else {
            panic!("expected build-info source command");
        };
        let BuildInfoSourceSubcommand::Cat(options) = source.command else {
            panic!("expected build-info source cat command");
        };
        assert_eq!(options.path, "Cargo.toml");

        let extract = Cli::try_parse_from([
            "noloong",
            "build-info",
            "source",
            "extract",
            "--output-dir",
            "out",
            "--force",
        ])
        .unwrap();
        let CliCommand::BuildInfo(command) = extract.command else {
            panic!("expected build-info command");
        };
        let BuildInfoSubcommand::Source(source) = command.command else {
            panic!("expected build-info source command");
        };
        let BuildInfoSourceSubcommand::Extract(options) = source.command else {
            panic!("expected build-info source extract command");
        };
        assert_eq!(options.output_dir, PathBuf::from("out"));
        assert!(options.force);

        let archive = Cli::try_parse_from([
            "noloong",
            "build-info",
            "source",
            "archive",
            "--output",
            "source.tar.zst",
        ])
        .unwrap();
        let CliCommand::BuildInfo(command) = archive.command else {
            panic!("expected build-info command");
        };
        let BuildInfoSubcommand::Source(source) = command.command else {
            panic!("expected build-info source command");
        };
        let BuildInfoSourceSubcommand::Archive(options) = source.command else {
            panic!("expected build-info source archive command");
        };
        assert_eq!(options.output, PathBuf::from("source.tar.zst"));
    }

    #[test]
    fn cli_profile_config_schema_rejects_output_and_check_together() {
        let error = Cli::try_parse_from([
            "noloong",
            "profile-config",
            "schema",
            "--output",
            "schemas/profile-config.schema.json",
            "--check",
            "schemas/profile-config.schema.json",
        ])
        .unwrap_err();

        assert!(error.to_string().contains("cannot be used with"));
    }

    #[test]
    fn profile_config_schema_check_accepts_matching_file() {
        let path = write_temp_file("profile-schema", "json", &profile_config_schema_json());

        run_profile_config_schema(ProfileConfigSchemaOptions {
            check: Some(path.clone()),
            ..Default::default()
        })
        .unwrap();
        remove_temp_file(path);
    }

    #[test]
    fn profile_config_schema_check_rejects_mismatch() {
        let path = write_temp_file("profile-schema-mismatch", "json", "{}\n");

        let error = run_profile_config_schema(ProfileConfigSchemaOptions {
            check: Some(path.clone()),
            ..Default::default()
        })
        .unwrap_err();
        remove_temp_file(path);

        assert!(error.to_string().contains("schema is out of date"));
    }

    #[test]
    fn telegram_text_input_detects_reply_to_bot() {
        let message = TelegramMessage {
            message_id: 2,
            message_thread_id: None,
            chat: TelegramChat {
                id: -100,
                kind: "supergroup".into(),
            },
            from: Some(TelegramUser {
                id: 7,
                username: Some("alice".into()),
            }),
            text: Some("continue".into()),
            caption: None,
            entities: Vec::new(),
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: Some(Box::new(TelegramMessage {
                message_id: 1,
                message_thread_id: None,
                chat: TelegramChat {
                    id: -100,
                    kind: "supergroup".into(),
                },
                from: Some(TelegramUser {
                    id: 1,
                    username: Some("Noloong_Bot".into()),
                }),
                text: Some("previous".into()),
                caption: None,
                entities: Vec::new(),
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            })),
        };

        let input =
            match TelegramInboundUpdate::from_message(message, Some("@noloong_bot")).unwrap() {
                TelegramInboundUpdate::Message(message) => message.into_text_input().unwrap(),
                TelegramInboundUpdate::Command(_) => panic!("expected text input"),
            };

        assert!(input.is_reply_to_bot);
    }

    #[test]
    fn telegram_config_uses_env_values() {
        let env = BTreeMap::from([
            ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
            ("TELEGRAM_BOT_TOKEN", "token"),
            ("TELEGRAM_BOT_USERNAME", "noloong_bot"),
            ("TELEGRAM_ALLOWED_USERS", "123456789"),
            ("TELEGRAM_LOCALE", "zh"),
        ]);

        let config = telegram_config_from_values(&TelegramBridgeOptions::default(), |name| {
            env.get(name).map(|value| value.to_string())
        })
        .unwrap();

        assert!(config.access.allows(1, Some(123456789)));
        assert_eq!(config.bot_username.as_deref(), Some("noloong_bot"));
        assert_eq!(config.locale, Locale::Zh);
    }

    #[test]
    fn weixin_config_uses_env_values() {
        let env = BTreeMap::from([
            ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
            ("WEIXIN_ACCOUNT_ID", "wx-bot"),
            ("WEIXIN_TOKEN", "token"),
            ("WEIXIN_ALLOWED_USERS", "user-1,user-2"),
            ("WEIXIN_LOCALE", "zh"),
        ]);

        let config = weixin_config_from_values(&WeixinBridgeOptions::default(), |name| {
            env.get(name).map(|value| value.to_string())
        })
        .unwrap();

        assert_eq!(config.account_id, "wx-bot");
        assert!(config.access.allows_dm("user-1"));
        assert!(!config.access.allows_dm("user-3"));
        assert_eq!(config.locale, Locale::Zh);
    }

    #[test]
    fn weixin_embedded_can_inherit_profile_locale() {
        let config =
            HostProfileConfig::load("examples/profile-configs/weixin-chatgpt-subscription.json")
                .unwrap();

        assert_eq!(profile_locale(&config, None), Some(Locale::Zh));
    }

    #[test]
    fn weixin_config_rejects_missing_allowlist() {
        let env = BTreeMap::from([
            ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
            ("WEIXIN_ACCOUNT_ID", "wx-bot"),
            ("WEIXIN_TOKEN", "token"),
        ]);

        let error = weixin_config_from_values(&WeixinBridgeOptions::default(), |name| {
            env.get(name).map(|value| value.to_string())
        })
        .unwrap_err();

        assert!(error.to_string().contains("allowlist"));
    }

    #[test]
    fn telegram_config_rejects_invalid_locale_env() {
        let env = BTreeMap::from([
            ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
            ("TELEGRAM_BOT_TOKEN", "token"),
            ("TELEGRAM_ALLOWED_USERS", "123456789"),
            ("TELEGRAM_LOCALE", "fr"),
        ]);

        let error = telegram_config_from_values(&TelegramBridgeOptions::default(), |name| {
            env.get(name).map(|value| value.to_string())
        })
        .unwrap_err();

        assert!(error.to_string().contains("invalid locale"));
    }

    #[test]
    fn telegram_config_ignores_empty_proxy_env() {
        let env = BTreeMap::from([
            ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
            ("TELEGRAM_BOT_TOKEN", "token"),
            ("TELEGRAM_ALLOWED_USERS", "123456789"),
            ("TELEGRAM_PROXY", ""),
        ]);

        let config = telegram_config_from_values(&TelegramBridgeOptions::default(), |name| {
            env.get(name).map(|value| value.to_string())
        })
        .unwrap();

        assert_eq!(config.network.proxy_url, None);
    }

    #[test]
    fn telegram_config_parses_manual_unsupported_media_fallback() {
        let env = BTreeMap::from([
            ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
            ("TELEGRAM_BOT_TOKEN", "token"),
            ("TELEGRAM_ALLOWED_USERS", "123456789"),
            ("TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_TO_FILE", "audio,video"),
        ]);

        let config = telegram_config_from_values(&TelegramBridgeOptions::default(), |name| {
            env.get(name).map(|value| value.to_string())
        })
        .unwrap();

        assert_eq!(
            config.file_policy.unsupported_media_fallback.audio,
            TelegramNativeMediaHandling::File
        );
        assert_eq!(
            config.file_policy.unsupported_media_fallback.voice,
            TelegramNativeMediaHandling::Native
        );
        assert_eq!(
            config.file_policy.unsupported_media_fallback.video,
            TelegramNativeMediaHandling::File
        );
    }

    #[test]
    fn telegram_embedded_mode_derives_media_fallback_from_profile_provider() {
        let config = serde_json::from_value::<HostProfileConfig>(json!({
            "defaultProfileId": "chatgpt",
            "profiles": [
                {
                    "profileId": "chatgpt",
                    "displayName": "ChatGPT",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini",
                        "allowFileDataUrlInput": true
                    }
                }
            ]
        }))
        .unwrap();
        let mut file_policy = TelegramFilePolicy::default();

        apply_profile_media_fallback_policy(&mut file_policy, &config, None);

        let fallback = file_policy.unsupported_media_fallback;
        assert_eq!(
            fallback.audio.decision_for_mime_type("application/pdf"),
            TelegramNativeMediaDecision::File
        );
        assert_eq!(
            fallback.audio.decision_for_mime_type("audio/ogg"),
            TelegramNativeMediaDecision::Unsupported
        );
        assert_eq!(
            fallback.video.decision_for_mime_type("video/mp4"),
            TelegramNativeMediaDecision::Unsupported
        );
    }

    #[test]
    fn telegram_chat_completions_fallback_keeps_supported_audio_native() {
        let config = serde_json::from_value::<HostProfileConfig>(json!({
            "profiles": [
                {
                    "profileId": "chat",
                    "displayName": "Chat",
                    "provider": {
                        "type": "chat_completions",
                        "model": "openrouter/free"
                    }
                }
            ]
        }))
        .unwrap();

        let policy = profile_media_fallback_policy(&config, None);

        assert_eq!(
            policy.audio.decision_for_mime_type("audio/mpeg"),
            TelegramNativeMediaDecision::Native
        );
        assert_eq!(
            policy.audio.decision_for_mime_type("audio/ogg"),
            TelegramNativeMediaDecision::Unsupported
        );
        assert_eq!(policy.video, TelegramNativeMediaHandling::Native);
    }

    #[tokio::test]
    async fn telegram_callback_resolves_approval_and_deletes_card() {
        let fixture = TelegramCallbackFixture::new().await;

        fixture.handle_callback("cb-1", 621).await.unwrap();
        fixture.handle_callback("cb-2", 621).await.unwrap();

        assert_eq!(fixture.interaction.methods(), vec!["approval/resolve"]);
        assert_eq!(
            fixture.api.answered_texts(),
            vec![Some("Recorded".into()), Some("Approval expired".into())]
        );
        assert_eq!(fixture.api.deleted_message_ids(), vec![10]);
        assert!(fixture.api.edited_reply_markup().is_empty());
    }

    #[tokio::test]
    async fn telegram_callback_rejects_unauthorized_without_consuming_approval() {
        let fixture = TelegramCallbackFixture::new().await;

        fixture.handle_callback("cb-1", 999).await.unwrap();
        fixture.handle_callback("cb-2", 621).await.unwrap();

        assert_eq!(fixture.interaction.methods(), vec!["approval/resolve"]);
        assert_eq!(
            fixture.api.answered_texts(),
            vec![Some("Not allowed".into()), Some("Recorded".into())]
        );
    }

    #[tokio::test]
    async fn telegram_approvals_command_lists_runtime_approvals() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture.interaction.set_approval_list(BTreeMap::from([(
            "approval-1".into(),
            approval_request("approval-1"),
        )]));

        fixture
            .handler
            .handle_command(approvals_command())
            .await
            .unwrap();

        assert_eq!(
            fixture.api.sent_texts().last().unwrap(),
            "Pending approvals: 1\n1\\. \\`host\\_exec\\` \\(approval\\-1\\)"
        );
        assert!(
            fixture
                .interaction
                .methods()
                .into_iter()
                .any(|method| method == "approval/list")
        );
    }

    #[tokio::test]
    async fn telegram_approve_command_resolves_only_pending_approval() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture.interaction.set_approval_list(BTreeMap::from([(
            "approval-1".into(),
            approval_request("approval-1"),
        )]));

        fixture
            .handler
            .handle_command(telegram_command(22, "approve"))
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        let (_, params) = calls
            .iter()
            .find(|(method, _)| method == "approval/resolve")
            .unwrap();
        assert_eq!(params["approvalId"], "approval-1");
        assert_eq!(params["decision"]["outcome"], "allow");
        assert_eq!(fixture.api.deleted_message_ids(), vec![10]);
        assert!(fixture.api.edited_texts().is_empty());
    }

    #[tokio::test]
    async fn telegram_deny_command_resolves_selected_pending_approval() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture.interaction.set_approval_list(BTreeMap::from([
            ("approval-1".into(), approval_request("approval-1")),
            ("approval-2".into(), approval_request("approval-2")),
        ]));

        fixture
            .handler
            .handle_command(telegram_command_with_args(23, "deny", "2"))
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        let (_, params) = calls
            .iter()
            .find(|(method, _)| method == "approval/resolve")
            .unwrap();
        assert_eq!(params["approvalId"], "approval-2");
        assert_eq!(params["decision"]["outcome"], "deny");
        assert_eq!(
            fixture.api.sent_texts().last().unwrap(),
            "Approval resolved: deny"
        );
    }

    #[tokio::test]
    async fn telegram_registers_command_menu_payload() {
        let api = Arc::new(FakeTelegramApi::default());

        register_telegram_commands(api.as_ref(), TelegramUiCatalog::new(Locale::Zh))
            .await
            .unwrap();

        let requests = api.command_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].language_code, None);
        assert!(requests[0].commands.iter().any(|command| {
            command.command == "approvals" && command.description == "列出待处理审批"
        }));
        assert_eq!(requests[0].commands.len(), 18);
    }

    #[tokio::test]
    async fn telegram_unknown_command_returns_help_without_prompt() {
        let fixture = TelegramCallbackFixture::new().await;

        fixture
            .handler
            .handle_command(unknown_command())
            .await
            .unwrap();

        assert!(fixture.interaction.methods().is_empty());
        let sent_texts = fixture.api.sent_texts();
        let text = sent_texts.last().unwrap();
        assert!(text.contains("Unknown command"));
        assert!(text.contains("/approvals"));
    }

    #[tokio::test]
    async fn telegram_known_future_command_returns_stub_without_prompt() {
        let fixture = TelegramCallbackFixture::new().await;

        fixture
            .handler
            .handle_command(telegram_command(5, "settings"))
            .await
            .unwrap();

        assert!(fixture.interaction.methods().is_empty());
        assert_eq!(
            fixture.api.sent_texts().last().unwrap(),
            "/settings is in the cockpit menu\\. Its control surface is not implemented yet\\."
        );
    }

    #[tokio::test]
    async fn telegram_profiles_command_lists_profiles_and_selects_default() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.interaction.set_profiles(vec![
            profile_descriptor("profile-1"),
            profile_descriptor("profile-2"),
        ]);

        fixture
            .handler
            .handle_command(telegram_command(6, "profiles"))
            .await
            .unwrap();
        let callback_data = fixture.api.last_sent_callback_data(1, 0);
        fixture
            .handle_callback_with_data("profile-cb", 621, &callback_data)
            .await
            .unwrap();
        fixture
            .handler
            .handle_command(telegram_command(7, "new"))
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        assert!(calls.iter().any(|(method, _)| method == "profile/list"));
        assert!(calls.iter().any(|(method, params)| {
            method == "session/create" && params["profileId"] == "profile-2"
        }));
        assert!(
            fixture
                .api
                .edited_texts()
                .iter()
                .any(|text| text.contains("Default profile selected"))
        );
    }

    #[tokio::test]
    async fn telegram_sessions_command_switches_and_confirms_delete() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.interaction.set_sessions(vec![
            telegram_session_descriptor("telegram:42"),
            telegram_session_descriptor_with_status(
                "telegram:42:session:9",
                InteractionSessionStatus::Running,
            ),
        ]);

        fixture
            .handler
            .handle_command(telegram_command(8, "sessions"))
            .await
            .unwrap();
        let switch_data = fixture.api.last_sent_callback_data(1, 0);
        let delete_data = fixture.api.last_sent_callback_data(1, 1);
        fixture
            .handle_callback_with_data("switch-cb", 621, &switch_data)
            .await
            .unwrap();

        fixture
            .handler
            .handle_command(telegram_command(9, "sessions"))
            .await
            .unwrap();
        fixture
            .handle_callback_with_data("delete-cb", 621, &delete_data)
            .await
            .unwrap();
        let confirm_data = fixture.api.edited_callback_data(0, 0);
        fixture
            .handle_callback_with_data("confirm-cb", 621, &confirm_data)
            .await
            .unwrap();
        fixture
            .handle_callback_with_data("confirm-cb-repeat", 621, &confirm_data)
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        assert!(calls.iter().any(|(method, params)| {
            method == "session/list"
                && params["metadataEquals"]["channel"] == "telegram"
                && params["metadataEquals"]["chatId"] == 42
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "session/get" && params["sessionId"] == "telegram:42:session:9"
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "session/delete"
                && params["sessionId"] == "telegram:42:session:9"
                && params["forceAbort"] == true
        }));
        assert_eq!(
            calls
                .iter()
                .filter(|(method, _)| method == "session/delete")
                .count(),
            1
        );
        assert!(
            fixture
                .api
                .answered_texts()
                .iter()
                .any(|text| text.as_deref() == Some("Action expired"))
        );
    }

    #[tokio::test]
    async fn telegram_status_command_reads_active_session() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;

        fixture
            .handler
            .handle_command(telegram_command(10, "status"))
            .await
            .unwrap();

        assert!(
            fixture
                .interaction
                .calls()
                .iter()
                .any(|(method, _)| method == "session/get")
        );
        assert!(
            fixture
                .api
                .sent_texts()
                .last()
                .unwrap()
                .contains("Active session")
        );
    }

    #[tokio::test]
    async fn telegram_continue_command_calls_agent_continue() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;

        fixture
            .handler
            .handle_command(telegram_command(11, "continue"))
            .await
            .unwrap();

        assert!(fixture.interaction.calls().iter().any(|(method, params)| {
            method == "agent/continue" && params["sessionId"] == "telegram:42"
        }));
        assert_eq!(
            fixture.api.sent_texts().last().unwrap(),
            "Run continued\nSession: telegram:42"
        );
    }

    #[tokio::test]
    async fn telegram_abort_command_confirms_running_session() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture
            .interaction
            .set_session_status("telegram:42", InteractionSessionStatus::Running);

        fixture
            .handler
            .handle_command(telegram_command(12, "abort"))
            .await
            .unwrap();
        let callback_data = fixture.api.last_sent_callback_data(0, 0);
        assert!(
            !fixture
                .interaction
                .methods()
                .iter()
                .any(|method| method == "agent/abort")
        );

        fixture
            .handle_callback_with_data("abort-cb", 621, &callback_data)
            .await
            .unwrap();

        assert!(fixture.interaction.calls().iter().any(|(method, params)| {
            method == "agent/abort" && params["sessionId"] == "telegram:42"
        }));
        assert_eq!(
            fixture.api.edited_texts().last().unwrap(),
            "Run aborted\nSession: telegram:42"
        );
    }

    #[tokio::test]
    async fn telegram_queue_command_lists_and_controls_queues() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture.interaction.set_queue(
            "telegram:42",
            TelegramQueueKind::Steering,
            vec![queued_user_message("queued-steer", "queued steering")],
        );

        fixture
            .handler
            .handle_command(telegram_command(13, "queue"))
            .await
            .unwrap();
        let clear_data = fixture.api.last_sent_callback_data(0, 0);
        let set_mode_data = fixture.api.last_sent_callback_data(1, 0);

        assert_eq!(
            fixture.api.sent_texts().last().unwrap(),
            "Queues: 1\nSteering: 1\n  1\\. user input: queued steering\nFollow\\-up: 0\n  empty"
        );

        fixture
            .handle_callback_with_data("queue-clear-cb", 621, &clear_data)
            .await
            .unwrap();
        fixture
            .handle_callback_with_data("queue-mode-cb", 621, &set_mode_data)
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        assert!(
            calls.iter().any(|(method, params)| {
                method == "queue/list" && params["queue"] == "steering"
            })
        );
        assert!(
            calls.iter().any(|(method, params)| {
                method == "queue/clear" && params["queue"] == "steering"
            })
        );
        assert!(calls.iter().any(|(method, params)| {
            method == "queue/set_mode" && params["queue"] == "steering" && params["mode"] == "all"
        }));
    }

    #[tokio::test]
    async fn telegram_queue_command_with_args_adds_follow_up() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;

        fixture
            .handler
            .handle_command(telegram_command_with_args(14, "queue", "use this next"))
            .await
            .unwrap();

        assert!(fixture.interaction.calls().iter().any(|(method, params)| {
            method == "agent/follow_up"
                && params["sessionId"] == "telegram:42"
                && params["message"]["content"][0]["text"] == "use this next"
        }));
        assert_eq!(
            fixture.api.sent_texts().last().unwrap(),
            "Follow\\-up queued\nSession: telegram:42"
        );
    }

    #[tokio::test]
    async fn telegram_processes_command_lists_and_opens_job() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture
            .interaction
            .set_processes(vec![job_snapshot("job-1", JobStatus::Running)]);
        fixture
            .interaction
            .set_process_output(process_output("job-1", "hello"));

        fixture
            .handler
            .handle_command(telegram_command(15, "processes"))
            .await
            .unwrap();
        let open_data = fixture.api.last_sent_callback_data(0, 0);
        fixture
            .handle_callback_with_data("process-open-cb", 621, &open_data)
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        assert!(calls.iter().any(|(method, params)| {
            method == "process/list" && params["sessionId"] == "telegram:42"
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "process/read"
                && params["sessionId"] == "telegram:42"
                && params["jobId"] == "job-1"
                && params["maxBytes"] == process_output_read_max_bytes()
        }));
    }

    #[tokio::test]
    async fn telegram_process_command_confirms_write_and_terminate() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture
            .interaction
            .set_processes(vec![job_snapshot("job-1", JobStatus::Running)]);
        fixture
            .interaction
            .set_process_output(process_output("job-1", "hello"));

        fixture
            .handler
            .handle_command(telegram_command_with_args(
                16,
                "process",
                "job-1 write input",
            ))
            .await
            .unwrap();
        let write_data = fixture.api.last_sent_callback_data(0, 0);
        fixture
            .handle_callback_with_data("process-write-cb", 621, &write_data)
            .await
            .unwrap();

        fixture
            .handler
            .handle_command(telegram_command_with_args(17, "process", "job-1"))
            .await
            .unwrap();
        let terminate_data = fixture.api.last_sent_callback_data(1, 0);
        fixture
            .handle_callback_with_data("process-terminate-request-cb", 621, &terminate_data)
            .await
            .unwrap();
        let confirm_data = fixture.api.edited_callback_data(0, 0);
        fixture
            .handle_callback_with_data("process-terminate-cb", 621, &confirm_data)
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        assert!(calls.iter().any(|(method, params)| {
            method == "process/write"
                && params["sessionId"] == "telegram:42"
                && params["jobId"] == "job-1"
                && params["text"] == "input"
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "process/terminate"
                && params["sessionId"] == "telegram:42"
                && params["jobId"] == "job-1"
        }));
    }

    #[tokio::test]
    async fn telegram_process_command_sends_long_output_as_document() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture.interaction.set_process_output(process_output(
            "job-long",
            &"x".repeat(PROCESS_OUTPUT_INLINE_CHAR_LIMIT + 1),
        ));

        fixture
            .handler
            .handle_command(telegram_command_with_args(18, "process", "job-long"))
            .await
            .unwrap();

        assert_eq!(fixture.api.document_requests().len(), 1);
        assert!(fixture.interaction.calls().iter().any(|(method, params)| {
            method == "process/read"
                && params["sessionId"] == "telegram:42"
                && params["jobId"] == "job-long"
        }));
    }

    #[tokio::test]
    async fn telegram_manifest_command_approves_and_applies_proposal() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;
        fixture
            .interaction
            .set_manifest_proposals(vec![manifest_proposal("manifest-proposal-1")]);

        fixture
            .handler
            .handle_command(telegram_command(19, "manifest"))
            .await
            .unwrap();
        let approve_data = fixture.api.last_sent_callback_data(0, 0);
        fixture
            .handle_callback_with_data("manifest-approve-cb", 621, &approve_data)
            .await
            .unwrap();
        let apply_data = fixture.api.edited_callback_data(0, 0);
        fixture
            .handle_callback_with_data("manifest-apply-request-cb", 621, &apply_data)
            .await
            .unwrap();
        let confirm_data = fixture.api.edited_callback_data(0, 0);
        fixture
            .handle_callback_with_data("manifest-apply-cb", 621, &confirm_data)
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        assert!(calls.iter().any(|(method, params)| {
            method == "manifest/get" && params["sessionId"] == "telegram:42"
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "manifest/system_prompt/get" && params["sessionId"] == "telegram:42"
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "manifest/proposals/list" && params["sessionId"] == "telegram:42"
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "manifest/proposals/approve"
                && params["sessionId"] == "telegram:42"
                && params["proposalId"] == "manifest-proposal-1"
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "manifest/apply_approved" && params["sessionId"] == "telegram:42"
        }));
    }

    #[tokio::test]
    async fn telegram_subagent_command_spawns_child_and_prompts() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture.establish_session().await;

        fixture
            .handler
            .handle_command(telegram_command_with_args(
                20,
                "subagent",
                "researcher inspect storage",
            ))
            .await
            .unwrap();
        fixture
            .handler
            .handle_command(telegram_command(21, "sessions"))
            .await
            .unwrap();

        let calls = fixture.interaction.calls();
        assert!(calls.iter().any(|(method, params)| {
            method == "subagent/spawn"
                && params["parentSessionId"] == "telegram:42"
                && params["role"] == "researcher"
                && params["metadata"]["channel"] == "telegram"
        }));
        assert!(calls.iter().any(|(method, params)| {
            method == "agent/prompt"
                && params["sessionId"] == "session-subagent-1"
                && params["input"]["message"]["content"][0]["text"] == "inspect storage"
        }));
        assert!(
            fixture
                .api
                .sent_texts()
                .iter()
                .any(|text| text.contains("session\\-subagent\\-1"))
        );
    }

    #[tokio::test]
    async fn telegram_submission_setup_failure_is_reported_without_polling_failure() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture
            .interaction
            .fail_method("session/create", "session store rejected create");

        fixture
            .handler
            .handle_text_input_message(telegram_text_input("hello"))
            .await
            .unwrap();

        assert!(fixture.api.sent_texts().iter().any(|text| {
            text.contains("Message could not be submitted to the agent")
                && text.contains("session store rejected create")
        }));
    }

    #[tokio::test]
    async fn telegram_prompt_jsonrpc_failure_is_left_to_display_delivery() {
        let fixture = TelegramCallbackFixture::new().await;
        fixture
            .interaction
            .fail_method("agent/prompt", "provider rejected media");

        fixture
            .handler
            .handle_text_input_message(telegram_text_input("hello"))
            .await
            .unwrap();

        assert!(
            !fixture
                .api
                .sent_texts()
                .iter()
                .any(|text| text.contains("Message could not be submitted to the agent"))
        );
    }

    struct TelegramCallbackFixture {
        handler: BridgeUpdateHandler,
        api: Arc<FakeTelegramApi>,
        interaction: Arc<FakeInteraction>,
        callback_data: String,
    }

    impl TelegramCallbackFixture {
        async fn new() -> Self {
            let api = Arc::new(FakeTelegramApi::default());
            let interaction = Arc::new(FakeInteraction::default());
            let bridge = Arc::new(
                noloong_agent_telegram::bridge::TelegramBridge::new(
                    telegram_test_config(),
                    interaction.clone(),
                )
                .unwrap(),
            );
            let delivery = TelegramDelivery::new(api.clone(), 3900);
            let display_states = Arc::new(Mutex::new(BTreeMap::new()));
            let key = noloong_agent_telegram::session::TelegramSessionKey::new(42, None);
            let state = Arc::new(Mutex::new(TelegramDisplayState::default()));
            display_states.lock().await.insert(key, state.clone());
            {
                let mut state = state.lock().await;
                deliver_display_event(
                    &mut state,
                    &delivery,
                    TelegramMessageTarget::chat(42),
                    approval_notification(),
                    true,
                    std::time::Duration::ZERO,
                    TelegramUiCatalog::new(Locale::En),
                )
                .await
                .unwrap();
            }
            let callback_data = api.sent_callback_data(0, 0);
            let handler = BridgeUpdateHandler {
                bridge,
                api: api.clone(),
                delivery,
                media_resolver: TelegramAttachmentResolver::new(
                    api.clone(),
                    TelegramFilePolicy::default(),
                ),
                display_states,
                session_actions: Arc::new(Mutex::new(TelegramSessionActionStore::default())),
                catalog: TelegramUiCatalog::new(Locale::En),
                bot_username: None,
            };

            Self {
                handler,
                api,
                interaction,
                callback_data,
            }
        }

        async fn handle_callback(
            &self,
            id: &str,
            user_id: u64,
        ) -> Result<(), noloong_agent_telegram::polling::TelegramPollingError> {
            self.handle_callback_with_data(id, user_id, &self.callback_data)
                .await
        }

        async fn handle_callback_with_data(
            &self,
            id: &str,
            user_id: u64,
            data: &str,
        ) -> Result<(), noloong_agent_telegram::polling::TelegramPollingError> {
            self.handler
                .handle_callback(callback_query(id, user_id, data))
                .await
        }

        async fn establish_session(&self) {
            self.handler
                .bridge
                .handle_text_message(telegram_text_input("hello"), None)
                .await
                .unwrap();
        }
    }

    fn telegram_test_config() -> noloong_agent_telegram::config::TelegramBridgeConfig {
        telegram_config_from_values(
            &TelegramBridgeOptions {
                interaction_url: Some("ws://127.0.0.1:8787/jsonrpc/ws".into()),
                bot_token: Some("token".into()),
                allowed_users: Some("621".into()),
                profile_id: Some("profile-1".into()),
                ..Default::default()
            },
            |_| None,
        )
        .unwrap()
    }

    fn approval_notification() -> noloong_agent_telegram::bridge::InteractionDisplayNotification {
        noloong_agent_telegram::bridge::InteractionDisplayNotification {
            session_id: "session-1".into(),
            subscription_id: "subscription-1".into(),
            event: noloong_agent::interaction::DisplayEvent::ApprovalRequested {
                approval: approval_request("approval-1"),
            },
        }
    }

    fn approval_request(approval_id: &str) -> ToolApprovalRequest {
        ToolApprovalRequest {
            approval_id: approval_id.into(),
            tool_call: ToolCall {
                id: "tool-1".into(),
                name: "host_exec".into(),
                arguments: json!({"cmd": "ls"}),
            },
            permissions: Vec::new(),
            hook_id: None,
            request: ToolApprovalRequestSpec {
                prompt: Some("Run command?".into()),
                reason: None,
                expires_at_ms: None,
                metadata: Value::Object(Default::default()),
            },
        }
    }

    fn approvals_command() -> TelegramCommand {
        telegram_command(3, "approvals")
    }

    fn unknown_command() -> TelegramCommand {
        telegram_command(4, "unknown")
    }

    fn telegram_command(message_id: i64, name: &str) -> TelegramCommand {
        TelegramCommand {
            context: telegram_inbound_context(message_id),
            name: name.into(),
            bot_username: None,
            args: String::new(),
            raw_text: format!("/{name}"),
        }
    }

    fn telegram_command_with_args(message_id: i64, name: &str, args: &str) -> TelegramCommand {
        TelegramCommand {
            context: telegram_inbound_context(message_id),
            name: name.into(),
            bot_username: None,
            args: args.into(),
            raw_text: format!("/{name} {args}"),
        }
    }

    fn queued_user_message(id: &str, text: &str) -> TelegramQueuedMessage {
        TelegramQueuedMessage {
            message: AgentMessage::user(id, text),
            intent: TelegramQueuedMessageIntent::UserInput,
        }
    }

    fn job_snapshot(job_id: &str, status: JobStatus) -> JobSnapshot {
        JobSnapshot {
            job_id: job_id.into(),
            command: "echo hello".into(),
            shell: "sh".into(),
            cwd: PathBuf::from("/tmp"),
            status,
            started_at_ms: 1,
            ended_at_ms: None,
            next_cursor: 1,
            dropped_before_seq: 0,
        }
    }

    fn process_output(job_id: &str, text: &str) -> ProcessOutput {
        ProcessOutput {
            job_id: job_id.into(),
            chunks: vec![OutputChunk {
                seq: 1,
                stream: ProcessOutputStream::Stdout,
                text: text.into(),
                byte_len: text.len(),
            }],
            next_cursor: 2,
            dropped_before_seq: 0,
            truncated: false,
            status: JobStatus::Running,
        }
    }

    fn manifest_proposal(proposal_id: &str) -> ManifestPatchProposal {
        ManifestPatchProposal {
            proposal_id: proposal_id.into(),
            patch: ManifestPatch::UpsertSystemPromptAddition {
                addition: SystemPromptAddition::new("telegram.test", "Test addition."),
            },
            summary: "upsert system prompt addition telegram.test".into(),
        }
    }

    fn telegram_text_input(text: &str) -> TelegramTextInput {
        TelegramTextInput {
            chat_id: 42,
            thread_id: None,
            chat_kind: TelegramChatKind::Private,
            user_id: Some(621),
            message_id: 2,
            text: text.into(),
            is_reply_to_bot: false,
            reply_to: None,
        }
    }

    fn telegram_inbound_context(message_id: i64) -> TelegramInboundContext {
        TelegramInboundContext {
            chat_id: 42,
            thread_id: None,
            chat_kind: TelegramChatKind::Private,
            user_id: Some(621),
            message_id,
            is_reply_to_bot: false,
            reply_to: None,
        }
    }

    fn callback_query(id: &str, user_id: u64, data: &str) -> TelegramCallbackQuery {
        TelegramCallbackQuery {
            id: id.into(),
            from: TelegramUser {
                id: user_id,
                username: Some("alice".into()),
            },
            message: Some(TelegramMessage {
                message_id: 10,
                message_thread_id: None,
                chat: TelegramChat {
                    id: 42,
                    kind: "private".into(),
                },
                from: None,
                text: None,
                caption: None,
                entities: Vec::new(),
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            }),
            data: Some(data.into()),
        }
    }

    #[derive(Default)]
    struct FakeInteraction {
        calls: StdMutex<Vec<(String, Value)>>,
        approval_list: StdMutex<BTreeMap<String, ToolApprovalRequest>>,
        profiles: StdMutex<Vec<InteractionProfileDescriptor>>,
        sessions: StdMutex<Vec<InteractionSessionDescriptor>>,
        queues: StdMutex<BTreeMap<(String, TelegramQueueKind), Vec<TelegramQueuedMessage>>>,
        queue_modes: StdMutex<BTreeMap<(String, TelegramQueueKind), QueueMode>>,
        processes: StdMutex<Vec<JobSnapshot>>,
        process_outputs: StdMutex<BTreeMap<String, ProcessOutput>>,
        manifest_proposals: StdMutex<Vec<ManifestPatchProposal>>,
        approved_manifest_proposals: StdMutex<Vec<ManifestPatchProposal>>,
        failures: StdMutex<BTreeMap<String, String>>,
    }

    impl FakeInteraction {
        fn methods(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|(method, _)| method.clone())
                .collect()
        }

        fn calls(&self) -> Vec<(String, Value)> {
            self.calls.lock().unwrap().clone()
        }

        fn set_approval_list(&self, approvals: BTreeMap<String, ToolApprovalRequest>) {
            *self.approval_list.lock().unwrap() = approvals;
        }

        fn set_profiles(&self, profiles: Vec<InteractionProfileDescriptor>) {
            *self.profiles.lock().unwrap() = profiles;
        }

        fn set_sessions(&self, sessions: Vec<InteractionSessionDescriptor>) {
            *self.sessions.lock().unwrap() = sessions;
        }

        fn set_session_status(&self, session_id: &str, status: InteractionSessionStatus) {
            let mut sessions = self.sessions.lock().unwrap();
            let Some(session) = sessions
                .iter_mut()
                .find(|session| session.session_id == session_id)
            else {
                return;
            };
            session.status = status;
        }

        fn set_queue(
            &self,
            session_id: &str,
            queue: TelegramQueueKind,
            messages: Vec<TelegramQueuedMessage>,
        ) {
            self.queues
                .lock()
                .unwrap()
                .insert((session_id.into(), queue), messages);
        }

        fn set_processes(&self, processes: Vec<JobSnapshot>) {
            *self.processes.lock().unwrap() = processes;
        }

        fn set_process_output(&self, output: ProcessOutput) {
            self.process_outputs
                .lock()
                .unwrap()
                .insert(output.job_id.clone(), output);
        }

        fn set_manifest_proposals(&self, proposals: Vec<ManifestPatchProposal>) {
            *self.manifest_proposals.lock().unwrap() = proposals;
        }

        fn fail_method(&self, method: &str, message: &str) {
            self.failures
                .lock()
                .unwrap()
                .insert(method.into(), message.into());
        }
    }

    impl TelegramInteractionClient for FakeInteraction {
        fn request_value<'a>(
            &'a self,
            method: &'a str,
            params: Value,
        ) -> TelegramInteractionFuture<'a, Value> {
            Box::pin(async move {
                self.calls
                    .lock()
                    .unwrap()
                    .push((method.into(), params.clone()));
                if let Some(message) = self.failures.lock().unwrap().get(method).cloned() {
                    return Err(TelegramBridgeError::Interaction(
                        InteractionClientError::JsonRpc {
                            code: -32603,
                            message,
                            data: None,
                        },
                    ));
                }
                match method {
                    "approval/list" => {
                        let approvals = self.approval_list.lock().unwrap().clone();
                        Ok(serde_json::to_value(approvals).unwrap())
                    }
                    "profile/list" => {
                        let profiles = self.profiles.lock().unwrap().clone();
                        let profiles = if profiles.is_empty() {
                            vec![profile_descriptor("profile-1")]
                        } else {
                            profiles
                        };
                        Ok(serde_json::to_value(profiles).unwrap())
                    }
                    "session/create" => {
                        let session_id = params["sessionId"]
                            .as_str()
                            .unwrap_or("session-1")
                            .to_owned();
                        let profile_id = params["profileId"]
                            .as_str()
                            .unwrap_or("profile-1")
                            .to_owned();
                        let descriptor = session_descriptor_with(
                            &session_id,
                            &profile_id,
                            InteractionSessionStatus::Idle,
                            params["metadata"].as_object().cloned().unwrap_or_default(),
                        );
                        self.sessions.lock().unwrap().push(descriptor.clone());
                        Ok(serde_json::to_value(descriptor).unwrap())
                    }
                    "session/list" => {
                        let sessions = self.sessions.lock().unwrap().clone();
                        Ok(serde_json::to_value(sessions).unwrap())
                    }
                    "session/get" => {
                        let session_id = params["sessionId"].as_str().unwrap_or("session-1");
                        let descriptor = self
                            .sessions
                            .lock()
                            .unwrap()
                            .iter()
                            .find(|session| session.session_id == session_id)
                            .cloned()
                            .unwrap_or_else(|| {
                                session_descriptor_with(
                                    session_id,
                                    "profile-1",
                                    InteractionSessionStatus::Idle,
                                    Default::default(),
                                )
                            });
                        Ok(serde_json::to_value(descriptor).unwrap())
                    }
                    "session/delete" => {
                        let session_id = params["sessionId"].as_str().unwrap_or("session-1");
                        let deleted = {
                            let mut sessions = self.sessions.lock().unwrap();
                            let index = sessions
                                .iter()
                                .position(|session| session.session_id == session_id);
                            index.map(|index| sessions.remove(index))
                        };
                        Ok(serde_json::to_value(deleted).unwrap())
                    }
                    "agent/continue" => {
                        let request = parse_fake_request::<FakeSessionRequest>(params);
                        Ok(serde_json::to_value(self.session_by_id(&request.session_id)).unwrap())
                    }
                    "agent/abort" => {
                        let request = parse_fake_request::<FakeSessionRequest>(params);
                        let mut descriptor = self.session_by_id(&request.session_id);
                        descriptor.status = InteractionSessionStatus::Aborted;
                        Ok(serde_json::to_value(descriptor).unwrap())
                    }
                    "agent/follow_up" => {
                        let request = parse_fake_request::<FakeFollowUpRequest>(params);
                        self.queues
                            .lock()
                            .unwrap()
                            .entry((request.session_id.clone(), TelegramQueueKind::FollowUp))
                            .or_default()
                            .push(TelegramQueuedMessage {
                                message: request.message,
                                intent: TelegramQueuedMessageIntent::UserInput,
                            });
                        Ok(serde_json::to_value(self.session_by_id(&request.session_id)).unwrap())
                    }
                    "agent/prompt" => {
                        let request = parse_fake_request::<FakePromptRequest>(params);
                        let mut descriptor = self.session_by_id(&request.session_id);
                        let _input = request.input;
                        descriptor.status = InteractionSessionStatus::Running;
                        self.upsert_session(descriptor.clone());
                        Ok(serde_json::to_value(descriptor).unwrap())
                    }
                    "queue/list" => {
                        let request = parse_fake_request::<FakeQueueRequest>(params);
                        let messages = self.queue_messages(&request.session_id, request.queue);
                        Ok(serde_json::to_value(messages).unwrap())
                    }
                    "queue/clear" => {
                        let request = parse_fake_request::<FakeQueueRequest>(params);
                        self.queues
                            .lock()
                            .unwrap()
                            .insert((request.session_id, request.queue), Vec::new());
                        Ok(serde_json::to_value(Vec::<TelegramQueuedMessage>::new()).unwrap())
                    }
                    "queue/set_mode" => {
                        let request = parse_fake_request::<FakeQueueSetModeRequest>(params);
                        let FakeQueueSetModeRequest {
                            session_id,
                            queue,
                            mode,
                        } = request;
                        self.queue_modes
                            .lock()
                            .unwrap()
                            .insert((session_id.clone(), queue), mode);
                        let messages = self.queue_messages(&session_id, queue);
                        Ok(serde_json::to_value(messages).unwrap())
                    }
                    "process/list" => {
                        let _request = parse_fake_request::<FakeSessionRequest>(params);
                        Ok(serde_json::to_value(self.processes.lock().unwrap().clone()).unwrap())
                    }
                    "process/read" => {
                        let request = parse_fake_request::<FakeProcessReadRequest>(params);
                        let FakeProcessReadRequest {
                            session_id: _session_id,
                            job_id,
                            after_seq: _after_seq,
                            max_bytes: _max_bytes,
                            wait_ms: _wait_ms,
                        } = request;
                        Ok(serde_json::to_value(self.process_output(&job_id)).unwrap())
                    }
                    "process/wait" => {
                        let request = parse_fake_request::<FakeProcessWaitRequest>(params);
                        let FakeProcessWaitRequest {
                            session_id: _session_id,
                            job_id,
                            timeout_ms: _timeout_ms,
                        } = request;
                        let output = self.process_output(&job_id);
                        Ok(serde_json::to_value(WaitOutcome {
                            job_id,
                            status: output.status,
                            timed_out: false,
                        })
                        .unwrap())
                    }
                    "process/write" => {
                        let request = parse_fake_request::<FakeProcessWriteRequest>(params);
                        let FakeProcessWriteRequest {
                            session_id: _session_id,
                            job_id,
                            text: _text,
                        } = request;
                        Ok(
                            serde_json::to_value(
                                self.process_snapshot(&job_id, JobStatus::Running),
                            )
                            .unwrap(),
                        )
                    }
                    "process/terminate" => {
                        let request = parse_fake_request::<FakeProcessJobRequest>(params);
                        let FakeProcessJobRequest {
                            session_id: _session_id,
                            job_id,
                        } = request;
                        Ok(serde_json::to_value(
                            self.process_snapshot(&job_id, JobStatus::Terminated),
                        )
                        .unwrap())
                    }
                    "manifest/get" => {
                        let request = parse_fake_request::<FakeSessionRequest>(params);
                        Ok(
                            serde_json::to_value(self.session_by_id(&request.session_id).manifest)
                                .unwrap(),
                        )
                    }
                    "manifest/system_prompt/get" => {
                        let request = parse_fake_request::<FakeSessionRequest>(params);
                        let manifest = self.session_by_id(&request.session_id).manifest;
                        let prompt = noloong_agent::system_prompt::resolve_system_prompt(
                            manifest.locale,
                            &manifest.system_prompt,
                            None,
                        );
                        Ok(serde_json::to_value(prompt).unwrap())
                    }
                    "manifest/proposals/list" => Ok(serde_json::to_value(
                        self.manifest_proposals.lock().unwrap().clone(),
                    )
                    .unwrap()),
                    "manifest/proposals/approve" => {
                        let request = parse_fake_request::<FakeManifestProposalRequest>(params);
                        let FakeManifestProposalRequest {
                            session_id: _session_id,
                            proposal_id,
                        } = request;
                        let proposal = {
                            let mut proposals = self.manifest_proposals.lock().unwrap();
                            let index = proposals
                                .iter()
                                .position(|proposal| proposal.proposal_id == proposal_id)
                                .unwrap();
                            proposals.remove(index)
                        };
                        self.approved_manifest_proposals
                            .lock()
                            .unwrap()
                            .push(proposal.clone());
                        Ok(serde_json::to_value(proposal).unwrap())
                    }
                    "manifest/apply_approved" => {
                        let _request = parse_fake_request::<FakeSessionRequest>(params);
                        let applied_proposal_ids = self
                            .approved_manifest_proposals
                            .lock()
                            .unwrap()
                            .drain(..)
                            .map(|proposal| proposal.proposal_id)
                            .collect::<Vec<_>>();
                        Ok(serde_json::json!({
                            "appliedProposalIds": applied_proposal_ids
                        }))
                    }
                    "subagent/spawn" => {
                        let request = parse_fake_request::<FakeSubagentSpawnRequest>(params);
                        let descriptor = session_descriptor_with_parent(
                            "session-subagent-1",
                            "profile-1",
                            Some(request.parent_session_id),
                            request.role,
                            InteractionSessionStatus::Idle,
                            request.metadata,
                        );
                        self.upsert_session(descriptor.clone());
                        Ok(serde_json::to_value(descriptor).unwrap())
                    }
                    "display/subscribe" => Ok(json!({"subscriptionId": "subscription-1"})),
                    _ => Ok(serde_json::to_value(session_descriptor()).unwrap()),
                }
            })
        }

        fn subscribe(&self) -> broadcast::Receiver<InteractionWsNotification> {
            let (_sender, receiver) = broadcast::channel(1);
            receiver
        }
    }

    impl FakeInteraction {
        fn session_by_id(&self, session_id: &str) -> InteractionSessionDescriptor {
            self.sessions
                .lock()
                .unwrap()
                .iter()
                .find(|session| session.session_id == session_id)
                .cloned()
                .unwrap_or_else(|| {
                    session_descriptor_with(
                        session_id,
                        "profile-1",
                        InteractionSessionStatus::Idle,
                        Default::default(),
                    )
                })
        }

        fn queue_messages(
            &self,
            session_id: &str,
            queue: TelegramQueueKind,
        ) -> Vec<TelegramQueuedMessage> {
            self.queues
                .lock()
                .unwrap()
                .get(&(session_id.into(), queue))
                .cloned()
                .unwrap_or_default()
        }

        fn process_output(&self, job_id: &str) -> ProcessOutput {
            self.process_outputs
                .lock()
                .unwrap()
                .get(job_id)
                .cloned()
                .unwrap_or_else(|| process_output(job_id, ""))
        }

        fn process_snapshot(&self, job_id: &str, status: JobStatus) -> JobSnapshot {
            self.processes
                .lock()
                .unwrap()
                .iter()
                .find(|snapshot| snapshot.job_id == job_id)
                .cloned()
                .map(|mut snapshot| {
                    snapshot.status = status.clone();
                    snapshot
                })
                .unwrap_or_else(|| job_snapshot(job_id, status))
        }

        fn upsert_session(&self, descriptor: InteractionSessionDescriptor) {
            let mut sessions = self.sessions.lock().unwrap();
            match sessions
                .iter_mut()
                .find(|session| session.session_id == descriptor.session_id)
            {
                Some(session) => *session = descriptor,
                None => sessions.push(descriptor),
            }
        }
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeSessionRequest {
        session_id: String,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeFollowUpRequest {
        session_id: String,
        message: AgentMessage,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakePromptRequest {
        session_id: String,
        input: Value,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeQueueRequest {
        session_id: String,
        queue: TelegramQueueKind,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeQueueSetModeRequest {
        session_id: String,
        queue: TelegramQueueKind,
        mode: QueueMode,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeProcessJobRequest {
        session_id: String,
        job_id: String,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeProcessReadRequest {
        session_id: String,
        job_id: String,
        after_seq: Option<u64>,
        max_bytes: Option<usize>,
        wait_ms: Option<u64>,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeProcessWaitRequest {
        session_id: String,
        job_id: String,
        timeout_ms: Option<u64>,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeProcessWriteRequest {
        session_id: String,
        job_id: String,
        text: String,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeManifestProposalRequest {
        session_id: String,
        proposal_id: String,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FakeSubagentSpawnRequest {
        parent_session_id: String,
        role: Option<String>,
        metadata: serde_json::Map<String, Value>,
    }

    fn parse_fake_request<T>(params: Value) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        serde_json::from_value(params).unwrap()
    }

    fn profile_descriptor(profile_id: &str) -> InteractionProfileDescriptor {
        InteractionProfileDescriptor {
            profile_id: profile_id.into(),
            display_name: profile_id.into(),
            description: None,
            default_manifest_patches: Vec::new(),
            metadata: Default::default(),
        }
    }

    fn session_descriptor() -> InteractionSessionDescriptor {
        session_descriptor_with(
            "session-1",
            "profile-1",
            InteractionSessionStatus::Idle,
            Default::default(),
        )
    }

    fn telegram_session_descriptor(session_id: &str) -> InteractionSessionDescriptor {
        telegram_session_descriptor_with_status(session_id, InteractionSessionStatus::Idle)
    }

    fn telegram_session_descriptor_with_status(
        session_id: &str,
        status: InteractionSessionStatus,
    ) -> InteractionSessionDescriptor {
        session_descriptor_with(
            session_id,
            "profile-1",
            status,
            telegram_session_metadata(42, None, "private"),
        )
    }

    fn session_descriptor_with(
        session_id: &str,
        profile_id: &str,
        status: InteractionSessionStatus,
        metadata: serde_json::Map<String, Value>,
    ) -> InteractionSessionDescriptor {
        session_descriptor_with_parent(session_id, profile_id, None, None, status, metadata)
    }

    fn session_descriptor_with_parent(
        session_id: &str,
        profile_id: &str,
        parent_session_id: Option<String>,
        role: Option<String>,
        status: InteractionSessionStatus,
        metadata: serde_json::Map<String, Value>,
    ) -> InteractionSessionDescriptor {
        InteractionSessionDescriptor {
            session_id: session_id.into(),
            profile_id: profile_id.into(),
            parent_session_id,
            role,
            status,
            manifest: AgentManifest::default(),
            state: AgentState::default(),
            metadata,
        }
    }

    #[derive(Default)]
    struct FakeTelegramApi {
        sent: StdMutex<Vec<TelegramSendMessageRequest>>,
        edited: StdMutex<Vec<TelegramEditMessageTextRequest>>,
        deleted: StdMutex<Vec<TelegramDeleteMessageRequest>>,
        documents: StdMutex<Vec<TelegramSendDocumentRequest>>,
        answered: StdMutex<Vec<(String, Option<String>)>>,
        command_requests: StdMutex<Vec<TelegramSetMyCommandsRequest>>,
    }

    impl FakeTelegramApi {
        fn sent_callback_data(&self, row: usize, column: usize) -> String {
            self.sent.lock().unwrap()[0]
                .reply_markup
                .as_ref()
                .unwrap()
                .inline_keyboard[row][column]
                .callback_data
                .clone()
        }

        fn last_sent_callback_data(&self, row: usize, column: usize) -> String {
            self.sent
                .lock()
                .unwrap()
                .last()
                .and_then(|request| request.reply_markup.as_ref())
                .unwrap()
                .inline_keyboard[row][column]
                .callback_data
                .clone()
        }

        fn sent_texts(&self) -> Vec<String> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.text.clone())
                .collect()
        }

        fn answered_texts(&self) -> Vec<Option<String>> {
            self.answered
                .lock()
                .unwrap()
                .iter()
                .map(|(_, text)| text.clone())
                .collect()
        }

        fn edited_reply_markup(
            &self,
        ) -> Vec<Option<noloong_agent_telegram::telegram_api::TelegramInlineKeyboardMarkup>>
        {
            self.edited
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.reply_markup.clone())
                .collect()
        }

        fn edited_texts(&self) -> Vec<String> {
            self.edited
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.text.clone())
                .collect()
        }

        fn edited_callback_data(&self, row: usize, column: usize) -> String {
            self.edited
                .lock()
                .unwrap()
                .last()
                .and_then(|request| request.reply_markup.as_ref())
                .unwrap()
                .inline_keyboard[row][column]
                .callback_data
                .clone()
        }

        fn deleted_message_ids(&self) -> Vec<i64> {
            self.deleted
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.message_id)
                .collect()
        }

        fn command_requests(&self) -> Vec<TelegramSetMyCommandsRequest> {
            self.command_requests.lock().unwrap().clone()
        }

        fn document_requests(&self) -> Vec<TelegramSendDocumentRequest> {
            self.documents.lock().unwrap().clone()
        }
    }

    impl TelegramApi for FakeTelegramApi {
        fn get_updates<'a>(
            &'a self,
            _offset: Option<i64>,
            _timeout_seconds: u64,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<TelegramUpdate>, TelegramApiError>> + Send + 'a>>
        {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn send_message<'a>(
            &'a self,
            request: TelegramSendMessageRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.sent.lock().unwrap().push(request.clone());
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id: 10,
                })
            })
        }

        fn edit_message_text<'a>(
            &'a self,
            request: TelegramEditMessageTextRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.edited.lock().unwrap().push(request.clone());
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id: request.message_id,
                })
            })
        }

        fn delete_message<'a>(
            &'a self,
            request: TelegramDeleteMessageRequest,
        ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
            Box::pin(async move {
                self.deleted.lock().unwrap().push(request);
                Ok(())
            })
        }

        fn send_document<'a>(
            &'a self,
            request: TelegramSendDocumentRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.documents.lock().unwrap().push(request.clone());
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id: 11,
                })
            })
        }

        fn answer_callback_query<'a>(
            &'a self,
            callback_query_id: &'a str,
            text: Option<&'a str>,
        ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
            Box::pin(async move {
                self.answered
                    .lock()
                    .unwrap()
                    .push((callback_query_id.into(), text.map(str::to_owned)));
                Ok(())
            })
        }

        fn set_my_commands<'a>(
            &'a self,
            request: TelegramSetMyCommandsRequest,
        ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
            Box::pin(async move {
                self.command_requests.lock().unwrap().push(request);
                Ok(())
            })
        }
    }
}

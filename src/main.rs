mod build_info;
mod chatgpt;
mod config;
mod host;
mod schema;
#[cfg(test)]
mod test_support;

use crate::{
    config::{
        DEFAULT_INTERACTION_TOKEN_ENV, DEFAULT_INTERACTION_URL_ENV, DEFAULT_PROFILE_CONFIG_ENV,
        DEFAULT_TELEGRAM_ALLOWED_CHATS_ENV, DEFAULT_TELEGRAM_ALLOWED_USERS_ENV,
        DEFAULT_TELEGRAM_BOT_TOKEN_ENV, DEFAULT_TELEGRAM_BOT_USERNAME_ENV,
        DEFAULT_TELEGRAM_DISABLE_ENV_PROXY_ENV, DEFAULT_TELEGRAM_DISABLE_FALLBACK_IPS_ENV,
        DEFAULT_TELEGRAM_FALLBACK_IPS_ENV, DEFAULT_TELEGRAM_FILE_DOWNLOAD_DIR_ENV,
        DEFAULT_TELEGRAM_FILE_INLINE_MAX_BYTES_ENV, DEFAULT_TELEGRAM_FILE_MAX_DOWNLOAD_BYTES_ENV,
        DEFAULT_TELEGRAM_FILE_RETENTION_SECONDS_ENV, DEFAULT_TELEGRAM_LOCALE_ENV,
        DEFAULT_TELEGRAM_OFFSET_CHECKPOINT_ENV, DEFAULT_TELEGRAM_PROXY_ENV,
        DEFAULT_TELEGRAM_REQUIRE_MENTION_ENV, DEFAULT_TELEGRAM_STARTUP_UPDATE_POLICY_ENV,
        HostProfileConfig, env_or_value, parse_bool_env, parse_csv_i64, parse_csv_u64,
    },
    host::build_registry,
};
use clap::{Args, Parser, Subcommand};
use noloong_agent::{
    Locale,
    interaction::{
        InteractionCapabilityPolicy, InteractionControlHandler, InteractionHttpTransportConfig,
        InteractionTransportAuth, InteractionWsClient, InteractionWsClientConfig,
        serve_interaction_http,
    },
};
use noloong_agent_telegram::{
    access::{TelegramAccessPolicy, TelegramChatKind, TelegramTextInput},
    bridge::TelegramBridge,
    config::{TelegramFilePolicy, TelegramStartupUpdatePolicy},
    delivery::{TelegramDelivery, TelegramMessageTarget},
    display::{TelegramDisplayState, deliver_display_event},
    i18n::TelegramUiCatalog,
    network::{
        TelegramNetworkConfig, TelegramNetworkResolutionMode, build_telegram_http_client,
        discover_fallback_addrs, network_resolution_mode,
    },
    polling::{
        FileTelegramOffsetStore, TelegramCallbackQuery, TelegramMessage, TelegramPollOutcome,
        TelegramPoller, TelegramPollingError, TelegramUpdate, TelegramUpdateHandler,
        TelegramUpdateHandlerFuture,
    },
    session::TelegramSessionKey,
    telegram_api::{ReqwestTelegramApi, TelegramApi},
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
    tokio::select! {
        result = run_telegram_bridge(bridge_options) => result,
        result = server => {
            result.map_err(|error| CliError::Task(error.to_string()))?
                .map_err(CliError::Interaction)
        }
    }
}

async fn run_telegram_bridge_with_config(
    mut config: noloong_agent_telegram::config::TelegramBridgeConfig,
) -> Result<(), CliError> {
    let mut client_config = InteractionWsClientConfig::new(&config.interaction_ws_url)
        .request_timeout(Duration::from_secs(30));
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
    let catalog = TelegramUiCatalog::new(config.locale);
    let display_states = Arc::new(Mutex::new(
        BTreeMap::<TelegramSessionKey, SharedDisplayState>::new(),
    ));
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
        display_states,
        catalog,
        bot_username: config.bot_username.clone(),
    });
    let offset_checkpoint_path = config
        .offset_checkpoint_path
        .clone()
        .or_else(|| default_telegram_offset_checkpoint_path(&config.bot_token));
    let mut poller = TelegramPoller::new(Arc::clone(&handler.api), handler)
        .with_startup_update_policy(config.startup_update_policy);
    if let Some(path) = offset_checkpoint_path {
        poller = poller.with_offset_store(Arc::new(FileTelegramOffsetStore::new(path)));
    }
    poller.initialize().await.map_err(CliError::Polling)?;
    log::info!("telegram bridge initialized; polling started");

    tokio::select! {
        result = run_polling_loop(poller) => result.map_err(CliError::Polling),
        result = display_task => result.map_err(|error| CliError::Task(error.to_string()))?,
    }
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
        let notification = notifications
            .recv()
            .await
            .map_err(|error| CliError::Task(error.to_string()))?;
        let Some(display) = TelegramBridge::parse_display_notification(notification)? else {
            continue;
        };
        let Some(key) = TelegramSessionKey::from_session_id(&display.session_id) else {
            continue;
        };
        let state = display_state_for(&display_states, key).await;
        let mut state = state.lock().await;
        deliver_display_event(
            &mut state,
            &delivery,
            TelegramMessageTarget::new(key.chat_id, key.thread_id),
            display,
            show_tool_status,
            edit_throttle,
            catalog,
        )
        .await?;
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
    display_states: SharedDisplayStates,
    catalog: TelegramUiCatalog,
    bot_username: Option<String>,
}

impl TelegramUpdateHandler for BridgeUpdateHandler {
    fn handle_update<'a>(&'a self, update: TelegramUpdate) -> TelegramUpdateHandlerFuture<'a> {
        Box::pin(async move {
            if let Some(message) = update.message
                && let Some(input) = telegram_text_input(message, self.bot_username.as_deref())
            {
                self.bridge
                    .handle_text_message(input, self.bot_username.as_deref())
                    .await
                    .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
            }
            if let Some(callback) = update.callback_query {
                self.handle_callback(callback).await?;
            }
            Ok(())
        })
    }
}

impl BridgeUpdateHandler {
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
        let selection = {
            let state = self.display_states.lock().await.get(&key).cloned();
            match state {
                Some(state) => state.lock().await.resolve_approval_callback(&data),
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
        self.delivery
            .edit_text(
                TelegramMessageTarget::chat(target.message.chat_id),
                target.message.message_id,
                &self.catalog.approval_resolved(&outcome),
                None,
            )
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        self.api
            .answer_callback_query(&callback.id, Some(self.catalog.callback_recorded()))
            .await
            .map_err(|error| TelegramPollingError::Handler(error.to_string()))?;
        Ok(())
    }
}

fn telegram_text_input(
    message: TelegramMessage,
    bot_username: Option<&str>,
) -> Option<TelegramTextInput> {
    let text = message.text?;
    let is_reply_to_bot = message
        .reply_to_message
        .as_ref()
        .and_then(|reply| reply.from.as_ref())
        .and_then(|user| user.username.as_deref())
        .is_some_and(|username| same_telegram_username(username, bot_username));
    Some(TelegramTextInput {
        chat_id: message.chat.id,
        thread_id: message.message_thread_id,
        chat_kind: TelegramChatKind::parse(&message.chat.kind),
        user_id: message.from.map(|user| user.id),
        message_id: message.message_id,
        text,
        is_reply_to_bot,
    })
}

fn same_telegram_username(username: &str, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return false;
    };
    username
        .trim_start_matches('@')
        .eq_ignore_ascii_case(expected.trim_start_matches('@'))
}

fn load_profile_config(path: Option<String>) -> Result<HostProfileConfig, CliError> {
    let path = env_or_value(path, DEFAULT_PROFILE_CONFIG_ENV)
        .ok_or(config::CliConfigError::MissingProfileConfig)?;
    let config = HostProfileConfig::load(path)?;
    config.validate()?;
    Ok(config)
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
    };
    let startup_update_policy = telegram_startup_update_policy(
        options.startup_update_policy,
        env_source(DEFAULT_TELEGRAM_STARTUP_UPDATE_POLICY_ENV),
    )?;
    let offset_checkpoint_path = options.offset_checkpoint_path.clone().or_else(|| {
        non_empty_option(env_source(DEFAULT_TELEGRAM_OFFSET_CHECKPOINT_ENV)).map(PathBuf::from)
    });
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
        offset_checkpoint_path,
        show_tool_status: true,
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

fn default_telegram_offset_checkpoint_path(bot_token: &str) -> Option<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    Some(
        home.join(".agents")
            .join("noloong")
            .join("telegram")
            .join(format!("{}.offset.json", stable_fingerprint(bot_token))),
    )
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
    #[arg(long = "telegram-startup-update-policy", value_parser = parse_telegram_startup_update_policy_arg)]
    startup_update_policy: Option<TelegramStartupUpdatePolicy>,
    #[arg(long = "telegram-offset-checkpoint")]
    offset_checkpoint_path: Option<PathBuf>,
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
    #[error("Telegram bridge failed: {0}")]
    TelegramBridge(#[from] noloong_agent_telegram::bridge::TelegramBridgeError),
    #[error("Telegram config failed: {0}")]
    TelegramConfig(#[from] noloong_agent_telegram::config::TelegramConfigError),
    #[error("Telegram network failed: {0}")]
    TelegramNetwork(#[from] noloong_agent_telegram::network::TelegramNetworkError),
    #[error("Telegram delivery failed: {0}")]
    TelegramDelivery(#[from] noloong_agent_telegram::delivery::TelegramDeliveryError),
    #[error("Telegram polling failed: {0}")]
    Polling(TelegramPollingError),
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
        BuildInfoSourceSubcommand, BuildInfoSubcommand, Cli, CliCommand, CliError,
        ProfileConfigSchemaOptions, ProfileConfigSubcommand, TelegramBridgeOptions,
        run_profile_config_schema, telegram_config_from_values, telegram_text_input,
        validate_interaction_bind,
    };
    use crate::schema::profile_config_schema_json;
    use crate::test_support::{remove_temp_file, write_temp_file};
    use clap::Parser;
    use noloong_agent::Locale;
    use noloong_agent_telegram::polling::{TelegramChat, TelegramMessage, TelegramUser};
    use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

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
                reply_to_message: None,
            })),
        };

        let input = telegram_text_input(message, Some("@noloong_bot")).unwrap();

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
}

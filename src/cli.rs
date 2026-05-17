use crate::{
    build_info,
    build_info_cli::{BuildInfoCommand, run_build_info},
    chatgpt,
    config::{
        self, DEFAULT_INTERACTION_TOKEN_ENV, DEFAULT_PROFILE_CONFIG_ENV, HostProfileConfig,
        env_or_value,
    },
    host::{self, build_registry},
    profile_config_cli::{
        ProfileConfigCommand, ProfileConfigSubcommand, run_profile_config_schema,
    },
    telegram_cli::{TelegramBridgeOptions, TelegramOptions, run_telegram, run_telegram_bridge},
    weixin_cli::{WeixinCommand, run_weixin},
};
use clap::{Args, Parser, Subcommand};
use noloong_agent::{
    Locale, ManifestPatch,
    interaction::{
        InteractionCapabilityPolicy, InteractionControlHandler, InteractionHttpTransportConfig,
        InteractionTransportAuth, serve_interaction_http,
    },
};
use noloong_agent_telegram::{polling::TelegramPollingError, telegram_api::TelegramApiError};
use std::{env, net::SocketAddr};
use thiserror::Error;
use tokio::net::TcpListener;

pub(crate) async fn run_cli(args: Vec<String>) -> Result<(), CliError> {
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

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(crate) struct ServeInteractionOptions {
    #[arg(long = "profile-config")]
    profile_config: Option<String>,
    #[arg(long = "bind")]
    bind: Option<SocketAddr>,
    #[arg(long = "interaction-token-env")]
    interaction_token_env: Option<String>,
}

#[derive(Clone, Debug, Parser, PartialEq, Eq)]
#[command(name = "noloong", version, about = "Noloong agent runtime")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: CliCommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub(crate) enum CliCommand {
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
pub(crate) struct ServeCommand {
    #[command(subcommand)]
    pub(crate) command: ServeSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub(crate) enum ServeSubcommand {
    Interaction(ServeInteractionOptions),
}

#[derive(Debug, Error)]
pub(crate) enum CliError {
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

pub(crate) fn load_profile_config(path: Option<String>) -> Result<HostProfileConfig, CliError> {
    let path = env_or_value(path, DEFAULT_PROFILE_CONFIG_ENV)
        .ok_or(config::CliConfigError::MissingProfileConfig)?;
    let config = HostProfileConfig::load(path)?;
    config.validate()?;
    Ok(config)
}

pub(crate) fn profile_locale(
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

pub(crate) fn interaction_token(token_env: Option<&str>) -> Option<String> {
    token_env
        .and_then(|env_name| env_or_value(None, env_name))
        .or_else(|| env_or_value(None, DEFAULT_INTERACTION_TOKEN_ENV))
}

pub(crate) fn validate_interaction_bind(
    bind: SocketAddr,
    token: Option<&str>,
) -> Result<(), CliError> {
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

pub(crate) fn default_embedded_interaction_bind() -> SocketAddr {
    "127.0.0.1:0"
        .parse()
        .expect("default embedded interaction bind address is valid")
}

pub(crate) fn process_env(name: &str) -> Option<String> {
    env::var(name).ok()
}

pub(crate) fn non_empty_option(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

pub(crate) fn parse_csv_strings(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn parse_locale_option(value: Option<String>) -> Result<Option<Locale>, CliError> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    Locale::parse(&value).map(Some).ok_or_else(|| {
        config::CliConfigError::ParseConfig(format!("invalid locale: {value}")).into()
    })
}

pub(crate) fn resolve_locale(
    cli_locale: Option<Locale>,
    env_locale: Option<String>,
) -> Result<Locale, CliError> {
    if let Some(locale) = cli_locale {
        return Ok(locale);
    }
    parse_locale_option(env_locale)?.map_or_else(|| Ok(Locale::detect()), Ok)
}

pub(crate) fn parse_locale_arg(value: &str) -> Result<Locale, String> {
    Locale::parse(value).ok_or_else(|| format!("invalid locale: {value}"))
}

pub(crate) fn parse_config_usize(
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

pub(crate) fn parse_config_optional_u64(
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

pub(crate) fn stable_fingerprint(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn generate_token() -> Result<String, CliError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| CliError::Random(error.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;

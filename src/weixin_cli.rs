use crate::{
    cli::{
        CliError, interaction_token, non_empty_option, parse_config_usize, parse_csv_strings,
        parse_locale_arg, process_env, profile_locale, resolve_locale, start_embedded_interaction,
    },
    config,
    config::{
        DEFAULT_INTERACTION_TOKEN_ENV, DEFAULT_INTERACTION_URL_ENV, DEFAULT_WEIXIN_ACCOUNT_ID_ENV,
        DEFAULT_WEIXIN_ALLOW_ALL_ENV, DEFAULT_WEIXIN_ALLOWED_USERS_ENV,
        DEFAULT_WEIXIN_BASE_URL_ENV, DEFAULT_WEIXIN_CDN_BASE_URL_ENV,
        DEFAULT_WEIXIN_FILE_DOWNLOAD_DIR_ENV, DEFAULT_WEIXIN_FILE_INLINE_MAX_BYTES_ENV,
        DEFAULT_WEIXIN_FILE_MAX_DOWNLOAD_BYTES_ENV, DEFAULT_WEIXIN_FILE_MAX_UPLOAD_BYTES_ENV,
        DEFAULT_WEIXIN_LOCALE_ENV, DEFAULT_WEIXIN_TOKEN_ENV, ensure_sqlite_database_parent,
        parse_bool_env, resolve_state_database_url,
    },
};
use clap::{Args, Subcommand};
use noloong_agent::Locale;
use noloong_agent_weixin::{
    config::{
        ILINK_BASE_URL, WEIXIN_CDN_BASE_URL, WeixinAccessPolicy, WeixinBridgeConfig,
        WeixinFilePolicy,
    },
    login::{WeixinLoginOptions, run_qr_login},
    runtime::run_weixin_bridge_with_config,
    state::WeixinAccountStore,
};
use std::{io, path::PathBuf};

pub(crate) async fn run_weixin(command: WeixinCommand) -> Result<(), CliError> {
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
    let embedded = start_embedded_interaction(options.profile_config).await?;
    let mut bridge_options = options.bridge;
    if bridge_options.locale.is_none() && process_env(DEFAULT_WEIXIN_LOCALE_ENV).is_none() {
        bridge_options.locale = profile_locale(
            embedded.profile_config(),
            bridge_options.profile_id.as_deref(),
        );
    }
    bridge_options.interaction_url = Some(embedded.interaction_ws_url().to_owned());
    bridge_options.interaction_token = Some(embedded.interaction_token().to_owned());
    let bridge_config = weixin_config_from_values(&bridge_options, process_env)?;
    embedded.run(run_weixin_bridge_config(bridge_config)).await
}

async fn run_weixin_bridge_config(config: WeixinBridgeConfig) -> Result<(), CliError> {
    let state_database_url = resolve_state_database_url()?;
    ensure_sqlite_database_parent(&state_database_url)?;
    run_weixin_bridge_with_config(config, state_database_url).await?;
    Ok(())
}

pub(crate) fn weixin_config_from_values(
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
    let configured_token = options
        .token
        .clone()
        .or_else(|| env_source(DEFAULT_WEIXIN_TOKEN_ENV));
    let configured_base_url = options
        .base_url
        .clone()
        .or_else(|| env_source(DEFAULT_WEIXIN_BASE_URL_ENV));
    let stored_account = if configured_token.is_none() || configured_base_url.is_none() {
        store.load(&account_id)?
    } else {
        None
    };
    let token = configured_token
        .or_else(|| stored_account.as_ref().map(|account| account.token.clone()))
        .ok_or(config::CliConfigError::MissingEnv(
            DEFAULT_WEIXIN_TOKEN_ENV.into(),
        ))?;
    let base_url = configured_base_url
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
    let locale = resolve_locale(options.locale, env_source(DEFAULT_WEIXIN_LOCALE_ENV))?;
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

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub(crate) struct WeixinCommand {
    #[command(subcommand)]
    pub(crate) command: WeixinSubcommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub(crate) enum WeixinSubcommand {
    Login(WeixinLoginCliOptions),
    Bridge(WeixinBridgeOptions),
    Run(WeixinRunOptions),
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub(crate) struct WeixinLoginCliOptions {
    #[arg(long = "bot-type", default_value = "3")]
    pub(crate) bot_type: String,
    #[arg(long = "timeout-seconds", default_value_t = 480)]
    pub(crate) timeout_seconds: u64,
    #[arg(long = "qr-png")]
    pub(crate) qr_png_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(crate) struct WeixinBridgeOptions {
    #[arg(long = "interaction-url")]
    pub(crate) interaction_url: Option<String>,
    #[arg(long = "interaction-token")]
    pub(crate) interaction_token: Option<String>,
    #[arg(long = "interaction-token-env")]
    pub(crate) interaction_token_env: Option<String>,
    #[arg(long = "weixin-account-id")]
    pub(crate) account_id: Option<String>,
    #[arg(long = "weixin-token")]
    pub(crate) token: Option<String>,
    #[arg(long = "weixin-base-url")]
    pub(crate) base_url: Option<String>,
    #[arg(long = "weixin-cdn-base-url")]
    pub(crate) cdn_base_url: Option<String>,
    #[arg(long = "weixin-allowed-users")]
    pub(crate) allowed_users: Option<String>,
    #[arg(long = "weixin-allow-all")]
    pub(crate) allow_all: bool,
    #[arg(long = "weixin-locale", value_parser = parse_locale_arg)]
    pub(crate) locale: Option<Locale>,
    #[arg(long = "weixin-max-outbound-chars")]
    pub(crate) max_outbound_chars: Option<usize>,
    #[arg(long = "weixin-file-inline-max-bytes")]
    pub(crate) file_inline_max_bytes: Option<usize>,
    #[arg(long = "weixin-file-max-download-bytes")]
    pub(crate) file_max_download_bytes: Option<usize>,
    #[arg(long = "weixin-file-max-upload-bytes")]
    pub(crate) file_max_upload_bytes: Option<usize>,
    #[arg(long = "weixin-file-download-dir")]
    pub(crate) file_download_dir: Option<PathBuf>,
    #[arg(long = "profile-id")]
    pub(crate) profile_id: Option<String>,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(crate) struct WeixinRunOptions {
    #[arg(long = "profile-config")]
    pub(crate) profile_config: Option<String>,
    #[command(flatten)]
    pub(crate) bridge: WeixinBridgeOptions,
}

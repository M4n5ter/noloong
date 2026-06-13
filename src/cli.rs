use crate::runtime_control::{
    AppRuntimeControlServer, RuntimeControlInteractionManager, start_app_runtime_control_server,
};
use crate::{
    build_info,
    build_info_cli::{BuildInfoCommand, run_build_info},
    chatgpt,
    config::{
        self, DEFAULT_INTERACTION_TOKEN_ENV, HostProfileConfig, env_or_value,
        resolve_profile_config_path,
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
    Locale,
    interaction::{
        InteractionCapabilityPolicy, InteractionControlHandler, InteractionHttpTransportConfig,
        InteractionTransportAuth, serve_interaction_http,
    },
};
use noloong_agent_telegram::{polling::TelegramPollingError, telegram_api::TelegramApiError};
use noloong_app::{
    APP_LAUNCH_OPTIONS_ENV, AppInteractionEndpoint, AppInteractionHttpClient, AppLaunchOptions,
    initialize_interaction_status,
};
use std::{
    env, fs,
    future::Future,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::Arc,
};
use thiserror::Error;
use tokio::{
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    task::JoinHandle,
    time::{Duration, sleep},
};

const APP_BUNDLE_EXECUTABLE_ENV: &str = "NOLOONG_APP_BUNDLE_EXECUTABLE";
const MACOS_APP_BUNDLE_NAME: &str = "Noloong.app";
const MACOS_APP_EXECUTABLE_NAME: &str = "Noloong";
const DESKTOP_APP_PACKAGE_DIR: &str = "apps/desktop";
const DESKTOP_DEV_SERVER_ADDRESS: &str = "127.0.0.1:5173";

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
        CliCommand::App(options) => run_app_command(options).await,
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
    App(AppOptions),
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub(crate) struct AppOptions {
    #[arg(long = "profile-config")]
    profile_config: Option<String>,
    #[arg(long = "locale", value_parser = parse_locale_arg)]
    locale: Option<Locale>,
    #[arg(long = "interaction-ws-url")]
    interaction_ws_url: Option<String>,
    #[arg(long = "interaction-token")]
    interaction_token: Option<String>,
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
    #[error("App failed: {0}")]
    App(#[from] noloong_app::AppError),
    #[error(
        "app bundle executable is missing; run `bun run app:bundle` first, or use `bun run app:dev` for Tauri development"
    )]
    AppBundleMissing,
    #[error("app bundle exited unsuccessfully: {executable} ({status})")]
    AppBundleExited {
        executable: String,
        status: ExitStatus,
    },
    #[error("development app command failed: {command} ({status})")]
    DevelopmentAppCommandExited {
        command: String,
        status: ExitStatus,
    },
    #[error("failed to serialize app launch options: {0}")]
    AppLaunchOptions(String),
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
    let path = resolve_profile_config_path(path.as_deref())?;
    let config = HostProfileConfig::load(path)?;
    config.validate()?;
    Ok(config)
}

pub(crate) struct EmbeddedInteraction {
    profile_config: HostProfileConfig,
    interaction_ws_url: String,
    interaction_token: String,
    listener: TcpListener,
}

pub(crate) struct EmbeddedInteractionServer {
    interaction_ws_url: String,
    interaction_token: String,
    server_task: JoinHandle<Result<(), noloong_agent::interaction::InteractionError>>,
}

pub(crate) struct PreparedAppLaunch {
    pub(crate) launch_options: AppLaunchOptions,
    embedded_server: Option<Arc<RuntimeControlInteractionManager>>,
    runtime_control_server: Option<AppRuntimeControlServer>,
}

pub(crate) async fn start_embedded_interaction(
    profile_config_path: Option<String>,
) -> Result<EmbeddedInteraction, CliError> {
    let profile_config = load_profile_config(profile_config_path)?;
    let token = generate_token()?;
    let listener = TcpListener::bind(default_embedded_interaction_bind()).await?;
    let address = listener.local_addr()?;
    Ok(EmbeddedInteraction {
        profile_config,
        interaction_ws_url: format!("ws://{address}/jsonrpc/ws"),
        interaction_token: token,
        listener,
    })
}

impl EmbeddedInteraction {
    pub(crate) fn profile_config(&self) -> &HostProfileConfig {
        &self.profile_config
    }

    pub(crate) fn interaction_ws_url(&self) -> &str {
        &self.interaction_ws_url
    }

    pub(crate) fn interaction_token(&self) -> &str {
        &self.interaction_token
    }

    pub(crate) async fn run(
        self,
        bridge: impl Future<Output = Result<(), CliError>>,
    ) -> Result<(), CliError> {
        let server = self.start_server().await?;
        run_with_embedded_interaction(server.server_task, bridge).await
    }

    pub(crate) async fn start_server(self) -> Result<EmbeddedInteractionServer, CliError> {
        let registry = build_registry(&self.profile_config).await?;
        let server_token = self.interaction_token.clone();
        let interaction_ws_url = self.interaction_ws_url;
        let interaction_token = self.interaction_token;
        let listener = self.listener;
        let server_task = tokio::spawn(async move {
            serve_interaction_http(
                listener,
                InteractionControlHandler::new(registry, InteractionCapabilityPolicy::allow_all()),
                InteractionHttpTransportConfig::bearer_token(server_token),
            )
            .await
        });
        Ok(EmbeddedInteractionServer {
            interaction_ws_url,
            interaction_token,
            server_task,
        })
    }
}

impl EmbeddedInteractionServer {
    pub(crate) fn endpoint(&self) -> AppInteractionEndpoint {
        AppInteractionEndpoint {
            ws_url: self.interaction_ws_url.clone(),
            bearer_token: Some(self.interaction_token.clone()),
        }
    }

    pub(crate) async fn shutdown(self) {
        self.server_task.abort();
        let _ = self.server_task.await;
    }
}

impl PreparedAppLaunch {
    #[cfg(test)]
    pub(crate) fn has_embedded_server(&self) -> bool {
        self.embedded_server.is_some()
    }

    pub(crate) async fn shutdown(self) {
        if let Some(control_server) = self.runtime_control_server {
            control_server.shutdown().await;
        }
        if let Some(server) = self.embedded_server {
            server.shutdown().await;
        }
    }
}

async fn run_app_command(options: AppOptions) -> Result<(), CliError> {
    let prepared = prepare_app_launch(options).await?;
    let result = match resolve_app_executable().await? {
        Some(mut app) => {
            terminate_existing_app_bundle_instances().await?;
            let result = run_app_bundle(app.executable.clone(), &prepared.launch_options).await;
            app.shutdown().await;
            result
        }
        None => Err(CliError::AppBundleMissing),
    };
    prepared.shutdown().await;
    result
}

struct ResolvedAppExecutable {
    executable: PathBuf,
    dev_server: Option<Child>,
}

impl ResolvedAppExecutable {
    async fn shutdown(&mut self) {
        if let Some(child) = &mut self.dev_server {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

async fn resolve_app_executable() -> Result<Option<ResolvedAppExecutable>, CliError> {
    if let Some(executable) = resolve_explicit_app_bundle_executable() {
        return Ok(Some(ResolvedAppExecutable {
            executable,
            dev_server: None,
        }));
    }
    if let Some(app) = ensure_development_app_executable().await? {
        return Ok(Some(app));
    }
    Ok(resolve_app_bundle_executable().map(|executable| ResolvedAppExecutable {
        executable,
        dev_server: None,
    }))
}

#[cfg(test)]
pub(crate) async fn prepare_direct_app_launch_options(
    mut options: AppLaunchOptions,
) -> Result<AppLaunchOptions, CliError> {
    if options.interaction_endpoint.is_some() && options.interaction_status.is_none() {
        options.interaction_status =
            initialize_app_interaction(options.interaction_endpoint.as_ref()).await;
    }
    Ok(options)
}

pub(crate) async fn prepare_app_launch(options: AppOptions) -> Result<PreparedAppLaunch, CliError> {
    let locale = options.locale.map(config_locale_from_runtime_locale);
    let profile_config_path = options.profile_config;
    let launch_profile_config_path =
        app_launch_profile_config_path(profile_config_path.as_deref())?;
    if options.interaction_ws_url.is_some() {
        let interaction_endpoint =
            app_interaction_endpoint(options.interaction_ws_url, options.interaction_token);
        let interaction_status = initialize_app_interaction(interaction_endpoint.as_ref()).await;
        return Ok(PreparedAppLaunch {
            launch_options: AppLaunchOptions {
                app_version: AppLaunchOptions::current_app_version(),
                profile_config_path: Some(launch_profile_config_path),
                locale,
                interaction_endpoint,
                interaction_status,
                runtime_control_endpoint: None,
            },
            embedded_server: None,
            runtime_control_server: None,
        });
    }

    if !Path::new(&launch_profile_config_path).exists() {
        return Ok(PreparedAppLaunch {
            launch_options: AppLaunchOptions {
                app_version: AppLaunchOptions::current_app_version(),
                profile_config_path: Some(launch_profile_config_path),
                locale,
                interaction_endpoint: None,
                interaction_status: None,
                runtime_control_endpoint: None,
            },
            embedded_server: None,
            runtime_control_server: None,
        });
    }

    let embedded = start_embedded_interaction(Some(launch_profile_config_path.clone())).await?;
    let server = embedded.start_server().await?;
    let interaction_endpoint = server.endpoint();
    let interaction_status = initialize_app_interaction(Some(&interaction_endpoint)).await;
    let embedded_server = Arc::new(RuntimeControlInteractionManager::new(server));
    let runtime_control_server =
        start_app_runtime_control_server(
            Some(launch_profile_config_path.clone()),
            Arc::clone(&embedded_server),
        )
        .await?;
    let runtime_control_endpoint = Some(runtime_control_server.endpoint());
    Ok(PreparedAppLaunch {
        launch_options: AppLaunchOptions {
            app_version: AppLaunchOptions::current_app_version(),
            profile_config_path: Some(launch_profile_config_path),
            locale,
            interaction_endpoint: Some(interaction_endpoint),
            interaction_status,
            runtime_control_endpoint,
        },
        embedded_server: Some(embedded_server),
        runtime_control_server: Some(runtime_control_server),
    })
}

fn app_launch_profile_config_path(value: Option<&str>) -> Result<String, CliError> {
    let path = resolve_profile_config_path(value)?;
    let absolute = if path.is_absolute() {
        path
    } else {
        env::current_dir()?.join(path)
    };
    Ok(absolute.display().to_string())
}

pub(crate) async fn initialize_app_interaction(
    endpoint: Option<&AppInteractionEndpoint>,
) -> Option<noloong_app::AppInteractionStatus> {
    let endpoint = endpoint?;
    let status = match AppInteractionHttpClient::from_endpoint(endpoint) {
        Ok(client) => initialize_interaction_status(&client).await,
        Err(error) => noloong_app::AppInteractionStatus::Failed {
            error: error.to_string(),
        },
    };
    Some(status)
}

async fn run_app_bundle(
    executable: PathBuf,
    launch_options: &AppLaunchOptions,
) -> Result<(), CliError> {
    let launch_options_json = serde_json::to_string(launch_options)
        .map_err(|error| CliError::AppLaunchOptions(error.to_string()))?;
    let status = Command::new(&executable)
        .env(APP_LAUNCH_OPTIONS_ENV, launch_options_json)
        .status()
        .await?;
    if status.success() {
        return Ok(());
    }
    Err(CliError::AppBundleExited {
        executable: executable.display().to_string(),
        status,
    })
}

#[cfg(target_os = "macos")]
async fn terminate_existing_app_bundle_instances() -> Result<(), CliError> {
    let status = Command::new("/usr/bin/pkill")
        .args(["-x", MACOS_APP_EXECUTABLE_NAME])
        .status()
        .await?;
    if status.success() || status.code() == Some(1) {
        return Ok(());
    }
    Err(CliError::AppBundleExited {
        executable: MACOS_APP_EXECUTABLE_NAME.into(),
        status,
    })
}

#[cfg(not(target_os = "macos"))]
async fn terminate_existing_app_bundle_instances() -> Result<(), CliError> {
    Ok(())
}

fn resolve_app_bundle_executable() -> Option<PathBuf> {
    app_bundle_executable_candidates()
        .into_iter()
        .find(|path| path.is_file())
}

fn resolve_explicit_app_bundle_executable() -> Option<PathBuf> {
    env::var_os(APP_BUNDLE_EXECUTABLE_ENV)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

#[cfg(debug_assertions)]
async fn ensure_development_app_executable() -> Result<Option<ResolvedAppExecutable>, CliError> {
    let current_dir = env::current_dir()?;
    if !current_dir.join(DESKTOP_APP_PACKAGE_DIR).join("package.json").is_file() {
        return Ok(None);
    }

    let dev_server = ensure_desktop_dev_server().await?;

    let app_status = Command::new("cargo")
        .args(["build", "-p", "noloong-app", "--bin", MACOS_APP_EXECUTABLE_NAME])
        .status()
        .await?;
    if !app_status.success() {
        return Err(CliError::DevelopmentAppCommandExited {
            command: format!("cargo build -p noloong-app --bin {MACOS_APP_EXECUTABLE_NAME}"),
            status: app_status,
        });
    }

    let debug_executable = macos_debug_app_executable_under_target(current_dir.join("target"));
    if !debug_executable.is_file() {
        return Ok(None);
    }
    let app_executable =
        ensure_development_app_bundle(current_dir.join("target"), &debug_executable)?;
    Ok(Some(ResolvedAppExecutable {
        executable: app_executable,
        dev_server,
    }))
}

#[cfg(not(debug_assertions))]
async fn ensure_development_app_executable() -> Result<Option<ResolvedAppExecutable>, CliError> {
    Ok(None)
}

#[cfg(debug_assertions)]
async fn ensure_desktop_dev_server() -> Result<Option<Child>, CliError> {
    if TcpStream::connect(DESKTOP_DEV_SERVER_ADDRESS).await.is_ok() {
        return Ok(None);
    }

    let mut child = Command::new("bun")
        .args(["--cwd", DESKTOP_APP_PACKAGE_DIR, "dev", "--host", "127.0.0.1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    for _ in 0..50 {
        if TcpStream::connect(DESKTOP_DEV_SERVER_ADDRESS).await.is_ok() {
            return Ok(Some(child));
        }
        if let Some(status) = child.try_wait()? {
            return Err(CliError::DevelopmentAppCommandExited {
                command: format!("bun --cwd {DESKTOP_APP_PACKAGE_DIR} dev --host 127.0.0.1"),
                status,
            });
        }
        sleep(Duration::from_millis(100)).await;
    }

    let _ = child.kill().await;
    let status = child.wait().await?;
    Err(CliError::DevelopmentAppCommandExited {
        command: format!("bun --cwd {DESKTOP_APP_PACKAGE_DIR} dev --host 127.0.0.1"),
        status,
    })
}

pub(crate) fn app_bundle_executable_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(macos_app_executable_under_target(
            current_dir.join("target"),
        ));
    }
    if let Ok(current_exe) = env::current_exe()
        && let Some(candidate) = app_bundle_executable_from_current_exe(&current_exe)
    {
        candidates.push(candidate);
    }
    candidates
}

pub(crate) fn app_bundle_executable_from_current_exe(current_exe: &Path) -> Option<PathBuf> {
    current_exe
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "target"))
        .map(macos_app_executable_under_target)
}

pub(crate) fn macos_app_executable_under_target(target_dir: impl AsRef<Path>) -> PathBuf {
    target_dir
        .as_ref()
        .join("release")
        .join("bundle")
        .join("macos")
        .join(MACOS_APP_BUNDLE_NAME)
        .join("Contents")
        .join("MacOS")
        .join(MACOS_APP_EXECUTABLE_NAME)
}

pub(crate) fn macos_debug_app_executable_under_target(target_dir: impl AsRef<Path>) -> PathBuf {
    target_dir.as_ref().join("debug").join(MACOS_APP_EXECUTABLE_NAME)
}

fn ensure_development_app_bundle(
    target_dir: impl AsRef<Path>,
    debug_executable: &Path,
) -> Result<PathBuf, CliError> {
    let bundle_dir = target_dir
        .as_ref()
        .join("release")
        .join("bundle")
        .join("macos")
        .join(MACOS_APP_BUNDLE_NAME);
    let contents_dir = bundle_dir.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    let resources_dir = contents_dir.join("Resources");
    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&resources_dir)?;

    let app_executable = macos_dir.join(MACOS_APP_EXECUTABLE_NAME);
    fs::copy(debug_executable, &app_executable)?;
    set_executable_permissions(&app_executable)?;
    fs::write(contents_dir.join("Info.plist"), development_app_info_plist())?;
    fs::write(contents_dir.join("PkgInfo"), "APPL????")?;
    Ok(app_executable)
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> Result<(), CliError> {
    Ok(())
}

fn development_app_info_plist() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>Noloong</string>
  <key>CFBundleExecutable</key>
  <string>Noloong</string>
  <key>CFBundleIdentifier</key>
  <string>com.noloong.desktop</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Noloong</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>0.1.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>10.13</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
"#
}

async fn run_with_embedded_interaction(
    server_task: JoinHandle<Result<(), noloong_agent::interaction::InteractionError>>,
    bridge: impl Future<Output = Result<(), CliError>>,
) -> Result<(), CliError> {
    let mut server_task = server_task;
    tokio::select! {
        result = bridge => {
            server_task.abort();
            let _ = server_task.await;
            result
        },
        result = &mut server_task => {
            result.map_err(|error| CliError::Task(error.to_string()))?
                .map_err(CliError::Interaction)
        }
    }
}

pub(crate) fn profile_locale(
    profile_config: &HostProfileConfig,
    selected_profile_id: Option<&str>,
) -> Option<Locale> {
    profile_config
        .selected_profile(selected_profile_id)
        .and_then(|profile| profile.locale_override())
        .map(runtime_locale_from_config_locale)
}

fn runtime_locale_from_config_locale(locale: config::Locale) -> Locale {
    match locale {
        config::Locale::En => Locale::En,
        config::Locale::Zh => Locale::Zh,
    }
}

fn config_locale_from_runtime_locale(locale: Locale) -> config::Locale {
    match locale {
        Locale::En => config::Locale::En,
        Locale::Zh => config::Locale::Zh,
    }
}

fn app_interaction_endpoint(
    interaction_ws_url: Option<String>,
    interaction_token: Option<String>,
) -> Option<AppInteractionEndpoint> {
    interaction_ws_url.map(|ws_url| AppInteractionEndpoint {
        ws_url,
        bearer_token: non_empty_option(interaction_token),
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

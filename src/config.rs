use jsonc_parser::{ParseOptions, parse_to_serde_value};
use noloong_agent::{AgentPluginDeclaration, ManifestPatch};
use noloong_agent_core::{
    AnthropicEffort, ContextCompactionMode, ResponsesReasoningEffort, ResponsesReasoningSummary,
    ResponsesStateMode,
};
use schemars::{JsonSchema, Schema};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const DEFAULT_PROFILE_CONFIG_ENV: &str = "NOLOONG_PROFILE_CONFIG";
pub const DEFAULT_INTERACTION_URL_ENV: &str = "NOLOONG_INTERACTION_URL";
pub const DEFAULT_INTERACTION_TOKEN_ENV: &str = "NOLOONG_INTERACTION_TOKEN";
pub const DEFAULT_TELEGRAM_BOT_TOKEN_ENV: &str = "TELEGRAM_BOT_TOKEN";
pub const DEFAULT_TELEGRAM_BOT_USERNAME_ENV: &str = "TELEGRAM_BOT_USERNAME";
pub const DEFAULT_TELEGRAM_ALLOWED_USERS_ENV: &str = "TELEGRAM_ALLOWED_USERS";
pub const DEFAULT_TELEGRAM_ALLOWED_CHATS_ENV: &str = "TELEGRAM_ALLOWED_CHATS";
pub const DEFAULT_TELEGRAM_REQUIRE_MENTION_ENV: &str = "TELEGRAM_REQUIRE_MENTION_IN_GROUPS";
pub const DEFAULT_TELEGRAM_PROXY_ENV: &str = "TELEGRAM_PROXY";
pub const DEFAULT_TELEGRAM_FALLBACK_IPS_ENV: &str = "TELEGRAM_FALLBACK_IPS";
pub const DEFAULT_TELEGRAM_DISABLE_FALLBACK_IPS_ENV: &str = "TELEGRAM_DISABLE_FALLBACK_IPS";
pub const DEFAULT_TELEGRAM_DISABLE_ENV_PROXY_ENV: &str = "TELEGRAM_DISABLE_ENV_PROXY";
pub const DEFAULT_TELEGRAM_LOCALE_ENV: &str = "TELEGRAM_LOCALE";
pub const DEFAULT_TELEGRAM_FILE_INLINE_MAX_BYTES_ENV: &str = "TELEGRAM_FILE_INLINE_MAX_BYTES";
pub const DEFAULT_TELEGRAM_FILE_MAX_DOWNLOAD_BYTES_ENV: &str = "TELEGRAM_FILE_MAX_DOWNLOAD_BYTES";
pub const DEFAULT_TELEGRAM_FILE_DOWNLOAD_DIR_ENV: &str = "TELEGRAM_FILE_DOWNLOAD_DIR";
pub const DEFAULT_TELEGRAM_FILE_RETENTION_SECONDS_ENV: &str = "TELEGRAM_FILE_RETENTION_SECONDS";
pub const DEFAULT_TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_ENV: &str =
    "TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_TO_FILE";
pub const DEFAULT_TELEGRAM_STARTUP_UPDATE_POLICY_ENV: &str = "TELEGRAM_STARTUP_UPDATE_POLICY";
pub const DEFAULT_TELEGRAM_OFFSET_CHECKPOINT_ENV: &str = "TELEGRAM_OFFSET_CHECKPOINT";
pub const DEFAULT_WEIXIN_ACCOUNT_ID_ENV: &str = "WEIXIN_ACCOUNT_ID";
pub const DEFAULT_WEIXIN_TOKEN_ENV: &str = "WEIXIN_TOKEN";
pub const DEFAULT_WEIXIN_BASE_URL_ENV: &str = "WEIXIN_BASE_URL";
pub const DEFAULT_WEIXIN_CDN_BASE_URL_ENV: &str = "WEIXIN_CDN_BASE_URL";
pub const DEFAULT_WEIXIN_ALLOWED_USERS_ENV: &str = "WEIXIN_ALLOWED_USERS";
pub const DEFAULT_WEIXIN_ALLOW_ALL_ENV: &str = "WEIXIN_ALLOW_ALL";
pub const DEFAULT_WEIXIN_LOCALE_ENV: &str = "WEIXIN_LOCALE";
pub const DEFAULT_WEIXIN_FILE_INLINE_MAX_BYTES_ENV: &str = "WEIXIN_FILE_INLINE_MAX_BYTES";
pub const DEFAULT_WEIXIN_FILE_MAX_DOWNLOAD_BYTES_ENV: &str = "WEIXIN_FILE_MAX_DOWNLOAD_BYTES";
pub const DEFAULT_WEIXIN_FILE_MAX_UPLOAD_BYTES_ENV: &str = "WEIXIN_FILE_MAX_UPLOAD_BYTES";
pub const DEFAULT_WEIXIN_FILE_DOWNLOAD_DIR_ENV: &str = "WEIXIN_FILE_DOWNLOAD_DIR";
pub const DEFAULT_STATE_DATABASE_URL_ENV: &str = "NOLOONG_STATE_DATABASE_URL";
pub const DEFAULT_CHATGPT_TOKEN_FILE_ENV: &str = "NOLOONG_CHATGPT_TOKEN_FILE";
const DEFAULT_STATE_DATABASE_FILE_RELATIVE: &[&str] = &[".agents", "noloong", "state.sqlite"];
const DEFAULT_CHATGPT_TOKEN_FILE_RELATIVE: &[&str] =
    &[".agents", "noloong", "chatgpt", "token.json"];

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostProfileConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile_id: Option<String>,
    pub profiles: Vec<RuntimeProfileConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_store: Option<RegistryStoreConfig>,
}

impl HostProfileConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, CliConfigError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .map_err(|error| CliConfigError::ReadConfig(format!("{}: {error}", path.display())))?;
        parse_profile_config_text(&text)
    }

    pub fn validate(&self) -> Result<(), CliConfigError> {
        if self.profiles.is_empty() {
            return Err(CliConfigError::MissingProfile);
        }
        if let Some(default_profile_id) = &self.default_profile_id
            && !self
                .profiles
                .iter()
                .any(|profile| &profile.profile_id == default_profile_id)
        {
            return Err(CliConfigError::UnknownDefaultProfile(
                default_profile_id.clone(),
            ));
        }
        for profile in &self.profiles {
            profile.provider.validate().map_err(|error| {
                CliConfigError::ParseConfig(format!(
                    "profile {} provider is invalid: {error}",
                    profile.profile_id
                ))
            })?;
            for plugin in &profile.plugins {
                plugin.validate().map_err(|error| {
                    CliConfigError::ParseConfig(format!(
                        "profile {} plugin {} is invalid: {error}",
                        profile.profile_id, plugin.plugin_id
                    ))
                })?;
            }
        }
        Ok(())
    }
}

impl BuiltInProviderConfig {
    fn validate(&self) -> Result<(), CliConfigError> {
        match self {
            Self::Responses {
                state_mode,
                reasoning,
                ..
            }
            | Self::ChatgptResponses {
                state_mode,
                reasoning,
                ..
            } => validate_responses_reasoning_state_mode(*state_mode, reasoning.as_ref()),
            Self::ChatCompletions { .. } | Self::AnthropicMessages { .. } => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileConfig {
    pub profile_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub provider: BuiltInProviderConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_store: Option<ProfileEventStoreConfig>,
    #[serde(default)]
    pub compaction: ProfileCompactionConfig,
    #[serde(default)]
    pub plugins: Vec<AgentPluginDeclaration>,
    #[serde(default)]
    pub manifest_patches: Vec<ManifestPatch>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum BuiltInProviderConfig {
    ChatCompletions {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key_env: Option<String>,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        extra_body: Map<String, Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_completion_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<ChatCompletionsReasoningConfig>,
    },
    Responses {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key_env: Option<String>,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        extra_body: Map<String, Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_output_tokens: Option<u64>,
        #[serde(default)]
        state_mode: ResponsesStateMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<ResponsesProviderReasoningConfig>,
        #[serde(default)]
        allow_file_data_url_input: bool,
    },
    AnthropicMessages {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_key_env: Option<String>,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        extra_body: Map<String, Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<AnthropicProviderReasoningConfig>,
    },
    ChatgptResponses {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        model: String,
        #[serde(default)]
        auth: ChatGptAuthConfig,
        #[serde(default)]
        state_mode: ResponsesStateMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<ResponsesProviderReasoningConfig>,
        #[serde(default)]
        allow_file_data_url_input: bool,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionsReasoningConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ChatCompletionsReasoningEffort>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatCompletionsReasoningEffort {
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

impl ChatCompletionsReasoningEffort {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResponsesProviderReasoningConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ResponsesProviderReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<ResponsesProviderReasoningSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub include_encrypted: Option<bool>,
}

pub(crate) fn validate_responses_reasoning_state_mode(
    state_mode: ResponsesStateMode,
    reasoning: Option<&ResponsesProviderReasoningConfig>,
) -> Result<(), CliConfigError> {
    if state_mode.is_stateless()
        && let Some(reasoning) = reasoning
        && reasoning.enabled
        && reasoning.include_encrypted == Some(false)
    {
        return Err(CliConfigError::ParseConfig(
            "stateless responses reasoning requires includeEncrypted to be omitted or true".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesProviderReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

impl From<ResponsesProviderReasoningEffort> for ResponsesReasoningEffort {
    fn from(effort: ResponsesProviderReasoningEffort) -> Self {
        match effort {
            ResponsesProviderReasoningEffort::Minimal => Self::Minimal,
            ResponsesProviderReasoningEffort::Low => Self::Low,
            ResponsesProviderReasoningEffort::Medium => Self::Medium,
            ResponsesProviderReasoningEffort::High => Self::High,
            ResponsesProviderReasoningEffort::XHigh => Self::XHigh,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesProviderReasoningSummary {
    Auto,
    Concise,
    Detailed,
    None,
}

impl From<ResponsesProviderReasoningSummary> for ResponsesReasoningSummary {
    fn from(summary: ResponsesProviderReasoningSummary) -> Self {
        match summary {
            ResponsesProviderReasoningSummary::Auto => Self::Auto,
            ResponsesProviderReasoningSummary::Concise => Self::Concise,
            ResponsesProviderReasoningSummary::Detailed => Self::Detailed,
            ResponsesProviderReasoningSummary::None => Self::None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicProviderReasoningConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<AnthropicProviderReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicProviderThinkingMode>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicProviderReasoningEffort {
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
}

impl From<AnthropicProviderReasoningEffort> for AnthropicEffort {
    fn from(effort: AnthropicProviderReasoningEffort) -> Self {
        match effort {
            AnthropicProviderReasoningEffort::Low => Self::Low,
            AnthropicProviderReasoningEffort::Medium => Self::Medium,
            AnthropicProviderReasoningEffort::High => Self::High,
            AnthropicProviderReasoningEffort::XHigh => Self::XHigh,
            AnthropicProviderReasoningEffort::Max => Self::Max,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicProviderThinkingMode {
    Adaptive,
    Disabled,
    Omit,
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ChatGptAuthConfig {
    TokenFile {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token_file_env: Option<String>,
    },
    EnvHeaders {
        id: String,
        headers: Vec<EnvHeaderConfig>,
    },
}

impl Default for ChatGptAuthConfig {
    fn default() -> Self {
        Self::TokenFile {
            token_file: None,
            token_file_env: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvHeaderConfig {
    pub name: String,
    pub env: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_prefix: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProfileCompactionConfig {
    #[default]
    Auto,
    None,
    OpenaiResponses {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_limit_model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compact_model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1))]
        input_limit_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(max = 1.0), transform = exclusive_minimum_zero)]
        trigger_ratio: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1))]
        summary_budget_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1))]
        keep_recent_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<ContextCompactionMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(range(min = 1))]
        request_timeout_secs: Option<u64>,
    },
}

fn exclusive_minimum_zero(schema: &mut Schema) {
    schema.insert("exclusiveMinimum".into(), 0.into());
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProfileEventStoreConfig {
    #[default]
    Memory,
    Sqlite {
        database_url: String,
        #[serde(default = "default_migrate_on_connect")]
        migrate_on_connect: bool,
    },
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RegistryStoreConfig {
    #[default]
    Memory,
    Sqlite {
        database_url: String,
    },
    Postgres {
        database_url: String,
    },
    ObjectMemory {
        #[serde(default)]
        prefix: String,
    },
    ObjectFs {
        root: String,
        #[serde(default)]
        prefix: String,
    },
}

#[derive(Debug, Error)]
pub enum CliConfigError {
    #[error("profile config is required; set --profile-config or {DEFAULT_PROFILE_CONFIG_ENV}")]
    MissingProfileConfig,
    #[error("at least one runtime profile is required")]
    MissingProfile,
    #[error("default runtime profile not found: {0}")]
    UnknownDefaultProfile(String),
    #[error("failed to read profile config: {0}")]
    ReadConfig(String),
    #[error("failed to parse profile config: {0}")]
    ParseConfig(String),
    #[error("required environment variable is missing: {0}")]
    MissingEnv(String),
    #[error("home directory is required to resolve the default ChatGPT token file")]
    MissingHome,
}

pub fn env_or_value(value: Option<String>, env_name: &str) -> Option<String> {
    value
        .or_else(|| env::var(env_name).ok())
        .filter(|value| !value.trim().is_empty())
}

pub fn parse_csv_i64(value: Option<String>) -> Result<Vec<i64>, CliConfigError> {
    parse_csv(value, |item| {
        item.parse::<i64>()
            .map_err(|_| CliConfigError::ParseConfig(format!("invalid integer: {item}")))
    })
}

pub fn parse_csv_u64(value: Option<String>) -> Result<Vec<u64>, CliConfigError> {
    parse_csv(value, |item| {
        item.parse::<u64>()
            .map_err(|_| CliConfigError::ParseConfig(format!("invalid integer: {item}")))
    })
}

pub fn parse_bool_env(value: Option<String>, default: bool) -> bool {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

pub fn resolve_chatgpt_token_file(
    token_file: Option<&str>,
    token_file_env: Option<&str>,
) -> Result<PathBuf, CliConfigError> {
    resolve_chatgpt_token_file_with_env(token_file, token_file_env, process_env)
}

pub fn resolve_state_database_url() -> Result<String, CliConfigError> {
    resolve_state_database_url_with_env(process_env)
}

pub fn resolve_state_database_url_with_env(
    env_source: impl Fn(&str) -> Option<String>,
) -> Result<String, CliConfigError> {
    if let Some(database_url) =
        env_source(DEFAULT_STATE_DATABASE_URL_ENV).filter(|value| !value.trim().is_empty())
    {
        return Ok(database_url);
    }
    let mut path = home_dir(&env_source)?;
    for component in DEFAULT_STATE_DATABASE_FILE_RELATIVE {
        path.push(component);
    }
    Ok(format!("sqlite:{}", path.display()))
}

pub fn ensure_sqlite_database_parent(database_url: &str) -> Result<(), CliConfigError> {
    let Some(path) = sqlite_database_path(database_url)? else {
        return Ok(());
    };
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            CliConfigError::ReadConfig(format!("{}: {error}", parent.display()))
        })?;
    }
    Ok(())
}

pub fn sqlite_database_path(database_url: &str) -> Result<Option<PathBuf>, CliConfigError> {
    match database_url {
        "" => Err(CliConfigError::ParseConfig(
            "sqlite database url is empty".into(),
        )),
        "sqlite::memory:" | "sqlite://memory" | ":memory:" => Ok(None),
        url if url.starts_with("sqlite://") => {
            sqlite_path_from_suffix(url.strip_prefix("sqlite://").unwrap_or_default())
        }
        url if url.starts_with("sqlite:") => {
            sqlite_path_from_suffix(url.strip_prefix("sqlite:").unwrap_or_default())
        }
        url if url.contains("://") => Err(CliConfigError::ParseConfig(format!(
            "state database URL must be sqlite, got: {url}"
        ))),
        path => Ok(Some(PathBuf::from(path))),
    }
}

fn sqlite_path_from_suffix(path: &str) -> Result<Option<PathBuf>, CliConfigError> {
    if path.is_empty() {
        return Err(CliConfigError::ParseConfig(
            "sqlite database path is empty".into(),
        ));
    }
    if path == ":memory:" || path == "memory" {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(path)))
}

pub fn resolve_chatgpt_token_file_with_env(
    token_file: Option<&str>,
    token_file_env: Option<&str>,
    env_source: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, CliConfigError> {
    if let Some(token_file) = non_empty_str(token_file) {
        return expand_home_path(token_file, &env_source);
    }
    if let Some(env_name) = non_empty_str(token_file_env) {
        let token_file = env_source(env_name)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| CliConfigError::MissingEnv(env_name.to_string()))?;
        return expand_home_path(&token_file, &env_source);
    }
    if let Some(token_file) =
        env_source(DEFAULT_CHATGPT_TOKEN_FILE_ENV).filter(|value| !value.trim().is_empty())
    {
        return expand_home_path(&token_file, &env_source);
    }
    let mut path = home_dir(&env_source)?;
    for component in DEFAULT_CHATGPT_TOKEN_FILE_RELATIVE {
        path.push(component);
    }
    Ok(path)
}

fn process_env(name: &str) -> Option<String> {
    env::var(name).ok()
}

fn expand_home_path(
    value: &str,
    env_source: &impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, CliConfigError> {
    let value = value.trim();
    if value == "~" {
        return home_dir(env_source);
    }
    if let Some(stripped) = value.strip_prefix("~/") {
        return Ok(home_dir(env_source)?.join(stripped));
    }
    Ok(PathBuf::from(value))
}

fn home_dir(env_source: &impl Fn(&str) -> Option<String>) -> Result<PathBuf, CliConfigError> {
    env_source("HOME")
        .or_else(|| env_source("USERPROFILE"))
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .ok_or(CliConfigError::MissingHome)
}

fn non_empty_str(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn default_migrate_on_connect() -> bool {
    true
}

fn parse_profile_config_text(text: &str) -> Result<HostProfileConfig, CliConfigError> {
    let value = parse_profile_config_value(text)?;
    serde_json::from_value(value).map_err(|error| CliConfigError::ParseConfig(error.to_string()))
}

pub(crate) fn parse_profile_config_value(text: &str) -> Result<Value, CliConfigError> {
    parse_to_serde_value::<Value>(text, &profile_jsonc_parse_options())
        .map_err(|error| CliConfigError::ParseConfig(error.to_string()))
}

pub(crate) fn profile_jsonc_parse_options() -> ParseOptions {
    ParseOptions {
        allow_comments: true,
        allow_loose_object_property_names: false,
        allow_trailing_commas: true,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
    }
}

fn parse_csv<T>(
    value: Option<String>,
    parse: impl Fn(&str) -> Result<T, CliConfigError>,
) -> Result<Vec<T>, CliConfigError> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(parse)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        AnthropicProviderReasoningEffort, AnthropicProviderThinkingMode, BuiltInProviderConfig,
        ChatCompletionsReasoningEffort, ChatGptAuthConfig, HostProfileConfig,
        ProfileCompactionConfig, ProfileEventStoreConfig, ResponsesProviderReasoningEffort,
        ResponsesProviderReasoningSummary, ResponsesStateMode, RuntimeProfileConfig,
        ensure_sqlite_database_parent, resolve_chatgpt_token_file_with_env,
        resolve_state_database_url_with_env, sqlite_database_path,
    };
    use crate::test_support::{remove_temp_file, write_temp_file};
    use noloong_agent_core::ContextCompactionMode;
    use std::path::PathBuf;

    #[test]
    fn profile_config_loads_chat_completions() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chat_completions",
                        "model": "gpt-5.4-mini",
                        "apiKeyEnv": "OPENROUTER_API_KEY"
                    }
                }]
            }"#,
        )
        .unwrap();

        assert!(config.validate().is_ok());
        assert!(matches!(
            config.profiles[0].provider,
            BuiltInProviderConfig::ChatCompletions { .. }
        ));
        assert_eq!(config.profiles[0].event_store, None);
    }

    #[test]
    fn profile_config_loads_chat_completions_reasoning() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {
                    "type": "chat_completions",
                    "model": "gpt-5.4-mini",
                    "reasoning": {
                        "enabled": true,
                        "effort": "xhigh"
                    }
                }
            }"#,
        )
        .unwrap();

        let BuiltInProviderConfig::ChatCompletions {
            reasoning: Some(reasoning),
            ..
        } = config.provider
        else {
            panic!("expected Chat Completions reasoning");
        };
        assert!(reasoning.enabled);
        assert_eq!(
            reasoning.effort,
            Some(ChatCompletionsReasoningEffort::XHigh)
        );
    }

    #[test]
    fn profile_config_loads_responses_reasoning() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {
                    "type": "responses",
                    "model": "gpt-5.4-mini",
                    "reasoning": {
                        "effort": "medium",
                        "summary": "detailed",
                        "includeEncrypted": true
                    }
                }
            }"#,
        )
        .unwrap();

        let BuiltInProviderConfig::Responses {
            reasoning: Some(reasoning),
            ..
        } = config.provider
        else {
            panic!("expected Responses reasoning");
        };
        assert!(reasoning.enabled);
        assert_eq!(
            reasoning.effort,
            Some(ResponsesProviderReasoningEffort::Medium)
        );
        assert_eq!(
            reasoning.summary,
            Some(ResponsesProviderReasoningSummary::Detailed)
        );
        assert_eq!(reasoning.include_encrypted, Some(true));
    }

    #[test]
    fn profile_config_loads_responses_state_mode() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {
                    "type": "responses",
                    "model": "gpt-5.4-mini",
                    "stateMode": "stateful"
                }
            }"#,
        )
        .unwrap();

        let BuiltInProviderConfig::Responses { state_mode, .. } = config.provider else {
            panic!("expected Responses provider");
        };
        assert_eq!(state_mode, ResponsesStateMode::Stateful);
    }

    #[test]
    fn profile_config_loads_responses_file_data_url_opt_in() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {
                    "type": "responses",
                    "model": "gpt-5.4-mini",
                    "allowFileDataUrlInput": true
                }
            }"#,
        )
        .unwrap();

        let BuiltInProviderConfig::Responses {
            allow_file_data_url_input,
            ..
        } = config.provider
        else {
            panic!("expected Responses provider");
        };
        assert!(allow_file_data_url_input);
    }

    #[test]
    fn profile_config_rejects_stateless_reasoning_without_encrypted_replay() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini",
                        "stateMode": "stateless",
                        "reasoning": {
                            "enabled": true,
                            "includeEncrypted": false
                        }
                    }
                }]
            }"#,
        )
        .unwrap();

        let error = config
            .validate()
            .expect_err("invalid stateless reasoning config");

        assert!(error.to_string().contains("includeEncrypted"));
    }

    #[test]
    fn profile_config_loads_anthropic_reasoning() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {
                    "type": "anthropic_messages",
                    "model": "claude-opus-4-7",
                    "reasoning": {
                        "effort": "max",
                        "thinking": "adaptive"
                    }
                }
            }"#,
        )
        .unwrap();

        let BuiltInProviderConfig::AnthropicMessages {
            reasoning: Some(reasoning),
            ..
        } = config.provider
        else {
            panic!("expected Anthropic reasoning");
        };
        assert_eq!(
            reasoning.effort,
            Some(AnthropicProviderReasoningEffort::Max)
        );
        assert_eq!(
            reasoning.thinking,
            Some(AnthropicProviderThinkingMode::Adaptive)
        );
    }

    #[test]
    fn profile_config_load_reads_json_file() {
        let path = write_temp_file(
            "profile-json",
            "json",
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.4-mini"}
                }]
            }"#,
        );

        let config = HostProfileConfig::load(&path).unwrap();
        remove_temp_file(path);

        assert_eq!(config.profiles[0].profile_id, "default");
    }

    #[test]
    fn profile_config_load_reads_jsonc_file() {
        let path = write_temp_file(
            "profile-jsonc",
            "jsonc",
            r#"{
                // Editor tooling can keep comments in profile configs.
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.4-mini"},
                }],
            }"#,
        );

        let config = HostProfileConfig::load(&path).unwrap();
        remove_temp_file(path);

        assert_eq!(config.profiles[0].profile_id, "default");
    }

    #[test]
    fn profile_config_loads_jsonc_example() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/profile-configs/telegram-openrouter-free.jsonc");

        let config = HostProfileConfig::load(path).unwrap();

        config.validate().unwrap();
        assert_eq!(config.profiles[0].profile_id, "telegram-openrouter-free");
    }

    #[test]
    fn profile_config_loads_weixin_chatgpt_example() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/profile-configs/weixin-chatgpt-subscription.json");

        let config = HostProfileConfig::load(path).unwrap();

        config.validate().unwrap();
        assert_eq!(config.default_profile_id.as_deref(), Some("weixin-chatgpt"));
        assert_eq!(config.profiles[0].metadata["channel"], "weixin");
    }

    #[test]
    fn profile_config_load_rejects_json5_only_syntax() {
        let path = write_temp_file(
            "profile-json5",
            "jsonc",
            r#"{
                profiles: [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.4-mini"}
                }]
            }"#,
        );

        let error = HostProfileConfig::load(&path).unwrap_err();
        remove_temp_file(path);

        assert!(error.to_string().contains("failed to parse profile config"));
    }

    #[test]
    fn runtime_profile_config_loads_sqlite_event_store() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {"type": "responses", "model": "gpt-5.4-mini"},
                "eventStore": {
                    "type": "sqlite",
                    "databaseUrl": "sqlite:target/noloong-events.sqlite"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            config.event_store,
            Some(ProfileEventStoreConfig::Sqlite {
                database_url: "sqlite:target/noloong-events.sqlite".into(),
                migrate_on_connect: true,
            })
        );
    }

    #[test]
    fn runtime_profile_config_loads_sqlite_event_store_without_migrations() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {"type": "responses", "model": "gpt-5.4-mini"},
                "eventStore": {
                    "type": "sqlite",
                    "databaseUrl": "sqlite:target/noloong-events.sqlite",
                    "migrateOnConnect": false
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            config.event_store,
            Some(ProfileEventStoreConfig::Sqlite {
                database_url: "sqlite:target/noloong-events.sqlite".into(),
                migrate_on_connect: false,
            })
        );
    }

    #[test]
    fn profile_config_rejects_unknown_provider() {
        let error = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "unknown", "model": "x"}
                }]
            }"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown variant"));
    }

    #[test]
    fn profile_config_builds_registry_store_config() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "registryStore": {"type": "sqlite", "databaseUrl": "sqlite::memory:"},
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.4-mini"}
                }]
            }"#,
        )
        .unwrap();

        assert!(config.validate().is_ok());
        assert!(matches!(
            config.registry_store,
            Some(super::RegistryStoreConfig::Sqlite { .. })
        ));
    }

    #[test]
    fn state_database_url_uses_default_home_path() {
        let url = resolve_state_database_url_with_env(|name| match name {
            "HOME" => Some("/home/alice".into()),
            _ => None,
        })
        .unwrap();

        assert_eq!(url, "sqlite:/home/alice/.agents/noloong/state.sqlite");
    }

    #[test]
    fn state_database_url_prefers_env() {
        let url = resolve_state_database_url_with_env(|name| match name {
            "HOME" => Some("/home/alice".into()),
            "NOLOONG_STATE_DATABASE_URL" => Some("sqlite:/tmp/noloong.sqlite".into()),
            _ => None,
        })
        .unwrap();

        assert_eq!(url, "sqlite:/tmp/noloong.sqlite");
    }

    #[test]
    fn state_database_url_ignores_empty_env() {
        let url = resolve_state_database_url_with_env(|name| match name {
            "HOME" => Some("/home/alice".into()),
            "NOLOONG_STATE_DATABASE_URL" => Some("   ".into()),
            _ => None,
        })
        .unwrap();

        assert_eq!(url, "sqlite:/home/alice/.agents/noloong/state.sqlite");
    }

    #[test]
    fn sqlite_database_path_parses_supported_urls() {
        assert_eq!(
            sqlite_database_path("sqlite:/tmp/noloong.sqlite").unwrap(),
            Some(PathBuf::from("/tmp/noloong.sqlite"))
        );
        assert_eq!(sqlite_database_path("sqlite::memory:").unwrap(), None);
        assert!(sqlite_database_path("postgres://localhost/db").is_err());
    }

    #[test]
    fn sqlite_database_parent_is_created() {
        let dir = crate::test_support::temp_dir("state-database-parent");
        let db = dir.join("nested").join("state.sqlite");
        let url = format!("sqlite:{}", db.display());

        ensure_sqlite_database_parent(&url).unwrap();

        assert!(db.parent().unwrap().is_dir());
        crate::test_support::remove_temp_dir(dir);
    }

    #[test]
    fn profile_config_loads_default_plugins() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.4-mini"},
                    "plugins": [{
                        "pluginId": "echo",
                        "displayName": "Echo",
                        "enabled": true,
                        "transport": {
                            "type": "stdio",
                            "command": "node",
                            "args": ["examples/extensions/echo.mjs"],
                            "env": {
                                "PATH": {
                                    "type": "host_env",
                                    "name": "PATH"
                                }
                            }
                        },
                        "allowedCapabilities": [
                            {"type": "tool", "name": "echo.run"}
                        ]
                    }]
                }]
            }"#,
        )
        .unwrap();

        config.validate().unwrap();
        assert_eq!(config.profiles[0].plugins.len(), 1);
        assert_eq!(config.profiles[0].plugins[0].plugin_id, "echo");
    }

    #[test]
    fn profile_config_loads_chatgpt_responses_with_default_token_file_auth() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "chatgpt_responses", "model": "gpt-5.4-mini"}
                }]
            }"#,
        )
        .unwrap();

        let BuiltInProviderConfig::ChatgptResponses { auth, .. } = &config.profiles[0].provider
        else {
            panic!("expected ChatGPT responses provider");
        };
        assert_eq!(auth, &ChatGptAuthConfig::default());
        assert_eq!(config.profiles[0].compaction, ProfileCompactionConfig::Auto);
    }

    #[test]
    fn profile_config_loads_chatgpt_responses_file_data_url_opt_in() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini",
                        "allowFileDataUrlInput": true
                    }
                }]
            }"#,
        )
        .unwrap();

        let BuiltInProviderConfig::ChatgptResponses {
            allow_file_data_url_input,
            ..
        } = &config.profiles[0].provider
        else {
            panic!("expected ChatGPT responses provider");
        };
        assert!(*allow_file_data_url_input);
    }

    #[test]
    fn profile_config_loads_chatgpt_env_headers_escape_hatch() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini",
                        "auth": {
                            "type": "env_headers",
                            "id": "custom-auth",
                            "headers": [{
                                "name": "Authorization",
                                "env": "CHATGPT_ACCESS_TOKEN",
                                "valuePrefix": "Bearer "
                            }]
                        }
                    }
                }]
            }"#,
        )
        .unwrap();

        let BuiltInProviderConfig::ChatgptResponses { auth, .. } = &config.profiles[0].provider
        else {
            panic!("expected ChatGPT responses provider");
        };
        assert!(matches!(auth, ChatGptAuthConfig::EnvHeaders { .. }));
    }

    #[test]
    fn profile_config_loads_openai_responses_compaction() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "chatgpt_responses", "model": "gpt-5.4-mini"},
                    "compaction": {
                        "type": "openai_responses",
                        "inputLimitModel": "gpt-5.4-mini",
                        "compactModel": "gpt-5.4-mini",
                        "inputLimitTokens": 200000,
                        "triggerRatio": 0.8,
                        "summaryBudgetTokens": 32000,
                        "keepRecentTokens": 64000,
                        "mode": "request_only",
                        "requestTimeoutSecs": 120
                    }
                }]
            }"#,
        )
        .unwrap();

        let ProfileCompactionConfig::OpenaiResponses {
            input_limit_model,
            compact_model,
            input_limit_tokens,
            trigger_ratio,
            summary_budget_tokens,
            keep_recent_tokens,
            mode,
            request_timeout_secs,
            ..
        } = &config.profiles[0].compaction
        else {
            panic!("expected OpenAI responses compaction");
        };
        assert_eq!(input_limit_model.as_deref(), Some("gpt-5.4-mini"));
        assert_eq!(compact_model.as_deref(), Some("gpt-5.4-mini"));
        assert_eq!(*input_limit_tokens, Some(200_000));
        assert_eq!(*trigger_ratio, Some(0.8));
        assert_eq!(*summary_budget_tokens, Some(32_000));
        assert_eq!(*keep_recent_tokens, Some(64_000));
        assert_eq!(*mode, Some(ContextCompactionMode::RequestOnly));
        assert_eq!(*request_timeout_secs, Some(120));
    }

    #[test]
    fn chatgpt_token_file_resolver_uses_default_home_path() {
        let path = resolve_chatgpt_token_file_with_env(None, None, |name| match name {
            "HOME" => Some("/home/alice".into()),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            path,
            PathBuf::from("/home/alice/.agents/noloong/chatgpt/token.json")
        );
    }

    #[test]
    fn chatgpt_token_file_resolver_prefers_explicit_path() {
        let path = resolve_chatgpt_token_file_with_env(
            Some("~/custom-token.json"),
            Some("CUSTOM_TOKEN"),
            |name| match name {
                "HOME" => Some("/home/alice".into()),
                "CUSTOM_TOKEN" => Some("/ignored/token.json".into()),
                "NOLOONG_CHATGPT_TOKEN_FILE" => Some("/ignored/default-token.json".into()),
                _ => None,
            },
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/home/alice/custom-token.json"));
    }

    #[test]
    fn chatgpt_token_file_resolver_uses_named_env_before_default_env() {
        let path =
            resolve_chatgpt_token_file_with_env(None, Some("CUSTOM_TOKEN"), |name| match name {
                "HOME" => Some("/home/alice".into()),
                "CUSTOM_TOKEN" => Some("~/from-custom-env.json".into()),
                "NOLOONG_CHATGPT_TOKEN_FILE" => Some("/ignored/default-token.json".into()),
                _ => None,
            })
            .unwrap();

        assert_eq!(path, PathBuf::from("/home/alice/from-custom-env.json"));
    }

    #[test]
    fn chatgpt_token_file_resolver_uses_default_env_before_home_default() {
        let path = resolve_chatgpt_token_file_with_env(None, None, |name| match name {
            "HOME" => Some("/home/alice".into()),
            "NOLOONG_CHATGPT_TOKEN_FILE" => Some("/tmp/token.json".into()),
            _ => None,
        })
        .unwrap();

        assert_eq!(path, PathBuf::from("/tmp/token.json"));
    }
}

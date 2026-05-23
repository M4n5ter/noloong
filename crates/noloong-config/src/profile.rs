use crate::{
    AgentPluginDeclaration, ContextCompactionMode, FileEditToolPolicy, Locale, ManifestPatch,
    ResponsesStateMode, SqliteDatabaseLocation,
};
use config::{Config, FileFormat};
use jsonc_parser::{ParseOptions, parse_to_serde_value};
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
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
const DEFAULT_PROFILE_CONFIG_FILE_RELATIVE: &[&str] =
    &[".agents", "noloong", "profile-config.jsonc"];
const DEFAULT_CHATGPT_TOKEN_FILE_RELATIVE: &[&str] =
    &[".agents", "noloong", "chatgpt", "token.json"];

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
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

    pub fn save_canonical(&self, path: impl AsRef<Path>) -> Result<(), CliConfigError> {
        let path = path.as_ref();
        let text = self.to_canonical_json()?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| {
                CliConfigError::ReadConfig(format!("{}: {error}", parent.display()))
            })?;
        }
        fs::write(path, text)
            .map_err(|error| CliConfigError::ReadConfig(format!("{}: {error}", path.display())))
    }

    pub fn to_canonical_json(&self) -> Result<String, CliConfigError> {
        serde_json::to_string_pretty(self).map_err(|error| {
            CliConfigError::ParseConfig(format!("failed to serialize profile config: {error}"))
        })
    }

    pub fn selected_profile(
        &self,
        selected_profile_id: Option<&str>,
    ) -> Option<&RuntimeProfileConfig> {
        self.selected_profile_index(selected_profile_id)
            .and_then(|index| self.profiles.get(index))
    }

    pub fn selected_profile_mut(
        &mut self,
        selected_profile_id: Option<&str>,
    ) -> Option<&mut RuntimeProfileConfig> {
        self.selected_profile_index(selected_profile_id)
            .and_then(|index| self.profiles.get_mut(index))
    }

    pub fn selected_profile_index(&self, selected_profile_id: Option<&str>) -> Option<usize> {
        let Some(profile_id) = selected_profile_id.or(self.default_profile_id.as_deref()) else {
            return (!self.profiles.is_empty()).then_some(0);
        };
        self.profiles
            .iter()
            .position(|profile| profile.profile_id == profile_id)
    }

    pub fn validate(&self) -> Result<(), CliConfigError> {
        if self.profiles.is_empty() {
            return Err(CliConfigError::MissingProfile);
        }
        let mut profile_ids = BTreeSet::new();
        for profile in &self.profiles {
            if !profile_ids.insert(profile.profile_id.clone()) {
                return Err(CliConfigError::DuplicateProfileId(
                    profile.profile_id.clone(),
                ));
            }
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

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
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

impl RuntimeProfileConfig {
    pub fn locale_override(&self) -> Option<Locale> {
        self.manifest_patches
            .iter()
            .rev()
            .find_map(|patch| match patch {
                ManifestPatch::SetLocale { locale } => Some(*locale),
                _ => None,
            })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
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

impl BuiltInProviderConfig {
    pub const fn type_tag(&self) -> &'static str {
        match self {
            Self::ChatCompletions { .. } => "chat_completions",
            Self::Responses { .. } => "responses",
            Self::AnthropicMessages { .. } => "anthropic_messages",
            Self::ChatgptResponses { .. } => "chatgpt_responses",
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Self::ChatCompletions { model, .. }
            | Self::Responses { model, .. }
            | Self::AnthropicMessages { model, .. }
            | Self::ChatgptResponses { model, .. } => model,
        }
    }

    pub fn model_mut(&mut self) -> &mut String {
        match self {
            Self::ChatCompletions { model, .. }
            | Self::Responses { model, .. }
            | Self::AnthropicMessages { model, .. }
            | Self::ChatgptResponses { model, .. } => model,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionsReasoningConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ChatCompletionsReasoningEffort>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
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

pub fn validate_responses_reasoning_state_mode(
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesProviderReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesProviderReasoningSummary {
    Auto,
    Concise,
    Detailed,
    None,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicProviderReasoningConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<AnthropicProviderReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicProviderThinkingMode>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicProviderReasoningEffort {
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicProviderThinkingMode {
    Adaptive,
    Disabled,
    Omit,
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
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

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvHeaderConfig {
    pub name: String,
    pub env: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_prefix: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
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

impl ProfileCompactionConfig {
    pub const fn type_tag(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::OpenaiResponses { .. } => "openai_responses",
        }
    }
}

fn exclusive_minimum_zero(schema: &mut Schema) {
    schema.insert("exclusiveMinimum".into(), 0.into());
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
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

impl ProfileEventStoreConfig {
    pub fn summary(&self) -> String {
        match self {
            Self::Memory => "memory".into(),
            Self::Sqlite { database_url, .. } => database_url.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
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
    #[error("duplicate runtime profile id: {0}")]
    DuplicateProfileId(String),
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

pub fn starter_profile_config() -> HostProfileConfig {
    HostProfileConfig {
        default_profile_id: Some("chatgpt-responses".into()),
        profiles: vec![RuntimeProfileConfig {
            profile_id: "chatgpt-responses".into(),
            display_name: "ChatGPT Responses".into(),
            description: Some("ChatGPT subscription through the Responses backend.".into()),
            provider: BuiltInProviderConfig::ChatgptResponses {
                provider_id: None,
                model: "gpt-5.4-mini".into(),
                auth: ChatGptAuthConfig::default(),
                state_mode: ResponsesStateMode::default(),
                reasoning: Some(ResponsesProviderReasoningConfig {
                    enabled: true,
                    effort: Some(ResponsesProviderReasoningEffort::Medium),
                    summary: Some(ResponsesProviderReasoningSummary::Auto),
                    include_encrypted: None,
                }),
                allow_file_data_url_input: true,
            },
            event_store: None,
            compaction: ProfileCompactionConfig::Auto,
            plugins: Vec::new(),
            manifest_patches: vec![
                ManifestPatch::SetLocale { locale: Locale::Zh },
                ManifestPatch::UpdateFileEditToolPolicy {
                    policy: FileEditToolPolicy::AutoByModel,
                },
            ],
            metadata: Map::new(),
        }],
        registry_store: None,
    }
}

pub fn resolve_profile_config_path(value: Option<&str>) -> Result<PathBuf, CliConfigError> {
    resolve_profile_config_path_with_env(value, process_env)
}

pub fn resolve_profile_config_path_with_env(
    value: Option<&str>,
    env_source: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, CliConfigError> {
    if let Some(value) = non_empty_str(value) {
        return expand_home_path(value, &env_source);
    }
    if let Some(value) =
        env_source(DEFAULT_PROFILE_CONFIG_ENV).filter(|value| !value.trim().is_empty())
    {
        return expand_home_path(&value, &env_source);
    }
    default_profile_config_path_with_env(env_source)
}

pub fn default_profile_config_path() -> Result<PathBuf, CliConfigError> {
    default_profile_config_path_with_env(process_env)
}

pub fn default_profile_config_path_with_env(
    env_source: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, CliConfigError> {
    let mut path = home_dir(&env_source)?;
    for component in DEFAULT_PROFILE_CONFIG_FILE_RELATIVE {
        path.push(component);
    }
    Ok(path)
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
    let location = SqliteDatabaseLocation::parse(database_url)
        .map_err(|error| CliConfigError::ParseConfig(error.to_string()))?;
    let Some(path) = location.path() else {
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

pub fn parse_profile_config_text(text: &str) -> Result<HostProfileConfig, CliConfigError> {
    let value = parse_profile_config_value(text)?;
    let json = serde_json::to_string(&value).map_err(|error| {
        CliConfigError::ParseConfig(format!("failed to normalize profile config: {error}"))
    })?;
    Config::builder()
        .add_source(config::File::from_str(&json, FileFormat::Json))
        .build()
        .and_then(Config::try_deserialize)
        .map_err(|error| CliConfigError::ParseConfig(error.to_string()))
}

pub fn parse_profile_config_value(text: &str) -> Result<Value, CliConfigError> {
    parse_to_serde_value::<Value>(text, &profile_jsonc_parse_options())
        .map_err(|error| CliConfigError::ParseConfig(error.to_string()))
}

pub fn profile_jsonc_parse_options() -> ParseOptions {
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
mod tests;

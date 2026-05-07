use jsonc_parser::{ParseOptions, parse_to_serde_value};
use noloong_agent::{AgentPluginDeclaration, ManifestPatch};
use noloong_agent_core::ContextCompactionMode;
use schemars::JsonSchema;
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
pub const DEFAULT_CHATGPT_TOKEN_FILE_ENV: &str = "NOLOONG_CHATGPT_TOKEN_FILE";
const DEFAULT_CHATGPT_TOKEN_FILE_RELATIVE: &[&str] =
    &[".agents", "noloong", "chatgpt", "token.json"];

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostProfileConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile_id: Option<String>,
    pub profiles: Vec<RuntimeProfileConfig>,
    #[serde(default)]
    pub registry_store: RegistryStoreConfig,
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

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileConfig {
    pub profile_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub provider: BuiltInProviderConfig,
    #[serde(default)]
    pub event_store: ProfileEventStoreConfig,
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
    },
    ChatgptResponses {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_id: Option<String>,
        model: String,
        #[serde(default)]
        auth: ChatGptAuthConfig,
    },
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

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Eq)]
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
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_window_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reserve_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keep_recent_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<ContextCompactionMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_timeout_secs: Option<u64>,
    },
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
        BuiltInProviderConfig, ChatGptAuthConfig, HostProfileConfig, ProfileCompactionConfig,
        ProfileEventStoreConfig, RuntimeProfileConfig, resolve_chatgpt_token_file_with_env,
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
                        "model": "gpt-5.5-mini",
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
        assert_eq!(
            config.profiles[0].event_store,
            ProfileEventStoreConfig::Memory
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
                    "provider": {"type": "responses", "model": "gpt-5.5-mini"}
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
                    "provider": {"type": "responses", "model": "gpt-5.5-mini"},
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
    fn profile_config_load_rejects_json5_only_syntax() {
        let path = write_temp_file(
            "profile-json5",
            "jsonc",
            r#"{
                profiles: [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.5-mini"}
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
                "provider": {"type": "responses", "model": "gpt-5.5-mini"},
                "eventStore": {
                    "type": "sqlite",
                    "databaseUrl": "sqlite:target/noloong-events.sqlite"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            config.event_store,
            ProfileEventStoreConfig::Sqlite {
                database_url: "sqlite:target/noloong-events.sqlite".into(),
                migrate_on_connect: true,
            }
        );
    }

    #[test]
    fn runtime_profile_config_loads_sqlite_event_store_without_migrations() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {"type": "responses", "model": "gpt-5.5-mini"},
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
            ProfileEventStoreConfig::Sqlite {
                database_url: "sqlite:target/noloong-events.sqlite".into(),
                migrate_on_connect: false,
            }
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
                    "provider": {"type": "responses", "model": "gpt-5.5-mini"}
                }]
            }"#,
        )
        .unwrap();

        assert!(config.validate().is_ok());
    }

    #[test]
    fn profile_config_loads_default_plugins() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.5-mini"},
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
                        "model": "gpt-5.4-mini",
                        "contextWindowTokens": 200000,
                        "reserveTokens": 32000,
                        "keepRecentTokens": 64000,
                        "mode": "request_only",
                        "requestTimeoutSecs": 120
                    }
                }]
            }"#,
        )
        .unwrap();

        let ProfileCompactionConfig::OpenaiResponses {
            model,
            context_window_tokens,
            reserve_tokens,
            keep_recent_tokens,
            mode,
            request_timeout_secs,
            ..
        } = &config.profiles[0].compaction
        else {
            panic!("expected OpenAI responses compaction");
        };
        assert_eq!(model.as_deref(), Some("gpt-5.4-mini"));
        assert_eq!(*context_window_tokens, Some(200_000));
        assert_eq!(*reserve_tokens, Some(32_000));
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

use noloong_agent::ManifestPatch;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, env, fs, path::Path};
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

#[derive(Clone, Debug, Deserialize, PartialEq)]
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
        serde_json::from_str(&text).map_err(|error| CliConfigError::ParseConfig(error.to_string()))
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
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileConfig {
    pub profile_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub provider: BuiltInProviderConfig,
    #[serde(default)]
    pub manifest_patches: Vec<ManifestPatch>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
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
        auth: EnvAuthProviderConfig,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EnvAuthProviderConfig {
    EnvHeaders {
        id: String,
        headers: Vec<EnvHeaderConfig>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvHeaderConfig {
    pub name: String,
    pub env: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_prefix: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
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
    use super::{BuiltInProviderConfig, HostProfileConfig};

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
}

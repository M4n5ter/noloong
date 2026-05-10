use crate::{access::TelegramAccessPolicy, network::TelegramNetworkConfig};
use noloong_agent::Locale;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr, time::Duration};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TelegramBridgeConfig {
    pub bot_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_username: Option<String>,
    pub interaction_ws_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default = "default_message_window_ms")]
    pub message_window_ms: u64,
    #[serde(default = "default_long_split_window_ms")]
    pub long_split_window_ms: u64,
    #[serde(default = "default_edit_throttle_ms")]
    pub edit_throttle_ms: u64,
    #[serde(default = "default_max_outbound_chars")]
    pub max_outbound_chars: usize,
    #[serde(default)]
    pub access: TelegramAccessPolicy,
    #[serde(default)]
    pub network: TelegramNetworkConfig,
    #[serde(default)]
    pub file_policy: TelegramFilePolicy,
    #[serde(default)]
    pub startup_update_policy: TelegramStartupUpdatePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_checkpoint_path: Option<PathBuf>,
    #[serde(default = "default_show_tool_status")]
    pub show_tool_status: bool,
    #[serde(default = "default_locale")]
    pub locale: Locale,
}

impl TelegramBridgeConfig {
    pub fn message_window(&self) -> Duration {
        Duration::from_millis(self.message_window_ms)
    }

    pub fn long_split_window(&self) -> Duration {
        Duration::from_millis(self.long_split_window_ms)
    }

    pub fn edit_throttle(&self) -> Duration {
        Duration::from_millis(self.edit_throttle_ms)
    }

    pub fn validate(&self) -> Result<(), TelegramConfigError> {
        if self.bot_token.trim().is_empty() {
            return Err(TelegramConfigError::MissingBotToken);
        }
        if self
            .bot_username
            .as_deref()
            .is_some_and(|username| username.trim().is_empty())
        {
            return Err(TelegramConfigError::InvalidBotUsername);
        }
        if self.interaction_ws_url.trim().is_empty() {
            return Err(TelegramConfigError::MissingInteractionUrl);
        }
        self.file_policy.validate()?;
        self.access.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TelegramFilePolicy {
    #[serde(default = "default_inline_max_bytes")]
    pub inline_max_bytes: usize,
    #[serde(default = "default_max_download_bytes")]
    pub max_download_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_seconds: Option<u64>,
}

impl Default for TelegramFilePolicy {
    fn default() -> Self {
        Self {
            inline_max_bytes: default_inline_max_bytes(),
            max_download_bytes: default_max_download_bytes(),
            download_dir: None,
            retention_seconds: None,
        }
    }
}

impl TelegramFilePolicy {
    fn validate(&self) -> Result<(), TelegramConfigError> {
        if self.inline_max_bytes > self.max_download_bytes {
            return Err(TelegramConfigError::InvalidFilePolicy(
                "inlineMaxBytes must be less than or equal to maxDownloadBytes".into(),
            ));
        }
        if self
            .download_dir
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err(TelegramConfigError::InvalidFilePolicy(
                "downloadDir must not be empty when configured".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelegramStartupUpdatePolicy {
    ProcessPending,
    #[default]
    SkipPendingWithoutCheckpoint,
}

impl FromStr for TelegramStartupUpdatePolicy {
    type Err = TelegramStartupUpdatePolicyParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "process_pending" => Ok(Self::ProcessPending),
            "skip_pending_without_checkpoint" => Ok(Self::SkipPendingWithoutCheckpoint),
            _ => Err(TelegramStartupUpdatePolicyParseError(value.into())),
        }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("invalid Telegram startup update policy: {0}")]
pub struct TelegramStartupUpdatePolicyParseError(String);

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TelegramConfigError {
    #[error("TELEGRAM_BOT_TOKEN or bot token config is required")]
    MissingBotToken,
    #[error("Telegram bot username must not be empty when configured")]
    InvalidBotUsername,
    #[error("NOLOONG_INTERACTION_URL or interaction URL config is required")]
    MissingInteractionUrl,
    #[error("Telegram allowlist is required unless allowAll is explicitly enabled")]
    MissingAllowlist,
    #[error("Telegram file policy is invalid: {0}")]
    InvalidFilePolicy(String),
}

fn default_message_window_ms() -> u64 {
    600
}

fn default_long_split_window_ms() -> u64 {
    2_000
}

fn default_edit_throttle_ms() -> u64 {
    750
}

fn default_max_outbound_chars() -> usize {
    3900
}

fn default_inline_max_bytes() -> usize {
    256 * 1024
}

fn default_max_download_bytes() -> usize {
    20 * 1024 * 1024
}

fn default_show_tool_status() -> bool {
    true
}

fn default_locale() -> Locale {
    Locale::detect()
}

#[cfg(test)]
mod tests {
    use super::{TelegramBridgeConfig, TelegramConfigError, TelegramStartupUpdatePolicy};
    use crate::access::TelegramAccessPolicy;

    #[test]
    fn config_validation_requires_allowlist() {
        let config = TelegramBridgeConfig {
            bot_token: "token".into(),
            bot_username: None,
            interaction_ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            interaction_bearer_token: None,
            profile_id: None,
            message_window_ms: 600,
            long_split_window_ms: 2_000,
            edit_throttle_ms: 750,
            max_outbound_chars: 3900,
            access: TelegramAccessPolicy::default(),
            network: Default::default(),
            file_policy: Default::default(),
            startup_update_policy: Default::default(),
            offset_checkpoint_path: None,
            show_tool_status: true,
            locale: noloong_agent::Locale::En,
        };

        assert_eq!(
            config.validate().unwrap_err(),
            TelegramConfigError::MissingAllowlist
        );
    }

    #[test]
    fn config_validation_allows_explicit_allow_all() {
        let mut config = TelegramBridgeConfig {
            bot_token: "token".into(),
            bot_username: None,
            interaction_ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            interaction_bearer_token: None,
            profile_id: None,
            message_window_ms: 600,
            long_split_window_ms: 2_000,
            edit_throttle_ms: 750,
            max_outbound_chars: 3900,
            access: TelegramAccessPolicy::allow_all(),
            network: Default::default(),
            file_policy: Default::default(),
            startup_update_policy: Default::default(),
            offset_checkpoint_path: None,
            show_tool_status: true,
            locale: noloong_agent::Locale::En,
        };

        assert!(config.validate().is_ok());
        config.bot_token.clear();
        assert_eq!(
            config.validate().unwrap_err(),
            TelegramConfigError::MissingBotToken
        );
    }

    #[test]
    fn startup_update_policy_parses_cli_values() {
        assert_eq!(
            "process-pending"
                .parse::<TelegramStartupUpdatePolicy>()
                .unwrap(),
            TelegramStartupUpdatePolicy::ProcessPending
        );
        assert_eq!(
            "skip_pending_without_checkpoint"
                .parse::<TelegramStartupUpdatePolicy>()
                .unwrap(),
            TelegramStartupUpdatePolicy::SkipPendingWithoutCheckpoint
        );
        assert!("unknown".parse::<TelegramStartupUpdatePolicy>().is_err());
    }
}

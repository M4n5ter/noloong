use crate::interaction::{AppInteractionEndpoint, AppInteractionStatus};
use noloong_config::Locale;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::env;
use thiserror::Error;

pub const APP_LAUNCH_OPTIONS_ENV: &str = "NOLOONG_APP_LAUNCH_OPTIONS_JSON";

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppLaunchOptions {
    #[serde(default = "default_app_version")]
    pub app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_config_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<Locale>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_endpoint: Option<AppInteractionEndpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_status: Option<AppInteractionStatus>,
}

impl Default for AppLaunchOptions {
    fn default() -> Self {
        Self {
            app_version: Self::current_app_version(),
            profile_config_path: None,
            locale: None,
            interaction_endpoint: None,
            interaction_status: None,
        }
    }
}

impl AppLaunchOptions {
    pub fn current_app_version() -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    pub fn from_env_or_default() -> Result<Self, AppError> {
        let Ok(raw) = env::var(APP_LAUNCH_OPTIONS_ENV) else {
            return Ok(Self::default());
        };
        serde_json::from_str(&raw)
            .map(Self::with_current_app_version)
            .map_err(|error| AppError::LaunchOptions(error.to_string()))
    }

    pub fn with_current_app_version(mut self) -> Self {
        if self.app_version.is_empty() {
            self.app_version = Self::current_app_version();
        }
        self
    }
}

fn default_app_version() -> String {
    AppLaunchOptions::current_app_version()
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("failed to launch app: {0}")]
    Launch(String),
    #[error("failed to read app launch options: {0}")]
    LaunchOptions(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_options_env_name_is_stable() {
        assert_eq!(APP_LAUNCH_OPTIONS_ENV, "NOLOONG_APP_LAUNCH_OPTIONS_JSON");
    }

    #[test]
    fn launch_options_parse_json_from_env_payload() {
        let options: AppLaunchOptions = serde_json::from_str(
            r#"{"appVersion":"","profileConfigPath":"/tmp/profile.jsonc","locale":"zh"}"#,
        )
        .unwrap();

        assert_eq!(
            options
                .with_current_app_version()
                .profile_config_path
                .as_deref(),
            Some("/tmp/profile.jsonc")
        );
    }
}

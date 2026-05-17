use noloong_agent::Locale;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};
use thiserror::Error;

pub const ILINK_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
pub const WEIXIN_CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeixinBridgeConfig {
    pub account_id: String,
    pub token: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_cdn_base_url")]
    pub cdn_base_url: String,
    pub interaction_ws_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_bearer_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default = "default_max_outbound_chars")]
    pub max_outbound_chars: usize,
    #[serde(default)]
    pub access: WeixinAccessPolicy,
    #[serde(default)]
    pub file_policy: WeixinFilePolicy,
    #[serde(default = "default_locale")]
    pub locale: Locale,
}

impl WeixinBridgeConfig {
    pub fn validate(&self) -> Result<(), WeixinConfigError> {
        if self.account_id.trim().is_empty() {
            return Err(WeixinConfigError::MissingAccountId);
        }
        if self.token.trim().is_empty() {
            return Err(WeixinConfigError::MissingToken);
        }
        if self.base_url.trim().is_empty() {
            return Err(WeixinConfigError::InvalidUrl(
                "baseUrl must not be empty".into(),
            ));
        }
        if self.cdn_base_url.trim().is_empty() {
            return Err(WeixinConfigError::InvalidUrl(
                "cdnBaseUrl must not be empty".into(),
            ));
        }
        if self.interaction_ws_url.trim().is_empty() {
            return Err(WeixinConfigError::MissingInteractionUrl);
        }
        if self.max_outbound_chars == 0 {
            return Err(WeixinConfigError::InvalidTextPolicy(
                "maxOutboundChars must be greater than zero".into(),
            ));
        }
        self.access.validate()?;
        self.file_policy.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeixinAccessPolicy {
    #[serde(default)]
    pub allow_all: bool,
    #[serde(default)]
    pub allowed_user_ids: BTreeSet<String>,
    #[serde(default)]
    pub group_policy: WeixinGroupPolicy,
}

impl WeixinAccessPolicy {
    pub fn allow_all() -> Self {
        Self {
            allow_all: true,
            ..Self::default()
        }
    }

    pub fn new(allowed_user_ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            allow_all: false,
            allowed_user_ids: allowed_user_ids
                .into_iter()
                .map(Into::into)
                .map(|value: String| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .collect(),
            group_policy: WeixinGroupPolicy::default(),
        }
    }

    pub fn validate(&self) -> Result<(), WeixinConfigError> {
        if self
            .allowed_user_ids
            .iter()
            .any(|user_id| user_id.trim().is_empty() || user_id.trim() != user_id)
        {
            return Err(WeixinConfigError::InvalidAccessPolicy(
                "allowedUserIds must not contain empty or whitespace-padded values".into(),
            ));
        }
        if self.allow_all || !self.allowed_user_ids.is_empty() {
            return self.group_policy.validate();
        }
        Err(WeixinConfigError::MissingAllowlist)
    }

    pub fn allows_dm(&self, user_id: &str) -> bool {
        self.allow_all || self.allowed_user_ids.contains(user_id)
    }

    pub fn allows_group(&self, group_id: &str, sender_id: &str) -> bool {
        match &self.group_policy {
            WeixinGroupPolicy::Disabled => false,
            WeixinGroupPolicy::AllowAll => {
                self.allow_all || self.allowed_user_ids.contains(sender_id)
            }
            WeixinGroupPolicy::Allowlist { group_ids } => {
                group_ids.contains(group_id)
                    && (self.allow_all || self.allowed_user_ids.contains(sender_id))
            }
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum WeixinGroupPolicy {
    #[default]
    Disabled,
    Allowlist {
        group_ids: BTreeSet<String>,
    },
    AllowAll,
}

impl WeixinGroupPolicy {
    fn validate(&self) -> Result<(), WeixinConfigError> {
        if let Self::Allowlist { group_ids } = self
            && group_ids
                .iter()
                .any(|group_id| group_id.trim().is_empty() || group_id.trim() != group_id)
        {
            return Err(WeixinConfigError::InvalidAccessPolicy(
                "groupPolicy.groupIds must not contain empty or whitespace-padded values".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeixinFilePolicy {
    #[serde(default = "default_inline_max_bytes")]
    pub inline_max_bytes: usize,
    #[serde(default = "default_max_download_bytes")]
    pub max_download_bytes: usize,
    #[serde(default = "default_max_upload_bytes")]
    pub max_upload_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<PathBuf>,
}

impl Default for WeixinFilePolicy {
    fn default() -> Self {
        Self {
            inline_max_bytes: default_inline_max_bytes(),
            max_download_bytes: default_max_download_bytes(),
            max_upload_bytes: default_max_upload_bytes(),
            download_dir: None,
        }
    }
}

impl WeixinFilePolicy {
    fn validate(&self) -> Result<(), WeixinConfigError> {
        if self.inline_max_bytes > self.max_download_bytes {
            return Err(WeixinConfigError::InvalidFilePolicy(
                "inlineMaxBytes must be less than or equal to maxDownloadBytes".into(),
            ));
        }
        if self.max_upload_bytes == 0 {
            return Err(WeixinConfigError::InvalidFilePolicy(
                "maxUploadBytes must be greater than zero".into(),
            ));
        }
        if self
            .download_dir
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err(WeixinConfigError::InvalidFilePolicy(
                "downloadDir must not be empty when configured".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum WeixinConfigError {
    #[error("WEIXIN_ACCOUNT_ID or account id config is required")]
    MissingAccountId,
    #[error("WEIXIN_TOKEN or token config is required")]
    MissingToken,
    #[error("NOLOONG_INTERACTION_URL or interaction URL config is required")]
    MissingInteractionUrl,
    #[error("Weixin allowlist is required unless allowAll is explicitly enabled")]
    MissingAllowlist,
    #[error("Weixin URL config is invalid: {0}")]
    InvalidUrl(String),
    #[error("Weixin access policy is invalid: {0}")]
    InvalidAccessPolicy(String),
    #[error("Weixin file policy is invalid: {0}")]
    InvalidFilePolicy(String),
    #[error("Weixin text policy is invalid: {0}")]
    InvalidTextPolicy(String),
}

fn default_base_url() -> String {
    ILINK_BASE_URL.into()
}

fn default_cdn_base_url() -> String {
    WEIXIN_CDN_BASE_URL.into()
}

fn default_max_outbound_chars() -> usize {
    2000
}

fn default_inline_max_bytes() -> usize {
    256 * 1024
}

fn default_max_download_bytes() -> usize {
    20 * 1024 * 1024
}

fn default_max_upload_bytes() -> usize {
    20 * 1024 * 1024
}

fn default_locale() -> Locale {
    Locale::detect()
}

#[cfg(test)]
mod tests {
    use super::{
        WeixinAccessPolicy, WeixinBridgeConfig, WeixinConfigError, WeixinFilePolicy,
        WeixinGroupPolicy,
    };

    #[test]
    fn validation_requires_allowlist() {
        let config = test_config(WeixinAccessPolicy::default());

        assert_eq!(
            config.validate().unwrap_err(),
            WeixinConfigError::MissingAllowlist
        );
    }

    #[test]
    fn validation_accepts_explicit_allow_all() {
        let mut config = test_config(WeixinAccessPolicy::allow_all());

        assert!(config.validate().is_ok());
        config.token.clear();
        assert_eq!(
            config.validate().unwrap_err(),
            WeixinConfigError::MissingToken
        );
    }

    #[test]
    fn validation_rejects_invalid_file_policy() {
        let mut config = test_config(WeixinAccessPolicy::allow_all());
        config.file_policy = WeixinFilePolicy {
            inline_max_bytes: 2,
            max_download_bytes: 1,
            max_upload_bytes: 1,
            download_dir: None,
        };

        assert!(matches!(
            config.validate().unwrap_err(),
            WeixinConfigError::InvalidFilePolicy(_)
        ));
    }

    #[test]
    fn group_allowlist_rejects_empty_values() {
        let access = WeixinAccessPolicy {
            allow_all: true,
            allowed_user_ids: Default::default(),
            group_policy: WeixinGroupPolicy::Allowlist {
                group_ids: [String::new()].into_iter().collect(),
            },
        };

        assert!(matches!(
            test_config(access).validate().unwrap_err(),
            WeixinConfigError::InvalidAccessPolicy(_)
        ));
    }

    #[test]
    fn access_allowlist_normalizes_constructor_and_rejects_padded_config() {
        let access = WeixinAccessPolicy::new([" user ", "", "other"]);

        assert!(access.allows_dm("user"));
        assert!(access.allows_dm("other"));
        assert!(!access.allows_dm(" user "));

        let access = WeixinAccessPolicy {
            allow_all: false,
            allowed_user_ids: [" user ".to_owned()].into_iter().collect(),
            group_policy: WeixinGroupPolicy::Disabled,
        };

        assert!(matches!(
            test_config(access).validate().unwrap_err(),
            WeixinConfigError::InvalidAccessPolicy(_)
        ));
    }

    fn test_config(access: WeixinAccessPolicy) -> WeixinBridgeConfig {
        WeixinBridgeConfig {
            account_id: "account".into(),
            token: "token".into(),
            base_url: "https://ilinkai.weixin.qq.com".into(),
            cdn_base_url: "https://novac2c.cdn.weixin.qq.com/c2c".into(),
            interaction_ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            interaction_bearer_token: None,
            profile_id: None,
            max_outbound_chars: 2000,
            access,
            file_policy: WeixinFilePolicy::default(),
            locale: noloong_agent::Locale::En,
        }
    }
}

use std::collections::BTreeSet;

use crate::config::TelegramConfigError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TelegramAccessPolicy {
    #[serde(default)]
    pub allow_all: bool,
    #[serde(default)]
    pub allowed_chat_ids: BTreeSet<i64>,
    #[serde(default)]
    pub allowed_user_ids: BTreeSet<u64>,
    #[serde(default = "default_require_mention_in_groups")]
    pub require_mention_in_groups: bool,
}

impl Default for TelegramAccessPolicy {
    fn default() -> Self {
        Self {
            allow_all: false,
            allowed_chat_ids: BTreeSet::new(),
            allowed_user_ids: BTreeSet::new(),
            require_mention_in_groups: default_require_mention_in_groups(),
        }
    }
}

impl TelegramAccessPolicy {
    pub fn new(
        allowed_chat_ids: impl IntoIterator<Item = i64>,
        allowed_user_ids: impl IntoIterator<Item = u64>,
    ) -> Self {
        Self {
            allow_all: false,
            allowed_chat_ids: allowed_chat_ids.into_iter().collect(),
            allowed_user_ids: allowed_user_ids.into_iter().collect(),
            require_mention_in_groups: default_require_mention_in_groups(),
        }
    }

    pub fn allow_all() -> Self {
        Self {
            allow_all: true,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), TelegramConfigError> {
        if self.allow_all || !self.is_empty() {
            return Ok(());
        }
        Err(TelegramConfigError::MissingAllowlist)
    }

    pub fn allows(&self, chat_id: i64, user_id: Option<u64>) -> bool {
        self.allow_all
            || self.allowed_chat_ids.contains(&chat_id)
            || user_id.is_some_and(|user_id| self.allowed_user_ids.contains(&user_id))
    }

    pub fn is_empty(&self) -> bool {
        self.allowed_chat_ids.is_empty() && self.allowed_user_ids.is_empty()
    }

    pub fn accepts_text(&self, input: &TelegramTextInput, bot_username: Option<&str>) -> bool {
        self.allows(input.chat_id, input.user_id)
            && (!self.requires_group_gate(input) || input.addresses_bot(bot_username))
            && !input.text.trim().is_empty()
    }

    fn requires_group_gate(&self, input: &TelegramTextInput) -> bool {
        self.require_mention_in_groups && input.chat_kind.is_group()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramTextInput {
    pub chat_id: i64,
    pub thread_id: Option<i64>,
    pub chat_kind: TelegramChatKind,
    pub user_id: Option<u64>,
    pub message_id: i64,
    pub text: String,
    pub is_reply_to_bot: bool,
}

impl TelegramTextInput {
    pub fn addresses_bot(&self, bot_username: Option<&str>) -> bool {
        self.is_reply_to_bot
            || bot_username
                .is_some_and(|username| telegram_text_mentions_username(&self.text, username))
    }

    pub fn text_without_bot_mention(&self, bot_username: Option<&str>) -> String {
        let Some(username) = bot_username else {
            return self.text.trim().to_owned();
        };
        telegram_text_without_username_mention(&self.text, username)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelegramChatKind {
    Private,
    Group,
    Supergroup,
    Channel,
    Unknown(String),
}

impl TelegramChatKind {
    pub fn parse(value: &str) -> Self {
        match value {
            "private" => Self::Private,
            "group" => Self::Group,
            "supergroup" => Self::Supergroup,
            "channel" => Self::Channel,
            other => Self::Unknown(other.into()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Private => "private",
            Self::Group => "group",
            Self::Supergroup => "supergroup",
            Self::Channel => "channel",
            Self::Unknown(value) => value,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group | Self::Supergroup)
    }
}

fn default_require_mention_in_groups() -> bool {
    true
}

pub fn telegram_username_matches(username: &str, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return false;
    };
    telegram_username_body(username).eq_ignore_ascii_case(telegram_username_body(expected))
}

pub(crate) fn telegram_text_mentions_username(text: &str, username: &str) -> bool {
    text.split_whitespace()
        .any(|word| mention_token_matches_username(word, username))
}

pub(crate) fn telegram_text_without_username_mention(text: &str, username: &str) -> String {
    text.split_whitespace()
        .filter(|word| !mention_token_matches_username(word, username))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

fn telegram_username_body(username: &str) -> &str {
    username.trim().trim_start_matches('@')
}

fn mention_token_matches_username(word: &str, username: &str) -> bool {
    let token = trim_mention_token(word);
    token.starts_with('@') && telegram_username_matches(token, Some(username))
}

fn trim_mention_token(word: &str) -> &str {
    word.trim_matches(|ch: char| ch.is_ascii_punctuation() && ch != '@' && ch != '_')
}

#[cfg(test)]
mod tests {
    use super::{TelegramAccessPolicy, TelegramChatKind, TelegramTextInput};

    #[test]
    fn access_policy_requires_allowlist_by_default() {
        assert!(TelegramAccessPolicy::default().validate().is_err());
        assert!(TelegramAccessPolicy::new([42], []).validate().is_ok());
        assert!(TelegramAccessPolicy::allow_all().validate().is_ok());
    }

    #[test]
    fn mention_gating_accepts_dm_without_mention() {
        let policy = TelegramAccessPolicy::new([], [7]);
        let input = TelegramTextInput {
            chat_id: 1,
            thread_id: None,
            chat_kind: TelegramChatKind::Private,
            user_id: Some(7),
            message_id: 1,
            text: "hello".into(),
            is_reply_to_bot: false,
        };

        assert!(policy.accepts_text(&input, Some("noloong_bot")));
    }

    #[test]
    fn mention_gating_requires_group_addressing() {
        let policy = TelegramAccessPolicy::new([42], []);
        let mut input = TelegramTextInput {
            chat_id: 42,
            thread_id: None,
            chat_kind: TelegramChatKind::Supergroup,
            user_id: Some(7),
            message_id: 1,
            text: "hello".into(),
            is_reply_to_bot: false,
        };

        assert!(!policy.accepts_text(&input, Some("noloong_bot")));
        input.text = "@noloong_bot hello".into();
        assert!(policy.accepts_text(&input, Some("noloong_bot")));
        assert_eq!(input.text_without_bot_mention(Some("noloong_bot")), "hello");
    }
}

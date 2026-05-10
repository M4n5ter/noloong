use crate::{
    callback::ShortCallbackStore,
    telegram_api::{TelegramInlineKeyboardButton, TelegramInlineKeyboardMarkup},
};
use std::collections::BTreeMap;

const SESSION_ACTION_PREFIX: &str = "sc:";
pub const TELEGRAM_METADATA_CHANNEL: &str = "channel";
pub const TELEGRAM_METADATA_CHANNEL_TELEGRAM: &str = "telegram";
pub const TELEGRAM_METADATA_CHAT_ID: &str = "chatId";
pub const TELEGRAM_METADATA_CHAT_TYPE: &str = "chatType";
pub const TELEGRAM_METADATA_THREAD_ID: &str = "threadId";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TelegramSessionMapper {
    sessions_by_chat: BTreeMap<TelegramSessionKey, String>,
}

impl TelegramSessionMapper {
    pub fn get(&self, key: &TelegramSessionKey) -> Option<&str> {
        self.sessions_by_chat.get(key).map(String::as_str)
    }

    pub fn insert(&mut self, key: TelegramSessionKey, session_id: String) {
        self.sessions_by_chat.insert(key, session_id);
    }

    pub fn session_id_for(key: TelegramSessionKey) -> String {
        key.session_id()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TelegramSessionKey {
    pub chat_id: i64,
    pub thread_id: Option<i64>,
}

impl TelegramSessionKey {
    pub fn new(chat_id: i64, thread_id: Option<i64>) -> Self {
        Self { chat_id, thread_id }
    }

    pub fn session_id(&self) -> String {
        match self.thread_id {
            Some(thread_id) => format!("telegram:{}:thread:{thread_id}", self.chat_id),
            None => format!("telegram:{}", self.chat_id),
        }
    }

    pub fn derived_session_id(&self, nonce: i64) -> String {
        format!("{}:session:{nonce}", self.session_id())
    }

    pub fn from_session_id(session_id: &str) -> Option<Self> {
        let value = session_id
            .split_once(":session:")
            .map_or(session_id, |(prefix, _)| prefix)
            .strip_prefix("telegram:")?;
        let (chat_id, thread_id) = match value.split_once(":thread:") {
            Some((chat_id, thread_id)) => (chat_id, Some(thread_id.parse().ok()?)),
            None => (value, None),
        };
        Some(Self {
            chat_id: chat_id.parse().ok()?,
            thread_id,
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct TelegramSessionActionStore {
    actions: ShortCallbackStore<TelegramSessionAction>,
}

impl TelegramSessionActionStore {
    pub fn button(
        &mut self,
        text: impl Into<String>,
        action: TelegramSessionAction,
    ) -> TelegramInlineKeyboardButton {
        let key = self.actions.insert(action);
        TelegramInlineKeyboardButton {
            text: text.into(),
            callback_data: format!("{SESSION_ACTION_PREFIX}{key}"),
        }
    }

    pub fn resolve(&mut self, data: &str) -> Option<TelegramSessionAction> {
        let key = data.strip_prefix(SESSION_ACTION_PREFIX)?;
        self.actions.remove(key)
    }

    pub fn is_session_action(data: &str) -> bool {
        data.starts_with(SESSION_ACTION_PREFIX)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramSessionAction {
    SelectProfile {
        profile_id: String,
    },
    SwitchSession {
        session_id: String,
    },
    RequestDelete {
        session_id: String,
    },
    ConfirmDelete {
        session_id: String,
        force_abort: bool,
    },
}

pub fn single_button_markup(button: TelegramInlineKeyboardButton) -> TelegramInlineKeyboardMarkup {
    TelegramInlineKeyboardMarkup {
        inline_keyboard: vec![vec![button]],
    }
}

pub fn telegram_session_metadata(
    chat_id: i64,
    thread_id: Option<i64>,
    chat_kind: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        TELEGRAM_METADATA_CHANNEL.into(),
        serde_json::Value::String(TELEGRAM_METADATA_CHANNEL_TELEGRAM.into()),
    );
    metadata.insert(TELEGRAM_METADATA_CHAT_ID.into(), serde_json::json!(chat_id));
    metadata.insert(
        TELEGRAM_METADATA_CHAT_TYPE.into(),
        serde_json::Value::String(chat_kind.into()),
    );
    if let Some(thread_id) = thread_id {
        metadata.insert(
            TELEGRAM_METADATA_THREAD_ID.into(),
            serde_json::json!(thread_id),
        );
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::{
        TelegramSessionAction, TelegramSessionActionStore, TelegramSessionKey,
        TelegramSessionMapper, telegram_session_metadata,
    };

    #[test]
    fn session_mapping_uses_stable_chat_and_thread_ids() {
        assert_eq!(
            TelegramSessionMapper::session_id_for(TelegramSessionKey::new(42, None)),
            "telegram:42"
        );
        assert_eq!(
            TelegramSessionMapper::session_id_for(TelegramSessionKey::new(42, Some(9))),
            "telegram:42:thread:9"
        );
    }

    #[test]
    fn session_key_parses_stable_session_ids() {
        assert_eq!(
            TelegramSessionKey::from_session_id("telegram:-100:thread:3"),
            Some(TelegramSessionKey::new(-100, Some(3)))
        );
        assert_eq!(
            TelegramSessionKey::from_session_id("telegram:42"),
            Some(TelegramSessionKey::new(42, None))
        );
        assert_eq!(
            TelegramSessionKey::from_session_id("telegram:42:session:9"),
            Some(TelegramSessionKey::new(42, None))
        );
        assert_eq!(
            TelegramSessionKey::from_session_id("telegram:-100:thread:3:session:9"),
            Some(TelegramSessionKey::new(-100, Some(3)))
        );
        assert_eq!(TelegramSessionKey::from_session_id("other:42"), None);
    }

    #[test]
    fn session_metadata_records_telegram_origin() {
        let metadata = telegram_session_metadata(-100, Some(3), "supergroup");
        assert_eq!(metadata["channel"], "telegram");
        assert_eq!(metadata["chatId"], -100);
        assert_eq!(metadata["threadId"], 3);
        assert_eq!(metadata["chatType"], "supergroup");
    }

    #[test]
    fn session_action_store_allocates_short_single_use_callbacks() {
        let mut store = TelegramSessionActionStore::default();
        let action = TelegramSessionAction::SwitchSession {
            session_id: "telegram:42".into(),
        };
        let button = store.button("Switch", action.clone());

        assert!(button.callback_data.len() <= 64);
        assert!(TelegramSessionActionStore::is_session_action(
            &button.callback_data
        ));
        assert_eq!(store.resolve(&button.callback_data), Some(action));
        assert_eq!(store.resolve(&button.callback_data), None);
    }
}

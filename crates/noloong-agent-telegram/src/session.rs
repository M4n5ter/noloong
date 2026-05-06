use std::collections::BTreeMap;

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

    pub fn from_session_id(session_id: &str) -> Option<Self> {
        let value = session_id.strip_prefix("telegram:")?;
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

pub fn telegram_session_metadata(
    chat_id: i64,
    thread_id: Option<i64>,
    chat_kind: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "channel".into(),
        serde_json::Value::String("telegram".into()),
    );
    metadata.insert("chatId".into(), serde_json::json!(chat_id));
    metadata.insert(
        "chatType".into(),
        serde_json::Value::String(chat_kind.into()),
    );
    if let Some(thread_id) = thread_id {
        metadata.insert("threadId".into(), serde_json::json!(thread_id));
    }
    metadata
}

#[cfg(test)]
mod tests {
    use super::{TelegramSessionKey, TelegramSessionMapper, telegram_session_metadata};

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
}

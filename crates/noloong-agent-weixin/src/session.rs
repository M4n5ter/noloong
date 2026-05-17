use serde_json::{Value, json};

pub const WEIXIN_METADATA_CHANNEL: &str = "channel";
pub const WEIXIN_METADATA_CHANNEL_WEIXIN: &str = "weixin";
pub const WEIXIN_METADATA_PEER_ID: &str = "peerId";
pub const WEIXIN_METADATA_CHAT_KIND: &str = "chatKind";
pub const WEIXIN_METADATA_ACCOUNT_ID: &str = "accountId";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WeixinSessionKey {
    pub account_id: String,
    pub peer_id: String,
    pub chat_kind: WeixinChatKind,
}

impl WeixinSessionKey {
    pub fn new(
        account_id: impl Into<String>,
        peer_id: impl Into<String>,
        chat_kind: WeixinChatKind,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            peer_id: peer_id.into(),
            chat_kind,
        }
    }

    pub fn session_id(&self) -> String {
        match self.chat_kind {
            WeixinChatKind::Dm => format!(
                "weixin:{}:{}",
                stable_component(&self.account_id),
                stable_component(&self.peer_id)
            ),
            WeixinChatKind::Group => format!(
                "weixin:{}:group:{}",
                stable_component(&self.account_id),
                stable_component(&self.peer_id)
            ),
        }
    }

    pub fn derived_session_id(&self, nonce: u64) -> String {
        format!("{}:session:{nonce}", self.session_id())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WeixinChatKind {
    Dm,
    Group,
}

impl WeixinChatKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dm => "dm",
            Self::Group => "group",
        }
    }
}

pub fn weixin_session_metadata(
    account_id: &str,
    peer_id: &str,
    chat_kind: WeixinChatKind,
) -> serde_json::Map<String, Value> {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        WEIXIN_METADATA_CHANNEL.into(),
        Value::String(WEIXIN_METADATA_CHANNEL_WEIXIN.into()),
    );
    metadata.insert(WEIXIN_METADATA_ACCOUNT_ID.into(), json!(account_id));
    metadata.insert(WEIXIN_METADATA_PEER_ID.into(), json!(peer_id));
    metadata.insert(WEIXIN_METADATA_CHAT_KIND.into(), json!(chat_kind.as_str()));
    metadata
}

fn stable_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{WeixinChatKind, WeixinSessionKey};

    #[test]
    fn session_id_is_stable_for_dm_and_group() {
        assert_eq!(
            WeixinSessionKey::new("bot", "user", WeixinChatKind::Dm).session_id(),
            "weixin:bot:user"
        );
        assert_eq!(
            WeixinSessionKey::new("bot", "room", WeixinChatKind::Group).session_id(),
            "weixin:bot:group:room"
        );
    }
}

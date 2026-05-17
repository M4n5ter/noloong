use crate::{
    config::WeixinAccessPolicy,
    ilink_api::{
        ITEM_FILE, ITEM_IMAGE, ITEM_TEXT, ITEM_VIDEO, ITEM_VOICE, MSG_TYPE_USER, WeixinMessage,
        WeixinMessageItem, WeixinRefMessage,
    },
    session::WeixinChatKind,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WeixinInboundUpdate {
    Message(WeixinInboundMessage),
    Command(WeixinCommand),
    Ignored,
}

impl WeixinInboundUpdate {
    pub fn from_message(
        account_id: &str,
        message: WeixinMessage,
        access: &WeixinAccessPolicy,
    ) -> Self {
        let Some(context) = WeixinInboundContext::from_message(account_id, &message) else {
            return Self::Ignored;
        };
        if !context.is_allowed(access) {
            return Self::Ignored;
        }
        let text = extract_text(&message.item_list);
        let command = extract_command_text(&message.item_list)
            .as_deref()
            .and_then(|text| {
                WeixinCommand::parse(context.clone(), text, message.item_list.clone())
            });
        if let Some(command) = command {
            return Self::Command(command);
        }
        if text.is_none() && !has_media(&message.item_list) {
            return Self::Ignored;
        }
        Self::Message(WeixinInboundMessage {
            context,
            text,
            items: message.item_list,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinInboundMessage {
    pub context: WeixinInboundContext,
    pub text: Option<String>,
    pub items: Vec<WeixinMessageItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinCommand {
    pub context: WeixinInboundContext,
    pub kind: WeixinCommandKind,
    pub selector: Option<usize>,
    pub args: String,
    pub raw_text: String,
    pub items: Vec<WeixinMessageItem>,
}

impl WeixinCommand {
    fn parse(
        context: WeixinInboundContext,
        text: &str,
        items: Vec<WeixinMessageItem>,
    ) -> Option<Self> {
        let trimmed = text.trim();
        let normalized = trimmed
            .strip_prefix('/')
            .or_else(|| trimmed.strip_prefix('／'))
            .map(str::trim)?;
        let (head, rest) = normalized
            .split_once(char::is_whitespace)
            .unwrap_or((normalized, ""));
        let kind = match head {
            "help" | "帮助" => WeixinCommandKind::Help,
            "status" | "状态" => WeixinCommandKind::Status,
            "new" | "新会话" | "新建" => WeixinCommandKind::New,
            "sessions" | "会话" | "会话列表" => WeixinCommandKind::Sessions,
            "switch" | "切换" => WeixinCommandKind::Switch,
            "delete" | "删除" => WeixinCommandKind::Delete,
            "approvals" | "审批" => WeixinCommandKind::Approvals,
            "approve" | "同意" | "批准" => WeixinCommandKind::Approve,
            "deny" | "拒绝" => WeixinCommandKind::Deny,
            "config" | "manifest" | "运行配置" | "配置" => WeixinCommandKind::RunConfig,
            "queue" | "队列" => WeixinCommandKind::Queue,
            "clearqueue" | "clear_queue" | "清空队列" => WeixinCommandKind::ClearQueue,
            "processes" | "进程列表" => WeixinCommandKind::Processes,
            "process" | "进程" => WeixinCommandKind::Process,
            "subagent" | "子任务" => WeixinCommandKind::Subagent,
            _ => return None,
        };
        let rest = rest.trim();
        let selector = rest
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<usize>().ok());
        Some(Self {
            context,
            kind,
            selector,
            args: rest.into(),
            raw_text: trimmed.into(),
            items,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeixinCommandKind {
    Help,
    Status,
    New,
    Sessions,
    Switch,
    Delete,
    Approvals,
    Approve,
    Deny,
    RunConfig,
    Queue,
    ClearQueue,
    Processes,
    Process,
    Subagent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinInboundContext {
    pub account_id: String,
    pub peer_id: String,
    pub sender_id: String,
    pub message_id: String,
    pub chat_kind: WeixinChatKind,
    pub context_token: Option<String>,
    pub reply_to: Option<WeixinReplyContext>,
}

impl WeixinInboundContext {
    pub fn from_message(account_id: &str, message: &WeixinMessage) -> Option<Self> {
        let sender_id = message.from_user_id.as_deref()?.trim();
        if sender_id.is_empty() || sender_id == account_id {
            return None;
        }
        if message
            .msg_type
            .is_some_and(|msg_type| msg_type != MSG_TYPE_USER)
        {
            return None;
        }
        let (chat_kind, peer_id) = guess_chat_kind_and_peer(message, account_id);
        if peer_id.trim().is_empty() {
            return None;
        }
        Some(Self {
            account_id: account_id.into(),
            peer_id,
            sender_id: sender_id.into(),
            message_id: message
                .message_id
                .clone()
                .unwrap_or_else(|| content_fingerprint(message)),
            chat_kind,
            context_token: message
                .context_token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
            reply_to: message
                .item_list
                .iter()
                .filter_map(|item| item.ref_msg.as_ref())
                .find_map(WeixinReplyContext::from_ref_message),
        })
    }

    fn is_allowed(&self, access: &WeixinAccessPolicy) -> bool {
        match self.chat_kind {
            WeixinChatKind::Dm => access.allows_dm(&self.sender_id),
            WeixinChatKind::Group => access.allows_group(&self.peer_id, &self.sender_id),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeixinReplyContext {
    pub title: Option<String>,
    pub text_preview: Option<String>,
    pub media_kinds: Vec<WeixinReplyMediaKind>,
}

impl WeixinReplyContext {
    fn from_ref_message(ref_message: &WeixinRefMessage) -> Option<Self> {
        let item = ref_message.message_item.as_deref()?;
        Some(Self {
            title: ref_message.title.clone(),
            text_preview: extract_text(std::slice::from_ref(item)),
            media_kinds: media_kinds(std::slice::from_ref(item)),
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WeixinReplyMediaKind {
    Image,
    Voice,
    File,
    Video,
}

pub fn extract_text(items: &[WeixinMessageItem]) -> Option<String> {
    for item in items {
        if item.kind != ITEM_TEXT {
            continue;
        }
        let text = item
            .text_item
            .as_ref()
            .map(|item| item.text.trim().to_owned())
            .unwrap_or_default();
        if let Some(ref_message) = &item.ref_msg
            && let Some(ref_item) = ref_message.message_item.as_deref()
        {
            let prefix = if has_media(std::slice::from_ref(ref_item)) {
                ref_message.title.as_ref().map_or_else(
                    || "[引用媒体]".into(),
                    |title| format!("[引用媒体: {title}]"),
                )
            } else {
                let ref_text = extract_text(std::slice::from_ref(ref_item)).unwrap_or_default();
                if ref_text.is_empty() {
                    "[引用]".into()
                } else {
                    format!("[引用: {ref_text}]")
                }
            };
            return Some(format!("{prefix}\n{text}").trim().to_owned());
        }
        return (!text.is_empty()).then_some(text);
    }
    for item in items {
        if item.kind == ITEM_VOICE
            && let Some(text) = item
                .voice_item
                .as_ref()
                .and_then(|voice| voice.text.as_deref())
                .map(str::trim)
                .filter(|text| !text.is_empty())
        {
            return Some(text.to_owned());
        }
    }
    None
}

pub fn extract_command_text(items: &[WeixinMessageItem]) -> Option<String> {
    items.iter().find_map(|item| {
        if item.kind != ITEM_TEXT {
            return None;
        }
        item.text_item
            .as_ref()
            .map(|item| item.text.trim())
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    })
}

pub fn has_media(items: &[WeixinMessageItem]) -> bool {
    items
        .iter()
        .any(|item| matches!(item.kind, ITEM_IMAGE | ITEM_FILE | ITEM_VIDEO | ITEM_VOICE))
}

pub fn media_kinds(items: &[WeixinMessageItem]) -> Vec<WeixinReplyMediaKind> {
    items
        .iter()
        .filter_map(|item| match item.kind {
            ITEM_IMAGE => Some(WeixinReplyMediaKind::Image),
            ITEM_FILE => Some(WeixinReplyMediaKind::File),
            ITEM_VIDEO => Some(WeixinReplyMediaKind::Video),
            ITEM_VOICE => Some(WeixinReplyMediaKind::Voice),
            _ => None,
        })
        .collect()
}

fn guess_chat_kind_and_peer(message: &WeixinMessage, account_id: &str) -> (WeixinChatKind, String) {
    let room_id = message
        .room_id
        .as_deref()
        .or(message.chat_room_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(room_id) = room_id {
        return (WeixinChatKind::Group, room_id.into());
    }
    let to_user_id = message.to_user_id.as_deref().unwrap_or_default().trim();
    if !to_user_id.is_empty() && to_user_id != account_id && message.msg_type == Some(MSG_TYPE_USER)
    {
        return (WeixinChatKind::Group, to_user_id.into());
    }
    (
        WeixinChatKind::Dm,
        message.from_user_id.clone().unwrap_or_default(),
    )
}

pub(crate) fn content_fingerprint(message: &WeixinMessage) -> String {
    let text = extract_text(&message.item_list).unwrap_or_default();
    format!(
        "content:{}:{}",
        message.from_user_id.as_deref().unwrap_or_default(),
        md5_hex(text.as_bytes())
    )
}

fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{WeixinCommandKind, WeixinInboundUpdate, extract_text};
    use crate::{
        config::WeixinAccessPolicy,
        ilink_api::{
            ITEM_TEXT, WeixinMessage, WeixinMessageItem, WeixinRefMessage, WeixinTextItem,
        },
    };

    #[test]
    fn text_item_extracts_text() {
        let item = WeixinMessageItem {
            kind: ITEM_TEXT,
            text_item: Some(WeixinTextItem {
                text: " hello ".into(),
            }),
            ..Default::default()
        };

        assert_eq!(extract_text(&[item]).as_deref(), Some("hello"));
    }

    #[test]
    fn command_parser_requires_prefix_for_chinese_numbered_commands() {
        let update = WeixinInboundUpdate::from_message(
            "bot",
            WeixinMessage {
                message_id: Some("m1".into()),
                from_user_id: Some("u1".into()),
                msg_type: Some(1),
                item_list: vec![WeixinMessageItem {
                    kind: ITEM_TEXT,
                    text_item: Some(WeixinTextItem {
                        text: "/同意 2".into(),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
            &WeixinAccessPolicy::new(["u1"]),
        );

        let WeixinInboundUpdate::Command(command) = update else {
            panic!("expected command");
        };
        assert_eq!(command.kind, WeixinCommandKind::Approve);
        assert_eq!(command.selector, Some(2));

        let plain = WeixinInboundUpdate::from_message(
            "bot",
            WeixinMessage {
                message_id: Some("m2".into()),
                from_user_id: Some("u1".into()),
                msg_type: Some(1),
                item_list: vec![WeixinMessageItem {
                    kind: ITEM_TEXT,
                    text_item: Some(WeixinTextItem {
                        text: "状态".into(),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
            &WeixinAccessPolicy::new(["u1"]),
        );

        assert!(matches!(plain, WeixinInboundUpdate::Message(_)));
    }

    #[test]
    fn command_parser_ignores_reply_context_prefix() {
        let update = WeixinInboundUpdate::from_message(
            "bot",
            WeixinMessage {
                message_id: Some("m1".into()),
                from_user_id: Some("u1".into()),
                msg_type: Some(1),
                item_list: vec![WeixinMessageItem {
                    kind: ITEM_TEXT,
                    text_item: Some(WeixinTextItem {
                        text: "/状态".into(),
                    }),
                    ref_msg: Some(WeixinRefMessage {
                        title: None,
                        message_item: Some(Box::new(WeixinMessageItem {
                            kind: ITEM_TEXT,
                            text_item: Some(WeixinTextItem {
                                text: "old message".into(),
                            }),
                            ..Default::default()
                        })),
                        extra: Default::default(),
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
            &WeixinAccessPolicy::new(["u1"]),
        );

        let WeixinInboundUpdate::Command(command) = update else {
            panic!("expected command");
        };
        assert_eq!(command.kind, WeixinCommandKind::Status);
        assert_eq!(command.raw_text, "/状态");
    }
}

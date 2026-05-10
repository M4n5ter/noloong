use noloong_agent::interaction::{AgentSessionQueuedMessage, AgentSessionQueuedMessageIntent};
use noloong_agent_core::{ContentBlock, MediaKind};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

const QUEUE_MESSAGE_RENDER_LIMIT: usize = 120;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TelegramQueueKind {
    Steering,
    FollowUp,
}

impl TelegramQueueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Steering => "steering",
            Self::FollowUp => "follow_up",
        }
    }
}

pub type TelegramQueuedMessage = AgentSessionQueuedMessage;
pub type TelegramQueuedMessageIntent = AgentSessionQueuedMessageIntent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramQueueSnapshot {
    pub steering: Vec<TelegramQueuedMessage>,
    pub follow_up: Vec<TelegramQueuedMessage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelegramQueueSummaryLabels<'a> {
    pub non_text_message: &'a str,
    pub json: &'a str,
    pub file: &'a str,
    pub image: &'a str,
    pub audio: &'a str,
    pub video: &'a str,
}

pub fn summarize_queued_message(
    message: &TelegramQueuedMessage,
    labels: TelegramQueueSummaryLabels<'_>,
) -> String {
    let mut summary = String::new();
    let mut truncated = false;
    for block in &message.message.content {
        let Some(part) = content_summary(block, labels) else {
            continue;
        };
        if !summary.is_empty() && append_with_limit(&mut summary, " ", QUEUE_MESSAGE_RENDER_LIMIT) {
            truncated = true;
            break;
        }
        if append_with_limit(&mut summary, &part, QUEUE_MESSAGE_RENDER_LIMIT) {
            truncated = true;
            break;
        }
    }
    if summary.is_empty() {
        summary.push_str(labels.non_text_message);
    }
    if truncated {
        truncate_to_chars(&mut summary, QUEUE_MESSAGE_RENDER_LIMIT.saturating_sub(3));
        summary.push_str("...");
    }
    summary
}

fn content_summary<'a>(
    block: &'a ContentBlock,
    labels: TelegramQueueSummaryLabels<'a>,
) -> Option<Cow<'a, str>> {
    match block {
        ContentBlock::Text { text } => {
            let text = text.trim();
            (!text.is_empty()).then_some(Cow::Borrowed(text))
        }
        ContentBlock::Json { .. } => Some(Cow::Borrowed(labels.json)),
        ContentBlock::Media { media } => Some(Cow::Owned(format!(
            "[{}]",
            media_kind_label(&media.kind, labels)
        ))),
        ContentBlock::Thinking { .. }
        | ContentBlock::ToolCall { .. }
        | ContentBlock::ToolResult { .. }
        | ContentBlock::ProviderPayload { .. } => None,
    }
}

fn media_kind_label<'a>(
    kind: &'a MediaKind,
    labels: TelegramQueueSummaryLabels<'a>,
) -> Cow<'a, str> {
    match kind {
        MediaKind::File => Cow::Borrowed(labels.file),
        MediaKind::Image => Cow::Borrowed(labels.image),
        MediaKind::Audio => Cow::Borrowed(labels.audio),
        MediaKind::Video => Cow::Borrowed(labels.video),
        MediaKind::Custom(kind) => Cow::Borrowed(kind),
    }
}

fn append_with_limit(target: &mut String, text: &str, max_chars: usize) -> bool {
    let mut remaining = max_chars.saturating_sub(target.chars().count());
    for ch in text.chars() {
        if remaining == 0 {
            return true;
        }
        target.push(ch);
        remaining -= 1;
    }
    false
}

fn truncate_to_chars(target: &mut String, max_chars: usize) {
    let Some((index, _)) = target.char_indices().nth(max_chars) else {
        return;
    };
    target.truncate(index);
}

#[cfg(test)]
mod tests {
    use super::{
        TelegramQueueSummaryLabels, TelegramQueuedMessage, TelegramQueuedMessageIntent,
        summarize_queued_message,
    };
    use noloong_agent_core::AgentMessage;

    #[test]
    fn queued_message_summary_truncates_text() {
        let message = TelegramQueuedMessage {
            message: AgentMessage::user("queued-1", "a".repeat(200)),
            intent: TelegramQueuedMessageIntent::UserInput,
        };

        let summary = summarize_queued_message(
            &message,
            TelegramQueueSummaryLabels {
                non_text_message: "[non-text]",
                json: "[json]",
                file: "file",
                image: "image",
                audio: "audio",
                video: "video",
            },
        );

        assert_eq!(summary.chars().count(), 120);
        assert!(summary.ends_with("..."));
    }
}

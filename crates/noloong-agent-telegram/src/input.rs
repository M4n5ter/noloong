use crate::{
    access::TelegramTextInput,
    text::{DEFAULT_TELEGRAM_TEXT_LIMIT_UTF16_UNITS, telegram_utf16_units},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramTextBatcherConfig {
    pub message_window_ms: u64,
    pub long_split_window_ms: u64,
}

impl Default for TelegramTextBatcherConfig {
    fn default() -> Self {
        Self {
            message_window_ms: 600,
            long_split_window_ms: 2_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramTextBatch {
    pub input: TelegramTextInput,
    pub ready_at_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TelegramTextBatcher {
    config: TelegramTextBatcherConfig,
    batches: BTreeMap<TelegramTextBatchKey, TelegramTextBatch>,
}

impl TelegramTextBatcher {
    pub fn new(config: TelegramTextBatcherConfig) -> Self {
        Self {
            config,
            batches: BTreeMap::new(),
        }
    }

    pub fn push(&mut self, input: TelegramTextInput, now_ms: u64) {
        let key = TelegramTextBatchKey::from_input(&input);
        let delay_ms =
            if telegram_utf16_units(&input.text) >= DEFAULT_TELEGRAM_TEXT_LIMIT_UTF16_UNITS {
                self.config.long_split_window_ms
            } else {
                self.config.message_window_ms
            };
        let ready_at_ms = now_ms.saturating_add(delay_ms);
        self.batches
            .entry(key)
            .and_modify(|batch| {
                batch.input.message_id = input.message_id;
                batch.input.text = join_text(&batch.input.text, &input.text);
                batch.ready_at_ms = ready_at_ms;
            })
            .or_insert(TelegramTextBatch { input, ready_at_ms });
    }

    pub fn ready_batches(&mut self, now_ms: u64) -> Vec<TelegramTextBatch> {
        let ready_keys = self
            .batches
            .iter()
            .filter_map(|(key, batch)| (batch.ready_at_ms <= now_ms).then_some(*key))
            .collect::<Vec<_>>();
        ready_keys
            .into_iter()
            .filter_map(|key| self.batches.remove(&key))
            .collect()
    }

    pub fn flush_all(&mut self) -> Vec<TelegramTextBatch> {
        std::mem::take(&mut self.batches).into_values().collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TelegramTextBatchKey {
    chat_id: i64,
    thread_id: Option<i64>,
    user_id: Option<u64>,
}

impl TelegramTextBatchKey {
    fn from_input(input: &TelegramTextInput) -> Self {
        Self {
            chat_id: input.chat_id,
            thread_id: input.thread_id,
            user_id: input.user_id,
        }
    }
}

fn join_text(existing: &str, next: &str) -> String {
    if existing.is_empty() {
        return next.into();
    }
    if next.is_empty() {
        return existing.into();
    }
    format!("{existing}\n{next}")
}

#[cfg(test)]
mod tests {
    use super::{TelegramTextBatcher, TelegramTextBatcherConfig};
    use crate::access::{TelegramChatKind, TelegramTextInput};

    #[test]
    fn text_batching_combines_continuous_messages() {
        let mut batcher = TelegramTextBatcher::new(TelegramTextBatcherConfig::default());
        batcher.push(input("hello", 1), 0);
        batcher.push(input("world", 2), 200);

        assert!(batcher.ready_batches(799).is_empty());
        let ready = batcher.ready_batches(800);

        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].input.text, "hello\nworld");
        assert_eq!(ready[0].input.message_id, 2);
    }

    #[test]
    fn split_threshold_batching_waits_longer() {
        let mut batcher = TelegramTextBatcher::new(TelegramTextBatcherConfig::default());
        batcher.push(input(&"x".repeat(3_900), 1), 0);

        assert!(batcher.ready_batches(1_999).is_empty());
        assert_eq!(batcher.ready_batches(2_000).len(), 1);
    }

    fn input(text: &str, message_id: i64) -> TelegramTextInput {
        TelegramTextInput {
            chat_id: 42,
            thread_id: None,
            chat_kind: TelegramChatKind::Private,
            user_id: Some(7),
            message_id,
            text: text.into(),
            is_reply_to_bot: false,
        }
    }
}

use crate::{
    render::{render_agent_message_text, render_markdown_v2},
    telegram_api::{
        TelegramApi, TelegramApiError, TelegramEditMessageTextRequest,
        TelegramInlineKeyboardMarkup, TelegramMessageHandle, TelegramParseMode,
        TelegramSendMessageRequest,
    },
    text::split_telegram_text,
};
use noloong_agent_core::AgentMessage;
use std::sync::Arc;
use thiserror::Error;

#[derive(Clone)]
pub struct TelegramDelivery {
    api: Arc<dyn TelegramApi>,
    max_message_units: usize,
}

impl TelegramDelivery {
    pub fn new(api: Arc<dyn TelegramApi>, max_message_units: usize) -> Self {
        Self {
            api,
            max_message_units,
        }
    }

    pub async fn send_agent_message(
        &self,
        chat_id: i64,
        message: &AgentMessage,
    ) -> TelegramDeliveryResult<Vec<TelegramMessageHandle>> {
        self.send_text(chat_id, &render_agent_message_text(message), None)
            .await
    }

    pub async fn send_text(
        &self,
        chat_id: i64,
        text: &str,
        reply_markup: Option<TelegramInlineKeyboardMarkup>,
    ) -> TelegramDeliveryResult<Vec<TelegramMessageHandle>> {
        let chunks = split_telegram_text(text, self.max_message_units);
        let mut sent = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            sent.push(
                self.send_one_text(chat_id, &chunk, reply_markup.clone())
                    .await?,
            );
        }
        Ok(sent)
    }

    pub async fn edit_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        reply_markup: Option<TelegramInlineKeyboardMarkup>,
    ) -> TelegramDeliveryResult<TelegramMessageHandle> {
        let rendered = render_markdown_v2(text);
        match self
            .api
            .edit_message_text(TelegramEditMessageTextRequest {
                chat_id,
                message_id,
                text: rendered,
                parse_mode: Some(TelegramParseMode::MarkdownV2),
                reply_markup: reply_markup.clone(),
            })
            .await
        {
            Ok(message) => Ok(message),
            Err(error) if error.is_message_not_modified() => Ok(TelegramMessageHandle {
                chat_id,
                message_id,
            }),
            Err(error) if error.is_parse_error() => self
                .api
                .edit_message_text(TelegramEditMessageTextRequest {
                    chat_id,
                    message_id,
                    text: text.into(),
                    parse_mode: None,
                    reply_markup,
                })
                .await
                .map_err(TelegramDeliveryError::Api),
            Err(error) => Err(TelegramDeliveryError::Api(error)),
        }
    }

    async fn send_one_text(
        &self,
        chat_id: i64,
        text: &str,
        reply_markup: Option<TelegramInlineKeyboardMarkup>,
    ) -> TelegramDeliveryResult<TelegramMessageHandle> {
        let rendered = render_markdown_v2(text);
        match self
            .api
            .send_message(TelegramSendMessageRequest {
                chat_id,
                text: rendered,
                parse_mode: Some(TelegramParseMode::MarkdownV2),
                reply_markup: reply_markup.clone(),
            })
            .await
        {
            Ok(message) => Ok(message),
            Err(error) if error.is_parse_error() => self
                .api
                .send_message(TelegramSendMessageRequest {
                    chat_id,
                    text: text.into(),
                    parse_mode: None,
                    reply_markup,
                })
                .await
                .map_err(TelegramDeliveryError::Api),
            Err(error) => Err(TelegramDeliveryError::Api(error)),
        }
    }
}

pub type TelegramDeliveryResult<T> = Result<T, TelegramDeliveryError>;

#[derive(Debug, Error)]
pub enum TelegramDeliveryError {
    #[error("telegram api request failed: {0}")]
    Api(#[from] TelegramApiError),
}

#[cfg(test)]
mod tests {
    use super::TelegramDelivery;
    use crate::{
        telegram_api::{
            TelegramApi, TelegramApiError, TelegramEditMessageTextRequest, TelegramMessageHandle,
            TelegramSendMessageRequest, TelegramUpdate,
        },
        text::split_telegram_text,
    };
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };

    #[test]
    fn split_keeps_head_and_tail_chunks() {
        let chunks = split_telegram_text("abc\ndef", 4);
        assert_eq!(chunks, vec!["abc\n", "def"]);
    }

    #[test]
    fn split_long_line_on_char_boundary() {
        let chunks = split_telegram_text("a你b好c", 3);
        assert_eq!(chunks, vec!["a你b", "好c"]);
    }

    #[tokio::test]
    async fn telegram_text_splitting_uses_utf16_limit() {
        let chunks = split_telegram_text("a😀b", 3);
        assert_eq!(chunks, vec!["a😀", "b"]);
    }

    #[tokio::test]
    async fn send_edit_fallbacks_retry_plain_text_after_parse_error() {
        let api = Arc::new(FakeTelegramApi::parse_error_once());
        let delivery = TelegramDelivery::new(api.clone(), 3900);

        let sent = delivery.send_text(42, "*hello*", None).await.unwrap();

        assert_eq!(sent[0].message_id, 1);
        let calls = api
            .sent_calls
            .lock()
            .expect("fake sent calls lock poisoned")
            .clone();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].parse_mode.is_some());
        assert!(calls[1].parse_mode.is_none());
    }

    #[tokio::test]
    async fn send_edit_fallbacks_treat_not_modified_as_success() {
        let api = Arc::new(FakeTelegramApi::message_not_modified());
        let delivery = TelegramDelivery::new(api, 3900);

        let edited = delivery.edit_text(42, 9, "same", None).await.unwrap();

        assert_eq!(edited.message_id, 9);
    }

    struct FakeTelegramApi {
        sent_calls: Mutex<Vec<TelegramSendMessageRequest>>,
        edited_calls: Mutex<Vec<TelegramEditMessageTextRequest>>,
        mode: FakeMode,
    }

    impl FakeTelegramApi {
        fn parse_error_once() -> Self {
            Self {
                sent_calls: Mutex::new(Vec::new()),
                edited_calls: Mutex::new(Vec::new()),
                mode: FakeMode::ParseErrorOnce,
            }
        }

        fn message_not_modified() -> Self {
            Self {
                sent_calls: Mutex::new(Vec::new()),
                edited_calls: Mutex::new(Vec::new()),
                mode: FakeMode::MessageNotModified,
            }
        }
    }

    enum FakeMode {
        ParseErrorOnce,
        MessageNotModified,
    }

    impl TelegramApi for FakeTelegramApi {
        fn get_updates<'a>(
            &'a self,
            _offset: Option<i64>,
            _timeout_seconds: u64,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<TelegramUpdate>, TelegramApiError>> + Send + 'a>>
        {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn send_message<'a>(
            &'a self,
            request: TelegramSendMessageRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                let mut calls = self
                    .sent_calls
                    .lock()
                    .expect("fake sent calls lock poisoned");
                calls.push(request.clone());
                if matches!(self.mode, FakeMode::ParseErrorOnce) && calls.len() == 1 {
                    return Err(TelegramApiError::Api {
                        code: 400,
                        description: "can't parse entities".into(),
                    });
                }
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id: 1,
                })
            })
        }

        fn edit_message_text<'a>(
            &'a self,
            request: TelegramEditMessageTextRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.edited_calls
                    .lock()
                    .expect("fake edited calls lock poisoned")
                    .push(request.clone());
                if matches!(self.mode, FakeMode::MessageNotModified) {
                    return Err(TelegramApiError::Api {
                        code: 400,
                        description: "message is not modified".into(),
                    });
                }
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id: request.message_id,
                })
            })
        }

        fn answer_callback_query<'a>(
            &'a self,
            _callback_query_id: &'a str,
            _text: Option<&'a str>,
        ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
    }
}

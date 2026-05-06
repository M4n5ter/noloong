use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, sync::Arc};

use crate::telegram_api::{TelegramApi, TelegramApiError};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramUpdate {
    pub update_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<TelegramMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramMessage {
    pub message_id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_thread_id: Option<i64>,
    pub chat: TelegramChat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<TelegramUser>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_message: Option<Box<TelegramMessage>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramChat {
    pub id: i64,
    #[serde(default, rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramUser {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TelegramCallbackQuery {
    pub id: String,
    pub from: TelegramUser,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<TelegramMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelegramApiResponse<T> {
    pub ok: bool,
    pub result: T,
}

pub type TelegramUpdateHandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), TelegramPollingError>> + Send + 'a>>;

pub trait TelegramUpdateHandler: Send + Sync {
    fn handle_update<'a>(&'a self, update: TelegramUpdate) -> TelegramUpdateHandlerFuture<'a>;
}

#[derive(Clone)]
pub struct TelegramPoller {
    api: Arc<dyn TelegramApi>,
    handler: Arc<dyn TelegramUpdateHandler>,
    offset: Option<i64>,
    conflict_retries: u8,
    network_retries: u8,
    timeout_seconds: u64,
}

impl TelegramPoller {
    pub fn new(api: Arc<dyn TelegramApi>, handler: Arc<dyn TelegramUpdateHandler>) -> Self {
        Self {
            api,
            handler,
            offset: None,
            conflict_retries: 0,
            network_retries: 0,
            timeout_seconds: 50,
        }
    }

    pub fn offset(&self) -> Option<i64> {
        self.offset
    }

    pub async fn poll_once(&mut self) -> Result<TelegramPollOutcome, TelegramPollingError> {
        match self
            .api
            .get_updates(self.offset, self.timeout_seconds)
            .await
        {
            Ok(updates) => {
                self.conflict_retries = 0;
                self.network_retries = 0;
                for update in updates {
                    let next_offset = update.update_id + 1;
                    if is_supported_update(&update) {
                        self.handler.handle_update(update).await?;
                    }
                    self.offset = Some(next_offset);
                }
                Ok(TelegramPollOutcome::Polled)
            }
            Err(error) if error.is_conflict() => {
                self.conflict_retries += 1;
                if self.conflict_retries > 3 {
                    Err(TelegramPollingError::ConflictLimit)
                } else {
                    Ok(TelegramPollOutcome::RetryAfter {
                        delay_seconds: 10,
                        reason: "telegram getUpdates conflict".into(),
                    })
                }
            }
            Err(TelegramApiError::Network(message)) => {
                self.network_retries += 1;
                if self.network_retries > 10 {
                    return Err(TelegramPollingError::NetworkLimit(message));
                }
                Ok(TelegramPollOutcome::RetryAfter {
                    delay_seconds: network_backoff_seconds(self.network_retries),
                    reason: message,
                })
            }
            Err(error) => Err(TelegramPollingError::Api(error)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramPollOutcome {
    Polled,
    RetryAfter { delay_seconds: u64, reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum TelegramPollingError {
    #[error("telegram api failed: {0}")]
    Api(#[from] TelegramApiError),
    #[error("telegram polling conflict did not clear after retries")]
    ConflictLimit,
    #[error("telegram network did not recover after retries: {0}")]
    NetworkLimit(String),
    #[error("telegram update handler failed: {0}")]
    Handler(String),
}

fn is_supported_update(update: &TelegramUpdate) -> bool {
    update
        .message
        .as_ref()
        .and_then(|message| message.text.as_ref())
        .is_some()
        || update.callback_query.is_some()
}

fn network_backoff_seconds(retries: u8) -> u64 {
    let delay = 5_u64.saturating_mul(1_u64 << retries.saturating_sub(1));
    delay.min(60)
}

#[cfg(test)]
mod tests {
    use super::{
        TelegramPollOutcome, TelegramPoller, TelegramPollingError, TelegramUpdateHandler,
        TelegramUpdateHandlerFuture,
    };
    use crate::{
        polling::{TelegramChat, TelegramMessage, TelegramUpdate},
        telegram_api::{
            TelegramApi, TelegramApiError, TelegramEditMessageTextRequest, TelegramMessageHandle,
            TelegramSendMessageRequest,
        },
    };
    use std::{
        collections::VecDeque,
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };

    #[tokio::test]
    async fn polling_advances_offset() {
        let api = Arc::new(FakeApi::with_updates(vec![
            text_update(7, "hello"),
            unsupported_update(8),
        ]));
        let handler = Arc::new(FakeHandler::default());
        let mut poller = TelegramPoller::new(api, handler.clone());

        poller.poll_once().await.unwrap();

        assert_eq!(poller.offset(), Some(9));
        assert_eq!(handler.handled_ids(), vec![7]);
    }

    #[test]
    fn telegram_update_deserializes_bot_api_snake_case() {
        let update = serde_json::from_value::<TelegramUpdate>(serde_json::json!({
            "update_id": 7,
            "message": {
                "message_id": 11,
                "message_thread_id": 3,
                "chat": {"id": -100, "type": "supergroup"},
                "from": {"id": 42, "username": "alice"},
                "text": "hello",
                "reply_to_message": {
                    "message_id": 10,
                    "chat": {"id": -100, "type": "supergroup"},
                    "from": {"id": 1, "username": "noloong_bot"},
                    "text": "previous"
                }
            }
        }))
        .unwrap();

        let message = update.message.unwrap();
        assert_eq!(update.update_id, 7);
        assert_eq!(message.message_id, 11);
        assert_eq!(message.message_thread_id, Some(3));
        assert_eq!(message.chat.kind, "supergroup");
        assert_eq!(
            message
                .reply_to_message
                .as_ref()
                .and_then(|reply| reply.from.as_ref())
                .and_then(|user| user.username.as_deref()),
            Some("noloong_bot")
        );
    }

    #[tokio::test]
    async fn polling_retries_network_errors() {
        let api = Arc::new(FakeApi::with_errors(vec![TelegramApiError::Network(
            "timeout".into(),
        )]));
        let handler = Arc::new(FakeHandler::default());
        let mut poller = TelegramPoller::new(api, handler);

        let outcome = poller.poll_once().await.unwrap();

        assert_eq!(
            outcome,
            TelegramPollOutcome::RetryAfter {
                delay_seconds: 5,
                reason: "timeout".into(),
            }
        );
    }

    #[tokio::test]
    async fn polling_conflict_becomes_fatal_after_retries() {
        let api = Arc::new(FakeApi::with_errors(vec![
            conflict(),
            conflict(),
            conflict(),
            conflict(),
        ]));
        let handler = Arc::new(FakeHandler::default());
        let mut poller = TelegramPoller::new(api, handler);

        assert!(poller.poll_once().await.is_ok());
        assert!(poller.poll_once().await.is_ok());
        assert!(poller.poll_once().await.is_ok());
        assert!(matches!(
            poller.poll_once().await.unwrap_err(),
            TelegramPollingError::ConflictLimit
        ));
    }

    fn text_update(update_id: i64, text: &str) -> TelegramUpdate {
        TelegramUpdate {
            update_id,
            message: Some(TelegramMessage {
                message_id: update_id,
                message_thread_id: None,
                chat: TelegramChat {
                    id: 42,
                    kind: "private".into(),
                },
                from: None,
                text: Some(text.into()),
                reply_to_message: None,
            }),
            callback_query: None,
        }
    }

    fn unsupported_update(update_id: i64) -> TelegramUpdate {
        TelegramUpdate {
            update_id,
            message: None,
            callback_query: None,
        }
    }

    fn conflict() -> TelegramApiError {
        TelegramApiError::Api {
            code: 409,
            description: "terminated by other getUpdates request".into(),
        }
    }

    #[derive(Default)]
    struct FakeHandler {
        handled: Mutex<Vec<i64>>,
    }

    impl FakeHandler {
        fn handled_ids(&self) -> Vec<i64> {
            self.handled
                .lock()
                .expect("fake handled lock poisoned")
                .clone()
        }
    }

    impl TelegramUpdateHandler for FakeHandler {
        fn handle_update<'a>(&'a self, update: TelegramUpdate) -> TelegramUpdateHandlerFuture<'a> {
            Box::pin(async move {
                self.handled
                    .lock()
                    .expect("fake handled lock poisoned")
                    .push(update.update_id);
                Ok(())
            })
        }
    }

    struct FakeApi {
        updates: Mutex<VecDeque<Vec<TelegramUpdate>>>,
        errors: Mutex<VecDeque<TelegramApiError>>,
    }

    impl FakeApi {
        fn with_updates(updates: Vec<TelegramUpdate>) -> Self {
            Self {
                updates: Mutex::new(VecDeque::from([updates])),
                errors: Mutex::new(VecDeque::new()),
            }
        }

        fn with_errors(errors: Vec<TelegramApiError>) -> Self {
            Self {
                updates: Mutex::new(VecDeque::new()),
                errors: Mutex::new(errors.into()),
            }
        }
    }

    impl TelegramApi for FakeApi {
        fn get_updates<'a>(
            &'a self,
            _offset: Option<i64>,
            _timeout_seconds: u64,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<TelegramUpdate>, TelegramApiError>> + Send + 'a>>
        {
            Box::pin(async move {
                if let Some(error) = self
                    .errors
                    .lock()
                    .expect("fake errors lock poisoned")
                    .pop_front()
                {
                    return Err(error);
                }
                Ok(self
                    .updates
                    .lock()
                    .expect("fake updates lock poisoned")
                    .pop_front()
                    .unwrap_or_default())
            })
        }

        fn send_message<'a>(
            &'a self,
            _request: TelegramSendMessageRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async {
                Ok(TelegramMessageHandle {
                    chat_id: 42,
                    message_id: 1,
                })
            })
        }

        fn edit_message_text<'a>(
            &'a self,
            _request: TelegramEditMessageTextRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async {
                Ok(TelegramMessageHandle {
                    chat_id: 42,
                    message_id: 1,
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

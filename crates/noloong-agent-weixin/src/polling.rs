use crate::{
    ilink_api::{LONG_POLL_TIMEOUT_MS, WeixinApi, WeixinApiError, WeixinMessage},
    state::{WeixinStateError, WeixinStateStore},
};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;

pub type WeixinUpdateHandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), WeixinPollingError>> + Send + 'a>>;

pub trait WeixinUpdateHandler: Send + Sync {
    fn handle_message<'a>(&'a self, message: WeixinMessage) -> WeixinUpdateHandlerFuture<'a>;
}

#[derive(Clone)]
pub struct WeixinPoller {
    api: Arc<dyn WeixinApi>,
    state: Arc<dyn WeixinStateStore>,
    handler: Arc<dyn WeixinUpdateHandler>,
    sync_buf: Option<String>,
    dedup: MessageDeduplicator,
    timeout_ms: u64,
    consecutive_failures: u8,
}

impl WeixinPoller {
    pub fn new(
        api: Arc<dyn WeixinApi>,
        state: Arc<dyn WeixinStateStore>,
        handler: Arc<dyn WeixinUpdateHandler>,
    ) -> Self {
        Self {
            api,
            state,
            handler,
            sync_buf: None,
            dedup: MessageDeduplicator::new(Duration::from_secs(300)),
            timeout_ms: LONG_POLL_TIMEOUT_MS,
            consecutive_failures: 0,
        }
    }

    pub async fn initialize(&mut self) -> Result<(), WeixinPollingError> {
        if self.sync_buf.is_none() {
            self.sync_buf = Some(self.state.load_sync_buf().await?);
        }
        Ok(())
    }

    pub async fn poll_once(&mut self) -> Result<WeixinPollOutcome, WeixinPollingError> {
        let sync_buf = self.sync_buf.clone().unwrap_or_default();
        match self.api.get_updates(&sync_buf, self.timeout_ms).await {
            Ok(response) => {
                self.consecutive_failures = 0;
                let message_count = response.msgs.len();
                if message_count > 0 {
                    log::info!(
                        "weixin polling received {message_count} message(s); sync_buf_updated={}",
                        !response.get_updates_buf.is_empty()
                    );
                }
                if let Some(timeout_ms) = response
                    .longpolling_timeout_ms
                    .filter(|timeout_ms| *timeout_ms > 0)
                {
                    self.timeout_ms = timeout_ms;
                }
                if !response.get_updates_buf.is_empty() {
                    self.sync_buf = Some(response.get_updates_buf.clone());
                    self.state.save_sync_buf(&response.get_updates_buf).await?;
                }
                for message in response.msgs {
                    if self.should_skip_message(&message) {
                        log::debug!(
                            "weixin polling skipped duplicate message; message_id={} from={}",
                            safe_log_id(message.message_id.as_deref()),
                            safe_log_id(message.from_user_id.as_deref())
                        );
                        continue;
                    }
                    log::debug!(
                        "weixin polling dispatching message; message_id={} from={}",
                        safe_log_id(message.message_id.as_deref()),
                        safe_log_id(message.from_user_id.as_deref())
                    );
                    if let Err(error) = self.handler.handle_message(message).await {
                        log::warn!("weixin update handler failed: {error}");
                    }
                }
                Ok(WeixinPollOutcome::Polled)
            }
            Err(error) if error.is_session_expired() => Ok(WeixinPollOutcome::RetryAfter {
                delay: Duration::from_secs(600),
                reason: error.to_string(),
            }),
            Err(error) if error.is_rate_limited() => Ok(WeixinPollOutcome::RetryAfter {
                delay: Duration::from_secs(2),
                reason: error.to_string(),
            }),
            Err(WeixinApiError::Network(message)) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                Ok(WeixinPollOutcome::RetryAfter {
                    delay: network_backoff(self.consecutive_failures),
                    reason: message,
                })
            }
            Err(error) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                Ok(WeixinPollOutcome::RetryAfter {
                    delay: if self.consecutive_failures >= 3 {
                        Duration::from_secs(30)
                    } else {
                        Duration::from_secs(2)
                    },
                    reason: error.to_string(),
                })
            }
        }
    }

    fn should_skip_message(&mut self, message: &WeixinMessage) -> bool {
        if let Some(message_id) = message.message_id.as_deref()
            && !message_id.trim().is_empty()
        {
            return self.dedup.seen(message_id);
        }
        if crate::input::extract_text(&message.item_list)
            .as_deref()
            .unwrap_or_default()
            .is_empty()
        {
            return false;
        }
        self.dedup.seen(&crate::input::content_fingerprint(message))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WeixinPollOutcome {
    Polled,
    RetryAfter { delay: Duration, reason: String },
}

#[derive(Clone)]
struct MessageDeduplicator {
    ttl: Duration,
    seen: BTreeMap<String, Instant>,
}

impl MessageDeduplicator {
    fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            seen: BTreeMap::new(),
        }
    }

    fn seen(&mut self, key: &str) -> bool {
        let now = Instant::now();
        self.seen
            .retain(|_, seen_at| now.duration_since(*seen_at) <= self.ttl);
        if self.seen.contains_key(key) {
            return true;
        }
        self.seen.insert(key.into(), now);
        false
    }
}

fn network_backoff(retries: u8) -> Duration {
    Duration::from_secs(match retries {
        0 | 1 => 2,
        2 => 5,
        3 => 10,
        _ => 30,
    })
}

fn safe_log_id(value: Option<&str>) -> String {
    let value = value.unwrap_or_default().trim();
    if value.is_empty() {
        return "?".into();
    }
    value.chars().take(12).collect()
}

#[derive(Debug, Error)]
pub enum WeixinPollingError {
    #[error("{0}")]
    Api(#[from] WeixinApiError),
    #[error("{0}")]
    State(#[from] WeixinStateError),
    #[error("Weixin update handler failed: {0}")]
    Handler(String),
}

pub async fn run_polling_loop(mut poller: WeixinPoller) -> Result<(), WeixinPollingError> {
    poller.initialize().await?;
    loop {
        match poller.poll_once().await? {
            WeixinPollOutcome::Polled => {}
            WeixinPollOutcome::RetryAfter { delay, reason } => {
                log::warn!(
                    "weixin polling retrying after {}s: {reason}",
                    delay.as_secs()
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageDeduplicator, WeixinPollOutcome, WeixinPoller, WeixinUpdateHandler};
    use crate::{
        ilink_api::{
            ITEM_TEXT, WeixinApi, WeixinApiFuture, WeixinConfigResponse, WeixinGetConfigRequest,
            WeixinGetUploadUrlRequest, WeixinMessage, WeixinMessageItem, WeixinQrCode,
            WeixinQrStatus, WeixinRawResponse, WeixinSendMessageRequest, WeixinSendMessageResponse,
            WeixinSendTypingRequest, WeixinTextItem, WeixinUpdatesResponse,
            WeixinUploadUrlResponse,
        },
        state::{SqliteWeixinStateStore, WeixinStateStore},
    };
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[test]
    fn dedup_expires_by_ttl() {
        let mut dedup = MessageDeduplicator::new(Duration::from_secs(300));

        assert!(!dedup.seen("a"));
        assert!(dedup.seen("a"));
    }

    #[tokio::test]
    async fn poller_saves_sync_buf_and_continues_after_handler_error() {
        let path = std::env::temp_dir().join(format!(
            "noloong-weixin-polling-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ));
        let state = Arc::new(SqliteWeixinStateStore::new(
            path.to_string_lossy().to_string(),
            "account",
        ));
        let api = Arc::new(FakeWeixinApi {
            response: Mutex::new(WeixinUpdatesResponse {
                get_updates_buf: "sync-2".into(),
                msgs: vec![WeixinMessage {
                    message_id: Some("m1".into()),
                    from_user_id: Some("u1".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        });
        let handler = Arc::new(FailingHandler::default());
        let mut poller = WeixinPoller::new(api, state.clone(), handler.clone());
        poller.initialize().await.unwrap();

        assert_eq!(poller.poll_once().await.unwrap(), WeixinPollOutcome::Polled);

        assert_eq!(state.load_sync_buf().await.unwrap(), "sync-2");
        assert_eq!(*handler.seen.lock().unwrap(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn poller_dispatches_repeated_text_with_distinct_message_ids() {
        let path = std::env::temp_dir().join(format!(
            "noloong-weixin-polling-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ));
        let state = Arc::new(SqliteWeixinStateStore::new(
            path.to_string_lossy().to_string(),
            "account",
        ));
        let message = |message_id: &str| WeixinMessage {
            message_id: Some(message_id.into()),
            from_user_id: Some("u1".into()),
            item_list: vec![WeixinMessageItem {
                kind: ITEM_TEXT,
                text_item: Some(WeixinTextItem {
                    text: "/同意 1".into(),
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        let api = Arc::new(FakeWeixinApi {
            response: Mutex::new(WeixinUpdatesResponse {
                get_updates_buf: "sync-2".into(),
                msgs: vec![message("m1"), message("m2")],
                ..Default::default()
            }),
        });
        let handler = Arc::new(FailingHandler::default());
        let mut poller = WeixinPoller::new(api, state.clone(), handler.clone());
        poller.initialize().await.unwrap();

        assert_eq!(poller.poll_once().await.unwrap(), WeixinPollOutcome::Polled);

        assert_eq!(*handler.seen.lock().unwrap(), 2);
        let _ = std::fs::remove_file(path);
    }

    #[derive(Default)]
    struct FailingHandler {
        seen: Mutex<usize>,
    }

    impl WeixinUpdateHandler for FailingHandler {
        fn handle_message<'a>(
            &'a self,
            _message: WeixinMessage,
        ) -> super::WeixinUpdateHandlerFuture<'a> {
            Box::pin(async move {
                *self.seen.lock().unwrap() += 1;
                Err(super::WeixinPollingError::Handler("boom".into()))
            })
        }
    }

    struct FakeWeixinApi {
        response: Mutex<WeixinUpdatesResponse>,
    }

    impl WeixinApi for FakeWeixinApi {
        fn get_updates<'a>(
            &'a self,
            _sync_buf: &'a str,
            _timeout_ms: u64,
        ) -> WeixinApiFuture<'a, WeixinUpdatesResponse> {
            Box::pin(async move { Ok(self.response.lock().unwrap().clone()) })
        }

        fn send_message<'a>(
            &'a self,
            _request: WeixinSendMessageRequest,
        ) -> WeixinApiFuture<'a, WeixinSendMessageResponse> {
            Box::pin(async { Ok(WeixinSendMessageResponse::default()) })
        }

        fn send_typing<'a>(
            &'a self,
            _request: WeixinSendTypingRequest,
        ) -> WeixinApiFuture<'a, WeixinRawResponse> {
            Box::pin(async { Ok(WeixinRawResponse::default()) })
        }

        fn get_config<'a>(
            &'a self,
            _request: WeixinGetConfigRequest,
        ) -> WeixinApiFuture<'a, WeixinConfigResponse> {
            Box::pin(async { Ok(WeixinConfigResponse::default()) })
        }

        fn get_upload_url<'a>(
            &'a self,
            _request: WeixinGetUploadUrlRequest,
        ) -> WeixinApiFuture<'a, WeixinUploadUrlResponse> {
            Box::pin(async { Ok(WeixinUploadUrlResponse::default()) })
        }

        fn upload_ciphertext<'a>(
            &'a self,
            _upload_url: &'a str,
            _ciphertext: Vec<u8>,
        ) -> WeixinApiFuture<'a, String> {
            Box::pin(async { Ok("encrypted".into()) })
        }

        fn download_bytes<'a>(&'a self, _url: &'a str) -> WeixinApiFuture<'a, Vec<u8>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_bot_qrcode<'a>(&'a self, _bot_type: &'a str) -> WeixinApiFuture<'a, WeixinQrCode> {
            Box::pin(async { Ok(WeixinQrCode::default()) })
        }

        fn get_qrcode_status<'a>(
            &'a self,
            _qrcode: &'a str,
        ) -> WeixinApiFuture<'a, WeixinQrStatus> {
            Box::pin(async { Ok(WeixinQrStatus::default()) })
        }
    }
}

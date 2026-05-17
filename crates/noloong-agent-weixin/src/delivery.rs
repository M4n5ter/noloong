use crate::{
    ilink_api::{
        TYPING_START, TYPING_STOP, WeixinApi, WeixinApiError, WeixinGetConfigRequest,
        WeixinGetUploadUrlRequest, WeixinOutboundItem, WeixinSendMessageRequest,
        WeixinSendTypingRequest, send_with_rate_limit_retry,
    },
    media::{
        aes_key_for_api, aes128_ecb_encrypt, ensure_outbound_file_size, media_bytes_for_outbound,
        outbound_file_name, outbound_mime_type,
    },
    render::render_agent_message_text,
    state::WeixinStateStore,
    text::split_weixin_text,
};
use md5::{Digest, Md5};
use noloong_agent_core::{AgentMessage, ContentBlock, MediaKind};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use thiserror::Error;
use uuid::Uuid;

const MEDIA_IMAGE: i64 = 1;
const MEDIA_FILE: i64 = 3;
const TYPING_TICKET_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct WeixinDelivery {
    api: Arc<dyn WeixinApi>,
    state: Arc<dyn WeixinStateStore>,
    cdn_base_url: String,
    max_message_chars: usize,
    max_upload_bytes: usize,
    typing_tickets: Arc<Mutex<BTreeMap<TypingTicketKey, TypingTicketEntry>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TypingTicketKey {
    peer_id: String,
    context_token: Option<String>,
}

#[derive(Clone, Debug)]
struct TypingTicketEntry {
    ticket: String,
    expires_at: Instant,
}

impl WeixinDelivery {
    pub fn new(
        api: Arc<dyn WeixinApi>,
        state: Arc<dyn WeixinStateStore>,
        cdn_base_url: impl Into<String>,
        max_message_chars: usize,
        max_upload_bytes: usize,
    ) -> Self {
        Self {
            api,
            state,
            cdn_base_url: cdn_base_url.into(),
            max_message_chars,
            max_upload_bytes,
            typing_tickets: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub async fn send_text(&self, peer_id: &str, text: &str) -> Result<(), WeixinDeliveryError> {
        for chunk in split_weixin_text(text, self.max_message_chars) {
            self.send_text_chunk(peer_id, &chunk).await?;
        }
        Ok(())
    }

    pub async fn send_agent_message(
        &self,
        peer_id: &str,
        message: &AgentMessage,
    ) -> Result<(), WeixinDeliveryError> {
        let mut pending_text = String::new();
        for block in &message.content {
            match block {
                ContentBlock::Media { media } => {
                    if !pending_text.trim().is_empty() {
                        self.send_text(peer_id, &pending_text).await?;
                        pending_text.clear();
                    }
                    if let Err(error) = self.send_media(peer_id, media).await {
                        self.send_text(peer_id, &format!("附件发送失败：{error}"))
                            .await?;
                    }
                }
                _ => {
                    if let Some(text) = crate::render::render_user_visible_content_block_text(block)
                    {
                        if !pending_text.is_empty() {
                            pending_text.push('\n');
                        }
                        pending_text.push_str(&text);
                    }
                }
            }
        }
        if !pending_text.trim().is_empty() {
            self.send_text(
                peer_id,
                &crate::text::normalize_weixin_markdown(&pending_text),
            )
            .await?;
        } else if !message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Media { .. }))
        {
            let text = render_agent_message_text(message);
            if !text.trim().is_empty() {
                self.send_text(peer_id, &text).await?;
            }
        }
        Ok(())
    }

    pub async fn send_typing(
        &self,
        peer_id: &str,
        active: bool,
    ) -> Result<(), WeixinDeliveryError> {
        let context_token = self.state.context_token(peer_id).await?;
        let key = TypingTicketKey {
            peer_id: peer_id.into(),
            context_token: context_token.clone(),
        };
        if !active {
            let Some(typing_ticket) = self.cached_typing_ticket(&key) else {
                log::debug!(
                    "weixin typing stop skipped; peer={} cached_ticket=false",
                    safe_log_id(peer_id)
                );
                return Ok(());
            };
            return self
                .send_typing_with_ticket(peer_id, typing_ticket, false, Some(&key))
                .await;
        }
        let config = self
            .api
            .get_config(WeixinGetConfigRequest {
                ilink_user_id: peer_id.into(),
                context_token,
            })
            .await?;
        let Some(typing_ticket) = config.typing_ticket else {
            log::debug!(
                "weixin typing skipped; peer={} ticket=false",
                safe_log_id(peer_id)
            );
            return Ok(());
        };
        self.remember_typing_ticket(key.clone(), typing_ticket.clone());
        self.send_typing_with_ticket(peer_id, typing_ticket, true, Some(&key))
            .await?;
        Ok(())
    }

    async fn send_typing_with_ticket(
        &self,
        peer_id: &str,
        typing_ticket: String,
        active: bool,
        key: Option<&TypingTicketKey>,
    ) -> Result<(), WeixinDeliveryError> {
        if let Err(error) = self
            .api
            .send_typing(WeixinSendTypingRequest {
                ilink_user_id: peer_id.into(),
                typing_ticket,
                status: if active { TYPING_START } else { TYPING_STOP },
            })
            .await
        {
            if let Some(key) = key {
                self.forget_typing_ticket(key);
            }
            return Err(error.into());
        }
        log::debug!(
            "weixin typing delivered; peer={} active={active}",
            safe_log_id(peer_id)
        );
        Ok(())
    }

    fn cached_typing_ticket(&self, key: &TypingTicketKey) -> Option<String> {
        let now = Instant::now();
        let mut tickets = self
            .typing_tickets
            .lock()
            .expect("weixin typing ticket cache lock poisoned");
        if let Some(entry) = tickets.get(key)
            && entry.expires_at > now
        {
            return Some(entry.ticket.clone());
        }
        tickets.remove(key);
        None
    }

    fn remember_typing_ticket(&self, key: TypingTicketKey, ticket: String) {
        self.typing_tickets
            .lock()
            .expect("weixin typing ticket cache lock poisoned")
            .insert(
                key,
                TypingTicketEntry {
                    ticket,
                    expires_at: Instant::now() + TYPING_TICKET_TTL,
                },
            );
    }

    fn forget_typing_ticket(&self, key: &TypingTicketKey) {
        self.typing_tickets
            .lock()
            .expect("weixin typing ticket cache lock poisoned")
            .remove(key);
    }

    async fn send_text_chunk(&self, peer_id: &str, text: &str) -> Result<(), WeixinDeliveryError> {
        if text.trim().is_empty() {
            return Ok(());
        }
        let context_token = self.state.context_token(peer_id).await?;
        let request = WeixinSendMessageRequest::text(peer_id, text, context_token.clone());
        match send_with_rate_limit_retry(self.api.as_ref(), request).await {
            Ok(_) => {
                log::info!(
                    "weixin text delivered; peer={} chars={} context_token={}",
                    safe_log_id(peer_id),
                    text.chars().count(),
                    context_token.is_some()
                );
                Ok(())
            }
            Err(error) if error.is_session_expired() && context_token.is_some() => {
                self.state.delete_context_token(peer_id).await?;
                send_with_rate_limit_retry(
                    self.api.as_ref(),
                    WeixinSendMessageRequest::text(peer_id, text, None),
                )
                .await?;
                log::info!(
                    "weixin text delivered after clearing stale context token; peer={} chars={}",
                    safe_log_id(peer_id),
                    text.chars().count()
                );
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn send_item(
        &self,
        peer_id: &str,
        item: WeixinOutboundItem,
    ) -> Result<(), WeixinDeliveryError> {
        let context_token = self.state.context_token(peer_id).await?;
        let request = WeixinSendMessageRequest::item(peer_id, item.clone(), context_token.clone());
        match send_with_rate_limit_retry(self.api.as_ref(), request).await {
            Ok(_) => {
                log::info!(
                    "weixin media item delivered; peer={} context_token={}",
                    safe_log_id(peer_id),
                    context_token.is_some()
                );
                Ok(())
            }
            Err(error) if error.is_session_expired() && context_token.is_some() => {
                self.state.delete_context_token(peer_id).await?;
                send_with_rate_limit_retry(
                    self.api.as_ref(),
                    WeixinSendMessageRequest::item(peer_id, item, None),
                )
                .await?;
                log::info!(
                    "weixin media item delivered after clearing stale context token; peer={}",
                    safe_log_id(peer_id)
                );
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn send_media(
        &self,
        peer_id: &str,
        media: &noloong_agent_core::MediaBlock,
    ) -> Result<(), WeixinDeliveryError> {
        ensure_outbound_file_size(media, self.max_upload_bytes).await?;
        let bytes = media_bytes_for_outbound(media).await?;
        if bytes.len() > self.max_upload_bytes {
            return Err(WeixinDeliveryError::FileTooLarge {
                limit: self.max_upload_bytes,
                actual: bytes.len(),
            });
        }
        let file_name = outbound_file_name(media);
        let mime_type = outbound_mime_type(media, &file_name);
        let media_type = if media.kind == MediaKind::Image || mime_type.starts_with("image/") {
            MEDIA_IMAGE
        } else {
            MEDIA_FILE
        };
        let mut aes_key = [0_u8; 16];
        getrandom::fill(&mut aes_key)
            .map_err(|error| WeixinDeliveryError::Random(error.to_string()))?;
        let ciphertext = aes128_ecb_encrypt(&bytes, &aes_key);
        let ciphertext_len = ciphertext.len() as u64;
        let filekey = format!("noloong-{}", Uuid::new_v4().simple());
        let rawfilemd5 = md5_hex(&bytes);
        let upload = self
            .api
            .get_upload_url(WeixinGetUploadUrlRequest {
                filekey: filekey.clone(),
                media_type,
                to_user_id: peer_id.into(),
                rawsize: bytes.len() as u64,
                rawfilemd5,
                filesize: ciphertext_len,
                aes_key_hex: hex::encode(aes_key),
                no_need_thumb: true,
            })
            .await?;
        let upload_url = upload
            .upload_full_url
            .clone()
            .or_else(|| {
                upload.upload_param.map(|param| {
                    format!(
                        "{}/upload?encrypted_query_param={}&filekey={}",
                        self.cdn_base_url.trim_end_matches('/'),
                        url_escape(&param),
                        url_escape(&filekey)
                    )
                })
            })
            .ok_or_else(|| {
                WeixinDeliveryError::Protocol("getuploadurl missing upload URL".into())
            })?;
        let encrypted_query_param = self.api.upload_ciphertext(&upload_url, ciphertext).await?;
        let aes_key_for_api = aes_key_for_api(&aes_key);
        let item = if media_type == MEDIA_IMAGE {
            WeixinOutboundItem::image(encrypted_query_param, aes_key_for_api, ciphertext_len)
        } else {
            WeixinOutboundItem::file(
                encrypted_query_param,
                aes_key_for_api,
                file_name,
                bytes.len() as u64,
            )
        };
        self.send_item(peer_id, item).await
    }
}

fn md5_hex(bytes: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn url_escape(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn safe_log_id(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "?".into();
    }
    value.chars().take(12).collect()
}

#[derive(Debug, Error)]
pub enum WeixinDeliveryError {
    #[error("{0}")]
    Api(#[from] WeixinApiError),
    #[error("{0}")]
    State(#[from] crate::state::WeixinStateError),
    #[error("{0}")]
    Media(#[from] crate::media::WeixinMediaError),
    #[error("Weixin outbound file is too large: limit {limit} bytes, actual {actual} bytes")]
    FileTooLarge { limit: usize, actual: usize },
    #[error("Weixin delivery protocol failed: {0}")]
    Protocol(String),
    #[error("random bytes failed: {0}")]
    Random(String),
}

#[cfg(test)]
mod tests {
    use crate::{
        ilink_api::{
            ITEM_IMAGE, TYPING_START, TYPING_STOP, WeixinApi, WeixinApiError, WeixinApiFuture,
            WeixinConfigResponse, WeixinGetConfigRequest, WeixinGetUploadUrlRequest, WeixinQrCode,
            WeixinQrStatus, WeixinRawResponse, WeixinSendMessageRequest, WeixinSendMessageResponse,
            WeixinSendTypingRequest, WeixinUpdatesResponse, WeixinUploadUrlResponse,
        },
        media::aes128_ecb_decrypt,
        state::{SqliteWeixinStateStore, WeixinStateStore},
    };
    use base64::{Engine as _, engine::general_purpose};
    use noloong_agent::{Locale, interaction::DisplayEvent};
    use noloong_agent_core::{AgentMessage, ContentBlock, MediaBlock, MediaKind};
    use std::sync::{Arc, Mutex};

    use super::WeixinDelivery;

    #[tokio::test]
    async fn stale_context_token_is_cleared_and_retried() {
        let path = std::env::temp_dir().join(format!(
            "noloong-weixin-delivery-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ));
        let state = Arc::new(
            SqliteWeixinStateStore::new(path.to_string_lossy().to_string(), "account").unwrap(),
        );
        state.save_context_token("user", "ctx").await.unwrap();
        let api = Arc::new(FakeWeixinApi::session_expired_once());
        let delivery = WeixinDelivery::new(
            api.clone(),
            state.clone(),
            "https://novac2c.cdn.weixin.qq.com/c2c",
            3500,
            1024,
        );

        delivery.send_text("user", "hello").await.unwrap();

        assert_eq!(state.context_token("user").await.unwrap(), None);
        assert_eq!(api.requests.lock().unwrap().len(), 2);
        assert_eq!(
            api.requests.lock().unwrap()[0].msg.context_token.as_deref(),
            Some("ctx")
        );
        assert_eq!(api.requests.lock().unwrap()[1].msg.context_token, None);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn outbound_inline_image_uploads_ciphertext_and_sends_image_item() {
        let path = std::env::temp_dir().join(format!(
            "noloong-weixin-delivery-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ));
        let state = Arc::new(
            SqliteWeixinStateStore::new(path.to_string_lossy().to_string(), "account").unwrap(),
        );
        state.save_context_token("user", "ctx").await.unwrap();
        let api = Arc::new(FakeWeixinApi::with_upload_param("upload param"));
        let delivery = WeixinDelivery::new(
            api.clone(),
            state,
            "https://novac2c.cdn.weixin.qq.com/c2c",
            3500,
            1024,
        );
        let plaintext = b"fake png bytes";
        let mut media = MediaBlock::inline_base64(
            MediaKind::Image,
            general_purpose::STANDARD.encode(plaintext),
        );
        media.mime_type = Some("image/png".into());
        media.name = Some("smoke.png".into());
        let message = AgentMessage::assistant("assistant-1", vec![ContentBlock::Media { media }]);

        delivery.send_agent_message("user", &message).await.unwrap();

        let upload_requests = api.upload_requests.lock().unwrap();
        assert_eq!(upload_requests.len(), 1);
        let upload_request = &upload_requests[0];
        assert_eq!(upload_request.media_type, 1);
        assert_eq!(upload_request.to_user_id, "user");
        assert_eq!(upload_request.rawsize, plaintext.len() as u64);
        assert_eq!(
            upload_request.filesize as usize,
            (plaintext.len() + 1).div_ceil(16) * 16
        );
        assert!(upload_request.no_need_thumb);

        let uploads = api.uploads.lock().unwrap();
        assert_eq!(uploads.len(), 1);
        assert!(uploads[0].0.contains("encrypted_query_param=upload+param"));
        assert!(uploads[0].0.contains(&upload_request.filekey));
        let key: [u8; 16] = hex::decode(&upload_request.aes_key_hex)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(aes128_ecb_decrypt(&uploads[0].1, &key).unwrap(), plaintext);

        let requests = api.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].msg.context_token.as_deref(), Some("ctx"));
        let item = &requests[0].msg.item_list[0];
        assert_eq!(item.kind, ITEM_IMAGE);
        let image = item.image_item.as_ref().unwrap();
        assert_eq!(image.mid_size, Some(uploads[0].1.len() as u64));
        let media = image.media.as_ref().unwrap();
        assert_eq!(
            media.encrypt_query_param.as_deref(),
            Some("encrypted-param")
        );
        assert!(media.aes_key.is_some());
        assert_eq!(media.encrypt_type, Some(1));
        let serialized_media = serde_json::to_value(media).unwrap();
        assert_eq!(serialized_media["encrypt_type"], 1);
        assert!(serialized_media.get("full_url").is_none());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn display_final_media_uploads_and_sends_item() {
        let path = std::env::temp_dir().join(format!(
            "noloong-weixin-display-media-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ));
        let state = Arc::new(
            SqliteWeixinStateStore::new(path.to_string_lossy().to_string(), "account").unwrap(),
        );
        state.save_context_token("user", "ctx").await.unwrap();
        let api = Arc::new(FakeWeixinApi::with_upload_param("upload param"));
        let delivery = WeixinDelivery::new(
            api.clone(),
            state,
            "https://novac2c.cdn.weixin.qq.com/c2c",
            3500,
            1024,
        );
        let mut media = MediaBlock::inline_base64(
            MediaKind::Image,
            general_purpose::STANDARD.encode(b"display image bytes"),
        );
        media.mime_type = Some("image/png".into());
        media.name = Some("display.png".into());
        let message = AgentMessage::assistant(
            "assistant-1",
            vec![
                ContentBlock::Text {
                    text: "see image".into(),
                },
                ContentBlock::Media { media },
            ],
        );

        crate::display::deliver_display_event(
            &mut crate::display::WeixinDisplayState::default(),
            &delivery,
            "user",
            Locale::Zh,
            &DisplayEvent::AssistantMessageFinal {
                run_id: "run-1".into(),
                display_message_id: "display-1".into(),
                message,
                truncated: false,
            },
        )
        .await
        .unwrap();

        let requests = api.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].msg.item_list[0]
                .text_item
                .as_ref()
                .unwrap()
                .text,
            "see image"
        );
        assert_eq!(requests[1].msg.item_list[0].kind, ITEM_IMAGE);
        assert_eq!(api.upload_requests.lock().unwrap().len(), 1);
        assert_eq!(api.uploads.lock().unwrap().len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn typing_stop_reuses_cached_ticket_without_get_config() {
        let path = std::env::temp_dir().join(format!(
            "noloong-weixin-typing-{}.sqlite",
            uuid::Uuid::new_v4().simple()
        ));
        let state = Arc::new(
            SqliteWeixinStateStore::new(path.to_string_lossy().to_string(), "account").unwrap(),
        );
        state.save_context_token("user", "ctx").await.unwrap();
        let api = Arc::new(FakeWeixinApi::with_typing_ticket("ticket-1"));
        let delivery = WeixinDelivery::new(
            api.clone(),
            state,
            "https://novac2c.cdn.weixin.qq.com/c2c",
            3500,
            1024,
        );

        delivery.send_typing("user", true).await.unwrap();
        delivery.send_typing("user", false).await.unwrap();

        assert_eq!(api.config_requests.lock().unwrap().len(), 1);
        let typing_requests = api.typing_requests.lock().unwrap();
        assert_eq!(typing_requests.len(), 2);
        assert_eq!(typing_requests[0].status, TYPING_START);
        assert_eq!(typing_requests[1].status, TYPING_STOP);
        let _ = std::fs::remove_file(path);
    }

    #[derive(Default)]
    struct FakeWeixinApi {
        fail_first: Mutex<bool>,
        requests: Mutex<Vec<WeixinSendMessageRequest>>,
        typing_requests: Mutex<Vec<WeixinSendTypingRequest>>,
        config_requests: Mutex<Vec<WeixinGetConfigRequest>>,
        upload_requests: Mutex<Vec<WeixinGetUploadUrlRequest>>,
        uploads: Mutex<Vec<(String, Vec<u8>)>>,
        upload_response: Mutex<WeixinUploadUrlResponse>,
        typing_ticket: Mutex<Option<String>>,
    }

    impl FakeWeixinApi {
        fn session_expired_once() -> Self {
            Self {
                fail_first: Mutex::new(true),
                requests: Mutex::new(Vec::new()),
                typing_requests: Mutex::new(Vec::new()),
                config_requests: Mutex::new(Vec::new()),
                upload_requests: Mutex::new(Vec::new()),
                uploads: Mutex::new(Vec::new()),
                upload_response: Mutex::new(WeixinUploadUrlResponse::default()),
                typing_ticket: Mutex::new(None),
            }
        }

        fn with_upload_param(upload_param: impl Into<String>) -> Self {
            Self {
                upload_response: Mutex::new(WeixinUploadUrlResponse {
                    upload_param: Some(upload_param.into()),
                    ..Default::default()
                }),
                ..Default::default()
            }
        }

        fn with_typing_ticket(ticket: impl Into<String>) -> Self {
            Self {
                typing_ticket: Mutex::new(Some(ticket.into())),
                ..Default::default()
            }
        }
    }

    impl WeixinApi for FakeWeixinApi {
        fn get_updates<'a>(
            &'a self,
            _sync_buf: &'a str,
            _timeout_ms: u64,
        ) -> WeixinApiFuture<'a, WeixinUpdatesResponse> {
            Box::pin(async { Ok(WeixinUpdatesResponse::default()) })
        }

        fn send_message<'a>(
            &'a self,
            request: WeixinSendMessageRequest,
        ) -> WeixinApiFuture<'a, WeixinSendMessageResponse> {
            Box::pin(async move {
                self.requests.lock().unwrap().push(request);
                if std::mem::take(&mut *self.fail_first.lock().unwrap()) {
                    return Err(WeixinApiError::SessionExpired {
                        endpoint: "sendmessage".into(),
                        ret: Some(-14),
                        errcode: None,
                        errmsg: None,
                    });
                }
                Ok(WeixinSendMessageResponse::default())
            })
        }

        fn send_typing<'a>(
            &'a self,
            request: WeixinSendTypingRequest,
        ) -> WeixinApiFuture<'a, WeixinRawResponse> {
            Box::pin(async move {
                self.typing_requests.lock().unwrap().push(request);
                Ok(WeixinRawResponse::default())
            })
        }

        fn get_config<'a>(
            &'a self,
            request: WeixinGetConfigRequest,
        ) -> WeixinApiFuture<'a, WeixinConfigResponse> {
            Box::pin(async move {
                self.config_requests.lock().unwrap().push(request);
                Ok(WeixinConfigResponse {
                    typing_ticket: self.typing_ticket.lock().unwrap().clone(),
                    ..Default::default()
                })
            })
        }

        fn get_upload_url<'a>(
            &'a self,
            request: WeixinGetUploadUrlRequest,
        ) -> WeixinApiFuture<'a, WeixinUploadUrlResponse> {
            Box::pin(async move {
                self.upload_requests.lock().unwrap().push(request);
                Ok(self.upload_response.lock().unwrap().clone())
            })
        }

        fn upload_ciphertext<'a>(
            &'a self,
            upload_url: &'a str,
            ciphertext: Vec<u8>,
        ) -> WeixinApiFuture<'a, String> {
            Box::pin(async move {
                self.uploads
                    .lock()
                    .unwrap()
                    .push((upload_url.into(), ciphertext));
                Ok("encrypted-param".into())
            })
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

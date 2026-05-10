use crate::{
    approval::{TelegramApprovalSelection, TelegramApprovalStore, render_approval_request},
    bridge::InteractionDisplayNotification,
    delivery::{
        TelegramDelivery, TelegramDeliveryResult, TelegramMessageTarget, TelegramPreviewMessage,
    },
    i18n::TelegramUiCatalog,
};
use noloong_agent::interaction::DisplayEvent;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Default)]
pub struct TelegramDisplayState {
    messages: BTreeMap<String, DisplayMessageState>,
    approvals: TelegramApprovalStore,
}

impl TelegramDisplayState {
    pub fn preview_message_id(&self, display_message_id: &str) -> Option<i64> {
        self.messages
            .get(display_message_id)
            .and_then(|message| message.preview_message_id)
    }

    pub fn has_message(&self, display_message_id: &str) -> bool {
        self.messages.contains_key(display_message_id)
    }

    pub fn resolve_approval_callback(&mut self, data: &str) -> Option<TelegramApprovalSelection> {
        self.approvals.resolve(data)
    }
}

#[derive(Debug, Default)]
struct DisplayMessageState {
    preview_message_id: Option<i64>,
    accumulated_text: String,
    last_edit_at: Option<Instant>,
}

pub async fn deliver_display_event(
    state: &mut TelegramDisplayState,
    delivery: &TelegramDelivery,
    target: TelegramMessageTarget,
    notification: InteractionDisplayNotification,
    show_tool_status: bool,
    edit_throttle: Duration,
    catalog: TelegramUiCatalog,
) -> TelegramDeliveryResult<()> {
    match notification.event {
        DisplayEvent::AssistantMessageDelta {
            display_message_id,
            text,
        } => {
            let now = Instant::now();
            let action = record_delta(state, display_message_id.clone(), text, now, edit_throttle);
            match action {
                DisplayPreviewAction::Send(text) => {
                    let Some(sent) = delivery
                        .send_text(target, &text, None)
                        .await?
                        .into_iter()
                        .next()
                    else {
                        return Ok(());
                    };
                    if let Some(message) = state.messages.get_mut(&display_message_id) {
                        message.preview_message_id = Some(sent.message_id);
                        message.last_edit_at = Some(now);
                    }
                }
                DisplayPreviewAction::Edit { message_id, text } => {
                    delivery.edit_text(target, message_id, &text, None).await?;
                }
                DisplayPreviewAction::Skip => {}
            }
        }
        DisplayEvent::AssistantMessageFinal {
            display_message_id,
            message,
            ..
        } => {
            let preview = state
                .messages
                .remove(&display_message_id)
                .and_then(preview_from_display_state);
            delivery
                .send_agent_final_message(target, preview, &message)
                .await?;
        }
        DisplayEvent::ApprovalRequested { approval } => {
            let text = render_approval_request(&approval, catalog);
            let buttons = state.approvals.allocate_buttons();
            let Some(sent) = delivery
                .send_text(target, &text, Some(buttons.markup(catalog)))
                .await?
                .into_iter()
                .next()
            else {
                return Ok(());
            };
            state.approvals.insert_target(
                &buttons,
                notification.session_id,
                approval.approval_id,
                sent,
            );
        }
        DisplayEvent::ToolStarted { tool_name, .. } if show_tool_status => {
            delivery
                .send_text(target, &catalog.tool_started(&tool_name), None)
                .await?;
        }
        DisplayEvent::ToolCompleted { tool_call_id, .. } if show_tool_status => {
            delivery
                .send_text(target, &catalog.tool_completed(&tool_call_id), None)
                .await?;
        }
        DisplayEvent::RunFailed { error, .. } => {
            delivery
                .send_text(target, &catalog.run_failed(&error), None)
                .await?;
        }
        DisplayEvent::RunPaused { .. }
        | DisplayEvent::RunStarted { .. }
        | DisplayEvent::RunCompleted { .. }
        | DisplayEvent::ToolUpdated { .. }
        | DisplayEvent::ToolStarted { .. }
        | DisplayEvent::ToolCompleted { .. }
        | DisplayEvent::RawEvent { .. } => {}
    }
    Ok(())
}

fn preview_from_display_state(message: DisplayMessageState) -> Option<TelegramPreviewMessage> {
    Some(TelegramPreviewMessage {
        message_id: message.preview_message_id?,
        text: message.accumulated_text,
    })
}

enum DisplayPreviewAction {
    Send(String),
    Edit { message_id: i64, text: String },
    Skip,
}

fn record_delta(
    state: &mut TelegramDisplayState,
    display_message_id: String,
    text: String,
    now: Instant,
    edit_throttle: Duration,
) -> DisplayPreviewAction {
    let message = state.messages.entry(display_message_id).or_default();
    message.accumulated_text.push_str(&text);
    let Some(message_id) = message.preview_message_id else {
        return DisplayPreviewAction::Send(message.accumulated_text.clone());
    };
    if !should_edit(message.last_edit_at, now, edit_throttle) {
        return DisplayPreviewAction::Skip;
    }
    message.last_edit_at = Some(now);
    DisplayPreviewAction::Edit {
        message_id,
        text: message.accumulated_text.clone(),
    }
}

fn should_edit(last_edit_at: Option<Instant>, now: Instant, edit_throttle: Duration) -> bool {
    last_edit_at
        .map(|last_edit_at| now.duration_since(last_edit_at) >= edit_throttle)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::{TelegramDisplayState, deliver_display_event};
    use crate::{
        bridge::InteractionDisplayNotification,
        delivery::{TelegramDelivery, TelegramMessageTarget},
        i18n::TelegramUiCatalog,
        telegram_api::{
            TelegramApi, TelegramApiError, TelegramEditMessageTextRequest, TelegramMessageHandle,
            TelegramSendMessageRequest, TelegramSendPhotoRequest, TelegramUpdate,
        },
    };
    use noloong_agent::Locale;
    use noloong_agent::interaction::DisplayEvent;
    use noloong_agent_core::{
        AgentMessage, ContentBlock, MediaBlock, MediaKind, ToolApprovalRequest,
        ToolApprovalRequestSpec, ToolCall,
    };
    use serde_json::{Map, Value, json};
    use std::{
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[tokio::test]
    async fn display_delta_edits_message() {
        let api = Arc::new(FakeTelegramApi::default());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut state = TelegramDisplayState::default();

        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageDelta {
                display_message_id: "m1".into(),
                text: "hello".into(),
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();
        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageDelta {
                display_message_id: "m1".into(),
                text: " world".into(),
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();

        assert_eq!(api.sent_count(), 1);
        assert_eq!(api.edited_count(), 1);
        assert_eq!(
            api.edited_texts(),
            vec![crate::render::render_markdown_v2("hello world")]
        );
        assert_eq!(state.preview_message_id("m1"), Some(1));
    }

    #[tokio::test]
    async fn display_delta_throttles_edits() {
        let api = Arc::new(FakeTelegramApi::default());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut state = TelegramDisplayState::default();

        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageDelta {
                display_message_id: "m1".into(),
                text: "hello".into(),
            }),
            true,
            Duration::from_secs(60),
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();
        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageDelta {
                display_message_id: "m1".into(),
                text: " world".into(),
            }),
            true,
            Duration::from_secs(60),
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();

        assert_eq!(api.sent_count(), 1);
        assert_eq!(api.edited_count(), 0);
    }

    #[tokio::test]
    async fn display_final_flushes_message() {
        let api = Arc::new(FakeTelegramApi::default());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut state = TelegramDisplayState::default();

        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageDelta {
                display_message_id: "m1".into(),
                text: "draft".into(),
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();
        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageFinal {
                display_message_id: "m1".into(),
                message: AgentMessage::assistant(
                    "a1",
                    vec![ContentBlock::Text {
                        text: "final".into(),
                    }],
                ),
                truncated: false,
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();

        assert_eq!(api.sent_count(), 1);
        assert_eq!(api.edited_count(), 1);
        assert_eq!(state.preview_message_id("m1"), None);
        assert!(!state.has_message("m1"));
    }

    #[tokio::test]
    async fn display_final_sends_media_natively_after_preview() {
        let api = Arc::new(FakeTelegramApi::default());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut state = TelegramDisplayState::default();

        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageDelta {
                display_message_id: "m1".into(),
                text: "draft".into(),
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();
        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageFinal {
                display_message_id: "m1".into(),
                message: AgentMessage::assistant(
                    "a1",
                    vec![
                        ContentBlock::Text {
                            text: "final".into(),
                        },
                        ContentBlock::Media {
                            media: MediaBlock::inline_base64(MediaKind::Image, "YWJj"),
                        },
                    ],
                ),
                truncated: false,
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();

        assert_eq!(api.sent_count(), 1);
        assert_eq!(api.edited_count(), 1);
        assert_eq!(api.photo_count(), 1);
    }

    #[tokio::test]
    async fn display_final_skips_noop_preview_edit_before_media() {
        let api = Arc::new(FakeTelegramApi::default());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut state = TelegramDisplayState::default();

        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageDelta {
                display_message_id: "m1".into(),
                text: "final".into(),
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();
        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::AssistantMessageFinal {
                display_message_id: "m1".into(),
                message: AgentMessage::assistant(
                    "a1",
                    vec![
                        ContentBlock::Text {
                            text: "final".into(),
                        },
                        ContentBlock::Media {
                            media: MediaBlock::inline_base64(MediaKind::Image, "YWJj"),
                        },
                    ],
                ),
                truncated: false,
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();

        assert_eq!(api.sent_count(), 1);
        assert_eq!(api.edited_count(), 0);
        assert_eq!(api.photo_count(), 1);
    }

    #[tokio::test]
    async fn approval_request_sends_markup_without_extra_edit() {
        let api = Arc::new(FakeTelegramApi::default());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut state = TelegramDisplayState::default();

        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::ApprovalRequested {
                approval: ToolApprovalRequest {
                    approval_id: "approval-1".into(),
                    tool_call: ToolCall {
                        id: "tool-1".into(),
                        name: "host_exec".into(),
                        arguments: json!({"cmd": "ls"}),
                    },
                    permissions: Vec::new(),
                    hook_id: None,
                    request: ToolApprovalRequestSpec {
                        prompt: Some("Run command?".into()),
                        reason: None,
                        expires_at_ms: None,
                        metadata: Value::Object(Map::new()),
                    },
                },
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::Zh),
        )
        .await
        .unwrap();

        assert_eq!(api.sent_count(), 1);
        assert_eq!(api.edited_count(), 0);
        assert!(api.sent_has_reply_markup());
        assert!(
            api.sent_texts()
                .into_iter()
                .any(|text| text.contains("需要审批工具"))
        );
    }

    #[tokio::test]
    async fn display_tool_status_can_be_rendered() {
        let api = Arc::new(FakeTelegramApi::default());
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let mut state = TelegramDisplayState::default();

        deliver_display_event(
            &mut state,
            &delivery,
            target(),
            notification(DisplayEvent::ToolStarted {
                tool_call_id: "tool-1".into(),
                tool_name: "host_exec".into(),
            }),
            true,
            Duration::ZERO,
            TelegramUiCatalog::new(Locale::En),
        )
        .await
        .unwrap();

        assert_eq!(api.sent_count(), 1);
    }

    fn notification(event: DisplayEvent) -> InteractionDisplayNotification {
        InteractionDisplayNotification {
            session_id: "session-1".into(),
            subscription_id: "subscription-1".into(),
            event,
        }
    }

    fn target() -> TelegramMessageTarget {
        TelegramMessageTarget::chat(42)
    }

    #[derive(Default)]
    struct FakeTelegramApi {
        sent: Mutex<Vec<TelegramSendMessageRequest>>,
        edited: Mutex<Vec<TelegramEditMessageTextRequest>>,
        photos: Mutex<Vec<TelegramSendPhotoRequest>>,
    }

    impl FakeTelegramApi {
        fn sent_count(&self) -> usize {
            self.sent.lock().expect("fake sent lock poisoned").len()
        }

        fn edited_count(&self) -> usize {
            self.edited.lock().expect("fake edited lock poisoned").len()
        }

        fn photo_count(&self) -> usize {
            self.photos.lock().expect("fake photos lock poisoned").len()
        }

        fn edited_texts(&self) -> Vec<String> {
            self.edited
                .lock()
                .expect("fake edited lock poisoned")
                .iter()
                .map(|request| request.text.clone())
                .collect()
        }

        fn sent_has_reply_markup(&self) -> bool {
            self.sent
                .lock()
                .expect("fake sent lock poisoned")
                .iter()
                .any(|request| request.reply_markup.is_some())
        }

        fn sent_texts(&self) -> Vec<String> {
            self.sent
                .lock()
                .expect("fake sent lock poisoned")
                .iter()
                .map(|request| request.text.clone())
                .collect()
        }
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
                let mut sent = self.sent.lock().expect("fake sent lock poisoned");
                let message_id = sent.len() as i64 + 1;
                sent.push(request.clone());
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id,
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
                self.edited
                    .lock()
                    .expect("fake edited lock poisoned")
                    .push(request.clone());
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

        fn send_photo<'a>(
            &'a self,
            request: TelegramSendPhotoRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>,
        > {
            Box::pin(async move {
                let mut photos = self.photos.lock().expect("fake photos lock poisoned");
                let message_id = photos.len() as i64 + 10;
                photos.push(request.clone());
                Ok(TelegramMessageHandle {
                    chat_id: request.chat_id,
                    message_id,
                })
            })
        }
    }
}

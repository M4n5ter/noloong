use super::NoloongAppView;
use crate::chat::{ChatComposer, ChatComposerAction, ChatTranscriptItem, ChatTranscriptRole};
use crate::{
    AppInteractionStatus, AppTextKey, ChatEmptyState,
    interaction::{
        AppDisplaySubscribeRequest, AppInteractionClient as _, AppInteractionDisplayNotification,
        AppInteractionSessionDescriptor, AppInteractionSessionStatus, AppPromptInput,
        AppPromptRequest, AppSessionCreateRequest, InteractionUxCapabilities,
    },
};
use gpui::{
    AnyElement, AsyncApp, Context, IntoElement, KeyDownEvent, ParentElement as _, SharedString,
    Styled, WeakEntity, div, prelude::*, px, rgb,
};
use gpui_component::{StyledExt as _, input::Input};
use std::time::{SystemTime, UNIX_EPOCH};

impl NoloongAppView {
    pub(super) fn render_chat(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.model.chat_sessions().is_empty() {
            return self.render_chat_workspace(cx);
        }
        self.render_chat_empty(cx)
    }

    fn render_chat_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .justify_between()
            .size_full()
            .max_w(px(1040.0))
            .child(
                div().flex().flex_col().gap_5().pt_20().child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_4()
                                .child(self.logo_badge(px(42.0)))
                                .child(
                                    div()
                                        .text_2xl()
                                        .font_semibold()
                                        .text_color(rgb(0xe6edf3))
                                        .child(self.chat_empty_title()),
                                ),
                        )
                        .child(
                            div()
                                .max_w(px(560.0))
                                .text_base()
                                .text_color(rgb(0x8d99a6))
                                .child(self.chat_empty_subtitle()),
                        )
                        .child(
                            div()
                                .id("chat-empty-action")
                                .mt_2()
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(0x314458))
                                .bg(rgb(0x121e2a))
                                .px_4()
                                .py_2()
                                .text_sm()
                                .font_semibold()
                                .text_color(rgb(0xaecbff))
                                .child(self.chat_empty_action_label())
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.handle_chat_empty_action(cx);
                                })),
                        ),
                ),
            )
            .child(
                div()
                    .h(px(150.0))
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(0x3a4552))
                    .bg(rgb(0x101820))
                    .p_5()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(div().text_color(rgb(0x7d8793)).child(
                        if self.model.has_interaction_endpoint() {
                            self.catalog.text(AppTextKey::ChatComposerPlaceholder)
                        } else {
                            self.catalog.text(AppTextKey::ChatDisabled)
                        },
                    ))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x7d8793))
                                    .child(self.chat_connection_status_text()),
                            )
                            .child(
                                div()
                                    .rounded_full()
                                    .px_4()
                                    .py_2()
                                    .bg(rgb(0x263a61))
                                    .text_color(rgb(0x9aa7bb))
                                    .child(self.catalog.text(AppTextKey::ChatDisabled)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn handle_chat_empty_action(&mut self, cx: &mut Context<Self>) {
        match self.model.chat_empty_state() {
            ChatEmptyState::MissingConfig => {
                self.model.route = crate::AppRoute::Settings;
                cx.notify();
            }
            ChatEmptyState::NoSession => self.start_create_chat_session(cx),
            ChatEmptyState::Connecting | ChatEmptyState::ConnectionFailed(_) => {}
        }
    }

    fn start_create_chat_session(&mut self, cx: &mut Context<Self>) {
        let Some(endpoint) = self.model.interaction_endpoint.clone() else {
            return;
        };
        let client = match crate::AppInteractionHttpClient::from_endpoint(&endpoint) {
            Ok(client) => client,
            Err(error) => {
                self.model.interaction_status = AppInteractionStatus::Failed(error.to_string());
                cx.notify();
                return;
            }
        };
        let profile_id = self.model.selected_profile_id.clone();
        self.chat_refresh_task = cx.spawn(async move |this, cx| {
            let result = client
                .create_session(AppSessionCreateRequest {
                    session_id: None,
                    profile_id,
                    metadata: Default::default(),
                })
                .await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |this, cx| {
                match result {
                    Ok(session) => this.model.apply_chat_session_descriptor(session),
                    Err(error) => {
                        this.model.interaction_status =
                            AppInteractionStatus::Failed(error.to_string());
                    }
                }
                cx.notify();
            });
        });
    }

    fn render_chat_workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .gap_6()
            .size_full()
            .max_w(px(1120.0))
            .child(self.render_session_list(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .flex_col()
                    .justify_between()
                    .gap_5()
                    .child(self.render_transcript(cx))
                    .child(self.render_composer(cx)),
            )
            .into_any_element()
    }

    fn render_session_list(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(px(240.0))
            .flex()
            .flex_col()
            .gap_2()
            .rounded_xl()
            .border_1()
            .border_color(rgb(0x253545))
            .bg(rgb(0x0f1821))
            .p_3()
            .children(self.model.chat_sessions().iter().map(|session| {
                let is_current =
                    self.model.current_chat_session_id() == Some(session.session_id.as_str());
                let session_id = session.session_id.clone();
                let element_id = SharedString::from(format!("chat-session-{session_id}"));
                div()
                    .id(element_id)
                    .rounded_lg()
                    .border_1()
                    .border_color(if is_current {
                        rgb(0x456ca8)
                    } else {
                        rgb(0x182635)
                    })
                    .bg(if is_current {
                        rgb(0x172741)
                    } else {
                        rgb(0x101923)
                    })
                    .px_3()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this: &mut Self, _, _window, cx| {
                        this.model.select_chat_session(&session_id);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(rgb(0xe6edf3))
                            .child(session.title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x7d8793))
                            .child(session.profile_id.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(status_color(session.status))
                            .child(status_label(session.status)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn render_transcript(&self, cx: &mut Context<Self>) -> AnyElement {
        let now_ms = current_time_ms();
        div()
            .flex()
            .flex_1()
            .flex_col()
            .gap_4()
            .overflow_hidden()
            .children(self.model.chat_transcript().iter().map(|item| {
                if item.role == ChatTranscriptRole::Thought {
                    return self.render_thought_item(item, cx);
                }
                let is_user = item.role == ChatTranscriptRole::User;
                let content =
                    if let Some(streaming) = &item.streaming {
                        div()
                            .flex()
                            .flex_wrap()
                            .children(streaming.visible_segments(now_ms).into_iter().map(
                                |segment| {
                                    div()
                                        .opacity(segment.opacity)
                                        .child(segment.text)
                                        .into_any_element()
                                },
                            ))
                            .into_any_element()
                    } else {
                        div().child(item.text.clone()).into_any_element()
                    };
                div()
                    .flex()
                    .justify_end_when(is_user)
                    .child(
                        div()
                            .max_w(px(680.0))
                            .rounded_xl()
                            .border_1()
                            .border_color(if is_user {
                                rgb(0x315f4b)
                            } else {
                                rgb(0x27384a)
                            })
                            .bg(if is_user {
                                rgb(0x16362b)
                            } else {
                                rgb(0x101923)
                            })
                            .px_4()
                            .py_3()
                            .text_base()
                            .text_color(rgb(0xe6edf3))
                            .child(content),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn render_thought_item(&self, item: &ChatTranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let thought_id = item.message_id.clone();
        let element_id = SharedString::from(format!("thought-{thought_id}"));
        let thought = item.thought.as_ref();
        let is_completed = thought.is_some_and(|thought| thought.completed);
        let is_expanded = thought.is_some_and(|thought| thought.expanded || !thought.completed);
        let summary = thought
            .map(|thought| thought.summary.as_str())
            .unwrap_or("");
        let raw = thought.map(|thought| thought.raw.as_str()).unwrap_or("");
        let body = if !summary.is_empty() { summary } else { raw };
        let title = thought
            .and_then(|thought| {
                thought
                    .completed
                    .then(|| {
                        thought
                            .elapsed_ms
                            .map(|elapsed| self.catalog.thought_elapsed(elapsed))
                    })
                    .flatten()
            })
            .unwrap_or_else(|| self.catalog.text(AppTextKey::Thinking).to_string());
        div()
            .flex()
            .justify_start()
            .child(
                div()
                    .id(element_id)
                    .max_w(px(680.0))
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(0x29384b))
                    .bg(rgb(0x0d151f))
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        if this.model.toggle_thought_expanded(&thought_id) {
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .font_semibold()
                            .text_color(if is_completed {
                                rgb(0x8da2ba)
                            } else {
                                rgb(0xaecbff)
                            })
                            .child(title),
                    )
                    .when(is_expanded && !body.is_empty(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xb8c4d2))
                                .child(body.to_string()),
                        )
                    })
                    .when(is_expanded && !raw.is_empty() && summary != raw, |this| {
                        this.child(
                            div()
                                .rounded_lg()
                                .border_1()
                                .border_color(rgb(0x253545))
                                .bg(rgb(0x111b26))
                                .px_3()
                                .py_2()
                                .text_xs()
                                .text_color(rgb(0x7f91a7))
                                .child(raw.to_string()),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut composer = ChatComposer::default();
        composer.set_text(self.chat_input.read(cx).value().to_string());
        let can_send = self.model.has_interaction_endpoint() && composer.can_send();
        div()
            .min_h(px(128.0))
            .rounded_xl()
            .border_1()
            .border_color(rgb(0x3a4552))
            .bg(rgb(0x101820))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" && !event.keystroke.modifiers.shift {
                    this.submit_chat_input(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div().flex_1().child(
                    Input::new(&self.chat_input)
                        .w_full()
                        .h_full()
                        .rounded_lg()
                        .border_0()
                        .bg(rgb(0x101820))
                        .text_color(rgb(0xe6edf3)),
                ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x7d8793))
                            .child(self.chat_connection_status_text()),
                    )
                    .child(
                        div()
                            .id("chat-send-button")
                            .rounded_full()
                            .px_4()
                            .py_2()
                            .bg(if can_send {
                                rgb(0x2d5f9f)
                            } else {
                                rgb(0x263a61)
                            })
                            .text_color(if can_send {
                                rgb(0xf1f6fb)
                            } else {
                                rgb(0x9aa7bb)
                            })
                            .opacity(if can_send { 1.0 } else { 0.55 })
                            .when(can_send, |this| {
                                this.cursor_pointer().hover(|style| style.bg(rgb(0x386fb8)))
                            })
                            .child("↑")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_chat_input(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn submit_chat_input(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let mut composer = ChatComposer::default();
        composer.set_text(self.chat_input.read(cx).value().to_string());
        let ChatComposerAction::Submit(text) = composer.press_enter(false) else {
            return;
        };
        self.chat_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.start_submit_chat_message(text, cx);
        cx.notify();
    }

    fn start_submit_chat_message(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(endpoint) = self.model.interaction_endpoint.clone() else {
            return;
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let current_session_id = self.model.current_chat_session_id().map(str::to_string);
        let profile_id = self.model.selected_profile_id.clone();
        self.chat_run_task = cx.spawn(async move |this, cx| {
            let client = match crate::interaction::AppInteractionWsClient::connect(&endpoint).await
            {
                Ok(client) => client,
                Err(error) => {
                    update_chat_error(this, cx, error.to_string());
                    return;
                }
            };
            let mut notifications = client.subscribe_notifications();
            let session_id = match current_session_id {
                Some(session_id) => session_id,
                None => match client
                    .create_session(AppSessionCreateRequest {
                        session_id: None,
                        profile_id,
                        metadata: Default::default(),
                    })
                    .await
                {
                    Ok(session) => {
                        let session_id = session.session_id.clone();
                        update_chat_session(this.clone(), cx, session);
                        session_id
                    }
                    Err(error) => {
                        update_chat_error(this, cx, error.to_string());
                        return;
                    }
                },
            };
            if let Err(error) = client
                .subscribe_display(AppDisplaySubscribeRequest {
                    session_id: session_id.clone(),
                    ux: Some(InteractionUxCapabilities {
                        display_events: true,
                        stream_text: true,
                        edit_message: true,
                        markdown: true,
                        max_message_bytes: Some(65_536),
                    }),
                })
                .await
            {
                update_chat_error(this, cx, error.to_string());
                return;
            }
            let prompt = client.prompt(AppPromptRequest {
                session_id,
                input: AppPromptInput::Text { text },
            });
            tokio::pin!(prompt);
            loop {
                tokio::select! {
                    result = &mut prompt => {
                        match result {
                            Ok(session) => update_chat_session(this.clone(), cx, session),
                            Err(error) => update_chat_error(this.clone(), cx, error.to_string()),
                        }
                        break;
                    }
                    notification = notifications.recv() => {
                        match notification {
                            Ok(notification) => {
                                if let Ok(display) = notification.display_event() {
                                    update_chat_display(this.clone(), cx, display);
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        });
    }

    fn chat_empty_title(&self) -> String {
        match self.model.chat_empty_state() {
            ChatEmptyState::MissingConfig => self.catalog.text(AppTextKey::ChatMissingConfigTitle),
            ChatEmptyState::Connecting => self.catalog.text(AppTextKey::ChatConnecting),
            ChatEmptyState::ConnectionFailed(_) => {
                self.catalog.text(AppTextKey::ChatConnectionFailedTitle)
            }
            ChatEmptyState::NoSession => self.catalog.text(AppTextKey::ChatEmptyNoSessionTitle),
        }
        .to_string()
    }

    fn chat_empty_subtitle(&self) -> String {
        match self.model.chat_empty_state() {
            ChatEmptyState::MissingConfig => {
                self.catalog.text(AppTextKey::ChatMissingConfigSubtitle)
            }
            ChatEmptyState::Connecting => self.catalog.text(AppTextKey::ChatComposerPlaceholder),
            ChatEmptyState::ConnectionFailed(error) => return error,
            ChatEmptyState::NoSession => self.catalog.text(AppTextKey::ChatEmptyNoSessionSubtitle),
        }
        .to_string()
    }

    fn chat_empty_action_label(&self) -> String {
        match self.model.chat_empty_state() {
            ChatEmptyState::MissingConfig => self.catalog.text(AppTextKey::ChatOpenSettingsAction),
            ChatEmptyState::Connecting | ChatEmptyState::ConnectionFailed(_) => {
                self.catalog.text(AppTextKey::ChatConnecting)
            }
            ChatEmptyState::NoSession => self.catalog.text(AppTextKey::ChatNewSessionAction),
        }
        .to_string()
    }

    fn chat_connection_status_text(&self) -> String {
        match &self.model.interaction_status {
            AppInteractionStatus::Unavailable => {
                self.catalog.text(AppTextKey::ChatDisabled).to_string()
            }
            AppInteractionStatus::Pending => {
                self.catalog.text(AppTextKey::ChatConnecting).to_string()
            }
            AppInteractionStatus::Ready { server_name, .. } => format!(
                "{server_name} · {}",
                self.catalog.text(AppTextKey::ChatConnected)
            ),
            AppInteractionStatus::Failed(error) => error.clone(),
        }
    }

    pub(super) fn render_placeholder(&self, key: AppTextKey) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .text_lg()
            .text_color(rgb(0xaab4c0))
            .child(self.catalog.text(key))
    }
}

trait ChatElementExt {
    fn justify_end_when(self, enabled: bool) -> Self;
}

impl<E> ChatElementExt for E
where
    E: Styled,
{
    fn justify_end_when(self, enabled: bool) -> Self {
        if enabled { self.justify_end() } else { self }
    }
}

fn status_label(status: AppInteractionSessionStatus) -> &'static str {
    match status {
        AppInteractionSessionStatus::Idle => "idle",
        AppInteractionSessionStatus::Running => "running",
        AppInteractionSessionStatus::Completed => "completed",
        AppInteractionSessionStatus::Aborted => "aborted",
        AppInteractionSessionStatus::Failed => "failed",
        AppInteractionSessionStatus::Paused => "paused",
    }
}

fn status_color(status: AppInteractionSessionStatus) -> gpui::Rgba {
    match status {
        AppInteractionSessionStatus::Running => rgb(0x93c5fd),
        AppInteractionSessionStatus::Paused => rgb(0xf8c76a),
        AppInteractionSessionStatus::Failed | AppInteractionSessionStatus::Aborted => rgb(0xff8b8b),
        AppInteractionSessionStatus::Idle | AppInteractionSessionStatus::Completed => rgb(0x7d8793),
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn update_chat_session(
    this: WeakEntity<NoloongAppView>,
    cx: &mut AsyncApp,
    session: AppInteractionSessionDescriptor,
) {
    let Some(this) = this.upgrade() else {
        return;
    };
    this.update(cx, |this, cx| {
        this.model.apply_chat_session_descriptor(session);
        cx.notify();
    });
}

fn update_chat_display(
    this: WeakEntity<NoloongAppView>,
    cx: &mut AsyncApp,
    display: AppInteractionDisplayNotification,
) {
    let Some(this) = this.upgrade() else {
        return;
    };
    this.update(cx, |this, cx| {
        this.model.apply_display_notification(display);
        cx.notify();
    });
}

fn update_chat_error(this: WeakEntity<NoloongAppView>, cx: &mut AsyncApp, error: String) {
    let Some(this) = this.upgrade() else {
        return;
    };
    this.update(cx, |this, cx| {
        this.model.interaction_status = AppInteractionStatus::Failed(error);
        cx.notify();
    });
}

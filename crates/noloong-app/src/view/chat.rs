use super::NoloongAppView;
use crate::chat::ChatTranscriptRole;
use crate::{
    AppInteractionStatus, AppTextKey, ChatEmptyState,
    interaction::{
        AppInteractionClient as _, AppInteractionSessionStatus, AppSessionCreateRequest,
    },
};
use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled, div, prelude::*,
    px, rgb,
};
use gpui_component::StyledExt as _;

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
                    .child(self.render_transcript())
                    .child(self.render_composer()),
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

    fn render_transcript(&self) -> AnyElement {
        div()
            .flex()
            .flex_1()
            .flex_col()
            .gap_4()
            .overflow_hidden()
            .children(self.model.chat_transcript().iter().map(|item| {
                let is_user = item.role == ChatTranscriptRole::User;
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
                            .child(item.text.clone()),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn render_composer(&self) -> AnyElement {
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
            .child(
                div()
                    .text_color(rgb(0x7d8793))
                    .child(self.catalog.text(AppTextKey::ChatComposerPlaceholder)),
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
                            .rounded_full()
                            .px_4()
                            .py_2()
                            .bg(rgb(0x263a61))
                            .text_color(rgb(0x9aa7bb))
                            .child(self.catalog.text(AppTextKey::ChatDisabled)),
                    ),
            )
            .into_any_element()
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

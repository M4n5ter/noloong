use super::NoloongAppView;
use crate::{AppInteractionStatus, AppTextKey, ChatEmptyState};
use gpui::{IntoElement, ParentElement as _, Styled as _, div, px, rgb};
use gpui_component::StyledExt as _;

impl NoloongAppView {
    pub(super) fn render_chat(&self) -> impl IntoElement {
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
                                .child(self.chat_empty_action_label()),
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

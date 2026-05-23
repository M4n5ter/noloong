use super::NoloongAppView;
use crate::chat::{
    ChatAttachmentDraft, ChatComposer, ChatComposerAction, ChatComposerSubmission, ChatRunStatus,
};
use crate::{
    AppInteractionStatus, AppTextKey, ChatEmptyState,
    interaction::{
        AppApprovalResolveRequest, AppDisplaySubscribeRequest, AppInteractionClient as _,
        AppInteractionDisplayNotification, AppInteractionSessionDescriptor, AppPromptInput,
        AppPromptRequest, AppSessionCreateRequest, AppSessionRequest, AppToolPermissionDecision,
        AppToolPermissionOutcome, InteractionUxCapabilities,
    },
};
use gpui::{
    AnyElement, AsyncApp, Context, ExternalPaths, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, PathPromptOptions, Styled, WeakEntity, div, prelude::*, px,
    rgb,
};
use gpui_component::{StyledExt as _, input::Input};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

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

    fn render_transcript(&self, cx: &mut Context<Self>) -> AnyElement {
        let now_ms = current_time_ms();
        div()
            .flex()
            .flex_1()
            .flex_col()
            .gap_4()
            .overflow_hidden()
            .children(
                self.model
                    .chat_transcript()
                    .iter()
                    .map(|item| self.render_transcript_item(item, now_ms, cx)),
            )
            .into_any_element()
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut composer = ChatComposer::default();
        composer.set_text(self.chat_input.read(cx).value().to_string());
        composer.set_attachments(self.chat_attachments.clone());
        let attachments_supported = self.chat_attachments_supported();
        let can_abort = self.model.can_abort_current_chat_run();
        let can_send = self.model.has_interaction_endpoint()
            && self.model.can_send_chat_message()
            && composer.can_send();
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
                    if !this.model.can_abort_current_chat_run() {
                        this.submit_chat_input(window, cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .when(attachments_supported, |this| {
                this.can_drop(|dragged, _, _| dragged.is::<ExternalPaths>())
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                        this.add_chat_attachment_paths(paths.paths().iter().cloned(), cx);
                    }))
            })
            .when(!composer.attachments().is_empty(), |this| {
                this.child(self.render_chat_attachment_chips(cx))
            })
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
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x7d8793))
                                    .child(self.chat_connection_status_text()),
                            )
                            .child(self.render_chat_workdir_button(cx))
                            .when(attachments_supported, |this| {
                                this.child(
                                    div()
                                        .id("chat-attach-button")
                                        .rounded_full()
                                        .border_1()
                                        .border_color(rgb(0x304052))
                                        .bg(rgb(0x121c27))
                                        .px_3()
                                        .py_1()
                                        .text_sm()
                                        .text_color(rgb(0xaecbff))
                                        .cursor_pointer()
                                        .hover(|style| style.bg(rgb(0x172638)))
                                        .child("+")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.start_pick_chat_attachments(window, cx);
                                        })),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("chat-send-button")
                            .rounded_full()
                            .px_4()
                            .py_2()
                            .bg(if can_abort {
                                rgb(0x66323a)
                            } else if can_send {
                                rgb(0x2d5f9f)
                            } else {
                                rgb(0x263a61)
                            })
                            .text_color(if can_abort || can_send {
                                rgb(0xf1f6fb)
                            } else {
                                rgb(0x9aa7bb)
                            })
                            .opacity(if can_abort || can_send { 1.0 } else { 0.55 })
                            .when(can_abort || can_send, |this| {
                                this.cursor_pointer().hover(|style| {
                                    if can_abort {
                                        style.bg(rgb(0x7a3d47))
                                    } else {
                                        style.bg(rgb(0x386fb8))
                                    }
                                })
                            })
                            .child(if can_abort { "■" } else { "↑" })
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.model.can_abort_current_chat_run() {
                                    this.start_abort_chat_run(cx);
                                } else {
                                    this.submit_chat_input(window, cx);
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn chat_attachments_supported(&self) -> bool {
        self.model.file_data_url_input().unwrap_or(false)
    }

    fn render_chat_attachment_chips(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children(
                self.chat_attachments
                    .iter()
                    .enumerate()
                    .map(|(index, attachment)| {
                        self.render_chat_attachment_chip(index, attachment, cx)
                    }),
            )
            .into_any_element()
    }

    fn render_chat_attachment_chip(
        &self,
        index: usize,
        attachment: &ChatAttachmentDraft,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let attachment_id = attachment.id.clone();
        div()
            .flex()
            .items_center()
            .gap_2()
            .max_w(px(260.0))
            .rounded_full()
            .border_1()
            .border_color(rgb(0x304052))
            .bg(rgb(0x132131))
            .px_3()
            .py_1()
            .text_sm()
            .text_color(rgb(0xcbd7e3))
            .child(div().truncate().child(attachment.file_name.clone()))
            .child(
                div()
                    .id(("chat-remove-attachment", index))
                    .rounded_full()
                    .px_1()
                    .text_color(rgb(0x8ea1b6))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(0xf1b4b4)))
                    .child("×")
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.remove_chat_attachment(&attachment_id, cx);
                    })),
            )
            .into_any_element()
    }

    fn start_pick_chat_attachments(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) {
        if !self.chat_attachments_supported() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(self.catalog.text(AppTextKey::AttachFiles).into()),
        });
        self.chat_attachment_task = cx.spawn(async move |this, cx| {
            let paths = match receiver.await {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.add_chat_attachment_paths(paths, cx);
            });
        });
    }

    fn add_chat_attachment_paths(
        &mut self,
        paths: impl IntoIterator<Item = PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if !self.chat_attachments_supported() {
            return;
        }
        let mut composer = ChatComposer::default();
        composer.set_attachments(std::mem::take(&mut self.chat_attachments));
        let mut first_error = None;
        for path in paths {
            if let Err(error) = composer.add_attachment_path(path) {
                first_error.get_or_insert_with(|| error.to_string());
            }
        }
        self.chat_attachments = composer.into_attachments();
        if let Some(error) = first_error {
            self.show_toast(
                format!(
                    "{}: {error}",
                    self.catalog.text(AppTextKey::AttachmentRejected)
                ),
                super::ToastTone::Error,
                cx,
            );
        }
        cx.notify();
    }

    fn remove_chat_attachment(&mut self, attachment_id: &str, cx: &mut Context<Self>) {
        let mut composer = ChatComposer::default();
        composer.set_attachments(std::mem::take(&mut self.chat_attachments));
        composer.remove_attachment(attachment_id);
        self.chat_attachments = composer.into_attachments();
        cx.notify();
    }

    fn submit_chat_input(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let mut composer = ChatComposer::default();
        composer.set_text(self.chat_input.read(cx).value().to_string());
        composer.set_attachments(self.chat_attachments.clone());
        let ChatComposerAction::Submit(submission) = composer.press_enter(false) else {
            return;
        };
        if !self.model.has_interaction_endpoint() || !self.model.can_send_chat_message() {
            return;
        }
        self.chat_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.chat_attachments.clear();
        self.start_submit_chat_message(submission, cx);
        cx.notify();
    }

    fn start_abort_chat_run(&mut self, cx: &mut Context<Self>) {
        let Some(endpoint) = self.model.interaction_endpoint.clone() else {
            return;
        };
        let Some(session_id) = self.model.current_chat_session_id().map(str::to_string) else {
            return;
        };
        self.chat_abort_task = cx.spawn(async move |this, cx| {
            let client = match crate::interaction::AppInteractionWsClient::connect(&endpoint).await
            {
                Ok(client) => client,
                Err(error) => {
                    update_chat_error(this, cx, error.to_string());
                    return;
                }
            };
            match client.abort(AppSessionRequest { session_id }).await {
                Ok(session) => update_chat_session(this, cx, session),
                Err(error) => update_chat_error(this, cx, error.to_string()),
            }
        });
    }

    pub(super) fn start_resolve_chat_approval(
        &mut self,
        approval_id: String,
        outcome: AppToolPermissionOutcome,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = self.model.interaction_endpoint.clone() else {
            return;
        };
        let Some(session_id) = self.model.current_chat_session_id().map(str::to_string) else {
            return;
        };
        self.chat_approval_task = cx.spawn(async move |this, cx| {
            let client = match crate::interaction::AppInteractionWsClient::connect(&endpoint).await
            {
                Ok(client) => client,
                Err(error) => {
                    update_chat_error(this, cx, error.to_string());
                    return;
                }
            };
            let request = AppApprovalResolveRequest {
                session_id,
                approval_id: approval_id.clone(),
                decision: AppToolPermissionDecision::from_outcome(outcome),
            };
            match client.resolve_approval(request).await {
                Ok(session) => {
                    let Some(this) = this.upgrade() else {
                        return;
                    };
                    this.update(cx, |this, cx| {
                        this.model
                            .apply_chat_approval_resolution(&approval_id, outcome, session);
                        cx.notify();
                    });
                }
                Err(error) => update_chat_error(this, cx, error.to_string()),
            }
        });
    }

    fn start_submit_chat_message(
        &mut self,
        submission: ChatComposerSubmission,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = self.model.interaction_endpoint.clone() else {
            return;
        };
        let input = submission.into_prompt_input(format!("app-user-{}", current_time_ms()));
        if matches!(&input, AppPromptInput::Text { text } if text.trim().is_empty()) {
            return;
        }
        let create_metadata = self.model.chat_session_metadata_for_prompt(&input);
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
                        metadata: create_metadata,
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
            let prompt = client.prompt(AppPromptRequest { session_id, input });
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

    pub(super) fn chat_connection_status_text(&self) -> String {
        if let Some(error) = self.model.chat_connection_error() {
            return error.to_string();
        }
        if let Some(run) = self.model.current_chat_run() {
            return match run.status {
                ChatRunStatus::Running => self.catalog.text(AppTextKey::ChatRunRunning).into(),
                ChatRunStatus::Paused => self.catalog.text(AppTextKey::ChatRunPaused).into(),
                ChatRunStatus::Completed => self.catalog.text(AppTextKey::ChatRunCompleted).into(),
                ChatRunStatus::Aborted => self.catalog.text(AppTextKey::ChatRunStopped).into(),
                ChatRunStatus::Failed => run
                    .error
                    .as_ref()
                    .map(|error| {
                        format!("{} · {error}", self.catalog.text(AppTextKey::ChatRunFailed))
                    })
                    .unwrap_or_else(|| self.catalog.text(AppTextKey::ChatRunFailed).into()),
            };
        }
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
        this.model.record_chat_connection_error(error);
        cx.notify();
    });
}

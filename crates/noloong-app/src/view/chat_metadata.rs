use super::{NoloongAppView, ToastTone};
use crate::chat::{ChatSessionSummary, SESSION_TITLE_METADATA_KEY, SESSION_WORKDIR_METADATA_KEY};
use crate::{
    AppTextKey,
    interaction::{
        AppInteractionClient as _, AppInteractionSessionDescriptor, AppInteractionSessionStatus,
        AppSessionMetadataUpdateRequest,
    },
};
use gpui::{
    AnyElement, AsyncApp, Context, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, PathPromptOptions, SharedString, Styled, WeakEntity, div, prelude::*, px,
    rgb,
};
use gpui_component::{StyledExt as _, input::Input};
use std::path::{Path, PathBuf};

impl NoloongAppView {
    pub(super) fn render_session_list(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    .child(self.render_session_title(session, is_current, cx))
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

    pub(super) fn render_chat_workdir_button(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("chat-workdir-button")
            .max_w(px(220.0))
            .rounded_full()
            .border_1()
            .border_color(rgb(0x304052))
            .bg(rgb(0x121c27))
            .px_3()
            .py_1()
            .text_sm()
            .text_color(rgb(0x9fb4ca))
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(rgb(0x172638))
                    .text_color(rgb(0xd6e7fb))
                    .border_color(rgb(0x436280))
            })
            .child(div().truncate().child(format!(
                "{} · {}",
                self.catalog.text(AppTextKey::Cwd),
                short_workdir_label(self.model.chat_workdir())
            )))
            .on_click(cx.listener(|this, _, window, cx| {
                this.start_pick_chat_workdir(window, cx);
            }))
            .into_any_element()
    }

    fn render_session_title(
        &self,
        session: &ChatSessionSummary,
        is_current: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.chat_renaming_session_id.as_deref() == Some(session.session_id.as_str()) {
            return div()
                .flex()
                .items_center()
                .gap_1()
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                    match event.keystroke.key.as_str() {
                        "enter" => {
                            this.commit_chat_session_rename(cx);
                            cx.stop_propagation();
                        }
                        "escape" => {
                            this.chat_renaming_session_id = None;
                            cx.notify();
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                }))
                .child(
                    Input::new(&self.chat_rename_input)
                        .h(px(28.0))
                        .flex_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x42669a))
                        .bg(rgb(0x101820))
                        .text_color(rgb(0xe6edf3)),
                )
                .child(self.rename_icon_button(
                    "commit-rename",
                    "✓",
                    cx.listener(|this, _, _window, cx| {
                        this.commit_chat_session_rename(cx);
                        cx.stop_propagation();
                    }),
                ))
                .child(self.rename_icon_button(
                    "cancel-rename",
                    "×",
                    cx.listener(|this, _, _window, cx| {
                        this.chat_renaming_session_id = None;
                        cx.notify();
                        cx.stop_propagation();
                    }),
                ))
                .into_any_element();
        }

        let session_id = session.session_id.clone();
        let title = session.title.clone();
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .text_color(rgb(0xe6edf3))
                    .child(title.clone()),
            )
            .when(is_current, |this| {
                this.child(self.rename_icon_button(
                    "start-rename",
                    "✎",
                    cx.listener(move |this, _, window, cx| {
                        this.chat_renaming_session_id = Some(session_id.clone());
                        this.chat_rename_input
                            .update(cx, |input, cx| input.set_value(title.clone(), window, cx));
                        cx.notify();
                        cx.stop_propagation();
                    }),
                ))
            })
            .into_any_element()
    }

    fn rename_icon_button(
        &self,
        id: &'static str,
        label: &'static str,
        handler: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.0))
            .rounded_full()
            .bg(rgb(0x172638))
            .text_xs()
            .font_semibold()
            .text_color(rgb(0xaecbff))
            .hover(|style| style.bg(rgb(0x22395a)).text_color(rgb(0xf1f6fb)))
            .cursor_pointer()
            .child(label)
            .on_click(handler)
    }

    fn start_pick_chat_workdir(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.catalog.text(AppTextKey::SelectWorkdir).into()),
        });
        self.chat_metadata_task = cx.spawn(async move |this, cx| {
            let paths = match receiver.await {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            let Some(workdir) = paths.into_iter().next() else {
                return;
            };
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.apply_chat_workdir(workdir, cx);
            });
        });
    }

    fn apply_chat_workdir(&mut self, workdir: PathBuf, cx: &mut Context<Self>) {
        if self.model.can_abort_current_chat_run() {
            self.show_toast(
                self.catalog.text(AppTextKey::WorkdirBusy),
                ToastTone::Error,
                cx,
            );
            return;
        }
        if self.model.current_chat_session_id().is_some() {
            self.start_update_current_chat_workdir(workdir, cx);
            return;
        }
        self.model.set_chat_workdir(workdir);
        self.show_toast(
            self.catalog.text(AppTextKey::WorkdirUpdated),
            ToastTone::Success,
            cx,
        );
        cx.notify();
    }

    fn commit_chat_session_rename(&mut self, cx: &mut Context<Self>) {
        let title = self.chat_rename_input.read(cx).value().trim().to_string();
        if title.is_empty() {
            return;
        }
        let Some(session_id) = self.chat_renaming_session_id.clone() else {
            return;
        };
        self.chat_renaming_session_id = None;
        self.start_rename_chat_session(session_id, title, cx);
    }

    fn start_rename_chat_session(
        &mut self,
        session_id: String,
        title: String,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = self.model.interaction_endpoint.clone() else {
            return;
        };
        self.chat_metadata_task = cx.spawn(async move |this, cx| {
            let client = match crate::AppInteractionHttpClient::from_endpoint(&endpoint) {
                Ok(client) => client,
                Err(error) => {
                    update_chat_error(this, cx, error.to_string());
                    return;
                }
            };
            let result = client
                .update_session_metadata(AppSessionMetadataUpdateRequest {
                    session_id,
                    metadata: [(SESSION_TITLE_METADATA_KEY.into(), serde_json::json!(title))]
                        .into_iter()
                        .collect(),
                })
                .await;
            match result {
                Ok(session) => update_chat_session(this, cx, session),
                Err(error) => update_chat_error(this, cx, error.to_string()),
            }
        });
    }

    fn start_update_current_chat_workdir(&mut self, workdir: PathBuf, cx: &mut Context<Self>) {
        let Some(endpoint) = self.model.interaction_endpoint.clone() else {
            self.model.set_chat_workdir(workdir);
            cx.notify();
            return;
        };
        let Some(session_id) = self.model.current_chat_session_id().map(str::to_string) else {
            self.model.set_chat_workdir(workdir);
            cx.notify();
            return;
        };
        self.chat_metadata_task = cx.spawn(async move |this, cx| {
            let client = match crate::AppInteractionHttpClient::from_endpoint(&endpoint) {
                Ok(client) => client,
                Err(error) => {
                    update_chat_error(this, cx, error.to_string());
                    return;
                }
            };
            let result = client
                .update_session_metadata(AppSessionMetadataUpdateRequest {
                    session_id,
                    metadata: [(
                        SESSION_WORKDIR_METADATA_KEY.into(),
                        serde_json::json!(workdir.display().to_string()),
                    )]
                    .into_iter()
                    .collect(),
                })
                .await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |this, cx| {
                match result {
                    Ok(session) => {
                        this.model.set_chat_workdir(workdir);
                        this.model.apply_chat_session_descriptor(session);
                        this.show_toast(
                            this.catalog.text(AppTextKey::WorkdirUpdated),
                            ToastTone::Success,
                            cx,
                        );
                    }
                    Err(error) => this.model.record_chat_connection_error(error.to_string()),
                }
                cx.notify();
            });
        });
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

fn short_workdir_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
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

fn update_chat_error(this: WeakEntity<NoloongAppView>, cx: &mut AsyncApp, error: String) {
    let Some(this) = this.upgrade() else {
        return;
    };
    this.update(cx, |this, cx| {
        this.model.record_chat_connection_error(error);
        cx.notify();
    });
}

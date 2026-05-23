use super::NoloongAppView;
use crate::chat::{ChatApprovalStatus, ChatToolActivity, ChatTranscriptItem, ChatTranscriptRole};
use crate::{AppTextKey, interaction::AppToolPermissionOutcome};
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    div, prelude::*, px, rgb,
};
use gpui_component::{StyledExt as _, scroll::ScrollableElement as _};

impl NoloongAppView {
    pub(super) fn render_transcript_item(
        &self,
        item: &ChatTranscriptItem,
        now_ms: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match item.role() {
            ChatTranscriptRole::Thought => return self.render_thought_item(item, cx),
            ChatTranscriptRole::Tool => return self.render_tool_item(item, cx),
            ChatTranscriptRole::Approval => return self.render_approval_item(item, cx),
            ChatTranscriptRole::User | ChatTranscriptRole::Assistant => {}
        }

        let is_user = item.role() == ChatTranscriptRole::User;
        let content = if let Some(streaming) = item.streaming() {
            div()
                .flex()
                .flex_wrap()
                .children(
                    streaming
                        .visible_segments(now_ms)
                        .into_iter()
                        .map(|segment| {
                            div()
                                .opacity(segment.opacity)
                                .child(segment.text)
                                .into_any_element()
                        }),
                )
                .into_any_element()
        } else {
            div().child(item.text()).into_any_element()
        };
        let row = div().flex();
        let row = if is_user { row.justify_end() } else { row };
        row.child(
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
    }

    fn render_tool_item(&self, item: &ChatTranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let Some(tool) = item.tool() else {
            return div().into_any_element();
        };
        let tool_call_id = tool.tool_call_id.clone();
        let title = if tool.completed {
            self.catalog.text(AppTextKey::ToolCompleted)
        } else {
            self.catalog.text(AppTextKey::ToolRunning)
        };
        div()
            .flex()
            .justify_start()
            .child(
                div()
                    .id(SharedString::from(format!("tool-{tool_call_id}")))
                    .max_w(px(680.0))
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(0x27384a))
                    .bg(rgb(0x0d151f))
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        if this.model.toggle_tool_expanded(&tool_call_id) {
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
                            .text_color(if tool.completed {
                                rgb(0x8da2ba)
                            } else {
                                rgb(0xaecbff)
                            })
                            .child(title)
                            .child("·")
                            .child(tool.tool_name.clone()),
                    )
                    .when(tool.expanded, |this| {
                        this.child(self.render_tool_details(tool))
                    }),
            )
            .into_any_element()
    }

    fn render_tool_details(&self, tool: &ChatToolActivity) -> AnyElement {
        let updates = tool.update_text();
        let output = tool.output.as_ref();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(!updates.is_empty(), |this| {
                this.child(self.render_tool_text_block(
                    self.catalog.text(AppTextKey::ToolUpdates),
                    updates,
                    false,
                ))
            })
            .when_some(output, |this, output| {
                let label = if output.is_error {
                    self.catalog.text(AppTextKey::ToolError)
                } else {
                    self.catalog.text(AppTextKey::ToolOutput)
                };
                this.child(self.render_tool_text_block(
                    label,
                    output.preview_text(),
                    output.is_error,
                ))
            })
            .into_any_element()
    }

    fn render_tool_text_block(
        &self,
        label: &'static str,
        text: String,
        is_error: bool,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(if is_error {
                        rgb(0xffa5ad)
                    } else {
                        rgb(0x7f91a7)
                    })
                    .child(label),
            )
            .child(
                div()
                    .max_h(px(220.0))
                    .overflow_y_scrollbar()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x253545))
                    .bg(rgb(0x111b26))
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(if is_error {
                        rgb(0xffc0c6)
                    } else {
                        rgb(0xb8c4d2)
                    })
                    .child(text),
            )
            .into_any_element()
    }

    fn render_approval_item(
        &self,
        item: &ChatTranscriptItem,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(approval) = item.approval() else {
            return div().into_any_element();
        };
        let approve_id = approval.approval_id.clone();
        let reject_id = approval.approval_id.clone();
        let status_text = match approval.status {
            ChatApprovalStatus::Pending => self.catalog.text(AppTextKey::ApprovalRequired),
            ChatApprovalStatus::Allowed => self.catalog.text(AppTextKey::ApprovalApproved),
            ChatApprovalStatus::Denied => self.catalog.text(AppTextKey::ApprovalRejected),
        };
        div()
            .flex()
            .justify_start()
            .child(
                div()
                    .id(SharedString::from(format!(
                        "approval-{}",
                        approval.approval_id
                    )))
                    .max_w(px(680.0))
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(0x5d4b2a))
                    .bg(rgb(0x17140d))
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .font_semibold()
                            .text_color(rgb(0xf8c76a))
                            .child(status_text)
                            .child("·")
                            .child(approval.tool_name.clone()),
                    )
                    .when_some(approval.prompt.clone(), |this, prompt| {
                        this.child(div().text_sm().text_color(rgb(0xd8c7a0)).child(prompt))
                    })
                    .when_some(approval.reason.clone(), |this, reason| {
                        this.child(div().text_xs().text_color(rgb(0xa9946a)).child(reason))
                    })
                    .when(approval.status == ChatApprovalStatus::Pending, |this| {
                        this.child(
                            div()
                                .flex()
                                .gap_2()
                                .child(self.render_approval_button(
                                    SharedString::from(format!("approval-approve-{approve_id}")),
                                    self.catalog.text(AppTextKey::Approve),
                                    true,
                                    approve_id,
                                    AppToolPermissionOutcome::Allow,
                                    cx,
                                ))
                                .child(self.render_approval_button(
                                    SharedString::from(format!("approval-reject-{reject_id}")),
                                    self.catalog.text(AppTextKey::Reject),
                                    false,
                                    reject_id,
                                    AppToolPermissionOutcome::Deny,
                                    cx,
                                )),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_approval_button(
        &self,
        id: SharedString,
        label: &'static str,
        positive: bool,
        approval_id: String,
        outcome: AppToolPermissionOutcome,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .rounded_full()
            .border_1()
            .border_color(if positive {
                rgb(0x315f4b)
            } else {
                rgb(0x5f3138)
            })
            .bg(if positive {
                rgb(0x153626)
            } else {
                rgb(0x36151d)
            })
            .px_3()
            .py_1()
            .text_sm()
            .font_semibold()
            .text_color(if positive {
                rgb(0xadf0c7)
            } else {
                rgb(0xffb4bc)
            })
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.start_resolve_chat_approval(approval_id.clone(), outcome, cx);
            }))
            .into_any_element()
    }

    fn render_thought_item(&self, item: &ChatTranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let thought_id = item.message_id.clone();
        let element_id = SharedString::from(format!("thought-{thought_id}"));
        let thought = item.thought();
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
}

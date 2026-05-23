use super::{NoloongAppView, ToastTone};
use crate::AppTextKey;
use gpui::{
    App, ClipboardItem, Context, IntoElement, ParentElement as _, Styled as _, Window, div,
    prelude::*, px, relative, rgb,
};
use gpui_component::{StyledExt as _, input::Input};

impl NoloongAppView {
    pub(super) fn render_jsonc_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .mx_auto()
            .size_full()
            .max_w(px(980.0))
            .rounded(px(20.0))
            .border_1()
            .border_color(if self.model.jsonc_error().is_some() {
                rgb(0x8b4f52)
            } else {
                rgb(0x3a4552)
            })
            .bg(rgb(0x0f1720))
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(self.jsonc_header(cx))
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    Input::new(&self.jsonc_input)
                        .appearance(false)
                        .size_full()
                        .text_sm()
                        .line_height(relative(1.35))
                        .text_color(rgb(0xc9d1d9)),
                ),
            )
            .when_some(self.model.jsonc_error(), |this, error| {
                this.child(self.jsonc_error_bar(error))
            })
    }

    fn jsonc_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_6()
            .py_5()
            .border_b_1()
            .border_color(rgb(0x26303a))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .font_semibold()
                            .text_color(rgb(0xe6edf3))
                            .child(self.catalog.text(AppTextKey::JsoncEditor)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x84909d))
                            .child(self.catalog.text(AppTextKey::JsoncEditorSubtitle)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        self.header_link("jsonc-copy", self.catalog.text(AppTextKey::Copy), {
                            let text = self.model.jsonc_text.clone();
                            cx.listener(move |_this, _, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                            })
                        }),
                    )
                    .child(self.header_link(
                        "jsonc-format",
                        self.catalog.text(AppTextKey::Format),
                        cx.listener(|this, _, window, cx| {
                            match this.model.format_jsonc() {
                                Ok(()) => {
                                    this.sync_jsonc_input(window, cx);
                                    this.sync_settings_inputs(window, cx);
                                }
                                Err(error) => {
                                    this.show_toast(
                                        format!(
                                            "{}: {error}",
                                            this.catalog.text(AppTextKey::JsoncInvalid)
                                        ),
                                        ToastTone::Error,
                                        cx,
                                    );
                                }
                            }
                            cx.notify();
                        }),
                    ))
                    .child(self.header_link(
                        "jsonc-close",
                        self.catalog.text(AppTextKey::BackToSettings),
                        cx.listener(|this, _, _window, cx| {
                            this.model.close_jsonc();
                            cx.notify();
                        }),
                    )),
            )
    }

    fn jsonc_error_bar(&self, error: &str) -> impl IntoElement {
        div()
            .border_t_1()
            .border_color(rgb(0x3b2a2d))
            .bg(rgb(0x1c1217))
            .px_5()
            .py_3()
            .text_xs()
            .line_height(relative(1.35))
            .text_color(rgb(0xffb4bc))
            .child(format!(
                "{}: {error}",
                self.catalog.text(AppTextKey::JsoncInvalid)
            ))
    }

    fn header_link(
        &self,
        id: &'static str,
        label: &'static str,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_2()
            .py_1()
            .rounded_md()
            .text_sm()
            .text_color(rgb(0x9ac1ff))
            .hover(|style| style.bg(rgb(0x1a2634)))
            .cursor_pointer()
            .child(label)
            .on_click(handler)
    }
}

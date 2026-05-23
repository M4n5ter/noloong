use super::NoloongAppView;
use crate::AppTextKey;
use gpui::{IntoElement, ParentElement as _, Styled as _, div, px, rgb};

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
                        .items_center()
                        .gap_4()
                        .child(self.logo_badge(px(42.0)))
                        .child(
                            div()
                                .text_lg()
                                .text_color(rgb(0xe6edf3))
                                .child(self.catalog.text(AppTextKey::ChatPlaceholder)),
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
                                    .child(self.catalog.text(AppTextKey::ChatTokenCounter)),
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

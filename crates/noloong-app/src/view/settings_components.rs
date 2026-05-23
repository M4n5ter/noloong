use super::{FORM_FIELD_WIDTH, FORM_GAP, FORM_LABEL_WIDTH, NoloongAppView};
use crate::AppTextKey;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window, div, prelude::*, px, relative, rgb,
};
use gpui_component::{
    StyledExt as _,
    input::{Input, InputState},
};
use noloong_config::Locale;

impl NoloongAppView {
    pub(super) fn panel(&self, rows: Vec<AnyElement>) -> impl IntoElement {
        rows.into_iter().fold(
            div()
                .flex()
                .flex_col()
                .gap_4()
                .rounded(px(24.0))
                .border_1()
                .border_color(rgb(0x263341))
                .bg(rgb(0x0f1720))
                .p_5(),
            |panel, row| panel.child(row),
        )
    }

    pub(super) fn detail_card(
        &self,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        facts: Vec<(SharedString, String)>,
    ) -> impl IntoElement {
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .rounded(px(18.0))
            .border_1()
            .border_color(rgb(0x253241))
            .bg(rgb(0x111a24))
            .px_4()
            .py_4()
            .child(
                div().flex().items_start().justify_between().gap_4().child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_w(px(0.0))
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(rgb(0xf1f6fb))
                                .child(title.into()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .line_height(relative(1.35))
                                .text_color(rgb(0x9faab6))
                                .child(description.into()),
                        ),
                ),
            );

        facts.into_iter().fold(card, |card, (label, value)| {
            card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_xs().text_color(rgb(0x748191)).child(label))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(0x182332))
                            .text_xs()
                            .font_medium()
                            .text_color(rgb(0xc7d3df))
                            .child(value),
                    ),
            )
        })
    }

    pub(super) fn empty_state(&self, message: AppTextKey) -> impl IntoElement {
        div()
            .rounded(px(18.0))
            .border_1()
            .border_color(rgb(0x263341))
            .bg(rgb(0x101922))
            .px_4()
            .py_4()
            .text_sm()
            .line_height(relative(1.35))
            .text_color(rgb(0x9faab6))
            .child(self.catalog.text(message))
    }

    pub(super) fn jsonc_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-jsonc-toggle")
            .px_4()
            .py_2()
            .rounded_full()
            .border_1()
            .border_color(rgb(0x344150))
            .bg(rgb(0x111921))
            .text_sm()
            .font_semibold()
            .text_color(rgb(0xa8c7ff))
            .hover(|style| style.bg(rgb(0x223044)))
            .cursor_pointer()
            .child(format!(
                "{}  ⌘⇧J",
                self.catalog.text(AppTextKey::JsoncButton)
            ))
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_jsonc_editor(window, cx);
            }))
    }

    pub(super) fn input_row(
        &self,
        label: impl Into<SharedString>,
        state: &Entity<InputState>,
    ) -> impl IntoElement {
        let disabled = self.model.is_settings_form_read_only();
        div()
            .flex()
            .items_center()
            .gap(px(FORM_GAP))
            .child(self.form_label(label))
            .child(
                div().w(px(FORM_FIELD_WIDTH)).child(
                    Input::new(state)
                        .disabled(disabled)
                        .w_full()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(0x354250))
                        .bg(rgb(0x121a23))
                        .text_color(rgb(0xe6edf3)),
                ),
            )
    }

    pub(super) fn summary_row(
        &self,
        label: impl Into<SharedString>,
        value: impl Into<SharedString>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(FORM_GAP))
            .child(self.form_label(label))
            .child(
                div()
                    .w(px(FORM_FIELD_WIDTH))
                    .min_h(px(38.0))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .py_2()
                            .text_color(rgb(0xa8b4c0))
                            .line_height(relative(1.3))
                            .child(value.into()),
                    ),
            )
    }

    pub(super) fn enabled_text(&self, enabled: bool) -> &'static str {
        if enabled {
            self.catalog.text(AppTextKey::Enabled)
        } else {
            self.catalog.text(AppTextKey::Disabled)
        }
    }

    pub(super) fn action_row(
        &self,
        label: impl Into<SharedString>,
        action: AnyElement,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(FORM_GAP))
            .child(self.form_label(label))
            .child(
                div()
                    .w(px(FORM_FIELD_WIDTH))
                    .flex()
                    .items_center()
                    .child(action),
            )
    }

    pub(super) fn action_button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        disabled: bool,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id.into())
            .px_4()
            .py_2()
            .rounded_full()
            .border_1()
            .border_color(rgb(0x344150))
            .bg(rgb(0x111921))
            .text_sm()
            .font_semibold()
            .text_color(rgb(0xa8c7ff))
            .opacity(if disabled { 0.5 } else { 1.0 })
            .when(!disabled, |this| {
                this.hover(|style| style.bg(rgb(0x223044))).cursor_pointer()
            })
            .child(label.into())
            .on_click(handler)
    }

    pub(super) fn choice_button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        selected: bool,
        disabled: bool,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id.into())
            .px_3()
            .py_2()
            .rounded_full()
            .border_1()
            .border_color(if selected {
                rgb(0x4d76b6)
            } else {
                rgb(0x344150)
            })
            .bg(if selected {
                rgb(0x243d66)
            } else {
                rgb(0x111921)
            })
            .text_sm()
            .font_semibold()
            .text_color(if selected {
                rgb(0xf4f8ff)
            } else {
                rgb(0xa8c7ff)
            })
            .opacity(if disabled { 0.5 } else { 1.0 })
            .when(!disabled, |this| {
                this.hover(|style| style.bg(rgb(0x223044))).cursor_pointer()
            })
            .child(label.into())
            .on_click(handler)
    }

    pub(super) fn toggle_row(
        &self,
        label: impl Into<SharedString>,
        enabled: bool,
        handler: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        let disabled = self.model.is_settings_form_read_only();
        div()
            .flex()
            .items_center()
            .gap(px(FORM_GAP))
            .child(self.form_label(label))
            .child(
                div()
                    .w(px(FORM_FIELD_WIDTH))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("settings-toggle")
                            .w(px(48.0))
                            .h(px(28.0))
                            .rounded_full()
                            .bg(if enabled {
                                rgb(0x4c7dff)
                            } else {
                                rgb(0x263341)
                            })
                            .p(px(3.0))
                            .opacity(if disabled { 0.55 } else { 1.0 })
                            .when(!disabled, |this| {
                                this.cursor_pointer().hover(|style| style.bg(rgb(0x42669a)))
                            })
                            .child(
                                div()
                                    .size(px(22.0))
                                    .rounded_full()
                                    .bg(rgb(0xf4f8fc))
                                    .when(enabled, |this| this.ml(px(20.0))),
                            )
                            .on_click(handler),
                    )
                    .child(div().text_sm().text_color(rgb(0xb7c2ce)).child(if enabled {
                        self.catalog.text(AppTextKey::Enabled)
                    } else {
                        self.catalog.text(AppTextKey::Disabled)
                    })),
            )
    }

    pub(super) fn locale_row(
        &self,
        label: &'static str,
        active_locale: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(FORM_GAP))
            .child(self.form_label(label))
            .child(
                div()
                    .w(px(FORM_FIELD_WIDTH))
                    .flex()
                    .gap_2()
                    .child(self.locale_option("zh", active_locale == "zh", Locale::Zh, cx))
                    .child(self.locale_option("en", active_locale == "en", Locale::En, cx)),
            )
    }

    pub(super) fn jsonc_lock_banner(
        &self,
        error: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .rounded(px(18.0))
            .border_1()
            .border_color(rgb(0x7a3f45))
            .bg(rgb(0x1a1117))
            .px_5()
            .py_4()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .line_height(relative(1.35))
                    .text_color(rgb(0xffbdc4))
                    .child(format!(
                        "{}: {error}",
                        self.catalog.text(AppTextKey::JsoncInvalid)
                    )),
            )
            .child(
                div()
                    .id("jsonc-lock-open")
                    .px_3()
                    .py_2()
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(0x7a4d57))
                    .text_color(rgb(0xffd2d8))
                    .cursor_pointer()
                    .child(self.catalog.text(AppTextKey::JsoncButton))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if !this.model.jsonc_open {
                            this.toggle_jsonc_editor(window, cx);
                        }
                    })),
            )
    }

    fn locale_option(
        &self,
        label: &'static str,
        selected: bool,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("locale-{label}")))
            .px_4()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(if selected {
                rgb(0x42669a)
            } else {
                rgb(0x344150)
            })
            .bg(if selected {
                rgb(0x263a61)
            } else {
                rgb(0x121a23)
            })
            .text_color(if selected {
                rgb(0xf5f9ff)
            } else {
                rgb(0xb7c2ce)
            })
            .when(self.model.is_settings_form_read_only(), |this| {
                this.opacity(0.55)
            })
            .when(!self.model.is_settings_form_read_only(), |this| {
                this.cursor_pointer().hover(|style| style.bg(rgb(0x223044)))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.model.is_settings_form_read_only() {
                    return;
                }
                this.model.set_locale(locale);
                this.sync_jsonc_input(window, cx);
                cx.notify();
            }))
    }

    fn form_label(&self, label: impl Into<SharedString>) -> impl IntoElement {
        div()
            .w(px(FORM_LABEL_WIDTH))
            .text_color(rgb(0xc9d1d9))
            .child(label.into())
    }
}

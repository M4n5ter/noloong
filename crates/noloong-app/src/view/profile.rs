use super::{FORM_FIELD_WIDTH, FORM_GAP, FORM_LABEL_WIDTH, NoloongAppView, ToastTone};
use crate::{AppStatus, AppTextKey};
use gpui::{
    Context, Entity, IntoElement, ParentElement as _, SharedString, Styled as _, div, prelude::*,
    px, rgb,
};
use gpui_component::{
    StyledExt as _,
    input::{Input, InputState},
};
use noloong_agent::Locale;

impl NoloongAppView {
    pub(super) fn render_profile(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let provider_type = self.model.provider_type();
        let locale = self.profile_locale();
        let compaction = self.compaction_summary();
        let storage = self.storage_summary();

        div()
            .flex()
            .flex_col()
            .gap_6()
            .w_full()
            .max_w(px(FORM_LABEL_WIDTH + FORM_GAP + FORM_FIELD_WIDTH))
            .child(self.section_with_action(
                self.catalog.text(AppTextKey::Identity),
                Some(self.jsonc_button(cx).into_any_element()),
                vec![
                    self.input_row(
                        self.catalog.text(AppTextKey::ProfileId),
                        &self.profile_id_input,
                    )
                    .into_any_element(),
                    self.input_row(
                        self.catalog.text(AppTextKey::DisplayName),
                        &self.display_name_input,
                    )
                    .into_any_element(),
                    self.summary_row(self.catalog.text(AppTextKey::Locale), locale)
                        .into_any_element(),
                    self.toggle_row(
                        self.catalog.text(AppTextKey::DefaultProfile),
                        self.model.is_selected_default_profile(),
                        cx,
                    )
                    .into_any_element(),
                ],
            ))
            .child(self.section(
                self.catalog.text(AppTextKey::Provider),
                vec![
                    self.summary_row(self.catalog.text(AppTextKey::ProviderType), provider_type)
                        .into_any_element(),
                    self.input_row(self.catalog.text(AppTextKey::Model), &self.model_input)
                        .into_any_element(),
                ],
            ))
            .child(self.section(
                self.catalog.text(AppTextKey::Compaction),
                vec![
                    self.summary_row(self.catalog.text(AppTextKey::Compaction), compaction)
                        .into_any_element(),
                ],
            ))
            .child(self.section(
                self.catalog.text(AppTextKey::Storage),
                vec![
                    self.summary_row(self.catalog.text(AppTextKey::Storage), storage)
                        .into_any_element(),
                ],
            ))
            .child(self.section(
                self.catalog.text(AppTextKey::Plugins),
                vec![
                    self.count_row(AppTextKey::Plugins, |profile| profile.plugins.len())
                        .into_any_element(),
                    self.count_row(AppTextKey::ManifestPatches, |profile| {
                        profile.manifest_patches.len()
                    })
                    .into_any_element(),
                    self.count_row(AppTextKey::Metadata, |profile| profile.metadata.len())
                        .into_any_element(),
                ],
            ))
    }

    fn section(&self, title: &'static str, rows: Vec<gpui::AnyElement>) -> impl IntoElement {
        self.section_with_action(title, None, rows)
    }

    fn section_with_action(
        &self,
        title: &'static str,
        action: Option<gpui::AnyElement>,
        rows: Vec<gpui::AnyElement>,
    ) -> impl IntoElement {
        rows.into_iter().fold(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .pb_5()
                .border_b_1()
                .border_color(rgb(0x25313d))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(
                            div()
                                .text_lg()
                                .font_semibold()
                                .text_color(rgb(0xe6edf3))
                                .child(title),
                        )
                        .children(action),
                ),
            |section, row| section.child(row),
        )
    }

    fn jsonc_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("profile-jsonc-toggle")
            .px_3()
            .py_1()
            .rounded_lg()
            .border_1()
            .border_color(if self.model.jsonc_open {
                rgb(0x42669a)
            } else {
                rgb(0x344150)
            })
            .bg(if self.model.jsonc_open {
                rgb(0x263a61)
            } else {
                rgb(0x121a23)
            })
            .text_sm()
            .font_semibold()
            .text_color(if self.model.jsonc_open {
                rgb(0x9ac1ff)
            } else {
                rgb(0xaab4c0)
            })
            .hover(|style| style.bg(rgb(0x223044)))
            .cursor_pointer()
            .child(self.catalog.text(AppTextKey::JsoncButton))
            .on_click(cx.listener(|this, _, window, cx| {
                if let Err(error) = this.model.toggle_jsonc() {
                    this.model.status = AppStatus::SaveFailed(error.to_string());
                    this.show_toast(
                        format!("{}: {error}", this.catalog.text(AppTextKey::JsoncInvalid)),
                        ToastTone::Error,
                        cx,
                    );
                } else {
                    this.sync_jsonc_input(window, cx);
                }
                cx.notify();
            }))
    }

    fn input_row(&self, label: &'static str, state: &Entity<InputState>) -> impl IntoElement {
        let disabled = self.model.is_profile_form_read_only();
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
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0x354250))
                        .bg(rgb(0x121a23))
                        .text_color(rgb(0xe6edf3)),
                ),
            )
    }

    fn summary_row(&self, label: &'static str, value: impl Into<SharedString>) -> impl IntoElement {
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
                            .max_w(px(FORM_FIELD_WIDTH))
                            .px_1()
                            .text_color(rgb(0xb7c2ce))
                            .child(value.into()),
                    ),
            )
    }

    fn count_row(
        &self,
        key: AppTextKey,
        count: impl FnOnce(&noloong_config::RuntimeProfileConfig) -> usize,
    ) -> impl IntoElement {
        let value = self
            .model
            .selected_profile()
            .map(count)
            .unwrap_or_default()
            .to_string();
        self.summary_row(self.catalog.text(key), value)
    }

    fn toggle_row(
        &self,
        label: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(FORM_GAP))
            .child(self.form_label(label))
            .child(
                div().w(px(FORM_FIELD_WIDTH)).child(
                    div()
                        .id("default-profile-toggle")
                        .w(px(54.0))
                        .h(px(28.0))
                        .rounded_full()
                        .p_1()
                        .bg(if enabled {
                            rgb(0x4d7dff)
                        } else {
                            rgb(0x303844)
                        })
                        .when(self.model.is_profile_form_read_only(), |this| {
                            this.opacity(0.55)
                        })
                        .when(!self.model.is_profile_form_read_only(), |this| {
                            this.cursor_pointer()
                        })
                        .child(
                            div()
                                .size(px(20.0))
                                .rounded_full()
                                .bg(rgb(0xffffff))
                                .when(enabled, |this| this.ml(px(26.0))),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.model.is_profile_form_read_only() {
                                return;
                            }
                            this.model
                                .set_default_profile(!this.model.is_selected_default_profile());
                            this.sync_jsonc_input(window, cx);
                            cx.notify();
                        })),
                ),
            )
    }

    fn form_label(&self, label: &'static str) -> impl IntoElement {
        div()
            .w(px(FORM_LABEL_WIDTH))
            .text_color(rgb(0xc9d1d9))
            .child(label)
    }

    fn profile_locale(&self) -> String {
        self.model
            .selected_profile()
            .and_then(|profile| profile.locale_override())
            .unwrap_or(Locale::En)
            .code()
            .to_string()
    }

    fn compaction_summary(&self) -> String {
        self.model
            .selected_profile()
            .map(|profile| profile.compaction.type_tag().to_string())
            .unwrap_or_default()
    }

    fn storage_summary(&self) -> String {
        self.model
            .selected_profile()
            .and_then(|profile| profile.event_store.as_ref())
            .map(|store| store.summary())
            .unwrap_or_else(|| "default".into())
    }
}

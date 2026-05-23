use super::{NoloongAppView, SettingsSection};
use crate::{AppTextKey, model::ProfileProviderSummary};
use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    prelude::*, px, rgb,
};
use gpui_component::StyledExt as _;

impl NoloongAppView {
    pub(super) fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_8()
            .w_full()
            .max_w(px(1180.0))
            .child(self.settings_sidebar(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .flex_1()
                    .min_w(px(0.0))
                    .max_w(px(860.0))
                    .child(self.settings_header(cx))
                    .when_some(self.model.jsonc_error(), |this, error| {
                        this.child(self.jsonc_lock_banner(error, cx))
                    })
                    .child(self.settings_panel(cx)),
            )
    }

    fn settings_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = SettingsSection::ALL
            .iter()
            .fold(div().flex().flex_col().gap_1(), |nav, section| {
                nav.child(self.settings_nav_item(*section, cx))
            });

        div()
            .w(px(196.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.profile_switcher(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .rounded(px(22.0))
                    .border_1()
                    .border_color(rgb(0x263341))
                    .bg(rgb(0x101821))
                    .p_2()
                    .child(nav),
            )
    }

    fn profile_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let read_only = self.model.is_settings_form_read_only();
        let profiles = self.model.provider_summaries();
        let profile_count = profiles.len();
        let list = profiles
            .into_iter()
            .fold(div().flex().flex_col().gap_1(), |list, profile| {
                list.child(self.profile_switcher_item(profile, read_only, cx))
            });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(18.0))
            .border_1()
            .border_color(rgb(0x263341))
            .bg(rgb(0x101821))
            .p_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(rgb(0x8f9dad))
                            .child(self.catalog.text(AppTextKey::ProviderConfigurations)),
                    )
                    .when(profile_count > 1, |this| {
                        this.child(
                            div()
                                .px_2()
                                .py_1()
                                .rounded_full()
                                .bg(rgb(0x172232))
                                .text_xs()
                                .text_color(rgb(0xb8c4d0))
                                .child(profile_count.to_string()),
                        )
                    }),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.sidebar_action_button(
                        "settings-add-provider",
                        "+",
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.add_provider_profile(window, cx);
                        }),
                    ))
                    .child(self.sidebar_action_button(
                        "settings-duplicate-provider",
                        "⧉",
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.duplicate_selected_provider_profile(window, cx);
                        }),
                    ))
                    .child(self.sidebar_action_button(
                        "settings-activate-provider",
                        "✓",
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.activate_selected_provider_profile(window, cx);
                        }),
                    ))
                    .child(self.sidebar_action_button(
                        "settings-remove-provider",
                        "−",
                        read_only || profile_count <= 1,
                        cx.listener(|this, _, window, cx| {
                            this.remove_selected_provider_profile(window, cx);
                        }),
                    )),
            )
    }

    fn profile_switcher_item(
        &self,
        profile: ProfileProviderSummary,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let profile_id = profile.profile_id.clone();
        let display_name = profile.display_name.clone();
        div()
            .id(SharedString::from(format!(
                "settings-profile-{}",
                profile_id
            )))
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .rounded(px(14.0))
            .border_1()
            .border_color(if profile.is_selected {
                rgb(0x42669a)
            } else {
                rgb(0x263341)
            })
            .bg(if profile.is_selected {
                rgb(0x17243b)
            } else {
                rgb(0x111a24)
            })
            .px_3()
            .py(px(7.0))
            .opacity(if disabled { 0.55 } else { 1.0 })
            .when(!disabled, |this| {
                this.hover(|style| style.bg(rgb(0x1a2634))).cursor_pointer()
            })
            .child(
                div()
                    .min_w(px(0.0))
                    .text_sm()
                    .font_semibold()
                    .text_color(rgb(0xf1f6fb))
                    .child(display_name),
            )
            .when(profile.is_active, |this| {
                this.child(
                    div()
                        .size(px(7.0))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(rgb(0x72d996)),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.model.is_settings_form_read_only() {
                    return;
                }
                this.select_provider_profile(profile_id.clone(), window, cx);
            }))
    }

    fn sidebar_action_button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        disabled: bool,
        handler: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id.into())
            .w(px(30.0))
            .h(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(rgb(0x344150))
            .bg(rgb(0x111921))
            .text_xs()
            .font_semibold()
            .text_color(rgb(0xa8c7ff))
            .opacity(if disabled { 0.5 } else { 1.0 })
            .when(!disabled, |this| {
                this.hover(|style| style.bg(rgb(0x223044))).cursor_pointer()
            })
            .child(label.into())
            .on_click(handler)
    }

    fn settings_nav_item(
        &self,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.settings_section == section;
        div()
            .id(SharedString::from(format!("settings-nav-{}", section.id())))
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(14.0))
            .px_2()
            .py(px(7.0))
            .text_sm()
            .font_semibold()
            .text_color(if selected {
                rgb(0xf4f8fc)
            } else {
                rgb(0x9da9b5)
            })
            .bg(if selected {
                rgb(0x233657)
            } else {
                rgb(0x101821)
            })
            .hover(|style| style.bg(rgb(0x1a2634)))
            .cursor_pointer()
            .child(
                div()
                    .w(px(26.0))
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(9.0))
                    .bg(if selected {
                        rgb(0x314b78)
                    } else {
                        rgb(0x151f2a)
                    })
                    .text_xs()
                    .child(section.icon()),
            )
            .child(self.catalog.text(section.title_key()))
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.settings_section = section;
                cx.notify();
            }))
    }

    fn settings_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_start()
            .justify_between()
            .gap_6()
            .pb_1()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_2xl()
                            .font_semibold()
                            .text_color(rgb(0xf3f7fb))
                            .child(self.catalog.text(self.settings_section.title_key())),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x84909d))
                            .child(self.catalog.text(self.settings_section.subtitle_key())),
                    ),
            )
            .child(self.jsonc_button(cx))
    }

    fn settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.settings_section {
            SettingsSection::General => self.general_panel(cx).into_any_element(),
            SettingsSection::Provider => self.provider_panel(cx).into_any_element(),
            SettingsSection::Storage => self.storage_panel().into_any_element(),
            SettingsSection::Skills => self.skills_panel(cx).into_any_element(),
            SettingsSection::Mcp => self.mcp_panel(cx).into_any_element(),
            SettingsSection::Runtime => self.runtime_panel(cx).into_any_element(),
            SettingsSection::Advanced => self.advanced_panel(cx).into_any_element(),
        }
    }
}

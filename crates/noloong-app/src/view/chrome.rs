use super::{
    NoloongAppView, TITLE_SAVE_ICON, TITLE_VALIDATE_ICON, TOOLBAR_CHAT_ICON, TOOLBAR_SETTINGS_ICON,
    TOOLBAR_TOOLS_ICON,
};
use crate::{AppRoute, AppStatus, AppTextKey};
use gpui::{
    App, Context, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
    prelude::*, px, rgb, svg,
};
use gpui_component::{StyledExt as _, TitleBar};

impl NoloongAppView {
    fn title_subtitle(&self) -> String {
        if self.model.jsonc_open {
            return self
                .catalog
                .text(AppTextKey::JsoncEditorSubtitle)
                .to_string();
        }
        let status = match &self.model.status {
            AppStatus::StarterDraft => self.catalog.text(AppTextKey::SettingsSubtitle),
            AppStatus::Loaded => self.catalog.text(AppTextKey::SettingsSubtitle),
            AppStatus::Dirty => self.catalog.text(AppTextKey::Unsaved),
            AppStatus::Valid => self.catalog.text(AppTextKey::Valid),
            AppStatus::Invalid(_) => self.catalog.text(AppTextKey::Invalid),
            AppStatus::Saved => self.catalog.text(AppTextKey::Saved),
            AppStatus::SaveFailed(_) => self.catalog.text(AppTextKey::SaveFailed),
        };
        format!("{status} - {}", self.model.config_path.display())
    }

    pub(super) fn render_title_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new()
            .h(px(52.0))
            .bg(rgb(0x0d141b))
            .border_color(rgb(0x1f2a34))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .size_full()
                    .pr_5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .min_w(px(260.0))
                            .child(self.logo_badge(px(32.0)))
                            .child(
                                div()
                                    .h(px(32.0))
                                    .px_4()
                                    .flex()
                                    .items_center()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(rgb(0x2d3743))
                                    .bg(rgb(0x111921))
                                    .text_sm()
                                    .font_semibold()
                                    .text_color(rgb(0xe6edf3))
                                    .child("Noloong"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .font_semibold()
                            .text_color(rgb(0xdde7f0))
                            .child(div().text_base().child(self.title()))
                            .child(
                                div()
                                    .text_xs()
                                    .font_normal()
                                    .text_color(rgb(0x84909d))
                                    .child(self.title_subtitle()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_1()
                            .min_w(px(260.0))
                            .child(self.title_icon_button(
                                "validate",
                                TITLE_VALIDATE_ICON,
                                cx.listener(|this, _, _window, cx| {
                                    this.validate_settings(cx);
                                }),
                            ))
                            .child(self.title_icon_button(
                                "save",
                                TITLE_SAVE_ICON,
                                cx.listener(|this, _, window, cx| {
                                    this.save_settings(window, cx);
                                }),
                            )),
                    ),
            )
    }

    fn title_icon_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("action-{id}")))
            .flex()
            .items_center()
            .justify_center()
            .size(px(36.0))
            .rounded(px(12.0))
            .border_1()
            .border_color(rgb(0x2d3743))
            .text_color(rgb(0xdde7f0))
            .bg(rgb(0x111921))
            .hover(|style| {
                style
                    .bg(rgb(0x1a2634))
                    .border_color(rgb(0x42669a))
                    .text_color(rgb(0xf5f9ff))
            })
            .cursor_pointer()
            .child(
                svg()
                    .external_path(icon_path)
                    .size(px(17.0))
                    .text_color(rgb(0xdde7f0)),
            )
            .on_click(handler)
    }

    pub(super) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .right(px(34.0))
            .top(px(168.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .p(px(6.0))
            .rounded(px(24.0))
            .border_1()
            .border_color(rgb(0x344150))
            .bg(rgb(0x121a23))
            .shadow_lg()
            .child(self.route_button(
                "chat",
                TOOLBAR_CHAT_ICON,
                self.model.route == AppRoute::Chat,
                cx.listener(|this, _, _window, cx| {
                    this.model.select_route(AppRoute::Chat);
                    cx.notify();
                }),
            ))
            .child(self.route_button(
                "tools",
                TOOLBAR_TOOLS_ICON,
                self.model.route == AppRoute::Tools,
                cx.listener(|this, _, _window, cx| {
                    this.model.select_route(AppRoute::Tools);
                    cx.notify();
                }),
            ))
            .child(self.route_button(
                "settings",
                TOOLBAR_SETTINGS_ICON,
                self.model.route == AppRoute::Settings,
                cx.listener(|this, _, _window, cx| {
                    this.model.select_route(AppRoute::Settings);
                    cx.notify();
                }),
            ))
    }

    fn route_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        active: bool,
        handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("route-{id}")))
            .flex()
            .items_center()
            .justify_center()
            .size(px(34.0))
            .rounded(px(13.0))
            .text_sm()
            .font_semibold()
            .text_color(if active { rgb(0x9ac1ff) } else { rgb(0xaab4c0) })
            .bg(if active { rgb(0x263a61) } else { rgb(0x151d26) })
            .hover(|style| style.bg(rgb(0x223044)))
            .cursor_pointer()
            .child(
                svg()
                    .external_path(icon_path)
                    .size(px(18.0))
                    .text_color(if active { rgb(0xf5f9ff) } else { rgb(0xd8e0ea) }),
            )
            .on_click(handler)
    }
}

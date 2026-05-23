use super::NoloongAppView;
use crate::AppTextKey;
use crate::model::SkillRootSummary;
use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    prelude::*, px, relative, rgb,
};
use gpui_component::StyledExt as _;
use noloong_config::Locale;

impl NoloongAppView {
    pub(super) fn general_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        self.panel(vec![
            self.input_row(
                self.catalog.text(AppTextKey::DisplayName),
                &self.display_name_input,
            )
            .into_any_element(),
            self.input_row(
                self.catalog.text(AppTextKey::Description),
                &self.description_input,
            )
            .into_any_element(),
            self.locale_row(
                self.catalog.text(AppTextKey::Locale),
                self.profile_locale(),
                cx,
            )
            .into_any_element(),
        ])
    }

    pub(super) fn storage_panel(&self) -> impl IntoElement {
        self.panel(vec![
            self.summary_row(
                self.catalog.text(AppTextKey::EventStore),
                self.model.event_store_summary(),
            )
            .into_any_element(),
            self.input_row(
                self.catalog.text(AppTextKey::DatabaseUrl),
                &self.event_store_url_input,
            )
            .into_any_element(),
            self.summary_row(
                self.catalog.text(AppTextKey::RegistryStore),
                self.model.registry_store_summary(),
            )
            .into_any_element(),
        ])
    }

    pub(super) fn skills_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let roots = self.model.skill_root_summaries();
        let read_only = self.model.is_settings_form_read_only();
        let mut rows: Vec<AnyElement> = vec![
            self.action_row(
                self.catalog.text(AppTextKey::Skills),
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.action_button(
                        "settings-add-skill-root",
                        self.catalog.text(AppTextKey::AddSkillRoot),
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.add_skill_root(window, cx);
                        }),
                    ))
                    .into_any_element(),
            )
            .into_any_element(),
        ];

        if roots.is_empty() {
            rows.push(self.empty_state(AppTextKey::EmptySkills).into_any_element());
            return self.panel(rows);
        }

        let root_list = roots
            .iter()
            .enumerate()
            .fold(div().flex().flex_col().gap_2(), |list, (index, root)| {
                list.child(self.skill_root_card(index, root.clone(), cx))
            });
        rows.push(root_list.into_any_element());

        if let Some(edit) = self.model.skill_root_edit(self.selected_skill_root_index) {
            rows.push(
                self.detail_card(
                    self.catalog.text(AppTextKey::EditSelectedRoot),
                    edit.root.clone(),
                    vec![
                        (self.catalog.text(AppTextKey::Plugin).into(), edit.plugin_id),
                        (
                            self.catalog.text(AppTextKey::Enabled).into(),
                            self.enabled_text(edit.enabled).into(),
                        ),
                    ],
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::RootPath),
                    &self.skill_root_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.action_row(
                    "",
                    self.action_button(
                        "settings-remove-skill-root",
                        self.catalog.text(AppTextKey::Remove),
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.remove_selected_skill_root(window, cx);
                        }),
                    )
                    .into_any_element(),
                )
                .into_any_element(),
            );
        }
        self.panel(rows)
    }

    fn skill_root_card(
        &self,
        index: usize,
        root: SkillRootSummary,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.selected_skill_root_index == index;
        div()
            .id(SharedString::from(format!("settings-skill-root-{index}")))
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .rounded(px(18.0))
            .border_1()
            .border_color(if selected {
                rgb(0x42669a)
            } else {
                rgb(0x253241)
            })
            .bg(if selected {
                rgb(0x17243b)
            } else {
                rgb(0x111a24)
            })
            .px_4()
            .py_3()
            .hover(|style| style.bg(rgb(0x1a2634)))
            .cursor_pointer()
            .child(
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
                            .child(root.root),
                    )
                    .child(
                        div()
                            .text_xs()
                            .line_height(relative(1.35))
                            .text_color(rgb(0x9faab6))
                            .child(format!("{} ({})", root.plugin_name, root.plugin_id)),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_full()
                    .bg(if root.enabled {
                        rgb(0x183324)
                    } else {
                        rgb(0x321f24)
                    })
                    .text_xs()
                    .font_medium()
                    .text_color(if root.enabled {
                        rgb(0x86efac)
                    } else {
                        rgb(0xffb4bd)
                    })
                    .child(self.enabled_text(root.enabled)),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_skill_root(index, window, cx);
            }))
    }

    pub(super) fn mcp_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let servers = self.model.mcp_server_summaries();
        let read_only = self.model.is_settings_form_read_only();
        let selected_summary = servers.get(self.selected_mcp_server_index).cloned();
        let mut rows: Vec<AnyElement> = vec![
            self.action_row(
                self.catalog.text(AppTextKey::Mcp),
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.action_button(
                        "settings-add-mcp-stdio",
                        self.catalog.text(AppTextKey::AddStdioMcp),
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.add_mcp_stdio_server(window, cx);
                        }),
                    ))
                    .child(self.action_button(
                        "settings-add-mcp-http",
                        self.catalog.text(AppTextKey::AddHttpMcp),
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.add_mcp_http_server(window, cx);
                        }),
                    ))
                    .into_any_element(),
            )
            .into_any_element(),
        ];

        if servers.is_empty() {
            rows.push(self.empty_state(AppTextKey::EmptyMcp).into_any_element());
            return self.panel(rows);
        }

        let server_list = servers.iter().enumerate().fold(
            div().flex().flex_col().gap_2(),
            |list, (index, server)| {
                list.child(self.mcp_server_card(
                    index,
                    server.server_id.clone(),
                    server.transport.clone(),
                    server.endpoint.clone(),
                    server.plugin_name.clone(),
                    server.plugin_id.clone(),
                    server.enabled,
                    cx,
                ))
            },
        );
        rows.push(server_list.into_any_element());

        if let Some(edit) = self.model.mcp_server_edit(self.selected_mcp_server_index) {
            let endpoint_label = if edit.transport == "stdio" {
                self.catalog.text(AppTextKey::Command)
            } else {
                self.catalog.text(AppTextKey::Url)
            };
            let transport_label = if edit.transport == "stdio" {
                self.catalog.text(AppTextKey::Stdio)
            } else {
                self.catalog.text(AppTextKey::StreamableHttp)
            };
            let mut facts = vec![
                (
                    self.catalog.text(AppTextKey::Transport).into(),
                    transport_label.into(),
                ),
                (
                    self.catalog.text(AppTextKey::ToolPrefix).into(),
                    edit.tool_name_prefix.clone(),
                ),
            ];
            if let Some(summary) = selected_summary {
                facts.push((
                    self.catalog.text(AppTextKey::Plugin).into(),
                    summary.plugin_id,
                ));
                facts.push((self.catalog.text(AppTextKey::Cwd).into(), summary.cwd));
                facts.push((
                    self.catalog.text(AppTextKey::Environment).into(),
                    summary.environment_count.to_string(),
                ));
                facts.push((
                    self.catalog.text(AppTextKey::Headers).into(),
                    summary.header_count.to_string(),
                ));
            }
            rows.push(
                self.detail_card(
                    self.catalog.text(AppTextKey::EditSelectedServer),
                    format!("{} · {}", edit.server_id, transport_label),
                    facts,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::ServerId),
                    &self.mcp_server_id_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.summary_row(self.catalog.text(AppTextKey::Transport), transport_label)
                    .into_any_element(),
            );
            rows.push(
                self.input_row(endpoint_label, &self.mcp_endpoint_input)
                    .into_any_element(),
            );
            if edit.transport == "stdio" {
                rows.push(
                    self.input_row(
                        self.catalog.text(AppTextKey::Arguments),
                        &self.mcp_args_input,
                    )
                    .into_any_element(),
                );
            }
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::ToolPrefix),
                    &self.mcp_tool_prefix_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::EnabledTools),
                    &self.mcp_enabled_tools_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::DisabledTools),
                    &self.mcp_disabled_tools_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::Timeout),
                    &self.mcp_timeout_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.action_row(
                    "",
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(self.action_button(
                            "settings-switch-mcp-transport",
                            self.catalog.text(AppTextKey::SwitchTransport),
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.switch_selected_mcp_transport(window, cx);
                            }),
                        ))
                        .child(self.action_button(
                            "settings-remove-mcp",
                            self.catalog.text(AppTextKey::Remove),
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.remove_selected_mcp_server(window, cx);
                            }),
                        ))
                        .into_any_element(),
                )
                .into_any_element(),
            );
        }
        self.panel(rows)
    }

    #[allow(clippy::too_many_arguments)]
    fn mcp_server_card(
        &self,
        index: usize,
        server_id: String,
        transport: String,
        endpoint: String,
        plugin_name: String,
        plugin_id: String,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.selected_mcp_server_index == index;
        div()
            .id(SharedString::from(format!("settings-mcp-server-{index}")))
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .rounded(px(18.0))
            .border_1()
            .border_color(if selected {
                rgb(0x42669a)
            } else {
                rgb(0x253241)
            })
            .bg(if selected {
                rgb(0x17243b)
            } else {
                rgb(0x111a24)
            })
            .px_4()
            .py_3()
            .hover(|style| style.bg(rgb(0x1a2634)))
            .cursor_pointer()
            .child(
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
                            .child(server_id),
                    )
                    .child(
                        div()
                            .text_xs()
                            .line_height(relative(1.35))
                            .text_color(rgb(0x9faab6))
                            .child(format!("{plugin_name} ({plugin_id}) · {endpoint}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(0x182332))
                            .text_xs()
                            .font_medium()
                            .text_color(rgb(0xc7d3df))
                            .child(transport),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(if enabled {
                                rgb(0x183324)
                            } else {
                                rgb(0x321f24)
                            })
                            .text_xs()
                            .font_medium()
                            .text_color(if enabled {
                                rgb(0x86efac)
                            } else {
                                rgb(0xffb4bd)
                            })
                            .child(self.enabled_text(enabled)),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_mcp_server(index, window, cx);
            }))
    }

    pub(super) fn runtime_panel(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = Vec::new();
        rows.extend(
            self.model
                .manifest_patch_summaries()
                .into_iter()
                .map(|patch| {
                    self.detail_card(
                        self.catalog.text(AppTextKey::ManifestPatches),
                        patch,
                        Vec::new(),
                    )
                    .into_any_element()
                }),
        );
        rows.extend(
            self.model
                .extension_summaries()
                .into_iter()
                .map(|extension| {
                    self.detail_card(
                        extension.plugin_name,
                        extension.transport,
                        vec![
                            (
                                self.catalog.text(AppTextKey::Plugin).into(),
                                extension.plugin_id,
                            ),
                            (
                                self.catalog.text(AppTextKey::Enabled).into(),
                                self.enabled_text(extension.enabled).into(),
                            ),
                            (
                                self.catalog.text(AppTextKey::Capabilities).into(),
                                extension.capabilities.len().to_string(),
                            ),
                        ],
                    )
                    .into_any_element()
                }),
        );
        rows.extend(self.model.metadata_rows().into_iter().map(|(key, value)| {
            self.detail_card(SharedString::from(key), value, Vec::new())
                .into_any_element()
        }));
        if rows.is_empty() {
            rows.push(
                self.empty_state(AppTextKey::EmptyRuntime)
                    .into_any_element(),
            );
        }
        self.panel(rows)
    }

    pub(super) fn advanced_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = vec![
            self.detail_card(
                self.catalog.text(AppTextKey::ConfigPath),
                self.model.config_path.display().to_string(),
                vec![(
                    self.catalog.text(AppTextKey::JsoncShortcutHint).into(),
                    "⌘⇧J".into(),
                )],
            )
            .into_any_element(),
            self.summary_row(
                self.catalog.text(AppTextKey::Plugins),
                self.model.plugin_count().to_string(),
            )
            .into_any_element(),
            self.summary_row(
                self.catalog.text(AppTextKey::Registry),
                self.model.registry_store_summary(),
            )
            .into_any_element(),
            self.summary_row(
                format!(
                    "{} {}",
                    self.catalog.text(AppTextKey::ManifestPatches),
                    self.catalog.text(AppTextKey::Count)
                ),
                self.model.manifest_patch_summaries().len().to_string(),
            )
            .into_any_element(),
            self.summary_row(
                format!(
                    "{} {}",
                    self.catalog.text(AppTextKey::Metadata),
                    self.catalog.text(AppTextKey::Count)
                ),
                self.model.metadata_rows().len().to_string(),
            )
            .into_any_element(),
            self.action_row(
                self.catalog.text(AppTextKey::JsoncForStructuredEditing),
                self.jsonc_button(cx).into_any_element(),
            )
            .into_any_element(),
        ];
        rows.extend(self.model.plugin_overviews().into_iter().map(|plugin| {
            self.detail_card(
                plugin.display_name,
                plugin.plugin_id,
                vec![
                    (
                        self.catalog.text(AppTextKey::Enabled).into(),
                        self.enabled_text(plugin.enabled).into(),
                    ),
                    (
                        self.catalog.text(AppTextKey::Components).into(),
                        plugin.component_count.to_string(),
                    ),
                    (
                        self.catalog.text(AppTextKey::OnLoadFailure).into(),
                        plugin.on_load_failure,
                    ),
                ],
            )
            .into_any_element()
        }));
        if rows.is_empty() {
            rows.push(
                self.empty_state(AppTextKey::EmptyAdvanced)
                    .into_any_element(),
            );
        }
        self.panel(rows)
    }

    fn profile_locale(&self) -> String {
        self.model
            .selected_profile()
            .and_then(|profile| profile.locale_override())
            .unwrap_or(Locale::En)
            .code()
            .to_string()
    }
}

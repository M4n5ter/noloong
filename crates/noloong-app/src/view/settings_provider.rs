use super::NoloongAppView;
use crate::AppTextKey;
use gpui::{Context, IntoElement, ParentElement as _, Styled as _, div};

impl NoloongAppView {
    pub(super) fn provider_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let read_only = self.model.is_settings_form_read_only();
        let mut rows = vec![
            self.detail_card(
                self.catalog.text(AppTextKey::ModelProvider),
                self.catalog.text(AppTextKey::ProviderSubtitle),
                Vec::new(),
            )
            .into_any_element(),
            self.summary_row(
                self.catalog.text(AppTextKey::ProviderType),
                self.model.provider_type(),
            )
            .into_any_element(),
            self.input_row(self.catalog.text(AppTextKey::Model), &self.model_input)
                .into_any_element(),
            self.input_row(
                self.catalog.text(AppTextKey::ProviderId),
                &self.provider_id_input,
            )
            .into_any_element(),
        ];

        if self.model.supports_base_url() {
            rows.push(
                self.input_row(self.catalog.text(AppTextKey::BaseUrl), &self.base_url_input)
                    .into_any_element(),
            );
        } else {
            rows.push(
                self.summary_row(
                    self.catalog.text(AppTextKey::BaseUrl),
                    self.catalog.text(AppTextKey::ProviderManaged),
                )
                .into_any_element(),
            );
        }

        if self.model.supports_api_key_env() {
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::ApiKeyEnv),
                    &self.api_key_env_input,
                )
                .into_any_element(),
            );
        } else if let Some(summary) = self.model.provider_auth_summary() {
            rows.push(
                self.summary_row(self.catalog.text(AppTextKey::ApiKeyEnv), summary)
                    .into_any_element(),
            );
        }

        if let Some(state_mode) = self.model.provider_state_mode() {
            rows.push(
                self.summary_row(self.catalog.text(AppTextKey::StateMode), state_mode)
                    .into_any_element(),
            );
        }

        if self.model.supports_max_tokens() {
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::MaxOutputTokens),
                    &self.max_tokens_input,
                )
                .into_any_element(),
            );
        } else {
            rows.push(
                self.summary_row(
                    self.catalog.text(AppTextKey::MaxOutputTokens),
                    self.catalog.text(AppTextKey::ProviderManaged),
                )
                .into_any_element(),
            );
        }

        if let Some(enabled) = self.model.file_data_url_input() {
            rows.push(
                self.toggle_row(
                    self.catalog.text(AppTextKey::FileDataUrlInput),
                    enabled,
                    cx.listener(|this, _, window, cx| {
                        if this.model.is_settings_form_read_only() {
                            return;
                        }
                        this.model.toggle_file_data_url_input();
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );
        }

        rows.push(
            self.detail_card(
                self.catalog.text(AppTextKey::Reasoning),
                self.catalog.text(AppTextKey::ReasoningSubtitle),
                Vec::new(),
            )
            .into_any_element(),
        );
        if let Some(summary) = self.model.reasoning_summary() {
            rows.push(
                self.toggle_row(
                    self.catalog.text(AppTextKey::Reasoning),
                    summary.enabled,
                    cx.listener(|this, _, window, cx| {
                        if this.model.is_settings_form_read_only() {
                            return;
                        }
                        this.model.toggle_reasoning_enabled();
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );
            rows.push(
                self.action_row(
                    self.catalog.text(AppTextKey::ReasoningEffort),
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.choice_button(
                            "reasoning-effort-default",
                            "default",
                            summary.effort == "default",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_effort("default");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-effort-minimal",
                            "minimal",
                            summary.effort == "minimal",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_effort("minimal");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-effort-low",
                            "low",
                            summary.effort == "low",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_effort("low");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-effort-medium",
                            "medium",
                            summary.effort == "medium",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_effort("medium");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-effort-high",
                            "high",
                            summary.effort == "high",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_effort("high");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-effort-xhigh",
                            "xhigh",
                            summary.effort == "xhigh",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_effort("xhigh");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .into_any_element(),
                )
                .into_any_element(),
            );
            rows.push(
                self.action_row(
                    self.catalog.text(AppTextKey::ReasoningSummary),
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.choice_button(
                            "reasoning-summary-default",
                            "default",
                            summary.summary == "-",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_summary("default");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-summary-auto",
                            "auto",
                            summary.summary == "auto",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_summary("auto");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-summary-concise",
                            "concise",
                            summary.summary == "concise",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_summary("concise");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-summary-detailed",
                            "detailed",
                            summary.summary == "detailed",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_summary("detailed");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "reasoning-summary-none",
                            "none",
                            summary.summary == "none",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_reasoning_summary("none");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .into_any_element(),
                )
                .into_any_element(),
            );
        } else {
            rows.push(
                self.summary_row(
                    self.catalog.text(AppTextKey::Reasoning),
                    self.catalog.text(AppTextKey::NotConfigured),
                )
                .into_any_element(),
            );
        }

        rows.push(
            self.detail_card(
                self.catalog.text(AppTextKey::Context),
                self.catalog.text(AppTextKey::ContextSubtitle),
                Vec::new(),
            )
            .into_any_element(),
        );
        rows.push(
            self.summary_row(
                self.catalog.text(AppTextKey::Compaction),
                self.compaction_summary(),
            )
            .into_any_element(),
        );
        let compaction = self.model.compaction_edit();
        rows.push(
            self.action_row(
                self.catalog.text(AppTextKey::CompactionMode),
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.choice_button(
                        "compaction-mode-auto",
                        "auto",
                        compaction.mode == "auto",
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.model.set_compaction_mode("auto");
                            this.sync_compaction_inputs(window, cx);
                            this.sync_jsonc_input(window, cx);
                            cx.notify();
                        }),
                    ))
                    .child(self.choice_button(
                        "compaction-mode-none",
                        "none",
                        compaction.mode == "none",
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.model.set_compaction_mode("none");
                            this.sync_compaction_inputs(window, cx);
                            this.sync_jsonc_input(window, cx);
                            cx.notify();
                        }),
                    ))
                    .child(self.choice_button(
                        "compaction-mode-openai-responses",
                        "openai_responses",
                        compaction.mode == "openai_responses",
                        read_only,
                        cx.listener(|this, _, window, cx| {
                            this.model.set_compaction_mode("openai_responses");
                            this.sync_compaction_inputs(window, cx);
                            this.sync_jsonc_input(window, cx);
                            cx.notify();
                        }),
                    ))
                    .into_any_element(),
            )
            .into_any_element(),
        );
        if compaction.mode == "openai_responses" {
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::ProviderId),
                    &self.compaction_id_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::InputLimitModel),
                    &self.compaction_input_limit_model_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::CompactModel),
                    &self.compaction_compact_model_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::InputLimitTokens),
                    &self.compaction_input_limit_tokens_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::TriggerRatio),
                    &self.compaction_trigger_ratio_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::SummaryBudgetTokens),
                    &self.compaction_summary_budget_tokens_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::KeepRecentTokens),
                    &self.compaction_keep_recent_tokens_input,
                )
                .into_any_element(),
            );
            rows.push(
                self.action_row(
                    self.catalog.text(AppTextKey::StateMode),
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.choice_button(
                            "compaction-state-default",
                            "default",
                            compaction.state_mode.is_empty(),
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_compaction_state_mode("default");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "compaction-state-persistent",
                            "persistent_state",
                            compaction.state_mode == "persistent_state",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_compaction_state_mode("persistent_state");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .child(self.choice_button(
                            "compaction-state-request-only",
                            "request_only",
                            compaction.state_mode == "request_only",
                            read_only,
                            cx.listener(|this, _, window, cx| {
                                this.model.set_compaction_state_mode("request_only");
                                this.sync_jsonc_input(window, cx);
                                cx.notify();
                            }),
                        ))
                        .into_any_element(),
                )
                .into_any_element(),
            );
            rows.push(
                self.input_row(
                    self.catalog.text(AppTextKey::RequestTimeout),
                    &self.compaction_timeout_input,
                )
                .into_any_element(),
            );
        }

        self.panel(rows)
    }

    fn compaction_summary(&self) -> String {
        self.model
            .selected_profile()
            .map(|profile| profile.compaction.type_tag().to_string())
            .unwrap_or_default()
    }
}

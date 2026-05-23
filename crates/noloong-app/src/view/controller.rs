use super::*;

impl NoloongAppView {
    pub(super) fn show_status_error_toast(&mut self, cx: &mut Context<Self>) {
        let text = match &self.model.status {
            AppStatus::Invalid(errors) => {
                format!(
                    "{}: {}",
                    self.catalog.text(AppTextKey::Invalid),
                    errors.join("; ")
                )
            }
            AppStatus::SaveFailed(error) => {
                format!("{}: {error}", self.catalog.text(AppTextKey::SaveFailed))
            }
            _ => self.catalog.text(AppTextKey::SaveFailed).to_string(),
        };
        self.show_toast(text, ToastTone::Error, cx);
    }

    pub(super) fn validate_settings(&mut self, cx: &mut Context<Self>) {
        if self.model.validate() {
            self.show_toast(self.catalog.text(AppTextKey::Valid), ToastTone::Success, cx);
        } else {
            self.show_status_error_toast(cx);
        }
    }

    pub(super) fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.model.save() {
            Ok(()) => {
                self.sync_jsonc_input(window, cx);
                self.show_toast(self.catalog.text(AppTextKey::Saved), ToastTone::Success, cx);
            }
            Err(error) => {
                self.model.status = AppStatus::SaveFailed(error.to_string());
                self.show_status_error_toast(cx);
            }
        }
    }

    pub(super) fn toggle_jsonc_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.model.toggle_jsonc() {
            Ok(()) => {
                self.sync_jsonc_input(window, cx);
                cx.notify();
            }
            Err(error) => {
                self.model.status = AppStatus::SaveFailed(error.to_string());
                self.show_toast(
                    format!("{}: {error}", self.catalog.text(AppTextKey::JsoncInvalid)),
                    ToastTone::Error,
                    cx,
                );
            }
        }
    }

    pub(super) fn sync_settings_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clamp_selected_skill_root_index();
        self.clamp_selected_mcp_server_index();
        let updates = [
            (&self.display_name_input, display_name(&self.model)),
            (&self.description_input, self.model.selected_description()),
            (&self.provider_id_input, self.model.provider_id()),
            (&self.model_input, self.model.model()),
            (&self.base_url_input, self.model.base_url()),
            (&self.api_key_env_input, self.model.api_key_env()),
            (&self.max_tokens_input, self.model.max_tokens_text()),
            (
                &self.event_store_url_input,
                self.model.event_store_sqlite_url(),
            ),
        ];
        for (input, value) in updates {
            if input.read(cx).value().as_ref() != value {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
        self.sync_compaction_inputs(window, cx);
        self.sync_skill_inputs(window, cx);
        self.sync_mcp_inputs(window, cx);
    }

    pub(super) fn sync_compaction_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let edit = self.model.compaction_edit();
        let updates = [
            (&self.compaction_id_input, edit.id),
            (
                &self.compaction_input_limit_model_input,
                edit.input_limit_model,
            ),
            (&self.compaction_compact_model_input, edit.compact_model),
            (
                &self.compaction_input_limit_tokens_input,
                edit.input_limit_tokens,
            ),
            (&self.compaction_trigger_ratio_input, edit.trigger_ratio),
            (
                &self.compaction_summary_budget_tokens_input,
                edit.summary_budget_tokens,
            ),
            (
                &self.compaction_keep_recent_tokens_input,
                edit.keep_recent_tokens,
            ),
            (&self.compaction_timeout_input, edit.request_timeout_secs),
        ];
        for (input, value) in updates {
            if input.read(cx).value().as_ref() != value {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
    }

    pub(super) fn sync_skill_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self
            .model
            .skill_root_edit(self.selected_skill_root_index)
            .map(|edit| edit.root)
            .unwrap_or_default();
        if self.skill_root_input.read(cx).value().as_ref() != value {
            self.skill_root_input
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }

    pub(super) fn sync_mcp_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let edit = self.model.mcp_server_edit(self.selected_mcp_server_index);
        let updates = [
            (
                &self.mcp_server_id_input,
                edit.as_ref()
                    .map(|edit| edit.server_id.clone())
                    .unwrap_or_default(),
            ),
            (
                &self.mcp_endpoint_input,
                edit.as_ref()
                    .map(|edit| edit.endpoint.clone())
                    .unwrap_or_default(),
            ),
            (
                &self.mcp_args_input,
                edit.as_ref()
                    .map(|edit| edit.args.clone())
                    .unwrap_or_default(),
            ),
            (
                &self.mcp_tool_prefix_input,
                edit.as_ref()
                    .map(|edit| edit.tool_name_prefix.clone())
                    .unwrap_or_default(),
            ),
            (
                &self.mcp_enabled_tools_input,
                edit.as_ref()
                    .map(|edit| edit.enabled_tools.clone())
                    .unwrap_or_default(),
            ),
            (
                &self.mcp_disabled_tools_input,
                edit.as_ref()
                    .map(|edit| edit.disabled_tools.clone())
                    .unwrap_or_default(),
            ),
            (
                &self.mcp_timeout_input,
                edit.as_ref()
                    .map(|edit| edit.request_timeout_secs.clone())
                    .unwrap_or_default(),
            ),
        ];
        for (input, value) in updates {
            if input.read(cx).value().as_ref() != value {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
    }

    pub(super) fn select_provider_profile(
        &mut self,
        profile_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.select_provider_profile(profile_id);
        self.clamp_selected_skill_root_index();
        self.clamp_selected_mcp_server_index();
        self.sync_settings_inputs(window, cx);
        cx.notify();
    }

    pub(super) fn add_provider_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        self.model.add_provider_profile();
        self.clamp_selected_skill_root_index();
        self.clamp_selected_mcp_server_index();
        self.sync_settings_inputs(window, cx);
        self.sync_jsonc_input(window, cx);
        cx.notify();
    }

    pub(super) fn duplicate_selected_provider_profile(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        self.model.duplicate_selected_provider_profile();
        self.clamp_selected_skill_root_index();
        self.clamp_selected_mcp_server_index();
        self.sync_settings_inputs(window, cx);
        self.sync_jsonc_input(window, cx);
        cx.notify();
    }

    pub(super) fn activate_selected_provider_profile(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        self.model.activate_selected_provider_profile();
        self.sync_jsonc_input(window, cx);
        cx.notify();
    }

    pub(super) fn remove_selected_provider_profile(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        self.model.remove_selected_provider_profile();
        self.clamp_selected_skill_root_index();
        self.clamp_selected_mcp_server_index();
        self.sync_settings_inputs(window, cx);
        self.sync_jsonc_input(window, cx);
        cx.notify();
    }

    pub(super) fn select_skill_root(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.model.skill_root_summaries().len();
        self.selected_skill_root_index = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
        self.sync_skill_inputs(window, cx);
        cx.notify();
    }

    pub(super) fn add_skill_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        let index = self.model.add_skill_root();
        self.select_skill_root(index, window, cx);
        self.sync_jsonc_input(window, cx);
    }

    pub(super) fn remove_selected_skill_root(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        self.model.remove_skill_root(self.selected_skill_root_index);
        self.clamp_selected_skill_root_index();
        self.sync_skill_inputs(window, cx);
        self.sync_jsonc_input(window, cx);
        cx.notify();
    }

    pub(super) fn clamp_selected_skill_root_index(&mut self) {
        let len = self.model.skill_root_summaries().len();
        if len == 0 {
            self.selected_skill_root_index = 0;
        } else if self.selected_skill_root_index >= len {
            self.selected_skill_root_index = len - 1;
        }
    }

    pub(super) fn select_mcp_server(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.model.mcp_server_summaries().len();
        self.selected_mcp_server_index = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
        self.sync_mcp_inputs(window, cx);
        cx.notify();
    }

    pub(super) fn add_mcp_stdio_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        let index = self.model.add_mcp_stdio_server();
        self.select_mcp_server(index, window, cx);
        self.sync_jsonc_input(window, cx);
    }

    pub(super) fn add_mcp_http_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        let index = self.model.add_mcp_http_server();
        self.select_mcp_server(index, window, cx);
        self.sync_jsonc_input(window, cx);
    }

    pub(super) fn remove_selected_mcp_server(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        self.model.remove_mcp_server(self.selected_mcp_server_index);
        self.clamp_selected_mcp_server_index();
        self.sync_mcp_inputs(window, cx);
        self.sync_jsonc_input(window, cx);
        cx.notify();
    }

    pub(super) fn switch_selected_mcp_transport(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model.is_settings_form_read_only() {
            return;
        }
        self.model
            .switch_mcp_transport(self.selected_mcp_server_index);
        self.sync_mcp_inputs(window, cx);
        self.sync_jsonc_input(window, cx);
        cx.notify();
    }

    pub(super) fn clamp_selected_mcp_server_index(&mut self) {
        let len = self.model.mcp_server_summaries().len();
        if len == 0 {
            self.selected_mcp_server_index = 0;
        } else if self.selected_mcp_server_index >= len {
            self.selected_mcp_server_index = len - 1;
        }
    }

    pub(super) fn sync_jsonc_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.jsonc_input.read(cx).value().as_ref() == self.model.jsonc_text {
            return;
        }
        self.jsonc_input.update(cx, |input, cx| {
            input.set_value(self.model.jsonc_text.clone(), window, cx)
        });
    }

    pub(super) fn show_toast(
        &mut self,
        text: impl Into<String>,
        tone: ToastTone,
        cx: &mut Context<Self>,
    ) {
        self.next_toast_id = self.next_toast_id.wrapping_add(1);
        let id = self.next_toast_id;
        let now = Instant::now();
        self.toasts.push(ToastMessage {
            id,
            text: text.into(),
            tone,
            created_at: now,
            motion: Some(ToastMotion {
                from_depth: 1,
                started_at: now,
            }),
        });
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.drain(0..self.toasts.len() - MAX_TOASTS);
        }
        self.start_toast_driver(cx);
        cx.notify();
    }

    pub(super) fn promote_toast(&mut self, toast_id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.toasts.iter().position(|toast| toast.id == toast_id) else {
            return;
        };
        let now = Instant::now();
        if self
            .last_toast_promotion_at
            .is_some_and(|last| now.saturating_duration_since(last) < TOAST_PROMOTE_COOLDOWN)
        {
            return;
        }
        let from_depth = self.toasts.len().saturating_sub(1).saturating_sub(index);
        if from_depth == 0 {
            return;
        }

        let mut toast = self.toasts.remove(index);
        toast.created_at = now;
        toast.motion = Some(ToastMotion {
            from_depth,
            started_at: now,
        });
        self.toasts.push(toast);
        self.last_toast_promotion_at = Some(now);
        self.start_toast_driver(cx);
        cx.notify();
    }

    pub(super) fn dismiss_toast(&mut self, toast_id: u64, cx: &mut Context<Self>) {
        let before = self.toasts.len();
        self.toasts.retain(|toast| toast.id != toast_id);
        if self.toasts.len() != before {
            cx.notify();
        }
    }

    pub(super) fn start_toast_driver(&mut self, cx: &mut Context<Self>) {
        self.toast_task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TOAST_TICK).await;
                let Some(this) = this.upgrade() else {
                    break;
                };
                let keep_running = this.update(cx, |this, cx| {
                    let now = Instant::now();
                    this.toasts.retain(|toast| toast.is_visible_at(now));
                    for toast in &mut this.toasts {
                        if toast.motion.is_some_and(|motion| {
                            now.saturating_duration_since(motion.started_at) >= TOAST_REORDER_FOR
                        }) {
                            toast.motion = None;
                        }
                    }
                    let keep_running = !this.toasts.is_empty();
                    cx.notify();
                    keep_running
                });
                if !keep_running {
                    break;
                }
            }
        });
    }

    pub(super) fn logo_badge(&self, size: gpui::Pixels) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .size(size)
            .rounded(px(9.0))
            .border_1()
            .border_color(rgb(0x2d3743))
            .bg(rgb(0x0b1118))
            .shadow_sm()
            .child(img(PathBuf::from(LOGO_PATH)).size(size - px(4.0)))
    }

    pub(super) fn title(&self) -> &'static str {
        if self.model.jsonc_open {
            return self.catalog.text(AppTextKey::JsoncEditor);
        }
        match self.model.route {
            AppRoute::Chat => self.catalog.text(AppTextKey::AppTitle),
            AppRoute::Tools => self.catalog.text(AppTextKey::Tools),
            AppRoute::Settings => self.catalog.text(AppTextKey::Settings),
        }
    }

    pub(super) fn render_page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.model.jsonc_open {
            return div()
                .relative()
                .size_full()
                .p_8()
                .child(self.render_jsonc_editor(cx))
                .into_any_element();
        }

        let content = match self.model.route {
            AppRoute::Chat => self.render_chat().into_any_element(),
            AppRoute::Tools => self
                .render_placeholder(AppTextKey::ToolsPlaceholder)
                .into_any_element(),
            AppRoute::Settings => self.render_settings(cx).into_any_element(),
        };

        div()
            .relative()
            .flex()
            .justify_center()
            .size_full()
            .p_10()
            .overflow_y_scrollbar()
            .child(content)
            .into_any_element()
    }

    pub(super) fn on_toggle_jsonc_editor(
        &mut self,
        _: &ToggleJsoncEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_jsonc_editor(window, cx);
    }

    pub(super) fn on_save_settings(
        &mut self,
        _: &SaveSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_settings(window, cx);
    }

    pub(super) fn on_validate_settings(&mut self, _: &ValidateSettings, cx: &mut Context<Self>) {
        self.validate_settings(cx);
    }

    pub(super) fn on_validate_settings_action(
        &mut self,
        action: &ValidateSettings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_validate_settings(action, cx);
    }

    pub(super) fn request_toast_animation_frame(&self, window: &mut Window) {
        let now = Instant::now();
        if self
            .toasts
            .iter()
            .any(|toast| toast.opacity_at(now) < 1.0 || toast.is_animating_at(now))
        {
            window.request_animation_frame();
        }
    }
}

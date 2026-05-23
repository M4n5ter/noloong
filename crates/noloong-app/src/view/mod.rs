use crate::interaction::AppInteractionClient as _;
use crate::{
    APP_KEY_CONTEXT, AppI18nCatalog, AppInteractionHttpClient, AppInteractionStatus, AppRoute,
    AppStatus, AppTextKey, AppViewModel, SaveSettings, ToggleJsoncEditor, ValidateSettings,
};
use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Task, Window, div, img,
    prelude::*, px, rgb,
};
use gpui_component::{
    input::{InputEvent, InputState},
    scroll::ScrollableElement as _,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

mod chat;
mod chat_items;
mod chrome;
mod controller;
mod jsonc;
mod jsonc_completion;
mod settings;
mod settings_components;
mod settings_panels;
mod settings_provider;
mod toast;

const LOGO_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/noloong-logo.png");
const TITLE_VALIDATE_ICON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/title-validate.svg");
const TITLE_SAVE_ICON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/title-save.svg");
const TOOLBAR_CHAT_ICON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/toolbar-chat.svg");
const TOOLBAR_TOOLS_ICON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/toolbar-tools.svg");
const TOOLBAR_SETTINGS_ICON: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/toolbar-settings.svg");
const FORM_LABEL_WIDTH: f32 = 220.0;
const FORM_FIELD_WIDTH: f32 = 640.0;
const FORM_GAP: f32 = 16.0;
const TOAST_VISIBLE_FOR: Duration = Duration::from_millis(3200);
const TOAST_FADE_FOR: Duration = Duration::from_millis(520);
const TOAST_REORDER_FOR: Duration = Duration::from_millis(220);
const TOAST_PROMOTE_COOLDOWN: Duration = Duration::from_millis(280);
const TOAST_TICK: Duration = Duration::from_millis(16);
const MAX_TOASTS: usize = 4;

pub(crate) struct NoloongAppView {
    model: AppViewModel,
    catalog: AppI18nCatalog,
    settings_section: SettingsSection,
    selected_skill_root_index: usize,
    selected_mcp_server_index: usize,
    display_name_input: Entity<InputState>,
    description_input: Entity<InputState>,
    provider_id_input: Entity<InputState>,
    model_input: Entity<InputState>,
    base_url_input: Entity<InputState>,
    api_key_env_input: Entity<InputState>,
    max_tokens_input: Entity<InputState>,
    compaction_id_input: Entity<InputState>,
    compaction_input_limit_model_input: Entity<InputState>,
    compaction_compact_model_input: Entity<InputState>,
    compaction_input_limit_tokens_input: Entity<InputState>,
    compaction_trigger_ratio_input: Entity<InputState>,
    compaction_summary_budget_tokens_input: Entity<InputState>,
    compaction_keep_recent_tokens_input: Entity<InputState>,
    compaction_timeout_input: Entity<InputState>,
    event_store_url_input: Entity<InputState>,
    skill_root_input: Entity<InputState>,
    mcp_server_id_input: Entity<InputState>,
    mcp_endpoint_input: Entity<InputState>,
    mcp_args_input: Entity<InputState>,
    mcp_tool_prefix_input: Entity<InputState>,
    mcp_enabled_tools_input: Entity<InputState>,
    mcp_disabled_tools_input: Entity<InputState>,
    mcp_timeout_input: Entity<InputState>,
    jsonc_input: Entity<InputState>,
    chat_input: Entity<InputState>,
    toasts: Vec<ToastMessage>,
    next_toast_id: u64,
    last_toast_promotion_at: Option<Instant>,
    chat_refresh_task: Task<()>,
    chat_run_task: Task<()>,
    chat_abort_task: Task<()>,
    chat_approval_task: Task<()>,
    toast_task: Task<()>,
    _subscriptions: Vec<gpui::Subscription>,
}

#[derive(Clone, Debug)]
struct ToastMessage {
    id: u64,
    text: String,
    tone: ToastTone,
    created_at: Instant,
    motion: Option<ToastMotion>,
}

#[derive(Clone, Copy, Debug)]
struct ToastMotion {
    from_depth: usize,
    started_at: Instant,
}

#[derive(Clone, Copy, Debug)]
enum ToastTone {
    Success,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSection {
    General,
    Provider,
    Storage,
    Skills,
    Mcp,
    Runtime,
    Advanced,
}

impl SettingsSection {
    const ALL: &'static [Self] = &[
        Self::General,
        Self::Provider,
        Self::Storage,
        Self::Skills,
        Self::Mcp,
        Self::Runtime,
        Self::Advanced,
    ];

    const fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Provider => "provider",
            Self::Storage => "storage",
            Self::Skills => "skills",
            Self::Mcp => "mcp",
            Self::Runtime => "runtime",
            Self::Advanced => "advanced",
        }
    }

    const fn icon(self) -> &'static str {
        match self {
            Self::General => "⌘",
            Self::Provider => "◈",
            Self::Storage => "▣",
            Self::Skills => "◇",
            Self::Mcp => "⌁",
            Self::Runtime => "⚙",
            Self::Advanced => "{}",
        }
    }

    const fn title_key(self) -> AppTextKey {
        match self {
            Self::General => AppTextKey::General,
            Self::Provider => AppTextKey::ModelProvider,
            Self::Storage => AppTextKey::Storage,
            Self::Skills => AppTextKey::Skills,
            Self::Mcp => AppTextKey::Mcp,
            Self::Runtime => AppTextKey::Runtime,
            Self::Advanced => AppTextKey::Advanced,
        }
    }

    const fn subtitle_key(self) -> AppTextKey {
        match self {
            Self::General => AppTextKey::GeneralSubtitle,
            Self::Provider => AppTextKey::ProviderSubtitle,
            Self::Storage => AppTextKey::StorageSubtitle,
            Self::Skills => AppTextKey::SkillsSubtitle,
            Self::Mcp => AppTextKey::McpSubtitle,
            Self::Runtime => AppTextKey::RuntimeSubtitle,
            Self::Advanced => AppTextKey::AdvancedSubtitle,
        }
    }
}

impl ToastMessage {
    fn opacity_at(&self, now: Instant) -> f32 {
        let age = now.saturating_duration_since(self.created_at);
        if age <= TOAST_VISIBLE_FOR {
            return 1.0;
        }
        let fade_age = age.saturating_sub(TOAST_VISIBLE_FOR);
        if fade_age >= TOAST_FADE_FOR {
            return 0.0;
        }
        1.0 - fade_age.as_secs_f32() / TOAST_FADE_FOR.as_secs_f32()
    }

    fn is_visible_at(&self, now: Instant) -> bool {
        self.opacity_at(now) > 0.0
    }

    fn is_animating_at(&self, now: Instant) -> bool {
        self.motion.is_some_and(|motion| {
            now.saturating_duration_since(motion.started_at) < TOAST_REORDER_FOR
        })
    }

    fn visual_depth(&self, target_depth: usize, now: Instant) -> f32 {
        let Some(motion) = self.motion else {
            return target_depth as f32;
        };
        let elapsed = now.saturating_duration_since(motion.started_at);
        if elapsed >= TOAST_REORDER_FOR {
            return target_depth as f32;
        }
        let t = ease_out_cubic(elapsed.as_secs_f32() / TOAST_REORDER_FOR.as_secs_f32());
        motion.from_depth as f32 + (target_depth as f32 - motion.from_depth as f32) * t
    }
}

impl NoloongAppView {
    pub(crate) fn new(model: AppViewModel, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let catalog = AppI18nCatalog::new(model.locale);
        let display_name_input =
            cx.new(|cx| InputState::new(window, cx).default_value(display_name(&model)));
        let description_input =
            cx.new(|cx| InputState::new(window, cx).default_value(model.selected_description()));
        let provider_id_input =
            cx.new(|cx| InputState::new(window, cx).default_value(model.provider_id()));
        let model_input = cx.new(|cx| InputState::new(window, cx).default_value(model.model()));
        let base_url_input =
            cx.new(|cx| InputState::new(window, cx).default_value(model.base_url()));
        let api_key_env_input =
            cx.new(|cx| InputState::new(window, cx).default_value(model.api_key_env()));
        let max_tokens_input =
            cx.new(|cx| InputState::new(window, cx).default_value(model.max_tokens_text()));
        let compaction = model.compaction_edit();
        let compaction_id_input =
            cx.new(|cx| InputState::new(window, cx).default_value(compaction.id.clone()));
        let compaction_input_limit_model_input = cx.new(|cx| {
            InputState::new(window, cx).default_value(compaction.input_limit_model.clone())
        });
        let compaction_compact_model_input = cx
            .new(|cx| InputState::new(window, cx).default_value(compaction.compact_model.clone()));
        let compaction_input_limit_tokens_input = cx.new(|cx| {
            InputState::new(window, cx).default_value(compaction.input_limit_tokens.clone())
        });
        let compaction_trigger_ratio_input = cx
            .new(|cx| InputState::new(window, cx).default_value(compaction.trigger_ratio.clone()));
        let compaction_summary_budget_tokens_input = cx.new(|cx| {
            InputState::new(window, cx).default_value(compaction.summary_budget_tokens.clone())
        });
        let compaction_keep_recent_tokens_input = cx.new(|cx| {
            InputState::new(window, cx).default_value(compaction.keep_recent_tokens.clone())
        });
        let compaction_timeout_input = cx.new(|cx| {
            InputState::new(window, cx).default_value(compaction.request_timeout_secs.clone())
        });
        let event_store_url_input =
            cx.new(|cx| InputState::new(window, cx).default_value(model.event_store_sqlite_url()));
        let skill_root = model
            .skill_root_edit(0)
            .map(|edit| edit.root)
            .unwrap_or_default();
        let skill_root_input = cx.new(|cx| InputState::new(window, cx).default_value(skill_root));
        let mcp_edit = model.mcp_server_edit(0);
        let mcp_server_id = mcp_edit
            .as_ref()
            .map(|edit| edit.server_id.clone())
            .unwrap_or_default();
        let mcp_endpoint = mcp_edit
            .as_ref()
            .map(|edit| edit.endpoint.clone())
            .unwrap_or_default();
        let mcp_args = mcp_edit
            .as_ref()
            .map(|edit| edit.args.clone())
            .unwrap_or_default();
        let mcp_tool_prefix = mcp_edit
            .as_ref()
            .map(|edit| edit.tool_name_prefix.clone())
            .unwrap_or_default();
        let mcp_enabled_tools = mcp_edit
            .as_ref()
            .map(|edit| edit.enabled_tools.clone())
            .unwrap_or_default();
        let mcp_disabled_tools = mcp_edit
            .as_ref()
            .map(|edit| edit.disabled_tools.clone())
            .unwrap_or_default();
        let mcp_timeout = mcp_edit
            .as_ref()
            .map(|edit| edit.request_timeout_secs.clone())
            .unwrap_or_default();
        let mcp_server_id_input =
            cx.new(|cx| InputState::new(window, cx).default_value(mcp_server_id));
        let mcp_endpoint_input =
            cx.new(|cx| InputState::new(window, cx).default_value(mcp_endpoint));
        let mcp_args_input = cx.new(|cx| InputState::new(window, cx).default_value(mcp_args));
        let mcp_tool_prefix_input =
            cx.new(|cx| InputState::new(window, cx).default_value(mcp_tool_prefix));
        let mcp_enabled_tools_input =
            cx.new(|cx| InputState::new(window, cx).default_value(mcp_enabled_tools));
        let mcp_disabled_tools_input =
            cx.new(|cx| InputState::new(window, cx).default_value(mcp_disabled_tools));
        let mcp_timeout_input = cx.new(|cx| InputState::new(window, cx).default_value(mcp_timeout));
        let jsonc_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .code_editor("jsonc")
                .line_number(true)
                .folding(false)
                .default_value(model.jsonc_text.clone());
            input.lsp.completion_provider = Some(std::rc::Rc::new(
                jsonc_completion::SettingsJsoncCompletionProvider::new(model.schema_index.clone()),
            ));
            input
        });
        let chat_input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 6)
                .placeholder(catalog.text(AppTextKey::ChatComposerPlaceholder))
        });

        let _subscriptions = vec![
            cx.subscribe_in(&chat_input, window, {
                move |_: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&display_name_input, window, {
                let state = display_name_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_display_name(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&description_input, window, {
                let state = description_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_description(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&provider_id_input, window, {
                let state = provider_id_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_provider_id(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&model_input, window, {
                let state = model_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_model(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&base_url_input, window, {
                let state = base_url_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_base_url(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&api_key_env_input, window, {
                let state = api_key_env_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_api_key_env(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&max_tokens_input, window, {
                let state = max_tokens_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_max_tokens_text(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&compaction_id_input, window, {
                let state = compaction_id_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_compaction_id(state.read(cx).value().to_string());
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&compaction_input_limit_model_input, window, {
                let state = compaction_input_limit_model_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_compaction_input_limit_model(state.read(cx).value().to_string());
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&compaction_compact_model_input, window, {
                let state = compaction_compact_model_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_compaction_compact_model(state.read(cx).value().to_string());
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&compaction_input_limit_tokens_input, window, {
                let state = compaction_input_limit_tokens_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_compaction_input_limit_tokens(state.read(cx).value().to_string());
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&compaction_trigger_ratio_input, window, {
                let state = compaction_trigger_ratio_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_compaction_trigger_ratio(state.read(cx).value().to_string());
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&compaction_summary_budget_tokens_input, window, {
                let state = compaction_summary_budget_tokens_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_compaction_summary_budget_tokens(
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&compaction_keep_recent_tokens_input, window, {
                let state = compaction_keep_recent_tokens_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_compaction_keep_recent_tokens(state.read(cx).value().to_string());
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&compaction_timeout_input, window, {
                let state = compaction_timeout_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_compaction_timeout(state.read(cx).value().to_string());
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&event_store_url_input, window, {
                let state = event_store_url_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_event_store_sqlite_url(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&skill_root_input, window, {
                let state = skill_root_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_skill_root(
                            this.selected_skill_root_index,
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&mcp_server_id_input, window, {
                let state = mcp_server_id_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_mcp_server_id(
                            this.selected_mcp_server_index,
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&mcp_endpoint_input, window, {
                let state = mcp_endpoint_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_mcp_endpoint(
                            this.selected_mcp_server_index,
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&mcp_args_input, window, {
                let state = mcp_args_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_mcp_args(
                            this.selected_mcp_server_index,
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&mcp_tool_prefix_input, window, {
                let state = mcp_tool_prefix_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_mcp_tool_prefix(
                            this.selected_mcp_server_index,
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&mcp_enabled_tools_input, window, {
                let state = mcp_enabled_tools_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_mcp_enabled_tools(
                            this.selected_mcp_server_index,
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&mcp_disabled_tools_input, window, {
                let state = mcp_disabled_tools_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_mcp_disabled_tools(
                            this.selected_mcp_server_index,
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&mcp_timeout_input, window, {
                let state = mcp_timeout_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model.set_mcp_timeout(
                            this.selected_mcp_server_index,
                            state.read(cx).value().to_string(),
                        );
                        this.sync_jsonc_input(window, cx);
                        cx.notify();
                    }
                }
            }),
            cx.subscribe_in(&jsonc_input, window, {
                let state = jsonc_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let updated = this
                            .model
                            .set_jsonc_text(state.read(cx).value().to_string());
                        if updated {
                            this.sync_settings_inputs(window, cx);
                        }
                        cx.notify();
                    }
                }
            }),
        ];

        let mut this = Self {
            model,
            catalog,
            settings_section: SettingsSection::General,
            selected_skill_root_index: 0,
            selected_mcp_server_index: 0,
            display_name_input,
            description_input,
            provider_id_input,
            model_input,
            base_url_input,
            api_key_env_input,
            max_tokens_input,
            compaction_id_input,
            compaction_input_limit_model_input,
            compaction_compact_model_input,
            compaction_input_limit_tokens_input,
            compaction_trigger_ratio_input,
            compaction_summary_budget_tokens_input,
            compaction_keep_recent_tokens_input,
            compaction_timeout_input,
            event_store_url_input,
            skill_root_input,
            mcp_server_id_input,
            mcp_endpoint_input,
            mcp_args_input,
            mcp_tool_prefix_input,
            mcp_enabled_tools_input,
            mcp_disabled_tools_input,
            mcp_timeout_input,
            jsonc_input,
            chat_input,
            toasts: Vec::new(),
            next_toast_id: 0,
            last_toast_promotion_at: None,
            chat_refresh_task: Task::ready(()),
            chat_run_task: Task::ready(()),
            chat_abort_task: Task::ready(()),
            chat_approval_task: Task::ready(()),
            toast_task: Task::ready(()),
            _subscriptions,
        };
        this.start_initial_chat_session_refresh(cx);
        this
    }

    fn start_initial_chat_session_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(endpoint) = self.model.interaction_endpoint.clone() else {
            return;
        };
        let client = match AppInteractionHttpClient::from_endpoint(&endpoint) {
            Ok(client) => client,
            Err(error) => {
                self.model.interaction_status = AppInteractionStatus::Failed(error.to_string());
                return;
            }
        };
        self.chat_refresh_task = cx.spawn(async move |this, cx| {
            let result = client.list_sessions().await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |this, cx| {
                match result {
                    Ok(sessions) => this.model.apply_chat_session_descriptors(sessions),
                    Err(error) => {
                        this.model.interaction_status =
                            AppInteractionStatus::Failed(error.to_string());
                    }
                }
                cx.notify();
            });
        });
    }
}

impl Render for NoloongAppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.request_toast_animation_frame(window);

        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x0d141b))
            .text_color(rgb(0xe6edf3))
            .key_context(APP_KEY_CONTEXT)
            .on_action(cx.listener(Self::on_toggle_jsonc_editor))
            .on_action(cx.listener(Self::on_save_settings))
            .on_action(cx.listener(Self::on_validate_settings_action))
            .child(self.render_title_bar(cx))
            .child(self.render_page(cx))
            .when(!self.model.jsonc_open, |this| {
                this.child(self.render_toolbar(cx))
            })
            .when(!self.toasts.is_empty(), |this| {
                this.child(self.render_toasts(cx))
            })
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn display_name(model: &AppViewModel) -> String {
    model
        .selected_profile()
        .map(|profile| profile.display_name.clone())
        .unwrap_or_default()
}

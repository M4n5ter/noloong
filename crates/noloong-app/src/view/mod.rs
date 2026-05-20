use crate::{AppI18nCatalog, AppRoute, AppStatus, AppTextKey, AppViewModel};
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
mod chrome;
mod jsonc;
mod jsonc_completion;
mod profile;
mod toast;

const LOGO_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/noloong-logo.png");
const TITLE_VALIDATE_ICON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/title-validate.svg");
const TITLE_SAVE_ICON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/title-save.svg");
const TOOLBAR_CHAT_ICON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/toolbar-chat.svg");
const TOOLBAR_PROFILE_ICON: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/toolbar-profile.svg");
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
    profile_id_input: Entity<InputState>,
    display_name_input: Entity<InputState>,
    model_input: Entity<InputState>,
    jsonc_input: Entity<InputState>,
    toasts: Vec<ToastMessage>,
    next_toast_id: u64,
    last_toast_promotion_at: Option<Instant>,
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
        let profile_id_input =
            cx.new(|cx| InputState::new(window, cx).default_value(profile_id(&model)));
        let display_name_input =
            cx.new(|cx| InputState::new(window, cx).default_value(display_name(&model)));
        let model_input = cx.new(|cx| InputState::new(window, cx).default_value(model.model()));
        let jsonc_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .code_editor("jsonc")
                .line_number(true)
                .folding(false)
                .default_value(model.jsonc_text.clone());
            input.lsp.completion_provider = Some(std::rc::Rc::new(
                jsonc_completion::ProfileJsoncCompletionProvider::new(model.schema_index.clone()),
            ));
            input
        });

        let _subscriptions = vec![
            cx.subscribe_in(&profile_id_input, window, {
                let state = profile_id_input.clone();
                move |this: &mut Self, _, event: &InputEvent, _window, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.model
                            .set_profile_id(state.read(cx).value().to_string());
                        this.sync_jsonc_input(_window, cx);
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
            cx.subscribe_in(&jsonc_input, window, {
                let state = jsonc_input.clone();
                move |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let updated = this
                            .model
                            .set_jsonc_text(state.read(cx).value().to_string());
                        if updated {
                            this.sync_profile_inputs(window, cx);
                        }
                        cx.notify();
                    }
                }
            }),
        ];

        Self {
            model,
            catalog,
            profile_id_input,
            display_name_input,
            model_input,
            jsonc_input,
            toasts: Vec::new(),
            next_toast_id: 0,
            last_toast_promotion_at: None,
            toast_task: Task::ready(()),
            _subscriptions,
        }
    }

    fn show_status_error_toast(&mut self, cx: &mut Context<Self>) {
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

    fn sync_profile_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let updates = [
            (&self.profile_id_input, profile_id(&self.model)),
            (&self.display_name_input, display_name(&self.model)),
            (&self.model_input, self.model.model()),
        ];
        for (input, value) in updates {
            if input.read(cx).value().as_ref() != value {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
    }

    fn sync_jsonc_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.jsonc_input.read(cx).value().as_ref() == self.model.jsonc_text {
            return;
        }
        self.jsonc_input.update(cx, |input, cx| {
            input.set_value(self.model.jsonc_text.clone(), window, cx)
        });
    }

    fn show_toast(&mut self, text: impl Into<String>, tone: ToastTone, cx: &mut Context<Self>) {
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

    fn promote_toast(&mut self, toast_id: u64, cx: &mut Context<Self>) {
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

    fn dismiss_toast(&mut self, toast_id: u64, cx: &mut Context<Self>) {
        let before = self.toasts.len();
        self.toasts.retain(|toast| toast.id != toast_id);
        if self.toasts.len() != before {
            cx.notify();
        }
    }

    fn start_toast_driver(&mut self, cx: &mut Context<Self>) {
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

    fn logo_badge(&self, size: gpui::Pixels) -> impl IntoElement {
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

    fn title(&self) -> &'static str {
        match self.model.route {
            AppRoute::Profile => self.catalog.text(AppTextKey::ProfileSettingsTitle),
            AppRoute::Chat => self.catalog.text(AppTextKey::AppTitle),
            AppRoute::Tools => self.catalog.text(AppTextKey::Tools),
            AppRoute::Settings => self.catalog.text(AppTextKey::Settings),
        }
    }

    fn render_page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let content = match self.model.route {
            AppRoute::Profile => self.render_profile(cx).into_any_element(),
            AppRoute::Chat => self.render_chat().into_any_element(),
            AppRoute::Tools => self
                .render_placeholder(AppTextKey::ToolsPlaceholder)
                .into_any_element(),
            AppRoute::Settings => self
                .render_placeholder(AppTextKey::SettingsPlaceholder)
                .into_any_element(),
        };

        let page = div().relative().flex().justify_center().size_full().p_10();

        if self.model.jsonc_open {
            page.overflow_hidden().child(content).into_any_element()
        } else {
            page.overflow_y_scrollbar()
                .child(content)
                .into_any_element()
        }
    }

    fn request_toast_animation_frame(&self, window: &mut Window) {
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
            .child(self.render_title_bar(cx))
            .child(self.render_page(cx))
            .child(self.render_toolbar(cx))
            .when(self.model.jsonc_open, |this| {
                this.child(self.render_jsonc_overlay(cx))
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

fn profile_id(model: &AppViewModel) -> String {
    model
        .selected_profile()
        .map(|profile| profile.profile_id.clone())
        .unwrap_or_default()
}

fn display_name(model: &AppViewModel) -> String {
    model
        .selected_profile()
        .map(|profile| profile.display_name.clone())
        .unwrap_or_default()
}

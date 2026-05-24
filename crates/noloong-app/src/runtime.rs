use crate::{APP_ID, APP_NAME, AppError, AppLaunchOptions, AppViewModel, view::NoloongAppView};
use crate::{SaveSettings, ToggleJsoncEditor, ValidateSettings};
use gpui::{
    App, AppContext as _, Bounds, KeyBinding, WindowBounds, WindowOptions, point, px, size,
};
use gpui_component::{Theme, ThemeMode, TitleBar};
use gpui_platform::application;

pub fn run_app(options: AppLaunchOptions) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    if crate::macos_bundle::should_relaunch_in_bundle() {
        return crate::macos_bundle::launch_in_bundle(options);
    }

    let model = AppViewModel::load(options)?;
    let app = application();
    app.on_reopen({
        let model = model.clone();
        move |cx| {
            if cx.windows().is_empty() {
                open_app_window(model.clone(), cx);
            }
        }
    });
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        cx.bind_keys([
            KeyBinding::new(
                "cmd-shift-j",
                ToggleJsoncEditor,
                Some(crate::APP_KEY_CONTEXT),
            ),
            KeyBinding::new("cmd-s", SaveSettings, Some(crate::APP_KEY_CONTEXT)),
            KeyBinding::new("cmd-enter", ValidateSettings, Some(crate::APP_KEY_CONTEXT)),
        ]);
        Theme::change(ThemeMode::Dark, None, cx);
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        open_app_window(model.clone(), cx);
        cx.activate(true);
    });
    Ok(())
}

fn open_app_window(model: AppViewModel, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1180.0), px(780.0)), cx);
    let mut titlebar = TitleBar::title_bar_options();
    titlebar.title = Some(APP_NAME.into());
    titlebar.traffic_light_position = Some(point(px(14.0), px(17.0)));
    let window_options = WindowOptions {
        app_id: Some(APP_ID.to_string()),
        titlebar: Some(titlebar),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    };

    cx.open_window(window_options, move |window, cx| {
        window.set_window_title(APP_NAME);
        window.set_app_id(APP_ID);
        window.activate_window();
        let view = cx.new(|cx| NoloongAppView::new(model, window, cx));
        cx.new(|cx| gpui_component::Root::new(view, window, cx))
    })
    .expect("failed to open noloong app window");
}

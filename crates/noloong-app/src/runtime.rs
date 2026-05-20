use crate::{AppError, AppLaunchOptions, AppViewModel, view::NoloongAppView};
use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, point, px, size};
use gpui_component::{Theme, ThemeMode, TitleBar};
use gpui_platform::application;

pub fn run_app(options: AppLaunchOptions) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    if crate::macos_bundle::should_relaunch_in_bundle() {
        return crate::macos_bundle::launch_in_bundle(options);
    }

    let model = AppViewModel::load(options)?;
    application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        let bounds = Bounds::centered(None, size(px(1180.0), px(780.0)), cx);
        let mut titlebar = TitleBar::title_bar_options();
        titlebar.traffic_light_position = Some(point(px(14.0), px(17.0)));
        let window_options = WindowOptions {
            titlebar: Some(titlebar),
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        cx.spawn({
            let model = model.clone();
            async move |cx| {
                cx.open_window(window_options, {
                    let model = model.clone();
                    move |window, cx| {
                        let view = cx.new(|cx| NoloongAppView::new(model, window, cx));
                        cx.new(|cx| gpui_component::Root::new(view, window, cx))
                    }
                })
                .expect("failed to open noloong app window");
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}

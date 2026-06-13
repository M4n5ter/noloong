use crate::{AppError, AppLaunchOptions};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const OPEN_SETTINGS_MENU_ID: &str = "open-settings";
const SETTINGS_WINDOW_LABEL: &str = "settings";

#[derive(Clone)]
pub(crate) struct AppState {
    launch_options: AppLaunchOptions,
}

impl AppState {
    pub(crate) fn launch_options(&self) -> &AppLaunchOptions {
        &self.launch_options
    }
}

#[tauri::command]
fn app_bootstrap(state: tauri::State<'_, AppState>) -> AppLaunchOptions {
    app_bootstrap_payload(state.inner())
}

#[tauri::command]
fn app_open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    open_settings_window(&app)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn app_bootstrap_payload(state: &AppState) -> AppLaunchOptions {
    state.launch_options.clone()
}

pub fn run_app(options: AppLaunchOptions) -> Result<(), AppError> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .menu(app_menu)
        .on_menu_event(|app, event| {
            if event.id() == OPEN_SETTINGS_MENU_ID {
                let _ = open_settings_window(app);
            }
        })
        .manage(AppState {
            launch_options: options.with_current_app_version(),
        })
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| anyhow::anyhow!("main webview window is missing"))?;
            window.show()?;
            window.set_focus()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_bootstrap,
            app_open_settings_window,
            crate::profile_config::app_profile_config_load,
            crate::profile_config::app_profile_config_validate,
            crate::profile_config::app_profile_config_save,
            crate::profile_config::app_profile_config_schema,
            crate::profile_config::app_profile_config_completions,
            crate::render_probe::app_render_probe_enabled,
            crate::render_probe::app_render_probe_report,
            crate::runtime_control::app_runtime_restart_interaction
        ])
        .run(tauri::generate_context!())
        .map_err(|error| AppError::Launch(error.to_string()))
}

fn open_settings_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        window.show()?;
        window.set_focus()?;
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(
        app,
        SETTINGS_WINDOW_LABEL,
        WebviewUrl::App("index.html?surface=settings".into()),
    )
    .title("Settings")
    .inner_size(920.0, 720.0)
    .min_inner_size(760.0, 560.0)
    .resizable(true)
    .minimizable(false)
    .maximizable(false)
    .focused(true)
    .build()?;
    window.show()?;
    window.set_focus()?;
    Ok(window)
}

fn app_menu<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    #[cfg(target_os = "macos")]
    {
        use tauri::menu::{AboutMetadata, Menu, MenuItemBuilder, PredefinedMenuItem, Submenu};

        let pkg_info = app_handle.package_info();
        let config = app_handle.config();
        let about_metadata = AboutMetadata {
            name: Some(pkg_info.name.clone()),
            version: Some(pkg_info.version.to_string()),
            copyright: config.bundle.copyright.clone(),
            authors: config.bundle.publisher.clone().map(|publisher| vec![publisher]),
            ..Default::default()
        };

        let settings_item = MenuItemBuilder::with_id(OPEN_SETTINGS_MENU_ID, "Settings...")
            .accelerator("CmdOrCtrl+,")
            .build(app_handle)?;

        let app_submenu = Submenu::with_items(
            app_handle,
            pkg_info.name.clone(),
            true,
            &[
                &PredefinedMenuItem::about(app_handle, None, Some(about_metadata))?,
                &PredefinedMenuItem::separator(app_handle)?,
                &settings_item,
                &PredefinedMenuItem::separator(app_handle)?,
                &PredefinedMenuItem::services(app_handle, None)?,
                &PredefinedMenuItem::separator(app_handle)?,
                &PredefinedMenuItem::hide(app_handle, None)?,
                &PredefinedMenuItem::hide_others(app_handle, None)?,
                &PredefinedMenuItem::separator(app_handle)?,
                &PredefinedMenuItem::quit(app_handle, None)?,
            ],
        )?;

        let file_submenu = Submenu::with_items(
            app_handle,
            "File",
            true,
            &[&PredefinedMenuItem::close_window(app_handle, None)?],
        )?;

        let edit_submenu = Submenu::with_items(
            app_handle,
            "Edit",
            true,
            &[
                &PredefinedMenuItem::undo(app_handle, None)?,
                &PredefinedMenuItem::redo(app_handle, None)?,
                &PredefinedMenuItem::separator(app_handle)?,
                &PredefinedMenuItem::cut(app_handle, None)?,
                &PredefinedMenuItem::copy(app_handle, None)?,
                &PredefinedMenuItem::paste(app_handle, None)?,
                &PredefinedMenuItem::select_all(app_handle, None)?,
            ],
        )?;

        let view_submenu = Submenu::with_items(
            app_handle,
            "View",
            true,
            &[&PredefinedMenuItem::fullscreen(app_handle, None)?],
        )?;

        let window_submenu = Submenu::with_items(
            app_handle,
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(app_handle, None)?,
                &PredefinedMenuItem::maximize(app_handle, None)?,
                &PredefinedMenuItem::separator(app_handle)?,
                &PredefinedMenuItem::close_window(app_handle, None)?,
            ],
        )?;

        let help_submenu = Submenu::with_items(app_handle, "Help", true, &[])?;

        return Menu::with_items(
            app_handle,
            &[
                &app_submenu,
                &file_submenu,
                &edit_submenu,
                &view_submenu,
                &window_submenu,
                &help_submenu,
            ],
        );
    }

    #[cfg(not(target_os = "macos"))]
    {
        tauri::menu::Menu::default(app_handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bootstrap_payload_returns_managed_launch_options() {
        let state = AppState {
            launch_options: AppLaunchOptions {
                app_version: "test-version".into(),
                profile_config_path: Some("/tmp/profile.jsonc".into()),
                locale: None,
                interaction_endpoint: None,
                interaction_status: None,
                runtime_control_endpoint: None,
            },
        };

        let payload = app_bootstrap_payload(&state);

        assert_eq!(payload.app_version, "test-version");
        assert_eq!(
            payload.profile_config_path.as_deref(),
            Some("/tmp/profile.jsonc")
        );
    }
}

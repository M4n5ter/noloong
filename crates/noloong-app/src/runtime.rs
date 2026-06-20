use crate::{AppError, AppLaunchOptions};
use serde::Deserialize;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const OPEN_SETTINGS_MENU_ID: &str = "open-settings";
const FOCUS_COMPOSER_MENU_ID: &str = "focus-composer";
const SEND_MESSAGE_MENU_ID: &str = "send-message";
const STOP_RESPONSE_MENU_ID: &str = "stop-response";
const CLEAR_COMPOSER_MENU_ID: &str = "clear-composer";
const SETTINGS_WINDOW_LABEL: &str = "settings";
const MAIN_WINDOW_LABEL: &str = "main";
const CONVERSATION_MENU_COMMAND_EVENT: &str = "noloong-conversation-menu-command";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConversationMenuEntry {
    Command(ConversationMenuCommandSpec),
    Separator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConversationMenuCommandSpec {
    id: &'static str,
    title: &'static str,
    accelerator: Option<&'static str>,
    initially_enabled: bool,
}

const CONVERSATION_MENU_ENTRIES: [ConversationMenuEntry; 5] = [
    ConversationMenuEntry::Command(ConversationMenuCommandSpec {
        id: FOCUS_COMPOSER_MENU_ID,
        title: "Focus Composer",
        accelerator: Some("CmdOrCtrl+L"),
        initially_enabled: true,
    }),
    ConversationMenuEntry::Command(ConversationMenuCommandSpec {
        id: SEND_MESSAGE_MENU_ID,
        title: "Send Message",
        accelerator: Some("CmdOrCtrl+Enter"),
        initially_enabled: false,
    }),
    ConversationMenuEntry::Command(ConversationMenuCommandSpec {
        id: STOP_RESPONSE_MENU_ID,
        title: "Stop Run",
        accelerator: Some("Esc"),
        initially_enabled: false,
    }),
    ConversationMenuEntry::Separator,
    ConversationMenuEntry::Command(ConversationMenuCommandSpec {
        id: CLEAR_COMPOSER_MENU_ID,
        title: "Clear Composer",
        accelerator: None,
        initially_enabled: false,
    }),
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationMenuState {
    can_focus_composer: bool,
    can_send_message: bool,
    can_stop_response: bool,
    can_clear_composer: bool,
}

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

#[tauri::command]
fn app_update_conversation_menu_state(
    app: tauri::AppHandle,
    state: ConversationMenuState,
) -> Result<(), String> {
    update_conversation_menu_state(&app, state).map_err(|error| error.to_string())
}

fn app_bootstrap_payload(state: &AppState) -> AppLaunchOptions {
    state.launch_options.clone()
}

pub fn run_app(options: AppLaunchOptions) -> Result<(), AppError> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .menu(app_menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN_SETTINGS_MENU_ID => {
                let _ = open_settings_window(app);
            }
            command if is_conversation_menu_command(command) => {
                let _ = app.emit_to(
                    MAIN_WINDOW_LABEL,
                    CONVERSATION_MENU_COMMAND_EVENT,
                    event.id().as_ref(),
                );
            }
            _ => {}
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
            crate::runtime_control::app_runtime_restart_interaction,
            app_update_conversation_menu_state
        ])
        .run(tauri::generate_context!())
        .map_err(|error| AppError::Launch(error.to_string()))
}

fn update_conversation_menu_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: ConversationMenuState,
) -> tauri::Result<()> {
    set_menu_item_enabled(app, FOCUS_COMPOSER_MENU_ID, state.can_focus_composer)?;
    set_menu_item_enabled(app, SEND_MESSAGE_MENU_ID, state.can_send_message)?;
    set_menu_item_enabled(app, STOP_RESPONSE_MENU_ID, state.can_stop_response)?;
    set_menu_item_enabled(app, CLEAR_COMPOSER_MENU_ID, state.can_clear_composer)
}

fn set_menu_item_enabled<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    id: &str,
    enabled: bool,
) -> tauri::Result<()> {
    if let Some(menu) = app.menu() {
        set_menu_item_enabled_in_items(menu.items()?, id, enabled)?;
    }
    Ok(())
}

fn set_menu_item_enabled_in_items<R: tauri::Runtime>(
    items: Vec<tauri::menu::MenuItemKind<R>>,
    id: &str,
    enabled: bool,
) -> tauri::Result<bool> {
    for item in items {
        if item.id().as_ref() == id
            && let Some(menu_item) = item.as_menuitem()
        {
            menu_item.set_enabled(enabled)?;
            return Ok(true);
        }
        if let tauri::menu::MenuItemKind::Submenu(submenu) = item
            && set_menu_item_enabled_in_items(submenu.items()?, id, enabled)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_conversation_menu_command(id: &str) -> bool {
    conversation_menu_command_specs()
        .iter()
        .any(|spec| spec.id == id)
}

fn conversation_menu_command_specs() -> Vec<ConversationMenuCommandSpec> {
    CONVERSATION_MENU_ENTRIES
        .iter()
        .filter_map(|entry| match entry {
            ConversationMenuEntry::Command(spec) => Some(*spec),
            ConversationMenuEntry::Separator => None,
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn conversation_menu_command_item<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    spec: ConversationMenuCommandSpec,
) -> tauri::Result<tauri::menu::MenuItem<R>> {
    let mut builder =
        tauri::menu::MenuItemBuilder::with_id(spec.id, spec.title).enabled(spec.initially_enabled);
    if let Some(accelerator) = spec.accelerator {
        builder = builder.accelerator(accelerator);
    }
    builder.build(app_handle)
}

#[cfg(target_os = "macos")]
enum BuiltConversationMenuEntry<R: tauri::Runtime> {
    Command(tauri::menu::MenuItem<R>),
    Separator(tauri::menu::PredefinedMenuItem<R>),
}

#[cfg(target_os = "macos")]
impl<R: tauri::Runtime> BuiltConversationMenuEntry<R> {
    fn as_menu_item(&self) -> &dyn tauri::menu::IsMenuItem<R> {
        match self {
            BuiltConversationMenuEntry::Command(item) => item,
            BuiltConversationMenuEntry::Separator(item) => item,
        }
    }
}

#[cfg(target_os = "macos")]
fn conversation_menu_entries<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> tauri::Result<Vec<BuiltConversationMenuEntry<R>>> {
    CONVERSATION_MENU_ENTRIES
        .iter()
        .map(|entry| match entry {
            ConversationMenuEntry::Command(spec) => {
                conversation_menu_command_item(app_handle, *spec)
                    .map(BuiltConversationMenuEntry::Command)
            }
            ConversationMenuEntry::Separator => {
                tauri::menu::PredefinedMenuItem::separator(app_handle)
                    .map(BuiltConversationMenuEntry::Separator)
            }
        })
        .collect()
}

fn open_settings_window<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> tauri::Result<WebviewWindow<R>> {
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

fn app_menu<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> tauri::Result<tauri::menu::Menu<R>> {
    #[cfg(target_os = "macos")]
    {
        use tauri::menu::{AboutMetadata, Menu, MenuItemBuilder, PredefinedMenuItem, Submenu};

        let pkg_info = app_handle.package_info();
        let config = app_handle.config();
        let about_metadata = AboutMetadata {
            name: Some(pkg_info.name.clone()),
            version: Some(pkg_info.version.to_string()),
            copyright: config.bundle.copyright.clone(),
            authors: config
                .bundle
                .publisher
                .clone()
                .map(|publisher| vec![publisher]),
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

        let conversation_menu_entries = conversation_menu_entries(app_handle)?;
        let conversation_menu_refs = conversation_menu_entries
            .iter()
            .map(|entry| entry.as_menu_item())
            .collect::<Vec<&dyn tauri::menu::IsMenuItem<R>>>();
        let conversation_submenu =
            Submenu::with_items(app_handle, "Conversation", true, &conversation_menu_refs)?;

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

        Menu::with_items(
            app_handle,
            &[
                &app_submenu,
                &file_submenu,
                &edit_submenu,
                &view_submenu,
                &conversation_submenu,
                &window_submenu,
                &help_submenu,
            ],
        )
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

    #[test]
    fn conversation_menu_command_specs_are_discoverable_and_actionable() {
        assert_eq!(
            CONVERSATION_MENU_ENTRIES,
            [
                ConversationMenuEntry::Command(ConversationMenuCommandSpec {
                    id: FOCUS_COMPOSER_MENU_ID,
                    title: "Focus Composer",
                    accelerator: Some("CmdOrCtrl+L"),
                    initially_enabled: true,
                }),
                ConversationMenuEntry::Command(ConversationMenuCommandSpec {
                    id: SEND_MESSAGE_MENU_ID,
                    title: "Send Message",
                    accelerator: Some("CmdOrCtrl+Enter"),
                    initially_enabled: false,
                }),
                ConversationMenuEntry::Command(ConversationMenuCommandSpec {
                    id: STOP_RESPONSE_MENU_ID,
                    title: "Stop Run",
                    accelerator: Some("Esc"),
                    initially_enabled: false,
                }),
                ConversationMenuEntry::Separator,
                ConversationMenuEntry::Command(ConversationMenuCommandSpec {
                    id: CLEAR_COMPOSER_MENU_ID,
                    title: "Clear Composer",
                    accelerator: None,
                    initially_enabled: false,
                }),
            ],
        );

        assert_eq!(
            conversation_menu_command_specs()
                .iter()
                .map(|spec| spec.id)
                .collect::<Vec<_>>(),
            [
                FOCUS_COMPOSER_MENU_ID,
                SEND_MESSAGE_MENU_ID,
                STOP_RESPONSE_MENU_ID,
                CLEAR_COMPOSER_MENU_ID,
            ],
        );

        assert!(is_conversation_menu_command(FOCUS_COMPOSER_MENU_ID));
        assert!(is_conversation_menu_command(SEND_MESSAGE_MENU_ID));
        assert!(is_conversation_menu_command(STOP_RESPONSE_MENU_ID));
        assert!(is_conversation_menu_command(CLEAR_COMPOSER_MENU_ID));
        assert!(!is_conversation_menu_command(OPEN_SETTINGS_MENU_ID));
    }
}

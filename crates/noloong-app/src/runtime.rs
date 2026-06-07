use crate::{AppError, AppLaunchOptions};

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

fn app_bootstrap_payload(state: &AppState) -> AppLaunchOptions {
    state.launch_options.clone()
}

pub fn run_app(options: AppLaunchOptions) -> Result<(), AppError> {
    tauri::Builder::default()
        .manage(AppState {
            launch_options: options.with_current_app_version(),
        })
        .invoke_handler(tauri::generate_handler![
            app_bootstrap,
            crate::profile_config::app_profile_config_load,
            crate::profile_config::app_profile_config_validate,
            crate::profile_config::app_profile_config_save,
            crate::profile_config::app_profile_config_schema,
            crate::profile_config::app_profile_config_completions
        ])
        .run(tauri::generate_context!())
        .map_err(|error| AppError::Launch(error.to_string()))
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

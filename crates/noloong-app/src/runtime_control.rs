use crate::{AppError, AppInteractionEndpoint, AppInteractionStatus, AppLaunchOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppRuntimeControlEndpoint {
    pub http_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppRuntimeRestartResult {
    pub interaction_endpoint: AppInteractionEndpoint,
    pub interaction_status: AppInteractionStatus,
}

#[derive(Debug, Error)]
pub enum AppRuntimeControlError {
    #[error("runtime control is unavailable")]
    Unavailable,
    #[error("runtime control HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("runtime control request failed: {0}")]
    Request(String),
    #[error("runtime control response failed: {0}")]
    Response(String),
}

#[tauri::command]
pub(crate) async fn app_runtime_restart_interaction(
    state: tauri::State<'_, crate::runtime::AppState>,
) -> Result<AppRuntimeRestartResult, String> {
    restart_interaction(state.inner().launch_options())
        .await
        .map_err(|error| error.to_string())
}

pub async fn restart_interaction(
    options: &AppLaunchOptions,
) -> Result<AppRuntimeRestartResult, AppRuntimeControlError> {
    let endpoint = options
        .runtime_control_endpoint
        .as_ref()
        .ok_or(AppRuntimeControlError::Unavailable)?;
    let client = reqwest::Client::new();
    let mut request = client.post(format!("{}/runtime/restart_interaction", endpoint.http_url));
    if let Some(token) = &endpoint.bearer_token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|error| AppRuntimeControlError::Request(error.to_string()))?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let message = response.text().await.unwrap_or_default();
        return Err(AppRuntimeControlError::Http { status, message });
    }
    response
        .json::<AppRuntimeRestartResult>()
        .await
        .map_err(|error| AppRuntimeControlError::Response(error.to_string()))
}

impl From<AppRuntimeControlError> for AppError {
    fn from(error: AppRuntimeControlError) -> Self {
        AppError::Launch(error.to_string())
    }
}

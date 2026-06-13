use crate::cli::{
    CliError, EmbeddedInteractionServer, default_embedded_interaction_bind, generate_token,
    initialize_app_interaction, start_embedded_interaction,
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    routing::post,
};
use noloong_app::{AppInteractionStatus, AppRuntimeControlEndpoint, AppRuntimeRestartResult};
use std::sync::Arc;
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

pub(crate) struct RuntimeControlInteractionManager {
    current_server: Mutex<Option<EmbeddedInteractionServer>>,
}

pub(crate) struct AppRuntimeControlServer {
    endpoint: AppRuntimeControlEndpoint,
    server_task: JoinHandle<Result<(), std::io::Error>>,
}

#[derive(Clone)]
struct AppRuntimeControlState {
    profile_config_path: Option<String>,
    token: String,
    interaction_manager: Arc<RuntimeControlInteractionManager>,
}

impl RuntimeControlInteractionManager {
    pub(crate) fn new(server: EmbeddedInteractionServer) -> Self {
        Self {
            current_server: Mutex::new(Some(server)),
        }
    }

    async fn restart(
        &self,
        profile_config_path: Option<String>,
    ) -> Result<AppRuntimeRestartResult, (StatusCode, String)> {
        let embedded = start_embedded_interaction(profile_config_path)
            .await
            .map_err(internal_control_error)?;
        let server = embedded
            .start_server()
            .await
            .map_err(internal_control_error)?;
        let interaction_endpoint = server.endpoint();
        let interaction_status = initialize_app_interaction(Some(&interaction_endpoint))
            .await
            .unwrap_or(AppInteractionStatus::Unavailable);
        if !matches!(interaction_status, AppInteractionStatus::Ready { .. }) {
            server.shutdown().await;
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("restarted interaction did not initialize: {interaction_status:?}"),
            ));
        }

        let previous = {
            let mut current_server = self.current_server.lock().await;
            current_server.replace(server)
        };
        if let Some(previous) = previous {
            previous.shutdown().await;
        }
        Ok(AppRuntimeRestartResult {
            interaction_endpoint,
            interaction_status,
        })
    }

    pub(crate) async fn shutdown(&self) {
        let current_server = {
            let mut current_server = self.current_server.lock().await;
            current_server.take()
        };
        if let Some(current_server) = current_server {
            current_server.shutdown().await;
        }
    }
}

impl AppRuntimeControlServer {
    pub(crate) fn endpoint(&self) -> AppRuntimeControlEndpoint {
        self.endpoint.clone()
    }

    pub(crate) async fn shutdown(self) {
        self.server_task.abort();
        let _ = self.server_task.await;
    }
}

pub(crate) async fn start_app_runtime_control_server(
    profile_config_path: Option<String>,
    interaction_manager: Arc<RuntimeControlInteractionManager>,
) -> Result<AppRuntimeControlServer, CliError> {
    let token = generate_token()?;
    let listener = TcpListener::bind(default_embedded_interaction_bind()).await?;
    let address = listener.local_addr()?;
    let state = Arc::new(AppRuntimeControlState {
        profile_config_path,
        token: token.clone(),
        interaction_manager,
    });
    let router = Router::new()
        .route(
            "/runtime/restart_interaction",
            post(app_runtime_restart_interaction),
        )
        .with_state(state);
    let server_task = tokio::spawn(async move { axum::serve(listener, router).await });
    Ok(AppRuntimeControlServer {
        endpoint: AppRuntimeControlEndpoint {
            http_url: format!("http://{address}"),
            bearer_token: Some(token),
        },
        server_task,
    })
}

async fn app_runtime_restart_interaction(
    State(state): State<Arc<AppRuntimeControlState>>,
    headers: HeaderMap,
) -> Result<Json<AppRuntimeRestartResult>, (StatusCode, String)> {
    authorize_runtime_control(&headers, &state.token)?;
    state
        .interaction_manager
        .restart(state.profile_config_path.clone())
        .await
        .map(Json)
}

fn authorize_runtime_control(headers: &HeaderMap, token: &str) -> Result<(), (StatusCode, String)> {
    let expected = format!("Bearer {token}");
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected);
    if authorized {
        return Ok(());
    }
    Err((StatusCode::UNAUTHORIZED, "unauthorized".into()))
}

fn internal_control_error(error: CliError) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

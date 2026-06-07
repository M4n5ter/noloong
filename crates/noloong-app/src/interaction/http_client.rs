use super::{
    AppApprovalResolveRequest, AppDisplaySubscribeRequest, AppInteractionEndpoint,
    AppInteractionSessionDescriptor, AppInteractionStatus, AppPromptRequest,
    AppSessionCreateRequest, AppSessionListRequest, AppSessionMetadataUpdateRequest,
    AppSessionRequest, AppSubscriptionResult, InteractionInitializeRequest,
    InteractionInitializeResult,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use thiserror::Error;
use url::Url;

pub trait AppInteractionClient {
    fn initialize(
        &self,
        request: InteractionInitializeRequest,
    ) -> impl Future<Output = Result<InteractionInitializeResult, AppInteractionError>> + Send + '_;

    fn list_sessions(
        &self,
        request: AppSessionListRequest,
    ) -> impl Future<Output = Result<Vec<AppInteractionSessionDescriptor>, AppInteractionError>>
    + Send
    + '_ {
        async {
            let _ = request;
            Err(AppInteractionError::Protocol(
                "session/list is not implemented for this interaction client".into(),
            ))
        }
    }

    fn get_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> impl Future<Output = Result<AppInteractionSessionDescriptor, AppInteractionError>> + Send + 'a
    {
        async move {
            let _ = session_id;
            Err(AppInteractionError::Protocol(
                "session/get is not implemented for this interaction client".into(),
            ))
        }
    }

    fn create_session(
        &self,
        request: AppSessionCreateRequest,
    ) -> impl Future<Output = Result<AppInteractionSessionDescriptor, AppInteractionError>> + Send + '_
    {
        async move {
            let _ = request;
            Err(AppInteractionError::Protocol(
                "session/create is not implemented for this interaction client".into(),
            ))
        }
    }

    fn update_session_metadata(
        &self,
        request: AppSessionMetadataUpdateRequest,
    ) -> impl Future<Output = Result<AppInteractionSessionDescriptor, AppInteractionError>> + Send + '_
    {
        async move {
            let _ = request;
            Err(AppInteractionError::Protocol(
                "session/update_metadata is not implemented for this interaction client".into(),
            ))
        }
    }

    fn prompt(
        &self,
        request: AppPromptRequest,
    ) -> impl Future<Output = Result<AppInteractionSessionDescriptor, AppInteractionError>> + Send + '_
    {
        async move {
            let _ = request;
            Err(AppInteractionError::Protocol(
                "agent/prompt is not implemented for this interaction client".into(),
            ))
        }
    }

    fn abort(
        &self,
        request: AppSessionRequest,
    ) -> impl Future<Output = Result<AppInteractionSessionDescriptor, AppInteractionError>> + Send + '_
    {
        async move {
            let _ = request;
            Err(AppInteractionError::Protocol(
                "agent/abort is not implemented for this interaction client".into(),
            ))
        }
    }

    fn resolve_approval(
        &self,
        request: AppApprovalResolveRequest,
    ) -> impl Future<Output = Result<AppInteractionSessionDescriptor, AppInteractionError>> + Send + '_
    {
        async move {
            let _ = request;
            Err(AppInteractionError::Protocol(
                "approval/resolve is not implemented for this interaction client".into(),
            ))
        }
    }

    fn subscribe_display(
        &self,
        request: AppDisplaySubscribeRequest,
    ) -> impl Future<Output = Result<AppSubscriptionResult, AppInteractionError>> + Send + '_ {
        async move {
            let _ = request;
            Err(AppInteractionError::Protocol(
                "display/subscribe is not implemented for this interaction client".into(),
            ))
        }
    }
}

#[derive(Clone)]
pub struct AppInteractionHttpClient {
    client: reqwest::Client,
    http_url: String,
    bearer_token: Option<String>,
    request_id: Arc<AtomicU64>,
}

impl AppInteractionHttpClient {
    pub fn from_endpoint(endpoint: &AppInteractionEndpoint) -> Result<Self, AppInteractionError> {
        Ok(Self {
            client: reqwest::Client::new(),
            http_url: interaction_http_url(&endpoint.ws_url)?,
            bearer_token: endpoint.bearer_token.clone(),
            request_id: Arc::new(AtomicU64::new(0)),
        })
    }

    async fn call<P, R>(&self, method: &'static str, params: P) -> Result<R, AppInteractionError>
    where
        P: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let mut http_request = self.client.post(&self.http_url);
        if let Some(token) = &self.bearer_token {
            http_request = http_request.bearer_auth(token);
        }
        let id = self.request_id.fetch_add(1, Ordering::SeqCst) + 1;
        let response = http_request
            .json(&JsonRpcRequest {
                jsonrpc: "2.0",
                id,
                method,
                params,
            })
            .send()
            .await
            .map_err(|error| AppInteractionError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| AppInteractionError::Transport(error.to_string()))?
            .json::<JsonRpcResponse<R>>()
            .await
            .map_err(|error| AppInteractionError::Protocol(error.to_string()))?;
        response.into_result()
    }
}

impl AppInteractionClient for AppInteractionHttpClient {
    async fn initialize(
        &self,
        request: InteractionInitializeRequest,
    ) -> Result<InteractionInitializeResult, AppInteractionError> {
        self.call("initialize", request).await
    }

    async fn list_sessions(
        &self,
        request: AppSessionListRequest,
    ) -> Result<Vec<AppInteractionSessionDescriptor>, AppInteractionError> {
        self.call("session/list", request).await
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.call(
            "session/get",
            AppSessionRequest {
                session_id: session_id.into(),
            },
        )
        .await
    }

    async fn create_session(
        &self,
        request: AppSessionCreateRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.call("session/create", request).await
    }

    async fn update_session_metadata(
        &self,
        request: AppSessionMetadataUpdateRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.call("session/update_metadata", request).await
    }

    async fn prompt(
        &self,
        request: AppPromptRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.call("agent/prompt", request).await
    }

    async fn abort(
        &self,
        request: AppSessionRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.call("agent/abort", request).await
    }

    async fn resolve_approval(
        &self,
        request: AppApprovalResolveRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.call("approval/resolve", request).await
    }

    async fn subscribe_display(
        &self,
        request: AppDisplaySubscribeRequest,
    ) -> Result<AppSubscriptionResult, AppInteractionError> {
        self.call("display/subscribe", request).await
    }
}

pub async fn initialize_interaction_status(
    client: &impl AppInteractionClient,
) -> AppInteractionStatus {
    match client
        .initialize(InteractionInitializeRequest::noloong_app())
        .await
    {
        Ok(result) => AppInteractionStatus::Ready {
            server_name: result.server.name,
            protocol_version: result.server.protocol_version,
            profiles: result.profiles,
        },
        Err(error) => AppInteractionStatus::Failed {
            error: error.to_string(),
        },
    }
}

#[derive(Serialize)]
struct JsonRpcRequest<P> {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: P,
}

#[derive(Deserialize)]
struct JsonRpcResponse<R> {
    result: Option<R>,
    error: Option<JsonRpcError>,
}

impl<R> JsonRpcResponse<R> {
    fn into_result(self) -> Result<R, AppInteractionError> {
        if let Some(result) = self.result {
            return Ok(result);
        }
        if let Some(error) = self.error {
            return Err(AppInteractionError::Protocol(format!(
                "json-rpc error {}: {}",
                error.code, error.message
            )));
        }
        Err(AppInteractionError::Protocol(
            "json-rpc response missing result and error".into(),
        ))
    }
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AppInteractionError {
    #[error("{0}")]
    Transport(String),
    #[error("{0}")]
    Protocol(String),
}

pub fn interaction_http_url(ws_url: &str) -> Result<String, AppInteractionError> {
    let mut url =
        Url::parse(ws_url).map_err(|error| AppInteractionError::Protocol(error.to_string()))?;
    let scheme = match url.scheme() {
        "ws" => "http",
        "wss" => "https",
        other => {
            return Err(AppInteractionError::Protocol(format!(
                "unsupported interaction websocket scheme: {other}"
            )));
        }
    };
    url.set_scheme(scheme).map_err(|_| {
        AppInteractionError::Protocol("failed to set interaction URL scheme".into())
    })?;
    let path = url.path().to_string();
    let http_path = path.strip_suffix("/ws").ok_or_else(|| {
        AppInteractionError::Protocol(format!(
            "interaction websocket URL must end with /ws: {ws_url}"
        ))
    })?;
    url.set_path(http_path);
    Ok(url.to_string())
}

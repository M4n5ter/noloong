use noloong_config::ManifestPatch;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::BTreeSet, future::Future, sync::Arc};
use thiserror::Error;
use url::Url;

mod ws;

pub use ws::{AppInteractionWsClient, AppInteractionWsNotification};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInteractionEndpoint {
    pub ws_url: String,
    pub bearer_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppInteractionStatus {
    Unavailable,
    Pending,
    Ready {
        server_name: String,
        protocol_version: String,
        profiles: Vec<InteractionProfileDescriptor>,
    },
    Failed(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionInitializeRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub requested_authority: BTreeSet<InteractionAuthorityCapability>,
    #[serde(default)]
    pub requested_ux: InteractionUxCapabilities,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl InteractionInitializeRequest {
    pub fn noloong_app() -> Self {
        Self {
            name: "noloong-app".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            requested_authority: [
                InteractionAuthorityCapability::AgentRun,
                InteractionAuthorityCapability::ApprovalResolve,
                InteractionAuthorityCapability::SessionDelete,
            ]
            .into_iter()
            .collect(),
            requested_ux: InteractionUxCapabilities {
                display_events: true,
                stream_text: true,
                edit_message: true,
                markdown: true,
                max_message_bytes: None,
            },
            metadata: Map::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum InteractionAuthorityCapability {
    #[serde(rename = "agent.run")]
    AgentRun,
    #[serde(rename = "approval.resolve")]
    ApprovalResolve,
    #[serde(rename = "session.delete")]
    SessionDelete,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionUxCapabilities {
    #[serde(default)]
    pub display_events: bool,
    #[serde(default)]
    pub stream_text: bool,
    #[serde(default)]
    pub edit_message: bool,
    #[serde(default)]
    pub markdown: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_message_bytes: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionInitializeResult {
    pub server: InteractionServerInfo,
    #[serde(default)]
    pub profiles: Vec<InteractionProfileDescriptor>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionServerInfo {
    pub name: String,
    pub protocol_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionProfileDescriptor {
    pub profile_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub default_manifest_patches: Vec<ManifestPatch>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppPromptRequest {
    pub session_id: String,
    pub input: AppPromptInput,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AppPromptInput {
    Text { text: String },
    Message { message: AppMessage },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInteractionSessionDescriptor {
    pub session_id: String,
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub status: AppInteractionSessionStatus,
    pub state: AppInteractionSessionState,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppInteractionSessionStatus {
    Idle,
    Running,
    Completed,
    Aborted,
    Failed,
    Paused,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInteractionSessionState {
    #[serde(default)]
    pub messages: Vec<AppMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppMessage {
    pub id: String,
    pub role: String,
    #[serde(default)]
    pub content: Vec<AppContentBlock>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppContentBlock {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AppDisplayEvent {
    RunStarted {
        run_id: String,
    },
    RunCompleted {
        run_id: String,
    },
    RunFailed {
        run_id: String,
        error: String,
    },
    RunPaused {
        run_id: String,
        reason: Value,
    },
    AssistantMessageDelta {
        run_id: String,
        display_message_id: String,
        text: String,
    },
    AssistantMessageFinal {
        run_id: String,
        display_message_id: String,
        message: AppMessage,
        #[serde(default)]
        truncated: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppDisplaySubscribeRequest {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ux: Option<InteractionUxCapabilities>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSubscriptionResult {
    pub subscription_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppInteractionDisplayNotification {
    pub session_id: String,
    pub subscription_id: String,
    pub event: AppDisplayEvent,
}

pub trait AppInteractionClient {
    fn initialize(
        &self,
        request: InteractionInitializeRequest,
    ) -> impl Future<Output = Result<InteractionInitializeResult, AppInteractionError>> + Send + '_;

    fn list_sessions(
        &self,
    ) -> impl Future<Output = Result<Vec<AppInteractionSessionDescriptor>, AppInteractionError>>
    + Send
    + '_ {
        async {
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
    ) -> Result<Vec<AppInteractionSessionDescriptor>, AppInteractionError> {
        self.call("session/list", serde_json::json!({})).await
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

    async fn prompt(
        &self,
        request: AppPromptRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.call("agent/prompt", request).await
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
        Err(error) => AppInteractionStatus::Failed(error.to_string()),
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

#[cfg(test)]
mod tests {
    use super::{
        AppDisplayEvent, AppDisplaySubscribeRequest, AppInteractionDisplayNotification,
        InteractionAuthorityCapability, InteractionInitializeRequest, InteractionUxCapabilities,
        interaction_http_url,
    };
    use serde_json::json;

    #[test]
    fn interaction_http_url_derives_jsonrpc_post_endpoint_from_ws_endpoint() {
        assert_eq!(
            interaction_http_url("ws://127.0.0.1:8787/jsonrpc/ws").unwrap(),
            "http://127.0.0.1:8787/jsonrpc"
        );
        assert_eq!(
            interaction_http_url("wss://noloong.example/jsonrpc/ws").unwrap(),
            "https://noloong.example/jsonrpc"
        );
    }

    #[test]
    fn noloong_app_initialize_request_asks_for_chat_authority_and_display_ux() {
        let request = InteractionInitializeRequest::noloong_app();

        assert!(
            request
                .requested_authority
                .contains(&InteractionAuthorityCapability::AgentRun)
        );
        assert!(
            request
                .requested_authority
                .contains(&InteractionAuthorityCapability::ApprovalResolve)
        );
        assert!(request.requested_ux.display_events);
        assert!(request.requested_ux.stream_text);
        assert!(request.requested_ux.markdown);
    }

    #[test]
    fn display_notification_decodes_assistant_delta() {
        let notification = serde_json::from_value::<AppInteractionDisplayNotification>(json!({
            "sessionId": "session-1",
            "subscriptionId": "subscription-1",
            "event": {
                "type": "assistant_message_delta",
                "runId": "run-1",
                "displayMessageId": "run-1:assistant",
                "text": "hello"
            }
        }))
        .unwrap();

        assert_eq!(notification.session_id, "session-1");
        assert_eq!(
            notification.event,
            AppDisplayEvent::AssistantMessageDelta {
                run_id: "run-1".into(),
                display_message_id: "run-1:assistant".into(),
                text: "hello".into(),
            }
        );
    }

    #[test]
    fn display_subscribe_request_requests_streaming_display_ux() {
        let request = AppDisplaySubscribeRequest {
            session_id: "session-1".into(),
            ux: Some(InteractionUxCapabilities {
                display_events: true,
                stream_text: true,
                edit_message: true,
                markdown: true,
                max_message_bytes: Some(65_536),
            }),
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "sessionId": "session-1",
                "ux": {
                    "displayEvents": true,
                    "streamText": true,
                    "editMessage": true,
                    "markdown": true,
                    "maxMessageBytes": 65536
                }
            })
        );
    }
}

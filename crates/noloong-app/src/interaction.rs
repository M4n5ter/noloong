use noloong_config::ManifestPatch;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeSet, future::Future};
use thiserror::Error;
use url::Url;

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

pub trait AppInteractionClient {
    fn initialize(
        &self,
        request: InteractionInitializeRequest,
    ) -> impl Future<Output = Result<InteractionInitializeResult, AppInteractionError>> + Send + '_;
}

#[derive(Clone)]
pub struct AppInteractionHttpClient {
    client: reqwest::Client,
    http_url: String,
    bearer_token: Option<String>,
}

impl AppInteractionHttpClient {
    pub fn from_endpoint(endpoint: &AppInteractionEndpoint) -> Result<Self, AppInteractionError> {
        Ok(Self {
            client: reqwest::Client::new(),
            http_url: interaction_http_url(&endpoint.ws_url)?,
            bearer_token: endpoint.bearer_token.clone(),
        })
    }
}

impl AppInteractionClient for AppInteractionHttpClient {
    async fn initialize(
        &self,
        request: InteractionInitializeRequest,
    ) -> Result<InteractionInitializeResult, AppInteractionError> {
        let mut http_request = self.client.post(&self.http_url);
        if let Some(token) = &self.bearer_token {
            http_request = http_request.bearer_auth(token);
        }
        let response = http_request
            .json(&JsonRpcRequest {
                jsonrpc: "2.0",
                id: 1,
                method: "initialize",
                params: request,
            })
            .send()
            .await
            .map_err(|error| AppInteractionError::Transport(error.to_string()))?
            .error_for_status()
            .map_err(|error| AppInteractionError::Transport(error.to_string()))?
            .json::<JsonRpcResponse<InteractionInitializeResult>>()
            .await
            .map_err(|error| AppInteractionError::Protocol(error.to_string()))?;
        response.into_result()
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
        InteractionAuthorityCapability, InteractionInitializeRequest, interaction_http_url,
    };

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
}

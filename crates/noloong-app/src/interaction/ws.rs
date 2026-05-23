use super::{
    AppDisplaySubscribeRequest, AppInteractionClient, AppInteractionDisplayNotification,
    AppInteractionEndpoint, AppInteractionError, AppInteractionSessionDescriptor, AppPromptRequest,
    AppSessionCreateRequest, AppSessionRequest, AppSubscriptionResult,
    InteractionInitializeRequest, InteractionInitializeResult,
};
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc, oneshot},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const OUTBOUND_BUFFER: usize = 128;
const NOTIFICATION_BUFFER: usize = 1024;

type ClientWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type PendingRequests =
    Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<Value, AppInteractionError>>>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInteractionWsNotification {
    pub method: String,
    pub params: Value,
}

#[derive(Clone)]
pub struct AppInteractionWsClient {
    inner: Arc<AppInteractionWsClientInner>,
}

struct AppInteractionWsClientInner {
    sender: mpsc::Sender<WsCommand>,
    pending: PendingRequests,
    notifications: broadcast::Sender<AppInteractionWsNotification>,
    next_id: AtomicU64,
}

enum WsCommand {
    Request(WsJsonRpcRequest),
}

impl AppInteractionWsClient {
    pub async fn connect(endpoint: &AppInteractionEndpoint) -> Result<Self, AppInteractionError> {
        let mut request = endpoint
            .ws_url
            .clone()
            .into_client_request()
            .map_err(|error| AppInteractionError::Transport(error.to_string()))?;
        if let Some(token) = &endpoint.bearer_token {
            request.headers_mut().insert(
                "authorization",
                HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|error| AppInteractionError::Transport(error.to_string()))?,
            );
        }
        let (websocket, _) = connect_async(request)
            .await
            .map_err(|error| AppInteractionError::Transport(error.to_string()))?;
        let (writer, reader) = websocket.split();
        let (sender, receiver) = mpsc::channel(OUTBOUND_BUFFER);
        let (notifications, _) = broadcast::channel(NOTIFICATION_BUFFER);
        let pending = Arc::new(Mutex::new(BTreeMap::new()));

        tokio::spawn(write_loop(writer, receiver, Arc::clone(&pending)));
        tokio::spawn(read_loop(
            reader,
            Arc::clone(&pending),
            notifications.clone(),
        ));

        Ok(Self {
            inner: Arc::new(AppInteractionWsClientInner {
                sender,
                pending,
                notifications,
                next_id: AtomicU64::new(1),
            }),
        })
    }

    pub fn subscribe_notifications(&self) -> broadcast::Receiver<AppInteractionWsNotification> {
        self.inner.notifications.subscribe()
    }

    pub async fn request_as<P, R>(
        &self,
        method: impl Into<String>,
        params: P,
    ) -> Result<R, AppInteractionError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let value = self.request(method, params).await?;
        serde_json::from_value(value)
            .map_err(|error| AppInteractionError::Protocol(error.to_string()))
    }

    pub async fn request<P>(
        &self,
        method: impl Into<String>,
        params: P,
    ) -> Result<Value, AppInteractionError>
    where
        P: Serialize,
    {
        let params = serde_json::to_value(params)
            .map_err(|error| AppInteractionError::Protocol(error.to_string()))?;
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .expect("interaction websocket pending lock poisoned")
            .insert(id, sender);
        let request = WsJsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        };
        if self
            .inner
            .sender
            .send(WsCommand::Request(request))
            .await
            .is_err()
        {
            self.remove_pending(id);
            return Err(AppInteractionError::Transport(
                "interaction websocket is closed".into(),
            ));
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppInteractionError::Transport(
                "interaction websocket is closed".into(),
            )),
            Err(_) => {
                self.remove_pending(id);
                Err(AppInteractionError::Transport(format!(
                    "interaction request timed out after {REQUEST_TIMEOUT:?}"
                )))
            }
        }
    }

    fn remove_pending(&self, id: u64) {
        self.inner
            .pending
            .lock()
            .expect("interaction websocket pending lock poisoned")
            .remove(&id);
    }
}

impl AppInteractionClient for AppInteractionWsClient {
    async fn initialize(
        &self,
        request: InteractionInitializeRequest,
    ) -> Result<InteractionInitializeResult, AppInteractionError> {
        self.request_as("initialize", request).await
    }

    async fn list_sessions(
        &self,
    ) -> Result<Vec<AppInteractionSessionDescriptor>, AppInteractionError> {
        self.request_as("session/list", serde_json::json!({})).await
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.request_as(
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
        self.request_as("session/create", request).await
    }

    async fn prompt(
        &self,
        request: AppPromptRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.request_as("agent/prompt", request).await
    }

    async fn abort(
        &self,
        request: AppSessionRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        self.request_as("agent/abort", request).await
    }

    async fn subscribe_display(
        &self,
        request: AppDisplaySubscribeRequest,
    ) -> Result<AppSubscriptionResult, AppInteractionError> {
        self.request_as("display/subscribe", request).await
    }
}

async fn write_loop(
    mut writer: SplitSink<ClientWebSocket, Message>,
    mut receiver: mpsc::Receiver<WsCommand>,
    pending: PendingRequests,
) {
    while let Some(command) = receiver.recv().await {
        let WsCommand::Request(request) = command;
        let id = request.id;
        let payload = match serde_json::to_string(&request) {
            Ok(payload) => payload,
            Err(error) => {
                fail_pending_request(
                    &pending,
                    id,
                    AppInteractionError::Protocol(error.to_string()),
                );
                continue;
            }
        };
        if let Err(error) = writer.send(Message::Text(payload.into())).await {
            fail_pending_request(
                &pending,
                id,
                AppInteractionError::Transport(error.to_string()),
            );
            break;
        }
    }
}

async fn read_loop(
    mut reader: SplitStream<ClientWebSocket>,
    pending: PendingRequests,
    notifications: broadcast::Sender<AppInteractionWsNotification>,
) {
    while let Some(message) = reader.next().await {
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                fail_all_pending(&pending, AppInteractionError::Transport(error.to_string()));
                break;
            }
        };
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => {
                fail_all_pending(
                    &pending,
                    AppInteractionError::Transport("interaction websocket closed".into()),
                );
                break;
            }
            _ => continue,
        };
        let incoming = match serde_json::from_str::<WsJsonRpcIncoming>(&text) {
            Ok(incoming) => incoming,
            Err(error) => {
                fail_all_pending(&pending, AppInteractionError::Protocol(error.to_string()));
                break;
            }
        };
        if let Some(method) = incoming.method {
            let _ = notifications.send(AppInteractionWsNotification {
                method,
                params: incoming.params.unwrap_or(Value::Null),
            });
            continue;
        }
        if let Some(id) = incoming.id {
            let result = incoming.into_result();
            if let Some(sender) = pending
                .lock()
                .expect("interaction websocket pending lock poisoned")
                .remove(&id)
            {
                let _ = sender.send(result);
            }
        }
    }
}

fn fail_pending_request(pending: &PendingRequests, id: u64, error: AppInteractionError) {
    if let Some(sender) = pending
        .lock()
        .expect("interaction websocket pending lock poisoned")
        .remove(&id)
    {
        let _ = sender.send(Err(error));
    }
}

fn fail_all_pending(pending: &PendingRequests, error: AppInteractionError) {
    for (_, sender) in pending
        .lock()
        .expect("interaction websocket pending lock poisoned")
        .split_off(&0)
    {
        let _ = sender.send(Err(error.clone()));
    }
}

#[derive(Serialize)]
struct WsJsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Deserialize)]
struct WsJsonRpcIncoming {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<WsJsonRpcError>,
    method: Option<String>,
    params: Option<Value>,
}

impl WsJsonRpcIncoming {
    fn into_result(self) -> Result<Value, AppInteractionError> {
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
struct WsJsonRpcError {
    code: i64,
    message: String,
}

impl AppInteractionWsNotification {
    pub fn display_event(&self) -> Result<AppInteractionDisplayNotification, AppInteractionError> {
        if self.method != "display/event" {
            return Err(AppInteractionError::Protocol(format!(
                "unexpected interaction notification method: {}",
                self.method
            )));
        }
        serde_json::from_value(self.params.clone())
            .map_err(|error| AppInteractionError::Protocol(error.to_string()))
    }
}

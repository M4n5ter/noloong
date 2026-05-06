use super::{JsonRpcErrorObject, JsonRpcNotification, JsonRpcRequest, JsonRpcResponsePayload};
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc, oneshot},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_OUTBOUND_BUFFER: usize = 1024;
const DEFAULT_NOTIFICATION_BUFFER: usize = 1024;

type ClientWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type PendingRequests = Arc<Mutex<BTreeMap<u64, oneshot::Sender<InteractionClientResult<Value>>>>>;

pub type InteractionClientResult<T> = Result<T, InteractionClientError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionWsClientConfig {
    pub url: String,
    pub bearer_token: Option<String>,
    pub request_timeout: Duration,
    pub outbound_buffer: usize,
    pub notification_buffer: usize,
}

impl InteractionWsClientConfig {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            bearer_token: None,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            outbound_buffer: DEFAULT_OUTBOUND_BUFFER,
            notification_buffer: DEFAULT_NOTIFICATION_BUFFER,
        }
    }

    pub fn bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    pub fn request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }

    pub fn outbound_buffer(mut self, outbound_buffer: usize) -> Self {
        self.outbound_buffer = outbound_buffer;
        self
    }

    pub fn notification_buffer(mut self, notification_buffer: usize) -> Self {
        self.notification_buffer = notification_buffer;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionWsNotification {
    pub method: String,
    pub params: Value,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum InteractionClientError {
    #[error("interaction websocket connection failed: {0}")]
    Connect(String),
    #[error("interaction websocket is closed")]
    Closed,
    #[error("interaction websocket transport failed: {0}")]
    Transport(String),
    #[error("interaction json-rpc encode failed: {0}")]
    Encode(String),
    #[error("interaction json-rpc decode failed: {0}")]
    Decode(String),
    #[error("interaction json-rpc protocol error: {0}")]
    Protocol(String),
    #[error("interaction request timed out after {0:?}")]
    Timeout(Duration),
    #[error("interaction json-rpc error {code}: {message}")]
    JsonRpc {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

#[derive(Clone)]
pub struct InteractionWsClient {
    inner: Arc<InteractionWsClientInner>,
}

struct InteractionWsClientInner {
    sender: mpsc::Sender<ClientCommand>,
    pending: PendingRequests,
    notifications: broadcast::Sender<InteractionWsNotification>,
    next_id: AtomicU64,
    request_timeout: Duration,
}

enum ClientCommand {
    Request(JsonRpcRequest),
    Close,
}

impl InteractionWsClient {
    pub async fn connect(
        config: InteractionWsClientConfig,
    ) -> InteractionClientResult<InteractionWsClient> {
        let mut request = config
            .url
            .clone()
            .into_client_request()
            .map_err(|error| InteractionClientError::Connect(error.to_string()))?;
        if let Some(token) = &config.bearer_token {
            request.headers_mut().insert(
                "authorization",
                format!("Bearer {token}").parse().map_err(header_error)?,
            );
        }

        let (websocket, _) = connect_async(request)
            .await
            .map_err(|error| InteractionClientError::Connect(error.to_string()))?;
        let (writer, reader) = websocket.split();
        let (sender, receiver) = mpsc::channel(config.outbound_buffer);
        let (notifications, _) = broadcast::channel(config.notification_buffer);
        let pending = Arc::new(Mutex::new(BTreeMap::new()));

        tokio::spawn(write_loop(writer, receiver, Arc::clone(&pending)));
        tokio::spawn(read_loop(
            reader,
            Arc::clone(&pending),
            notifications.clone(),
        ));

        Ok(Self {
            inner: Arc::new(InteractionWsClientInner {
                sender,
                pending,
                notifications,
                next_id: AtomicU64::new(1),
                request_timeout: config.request_timeout,
            }),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<InteractionWsNotification> {
        self.inner.notifications.subscribe()
    }

    pub async fn shutdown(&self) -> InteractionClientResult<()> {
        self.inner
            .sender
            .send(ClientCommand::Close)
            .await
            .map_err(|_| InteractionClientError::Closed)
    }

    pub async fn request<P>(
        &self,
        method: impl Into<String>,
        params: P,
    ) -> InteractionClientResult<Value>
    where
        P: Serialize,
    {
        let params = serde_json::to_value(params)
            .map_err(|error| InteractionClientError::Encode(error.to_string()))?;
        self.request_value(method, params).await
    }

    pub async fn request_as<P, R>(
        &self,
        method: impl Into<String>,
        params: P,
    ) -> InteractionClientResult<R>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let value = self.request(method, params).await?;
        serde_json::from_value(value)
            .map_err(|error| InteractionClientError::Decode(error.to_string()))
    }

    pub async fn request_value(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> InteractionClientResult<Value> {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .expect("interaction client pending request lock poisoned")
            .insert(id, sender);

        let request = JsonRpcRequest::new(id, method.into(), params);
        if self
            .inner
            .sender
            .send(ClientCommand::Request(request))
            .await
            .is_err()
        {
            self.remove_pending(id);
            return Err(InteractionClientError::Closed);
        }

        match tokio::time::timeout(self.inner.request_timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(InteractionClientError::Closed),
            Err(_) => {
                self.remove_pending(id);
                Err(InteractionClientError::Timeout(self.inner.request_timeout))
            }
        }
    }

    fn remove_pending(&self, id: u64) {
        self.inner
            .pending
            .lock()
            .expect("interaction client pending request lock poisoned")
            .remove(&id);
    }
}

async fn write_loop(
    mut writer: SplitSink<ClientWebSocket, Message>,
    mut receiver: mpsc::Receiver<ClientCommand>,
    pending: PendingRequests,
) {
    while let Some(command) = receiver.recv().await {
        match command {
            ClientCommand::Request(request) => {
                let payload = match serde_json::to_string(&request) {
                    Ok(payload) => payload,
                    Err(error) => {
                        fail_pending_request(
                            &pending,
                            request.id.as_u64(),
                            InteractionClientError::Encode(error.to_string()),
                        );
                        continue;
                    }
                };
                if let Err(error) = writer.send(Message::Text(payload.into())).await {
                    drain_pending(
                        &pending,
                        InteractionClientError::Transport(error.to_string()),
                    );
                    return;
                }
            }
            ClientCommand::Close => {
                let _ = writer.send(Message::Close(None)).await;
                drain_pending(&pending, InteractionClientError::Closed);
                return;
            }
        }
    }
    drain_pending(&pending, InteractionClientError::Closed);
}

async fn read_loop(
    mut reader: SplitStream<ClientWebSocket>,
    pending: PendingRequests,
    notifications: broadcast::Sender<InteractionWsNotification>,
) {
    while let Some(message) = reader.next().await {
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                drain_pending(
                    &pending,
                    InteractionClientError::Transport(error.to_string()),
                );
                return;
            }
        };
        match message {
            Message::Text(text) => {
                if let Err(error) = handle_text_message(text.as_str(), &pending, &notifications) {
                    drain_pending(&pending, error);
                    return;
                }
            }
            Message::Close(_) => {
                drain_pending(&pending, InteractionClientError::Closed);
                return;
            }
            Message::Binary(_) => {
                drain_pending(
                    &pending,
                    InteractionClientError::Protocol(
                        "binary websocket messages are not supported".into(),
                    ),
                );
                return;
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
    drain_pending(&pending, InteractionClientError::Closed);
}

fn handle_text_message(
    text: &str,
    pending: &PendingRequests,
    notifications: &broadcast::Sender<InteractionWsNotification>,
) -> InteractionClientResult<()> {
    let value = serde_json::from_str::<Value>(text)
        .map_err(|error| InteractionClientError::Decode(error.to_string()))?;
    if value.get("method").is_some() {
        let notification = serde_json::from_value::<JsonRpcNotification>(value)
            .map_err(|error| InteractionClientError::Decode(error.to_string()))?;
        let _ = notifications.send(InteractionWsNotification {
            method: notification.method,
            params: notification.params,
        });
        return Ok(());
    }

    let response = serde_json::from_value::<super::JsonRpcResponse>(value)
        .map_err(|error| InteractionClientError::Decode(error.to_string()))?;
    let response_id = response.id.as_u64().ok_or_else(|| {
        InteractionClientError::Protocol(format!(
            "response id is not an unsigned integer: {}",
            response.id
        ))
    })?;
    let Some(sender) = pending
        .lock()
        .expect("interaction client pending request lock poisoned")
        .remove(&response_id)
    else {
        return Ok(());
    };
    let result = match response.payload {
        JsonRpcResponsePayload::Result { result } => Ok(result),
        JsonRpcResponsePayload::Error { error } => Err(error_to_client_error(error)),
    };
    let _ = sender.send(result);
    Ok(())
}

fn fail_pending_request(pending: &PendingRequests, id: Option<u64>, error: InteractionClientError) {
    let Some(id) = id else {
        return;
    };
    if let Some(sender) = pending
        .lock()
        .expect("interaction client pending request lock poisoned")
        .remove(&id)
    {
        let _ = sender.send(Err(error));
    }
}

fn drain_pending(pending: &PendingRequests, error: InteractionClientError) {
    let requests = std::mem::take(
        &mut *pending
            .lock()
            .expect("interaction client pending request lock poisoned"),
    );
    for sender in requests.into_values() {
        let _ = sender.send(Err(error.clone()));
    }
}

fn error_to_client_error(error: JsonRpcErrorObject) -> InteractionClientError {
    InteractionClientError::JsonRpc {
        code: error.code,
        message: error.message,
        data: error.data,
    }
}

fn header_error(error: impl std::fmt::Display) -> InteractionClientError {
    InteractionClientError::Connect(format!("invalid authorization header: {error}"))
}

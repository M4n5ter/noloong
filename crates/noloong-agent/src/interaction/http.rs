use super::jsonrpc::{
    JsonRpcOutbound, dispatch_jsonrpc_request, jsonrpc_outbound_channel, parse_jsonrpc_request,
    request_response_notifier, send_close, send_response,
};
use super::protocol::method;
use super::{InteractionError, JsonRpcHandler, JsonRpcResponse};
use axum::{
    Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use std::fmt::{Debug, Formatter};
use tokio::{net::TcpListener, sync::mpsc};

const DEFAULT_MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionHttpTransportConfig {
    pub auth: InteractionTransportAuth,
    pub max_request_bytes: usize,
    pub outbound_buffer: usize,
}

impl InteractionHttpTransportConfig {
    pub fn bearer_token(token: impl Into<String>) -> Self {
        Self {
            auth: InteractionTransportAuth::BearerToken(token.into()),
            ..Self::default()
        }
    }

    pub fn without_auth(mut self) -> Self {
        self.auth = InteractionTransportAuth::None;
        self
    }

    pub fn max_request_bytes(mut self, max_request_bytes: usize) -> Self {
        self.max_request_bytes = max_request_bytes;
        self
    }

    pub fn outbound_buffer(mut self, outbound_buffer: usize) -> Self {
        self.outbound_buffer = outbound_buffer;
        self
    }
}

impl Default for InteractionHttpTransportConfig {
    fn default() -> Self {
        Self {
            auth: InteractionTransportAuth::None,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
            outbound_buffer: 1024,
        }
    }
}

impl Debug for InteractionHttpTransportConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionHttpTransportConfig")
            .field("auth", &self.auth)
            .field("max_request_bytes", &self.max_request_bytes)
            .field("outbound_buffer", &self.outbound_buffer)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum InteractionTransportAuth {
    None,
    BearerToken(String),
}

impl Debug for InteractionTransportAuth {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => formatter.write_str("None"),
            Self::BearerToken(_) => formatter.write_str("BearerToken(<redacted>)"),
        }
    }
}

pub fn interaction_http_router<H>(handler: H, config: InteractionHttpTransportConfig) -> Router
where
    H: JsonRpcHandler + Clone + Send + Sync + 'static,
{
    let max_request_bytes = config.max_request_bytes;
    Router::new()
        .route("/jsonrpc", post(http_jsonrpc::<H>))
        .route("/jsonrpc/ws", get(websocket_jsonrpc::<H>))
        .layer(DefaultBodyLimit::max(max_request_bytes))
        .with_state(InteractionHttpState { handler, config })
}

pub async fn serve_interaction_http<H>(
    listener: TcpListener,
    handler: H,
    config: InteractionHttpTransportConfig,
) -> Result<(), InteractionError>
where
    H: JsonRpcHandler + Clone + Send + Sync + 'static,
{
    axum::serve(listener, interaction_http_router(handler, config))
        .await
        .map_err(|error| {
            InteractionError::internal(format!("interaction http server failed: {error}"))
        })
}

#[derive(Clone)]
struct InteractionHttpState<H> {
    handler: H,
    config: InteractionHttpTransportConfig,
}

async fn http_jsonrpc<H>(
    State(state): State<InteractionHttpState<H>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response
where
    H: JsonRpcHandler + Clone + Send + Sync + 'static,
{
    if !is_authorized(&headers, &state.config.auth) {
        return unauthorized_response();
    }
    let request = match parse_jsonrpc_request(&body) {
        Ok(request) => request,
        Err(response) => return axum::Json(response).into_response(),
    };
    if requires_bidirectional_transport(&request.method) {
        let response = JsonRpcResponse::error(
            request.id,
            InteractionError::invalid_params(format!(
                "method {} requires bidirectional transport",
                request.method
            )),
        );
        return axum::Json(response).into_response();
    }

    let output =
        dispatch_jsonrpc_request(&state.handler, request, request_response_notifier()).await;
    axum::Json(output.response).into_response()
}

async fn websocket_jsonrpc<H>(
    State(state): State<InteractionHttpState<H>>,
    headers: HeaderMap,
    websocket: WebSocketUpgrade,
) -> Response
where
    H: JsonRpcHandler + Clone + Send + Sync + 'static,
{
    if !is_authorized(&headers, &state.config.auth) {
        return unauthorized_response();
    }
    websocket
        .max_message_size(state.config.max_request_bytes)
        .max_frame_size(state.config.max_request_bytes)
        .on_upgrade(move |socket| serve_jsonrpc_websocket(socket, state))
}

async fn serve_jsonrpc_websocket<H>(socket: WebSocket, state: InteractionHttpState<H>)
where
    H: JsonRpcHandler + Clone + Send + Sync + 'static,
{
    let handler = state.handler.connection_handler();
    let (sender, mut receiver) = socket.split();
    let (outbound_sender, outbound_receiver, notifier) =
        jsonrpc_outbound_channel(state.config.outbound_buffer);
    let writer = tokio::spawn(write_websocket_outbound(sender, outbound_receiver));

    while let Some(message) = receiver.next().await {
        let message = match message {
            Ok(message) => message,
            Err(_) => break,
        };
        match message {
            Message::Text(text) => {
                let request = match parse_jsonrpc_request(text.as_bytes()) {
                    Ok(request) => request,
                    Err(response) => {
                        if send_response(&outbound_sender, response).is_err() {
                            break;
                        }
                        continue;
                    }
                };
                let output = dispatch_jsonrpc_request(&handler, request, notifier.clone()).await;
                if send_response(&outbound_sender, output.response).is_err() {
                    break;
                }
                if output.shutdown {
                    let _ = send_close(&outbound_sender);
                    break;
                }
            }
            Message::Binary(_) => {
                let response = JsonRpcResponse::parse_error(InteractionError::invalid_params(
                    "binary websocket messages are not supported",
                ));
                if send_response(&outbound_sender, response).is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    let _ = send_close(&outbound_sender);
    let _ = writer.await;
}

async fn write_websocket_outbound(
    mut sender: SplitSink<WebSocket, Message>,
    mut receiver: mpsc::Receiver<JsonRpcOutbound>,
) {
    while let Some(outbound) = receiver.recv().await {
        match outbound {
            JsonRpcOutbound::Response(response) => {
                if send_json_websocket_message(&mut sender, &response)
                    .await
                    .is_err()
                {
                    return;
                }
            }
            JsonRpcOutbound::Notification(notification) => {
                if send_json_websocket_message(&mut sender, &notification)
                    .await
                    .is_err()
                {
                    return;
                }
            }
            JsonRpcOutbound::Close => {
                let _ = sender.send(Message::Close(None)).await;
                return;
            }
        }
    }
}

async fn send_json_websocket_message<T>(
    sender: &mut SplitSink<WebSocket, Message>,
    value: &T,
) -> Result<(), ()>
where
    T: serde::Serialize,
{
    let payload = serde_json::to_string(value).map_err(|_| ())?;
    sender
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ())
}

fn is_authorized(headers: &HeaderMap, auth: &InteractionTransportAuth) -> bool {
    match auth {
        InteractionTransportAuth::None => true,
        InteractionTransportAuth::BearerToken(token) => {
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                == Some(token.as_str())
        }
    }
}

fn unauthorized_response() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(WWW_AUTHENTICATE, "Bearer")],
        "unauthorized",
    )
        .into_response()
}

fn requires_bidirectional_transport(method: &str) -> bool {
    matches!(method, method::EVENT_SUBSCRIBE | method::DISPLAY_SUBSCRIBE)
}

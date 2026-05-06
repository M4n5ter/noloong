use super::{InteractionError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use serde::Serialize;
use serde_json::Value;
use std::{future::Future, pin::Pin};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, Lines};
use tokio::sync::mpsc;

const JSONRPC_OUTBOUND_BUFFER: usize = 1024;

pub type InteractionFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, InteractionError>> + Send + 'a>>;

pub trait JsonRpcHandler: Send + Sync {
    fn handle<'a>(
        &'a self,
        method: &'a str,
        params: Value,
        notifier: InteractionNotifier,
    ) -> InteractionFuture<'a, JsonRpcHandlerOutput>;
}

#[derive(Clone)]
pub struct InteractionNotifier {
    sink: InteractionNotifierSink,
}

#[derive(Clone)]
enum InteractionNotifierSink {
    Outbound(mpsc::Sender<JsonRpcOutbound>),
    #[cfg(feature = "interaction-http")]
    Discard,
}

impl InteractionNotifier {
    pub fn notify<T>(&self, method: impl Into<String>, params: &T) -> Result<(), InteractionError>
    where
        T: Serialize,
    {
        let params = serde_json::to_value(params).map_err(|error| {
            InteractionError::internal(format!("json-rpc notification encode failed: {error}"))
        })?;
        match &self.sink {
            InteractionNotifierSink::Outbound(sender) => sender
                .try_send(JsonRpcOutbound::Notification(JsonRpcNotification::new(
                    method, params,
                )))
                .map_err(|error| {
                    InteractionError::internal(format!(
                        "json-rpc notification writer is unavailable: {error}"
                    ))
                }),
            #[cfg(feature = "interaction-http")]
            InteractionNotifierSink::Discard => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonRpcHandlerOutput {
    pub result: Value,
    pub shutdown: bool,
}

impl JsonRpcHandlerOutput {
    pub fn result(result: Value) -> Self {
        Self {
            result,
            shutdown: false,
        }
    }

    pub fn shutdown(result: Value) -> Self {
        Self {
            result,
            shutdown: true,
        }
    }
}

pub async fn serve_jsonrpc<R, W, H>(
    reader: R,
    writer: W,
    handler: H,
) -> Result<(), InteractionError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
    H: JsonRpcHandler,
{
    let (outbound_sender, outbound_receiver, notifier) =
        jsonrpc_outbound_channel(JSONRPC_OUTBOUND_BUFFER);
    let writer_task = tokio::spawn(write_outbound_jsonrpc(writer, outbound_receiver));
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = next_line(&mut lines).await? {
        if line.trim().is_empty() {
            continue;
        }
        let request = match parse_jsonrpc_request(line.as_bytes()) {
            Ok(request) => request,
            Err(response) => {
                send_response(&outbound_sender, response)?;
                continue;
            }
        };
        let output = dispatch_jsonrpc_request(&handler, request, notifier.clone()).await;
        send_response(&outbound_sender, output.response)?;
        if output.shutdown {
            send_close(&outbound_sender)?;
            return await_writer(writer_task).await;
        }
    }
    send_close(&outbound_sender)?;
    await_writer(writer_task).await
}

#[derive(Debug)]
pub(crate) enum JsonRpcOutbound {
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    Close,
}

pub(crate) struct JsonRpcDispatchOutput {
    pub response: JsonRpcResponse,
    pub shutdown: bool,
}

pub(crate) fn jsonrpc_outbound_channel(
    buffer: usize,
) -> (
    mpsc::Sender<JsonRpcOutbound>,
    mpsc::Receiver<JsonRpcOutbound>,
    InteractionNotifier,
) {
    let (sender, receiver) = mpsc::channel(buffer);
    let notifier = InteractionNotifier {
        sink: InteractionNotifierSink::Outbound(sender.clone()),
    };
    (sender, receiver, notifier)
}

#[cfg(feature = "interaction-http")]
pub(crate) fn request_response_notifier() -> InteractionNotifier {
    InteractionNotifier {
        sink: InteractionNotifierSink::Discard,
    }
}

pub(crate) fn parse_jsonrpc_request(bytes: &[u8]) -> Result<JsonRpcRequest, JsonRpcResponse> {
    serde_json::from_slice::<JsonRpcRequest>(bytes).map_err(|error| {
        JsonRpcResponse::parse_error(InteractionError::invalid_params(format!(
            "invalid json-rpc request: {error}"
        )))
    })
}

pub(crate) async fn dispatch_jsonrpc_request<H>(
    handler: &H,
    request: JsonRpcRequest,
    notifier: InteractionNotifier,
) -> JsonRpcDispatchOutput
where
    H: JsonRpcHandler + ?Sized,
{
    let JsonRpcRequest {
        jsonrpc,
        id,
        method,
        params,
    } = request;
    if jsonrpc != "2.0" {
        return JsonRpcDispatchOutput {
            response: JsonRpcResponse::error(
                id,
                InteractionError::invalid_params(format!("unsupported jsonrpc version: {jsonrpc}")),
            ),
            shutdown: false,
        };
    }

    match handler.handle(&method, params, notifier).await {
        Ok(output) => JsonRpcDispatchOutput {
            response: JsonRpcResponse::result(id, output.result),
            shutdown: output.shutdown,
        },
        Err(error) => JsonRpcDispatchOutput {
            response: JsonRpcResponse::error(id, error),
            shutdown: false,
        },
    }
}

async fn write_outbound_jsonrpc<W>(
    mut writer: W,
    mut receiver: mpsc::Receiver<JsonRpcOutbound>,
) -> Result<(), InteractionError>
where
    W: AsyncWrite + Unpin,
{
    while let Some(outbound) = receiver.recv().await {
        match outbound {
            JsonRpcOutbound::Response(response) => write_json_line(&mut writer, &response).await?,
            JsonRpcOutbound::Notification(notification) => {
                write_json_line(&mut writer, &notification).await?
            }
            JsonRpcOutbound::Close => return Ok(()),
        }
    }
    Ok(())
}

pub(crate) fn send_response(
    sender: &mpsc::Sender<JsonRpcOutbound>,
    response: JsonRpcResponse,
) -> Result<(), InteractionError> {
    sender
        .try_send(JsonRpcOutbound::Response(response))
        .map_err(|error| {
            InteractionError::internal(format!("json-rpc response writer is unavailable: {error}"))
        })
}

pub(crate) fn send_close(sender: &mpsc::Sender<JsonRpcOutbound>) -> Result<(), InteractionError> {
    sender.try_send(JsonRpcOutbound::Close).map_err(|error| {
        InteractionError::internal(format!("json-rpc response writer is unavailable: {error}"))
    })
}

async fn await_writer(
    writer_task: tokio::task::JoinHandle<Result<(), InteractionError>>,
) -> Result<(), InteractionError> {
    writer_task.await.map_err(|error| {
        InteractionError::internal(format!("json-rpc writer task failed: {error}"))
    })?
}

async fn next_line<R>(lines: &mut Lines<BufReader<R>>) -> Result<Option<String>, InteractionError>
where
    R: AsyncRead + Unpin,
{
    lines
        .next_line()
        .await
        .map_err(|error| InteractionError::internal(format!("json-rpc read failed: {error}")))
}

pub async fn write_json_line<W, T>(writer: &mut W, value: &T) -> Result<(), InteractionError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let payload = serde_json::to_vec(value)
        .map_err(|error| InteractionError::internal(format!("json-rpc encode failed: {error}")))?;
    writer
        .write_all(&payload)
        .await
        .map_err(|error| InteractionError::internal(format!("json-rpc write failed: {error}")))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|error| InteractionError::internal(format!("json-rpc write failed: {error}")))?;
    writer
        .flush()
        .await
        .map_err(|error| InteractionError::internal(format!("json-rpc flush failed: {error}")))
}

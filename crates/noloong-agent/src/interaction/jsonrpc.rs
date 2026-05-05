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
    sender: mpsc::Sender<JsonRpcOutbound>,
}

impl InteractionNotifier {
    pub fn notify<T>(&self, method: impl Into<String>, params: &T) -> Result<(), InteractionError>
    where
        T: Serialize,
    {
        let params = serde_json::to_value(params).map_err(|error| {
            InteractionError::internal(format!("json-rpc notification encode failed: {error}"))
        })?;
        self.sender
            .try_send(JsonRpcOutbound::Notification(JsonRpcNotification::new(
                method, params,
            )))
            .map_err(|error| {
                InteractionError::internal(format!(
                    "json-rpc notification writer is unavailable: {error}"
                ))
            })
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
    let (outbound_sender, outbound_receiver) = mpsc::channel(JSONRPC_OUTBOUND_BUFFER);
    let writer_task = tokio::spawn(write_outbound_jsonrpc(writer, outbound_receiver));
    let notifier = InteractionNotifier {
        sender: outbound_sender.clone(),
    };
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = next_line(&mut lines).await? {
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => request,
            Err(error) => {
                send_response(
                    &outbound_sender,
                    JsonRpcResponse::parse_error(InteractionError::invalid_params(format!(
                        "invalid json-rpc request: {error}"
                    ))),
                )?;
                continue;
            }
        };
        if request.jsonrpc != "2.0" {
            send_response(
                &outbound_sender,
                JsonRpcResponse::error(
                    request.id,
                    InteractionError::invalid_params(format!(
                        "unsupported jsonrpc version: {}",
                        request.jsonrpc
                    )),
                ),
            )?;
            continue;
        }

        let id = request.id;
        let output = handler
            .handle(&request.method, request.params, notifier.clone())
            .await;
        match output {
            Ok(output) => {
                send_response(&outbound_sender, JsonRpcResponse::result(id, output.result))?;
                if output.shutdown {
                    send_close(&outbound_sender)?;
                    return await_writer(writer_task).await;
                }
            }
            Err(error) => {
                send_response(&outbound_sender, JsonRpcResponse::error(id, error))?;
            }
        }
    }
    send_close(&outbound_sender)?;
    await_writer(writer_task).await
}

#[derive(Debug)]
enum JsonRpcOutbound {
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    Close,
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

fn send_response(
    sender: &mpsc::Sender<JsonRpcOutbound>,
    response: JsonRpcResponse,
) -> Result<(), InteractionError> {
    sender
        .try_send(JsonRpcOutbound::Response(response))
        .map_err(|error| {
            InteractionError::internal(format!("json-rpc response writer is unavailable: {error}"))
        })
}

fn send_close(sender: &mpsc::Sender<JsonRpcOutbound>) -> Result<(), InteractionError> {
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

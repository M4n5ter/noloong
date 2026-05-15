use super::{ModelStreamRegistrations, PendingRequests, StdioFatalError};
use crate::{AgentCoreError, ModelStreamEvent};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};

pub(super) async fn read_stdout(
    stdout: tokio::process::ChildStdout,
    pending: PendingRequests,
    fatal_error: StdioFatalError,
    model_stream_sinks: ModelStreamRegistrations,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(value) => handle_message(value, &pending, &model_stream_sinks).await,
            Err(error) => {
                let error = format!("invalid json from extension: {error}");
                fail_pending(&pending, &fatal_error, error).await;
            }
        }
    }

    fail_pending(&pending, &fatal_error, "extension stdout closed".into()).await;
}

async fn fail_pending(pending: &PendingRequests, fatal_error: &StdioFatalError, error: String) {
    let mut fatal = fatal_error.lock().await;
    if fatal.is_none() {
        *fatal = Some(error.clone());
    }
    drop(fatal);

    let mut pending_guard = pending.lock().await;
    let pending = std::mem::take(&mut *pending_guard);
    for (_, sender) in pending {
        let _ = sender.send(Err(AgentCoreError::JsonRpc(error.clone())));
    }
}

async fn handle_message(
    value: Value,
    pending: &PendingRequests,
    model_stream_sinks: &ModelStreamRegistrations,
) {
    if let Some(id) = value.get("id").and_then(Value::as_u64) {
        let sender = pending.lock().await.remove(&id);
        if let Some(sender) = sender {
            let result = if let Some(error) = value.get("error") {
                Err(AgentCoreError::JsonRpc(error.to_string()))
            } else {
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ = sender.send(result);
        }
        return;
    }

    if value.get("method").and_then(Value::as_str) != Some("stream/event") {
        return;
    }
    let Some(params) = value.get("params") else {
        return;
    };
    let Some(stream_id) = params.get("streamId").and_then(Value::as_str) else {
        return;
    };
    let Some(event) = params.get("event") else {
        return;
    };
    if let Some(registration) = model_stream_sinks.lock().await.get(stream_id).cloned() {
        match serde_json::from_value::<ModelStreamEvent>(event.clone()) {
            Ok(event) => {
                let result = (registration.sink)(event.clone()).await.map(|()| event);
                let _ = registration.events.send(result).await;
            }
            Err(error) => {
                let _ = registration
                    .events
                    .send(Err(AgentCoreError::JsonRpc(format!(
                        "invalid stream event for {stream_id}: {error}"
                    ))))
                    .await;
            }
        }
    }
}

use crate::{
    Catalog, HostProcessManager, MessageKey, ProcessError, ProcessOutput, ProductToolName,
    ReadOutputRequest, StartCommandRequest,
};
use noloong_agent_core::{
    BoxFuture, CancellationToken, ContentBlock, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{json_tool_output, sequential_tool_spec};

#[derive(Clone)]
pub struct HostExecStartTool {
    manager: HostProcessManager,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct HostExecReadTool {
    manager: HostProcessManager,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct HostExecWaitTool {
    manager: HostProcessManager,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct HostExecWriteTool {
    manager: HostProcessManager,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct HostExecTerminateTool {
    manager: HostProcessManager,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct HostExecListTool {
    manager: HostProcessManager,
    catalog: Catalog,
}

impl HostExecStartTool {
    pub fn new(manager: HostProcessManager, catalog: Catalog) -> Self {
        Self { manager, catalog }
    }
}

impl HostExecReadTool {
    pub fn new(manager: HostProcessManager, catalog: Catalog) -> Self {
        Self { manager, catalog }
    }
}

impl HostExecWaitTool {
    pub fn new(manager: HostProcessManager, catalog: Catalog) -> Self {
        Self { manager, catalog }
    }
}

impl HostExecWriteTool {
    pub fn new(manager: HostProcessManager, catalog: Catalog) -> Self {
        Self { manager, catalog }
    }
}

impl HostExecTerminateTool {
    pub fn new(manager: HostProcessManager, catalog: Catalog) -> Self {
        Self { manager, catalog }
    }
}

impl HostExecListTool {
    pub fn new(manager: HostProcessManager, catalog: Catalog) -> Self {
        Self { manager, catalog }
    }
}

impl ToolProvider for HostExecStartTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            ProductToolName::HostExecStart.as_str(),
            self.catalog.message(MessageKey::HostExecStartDescription),
            json!({
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": {"type": "string"},
                    "shell": {"type": "string"},
                    "cwd": {"type": "string"},
                    "env": {"type": "object"},
                    "pipeStdin": {"type": "boolean"},
                    "foregroundWaitMs": {"type": "integer", "minimum": 0},
                    "maxSpoolBytes": {"type": "integer", "minimum": 1024}
                }
            }),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input = serde_json::from_value::<StartCommandInput>(request.arguments).map_err(
                |error| noloong_agent_core::AgentCoreError::InvalidEffect(error.to_string()),
            )?;
            let snapshot = self
                .manager
                .start(input.into_request())
                .await
                .map_err(process_error)?;
            Ok(output_json(json!({
                "jobId": snapshot.job_id,
                "command": snapshot.command,
                "shell": snapshot.shell,
                "cwd": snapshot.cwd,
                "status": snapshot.status,
                "startedAtMs": snapshot.started_at_ms,
                "endedAtMs": snapshot.ended_at_ms,
                "nextCursor": snapshot.next_cursor,
                "droppedBeforeSeq": snapshot.dropped_before_seq,
            })))
        })
    }
}

impl ToolProvider for HostExecReadTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            ProductToolName::HostExecRead.as_str(),
            self.catalog.message(MessageKey::HostExecReadDescription),
            json!({
                "type": "object",
                "required": ["jobId"],
                "properties": {
                    "jobId": {"type": "string"},
                    "afterSeq": {"type": "integer", "minimum": 0},
                    "maxBytes": {"type": "integer", "minimum": 1},
                    "waitMs": {"type": "integer", "minimum": 0}
                }
            }),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input =
                serde_json::from_value::<ReadInput>(request.arguments).map_err(|error| {
                    noloong_agent_core::AgentCoreError::InvalidEffect(error.to_string())
                })?;
            let job_id = input.job_id.clone();
            let output = self
                .manager
                .read(&job_id, input.into_request())
                .await
                .map_err(process_error)?;
            Ok(process_output(output))
        })
    }
}

impl ToolProvider for HostExecWaitTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            ProductToolName::HostExecWait.as_str(),
            self.catalog.message(MessageKey::HostExecWaitDescription),
            json!({
                "type": "object",
                "required": ["jobId"],
                "properties": {
                    "jobId": {"type": "string"},
                    "timeoutMs": {"type": "integer", "minimum": 0}
                }
            }),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input =
                serde_json::from_value::<WaitInput>(request.arguments).map_err(|error| {
                    noloong_agent_core::AgentCoreError::InvalidEffect(error.to_string())
                })?;
            let outcome = self
                .manager
                .wait(&input.job_id, input.timeout_ms)
                .await
                .map_err(process_error)?;
            Ok(output_json(json!(outcome)))
        })
    }
}

impl ToolProvider for HostExecWriteTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            ProductToolName::HostExecWrite.as_str(),
            self.catalog.message(MessageKey::HostExecWriteDescription),
            json!({
                "type": "object",
                "required": ["jobId", "text"],
                "properties": {
                    "jobId": {"type": "string"},
                    "text": {"type": "string"}
                }
            }),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input =
                serde_json::from_value::<WriteInput>(request.arguments).map_err(|error| {
                    noloong_agent_core::AgentCoreError::InvalidEffect(error.to_string())
                })?;
            let snapshot = self
                .manager
                .write(&input.job_id, &input.text)
                .await
                .map_err(process_error)?;
            Ok(output_json(json!(snapshot)))
        })
    }
}

impl ToolProvider for HostExecTerminateTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            ProductToolName::HostExecTerminate.as_str(),
            self.catalog
                .message(MessageKey::HostExecTerminateDescription),
            json!({
                "type": "object",
                "required": ["jobId"],
                "properties": {"jobId": {"type": "string"}}
            }),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input = serde_json::from_value::<JobInput>(request.arguments).map_err(|error| {
                noloong_agent_core::AgentCoreError::InvalidEffect(error.to_string())
            })?;
            let snapshot = self
                .manager
                .terminate(&input.job_id)
                .await
                .map_err(process_error)?;
            Ok(output_json(json!(snapshot)))
        })
    }
}

impl ToolProvider for HostExecListTool {
    fn spec(&self) -> ToolSpec {
        tool_spec(
            ProductToolName::HostExecList.as_str(),
            self.catalog.message(MessageKey::HostExecListDescription),
            json!({"type": "object", "properties": {}}),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        _request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let jobs = self.manager.list().await.map_err(process_error)?;
            Ok(output_json(json!({ "jobs": jobs })))
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartCommandInput {
    command: String,
    shell: Option<String>,
    cwd: Option<std::path::PathBuf>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, Option<String>>,
    #[serde(default)]
    pipe_stdin: bool,
    max_spool_bytes: Option<usize>,
    foreground_wait_ms: Option<u64>,
}

impl StartCommandInput {
    fn into_request(self) -> StartCommandRequest {
        StartCommandRequest {
            command: self.command,
            shell: self.shell,
            cwd: self.cwd,
            env: self.env,
            pipe_stdin: self.pipe_stdin,
            max_spool_bytes: self.max_spool_bytes,
            foreground_wait_ms: self.foreground_wait_ms,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadInput {
    job_id: String,
    after_seq: Option<u64>,
    max_bytes: Option<usize>,
    wait_ms: Option<u64>,
}

impl ReadInput {
    fn into_request(self) -> ReadOutputRequest {
        ReadOutputRequest {
            after_seq: self.after_seq,
            max_bytes: self.max_bytes,
            wait_ms: self.wait_ms,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaitInput {
    job_id: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteInput {
    job_id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobInput {
    job_id: String,
}

fn tool_spec(name: &str, description: &str, input_schema: Value) -> ToolSpec {
    sequential_tool_spec(
        name,
        description,
        input_schema,
        "host.command",
        "Execute or control host commands",
    )
}

fn output_json(value: Value) -> ToolOutput {
    json_tool_output(value)
}

fn process_output(output: ProcessOutput) -> ToolOutput {
    let ProcessOutput {
        job_id,
        chunks,
        next_cursor,
        dropped_before_seq,
        truncated,
        status,
    } = output;
    ToolOutput {
        content: vec![ContentBlock::Json {
            value: json!({
                "jobId": job_id,
                "chunks": chunks,
            }),
        }],
        details: json!({
            "jobId": job_id,
            "nextCursor": next_cursor,
            "droppedBeforeSeq": dropped_before_seq,
            "truncated": truncated,
            "status": status,
        }),
        is_error: false,
        updates: Vec::new(),
    }
}

fn process_error(error: ProcessError) -> noloong_agent_core::AgentCoreError {
    noloong_agent_core::AgentCoreError::Provider(error.to_string())
}

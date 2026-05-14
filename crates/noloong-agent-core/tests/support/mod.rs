pub mod core;
use noloong_agent_core::{
    AgentEventKind, AgentMessage, AgentState, BoxFuture, CancellationToken, ContentBlock,
    HttpAuthContext, HttpAuthHeader, HttpAuthHeaders, HttpAuthProvider, HttpAuthRefreshContext,
    HttpAuthRefreshResult, MessageRole, ModelStreamEvent, Result, RunReport, SseReconnectConfig,
    StdioExtensionConfig, ThinkingBlock, ToolExecutionMode, ToolOutput, ToolProvider, ToolRequest,
    ToolSpec,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::{Duration, sleep},
};

pub fn skip_when_env_missing(name: &str) -> bool {
    if std::env::var(name).is_ok() {
        return false;
    }
    init_test_logger();
    log::info!("skipping live test because {name} is not set");
    true
}

pub fn init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();
}

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is inside crates/noloong-agent-core")
        .to_path_buf()
}

pub fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "noloong-agent-core-{name}-{}-{nanos}",
        std::process::id()
    ))
}

pub fn fast_one_retry_reconnect() -> SseReconnectConfig {
    SseReconnectConfig {
        max_reconnects: 1,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
    }
}

pub fn jsonrpc_conformance_config(modes: &[&str]) -> StdioExtensionConfig {
    jsonrpc_conformance_config_with_timeouts(modes, Duration::from_secs(2), Duration::from_secs(2))
}

pub fn jsonrpc_conformance_config_with_timeouts(
    modes: &[&str],
    request_timeout: Duration,
    stream_timeout: Duration,
) -> StdioExtensionConfig {
    let mut config = StdioExtensionConfig::new("node")
        .arg(fixture_path("jsonrpc-conformance-extension.mjs").to_string_lossy())
        .request_timeout(request_timeout)
        .stream_timeout(stream_timeout);
    let mode = modes
        .iter()
        .copied()
        .filter(|mode| !mode.is_empty())
        .collect::<Vec<_>>()
        .join(",");
    if !mode.is_empty() {
        config = config.arg(format!("--mode={mode}"));
    }
    config
}

pub fn compaction_trigger_state() -> AgentState {
    AgentState {
        messages: vec![
            AgentMessage::user("u1", "old ".repeat(80)),
            AgentMessage::assistant(
                "a1",
                vec![ContentBlock::Text {
                    text: "old answer ".repeat(80),
                }],
            ),
            AgentMessage::user("u2", "recent"),
        ],
        ..AgentState::default()
    }
}

pub fn assert_exact_assistant_text(report: &RunReport, sentinel: &str) {
    let visible_text = assistant_visible_text(report);
    assert_eq!(
        visible_text.trim(),
        sentinel,
        "assistant visible text did not match sentinel; visible text: {visible_text}"
    );
}

pub fn assert_assistant_text_contains(report: &RunReport, expected: &str) {
    let visible_text = assistant_visible_text(report);
    assert_text_contains(&visible_text, expected);
}

pub fn assert_assistant_messages_contain(messages: &[AgentMessage], expected: &str) {
    let visible_text = assistant_visible_text_from_messages(messages);
    assert_text_contains(&visible_text, expected);
}

fn assert_text_contains(visible_text: &str, expected: &str) {
    assert!(
        visible_text.contains(expected),
        "assistant visible text did not include expected sentinel `{expected}`; visible text: {visible_text}"
    );
}

pub fn assistant_visible_text(report: &RunReport) -> String {
    assistant_visible_text_from_messages(&report.state.messages)
}

pub fn assistant_visible_text_from_messages(messages: &[AgentMessage]) -> String {
    messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::Assistant))
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub fn has_exact_tool_execution(report: &RunReport, expected_value: &str) -> bool {
    has_tool_execution(report, |text| text == expected_value)
}

pub fn has_visible_thinking(report: &RunReport, provider_id: &str) -> bool {
    has_thinking_event(report, provider_id, false) || has_thinking_block(report, true, false)
}

pub fn has_visible_or_raw_thinking_event_and_block(report: &RunReport, provider_id: &str) -> bool {
    has_thinking_event(report, provider_id, true) && has_thinking_block(report, false, true)
}

pub fn has_thinking_event(report: &RunReport, provider_id: &str, allow_raw: bool) -> bool {
    report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ModelStreamEvent {
                provider,
                event: ModelStreamEvent::ThinkingDelta { delta }
            } if provider == provider_id
                && (
                    delta.text_delta.as_deref().is_some_and(|text| !text.trim().is_empty())
                    || (allow_raw && delta.raw_snapshot.is_some())
                )
        )
    })
}

pub fn has_thinking_block(report: &RunReport, assistant_only: bool, allow_raw: bool) -> bool {
    report.state.messages.iter().any(|message| {
        (!assistant_only || matches!(message.role, MessageRole::Assistant))
            && message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Thinking { thinking } if thinking_has_signal(thinking, allow_raw)
                )
            })
    })
}

fn has_tool_execution(report: &RunReport, matches_text: impl Fn(&str) -> bool) -> bool {
    report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolExecutionCompleted { output, .. }
                if !output.is_error
                    && output.content.iter().any(|block| {
                        matches!(block, ContentBlock::Text { text } if matches_text(text))
                    })
        )
    })
}

fn thinking_has_signal(thinking: &ThinkingBlock, allow_raw: bool) -> bool {
    thinking
        .text
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty())
        || (allow_raw && thinking.raw.is_some())
}

#[derive(Clone, Debug)]
pub struct MockResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: &'static str,
    pub close_delimited: bool,
    pub hang_after_response: bool,
}

impl MockResponse {
    pub fn new(status: u16, content_type: &'static str, body: &'static str) -> Self {
        Self {
            status,
            content_type,
            body,
            close_delimited: false,
            hang_after_response: false,
        }
    }

    pub fn close_delimited(status: u16, content_type: &'static str, body: &'static str) -> Self {
        Self {
            status,
            content_type,
            body,
            close_delimited: true,
            hang_after_response: false,
        }
    }

    pub fn hang_after_headers(status: u16, content_type: &'static str) -> Self {
        Self {
            status,
            content_type,
            body: "",
            close_delimited: true,
            hang_after_response: true,
        }
    }

    pub fn hang_after_body(status: u16, content_type: &'static str, body: &'static str) -> Self {
        Self {
            status,
            content_type,
            body,
            close_delimited: true,
            hang_after_response: true,
        }
    }
}

#[derive(Debug)]
pub struct CapturedRequest {
    pub raw: String,
    pub headers: BTreeMap<String, String>,
    pub json: Value,
}

impl CapturedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

pub struct MockServer {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl MockServer {
    pub async fn spawn(
        status: u16,
        content_type: &'static str,
        body: &'static str,
    ) -> Result<Self> {
        Self::spawn_many(vec![MockResponse::new(status, content_type, body)]).await
    }

    pub async fn spawn_many(responses: Vec<MockResponse>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?.to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let request_slot = Arc::clone(&requests);
        tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.expect("mock server accept failed");
                let request = read_http_request(&mut socket)
                    .await
                    .expect("mock server read failed");
                request_slot
                    .lock()
                    .expect("request lock poisoned")
                    .push(request);
                let body = response.body;
                let hang_after_response = response.hang_after_response;
                let response_text = if response.close_delimited {
                    format!(
                        "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nConnection: close\r\n\r\n{body}",
                        response.status, response.content_type
                    )
                } else {
                    format!(
                        "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        response.status,
                        response.content_type,
                        body.len()
                    )
                };
                socket
                    .write_all(response_text.as_bytes())
                    .await
                    .expect("mock server write failed");
                if hang_after_response {
                    tokio::spawn(async move {
                        sleep(Duration::from_secs(5)).await;
                        drop(socket);
                    });
                }
            }
        });
        Ok(Self { address, requests })
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn request_json(&self) -> Value {
        self.request().json
    }

    pub fn request(&self) -> CapturedRequest {
        let request = self
            .requests
            .lock()
            .expect("request lock poisoned")
            .first()
            .cloned()
            .expect("request was not received");
        parse_request(request)
    }

    pub fn requests_json(&self) -> Vec<Value> {
        self.requests()
            .into_iter()
            .map(|request| request.json)
            .collect()
    }

    pub fn requests(&self) -> Vec<CapturedRequest> {
        self.requests
            .lock()
            .expect("request lock poisoned")
            .iter()
            .cloned()
            .map(parse_request)
            .collect()
    }

    pub fn request_count(&self) -> usize {
        self.requests.lock().expect("request lock poisoned").len()
    }
}

pub struct HangingServer {
    address: String,
}

impl HangingServer {
    pub async fn spawn() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?.to_string();
        tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.expect("mock server accept failed");
            sleep(Duration::from_secs(5)).await;
        });
        Ok(Self { address })
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

fn parse_request(raw: String) -> CapturedRequest {
    let (head, body) = raw.split_once("\r\n\r\n").expect("request body separator");
    CapturedRequest {
        headers: parse_headers(head),
        json: serde_json::from_str(body).expect("request body is valid JSON"),
        raw,
    }
}

fn parse_headers(head: &str) -> BTreeMap<String, String> {
    head.lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

async fn read_http_request(socket: &mut tokio::net::TcpStream) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            return Ok(String::from_utf8_lossy(&buffer).to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
    };
    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    let total_len = header_end + 4 + content_length;
    while buffer.len() < total_len {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

pub const DOTTED_TEST_TOOL_NAME: &str = "host.exec.start";

pub fn dotted_tool_spec(execution_mode: Option<ToolExecutionMode>) -> ToolSpec {
    value_echo_tool_spec(
        DOTTED_TEST_TOOL_NAME,
        "Starts a command in a dotted-name test tool.",
        execution_mode,
    )
}

pub fn is_provider_safe_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub struct LiveEchoTool;

impl ToolProvider for LiveEchoTool {
    fn spec(&self) -> ToolSpec {
        value_echo_tool_spec(
            "live_echo",
            "Echoes a value for live model tool-call conformance tests.",
            None,
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        value_echo_tool_output(request)
    }
}

pub struct ValueEchoTool {
    name: String,
    description: String,
}

impl ValueEchoTool {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

impl ToolProvider for ValueEchoTool {
    fn spec(&self) -> ToolSpec {
        value_echo_tool_spec(&self.name, &self.description, None)
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        value_echo_tool_output(request)
    }
}

fn value_echo_tool_spec(
    name: &str,
    description: &str,
    execution_mode: Option<ToolExecutionMode>,
) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: description.into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "value": {
                    "type": "string"
                }
            },
            "required": ["value"],
            "additionalProperties": false
        }),
        execution_mode,
        permissions: Vec::new(),
    }
}

fn value_echo_tool_output(request: ToolRequest) -> BoxFuture<'static, ToolOutput> {
    Box::pin(async move {
        Ok(ToolOutput {
            content: vec![ContentBlock::Text {
                text: request
                    .arguments
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }],
            details: request.arguments,
            is_error: false,
            updates: Vec::new(),
        })
    })
}

#[derive(Clone)]
pub struct TestAuthProvider {
    id: String,
    headers: Vec<HttpAuthHeader>,
    refresh_result: HttpAuthRefreshResult,
    header_contexts: Arc<Mutex<Vec<HttpAuthContext>>>,
    refresh_contexts: Arc<Mutex<Vec<HttpAuthRefreshContext>>>,
}

impl TestAuthProvider {
    pub fn new(id: impl Into<String>, headers: Vec<HttpAuthHeader>) -> Self {
        Self {
            id: id.into(),
            headers,
            refresh_result: HttpAuthRefreshResult::deny(),
            header_contexts: Arc::new(Mutex::new(Vec::new())),
            refresh_contexts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn refresh_result(mut self, refresh_result: HttpAuthRefreshResult) -> Self {
        self.refresh_result = refresh_result;
        self
    }

    pub fn header_contexts(&self) -> Vec<HttpAuthContext> {
        self.header_contexts
            .lock()
            .expect("header contexts lock poisoned")
            .clone()
    }

    pub fn refresh_contexts(&self) -> Vec<HttpAuthRefreshContext> {
        self.refresh_contexts
            .lock()
            .expect("refresh contexts lock poisoned")
            .clone()
    }
}

impl HttpAuthProvider for TestAuthProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn headers<'a>(
        &'a self,
        context: HttpAuthContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthHeaders> {
        let headers = self.headers.clone();
        self.header_contexts
            .lock()
            .expect("header contexts lock poisoned")
            .push(context);
        Box::pin(async move { Ok(HttpAuthHeaders::new(headers)) })
    }

    fn refresh<'a>(
        &'a self,
        context: HttpAuthRefreshContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthRefreshResult> {
        let refresh_result = self.refresh_result.clone();
        self.refresh_contexts
            .lock()
            .expect("refresh contexts lock poisoned")
            .push(context);
        Box::pin(async move { Ok(refresh_result) })
    }
}

pub const RED_DOT_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

pub fn silent_wav_base64() -> String {
    let sample_rate = 8_000_u32;
    let samples = sample_rate / 5;
    let data_size = samples * 2;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.extend(std::iter::repeat_n(0_u8, data_size as usize));
    base64_encode(&bytes)
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(third & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

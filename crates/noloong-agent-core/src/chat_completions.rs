use crate::{
    AgentCoreError, AgentMessage, CancellationToken, ContentBlock, MessageRole, ModelProvider,
    ModelRequest, ModelStreamEvent, ModelStreamSink, Result, StopReason, ThinkingBlock,
    ThinkingDelta, ThinkingKind, ToolCall, ToolSpec,
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    env,
    fmt::{Debug, Formatter},
    time::Duration,
};

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";
const OPENAI_CHAT_REASONING_REPLAY_KIND: &str = "openai_chat_reasoning_replay";

#[derive(Clone)]
pub struct ChatCompletionsProviderConfig {
    pub id: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub extra_body: Map<String, Value>,
    pub max_completion_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub include_usage: bool,
}

impl Debug for ChatCompletionsProviderConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatCompletionsProviderConfig")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("api_key_env", &self.api_key_env)
            .field("headers", &self.headers)
            .field("extra_body", &self.extra_body)
            .field("max_completion_tokens", &self.max_completion_tokens)
            .field("temperature", &self.temperature)
            .field("request_timeout", &self.request_timeout)
            .field("stream_idle_timeout", &self.stream_idle_timeout)
            .field("include_usage", &self.include_usage)
            .finish()
    }
}

impl ChatCompletionsProviderConfig {
    pub fn new(id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            base_url: DEFAULT_OPENAI_BASE_URL.into(),
            model: model.into(),
            api_key: None,
            api_key_env: Some(DEFAULT_OPENAI_API_KEY_ENV.into()),
            headers: BTreeMap::new(),
            extra_body: Map::new(),
            max_completion_tokens: None,
            temperature: None,
            request_timeout: Duration::from_secs(60),
            stream_idle_timeout: Duration::from_secs(300),
            include_usage: true,
        }
    }

    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn api_key_env(mut self, api_key_env: impl Into<String>) -> Self {
        self.api_key_env = Some(api_key_env.into());
        self
    }

    pub fn without_api_key(mut self) -> Self {
        self.api_key = None;
        self.api_key_env = None;
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn extra_body(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra_body.insert(key.into(), value);
        self
    }

    pub fn max_completion_tokens(mut self, max_completion_tokens: u64) -> Self {
        self.max_completion_tokens = Some(max_completion_tokens);
        self
    }

    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }

    pub fn stream_idle_timeout(mut self, stream_idle_timeout: Duration) -> Self {
        self.stream_idle_timeout = stream_idle_timeout;
        self
    }

    pub fn include_usage(mut self, include_usage: bool) -> Self {
        self.include_usage = include_usage;
        self
    }
}

pub struct ChatCompletionsProvider {
    config: ChatCompletionsProviderConfig,
    client: reqwest::Client,
}

impl ChatCompletionsProvider {
    pub fn new(config: ChatCompletionsProviderConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(config.request_timeout)
            .build()?;
        Ok(Self { config, client })
    }

    pub fn config(&self) -> &ChatCompletionsProviderConfig {
        &self.config
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }

    fn api_key(&self) -> Result<Option<String>> {
        if let Some(api_key) = &self.config.api_key {
            return Ok(Some(api_key.clone()));
        }
        let Some(api_key_env) = &self.config.api_key_env else {
            return Ok(None);
        };
        env::var(api_key_env).map(Some).map_err(|_| {
            AgentCoreError::Provider(format!(
                "missing API key environment variable: {api_key_env}"
            ))
        })
    }
}

impl ModelProvider for ChatCompletionsProvider {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let payload = build_chat_payload(&self.config, &request)?;
            let stream_id = format!("chat-completions-{}-{}", request.run_id, request.turn_id);
            let mut request_builder = self
                .client
                .post(self.endpoint())
                .headers(headers_from_config(&self.config)?)
                .json(&payload);
            if let Some(api_key) = self.api_key()? {
                request_builder = request_builder.bearer_auth(api_key);
            }

            let request_timeout = tokio::time::sleep(self.config.request_timeout);
            tokio::pin!(request_timeout);
            let response = tokio::select! {
                response = request_builder.send() => response?,
                _ = cancellation.cancelled() => return Err(AgentCoreError::Aborted),
                _ = &mut request_timeout => {
                    return Err(AgentCoreError::Provider("chat completions request timed out".into()));
                }
            };
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(AgentCoreError::Provider(format!(
                    "chat completions request failed with status {status}: {}",
                    body.chars().take(2048).collect::<String>()
                )));
            }

            let mut events = Vec::new();
            emit_event(
                &stream,
                &mut events,
                ModelStreamEvent::Started { stream_id },
            )
            .await?;

            let mut decoder = SseDecoder::default();
            let mut state = ChatStreamState::new(&self.config);
            let mut response = response;
            loop {
                let chunk = tokio::select! {
                    chunk = response.chunk() => chunk?,
                    _ = cancellation.cancelled() => return Err(AgentCoreError::Aborted),
                    _ = tokio::time::sleep(self.config.stream_idle_timeout) => {
                        return Err(AgentCoreError::Provider("chat completions stream timed out".into()));
                    }
                };
                let Some(chunk) = chunk else {
                    break;
                };
                for data in decoder.push(&String::from_utf8_lossy(&chunk)) {
                    if data == "[DONE]" {
                        state.done = true;
                        break;
                    }
                    for event in state.apply_chunk(&data)? {
                        emit_event(&stream, &mut events, event).await?;
                    }
                }
                if state.done {
                    break;
                }
            }

            for data in decoder.finish() {
                if data != "[DONE]" {
                    for event in state.apply_chunk(&data)? {
                        emit_event(&stream, &mut events, event).await?;
                    }
                }
            }
            for event in state.finish_events() {
                emit_event(&stream, &mut events, event).await?;
            }
            Ok(events)
        })
    }
}

fn headers_from_config(config: &ChatCompletionsProviderConfig) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    for (name, value) in &config.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| AgentCoreError::Provider(format!("invalid header name: {error}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|error| AgentCoreError::Provider(format!("invalid header value: {error}")))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

async fn emit_event(
    stream: &ModelStreamSink,
    events: &mut Vec<ModelStreamEvent>,
    event: ModelStreamEvent,
) -> Result<()> {
    stream(event.clone()).await?;
    events.push(event);
    Ok(())
}

fn build_chat_payload(
    config: &ChatCompletionsProviderConfig,
    request: &ModelRequest,
) -> Result<Value> {
    let mut payload = Map::new();
    payload.insert("model".into(), Value::String(config.model.clone()));
    payload.insert(
        "messages".into(),
        Value::Array(to_chat_messages(config, request)?),
    );
    payload.insert("stream".into(), Value::Bool(true));
    if config.include_usage {
        payload.insert("stream_options".into(), json!({ "include_usage": true }));
    }
    if let Some(max_completion_tokens) = config.max_completion_tokens {
        payload.insert(
            "max_completion_tokens".into(),
            Value::Number(max_completion_tokens.into()),
        );
    }
    if let Some(temperature) = config.temperature {
        payload.insert("temperature".into(), json!(temperature));
    }
    if !request.tools.is_empty() {
        payload.insert("tools".into(), Value::Array(to_chat_tools(&request.tools)));
    }
    payload.extend(config.extra_body.clone());
    Ok(Value::Object(payload))
}

fn to_chat_messages(
    config: &ChatCompletionsProviderConfig,
    request: &ModelRequest,
) -> Result<Vec<Value>> {
    let mut messages = Vec::new();
    for message in &request.messages {
        match &message.role {
            MessageRole::System => messages.push(json!({
                "role": "system",
                "content": render_content_text(&message.content)?
            })),
            MessageRole::User | MessageRole::Custom(_) => messages.push(json!({
                "role": message.role.as_str(),
                "content": render_content_text(&message.content)?
            })),
            MessageRole::Assistant => messages.push(to_assistant_message(config, message)?),
            MessageRole::ToolResult => {
                messages.extend(to_tool_result_messages(message)?);
            }
        }
    }
    Ok(messages)
}

fn to_assistant_message(
    config: &ChatCompletionsProviderConfig,
    message: &AgentMessage,
) -> Result<Value> {
    let mut rendered = Map::new();
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    for block in &message.content {
        match block {
            ContentBlock::Text { text } => text_parts.push(text.clone()),
            ContentBlock::Json { value } => text_parts.push(value.to_string()),
            ContentBlock::Thinking { thinking } => {
                if let Some((field, value)) = replay_reasoning(config, thinking) {
                    rendered.entry(field).or_insert(value);
                }
            }
            ContentBlock::ToolCall { tool_call } => {
                tool_calls.push(json!({
                    "id": tool_call.id,
                    "type": "function",
                    "function": {
                        "name": tool_call.name,
                        "arguments": tool_call.arguments.to_string()
                    }
                }));
            }
            ContentBlock::ToolResult { .. } => {}
        }
    }

    rendered.insert("role".into(), Value::String("assistant".into()));
    if text_parts.is_empty() && !tool_calls.is_empty() {
        rendered.insert("content".into(), Value::Null);
    } else {
        rendered.insert("content".into(), Value::String(text_parts.join("\n")));
    }
    if !tool_calls.is_empty() {
        rendered.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    Ok(Value::Object(rendered))
}

fn to_tool_result_messages(message: &AgentMessage) -> Result<Vec<Value>> {
    let mut messages = Vec::new();
    for block in &message.content {
        if let ContentBlock::ToolResult {
            tool_call_id,
            tool_name,
            content,
            ..
        } = block
        {
            messages.push(json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "name": tool_name,
                "content": render_content_text(content)?
            }));
        }
    }
    Ok(messages)
}

fn render_content_text(content: &[ContentBlock]) -> Result<String> {
    let mut parts = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => parts.push(text.clone()),
            ContentBlock::Json { value } => parts.push(value.to_string()),
            ContentBlock::Thinking { thinking } => {
                if let Some(text) = &thinking.text {
                    parts.push(text.clone());
                }
            }
            ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => {
                return Err(AgentCoreError::Provider(
                    "tool blocks cannot be rendered as chat message text".into(),
                ));
            }
        }
    }
    Ok(parts.join("\n"))
}

fn to_chat_tools(tools: &[ToolSpec]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema
                }
            })
        })
        .collect()
}

fn replay_reasoning(
    config: &ChatCompletionsProviderConfig,
    thinking: &ThinkingBlock,
) -> Option<(String, Value)> {
    let descriptor = serde_json::from_value::<ChatReasoningReplayDescriptor>(
        thinking.replay_descriptor.as_ref()?.clone(),
    )
    .ok()?;
    if descriptor.v != 1 || descriptor.kind != OPENAI_CHAT_REASONING_REPLAY_KIND {
        return None;
    }
    if descriptor.provider_id.as_str() != config.id.as_str() {
        return None;
    }
    if descriptor.model.as_str() != config.model.as_str() {
        return None;
    }
    Some((descriptor.field.wire_name().into(), thinking.raw.clone()?))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatReasoningReplayDescriptor {
    v: u64,
    kind: String,
    provider_id: String,
    model: String,
    field: ReasoningField,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
enum ReasoningField {
    #[serde(rename = "reasoning_content")]
    ReasoningContent,
    #[serde(rename = "reasoning")]
    Reasoning,
    #[serde(rename = "reasoning_text")]
    ReasoningText,
    #[serde(rename = "reasoning_details")]
    ReasoningDetails,
}

impl ReasoningField {
    fn wire_name(self) -> &'static str {
        match self {
            Self::ReasoningContent => "reasoning_content",
            Self::Reasoning => "reasoning",
            Self::ReasoningText => "reasoning_text",
            Self::ReasoningDetails => "reasoning_details",
        }
    }
}

#[derive(Default)]
struct SseDecoder {
    buffer: String,
    pending_cr: bool,
}

impl SseDecoder {
    fn push(&mut self, chunk: &str) -> Vec<String> {
        self.push_normalized(chunk);
        self.drain_frames()
    }

    fn finish(&mut self) -> Vec<String> {
        if !self.buffer.trim().is_empty() {
            self.buffer.push_str("\n\n");
        }
        self.drain_frames()
    }

    fn push_normalized(&mut self, chunk: &str) {
        for character in chunk.chars() {
            match character {
                '\r' => {
                    self.buffer.push('\n');
                    self.pending_cr = true;
                }
                '\n' if self.pending_cr => {
                    self.pending_cr = false;
                }
                '\n' => {
                    self.buffer.push('\n');
                    self.pending_cr = false;
                }
                character => {
                    self.buffer.push(character);
                    self.pending_cr = false;
                }
            }
        }
    }

    fn drain_frames(&mut self) -> Vec<String> {
        let mut frames = Vec::new();
        while let Some(index) = self.buffer.find("\n\n") {
            let frame = self.buffer[..index].to_string();
            self.buffer.drain(..index + 2);
            if let Some(data) = parse_sse_frame(&frame) {
                frames.push(data);
            }
        }
        frames
    }
}

fn parse_sse_frame(frame: &str) -> Option<String> {
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|line| line.strip_prefix(' ').unwrap_or(line).to_string())
        .collect::<Vec<_>>();
    if data.is_empty() {
        None
    } else {
        Some(data.join("\n"))
    }
}

#[derive(Default)]
struct ChatStreamState {
    replay_scope: ReasoningReplayScope,
    tool_calls: BTreeMap<u64, PartialToolCall>,
    reasoning: BTreeMap<ReasoningField, ReasoningReplayState>,
    stop_reason: Option<StopReason>,
    tool_calls_emitted: bool,
    finished: bool,
    done: bool,
}

impl ChatStreamState {
    fn new(config: &ChatCompletionsProviderConfig) -> Self {
        Self {
            replay_scope: ReasoningReplayScope {
                provider_id: config.id.clone(),
                model: config.model.clone(),
            },
            ..Self::default()
        }
    }

    fn apply_chunk(&mut self, data: &str) -> Result<Vec<ModelStreamEvent>> {
        let value = serde_json::from_str::<Value>(data)?;
        let mut events = Vec::new();
        let Some(choices) = value.get("choices").and_then(Value::as_array) else {
            return Ok(events);
        };
        for choice in choices {
            if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.stop_reason = Some(map_finish_reason(finish_reason));
            }
            let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
                continue;
            };
            if let Some(text) = delta.get("content").and_then(Value::as_str)
                && !text.is_empty()
            {
                events.push(ModelStreamEvent::TextDelta { text: text.into() });
            }
            if let Some(delta) = self.extract_thinking_delta(delta) {
                events.push(ModelStreamEvent::ThinkingDelta { delta });
            }
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                self.absorb_tool_call_chunks(tool_calls);
            }
            if let Some(function_call) = delta.get("function_call").and_then(Value::as_object) {
                self.absorb_legacy_function_call(function_call);
            }
        }
        Ok(events)
    }

    fn finish_events(&mut self) -> Vec<ModelStreamEvent> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        let mut events = Vec::new();
        if !self.tool_calls_emitted {
            self.tool_calls_emitted = true;
            events.extend(self.tool_calls.values().map(PartialToolCall::to_event));
        }
        events.push(ModelStreamEvent::Finished {
            stop_reason: self.stop_reason.clone().unwrap_or(StopReason::Stop),
        });
        events
    }

    fn absorb_tool_call_chunks(&mut self, tool_call_chunks: &[Value]) {
        for (fallback_index, chunk) in tool_call_chunks.iter().enumerate() {
            let Some(chunk) = chunk.as_object() else {
                continue;
            };
            let index = chunk
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or(fallback_index as u64);
            let partial = self
                .tool_calls
                .entry(index)
                .or_insert_with(|| PartialToolCall::new(format!("call-{index}"), String::new()));
            if let Some(id) = chunk.get("id").and_then(Value::as_str)
                && !id.is_empty()
            {
                partial.id = id.into();
            }
            if let Some(function) = chunk.get("function").and_then(Value::as_object) {
                if let Some(name) = function.get("name").and_then(Value::as_str)
                    && !name.is_empty()
                {
                    partial.name = name.into();
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    partial.arguments_json.push_str(arguments);
                }
            }
        }
    }

    fn absorb_legacy_function_call(&mut self, function_call: &Map<String, Value>) {
        let partial = self
            .tool_calls
            .entry(0)
            .or_insert_with(|| PartialToolCall::new("function-call-0", String::new()));
        if let Some(name) = function_call.get("name").and_then(Value::as_str)
            && !name.is_empty()
        {
            partial.name = name.into();
        }
        if let Some(arguments) = function_call.get("arguments").and_then(Value::as_str) {
            partial.arguments_json.push_str(arguments);
        }
    }

    fn extract_thinking_delta(&mut self, delta: &Map<String, Value>) -> Option<ThinkingDelta> {
        let (field, value) = extract_reasoning_value(delta)?;
        let reasoning = self
            .reasoning
            .entry(field)
            .or_insert_with(|| ReasoningReplayState::new(field));
        reasoning.merge(value);
        let rendered = render_reasoning_value(&reasoning.value);
        let text_delta = delta_from_cumulative_value(&reasoning.rendered, &rendered);
        reasoning.rendered = rendered;

        let kind = reasoning_value_kind(&reasoning.value);
        let replay_descriptor = serde_json::to_value(ChatReasoningReplayDescriptor {
            v: 1,
            kind: OPENAI_CHAT_REASONING_REPLAY_KIND.into(),
            provider_id: self.replay_scope.provider_id.clone(),
            model: self.replay_scope.model.clone(),
            field: reasoning.field,
        })
        .ok()?;
        let mut metadata = Map::new();
        metadata.insert(
            "field".into(),
            Value::String(reasoning.field.wire_name().into()),
        );

        Some(ThinkingDelta {
            kind,
            text_delta: (!text_delta.is_empty()).then_some(text_delta),
            raw_snapshot: Some(reasoning.value.clone()),
            replay_descriptor: Some(replay_descriptor),
            metadata,
        })
    }
}

#[derive(Default)]
struct ReasoningReplayScope {
    provider_id: String,
    model: String,
}

#[derive(Clone, Debug)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments_json: String,
}

impl PartialToolCall {
    fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments_json: String::new(),
        }
    }

    fn to_event(&self) -> ModelStreamEvent {
        let arguments = if self.arguments_json.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(&self.arguments_json)
                .unwrap_or_else(|_| Value::String(self.arguments_json.clone()))
        };
        ModelStreamEvent::ToolCall {
            tool_call: ToolCall {
                id: self.id.clone(),
                name: self.name.clone(),
                arguments,
            },
        }
    }
}

struct ReasoningReplayState {
    field: ReasoningField,
    value: Value,
    rendered: String,
}

impl ReasoningReplayState {
    fn new(field: ReasoningField) -> Self {
        Self {
            field,
            value: Value::Null,
            rendered: String::new(),
        }
    }

    fn merge(&mut self, incoming: Value) {
        self.value = merge_reasoning_value(self.field, self.value.clone(), incoming);
    }
}

fn extract_reasoning_value(delta: &Map<String, Value>) -> Option<(ReasoningField, Value)> {
    for field in [
        ReasoningField::ReasoningContent,
        ReasoningField::Reasoning,
        ReasoningField::ReasoningText,
    ] {
        if let Some(value) = delta.get(field.wire_name()) {
            if value.as_str().is_some_and(|text| !text.is_empty()) {
                return Some((field, value.clone()));
            }
            if value.as_object().is_some_and(|object| !object.is_empty())
                || value.as_array().is_some_and(|array| !array.is_empty())
            {
                return Some((field, value.clone()));
            }
        }
    }
    delta
        .get(ReasoningField::ReasoningDetails.wire_name())
        .and_then(|value| {
            normalize_reasoning_details(value)
                .map(|details| (ReasoningField::ReasoningDetails, details))
        })
}

fn merge_reasoning_value(field: ReasoningField, existing: Value, incoming: Value) -> Value {
    if field == ReasoningField::ReasoningDetails {
        return merge_reasoning_details_value(existing, incoming);
    }
    match (existing, incoming) {
        (Value::String(existing), Value::String(incoming)) => {
            Value::String(merge_reasoning_text(&existing, &incoming))
        }
        (Value::Object(existing), Value::Object(incoming)) => {
            Value::Object(merge_reasoning_detail(existing, incoming))
        }
        (Value::Null, incoming) => incoming,
        (_, incoming) => incoming,
    }
}

fn merge_reasoning_details_value(existing: Value, incoming: Value) -> Value {
    match (existing, incoming) {
        (Value::Object(existing), Value::Object(incoming)) => {
            Value::Object(merge_reasoning_detail(existing, incoming))
        }
        (Value::Array(existing), Value::Array(incoming)) => {
            Value::Array(merge_reasoning_details(existing, incoming))
        }
        (_, incoming) => incoming,
    }
}

fn merge_reasoning_details(existing: Vec<Value>, incoming: Vec<Value>) -> Vec<Value> {
    let mut merged = existing;
    for incoming_detail in incoming {
        let Some(incoming_object) = incoming_detail.as_object() else {
            continue;
        };
        let index = find_reasoning_detail_index(&merged, incoming_object);
        if let Some(index) = index {
            let existing_object = merged[index].as_object().cloned().unwrap_or_default();
            merged[index] = Value::Object(merge_reasoning_detail(
                existing_object,
                incoming_object.clone(),
            ));
        } else {
            merged.push(Value::Object(incoming_object.clone()));
        }
    }
    merged
}

fn find_reasoning_detail_index(details: &[Value], needle: &Map<String, Value>) -> Option<usize> {
    if let Some(index) = needle.get("index").and_then(Value::as_u64)
        && let Some(offset) = details
            .iter()
            .position(|detail| detail.get("index").and_then(Value::as_u64) == Some(index))
    {
        return Some(offset);
    }
    let id = needle.get("id").and_then(Value::as_str)?;
    details
        .iter()
        .position(|detail| detail.get("id").and_then(Value::as_str) == Some(id))
}

fn merge_reasoning_detail(
    mut existing: Map<String, Value>,
    incoming: Map<String, Value>,
) -> Map<String, Value> {
    for (key, value) in incoming {
        let previous = existing.get(&key);
        let merged_value = match (key.as_str(), previous, &value) {
            ("text" | "data", Some(Value::String(previous)), Value::String(value)) => {
                Value::String(merge_reasoning_text(previous, value))
            }
            ("summary", Some(Value::String(previous)), Value::String(value)) => {
                Value::String(merge_reasoning_text(previous, value))
            }
            ("summary", Some(Value::Array(previous)), Value::Array(value)) => {
                Value::Array(merge_reasoning_list(previous, value))
            }
            _ => value,
        };
        existing.insert(key, merged_value);
    }
    existing
}

fn merge_reasoning_text(existing: &str, incoming: &str) -> String {
    if incoming.starts_with(existing) {
        incoming.into()
    } else {
        format!("{existing}{incoming}")
    }
}

fn merge_reasoning_list(existing: &[Value], incoming: &[Value]) -> Vec<Value> {
    if incoming.starts_with(existing) {
        incoming.to_vec()
    } else {
        existing.iter().chain(incoming).cloned().collect()
    }
}

fn normalize_reasoning_details(value: &Value) -> Option<Value> {
    match value {
        Value::Object(object) if !object.is_empty() => Some(Value::Object(object.clone())),
        Value::Array(values) => {
            let values = values
                .iter()
                .filter_map(|value| value.as_object().cloned().map(Value::Object))
                .collect::<Vec<_>>();
            (!values.is_empty()).then_some(Value::Array(values))
        }
        _ => None,
    }
}

fn render_reasoning_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(detail) => reasoning_detail_text(detail),
        Value::Array(details) => details
            .iter()
            .filter_map(Value::as_object)
            .map(reasoning_detail_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn reasoning_detail_text(detail: &Map<String, Value>) -> String {
    if let Some(text) = detail.get("text").and_then(Value::as_str) {
        return text.into();
    }
    match detail.get("summary") {
        Some(Value::String(summary)) => summary.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn reasoning_value_kind(value: &Value) -> ThinkingKind {
    match value {
        Value::Object(detail)
            if detail.get("text").is_none() && detail.get("summary").is_some() =>
        {
            ThinkingKind::Summary
        }
        Value::Array(details)
            if details
                .iter()
                .all(|detail| detail.get("text").is_none() && detail.get("summary").is_some()) =>
        {
            ThinkingKind::Summary
        }
        _ => ThinkingKind::Raw,
    }
}

fn delta_from_cumulative_value(previous: &str, current: &str) -> String {
    current
        .strip_prefix(previous)
        .map_or_else(|| current.into(), ToString::to_string)
}

fn map_finish_reason(finish_reason: &str) -> StopReason {
    match finish_reason {
        "stop" | "end" => StopReason::Stop,
        "length" => StopReason::Length,
        "function_call" | "tool_calls" | "tool_use" => StopReason::ToolUse,
        "content_filter" | "network_error" => StopReason::Error,
        _ => StopReason::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_decoder_handles_multiline_data_and_done() {
        let mut decoder = SseDecoder::default();

        let frames = decoder.push(": ignored\n\ndata: one\ndata: two\n\ndata: [DONE]\n\n");

        assert_eq!(frames, ["one\ntwo", "[DONE]"]);
    }

    #[test]
    fn sse_decoder_normalizes_split_crlf_without_extra_frame_boundary() {
        let mut decoder = SseDecoder::default();

        assert!(decoder.push("data: one\r").is_empty());
        assert!(decoder.push("\ndata: two\r").is_empty());
        assert_eq!(decoder.push("\n\r\n"), ["one\ntwo"]);
    }

    #[test]
    fn reasoning_details_merge_prefix_text_by_index() {
        let mut state = ChatStreamState::default();
        let first = state
            .extract_thinking_delta(&Map::from_iter([(
                "reasoning_details".into(),
                json!([{ "index": 0, "text": "ab" }]),
            )]))
            .expect("first reasoning delta");
        let second = state
            .extract_thinking_delta(&Map::from_iter([(
                "reasoning_details".into(),
                json!([{ "index": 0, "text": "abcd" }]),
            )]))
            .expect("second reasoning delta");

        assert_eq!(first.text_delta.as_deref(), Some("ab"));
        assert_eq!(second.text_delta.as_deref(), Some("cd"));
        assert_eq!(
            second.raw_snapshot,
            Some(json!([{ "index": 0, "text": "abcd" }]))
        );
    }
}

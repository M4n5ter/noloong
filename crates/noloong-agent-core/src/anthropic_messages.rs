use crate::provider_utils::{
    ReplayScopeMatch, emit_model_stream_event, headers_from_map, replay_scope_match,
    resolve_api_key,
};
use crate::sse::{SseFrameResult, SseReconnectConfig, SseStreamOptions, run_sse_model_stream};
use crate::tool_arguments::parse_tool_arguments;
use crate::{
    AgentCoreError, AgentMessage, CancellationToken, ContentBlock, MediaBlock, MediaEncoding,
    MediaKind, MediaSource, MessageRole, ModelProvider, ModelRequest, ModelStreamEvent,
    ModelStreamSink, Result, StopReason, ThinkingBlock, ThinkingDelta, ThinkingKind, ToolCall,
    ToolSpec,
};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    fmt::{Debug, Formatter},
    time::Duration,
};

const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_ANTHROPIC_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u64 = 1024;
const ANTHROPIC_THINKING_REPLAY_KIND: &str = "anthropic_messages_thinking_replay";
const FILES_API_BETA_HEADER: &str = "files-api-2025-04-14";

#[derive(Clone)]
pub struct AnthropicMessagesProviderConfig {
    pub id: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub auth_scheme: AnthropicAuthScheme,
    pub headers: BTreeMap<String, String>,
    pub extra_body: Map<String, Value>,
    pub max_tokens: u64,
    pub temperature: Option<f64>,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub stream_reconnect: SseReconnectConfig,
    pub anthropic_version: Option<String>,
    pub beta_headers: Vec<String>,
    pub thinking: Option<AnthropicThinkingConfig>,
    pub allow_files_api_media: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnthropicAuthScheme {
    XApiKey,
    Bearer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnthropicThinkingConfig {
    pub budget_tokens: u64,
}

impl Debug for AnthropicMessagesProviderConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnthropicMessagesProviderConfig")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("api_key_env", &self.api_key_env)
            .field("auth_scheme", &self.auth_scheme)
            .field("headers", &self.headers)
            .field("extra_body", &self.extra_body)
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("request_timeout", &self.request_timeout)
            .field("stream_idle_timeout", &self.stream_idle_timeout)
            .field("stream_reconnect", &self.stream_reconnect)
            .field("anthropic_version", &self.anthropic_version)
            .field("beta_headers", &self.beta_headers)
            .field("thinking", &self.thinking)
            .field("allow_files_api_media", &self.allow_files_api_media)
            .finish()
    }
}

impl AnthropicMessagesProviderConfig {
    pub fn new(id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            base_url: DEFAULT_ANTHROPIC_BASE_URL.into(),
            model: model.into(),
            api_key: None,
            api_key_env: Some(DEFAULT_ANTHROPIC_API_KEY_ENV.into()),
            auth_scheme: AnthropicAuthScheme::XApiKey,
            headers: BTreeMap::new(),
            extra_body: Map::new(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            request_timeout: Duration::from_secs(60),
            stream_idle_timeout: Duration::from_secs(300),
            stream_reconnect: SseReconnectConfig::default(),
            anthropic_version: Some(DEFAULT_ANTHROPIC_VERSION.into()),
            beta_headers: Vec::new(),
            thinking: None,
            allow_files_api_media: false,
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

    pub fn auth_scheme(mut self, auth_scheme: AnthropicAuthScheme) -> Self {
        self.auth_scheme = auth_scheme;
        self
    }

    pub fn anthropic_version(mut self, anthropic_version: impl Into<String>) -> Self {
        self.anthropic_version = Some(anthropic_version.into());
        self
    }

    pub fn without_anthropic_version(mut self) -> Self {
        self.anthropic_version = None;
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn beta_header(mut self, beta_header: impl Into<String>) -> Self {
        self.beta_headers.push(beta_header.into());
        self
    }

    pub fn extra_body(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra_body.insert(key.into(), value);
        self
    }

    pub fn max_tokens(mut self, max_tokens: u64) -> Self {
        self.max_tokens = max_tokens;
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

    pub fn stream_reconnect(mut self, stream_reconnect: SseReconnectConfig) -> Self {
        self.stream_reconnect = stream_reconnect;
        self
    }

    pub fn enable_thinking(mut self, budget_tokens: u64) -> Self {
        self.thinking = Some(AnthropicThinkingConfig { budget_tokens });
        self
    }

    pub fn allow_files_api_media(mut self, allow_files_api_media: bool) -> Self {
        self.allow_files_api_media = allow_files_api_media;
        self
    }
}

pub struct AnthropicMessagesProvider {
    config: AnthropicMessagesProviderConfig,
    client: reqwest::Client,
}

impl AnthropicMessagesProvider {
    pub fn new(config: AnthropicMessagesProviderConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(config.request_timeout)
            .build()?;
        Ok(Self { config, client })
    }

    pub fn config(&self) -> &AnthropicMessagesProviderConfig {
        &self.config
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.config.base_url.trim_end_matches('/'))
    }

    fn api_key(&self) -> Result<Option<String>> {
        resolve_api_key(&self.config.api_key, &self.config.api_key_env)
    }
}

impl ModelProvider for AnthropicMessagesProvider {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn model_name(&self) -> Option<&str> {
        Some(&self.config.model)
    }

    fn stream_model<'a>(
        &'a self,
        request: ModelRequest,
        stream: ModelStreamSink,
        cancellation: CancellationToken,
    ) -> crate::providers::BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let payload = build_anthropic_payload(&self.config, &request)?;
            let headers = headers_from_config(&self.config)?;
            let api_key = self.api_key()?;
            let auth_scheme = self.config.auth_scheme;

            let mut events = Vec::new();
            let mut state = AnthropicStreamState::new(&self.config, &request);
            run_sse_model_stream(
                SseStreamOptions {
                    provider_label: "anthropic messages",
                    request_timeout: self.config.request_timeout,
                    stream_idle_timeout: self.config.stream_idle_timeout,
                    reconnect: &self.config.stream_reconnect,
                    cancellation: &cancellation,
                },
                &stream,
                &mut events,
                || {
                    let mut request_builder = self
                        .client
                        .post(self.endpoint())
                        .headers(headers.clone())
                        .json(&payload);
                    if let Some(api_key) = &api_key {
                        request_builder = match auth_scheme {
                            AnthropicAuthScheme::XApiKey => {
                                request_builder.header("x-api-key", api_key)
                            }
                            AnthropicAuthScheme::Bearer => request_builder.bearer_auth(api_key),
                        };
                    }
                    Ok(request_builder)
                },
                |data| {
                    let events = state.apply_chunk(data)?;
                    Ok(SseFrameResult::new(events, state.done))
                },
            )
            .await?;
            for event in state.finish_events() {
                emit_model_stream_event(&stream, &mut events, event).await?;
            }
            Ok(events)
        })
    }
}

fn headers_from_config(config: &AnthropicMessagesProviderConfig) -> Result<HeaderMap> {
    let mut headers = headers_from_map(&config.headers)?;
    if let Some(version) = &config.anthropic_version {
        let value = HeaderValue::from_str(version).map_err(|error| {
            AgentCoreError::Provider(format!("invalid anthropic-version header: {error}"))
        })?;
        headers.insert("anthropic-version", value);
    }
    let beta_headers = effective_beta_headers(config);
    if !beta_headers.is_empty() {
        let value = HeaderValue::from_str(&beta_headers.join(",")).map_err(|error| {
            AgentCoreError::Provider(format!("invalid anthropic-beta header: {error}"))
        })?;
        headers.insert("anthropic-beta", value);
    }
    Ok(headers)
}

fn effective_beta_headers(config: &AnthropicMessagesProviderConfig) -> Vec<String> {
    let mut beta_headers = config.beta_headers.clone();
    if config.allow_files_api_media
        && !beta_headers
            .iter()
            .any(|header| header == FILES_API_BETA_HEADER)
    {
        beta_headers.push(FILES_API_BETA_HEADER.into());
    }
    beta_headers
}

fn build_anthropic_payload(
    config: &AnthropicMessagesProviderConfig,
    request: &ModelRequest,
) -> Result<Value> {
    let mut payload = Map::new();
    payload.insert("model".into(), Value::String(config.model.clone()));
    payload.insert("max_tokens".into(), Value::Number(config.max_tokens.into()));
    payload.insert("stream".into(), Value::Bool(true));
    if let Some(temperature) = config.temperature {
        payload.insert("temperature".into(), json!(temperature));
    }
    if let Some(thinking) = &config.thinking {
        payload.insert(
            "thinking".into(),
            json!({
                "type": "enabled",
                "budget_tokens": thinking.budget_tokens,
            }),
        );
    }
    if !request.tools.is_empty() {
        payload.insert(
            "tools".into(),
            Value::Array(request.tools.iter().map(to_anthropic_tool).collect()),
        );
    }
    if let Some(system) = render_system_messages(&request.messages)? {
        payload.insert("system".into(), system);
    }
    payload.insert(
        "messages".into(),
        Value::Array(to_anthropic_messages(config, request)?),
    );
    payload.extend(config.extra_body.clone());
    Ok(Value::Object(payload))
}

fn render_system_messages(messages: &[AgentMessage]) -> Result<Option<Value>> {
    let mut blocks = Vec::new();
    for message in messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::System))
    {
        blocks.extend(render_anthropic_content_text_only(&message.content)?);
    }
    Ok((!blocks.is_empty()).then_some(Value::Array(blocks)))
}

fn to_anthropic_messages(
    config: &AnthropicMessagesProviderConfig,
    request: &ModelRequest,
) -> Result<Vec<Value>> {
    let mut messages = Vec::new();
    for message in request
        .messages
        .iter()
        .filter(|message| !matches!(message.role, MessageRole::System))
    {
        match &message.role {
            MessageRole::User => messages.push(json!({
                "role": "user",
                "content": render_anthropic_user_content(config, &message.content)?,
            })),
            MessageRole::Assistant => messages.push(json!({
                "role": "assistant",
                "content": render_anthropic_assistant_content(config, &message.content)?,
            })),
            MessageRole::ToolResult => messages.push(json!({
                "role": "user",
                "content": render_anthropic_tool_results(config, &message.content)?,
            })),
            MessageRole::Custom(role) => {
                return Err(AgentCoreError::Provider(format!(
                    "custom role cannot be rendered for anthropic messages: {role}"
                )));
            }
            MessageRole::System => {}
        }
    }
    Ok(messages)
}

fn render_anthropic_user_content(
    config: &AnthropicMessagesProviderConfig,
    content: &[ContentBlock],
) -> Result<Vec<Value>> {
    let mut blocks = Vec::new();
    for block in content {
        if let Some(block) = render_text_json_thinking(block, ThinkingRenderPolicy::Text)? {
            blocks.push(block);
            continue;
        }
        match block {
            ContentBlock::Media { media } => blocks.push(media_to_anthropic_block(config, media)?),
            ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => {
                return Err(AgentCoreError::Provider(
                    "tool blocks cannot be rendered as anthropic user content".into(),
                ));
            }
            ContentBlock::Text { .. }
            | ContentBlock::Json { .. }
            | ContentBlock::Thinking { .. } => {}
        }
    }
    Ok(blocks)
}

fn render_anthropic_content_text_only(content: &[ContentBlock]) -> Result<Vec<Value>> {
    let mut blocks = Vec::new();
    for block in content {
        if let Some(block) = render_text_json_thinking(block, ThinkingRenderPolicy::Text)? {
            blocks.push(block);
            continue;
        }
        match block {
            ContentBlock::Media { .. } => {
                return Err(AgentCoreError::Provider(
                    "media blocks cannot be rendered as anthropic system content".into(),
                ));
            }
            ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => {
                return Err(AgentCoreError::Provider(
                    "tool blocks cannot be rendered as anthropic text content".into(),
                ));
            }
            ContentBlock::Text { .. }
            | ContentBlock::Json { .. }
            | ContentBlock::Thinking { .. } => {}
        }
    }
    Ok(blocks)
}

fn render_anthropic_assistant_content(
    config: &AnthropicMessagesProviderConfig,
    content: &[ContentBlock],
) -> Result<Vec<Value>> {
    let mut blocks = Vec::new();
    for block in content {
        if let Some(block) = render_text_json_thinking(block, ThinkingRenderPolicy::Replay(config))?
        {
            blocks.push(block);
            continue;
        }
        match block {
            ContentBlock::ToolCall { tool_call } => blocks.push(json!({
                "type": "tool_use",
                "id": tool_call.id,
                "name": tool_call.name,
                "input": tool_call.arguments,
            })),
            ContentBlock::Media { .. } => {
                return Err(AgentCoreError::Provider(
                    "assistant media blocks cannot be rendered for anthropic messages".into(),
                ));
            }
            ContentBlock::ToolResult { .. } => {
                return Err(AgentCoreError::Provider(
                    "tool result blocks cannot be rendered as anthropic assistant content".into(),
                ));
            }
            ContentBlock::Text { .. }
            | ContentBlock::Json { .. }
            | ContentBlock::Thinking { .. } => {}
        }
    }
    Ok(blocks)
}

fn render_anthropic_tool_results(
    config: &AnthropicMessagesProviderConfig,
    content: &[ContentBlock],
) -> Result<Vec<Value>> {
    let mut blocks = Vec::new();
    for block in content {
        match block {
            ContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => blocks.push(json!({
                "type": "tool_result",
                "tool_use_id": tool_call_id,
                "content": render_tool_result_content(config, content)?,
                "is_error": is_error,
            })),
            _ => {
                return Err(AgentCoreError::Provider(
                    "only tool result blocks can be rendered as anthropic tool result messages"
                        .into(),
                ));
            }
        }
    }
    Ok(blocks)
}

fn render_tool_result_content(
    config: &AnthropicMessagesProviderConfig,
    content: &[ContentBlock],
) -> Result<Vec<Value>> {
    let mut blocks = Vec::new();
    for block in content {
        if let Some(block) = render_text_json_thinking(
            block,
            ThinkingRenderPolicy::Reject(
                "thinking blocks cannot be rendered as anthropic tool results",
            ),
        )? {
            blocks.push(block);
            continue;
        }
        match block {
            ContentBlock::Media { media } => blocks.push(media_to_anthropic_block(config, media)?),
            ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => {
                return Err(AgentCoreError::Provider(
                    "nested tool blocks cannot be rendered as anthropic tool results".into(),
                ));
            }
            ContentBlock::Text { .. }
            | ContentBlock::Json { .. }
            | ContentBlock::Thinking { .. } => {}
        }
    }
    Ok(blocks)
}

enum ThinkingRenderPolicy<'a> {
    Text,
    Replay(&'a AnthropicMessagesProviderConfig),
    Reject(&'static str),
}

fn render_text_json_thinking(
    block: &ContentBlock,
    thinking_policy: ThinkingRenderPolicy<'_>,
) -> Result<Option<Value>> {
    match block {
        ContentBlock::Text { text } => Ok(Some(anthropic_text_block(text.clone()))),
        ContentBlock::Json { value } => Ok(Some(anthropic_text_block(value.to_string()))),
        ContentBlock::Thinking { thinking } => match thinking_policy {
            ThinkingRenderPolicy::Text => Ok(thinking
                .text
                .as_ref()
                .map(|text| anthropic_text_block(text.clone()))),
            ThinkingRenderPolicy::Replay(config) => Ok(replay_thinking(config, thinking)),
            ThinkingRenderPolicy::Reject(message) => Err(AgentCoreError::Provider(message.into())),
        },
        _ => Ok(None),
    }
}

fn media_to_anthropic_block(
    config: &AnthropicMessagesProviderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    match &media.kind {
        MediaKind::Image => image_to_anthropic_block(media),
        MediaKind::File => document_to_anthropic_block(config, media),
        MediaKind::Audio => Err(AgentCoreError::Provider(
            "audio media is not supported by the built-in anthropic messages provider v1".into(),
        )),
        MediaKind::Video => Err(AgentCoreError::Provider(
            "video media is not supported by the built-in anthropic messages provider v1".into(),
        )),
        MediaKind::Custom(kind) => Err(AgentCoreError::Provider(format!(
            "custom media kind cannot be rendered by anthropic messages: {kind}"
        ))),
    }
}

fn image_to_anthropic_block(media: &MediaBlock) -> Result<Value> {
    Ok(json!({
        "type": "image",
        "source": media_source_to_anthropic_source(media, "image")?,
    }))
}

fn document_to_anthropic_block(
    config: &AnthropicMessagesProviderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    let mut block = Map::new();
    block.insert("type".into(), Value::String("document".into()));
    block.insert(
        "source".into(),
        media_source_to_anthropic_document_source(config, media)?,
    );
    if let Some(name) = &media.name {
        block.insert("title".into(), Value::String(name.clone()));
    }
    Ok(Value::Object(block))
}

fn media_source_to_anthropic_source(media: &MediaBlock, label: &str) -> Result<Value> {
    match &media.source {
        MediaSource::Inline {
            data,
            encoding: MediaEncoding::Base64,
        } => {
            let mime_type = media.mime_type.as_deref().ok_or_else(|| {
                AgentCoreError::Provider(format!("inline {label} media requires mime_type"))
            })?;
            Ok(json!({
                "type": "base64",
                "media_type": mime_type,
                "data": data,
            }))
        }
        MediaSource::Inline { .. } => Err(AgentCoreError::Provider(format!(
            "inline {label} media must use base64 encoding"
        ))),
        MediaSource::Uri { uri } => Ok(json!({
            "type": "url",
            "url": uri,
        })),
        MediaSource::Provider { .. } => Err(AgentCoreError::Provider(format!(
            "provider-referenced {label} media is not supported"
        ))),
    }
}

fn media_source_to_anthropic_document_source(
    config: &AnthropicMessagesProviderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    match &media.source {
        MediaSource::Provider { provider_id, id } => {
            if !config.allow_files_api_media {
                return Err(AgentCoreError::Provider(
                    "provider file media requires allow_files_api_media(true)".into(),
                ));
            }
            if provider_id != &config.id {
                return Err(AgentCoreError::Provider(
                    "provider file media source does not match the anthropic provider id".into(),
                ));
            }
            Ok(json!({
                "type": "file",
                "file_id": id,
            }))
        }
        _ => media_source_to_anthropic_source(media, "document"),
    }
}

fn anthropic_text_block(text: String) -> Value {
    json!({ "type": "text", "text": text })
}

fn to_anthropic_tool(tool: &ToolSpec) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.input_schema,
    })
}

fn replay_thinking(
    config: &AnthropicMessagesProviderConfig,
    thinking: &ThinkingBlock,
) -> Option<Value> {
    let descriptor = serde_json::from_value::<AnthropicThinkingReplayDescriptor>(
        thinking.replay_descriptor.as_ref()?.clone(),
    )
    .ok()?;
    match replay_scope_match(
        descriptor.v,
        &descriptor.kind,
        ANTHROPIC_THINKING_REPLAY_KIND,
        &descriptor.provider_id,
        &config.id,
        &descriptor.model,
        &config.model,
    ) {
        ReplayScopeMatch::Match => {}
        ReplayScopeMatch::Ignore | ReplayScopeMatch::Unsupported => return None,
    }
    let text = thinking
        .raw
        .as_ref()
        .and_then(|raw| raw.get("thinking"))
        .and_then(Value::as_str)
        .or(thinking.text.as_deref())?;
    let mut block = Map::new();
    block.insert("type".into(), Value::String("thinking".into()));
    block.insert("thinking".into(), Value::String(text.into()));
    if let Some(signature) = descriptor
        .signature
        .as_deref()
        .or_else(|| thinking.metadata.get("signature").and_then(Value::as_str))
    {
        block.insert("signature".into(), Value::String(signature.into()));
    }
    Some(Value::Object(block))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnthropicThinkingReplayDescriptor {
    v: u64,
    kind: String,
    provider_id: String,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

#[derive(Default)]
struct AnthropicStreamState {
    provider_id: String,
    model: String,
    run_id: String,
    turn_id: u64,
    tool_calls: BTreeMap<u64, PartialAnthropicToolCall>,
    thinking: BTreeMap<u64, AnthropicThinkingState>,
    stop_reason: Option<StopReason>,
    finished: bool,
    done: bool,
}

impl AnthropicStreamState {
    fn new(config: &AnthropicMessagesProviderConfig, request: &ModelRequest) -> Self {
        Self {
            provider_id: config.id.clone(),
            model: config.model.clone(),
            run_id: request.run_id.clone(),
            turn_id: request.turn_id,
            ..Self::default()
        }
    }

    fn apply_chunk(&mut self, data: &str) -> Result<Vec<ModelStreamEvent>> {
        if data.trim() == "[DONE]" {
            self.done = true;
            return Ok(self.finish_events());
        }
        let value = serde_json::from_str::<Value>(data)?;
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            return Ok(Vec::new());
        };
        match event_type {
            "message_start" => Ok(vec![self.started_event(&value)]),
            "content_block_start" => self.apply_content_block_start(&value),
            "content_block_delta" => self.apply_content_block_delta(&value),
            "content_block_stop" => self.apply_content_block_stop(&value),
            "message_delta" => {
                if let Some(stop_reason) = value
                    .get("delta")
                    .and_then(Value::as_object)
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    self.stop_reason = Some(map_stop_reason(stop_reason));
                }
                Ok(Vec::new())
            }
            "message_stop" => {
                self.done = true;
                Ok(self.finish_events())
            }
            "error" => {
                let message = value
                    .get("error")
                    .and_then(Value::as_object)
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("anthropic messages stream error");
                self.done = true;
                self.finished = true;
                Ok(vec![ModelStreamEvent::Failed {
                    error: message.into(),
                }])
            }
            "ping" => Ok(Vec::new()),
            _ => Ok(Vec::new()),
        }
    }

    fn started_event(&mut self, value: &Value) -> ModelStreamEvent {
        let stream_id = value
            .get("message")
            .and_then(Value::as_object)
            .and_then(|message| message.get("id"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("anthropic-messages-{}-{}", self.run_id, self.turn_id));
        ModelStreamEvent::Started { stream_id }
    }

    fn apply_content_block_start(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let Some(index) = value.get("index").and_then(Value::as_u64) else {
            return Ok(Vec::new());
        };
        let Some(block) = value.get("content_block").and_then(Value::as_object) else {
            return Ok(Vec::new());
        };
        match block.get("type").and_then(Value::as_str) {
            Some("tool_use") => {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let arguments_json = block
                    .get("input")
                    .filter(|input| !input.is_null())
                    .map(Value::to_string)
                    .filter(|input| input != "{}")
                    .unwrap_or_default();
                self.tool_calls.insert(
                    index,
                    PartialAnthropicToolCall {
                        id,
                        name,
                        arguments_json,
                    },
                );
            }
            Some("thinking") => {
                self.thinking.entry(index).or_default();
            }
            _ => {}
        }
        Ok(Vec::new())
    }

    fn apply_content_block_delta(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let Some(index) = value.get("index").and_then(Value::as_u64) else {
            return Ok(Vec::new());
        };
        let Some(delta) = value.get("delta").and_then(Value::as_object) else {
            return Ok(Vec::new());
        };
        match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => {
                if let Some(text) = delta.get("text").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    return Ok(vec![ModelStreamEvent::TextDelta { text: text.into() }]);
                }
            }
            Some("input_json_delta") => {
                let partial = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.tool_calls
                    .entry(index)
                    .or_default()
                    .arguments_json
                    .push_str(partial);
            }
            Some("thinking_delta") => {
                if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
                    let state = self.thinking.entry(index).or_default();
                    state.thinking.push_str(thinking);
                    return Ok(vec![anthropic_thinking_delta(
                        &self.provider_id,
                        &self.model,
                        index,
                        state,
                        Some(thinking.into()),
                        false,
                    )]);
                }
            }
            Some("signature_delta") => {
                if let Some(signature) = delta.get("signature").and_then(Value::as_str) {
                    let state = self.thinking.entry(index).or_default();
                    state.signature = Some(signature.into());
                    return Ok(vec![anthropic_thinking_delta(
                        &self.provider_id,
                        &self.model,
                        index,
                        state,
                        None,
                        true,
                    )]);
                }
            }
            _ => {}
        }
        Ok(Vec::new())
    }

    fn apply_content_block_stop(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let Some(index) = value.get("index").and_then(Value::as_u64) else {
            return Ok(Vec::new());
        };
        if let Some(tool_call) = self.tool_calls.remove(&index) {
            return Ok(vec![tool_call.to_event(index)]);
        }
        if let Some(thinking) = self.thinking.remove(&index) {
            return Ok(vec![anthropic_thinking_delta(
                &self.provider_id,
                &self.model,
                index,
                &thinking,
                None,
                true,
            )]);
        }
        Ok(Vec::new())
    }

    fn finish_events(&mut self) -> Vec<ModelStreamEvent> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        vec![ModelStreamEvent::Finished {
            stop_reason: self.stop_reason.clone().unwrap_or(StopReason::Stop),
        }]
    }
}

#[derive(Clone, Default)]
struct AnthropicThinkingState {
    thinking: String,
    signature: Option<String>,
}

fn anthropic_thinking_delta(
    provider_id: &str,
    model: &str,
    index: u64,
    state: &AnthropicThinkingState,
    text_delta: Option<String>,
    include_raw_snapshot: bool,
) -> ModelStreamEvent {
    let replay_descriptor = serde_json::to_value(AnthropicThinkingReplayDescriptor {
        v: 1,
        kind: ANTHROPIC_THINKING_REPLAY_KIND.into(),
        provider_id: provider_id.into(),
        model: model.into(),
        signature: state.signature.clone(),
    })
    .ok();
    let mut metadata = Map::new();
    metadata.insert("index".into(), Value::Number(index.into()));
    if let Some(signature) = &state.signature {
        metadata.insert("signature".into(), Value::String(signature.clone()));
    }
    ModelStreamEvent::ThinkingDelta {
        delta: ThinkingDelta {
            kind: ThinkingKind::Raw,
            text_delta,
            raw_snapshot: include_raw_snapshot.then(|| {
                json!({
                    "thinking": state.thinking,
                    "signature": state.signature,
                })
            }),
            replay_descriptor,
            metadata,
        },
    }
}

#[derive(Default)]
struct PartialAnthropicToolCall {
    id: String,
    name: String,
    arguments_json: String,
}

impl PartialAnthropicToolCall {
    fn to_event(&self, index: u64) -> ModelStreamEvent {
        let arguments = parse_tool_arguments(&self.arguments_json);
        ModelStreamEvent::ToolCall {
            tool_call: ToolCall {
                id: if self.id.is_empty() {
                    format!("toolu-{index}")
                } else {
                    self.id.clone()
                },
                name: self.name.clone(),
                arguments,
            },
        }
    }
}

fn map_stop_reason(stop_reason: &str) -> StopReason {
    match stop_reason {
        "end_turn" | "stop_sequence" => StopReason::Stop,
        "max_tokens" => StopReason::Length,
        "tool_use" => StopReason::ToolUse,
        _ => StopReason::Error,
    }
}

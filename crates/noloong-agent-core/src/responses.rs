use crate::provider_utils::{
    ReplayScopeMatch, emit_model_stream_event, headers_from_map, refresh_auth_provider,
    replay_scope_match, resolve_auth_headers,
};
use crate::sse::{SseFrameResult, SseReconnectConfig, SseStreamOptions, run_sse_model_stream};
use crate::tool_arguments::parse_tool_arguments;
use crate::tool_names::ProviderToolNameCodec;
use crate::{
    AgentCoreError, AgentMessage, CancellationToken, ContentBlock, HttpAuthContext, HttpAuthHeader,
    HttpAuthProvider, HttpAuthRefreshContext, MediaBlock, MediaEncoding, MediaKind, MediaSource,
    MessageRole, ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink, Result,
    StopReason, ThinkingBlock, ThinkingDelta, ThinkingKind, ToolCall, ToolSpec,
};
use reqwest::header::HeaderMap;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Debug, Formatter},
    sync::Arc,
    time::Duration,
};

const DEFAULT_RESPONSES_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";
const RESPONSES_REASONING_REPLAY_KIND: &str = "openai_responses_reasoning_replay";
pub const RESPONSES_PROVIDER_PAYLOAD: &str = "openai.responses";
pub const RESPONSES_RESPONSE_ITEM_PAYLOAD: &str = "response_item";

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ResponsesStateMode {
    #[default]
    Stateless,
    Stateful,
}

impl ResponsesStateMode {
    pub const fn store(self) -> bool {
        matches!(self, Self::Stateful)
    }

    pub const fn is_stateless(self) -> bool {
        matches!(self, Self::Stateless)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponsesReplayItemSource {
    RequestHistory,
    ThinkingHistory,
    CompactOutput,
}

#[derive(Clone)]
pub struct ResponsesApiProviderConfig {
    pub id: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub auth_provider: Option<Arc<dyn HttpAuthProvider>>,
    pub headers: BTreeMap<String, String>,
    pub extra_body: Map<String, Value>,
    pub max_output_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub text: Option<Value>,
    pub fallback_instructions: Option<String>,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub stream_reconnect: SseReconnectConfig,
    pub state_mode: ResponsesStateMode,
    pub reasoning: Option<ResponsesReasoningConfig>,
    pub include_encrypted_reasoning: bool,
    pub native_tools: Vec<Value>,
    pub function_tool_strict: Option<bool>,
    pub allow_file_data_url_input: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResponsesReasoningConfig {
    pub effort: Option<ResponsesReasoningEffort>,
    pub summary: Option<ResponsesReasoningSummary>,
}

impl ResponsesReasoningConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn effort(mut self, effort: ResponsesReasoningEffort) -> Self {
        self.effort = Some(effort);
        self
    }

    pub fn summary(mut self, summary: ResponsesReasoningSummary) -> Self {
        self.summary = Some(summary);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponsesReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Custom(String),
}

impl ResponsesReasoningEffort {
    fn as_str(&self) -> &str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Custom(effort) => effort,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponsesReasoningSummary {
    Auto,
    Concise,
    Detailed,
    None,
    Custom(String),
}

impl ResponsesReasoningSummary {
    fn as_str(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Concise => "concise",
            Self::Detailed => "detailed",
            Self::None => "none",
            Self::Custom(summary) => summary,
        }
    }
}

impl Debug for ResponsesApiProviderConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResponsesApiProviderConfig")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("api_key_env", &self.api_key_env)
            .field(
                "auth_provider",
                &self.auth_provider.as_ref().map(|provider| provider.id()),
            )
            .field("headers", &self.headers)
            .field("extra_body", &self.extra_body)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("temperature", &self.temperature)
            .field("text", &self.text)
            .field("fallback_instructions", &self.fallback_instructions)
            .field("request_timeout", &self.request_timeout)
            .field("stream_idle_timeout", &self.stream_idle_timeout)
            .field("stream_reconnect", &self.stream_reconnect)
            .field("state_mode", &self.state_mode)
            .field("reasoning", &self.reasoning)
            .field(
                "include_encrypted_reasoning",
                &self.include_encrypted_reasoning,
            )
            .field("native_tools", &self.native_tools)
            .field("function_tool_strict", &self.function_tool_strict)
            .field("allow_file_data_url_input", &self.allow_file_data_url_input)
            .finish()
    }
}

impl ResponsesApiProviderConfig {
    pub fn new(id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            base_url: DEFAULT_RESPONSES_BASE_URL.into(),
            model: model.into(),
            api_key: None,
            api_key_env: Some(DEFAULT_OPENAI_API_KEY_ENV.into()),
            auth_provider: None,
            headers: BTreeMap::new(),
            extra_body: Map::new(),
            max_output_tokens: None,
            temperature: None,
            text: None,
            fallback_instructions: None,
            request_timeout: Duration::from_secs(60),
            stream_idle_timeout: Duration::from_secs(300),
            stream_reconnect: SseReconnectConfig::default(),
            state_mode: ResponsesStateMode::default(),
            reasoning: None,
            include_encrypted_reasoning: false,
            native_tools: Vec::new(),
            function_tool_strict: None,
            allow_file_data_url_input: false,
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

    pub fn auth_provider(mut self, auth_provider: Arc<dyn HttpAuthProvider>) -> Self {
        self.auth_provider = Some(auth_provider);
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

    pub fn max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn text_controls(mut self, text: Value) -> Self {
        self.text = Some(text);
        self
    }

    pub fn fallback_instructions(mut self, fallback_instructions: impl Into<String>) -> Self {
        self.fallback_instructions = Some(fallback_instructions.into());
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

    pub fn store(mut self, store: bool) -> Self {
        self.state_mode = if store {
            ResponsesStateMode::Stateful
        } else {
            ResponsesStateMode::Stateless
        };
        self
    }

    pub fn with_state_mode(mut self, state_mode: ResponsesStateMode) -> Self {
        self.state_mode = state_mode;
        self
    }

    pub fn stateless(mut self) -> Self {
        self.state_mode = ResponsesStateMode::Stateless;
        self
    }

    pub fn stateful(mut self) -> Self {
        self.state_mode = ResponsesStateMode::Stateful;
        self
    }

    pub fn reasoning(mut self, reasoning: ResponsesReasoningConfig) -> Self {
        self.reasoning = Some(reasoning);
        self
    }

    pub fn include_encrypted_reasoning(mut self, include_encrypted_reasoning: bool) -> Self {
        self.include_encrypted_reasoning = include_encrypted_reasoning;
        self
    }

    pub fn native_tool(mut self, tool: Value) -> Self {
        self.native_tools.push(tool);
        self
    }

    pub fn native_tools(mut self, tools: impl IntoIterator<Item = Value>) -> Self {
        self.native_tools = tools.into_iter().collect();
        self
    }

    pub fn function_tool_strict(mut self, strict: bool) -> Self {
        self.function_tool_strict = Some(strict);
        self
    }

    pub fn allow_file_data_url_input(mut self, allow_file_data_url_input: bool) -> Self {
        self.allow_file_data_url_input = allow_file_data_url_input;
        self
    }
}

#[derive(Clone, Debug)]
pub struct ResponsesApiRequestRenderConfig {
    pub provider_id: String,
    pub model: String,
    pub extra_body: Map<String, Value>,
    pub max_output_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub text: Option<Value>,
    pub fallback_instructions: Option<String>,
    pub state_mode: ResponsesStateMode,
    pub reasoning: Option<ResponsesReasoningConfig>,
    pub include_encrypted_reasoning: bool,
    pub native_tools: Vec<Value>,
    pub function_tool_strict: Option<bool>,
    pub allow_file_data_url_input: bool,
}

impl ResponsesApiRequestRenderConfig {
    pub fn new(provider_id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model: model.into(),
            extra_body: Map::new(),
            max_output_tokens: None,
            temperature: None,
            text: None,
            fallback_instructions: None,
            state_mode: ResponsesStateMode::default(),
            reasoning: None,
            include_encrypted_reasoning: false,
            native_tools: Vec::new(),
            function_tool_strict: None,
            allow_file_data_url_input: false,
        }
    }

    pub fn extra_body(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra_body.insert(key.into(), value);
        self
    }

    pub fn max_output_tokens(mut self, max_output_tokens: u64) -> Self {
        self.max_output_tokens = Some(max_output_tokens);
        self
    }

    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn text_controls(mut self, text: Value) -> Self {
        self.text = Some(text);
        self
    }

    pub fn fallback_instructions(mut self, fallback_instructions: impl Into<String>) -> Self {
        self.fallback_instructions = Some(fallback_instructions.into());
        self
    }

    pub fn store(mut self, store: bool) -> Self {
        self.state_mode = if store {
            ResponsesStateMode::Stateful
        } else {
            ResponsesStateMode::Stateless
        };
        self
    }

    pub fn with_state_mode(mut self, state_mode: ResponsesStateMode) -> Self {
        self.state_mode = state_mode;
        self
    }

    pub fn stateless(mut self) -> Self {
        self.state_mode = ResponsesStateMode::Stateless;
        self
    }

    pub fn stateful(mut self) -> Self {
        self.state_mode = ResponsesStateMode::Stateful;
        self
    }

    pub fn reasoning(mut self, reasoning: ResponsesReasoningConfig) -> Self {
        self.reasoning = Some(reasoning);
        self
    }

    pub fn include_encrypted_reasoning(mut self, include_encrypted_reasoning: bool) -> Self {
        self.include_encrypted_reasoning = include_encrypted_reasoning;
        self
    }

    pub fn native_tool(mut self, tool: Value) -> Self {
        self.native_tools.push(tool);
        self
    }

    pub fn native_tools(mut self, tools: impl IntoIterator<Item = Value>) -> Self {
        self.native_tools = tools.into_iter().collect();
        self
    }

    pub fn function_tool_strict(mut self, strict: bool) -> Self {
        self.function_tool_strict = Some(strict);
        self
    }

    pub fn allow_file_data_url_input(mut self, allow_file_data_url_input: bool) -> Self {
        self.allow_file_data_url_input = allow_file_data_url_input;
        self
    }
}

impl From<&ResponsesApiProviderConfig> for ResponsesApiRequestRenderConfig {
    fn from(config: &ResponsesApiProviderConfig) -> Self {
        Self {
            provider_id: config.id.clone(),
            model: config.model.clone(),
            extra_body: config.extra_body.clone(),
            max_output_tokens: config.max_output_tokens,
            temperature: config.temperature,
            text: config.text.clone(),
            fallback_instructions: config.fallback_instructions.clone(),
            state_mode: config.state_mode,
            reasoning: config.reasoning.clone(),
            include_encrypted_reasoning: config.include_encrypted_reasoning,
            native_tools: config.native_tools.clone(),
            function_tool_strict: config.function_tool_strict,
            allow_file_data_url_input: config.allow_file_data_url_input,
        }
    }
}

pub struct ResponsesApiProvider {
    config: ResponsesApiProviderConfig,
    client: reqwest::Client,
    endpoint: String,
    headers: HeaderMap,
}

impl ResponsesApiProvider {
    pub fn new(config: ResponsesApiProviderConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(config.request_timeout)
            .build()?;
        let endpoint = format!("{}/responses", config.base_url.trim_end_matches('/'));
        let headers = headers_from_map(&config.headers)?;
        Ok(Self {
            config,
            client,
            endpoint,
            headers,
        })
    }

    pub fn config(&self) -> &ResponsesApiProviderConfig {
        &self.config
    }

    async fn request_headers(
        &self,
        attempt: u32,
        refreshed_headers: Option<Vec<HttpAuthHeader>>,
        cancellation: CancellationToken,
    ) -> Result<HeaderMap> {
        let mut headers = self.headers.clone();
        headers.extend(
            resolve_auth_headers(
                self.config.auth_provider.as_ref(),
                &self.config.api_key,
                &self.config.api_key_env,
                self.auth_context("POST", attempt),
                refreshed_headers,
                cancellation,
            )
            .await?,
        );
        Ok(headers)
    }

    fn auth_context(&self, method: &str, attempt: u32) -> HttpAuthContext {
        HttpAuthContext::new(&self.config.id, method, &self.endpoint, attempt)
    }
}

impl ModelProvider for ResponsesApiProvider {
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
            let render_config = ResponsesApiRequestRenderConfig::from(&self.config);
            let tool_names = ProviderToolNameCodec::new(&request.tools);
            let payload = render_responses_api_request_with_tool_names(
                &render_config,
                &tool_names,
                &request,
            )?;

            let mut events = Vec::new();
            let mut state = ResponsesStreamState::new(&self.config, &request, tool_names);
            let mut attempt = 0_u32;
            let mut refreshed_headers = None;
            loop {
                let headers = self
                    .request_headers(attempt, refreshed_headers.take(), cancellation.clone())
                    .await?;
                let result = run_sse_model_stream(
                    SseStreamOptions {
                        provider_label: "responses api",
                        request_timeout: self.config.request_timeout,
                        stream_idle_timeout: self.config.stream_idle_timeout,
                        reconnect: &self.config.stream_reconnect,
                        cancellation: &cancellation,
                    },
                    &stream,
                    &mut events,
                    || {
                        Ok(self
                            .client
                            .post(&self.endpoint)
                            .headers(headers.clone())
                            .json(&payload))
                    },
                    |data| {
                        let events = state.apply_chunk(data)?;
                        Ok(SseFrameResult::new(events, state.done))
                    },
                )
                .await;
                match result {
                    Ok(()) => break,
                    Err(error @ AgentCoreError::HttpStatus { status: 401, .. })
                        if attempt == 0 && self.config.auth_provider.is_some() =>
                    {
                        let refresh_context = HttpAuthRefreshContext::unauthorized(
                            self.auth_context("POST", attempt),
                            401,
                        );
                        if let Some(refresh) = refresh_auth_provider(
                            self.config.auth_provider.as_ref(),
                            refresh_context,
                            cancellation.clone(),
                        )
                        .await?
                            && refresh.retry
                        {
                            refreshed_headers = refresh.headers;
                            attempt += 1;
                            continue;
                        }
                        return Err(error);
                    }
                    Err(error) => return Err(error),
                }
            }
            for event in state.finish_events() {
                emit_model_stream_event(&stream, &mut events, event).await?;
            }
            Ok(events)
        })
    }
}

pub fn render_responses_api_request(
    config: &ResponsesApiRequestRenderConfig,
    request: &ModelRequest,
) -> Result<Value> {
    let tool_names = ProviderToolNameCodec::new(&request.tools);
    render_responses_api_request_with_tool_names(config, &tool_names, request)
}

fn render_responses_api_request_with_tool_names(
    config: &ResponsesApiRequestRenderConfig,
    tool_names: &ProviderToolNameCodec,
    request: &ModelRequest,
) -> Result<Value> {
    validate_responses_extra_body(&config.extra_body)?;
    let mut payload = Map::new();
    let rendered_messages = render_responses_messages(config, tool_names, &request.messages)?;
    payload.insert("model".into(), Value::String(config.model.clone()));
    payload.insert("input".into(), Value::Array(rendered_messages.input));
    payload.insert("stream".into(), Value::Bool(true));
    payload.insert("store".into(), Value::Bool(config.state_mode.store()));
    if let Some(max_output_tokens) = config.max_output_tokens {
        payload.insert(
            "max_output_tokens".into(),
            Value::Number(max_output_tokens.into()),
        );
    }
    if let Some(temperature) = config.temperature {
        payload.insert("temperature".into(), json!(temperature));
    }
    if let Some(text) = &config.text {
        payload.insert("text".into(), text.clone());
    }
    if let Some(reasoning) = &config.reasoning {
        payload.insert("reasoning".into(), reasoning_to_value(reasoning));
    }
    if should_include_encrypted_reasoning(config) {
        payload.insert(
            "include".into(),
            Value::Array(vec![Value::String("reasoning.encrypted_content".into())]),
        );
    }
    if let Some(instructions) = rendered_messages
        .instructions
        .or_else(|| config.fallback_instructions.clone())
        .filter(|instructions| !instructions.is_empty())
    {
        payload.insert("instructions".into(), Value::String(instructions));
    }
    let tools = to_responses_tools(config, tool_names, &request.tools)?;
    if !tools.is_empty() {
        payload.insert("tools".into(), Value::Array(tools));
    }
    payload.extend(config.extra_body.clone());
    Ok(Value::Object(payload))
}

fn validate_responses_extra_body(extra_body: &Map<String, Value>) -> Result<()> {
    for reserved in ["store", "include"] {
        if extra_body.contains_key(reserved) {
            return Err(AgentCoreError::Provider(format!(
                "responses extra body cannot override reserved field: {reserved}"
            )));
        }
    }
    Ok(())
}

fn should_include_encrypted_reasoning(config: &ResponsesApiRequestRenderConfig) -> bool {
    config.include_encrypted_reasoning
        || (config.state_mode.is_stateless() && config.reasoning.is_some())
}

fn reasoning_to_value(reasoning: &ResponsesReasoningConfig) -> Value {
    let mut object = Map::new();
    if let Some(effort) = &reasoning.effort {
        object.insert("effort".into(), Value::String(effort.as_str().into()));
    }
    if let Some(summary) = &reasoning.summary {
        object.insert("summary".into(), Value::String(summary.as_str().into()));
    }
    Value::Object(object)
}

struct RenderedResponsesMessages {
    input: Vec<Value>,
    instructions: Option<String>,
}

fn render_responses_messages(
    config: &ResponsesApiRequestRenderConfig,
    tool_names: &ProviderToolNameCodec,
    messages: &[AgentMessage],
) -> Result<RenderedResponsesMessages> {
    let mut input = Vec::new();
    let mut instruction_parts = Vec::new();
    for message in messages {
        if let Some(items) =
            render_responses_provider_payload_items(config.state_mode, &message.content)?
        {
            input.extend(items);
            continue;
        }
        match &message.role {
            MessageRole::System => {
                instruction_parts.extend(render_text_only_blocks(
                    &message.content,
                    "media blocks cannot be rendered as responses instructions",
                    "tool blocks cannot be rendered as responses instructions",
                )?);
            }
            MessageRole::User => input.push(json!({
                "type": "message",
                "role": "user",
                "content": render_user_content(config, &message.content)?,
            })),
            MessageRole::Assistant => {
                input.extend(render_assistant_items(config, tool_names, message)?);
            }
            MessageRole::ToolResult => {
                input.extend(render_tool_result_items(&message.content)?);
            }
            MessageRole::Custom(role) => {
                return Err(AgentCoreError::Provider(format!(
                    "custom role cannot be rendered for responses api: {role}"
                )));
            }
        }
    }
    Ok(RenderedResponsesMessages {
        input,
        instructions: (!instruction_parts.is_empty()).then_some(instruction_parts.join("\n")),
    })
}

fn render_responses_provider_payload_items(
    state_mode: ResponsesStateMode,
    content: &[ContentBlock],
) -> Result<Option<Vec<Value>>> {
    let payload_count = content
        .iter()
        .filter(|block| matches!(block, ContentBlock::ProviderPayload { .. }))
        .count();
    if payload_count == 0 {
        return Ok(None);
    }
    if payload_count != content.len() {
        return Err(AgentCoreError::Provider(
            "responses provider payload blocks cannot be mixed with normal content".into(),
        ));
    }

    let mut items = Vec::with_capacity(payload_count);
    for block in content {
        let ContentBlock::ProviderPayload {
            provider,
            kind,
            value,
        } = block
        else {
            continue;
        };
        if provider != RESPONSES_PROVIDER_PAYLOAD || kind != RESPONSES_RESPONSE_ITEM_PAYLOAD {
            return Err(AgentCoreError::Provider(format!(
                "unsupported responses provider payload: {provider}/{kind}"
            )));
        }
        let Some(item) = normalize_responses_replay_item(
            value.clone(),
            state_mode,
            ResponsesReplayItemSource::RequestHistory,
        )?
        else {
            continue;
        };
        items.push(item);
    }
    Ok(Some(items))
}

pub fn normalize_responses_replay_item(
    item: Value,
    state_mode: ResponsesStateMode,
    source: ResponsesReplayItemSource,
) -> Result<Option<Value>> {
    if matches!(state_mode, ResponsesStateMode::Stateful) {
        return normalize_stateful_responses_replay_item(item).map(Some);
    }

    let Some(item_type) = item.get("type").and_then(Value::as_str) else {
        return unsafe_responses_replay_item(source, "responses replay item is missing type");
    };
    match item_type {
        "message" => Ok(Some(strip_response_item_id(item))),
        "reasoning" => {
            if source == ResponsesReplayItemSource::ThinkingHistory
                && !has_non_empty_string(&item, "encrypted_content")
            {
                return Ok(None);
            }
            if is_stateless_reasoning_replayable(&item) {
                Ok(Some(ensure_reasoning_summary_array(
                    strip_response_item_id(item),
                )?))
            } else {
                unsafe_responses_replay_item(
                    source,
                    "stateless responses reasoning replay requires encrypted_content, non-empty summary, or non-empty content",
                )
            }
        }
        "compaction" | "context_compaction" => {
            if has_non_empty_string(&item, "encrypted_content") {
                Ok(Some(strip_response_item_id(item)))
            } else {
                unsafe_responses_replay_item(
                    source,
                    "stateless responses compaction replay requires encrypted_content",
                )
            }
        }
        _ => unsafe_responses_replay_item(
            source,
            &format!("stateless responses replay item is not safe to replay: {item_type}"),
        ),
    }
}

fn normalize_stateful_responses_replay_item(item: Value) -> Result<Value> {
    if item.get("type").and_then(Value::as_str) == Some("reasoning") {
        ensure_reasoning_summary_array(item)
    } else {
        Ok(item)
    }
}

fn ensure_reasoning_summary_array(mut item: Value) -> Result<Value> {
    if let Value::Object(object) = &mut item {
        match object.get("summary") {
            Some(Value::Array(_)) => {}
            Some(_) => {
                return Err(AgentCoreError::Provider(
                    "responses reasoning replay summary must be an array".into(),
                ));
            }
            None => {
                object.insert("summary".into(), Value::Array(Vec::new()));
            }
        }
    }
    Ok(item)
}

fn strip_response_item_id(mut item: Value) -> Value {
    if let Value::Object(object) = &mut item {
        object.remove("id");
    }
    item
}

fn has_non_empty_string(item: &Value, key: &str) -> bool {
    item.get(key)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
}

fn has_non_empty_array(item: &Value, key: &str) -> bool {
    item.get(key)
        .and_then(Value::as_array)
        .is_some_and(|value| !value.is_empty())
}

fn is_stateless_reasoning_replayable(item: &Value) -> bool {
    has_non_empty_string(item, "encrypted_content")
        || has_non_empty_array(item, "summary")
        || has_non_empty_array(item, "content")
}

fn unsafe_responses_replay_item(
    source: ResponsesReplayItemSource,
    message: &str,
) -> Result<Option<Value>> {
    match source {
        ResponsesReplayItemSource::RequestHistory => {
            Err(AgentCoreError::Provider(message.to_string()))
        }
        ResponsesReplayItemSource::ThinkingHistory => Ok(None),
        ResponsesReplayItemSource::CompactOutput => Ok(None),
    }
}

fn render_user_content(
    config: &ResponsesApiRequestRenderConfig,
    content: &[ContentBlock],
) -> Result<Vec<Value>> {
    let mut parts = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => parts.push(input_text_part(text.clone())),
            ContentBlock::Json { value } => parts.push(input_text_part(value.to_string())),
            ContentBlock::Thinking { thinking } => {
                if let Some(text) = &thinking.text {
                    parts.push(input_text_part(text.clone()));
                }
            }
            ContentBlock::Media { media } => {
                parts.push(media_to_responses_input_part(config, media)?)
            }
            ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => {
                return Err(AgentCoreError::Provider(
                    "tool blocks cannot be rendered as responses user content".into(),
                ));
            }
            ContentBlock::ProviderPayload { .. } => {
                return Err(AgentCoreError::Provider(
                    "provider payload blocks must occupy an entire responses message".into(),
                ));
            }
        }
    }
    Ok(parts)
}

fn render_assistant_items(
    config: &ResponsesApiRequestRenderConfig,
    tool_names: &ProviderToolNameCodec,
    message: &AgentMessage,
) -> Result<Vec<Value>> {
    let mut items = Vec::new();
    let mut text_parts = Vec::new();
    for block in &message.content {
        match block {
            ContentBlock::Text { text } => text_parts.push(output_text_part(text.clone())),
            ContentBlock::Json { value } => text_parts.push(output_text_part(value.to_string())),
            ContentBlock::Thinking { thinking } => {
                flush_assistant_text_message(&mut items, &mut text_parts);
                if let Some(item) = replay_reasoning(config, thinking)? {
                    items.push(item);
                }
            }
            ContentBlock::ToolCall { tool_call } => {
                flush_assistant_text_message(&mut items, &mut text_parts);
                items.push(function_call_item(config, tool_names, tool_call)?);
            }
            ContentBlock::Media { .. } => {
                return Err(AgentCoreError::Provider(
                    "assistant media blocks cannot be rendered for responses api v1".into(),
                ));
            }
            ContentBlock::ToolResult { .. } => {
                return Err(AgentCoreError::Provider(
                    "tool result blocks cannot be rendered as responses assistant content".into(),
                ));
            }
            ContentBlock::ProviderPayload { .. } => {
                return Err(AgentCoreError::Provider(
                    "provider payload blocks must occupy an entire responses message".into(),
                ));
            }
        }
    }
    flush_assistant_text_message(&mut items, &mut text_parts);
    Ok(items)
}

fn flush_assistant_text_message(items: &mut Vec<Value>, text_parts: &mut Vec<Value>) {
    if text_parts.is_empty() {
        return;
    }
    items.push(json!({
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": std::mem::take(text_parts),
    }));
}

fn render_tool_result_items(content: &[ContentBlock]) -> Result<Vec<Value>> {
    let mut items = Vec::new();
    for block in content {
        match block {
            ContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let mut item = Map::new();
                item.insert("type".into(), Value::String("function_call_output".into()));
                item.insert("call_id".into(), Value::String(tool_call_id.clone()));
                item.insert(
                    "output".into(),
                    Value::String(render_tool_result_output(content)?),
                );
                if *is_error {
                    item.insert("status".into(), Value::String("failed".into()));
                }
                items.push(Value::Object(item));
            }
            _ => {
                return Err(AgentCoreError::Provider(
                    "only tool result blocks can be rendered as responses tool result messages"
                        .into(),
                ));
            }
        }
    }
    Ok(items)
}

fn render_tool_result_output(content: &[ContentBlock]) -> Result<String> {
    Ok(render_text_only_blocks(
        content,
        "media blocks cannot be rendered as responses tool results",
        "nested tool blocks cannot be rendered as responses tool results",
    )?
    .join("\n"))
}

fn render_text_only_blocks(
    content: &[ContentBlock],
    media_error: &'static str,
    tool_error: &'static str,
) -> Result<Vec<String>> {
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
            ContentBlock::Media { .. } => return Err(AgentCoreError::Provider(media_error.into())),
            ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => {
                return Err(AgentCoreError::Provider(tool_error.into()));
            }
            ContentBlock::ProviderPayload { .. } => {
                return Err(AgentCoreError::Provider(
                    "provider payload blocks cannot be rendered as plain responses text".into(),
                ));
            }
        }
    }
    Ok(parts)
}

fn input_text_part(text: String) -> Value {
    json!({ "type": "input_text", "text": text })
}

fn output_text_part(text: String) -> Value {
    json!({ "type": "output_text", "text": text, "annotations": [] })
}

fn function_call_item(
    config: &ResponsesApiRequestRenderConfig,
    tool_names: &ProviderToolNameCodec,
    tool_call: &ToolCall,
) -> Result<Value> {
    Ok(json!({
        "type": "function_call",
        "call_id": tool_call.id,
        "name": tool_names.provider_name(&tool_call.name, &config.provider_id, &config.model)?,
        "arguments": tool_call.arguments.to_string(),
        "status": "completed",
    }))
}

fn media_to_responses_input_part(
    config: &ResponsesApiRequestRenderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    match &media.kind {
        MediaKind::Image => image_to_responses_part(config, media),
        MediaKind::File => file_to_responses_part(config, media),
        MediaKind::Audio => Err(AgentCoreError::Provider(
            "audio media is not supported by the built-in responses api provider v1".into(),
        )),
        MediaKind::Video => Err(AgentCoreError::Provider(
            "video media is not supported by the built-in responses api provider v1".into(),
        )),
        MediaKind::Custom(kind) => Err(AgentCoreError::Provider(format!(
            "custom media kind cannot be rendered by responses api: {kind}"
        ))),
    }
}

fn image_to_responses_part(
    config: &ResponsesApiRequestRenderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    let mut part = Map::new();
    part.insert("type".into(), Value::String("input_image".into()));
    match &media.source {
        MediaSource::Uri { uri } => {
            part.insert("image_url".into(), Value::String(uri.clone()));
        }
        MediaSource::Inline {
            data,
            encoding: MediaEncoding::Base64,
        } => {
            part.insert(
                "image_url".into(),
                Value::String(data_url(media, data, "image")?),
            );
        }
        MediaSource::Inline { .. } => {
            return Err(AgentCoreError::Provider(
                "inline image media must use base64 encoding".into(),
            ));
        }
        MediaSource::Provider { provider_id, id } => {
            ensure_provider_scope(config, provider_id, "image")?;
            part.insert("file_id".into(), Value::String(id.clone()));
        }
    }
    Ok(Value::Object(part))
}

fn file_to_responses_part(
    config: &ResponsesApiRequestRenderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    let mut part = Map::new();
    part.insert("type".into(), Value::String("input_file".into()));
    match &media.source {
        MediaSource::Uri { uri } => {
            part.insert("file_url".into(), Value::String(uri.clone()));
        }
        MediaSource::Provider { provider_id, id } => {
            ensure_provider_scope(config, provider_id, "file")?;
            part.insert("file_id".into(), Value::String(id.clone()));
        }
        MediaSource::Inline {
            data,
            encoding: MediaEncoding::Base64,
        } => {
            if !config.allow_file_data_url_input {
                return Err(AgentCoreError::Provider(
                    "inline file media requires allow_file_data_url_input(true) for responses api"
                        .into(),
                ));
            }
            part.insert(
                "file_data".into(),
                Value::String(data_url(media, data, "file")?),
            );
        }
        MediaSource::Inline { .. } => {
            return Err(AgentCoreError::Provider(
                "inline file media must use base64 encoding".into(),
            ));
        }
    }
    if let Some(name) = &media.name {
        part.insert("filename".into(), Value::String(name.clone()));
    }
    Ok(Value::Object(part))
}

fn ensure_provider_scope(
    config: &ResponsesApiRequestRenderConfig,
    provider_id: &str,
    label: &str,
) -> Result<()> {
    if provider_id == config.provider_id.as_str() {
        Ok(())
    } else {
        Err(AgentCoreError::Provider(format!(
            "provider {label} media source does not match the responses provider id"
        )))
    }
}

fn data_url(media: &MediaBlock, data: &str, label: &str) -> Result<String> {
    let mime_type = media.mime_type.as_deref().ok_or_else(|| {
        AgentCoreError::Provider(format!("inline {label} media requires mime_type"))
    })?;
    Ok(format!("data:{mime_type};base64,{data}"))
}

fn to_responses_tools(
    config: &ResponsesApiRequestRenderConfig,
    tool_names: &ProviderToolNameCodec,
    tools: &[ToolSpec],
) -> Result<Vec<Value>> {
    let mut rendered = tools
        .iter()
        .map(|tool| to_responses_function_tool(config, tool_names, tool))
        .collect::<Result<Vec<_>>>()?;
    rendered.extend(config.native_tools.clone());
    Ok(rendered)
}

fn to_responses_function_tool(
    config: &ResponsesApiRequestRenderConfig,
    tool_names: &ProviderToolNameCodec,
    tool: &ToolSpec,
) -> Result<Value> {
    let mut value = json!({
        "type": "function",
        "name": tool_names.provider_name(&tool.name, &config.provider_id, &config.model)?,
        "description": tool.description,
        "parameters": tool.input_schema,
    });
    if let Some(strict) = config.function_tool_strict
        && let Value::Object(object) = &mut value
    {
        object.insert("strict".into(), Value::Bool(strict));
    }
    Ok(value)
}

fn replay_reasoning(
    config: &ResponsesApiRequestRenderConfig,
    thinking: &ThinkingBlock,
) -> Result<Option<Value>> {
    let descriptor = serde_json::from_value::<ResponsesReasoningReplayDescriptor>(
        match thinking.replay_descriptor.as_ref() {
            Some(descriptor) => descriptor.clone(),
            None => return Ok(None),
        },
    )
    .ok();
    let Some(descriptor) = descriptor else {
        return Ok(None);
    };
    match replay_scope_match(
        descriptor.v,
        &descriptor.kind,
        RESPONSES_REASONING_REPLAY_KIND,
        &descriptor.provider_id,
        &config.provider_id,
        &descriptor.model,
        &config.model,
    ) {
        ReplayScopeMatch::Match => {}
        ReplayScopeMatch::Ignore | ReplayScopeMatch::Unsupported => return Ok(None),
    }
    let Some(raw) = thinking.raw.as_ref() else {
        return Ok(None);
    };
    if config.state_mode == ResponsesStateMode::Stateless
        && !has_non_empty_string(raw, "encrypted_content")
    {
        return Ok(None);
    }
    normalize_responses_replay_item(
        raw.clone(),
        config.state_mode,
        ResponsesReplayItemSource::ThinkingHistory,
    )
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponsesReasoningReplayDescriptor {
    v: u64,
    kind: String,
    provider_id: String,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    item_id: Option<String>,
}

#[derive(Default)]
struct ResponsesStreamState {
    provider_id: String,
    model: String,
    run_id: String,
    turn_id: u64,
    state_mode: ResponsesStateMode,
    tool_names: ProviderToolNameCodec,
    started: bool,
    tool_calls: BTreeMap<String, PartialResponsesToolCall>,
    tool_keys_by_output_index: BTreeMap<u64, String>,
    emitted_tool_calls: BTreeSet<String>,
    reasoning: BTreeMap<String, ResponsesReasoningState>,
    emitted_reasoning_items: BTreeSet<String>,
    stop_reason: Option<StopReason>,
    finished: bool,
    done: bool,
}

impl ResponsesStreamState {
    fn new(
        config: &ResponsesApiProviderConfig,
        request: &ModelRequest,
        tool_names: ProviderToolNameCodec,
    ) -> Self {
        Self {
            provider_id: config.id.clone(),
            model: config.model.clone(),
            run_id: request.run_id.clone(),
            turn_id: request.turn_id,
            state_mode: config.state_mode,
            tool_names,
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
            "response.created" => Ok(vec![self.started_event(&value)]),
            "response.output_text.delta" => self.output_text_delta(&value),
            "response.content_part.delta" => self.content_part_delta(&value),
            "response.output_item.added" => {
                self.absorb_output_item_added(&value);
                Ok(Vec::new())
            }
            "response.function_call_arguments.delta" => {
                self.absorb_function_arguments_delta(&value);
                Ok(Vec::new())
            }
            "response.function_call_arguments.done" => self.function_arguments_done(&value),
            "response.output_item.done" => self.output_item_done(&value),
            "response.reasoning_summary_text.delta" => {
                self.reasoning_text_delta(&value, ThinkingKind::Summary)
            }
            "response.reasoning_text.delta" => self.reasoning_text_delta(&value, ThinkingKind::Raw),
            "response.reasoning_summary_text.done" | "response.reasoning_text.done" => {
                self.reasoning_text_done(&value)
            }
            "response.completed" | "response.done" => self.response_completed(&value),
            "response.incomplete" => self.response_incomplete(&value),
            "response.failed" | "response.error" | "error" => Ok(vec![self.failed_event(&value)]),
            "response.in_progress"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.done" => Ok(Vec::new()),
            _ => Ok(Vec::new()),
        }
    }

    fn started_event(&mut self, value: &Value) -> ModelStreamEvent {
        self.started = true;
        let stream_id = response_id(value)
            .map(ToString::to_string)
            .unwrap_or_else(|| self.fallback_stream_id());
        ModelStreamEvent::Started { stream_id }
    }

    fn ensure_started(&mut self) -> Vec<ModelStreamEvent> {
        if self.started {
            Vec::new()
        } else {
            vec![self.started_event(&Value::Null)]
        }
    }

    fn output_text_delta(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let Some(text) = value.get("delta").and_then(Value::as_str) else {
            return Ok(Vec::new());
        };
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let mut events = self.ensure_started();
        events.push(ModelStreamEvent::TextDelta { text: text.into() });
        Ok(events)
    }

    fn content_part_delta(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let text = value
            .get("delta")
            .and_then(Value::as_str)
            .or_else(|| {
                value
                    .get("delta")
                    .and_then(Value::as_object)
                    .and_then(|delta| delta.get("text"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                value
                    .get("part")
                    .and_then(Value::as_object)
                    .and_then(|part| part.get("text"))
                    .and_then(Value::as_str)
            });
        let Some(text) = text else {
            return Ok(Vec::new());
        };
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let mut events = self.ensure_started();
        events.push(ModelStreamEvent::TextDelta { text: text.into() });
        Ok(events)
    }

    fn absorb_output_item_added(&mut self, value: &Value) {
        let Some(item) = value.get("item").and_then(Value::as_object) else {
            return;
        };
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            self.absorb_function_call_item(item, value.get("output_index").and_then(Value::as_u64));
        }
    }

    fn absorb_function_arguments_delta(&mut self, value: &Value) {
        let Some(delta) = value.get("delta").and_then(Value::as_str) else {
            return;
        };
        let key = self.tool_key_from_event(value);
        self.tool_calls
            .entry(key)
            .or_default()
            .arguments_json
            .push_str(delta);
    }

    fn function_arguments_done(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        if let Some(item) = value.get("item").and_then(Value::as_object) {
            self.absorb_function_call_item(item, value.get("output_index").and_then(Value::as_u64));
        } else {
            let key = self.tool_key_from_event(value);
            if let Some(arguments) = value.get("arguments").and_then(Value::as_str) {
                self.tool_calls.entry(key).or_default().arguments_json = arguments.into();
            }
        }
        self.emit_tool_call(value)
    }

    fn output_item_done(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let Some(item) = value.get("item").and_then(Value::as_object) else {
            return Ok(Vec::new());
        };
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                self.absorb_function_call_item(
                    item,
                    value.get("output_index").and_then(Value::as_u64),
                );
                self.emit_tool_call(value)
            }
            Some("reasoning") => {
                let mut events = self.ensure_started();
                events.extend(self.reasoning_item_events(item));
                Ok(events)
            }
            _ => Ok(Vec::new()),
        }
    }

    fn reasoning_text_delta(
        &mut self,
        value: &Value,
        kind: ThinkingKind,
    ) -> Result<Vec<ModelStreamEvent>> {
        let Some(text) = value.get("delta").and_then(Value::as_str) else {
            return Ok(Vec::new());
        };
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let key = reasoning_key_from_event(value);
        let state = self.reasoning.entry(key.clone()).or_default();
        state.kind = kind.clone();
        state.text.push_str(text);
        let mut events = self.ensure_started();
        events.push(self.thinking_delta(kind, Some(text.into()), None, Some(key), false));
        Ok(events)
    }

    fn reasoning_text_done(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let key = reasoning_key_from_event(value);
        let text = value.get("text").and_then(Value::as_str);
        if let Some(text) = text {
            self.reasoning.entry(key.clone()).or_default().text = text.into();
        }
        let Some(state) = self.reasoning.get(&key).cloned() else {
            return Ok(Vec::new());
        };
        let raw_snapshot = if state.kind == ThinkingKind::Summary {
            json!({
                "type": "reasoning",
                "id": key.clone(),
                "summary": [{
                    "type": "summary_text",
                    "text": state.text,
                }],
            })
        } else {
            json!({
                "type": "reasoning",
                "id": key.clone(),
                "summary": [],
                "content": [{
                    "type": "reasoning_text",
                    "text": state.text,
                }],
            })
        };
        let mut events = self.ensure_started();
        events.push(self.thinking_delta(state.kind, None, Some(raw_snapshot), Some(key), false));
        Ok(events)
    }

    fn response_completed(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let mut events = self.ensure_started();
        if let Some(output) = value
            .get("response")
            .and_then(Value::as_object)
            .and_then(|response| response.get("output"))
            .and_then(Value::as_array)
        {
            events.extend(self.output_item_events(output)?);
        }
        self.done = true;
        events.extend(self.finish_events());
        Ok(events)
    }

    fn response_incomplete(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        self.stop_reason = Some(map_incomplete_stop_reason(value));
        self.response_completed(value)
    }

    fn failed_event(&mut self, value: &Value) -> ModelStreamEvent {
        self.done = true;
        self.finished = true;
        let error = value
            .get("error")
            .and_then(|error| {
                error
                    .as_object()
                    .and_then(|object| object.get("message"))
                    .and_then(Value::as_str)
                    .or_else(|| error.as_str())
            })
            .or_else(|| value.get("message").and_then(Value::as_str))
            .unwrap_or("responses api stream error");
        ModelStreamEvent::Failed {
            error: error.into(),
        }
    }

    fn output_item_events(&mut self, output: &[Value]) -> Result<Vec<ModelStreamEvent>> {
        let mut events = Vec::new();
        for item in output.iter().filter_map(Value::as_object) {
            match item.get("type").and_then(Value::as_str) {
                Some("function_call") => {
                    let key = tool_key_from_item(item);
                    if !self.emitted_tool_calls.contains(&key) {
                        self.absorb_function_call_item(item, None);
                        events.extend(self.emit_tool_call_from_key(key)?);
                    }
                }
                Some("reasoning") => events.extend(self.reasoning_item_events(item)),
                _ => {}
            }
        }
        Ok(events)
    }

    fn reasoning_item_events(&mut self, item: &Map<String, Value>) -> Vec<ModelStreamEvent> {
        let (identity, item_id) = reasoning_item_identity(item);
        if !self.emitted_reasoning_items.insert(identity) {
            return Vec::new();
        }
        let raw_snapshot = Value::Object(item.clone());
        let kind = if item.get("encrypted_content").is_some() {
            ThinkingKind::Encrypted
        } else if item
            .get("summary")
            .and_then(Value::as_array)
            .is_some_and(|summary| !summary.is_empty())
        {
            ThinkingKind::Summary
        } else {
            ThinkingKind::Raw
        };
        let replayable = self.is_replayable_reasoning_item(item);
        vec![self.thinking_delta(kind, None, Some(raw_snapshot), item_id, replayable)]
    }

    fn absorb_function_call_item(&mut self, item: &Map<String, Value>, output_index: Option<u64>) {
        let key = tool_key_from_item(item);
        let partial = self.tool_calls.entry(key.clone()).or_default();
        if let Some(id) = item.get("call_id").and_then(Value::as_str)
            && !id.is_empty()
        {
            partial.id = id.into();
        }
        if let Some(id) = item.get("id").and_then(Value::as_str)
            && partial.item_id.is_empty()
        {
            partial.item_id = id.into();
        }
        if let Some(name) = item.get("name").and_then(Value::as_str)
            && !name.is_empty()
        {
            partial.name = name.into();
        }
        if let Some(arguments) = item.get("arguments").and_then(Value::as_str) {
            partial.arguments_json = arguments.into();
        }
        if let Some(output_index) = output_index {
            self.tool_keys_by_output_index.insert(output_index, key);
        }
    }

    fn tool_key_from_event(&self, value: &Value) -> String {
        value
            .get("item_id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                value
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .and_then(|index| self.tool_keys_by_output_index.get(&index).cloned())
            })
            .or_else(|| {
                value
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .map(|index| format!("output-{index}"))
            })
            .unwrap_or_else(|| "output-0".into())
    }

    fn emit_tool_call(&mut self, value: &Value) -> Result<Vec<ModelStreamEvent>> {
        let key = self.tool_key_from_event(value);
        self.emit_tool_call_from_key(key)
    }

    fn emit_tool_call_from_key(&mut self, key: String) -> Result<Vec<ModelStreamEvent>> {
        if !self.emitted_tool_calls.insert(key.clone()) {
            return Ok(Vec::new());
        }
        let Some(tool_call) = self.tool_calls.get(&key) else {
            return Ok(Vec::new());
        };
        let event = tool_call.to_event(&key, &self.tool_names, &self.provider_id, &self.model)?;
        let mut events = self.ensure_started();
        events.push(event);
        Ok(events)
    }

    fn thinking_delta(
        &self,
        kind: ThinkingKind,
        text_delta: Option<String>,
        raw_snapshot: Option<Value>,
        item_id: Option<String>,
        replayable: bool,
    ) -> ModelStreamEvent {
        let replay_descriptor = replayable
            .then(|| {
                serde_json::to_value(ResponsesReasoningReplayDescriptor {
                    v: 1,
                    kind: RESPONSES_REASONING_REPLAY_KIND.into(),
                    provider_id: self.provider_id.clone(),
                    model: self.model.clone(),
                    item_id: item_id.clone(),
                })
                .ok()
            })
            .flatten();
        let mut metadata = Map::new();
        if let Some(item_id) = item_id {
            metadata.insert("itemId".into(), Value::String(item_id));
        }
        ModelStreamEvent::ThinkingDelta {
            delta: ThinkingDelta {
                kind,
                text_delta,
                raw_snapshot,
                replay_descriptor,
                metadata,
            },
        }
    }

    fn is_replayable_reasoning_item(&self, item: &Map<String, Value>) -> bool {
        match self.state_mode {
            ResponsesStateMode::Stateless => item
                .get("encrypted_content")
                .and_then(Value::as_str)
                .is_some_and(|content| !content.is_empty()),
            ResponsesStateMode::Stateful => item
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty()),
        }
    }

    fn finish_events(&mut self) -> Vec<ModelStreamEvent> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        let mut events = self.ensure_started();
        events.push(ModelStreamEvent::Finished {
            stop_reason: self.stop_reason.clone().unwrap_or(StopReason::Stop),
        });
        events
    }

    fn fallback_stream_id(&self) -> String {
        format!("responses-api-{}-{}", self.run_id, self.turn_id)
    }
}

#[derive(Clone, Default)]
struct ResponsesReasoningState {
    kind: ThinkingKind,
    text: String,
}

#[derive(Default)]
struct PartialResponsesToolCall {
    id: String,
    item_id: String,
    name: String,
    arguments_json: String,
}

impl PartialResponsesToolCall {
    fn to_event(
        &self,
        key: &str,
        tool_names: &ProviderToolNameCodec,
        provider_id: &str,
        model: &str,
    ) -> Result<ModelStreamEvent> {
        let arguments = parse_tool_arguments(&self.arguments_json);
        Ok(ModelStreamEvent::ToolCall {
            tool_call: ToolCall {
                id: if self.id.is_empty() {
                    key.into()
                } else {
                    self.id.clone()
                },
                name: tool_names.canonical_name(&self.name, provider_id, model)?,
                arguments,
            },
        })
    }
}

fn response_id(value: &Value) -> Option<&str> {
    value
        .get("response")
        .and_then(Value::as_object)
        .and_then(|response| response.get("id"))
        .and_then(Value::as_str)
}

fn tool_key_from_item(item: &Map<String, Value>) -> String {
    item.get("id")
        .and_then(Value::as_str)
        .or_else(|| item.get("call_id").and_then(Value::as_str))
        .unwrap_or("output-0")
        .into()
}

fn reasoning_key_from_event(value: &Value) -> String {
    value
        .get("item_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            value
                .get("output_index")
                .and_then(Value::as_u64)
                .map(|index| format!("reasoning-{index}"))
        })
        .unwrap_or_else(|| "reasoning-0".into())
}

fn reasoning_item_identity(item: &Map<String, Value>) -> (String, Option<String>) {
    if let Some(id) = item.get("id").and_then(Value::as_str) {
        return (id.into(), Some(id.into()));
    }
    (
        serde_json::to_string(&Value::Object(item.clone())).unwrap_or_else(|_| "reasoning".into()),
        None,
    )
}

fn map_incomplete_stop_reason(value: &Value) -> StopReason {
    let reason = value
        .get("response")
        .and_then(Value::as_object)
        .and_then(|response| response.get("incomplete_details"))
        .and_then(Value::as_object)
        .and_then(|details| details.get("reason"))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("incomplete_details")
                .and_then(Value::as_object)
                .and_then(|details| details.get("reason"))
                .and_then(Value::as_str)
        });
    match reason {
        Some("max_output_tokens" | "max_tokens") => StopReason::Length,
        _ => StopReason::Error,
    }
}

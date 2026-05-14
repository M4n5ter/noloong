use crate::provider_utils::{
    LocalFileUriMediaMaterialization, ReplayScopeMatch, emit_model_stream_event, headers_from_map,
    materialize_local_file_uri_media_in_request, refresh_auth_provider, replay_scope_match,
    resolve_auth_headers,
};
use crate::sse::{SseFrameResult, SseReconnectConfig, SseStreamOptions, run_sse_model_stream};
use crate::tool_arguments::parse_tool_arguments;
use crate::tool_names::ProviderToolNameCodec;
use crate::{
    AgentCoreError, AgentMessage, CancellationToken, ContentBlock, HttpAuthContext, HttpAuthHeader,
    HttpAuthProvider, HttpAuthRefreshContext, MediaBlock, MediaDelta, MediaEncoding, MediaKind,
    MediaSource, MessageRole, ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink,
    Result, StopReason, ThinkingBlock, ThinkingDelta, ThinkingKind, ToolCall, ToolSpec,
};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    fmt::{Debug, Formatter},
    sync::Arc,
    time::Duration,
};

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";
const OPENAI_CHAT_REASONING_REPLAY_KIND: &str = "openai_chat_reasoning_replay";
const OPENAI_CHAT_MEDIA_REPLAY_KIND: &str = "openai_chat_media_replay";

#[derive(Clone)]
pub struct ChatCompletionsProviderConfig {
    pub id: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub auth_provider: Option<Arc<dyn HttpAuthProvider>>,
    pub headers: BTreeMap<String, String>,
    pub extra_body: Map<String, Value>,
    pub max_completion_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub stream_reconnect: SseReconnectConfig,
    pub include_usage: bool,
    pub image_detail: ChatImageDetail,
    pub allow_provider_video_file_media: bool,
    pub output_modalities: Vec<ChatOutputModality>,
    pub output_audio: Option<ChatOutputAudioConfig>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ChatImageDetail {
    #[default]
    Auto,
    Low,
    High,
    Custom(String),
}

impl ChatImageDetail {
    fn as_str(&self) -> &str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::High => "high",
            Self::Custom(detail) => detail,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatOutputModality {
    Text,
    Audio,
    Custom(String),
}

impl ChatOutputModality {
    fn as_str(&self) -> &str {
        match self {
            Self::Text => "text",
            Self::Audio => "audio",
            Self::Custom(modality) => modality,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatAudioFormat {
    Wav,
    Mp3,
    Flac,
    Opus,
    Pcm16,
    Custom(String),
}

impl ChatAudioFormat {
    fn as_str(&self) -> &str {
        match self {
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Flac => "flac",
            Self::Opus => "opus",
            Self::Pcm16 => "pcm16",
            Self::Custom(format) => format,
        }
    }

    fn from_wire(format: &str) -> Self {
        match format {
            "wav" => Self::Wav,
            "mp3" => Self::Mp3,
            "flac" => Self::Flac,
            "opus" => Self::Opus,
            "pcm16" => Self::Pcm16,
            _ => Self::Custom(format.into()),
        }
    }

    fn mime_type(&self) -> Option<&'static str> {
        match self {
            Self::Wav => Some("audio/wav"),
            Self::Mp3 => Some("audio/mpeg"),
            Self::Flac => Some("audio/flac"),
            Self::Opus => Some("audio/opus"),
            Self::Pcm16 => Some("audio/pcm"),
            Self::Custom(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatOutputAudioConfig {
    pub format: ChatAudioFormat,
    pub voice: String,
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
            .field(
                "auth_provider",
                &self.auth_provider.as_ref().map(|provider| provider.id()),
            )
            .field("headers", &self.headers)
            .field("extra_body", &self.extra_body)
            .field("max_completion_tokens", &self.max_completion_tokens)
            .field("temperature", &self.temperature)
            .field("request_timeout", &self.request_timeout)
            .field("stream_idle_timeout", &self.stream_idle_timeout)
            .field("stream_reconnect", &self.stream_reconnect)
            .field("include_usage", &self.include_usage)
            .field("image_detail", &self.image_detail)
            .field(
                "allow_provider_video_file_media",
                &self.allow_provider_video_file_media,
            )
            .field("output_modalities", &self.output_modalities)
            .field("output_audio", &self.output_audio)
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
            auth_provider: None,
            headers: BTreeMap::new(),
            extra_body: Map::new(),
            max_completion_tokens: None,
            temperature: None,
            request_timeout: Duration::from_secs(60),
            stream_idle_timeout: Duration::from_secs(300),
            stream_reconnect: SseReconnectConfig::default(),
            include_usage: true,
            image_detail: ChatImageDetail::default(),
            allow_provider_video_file_media: false,
            output_modalities: vec![ChatOutputModality::Text],
            output_audio: None,
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

    pub fn stream_reconnect(mut self, stream_reconnect: SseReconnectConfig) -> Self {
        self.stream_reconnect = stream_reconnect;
        self
    }

    pub fn include_usage(mut self, include_usage: bool) -> Self {
        self.include_usage = include_usage;
        self
    }

    pub fn image_detail(mut self, image_detail: ChatImageDetail) -> Self {
        self.image_detail = image_detail;
        self
    }

    pub fn allow_provider_video_file_media(
        mut self,
        allow_provider_video_file_media: bool,
    ) -> Self {
        self.allow_provider_video_file_media = allow_provider_video_file_media;
        self
    }

    pub fn output_modalities(
        mut self,
        output_modalities: impl IntoIterator<Item = ChatOutputModality>,
    ) -> Self {
        self.output_modalities = output_modalities.into_iter().collect();
        self
    }

    pub fn enable_audio_output(
        mut self,
        format: ChatAudioFormat,
        voice: impl Into<String>,
    ) -> Self {
        if !self
            .output_modalities
            .iter()
            .any(|modality| modality.as_str() == "audio")
        {
            self.output_modalities.push(ChatOutputModality::Audio);
        }
        self.output_audio = Some(ChatOutputAudioConfig {
            format,
            voice: voice.into(),
        });
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

    async fn request_headers(
        &self,
        url: &str,
        attempt: u32,
        refreshed_headers: Option<Vec<HttpAuthHeader>>,
        cancellation: CancellationToken,
    ) -> Result<HeaderMap> {
        let mut headers = headers_from_map(&self.config.headers)?;
        headers.extend(
            resolve_auth_headers(
                self.config.auth_provider.as_ref(),
                &self.config.api_key,
                &self.config.api_key_env,
                self.auth_context("POST", url, attempt),
                refreshed_headers,
                cancellation,
            )
            .await?,
        );
        Ok(headers)
    }

    fn auth_context(&self, method: &str, url: &str, attempt: u32) -> HttpAuthContext {
        HttpAuthContext::new(&self.config.id, method, url, attempt)
    }
}

impl ModelProvider for ChatCompletionsProvider {
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
            let request =
                materialize_local_file_uri_media_in_request(request, |media| match &media.kind {
                    MediaKind::Image | MediaKind::Audio | MediaKind::File | MediaKind::Video => {
                        LocalFileUriMediaMaterialization::Inline
                    }
                    MediaKind::Custom(_) => LocalFileUriMediaMaterialization::Leave,
                })
                .await?;
            let tool_names = ProviderToolNameCodec::new(&request.tools);
            let payload = build_chat_payload(&self.config, &tool_names, &request)?;
            let stream_id = format!("chat-completions-{}-{}", request.run_id, request.turn_id);

            let mut events = Vec::new();
            let mut state = ChatStreamState::new(&self.config, tool_names);
            let mut started = false;
            let endpoint = self.endpoint();
            let mut attempt = 0_u32;
            let mut refreshed_headers = None;
            loop {
                let headers = self
                    .request_headers(
                        &endpoint,
                        attempt,
                        refreshed_headers.take(),
                        cancellation.clone(),
                    )
                    .await?;
                let result = run_sse_model_stream(
                    SseStreamOptions {
                        provider_label: "chat completions",
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
                            .post(&endpoint)
                            .headers(headers.clone())
                            .json(&payload))
                    },
                    |data| {
                        let mut events = Vec::new();
                        if !started {
                            started = true;
                            events.push(ModelStreamEvent::Started {
                                stream_id: stream_id.clone(),
                            });
                        }
                        if data == "[DONE]" {
                            state.done = true;
                            return Ok(SseFrameResult::new(events, true));
                        }
                        events.extend(state.apply_chunk(data)?);
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
                            self.auth_context("POST", &endpoint, attempt),
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
            for event in state.finish_events()? {
                emit_model_stream_event(&stream, &mut events, event).await?;
            }
            Ok(events)
        })
    }
}

fn build_chat_payload(
    config: &ChatCompletionsProviderConfig,
    tool_names: &ProviderToolNameCodec,
    request: &ModelRequest,
) -> Result<Value> {
    let mut payload = Map::new();
    payload.insert("model".into(), Value::String(config.model.clone()));
    payload.insert(
        "messages".into(),
        Value::Array(to_chat_messages(config, tool_names, request)?),
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
    let output_modalities = effective_output_modalities(config)?;
    if output_modalities != ["text"] {
        payload.insert(
            "modalities".into(),
            Value::Array(
                output_modalities
                    .iter()
                    .map(|modality| Value::String((*modality).into()))
                    .collect(),
            ),
        );
    }
    if let Some(audio) = &config.output_audio {
        payload.insert(
            "audio".into(),
            json!({
                "format": audio.format.as_str(),
                "voice": audio.voice,
            }),
        );
    }
    if !request.tools.is_empty() {
        payload.insert(
            "tools".into(),
            Value::Array(to_chat_tools(config, tool_names, &request.tools)?),
        );
    }
    payload.extend(config.extra_body.clone());
    Ok(Value::Object(payload))
}

fn effective_output_modalities(config: &ChatCompletionsProviderConfig) -> Result<Vec<&str>> {
    let mut modalities = if config.output_modalities.is_empty() {
        vec!["text"]
    } else {
        config
            .output_modalities
            .iter()
            .map(ChatOutputModality::as_str)
            .collect::<Vec<_>>()
    };
    let has_audio = modalities.contains(&"audio");
    if config.output_audio.is_some() && !has_audio {
        modalities.push("audio");
    } else if config.output_audio.is_none() && has_audio {
        return Err(AgentCoreError::Provider(
            "audio output modality requires output audio config".into(),
        ));
    }
    Ok(modalities)
}

fn to_chat_messages(
    config: &ChatCompletionsProviderConfig,
    tool_names: &ProviderToolNameCodec,
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
                "content": render_user_content(config, &message.content)?
            })),
            MessageRole::Assistant => {
                messages.push(to_assistant_message(config, tool_names, message)?)
            }
            MessageRole::ToolResult => {
                messages.extend(to_tool_result_messages(config, tool_names, message)?);
            }
        }
    }
    Ok(messages)
}

fn to_assistant_message(
    config: &ChatCompletionsProviderConfig,
    tool_names: &ProviderToolNameCodec,
    message: &AgentMessage,
) -> Result<Value> {
    let mut rendered = Map::new();
    let mut text_parts: Vec<String> = Vec::new();
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
            ContentBlock::Media { media } => match replay_media(config, media) {
                MediaReplay::RenderAudio(value) => {
                    rendered.entry("audio").or_insert(value);
                }
                MediaReplay::Ignore => {}
                MediaReplay::Unsupported => {
                    return Err(AgentCoreError::Provider(
                            "assistant media blocks cannot be rendered for chat completions without a matching replay descriptor".into(),
                        ));
                }
            },
            ContentBlock::ToolCall { tool_call } => {
                tool_calls.push(json!({
                    "id": tool_call.id,
                    "type": "function",
                    "function": {
                        "name": tool_names.provider_name(&tool_call.name, &config.id, &config.model)?,
                        "arguments": tool_call.arguments.to_string()
                    }
                }));
            }
            ContentBlock::ToolResult { .. } => {}
            ContentBlock::ProviderPayload { .. } => {
                return Err(AgentCoreError::Provider(
                    "provider payload blocks cannot be rendered for chat completions".into(),
                ));
            }
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

fn render_user_content(
    config: &ChatCompletionsProviderConfig,
    content: &[ContentBlock],
) -> Result<Value> {
    let mut text_parts: Vec<String> = Vec::new();
    let mut content_parts: Option<Vec<Value>> = None;
    for block in content {
        match block {
            ContentBlock::Media { media } => {
                let parts = content_parts.get_or_insert_with(|| {
                    text_parts
                        .drain(..)
                        .filter(|text| !text.is_empty())
                        .map(chat_text_part)
                        .collect()
                });
                parts.push(media_to_chat_content_part(config, media)?);
            }
            block => {
                if let Some(text) = render_text_block(block)? {
                    if let Some(parts) = &mut content_parts {
                        if !text.is_empty() {
                            parts.push(chat_text_part(text));
                        }
                    } else {
                        text_parts.push(text);
                    }
                }
            }
        }
    }
    if let Some(parts) = content_parts {
        Ok(Value::Array(parts))
    } else {
        Ok(Value::String(text_parts.join("\n")))
    }
}

fn chat_text_part(text: String) -> Value {
    json!({ "type": "text", "text": text })
}

fn media_to_chat_content_part(
    config: &ChatCompletionsProviderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    match &media.kind {
        MediaKind::Image => image_to_chat_content_part(config, media),
        MediaKind::Audio => audio_to_chat_content_part(media),
        MediaKind::File => file_to_chat_content_part(config, media),
        MediaKind::Video => video_to_chat_content_part(config, media),
        MediaKind::Custom(_) => Err(AgentCoreError::Provider(
            "custom media kinds cannot be rendered by the built-in chat completions provider"
                .into(),
        )),
    }
}

fn image_to_chat_content_part(
    config: &ChatCompletionsProviderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    let url = match &media.source {
        MediaSource::Uri { uri } => uri.clone(),
        MediaSource::Inline {
            data,
            encoding: MediaEncoding::Base64,
        } => {
            let mime_type = media.mime_type.as_deref().ok_or_else(|| {
                AgentCoreError::Provider("inline image media requires mime_type".into())
            })?;
            format!("data:{mime_type};base64,{data}")
        }
        MediaSource::Inline { .. } => {
            return Err(AgentCoreError::Provider(
                "inline image media must use base64 encoding".into(),
            ));
        }
        MediaSource::Provider { .. } => {
            return Err(AgentCoreError::Provider(
                "provider-referenced image media cannot be rendered as chat completions image_url"
                    .into(),
            ));
        }
    };
    Ok(json!({
        "type": "image_url",
        "image_url": {
            "url": url,
            "detail": config.image_detail.as_str()
        }
    }))
}

fn audio_to_chat_content_part(media: &MediaBlock) -> Result<Value> {
    let MediaSource::Inline {
        data,
        encoding: MediaEncoding::Base64,
    } = &media.source
    else {
        return Err(AgentCoreError::Provider(
            "chat completions audio input requires inline base64 media".into(),
        ));
    };
    let format = input_audio_format(media)?;
    Ok(json!({
        "type": "input_audio",
        "input_audio": {
            "data": data,
            "format": format
        }
    }))
}

fn video_to_chat_content_part(
    config: &ChatCompletionsProviderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    let url = match &media.source {
        MediaSource::Uri { uri } => uri.clone(),
        MediaSource::Inline {
            data,
            encoding: MediaEncoding::Base64,
        } => {
            let mime_type = media.mime_type.as_deref().ok_or_else(|| {
                AgentCoreError::Provider("inline video media requires mime_type".into())
            })?;
            format!("data:{mime_type};base64,{data}")
        }
        MediaSource::Inline { .. } => {
            return Err(AgentCoreError::Provider(
                "inline video media must use base64 encoding".into(),
            ));
        }
        MediaSource::Provider { .. } => {
            if config.allow_provider_video_file_media {
                return provider_file_to_chat_content_part(config, media);
            }
            return Err(AgentCoreError::Provider(
                "provider video media requires allow_provider_video_file_media(true)".into(),
            ));
        }
    };
    Ok(json!({
        "type": "video_url",
        "video_url": {
            "url": url
        }
    }))
}

fn input_audio_format(media: &MediaBlock) -> Result<&'static str> {
    match media.mime_type.as_deref() {
        Some("audio/wav" | "audio/x-wav") => Ok("wav"),
        Some("audio/mpeg" | "audio/mp3") => Ok("mp3"),
        Some(mime_type) => Err(AgentCoreError::Provider(format!(
            "unsupported chat completions input audio MIME type: {mime_type}"
        ))),
        None => Err(AgentCoreError::Provider(
            "chat completions input audio requires mime_type".into(),
        )),
    }
}

fn file_to_chat_content_part(
    config: &ChatCompletionsProviderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    match &media.source {
        MediaSource::Provider { .. } => provider_file_to_chat_content_part(config, media),
        MediaSource::Inline {
            data,
            encoding: MediaEncoding::Base64,
        } => {
            let mut file = Map::new();
            file.insert("file_data".into(), Value::String(data.clone()));
            if let Some(name) = &media.name {
                file.insert("filename".into(), Value::String(name.clone()));
            }
            Ok(json!({
                "type": "file",
                "file": file
            }))
        }
        MediaSource::Inline { .. } => Err(AgentCoreError::Provider(
            "inline file media must use base64 encoding".into(),
        )),
        MediaSource::Uri { .. } => Err(AgentCoreError::Provider(
            "chat completions file input does not support URI media".into(),
        )),
    }
}

fn provider_file_to_chat_content_part(
    config: &ChatCompletionsProviderConfig,
    media: &MediaBlock,
) -> Result<Value> {
    let MediaSource::Provider { provider_id, id } = &media.source else {
        return Err(AgentCoreError::Provider(
            "provider file media requires a provider source".into(),
        ));
    };
    if provider_id != &config.id {
        return Err(AgentCoreError::Provider(
            "provider media source does not match the chat completions provider id".into(),
        ));
    }
    Ok(json!({
        "type": "file",
        "file": {
            "file_id": id
        }
    }))
}

fn to_tool_result_messages(
    config: &ChatCompletionsProviderConfig,
    tool_names: &ProviderToolNameCodec,
    message: &AgentMessage,
) -> Result<Vec<Value>> {
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
                "name": tool_names.provider_name(tool_name, &config.id, &config.model)?,
                "content": render_content_text(content)?
            }));
        }
    }
    Ok(messages)
}

fn render_content_text(content: &[ContentBlock]) -> Result<String> {
    let mut parts = Vec::new();
    for block in content {
        if let Some(text) = render_text_block(block)? {
            parts.push(text);
        }
    }
    Ok(parts.join("\n"))
}

fn render_text_block(block: &ContentBlock) -> Result<Option<String>> {
    match block {
        ContentBlock::Text { text } => Ok(Some(text.clone())),
        ContentBlock::Json { value } => Ok(Some(value.to_string())),
        ContentBlock::Thinking { thinking } => Ok(thinking.text.clone()),
        ContentBlock::Media { .. } => Err(AgentCoreError::Provider(
            "media blocks cannot be rendered as chat message text".into(),
        )),
        ContentBlock::ToolCall { .. } | ContentBlock::ToolResult { .. } => Err(
            AgentCoreError::Provider("tool blocks cannot be rendered as chat message text".into()),
        ),
        ContentBlock::ProviderPayload { .. } => Err(AgentCoreError::Provider(
            "provider payload blocks cannot be rendered as chat message text".into(),
        )),
    }
}

fn to_chat_tools(
    config: &ChatCompletionsProviderConfig,
    tool_names: &ProviderToolNameCodec,
    tools: &[ToolSpec],
) -> Result<Vec<Value>> {
    tools
        .iter()
        .map(|tool| {
            Ok(json!({
                "type": "function",
                "function": {
                    "name": tool_names.provider_name(&tool.name, &config.id, &config.model)?,
                    "description": tool.description,
                    "parameters": tool.input_schema
                }
            }))
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
    match replay_scope_match(
        descriptor.v,
        &descriptor.kind,
        OPENAI_CHAT_REASONING_REPLAY_KIND,
        &descriptor.provider_id,
        &config.id,
        &descriptor.model,
        &config.model,
    ) {
        ReplayScopeMatch::Match => {}
        ReplayScopeMatch::Ignore | ReplayScopeMatch::Unsupported => return None,
    }
    Some((descriptor.field.wire_name().into(), thinking.raw.clone()?))
}

enum MediaReplay {
    RenderAudio(Value),
    Ignore,
    Unsupported,
}

fn replay_media(config: &ChatCompletionsProviderConfig, media: &MediaBlock) -> MediaReplay {
    let Some(replay_descriptor) = &media.replay_descriptor else {
        return MediaReplay::Unsupported;
    };
    let Ok(descriptor) =
        serde_json::from_value::<ChatMediaReplayDescriptor>(replay_descriptor.clone())
    else {
        return MediaReplay::Unsupported;
    };
    match replay_scope_match(
        descriptor.v,
        &descriptor.kind,
        OPENAI_CHAT_MEDIA_REPLAY_KIND,
        &descriptor.provider_id,
        &config.id,
        &descriptor.model,
        &config.model,
    ) {
        ReplayScopeMatch::Match => {}
        ReplayScopeMatch::Ignore => return MediaReplay::Ignore,
        ReplayScopeMatch::Unsupported => return MediaReplay::Unsupported,
    }
    if descriptor.field != MediaReplayField::Audio || !matches!(&media.kind, MediaKind::Audio) {
        return MediaReplay::Unsupported;
    }
    let MediaSource::Provider { provider_id, id } = &media.source else {
        return MediaReplay::Unsupported;
    };
    if provider_id != &config.id {
        return MediaReplay::Unsupported;
    }
    MediaReplay::RenderAudio(json!({ "id": id }))
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatMediaReplayDescriptor {
    v: u64,
    kind: String,
    provider_id: String,
    model: String,
    field: MediaReplayField,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum MediaReplayField {
    Audio,
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
struct ChatStreamState {
    replay_scope: ReasoningReplayScope,
    tool_names: ProviderToolNameCodec,
    tool_calls: BTreeMap<u64, PartialToolCall>,
    reasoning: BTreeMap<ReasoningField, ReasoningReplayState>,
    stop_reason: Option<StopReason>,
    tool_calls_emitted: bool,
    finished: bool,
    done: bool,
}

impl ChatStreamState {
    fn new(config: &ChatCompletionsProviderConfig, tool_names: ProviderToolNameCodec) -> Self {
        Self {
            replay_scope: ReasoningReplayScope {
                provider_id: config.id.clone(),
                model: config.model.clone(),
            },
            tool_names,
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
            if let Some(delta) = self.extract_audio_delta(delta) {
                events.push(ModelStreamEvent::MediaDelta { delta });
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

    fn finish_events(&mut self) -> Result<Vec<ModelStreamEvent>> {
        if self.finished {
            return Ok(Vec::new());
        }
        self.finished = true;
        let mut events = Vec::new();
        if !self.tool_calls_emitted {
            self.tool_calls_emitted = true;
            for tool_call in self.tool_calls.values() {
                events.push(tool_call.to_event(
                    &self.tool_names,
                    &self.replay_scope.provider_id,
                    &self.replay_scope.model,
                )?);
            }
        }
        events.push(ModelStreamEvent::Finished {
            stop_reason: self.stop_reason.clone().unwrap_or(StopReason::Stop),
        });
        Ok(events)
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

    fn extract_audio_delta(&self, delta: &Map<String, Value>) -> Option<MediaDelta> {
        let audio = delta.get("audio")?.as_object()?;
        let data_delta = audio
            .get("data")
            .or_else(|| audio.get("delta"))
            .and_then(Value::as_str)
            .filter(|data| !data.is_empty())
            .map(ToString::to_string);
        let source = audio
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(|id| MediaSource::Provider {
                provider_id: self.replay_scope.provider_id.clone(),
                id: id.into(),
            });
        let format = audio.get("format").and_then(Value::as_str);
        let mime_type = format
            .and_then(|format| ChatAudioFormat::from_wire(format).mime_type())
            .map(str::to_string);
        let replay_descriptor = source.as_ref().and_then(|_| {
            serde_json::to_value(ChatMediaReplayDescriptor {
                v: 1,
                kind: OPENAI_CHAT_MEDIA_REPLAY_KIND.into(),
                provider_id: self.replay_scope.provider_id.clone(),
                model: self.replay_scope.model.clone(),
                field: MediaReplayField::Audio,
            })
            .ok()
        });
        let mut metadata = Map::new();
        for key in ["id", "transcript", "expires_at", "expiresAt", "format"] {
            if let Some(value) = audio.get(key) {
                metadata.insert(key.into(), value.clone());
            }
        }
        let delta = MediaDelta {
            kind: MediaKind::Audio,
            data_delta,
            source,
            mime_type,
            name: None,
            replay_descriptor,
            metadata,
            done: audio.get("done").and_then(Value::as_bool).unwrap_or(false),
        };
        (!delta.is_empty()).then_some(delta)
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

    fn to_event(
        &self,
        tool_names: &ProviderToolNameCodec,
        provider_id: &str,
        model: &str,
    ) -> Result<ModelStreamEvent> {
        let arguments = parse_tool_arguments(&self.arguments_json);
        Ok(ModelStreamEvent::ToolCall {
            tool_call: ToolCall {
                id: self.id.clone(),
                name: tool_names.canonical_name(&self.name, provider_id, model)?,
                arguments,
            },
        })
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

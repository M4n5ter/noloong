//! OpenAI Responses compact endpoint integration.

use crate::util::body_preview;
use noloong_agent_core::{
    AgentCoreError, AgentMessage, BoxFuture, CancellationToken, ContentBlock,
    ContextCompactionOutput, ContextCompactionRequest, ContextCompactor, HttpAuthContext,
    HttpAuthHeader, HttpAuthProvider, HttpAuthRefreshContext, ModelRequest,
    ResponsesApiRequestRenderConfig, ToolSpec, render_responses_api_request,
};
use reqwest::{
    StatusCode,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use serde::Deserialize;
use serde_json::Value;
use std::{fmt::Debug, sync::Arc, time::Duration};

pub const OPENAI_RESPONSES_PAYLOAD_PROVIDER: &str = "openai.responses";
pub const OPENAI_RESPONSES_RESPONSE_ITEM_KIND: &str = "response_item";

#[derive(Clone)]
pub struct OpenAiResponsesCompactorConfig {
    pub id: String,
    pub base_url: String,
    pub render: ResponsesApiRequestRenderConfig,
    pub auth_provider: Option<Arc<dyn HttpAuthProvider>>,
    pub headers: Vec<(String, String)>,
    pub tools: Vec<ToolSpec>,
    pub request_timeout: Duration,
    pub parallel_tool_calls: bool,
}

impl OpenAiResponsesCompactorConfig {
    pub fn new(id: impl Into<String>, model: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            render: ResponsesApiRequestRenderConfig::new(id.clone(), model)
                .fallback_instructions(crate::provider::CHATGPT_CODEX_FALLBACK_INSTRUCTIONS),
            id,
            base_url: crate::provider::CHATGPT_CODEX_RESPONSES_BASE_URL.into(),
            auth_provider: None,
            headers: Vec::new(),
            tools: Vec::new(),
            request_timeout: Duration::from_secs(60),
            parallel_tool_calls: true,
        }
    }

    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn auth_provider(mut self, auth_provider: Arc<dyn HttpAuthProvider>) -> Self {
        self.auth_provider = Some(auth_provider);
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn tool(mut self, tool: ToolSpec) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn tools(mut self, tools: impl IntoIterator<Item = ToolSpec>) -> Self {
        self.tools = tools.into_iter().collect();
        self
    }

    pub fn request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }

    pub fn parallel_tool_calls(mut self, parallel_tool_calls: bool) -> Self {
        self.parallel_tool_calls = parallel_tool_calls;
        self
    }

    pub fn render(mut self, render: ResponsesApiRequestRenderConfig) -> Self {
        self.render = render;
        self
    }
}

impl Debug for OpenAiResponsesCompactorConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiResponsesCompactorConfig")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .field("render", &self.render)
            .field(
                "auth_provider",
                &self.auth_provider.as_ref().map(|provider| provider.id()),
            )
            .field("headers", &self.headers)
            .field("tools", &self.tools)
            .field("request_timeout", &self.request_timeout)
            .field("parallel_tool_calls", &self.parallel_tool_calls)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct OpenAiResponsesCompactor {
    config: OpenAiResponsesCompactorConfig,
    client: reqwest::Client,
    endpoint: String,
}

impl OpenAiResponsesCompactor {
    pub fn new(config: OpenAiResponsesCompactorConfig) -> noloong_agent_core::Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(config.request_timeout)
            .build()?;
        let endpoint = format!(
            "{}/responses/compact",
            config.base_url.trim_end_matches('/')
        );
        Ok(Self {
            config,
            client,
            endpoint,
        })
    }

    pub fn config(&self) -> &OpenAiResponsesCompactorConfig {
        &self.config
    }

    pub async fn compact_request(
        &self,
        request: ContextCompactionRequest,
        cancellation: CancellationToken,
    ) -> noloong_agent_core::Result<ContextCompactionOutput> {
        let payload = self.render_compact_payload(&request)?;
        let mut attempt = 0;
        let mut retry_auth_headers = None;
        loop {
            let headers = self
                .auth_headers(attempt, cancellation.clone(), retry_auth_headers.take())
                .await?;
            let response = self
                .client
                .post(&self.endpoint)
                .headers(headers)
                .json(&payload)
                .send()
                .await?;
            let status = response.status();
            let body = response.text().await?;
            if status.is_success() {
                let response = serde_json::from_str::<CompactResponse>(&body)?;
                let messages = response
                    .output
                    .into_iter()
                    .enumerate()
                    .map(|(index, item)| {
                        response_item_message(&request.run_id, request.turn_id, index, item)
                    })
                    .collect();
                return Ok(ContextCompactionOutput::replacement(messages));
            }
            if status == StatusCode::UNAUTHORIZED
                && attempt == 0
                && let Some(auth_provider) = &self.config.auth_provider
            {
                let context =
                    HttpAuthContext::new(&self.config.id, "POST", self.endpoint.clone(), attempt);
                let refresh = auth_provider
                    .refresh(
                        HttpAuthRefreshContext::unauthorized(context, status.as_u16()),
                        cancellation.clone(),
                    )
                    .await?;
                if refresh.retry {
                    retry_auth_headers = refresh.headers;
                    attempt += 1;
                    continue;
                }
            }
            return Err(AgentCoreError::HttpStatus {
                provider: self.config.id.clone(),
                status: status.as_u16(),
                body: body_preview(&body),
            });
        }
    }

    fn render_compact_payload(
        &self,
        request: &ContextCompactionRequest,
    ) -> noloong_agent_core::Result<Value> {
        let model_request = ModelRequest {
            run_id: request.run_id.clone(),
            turn_id: request.turn_id,
            messages: request.current_messages.clone(),
            context: request.metadata.clone(),
            tools: self.config.tools.clone(),
            metadata: request.metadata.clone(),
        };
        let mut payload = render_responses_api_request(&self.config.render, &model_request)?;
        let Some(object) = payload.as_object_mut() else {
            return Err(AgentCoreError::Provider(
                "responses request renderer returned non-object payload".into(),
            ));
        };
        object.remove("stream");
        object.remove("store");
        object.insert(
            "parallel_tool_calls".into(),
            Value::Bool(self.config.parallel_tool_calls),
        );
        object
            .entry("tools")
            .or_insert_with(|| Value::Array(Vec::new()));
        Ok(payload)
    }

    async fn auth_headers(
        &self,
        attempt: u32,
        cancellation: CancellationToken,
        retry_auth_headers: Option<Vec<HttpAuthHeader>>,
    ) -> noloong_agent_core::Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        for (name, value) in &self.config.headers {
            insert_header(&mut headers, name, value)?;
        }
        if let Some(auth_headers) = retry_auth_headers {
            for header in auth_headers {
                insert_auth_header(&mut headers, header)?;
            }
            return Ok(headers);
        }
        if let Some(auth_provider) = &self.config.auth_provider {
            let context =
                HttpAuthContext::new(&self.config.id, "POST", self.endpoint.clone(), attempt);
            let auth_headers = auth_provider.headers(context, cancellation).await?;
            for header in auth_headers.headers {
                insert_auth_header(&mut headers, header)?;
            }
        }
        Ok(headers)
    }
}

impl ContextCompactor for OpenAiResponsesCompactor {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ContextCompactionOutput> {
        Box::pin(async move { self.compact_request(request, cancellation).await })
    }
}

#[derive(Debug, Deserialize)]
struct CompactResponse {
    output: Vec<Value>,
}

fn response_item_message(run_id: &str, turn_id: u64, index: usize, item: Value) -> AgentMessage {
    AgentMessage::assistant(
        format!("{run_id}:{turn_id}:compact:{index}"),
        vec![ContentBlock::ProviderPayload {
            provider: OPENAI_RESPONSES_PAYLOAD_PROVIDER.into(),
            kind: OPENAI_RESPONSES_RESPONSE_ITEM_KIND.into(),
            value: item,
        }],
    )
}

fn insert_auth_header(
    headers: &mut HeaderMap,
    header: HttpAuthHeader,
) -> noloong_agent_core::Result<()> {
    insert_header(headers, &header.name, &header.value)
}

fn insert_header(
    headers: &mut HeaderMap,
    name: &str,
    value: &str,
) -> noloong_agent_core::Result<()> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|error| AgentCoreError::Provider(format!("invalid header name: {error}")))?;
    let value = HeaderValue::from_str(value)
        .map_err(|error| AgentCoreError::Provider(format!("invalid header value: {error}")))?;
    headers.insert(name, value);
    Ok(())
}

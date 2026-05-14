use crate::{
    AgentCoreError, CancellationToken, EventSinkFuture, HttpAuthContext, HttpAuthHeader,
    HttpAuthProvider, HttpAuthRefreshContext, HttpAuthRefreshResult, MediaBlock, MediaEncoding,
    MediaSource, ModelProvider, ModelRequest, ModelStreamEvent, ModelStreamSink, Result,
};
use base64::{Engine as _, engine::general_purpose};
use reqwest::{
    Url,
    header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue},
};
use std::{collections::BTreeMap, env, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplayScopeMatch {
    Match,
    Ignore,
    Unsupported,
}

pub(crate) async fn emit_model_stream_event(
    stream: &ModelStreamSink,
    events: &mut Vec<ModelStreamEvent>,
    event: ModelStreamEvent,
) -> Result<()> {
    stream(event.clone()).await?;
    events.push(event);
    Ok(())
}

pub(crate) struct CollectedModelStream {
    pub events: Vec<ModelStreamEvent>,
    pub emitted_events: bool,
}

pub(crate) async fn collect_model_stream(
    provider: &dyn ModelProvider,
    request: ModelRequest,
    outer_sink: Option<ModelStreamSink>,
    cancellation: CancellationToken,
) -> Result<CollectedModelStream> {
    let emitted_events = Arc::new(Mutex::new(Vec::new()));
    let emitted_events_for_sink = Arc::clone(&emitted_events);
    let sink: ModelStreamSink = Arc::new(move |event| {
        let emitted_events = Arc::clone(&emitted_events_for_sink);
        let outer_sink = outer_sink.clone();
        Box::pin(async move {
            emitted_events.lock().await.push(event.clone());
            if let Some(outer_sink) = outer_sink {
                outer_sink(event).await?;
            }
            Ok(())
        }) as EventSinkFuture
    });

    let returned_events = provider.stream_model(request, sink, cancellation).await?;
    let mut emitted_events = emitted_events.lock().await;
    if emitted_events.is_empty() {
        Ok(CollectedModelStream {
            events: returned_events,
            emitted_events: false,
        })
    } else {
        Ok(CollectedModelStream {
            events: std::mem::take(&mut *emitted_events),
            emitted_events: true,
        })
    }
}

pub(crate) fn headers_from_map(headers: &BTreeMap<String, String>) -> Result<HeaderMap> {
    let mut rendered = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| AgentCoreError::Provider(format!("invalid header name: {error}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|error| AgentCoreError::Provider(format!("invalid header value: {error}")))?;
        rendered.insert(name, value);
    }
    Ok(rendered)
}

pub(crate) fn headers_from_http_auth(headers: &[HttpAuthHeader]) -> Result<HeaderMap> {
    let mut rendered = HeaderMap::new();
    for header in headers {
        let name = HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|error| AgentCoreError::Provider(format!("invalid header name: {error}")))?;
        let value = HeaderValue::from_str(&header.value)
            .map_err(|error| AgentCoreError::Provider(format!("invalid header value: {error}")))?;
        rendered.insert(name, value);
    }
    Ok(rendered)
}

pub(crate) async fn resolve_auth_headers(
    auth_provider: Option<&Arc<dyn HttpAuthProvider>>,
    api_key: &Option<String>,
    api_key_env: &Option<String>,
    context: HttpAuthContext,
    refreshed_headers: Option<Vec<HttpAuthHeader>>,
    cancellation: CancellationToken,
) -> Result<HeaderMap> {
    if let Some(refreshed_headers) = refreshed_headers {
        return headers_from_http_auth(&refreshed_headers);
    }
    if let Some(auth_provider) = auth_provider {
        let headers = auth_provider.headers(context, cancellation).await?;
        return headers_from_http_auth(&headers.headers);
    }

    let mut headers = HeaderMap::new();
    if let Some(api_key) = resolve_api_key(api_key, api_key_env)? {
        let value = HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|error| {
            AgentCoreError::Provider(format!("invalid bearer authorization header: {error}"))
        })?;
        headers.insert(AUTHORIZATION, value);
    }
    Ok(headers)
}

pub(crate) async fn refresh_auth_provider(
    auth_provider: Option<&Arc<dyn HttpAuthProvider>>,
    context: HttpAuthRefreshContext,
    cancellation: CancellationToken,
) -> Result<Option<HttpAuthRefreshResult>> {
    let Some(auth_provider) = auth_provider else {
        return Ok(None);
    };
    Ok(Some(auth_provider.refresh(context, cancellation).await?))
}

pub(crate) fn resolve_api_key(
    api_key: &Option<String>,
    api_key_env: &Option<String>,
) -> Result<Option<String>> {
    if let Some(api_key) = api_key {
        return Ok(Some(api_key.clone()));
    }
    let Some(api_key_env) = api_key_env else {
        return Ok(None);
    };
    env::var(api_key_env).map(Some).map_err(|_| {
        AgentCoreError::Provider(format!(
            "missing API key environment variable: {api_key_env}"
        ))
    })
}

#[derive(Clone, Copy)]
pub(crate) enum LocalFileUriMediaMaterialization {
    Inline,
    Leave,
    Reject(&'static str),
}

pub(crate) async fn materialize_local_file_uri_media_in_request<F>(
    mut request: ModelRequest,
    policy: F,
) -> Result<ModelRequest>
where
    F: Fn(&MediaBlock) -> LocalFileUriMediaMaterialization + Copy + Send + Sync,
{
    for message in &mut request.messages {
        materialize_local_file_uri_media_in_content(&mut message.content, policy).await?;
    }
    Ok(request)
}

fn materialize_local_file_uri_media_in_content<'a, F>(
    content: &'a mut [crate::ContentBlock],
    policy: F,
) -> crate::providers::BoxFuture<'a, ()>
where
    F: Fn(&MediaBlock) -> LocalFileUriMediaMaterialization + Copy + Send + Sync + 'a,
{
    Box::pin(async move {
        for block in content {
            match block {
                crate::ContentBlock::Media { media } => {
                    materialize_local_file_uri_media(media, policy).await?;
                }
                crate::ContentBlock::ToolResult { content, .. } => {
                    materialize_local_file_uri_media_in_content(content, policy).await?;
                }
                crate::ContentBlock::Thinking { .. }
                | crate::ContentBlock::Text { .. }
                | crate::ContentBlock::Json { .. }
                | crate::ContentBlock::ToolCall { .. }
                | crate::ContentBlock::ProviderPayload { .. } => {}
            }
        }
        Ok(())
    })
}

async fn materialize_local_file_uri_media<F>(media: &mut MediaBlock, policy: F) -> Result<()>
where
    F: Fn(&MediaBlock) -> LocalFileUriMediaMaterialization,
{
    if local_file_uri_path(media)?.is_none() {
        return Ok(());
    }
    match policy(media) {
        LocalFileUriMediaMaterialization::Inline => {
            inline_local_file_uri_media(media).await?;
        }
        LocalFileUriMediaMaterialization::Leave => {}
        LocalFileUriMediaMaterialization::Reject(message) => {
            return Err(AgentCoreError::Provider(message.into()));
        }
    }
    Ok(())
}

pub(crate) async fn inline_local_file_uri_media(media: &mut MediaBlock) -> Result<bool> {
    let Some(path) = local_file_uri_path(media)? else {
        return Ok(false);
    };
    let data = tokio::fs::read(&path).await.map_err(|source| {
        AgentCoreError::Provider(format!(
            "failed to read local media file {}: {source}",
            path.display()
        ))
    })?;
    if media.name.is_none() {
        media.name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned);
    }
    media.source = MediaSource::Inline {
        data: general_purpose::STANDARD.encode(data),
        encoding: MediaEncoding::Base64,
    };
    Ok(true)
}

pub(crate) fn local_file_uri_path(media: &MediaBlock) -> Result<Option<PathBuf>> {
    let MediaSource::Uri { uri } = &media.source else {
        return Ok(None);
    };
    let url = match Url::parse(uri) {
        Ok(url) => url,
        Err(source) if uri.trim_start().starts_with("file:") => {
            return Err(AgentCoreError::Provider(format!(
                "local file URI cannot be parsed: {uri}: {source}"
            )));
        }
        Err(_) => return Ok(None),
    };
    if url.scheme() != "file" {
        return Ok(None);
    }
    url.to_file_path().map(Some).map_err(|()| {
        AgentCoreError::Provider(format!(
            "local file URI cannot be converted to a path: {uri}"
        ))
    })
}

pub(crate) fn replay_scope_match(
    version: u64,
    kind: &str,
    expected_kind: &str,
    provider_id: &str,
    expected_provider_id: &str,
    model: &str,
    expected_model: &str,
) -> ReplayScopeMatch {
    if version != 1 || kind != expected_kind {
        return ReplayScopeMatch::Unsupported;
    }
    if provider_id != expected_provider_id || model != expected_model {
        return ReplayScopeMatch::Ignore;
    }
    ReplayScopeMatch::Match
}

#[cfg(test)]
mod tests {
    use super::local_file_uri_path;
    use crate::{MediaBlock, MediaKind};

    #[test]
    fn malformed_file_uri_reports_provider_error() {
        let media = MediaBlock::uri(MediaKind::File, " file://[::1");

        let error = local_file_uri_path(&media).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("local file URI cannot be parsed")
        );
    }

    #[test]
    fn non_file_invalid_uri_is_ignored_by_local_materialization() {
        let media = MediaBlock::uri(MediaKind::File, "not a url");

        assert!(local_file_uri_path(&media).unwrap().is_none());
    }
}

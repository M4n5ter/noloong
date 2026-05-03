use crate::{AgentCoreError, ModelStreamEvent, ModelStreamSink, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::{collections::BTreeMap, env};

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

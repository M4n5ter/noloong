use crate::{CancellationToken, providers::BoxFuture};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub trait HttpAuthProvider: Send + Sync {
    fn id(&self) -> &str;

    fn headers<'a>(
        &'a self,
        context: HttpAuthContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthHeaders>;

    fn refresh<'a>(
        &'a self,
        _context: HttpAuthRefreshContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthRefreshResult> {
        Box::pin(async { Ok(HttpAuthRefreshResult::deny()) })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthContext {
    pub provider_id: String,
    pub method: String,
    pub url: String,
    pub attempt: u32,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl HttpAuthContext {
    pub fn new(
        provider_id: impl Into<String>,
        method: impl Into<String>,
        url: impl Into<String>,
        attempt: u32,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            method: method.into(),
            url: url.into(),
            attempt,
            metadata: Map::new(),
        }
    }

    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthHeaders {
    #[serde(default)]
    pub headers: Vec<HttpAuthHeader>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl HttpAuthHeaders {
    pub fn new(headers: Vec<HttpAuthHeader>) -> Self {
        Self {
            headers,
            metadata: Map::new(),
        }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthHeader {
    pub name: String,
    pub value: String,
}

impl HttpAuthHeader {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthRefreshContext {
    pub context: HttpAuthContext,
    pub reason: HttpAuthRefreshReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl HttpAuthRefreshContext {
    pub fn unauthorized(context: HttpAuthContext, status: u16) -> Self {
        Self {
            context,
            reason: HttpAuthRefreshReason::Unauthorized,
            status: Some(status),
            metadata: Map::new(),
        }
    }

    pub fn proactive(context: HttpAuthContext) -> Self {
        Self {
            context,
            reason: HttpAuthRefreshReason::Proactive,
            status: None,
            metadata: Map::new(),
        }
    }

    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HttpAuthRefreshReason {
    Unauthorized,
    Proactive,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuthRefreshResult {
    pub retry: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<HttpAuthHeader>>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl HttpAuthRefreshResult {
    pub fn retry() -> Self {
        Self {
            retry: true,
            headers: None,
            metadata: Map::new(),
        }
    }

    pub fn retry_with_headers(headers: Vec<HttpAuthHeader>) -> Self {
        Self {
            retry: true,
            headers: Some(headers),
            metadata: Map::new(),
        }
    }

    pub fn deny() -> Self {
        Self {
            retry: false,
            headers: None,
            metadata: Map::new(),
        }
    }

    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

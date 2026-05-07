use super::{
    ChatGptLoginConfig, DEFAULT_SCOPE, PkceCodes, exchange_authorization_code, generate_pkce,
    generate_state, persist_exchanged_tokens,
};
use crate::auth::{ChatGptTokenData, ChatGptTokenStore};
use crate::{OpenAiIntegrationError, Result};
use std::io;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use url::Url;

#[derive(Debug)]
pub struct BrowserLoginServer {
    listener: TcpListener,
    session: BrowserLoginSession,
}

impl BrowserLoginServer {
    pub async fn bind(config: ChatGptLoginConfig) -> Result<Self> {
        let (listener, port) = bind_callback_listener(&config).await?;
        let session = BrowserLoginSession::new(&config, port)?;
        Ok(Self { listener, session })
    }

    pub fn session(&self) -> &BrowserLoginSession {
        &self.session
    }

    pub fn authorization_url(&self) -> &str {
        &self.session.authorization_url
    }

    pub async fn wait_for_callback(self) -> Result<BrowserCallback> {
        loop {
            let (mut stream, _) = self.listener.accept().await?;
            let mut buffer = vec![0; 8192];
            let read = stream.read(&mut buffer).await?;
            let request = String::from_utf8_lossy(&buffer[..read]);
            let callback = parse_http_request_callback(&request, &self.session.state);
            let response = match callback {
                Ok(_) => plain_text_http_response("200 OK", "Login completed.\n"),
                Err(_) => plain_text_http_response("400 Bad Request", "Login failed.\n"),
            };
            stream.write_all(response.as_bytes()).await?;
            match callback {
                Ok(callback) => return Ok(callback),
                Err(OpenAiIntegrationError::OAuthStateMismatch) => continue,
                Err(error) => return Err(error),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserLoginSession {
    pub port: u16,
    pub redirect_uri: String,
    pub authorization_url: String,
    pub pkce: PkceCodes,
    pub state: String,
}

impl BrowserLoginSession {
    pub fn new(config: &ChatGptLoginConfig, port: u16) -> Result<Self> {
        Self::with_pkce_state(config, port, generate_pkce()?, generate_state()?)
    }

    pub fn with_pkce_state(
        config: &ChatGptLoginConfig,
        port: u16,
        pkce: PkceCodes,
        state: impl Into<String>,
    ) -> Result<Self> {
        let state = state.into();
        let redirect_uri = format!("http://localhost:{port}/auth/callback");
        let authorization_url = build_authorization_url(config, &redirect_uri, &pkce, &state)?;
        Ok(Self {
            port,
            redirect_uri,
            authorization_url,
            pkce,
            state,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserCallback {
    pub code: String,
    pub state: String,
}

impl BrowserCallback {
    pub fn from_redirect_url(url: &str, expected_state: &str) -> Result<Self> {
        let url = Url::parse(url)?;
        let mut code = None;
        let mut state = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "code" => code = Some(value.into_owned()),
                "state" => state = Some(value.into_owned()),
                _ => {}
            }
        }
        let state = state.ok_or(OpenAiIntegrationError::OAuthStateMismatch)?;
        if state != expected_state {
            return Err(OpenAiIntegrationError::OAuthStateMismatch);
        }
        let code = code.ok_or(OpenAiIntegrationError::MissingAuthorizationCode)?;
        Ok(Self { code, state })
    }
}

pub async fn complete_browser_login(
    client: &reqwest::Client,
    config: &ChatGptLoginConfig,
    session: &BrowserLoginSession,
    callback: BrowserCallback,
    store: &dyn ChatGptTokenStore,
) -> Result<ChatGptTokenData> {
    let tokens = exchange_authorization_code(
        client,
        config,
        &session.redirect_uri,
        &session.pkce,
        &callback.code,
    )
    .await?;
    persist_exchanged_tokens(store, tokens)
}

fn build_authorization_url(
    config: &ChatGptLoginConfig,
    redirect_uri: &str,
    pkce: &PkceCodes,
    state: &str,
) -> Result<String> {
    let mut url = Url::parse(&format!("{}/oauth/authorize", config.issuer_base()))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &config.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", DEFAULT_SCOPE)
            .append_pair("code_challenge", &pkce.code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("id_token_add_organizations", "true")
            .append_pair("codex_cli_simplified_flow", "true")
            .append_pair("state", state)
            .append_pair("originator", &config.originator);
        if let Some(workspace_id) = &config.forced_workspace_id {
            query.append_pair("allowed_workspace_id", workspace_id);
        }
    }
    Ok(url.to_string())
}

async fn bind_callback_listener(config: &ChatGptLoginConfig) -> Result<(TcpListener, u16)> {
    match TcpListener::bind(("127.0.0.1", config.preferred_callback_port)).await {
        Ok(listener) => {
            let port = listener.local_addr()?.port();
            Ok((listener, port))
        }
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            let listener = TcpListener::bind(("127.0.0.1", config.fallback_callback_port)).await?;
            let port = listener.local_addr()?.port();
            Ok((listener, port))
        }
        Err(error) => Err(error.into()),
    }
}

fn plain_text_http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn parse_http_request_callback(request: &str, expected_state: &str) -> Result<BrowserCallback> {
    let request_target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| OpenAiIntegrationError::Login("invalid callback request".into()))?;
    BrowserCallback::from_redirect_url(&format!("http://localhost{request_target}"), expected_state)
}

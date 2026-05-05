use std::time::Duration;

pub const DEFAULT_ISSUER: &str = "https://auth.openai.com";
pub const DEFAULT_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_ORIGINATOR: &str = "codex_cli_rs";
pub const DEFAULT_BROWSER_CALLBACK_PORT: u16 = 1455;
pub const FALLBACK_BROWSER_CALLBACK_PORT: u16 = 1457;
pub const DEFAULT_SCOPE: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatGptLoginConfig {
    pub issuer: String,
    pub client_id: String,
    pub originator: String,
    pub preferred_callback_port: u16,
    pub fallback_callback_port: u16,
    pub forced_workspace_id: Option<String>,
    pub device_poll_timeout: Duration,
}

impl ChatGptLoginConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = issuer.into();
        self
    }

    pub fn client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn originator(mut self, originator: impl Into<String>) -> Self {
        self.originator = originator.into();
        self
    }

    pub fn preferred_callback_port(mut self, port: u16) -> Self {
        self.preferred_callback_port = port;
        self
    }

    pub fn fallback_callback_port(mut self, port: u16) -> Self {
        self.fallback_callback_port = port;
        self
    }

    pub fn forced_workspace_id(mut self, workspace_id: impl Into<String>) -> Self {
        self.forced_workspace_id = Some(workspace_id.into());
        self
    }

    pub fn device_poll_timeout(mut self, timeout: Duration) -> Self {
        self.device_poll_timeout = timeout;
        self
    }

    pub fn issuer_base(&self) -> String {
        self.issuer.trim_end_matches('/').to_string()
    }

    pub fn token_endpoint(&self) -> String {
        format!("{}/oauth/token", self.issuer_base())
    }

    pub fn device_api_base(&self) -> String {
        format!("{}/api/accounts", self.issuer_base())
    }
}

impl Default for ChatGptLoginConfig {
    fn default() -> Self {
        Self {
            issuer: DEFAULT_ISSUER.into(),
            client_id: DEFAULT_CLIENT_ID.into(),
            originator: DEFAULT_ORIGINATOR.into(),
            preferred_callback_port: DEFAULT_BROWSER_CALLBACK_PORT,
            fallback_callback_port: FALLBACK_BROWSER_CALLBACK_PORT,
            forced_workspace_id: None,
            device_poll_timeout: Duration::from_secs(15 * 60),
        }
    }
}

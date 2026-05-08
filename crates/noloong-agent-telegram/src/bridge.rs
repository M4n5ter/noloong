use crate::{
    access::{TelegramAccessPolicy, TelegramTextInput},
    config::{TelegramBridgeConfig, TelegramConfigError},
    session::{TelegramSessionKey, telegram_session_metadata},
};
use noloong_agent::interaction::{
    DISPLAY_EVENT_NOTIFICATION, DisplayEvent, InteractionAuthorityCapability,
    InteractionClientError, InteractionClientInfo, InteractionProfileDescriptor,
    InteractionSessionDescriptor, InteractionSessionStatus, InteractionUxCapabilities,
    InteractionWsClient, InteractionWsNotification,
};
use noloong_agent::{ManifestPatch, SystemPromptAddition};
use noloong_agent_core::{AgentMessage, ToolPermissionDecision};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::sync::broadcast;

const METHOD_INITIALIZE: &str = "initialize";
const METHOD_AGENT_PROMPT: &str = "agent/prompt";
const METHOD_AGENT_FOLLOW_UP: &str = "agent/follow_up";
const METHOD_APPROVAL_RESOLVE: &str = "approval/resolve";
const METHOD_SESSION_CREATE: &str = "session/create";
const METHOD_SESSION_GET: &str = "session/get";
const METHOD_DISPLAY_SUBSCRIBE: &str = "display/subscribe";
const TELEGRAM_SYSTEM_PROMPT_ADDITION_ID: &str = "noloong.interaction.telegram";

pub type TelegramBridgeResult<T> = Result<T, TelegramBridgeError>;
pub type TelegramInteractionFuture<'a, T> =
    Pin<Box<dyn Future<Output = TelegramBridgeResult<T>> + Send + 'a>>;

pub trait TelegramInteractionClient: Send + Sync {
    fn request_value<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> TelegramInteractionFuture<'a, Value>;

    fn subscribe(&self) -> broadcast::Receiver<InteractionWsNotification>;
}

impl TelegramInteractionClient for InteractionWsClient {
    fn request_value<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> TelegramInteractionFuture<'a, Value> {
        Box::pin(async move {
            self.request_value(method.to_owned(), params)
                .await
                .map_err(TelegramBridgeError::Interaction)
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<InteractionWsNotification> {
        self.subscribe()
    }
}

#[derive(Debug, Error)]
pub enum TelegramBridgeError {
    #[error("{0}")]
    Config(#[from] TelegramConfigError),
    #[error("interaction request failed: {0}")]
    Interaction(#[from] InteractionClientError),
    #[error("interaction response decode failed: {0}")]
    Decode(String),
    #[error("telegram message is not allowed")]
    Unauthorized,
    #[error("telegram message does not address this bot")]
    NotAddressed,
    #[error("telegram message is empty")]
    EmptyMessage,
    #[error("interaction server did not expose any runtime profile")]
    NoProfiles,
    #[error("session was not found after creation: {0}")]
    MissingSession(String),
}

pub struct TelegramBridge {
    config: TelegramBridgeConfig,
    interaction: Arc<dyn TelegramInteractionClient>,
    state: Mutex<TelegramBridgeState>,
}

#[derive(Default)]
struct TelegramBridgeState {
    profile_id: Option<String>,
    sessions: BTreeMap<TelegramSessionKey, TelegramRuntimeSession>,
}

#[derive(Clone, Debug)]
struct TelegramRuntimeSession {
    session_id: String,
    status: InteractionSessionStatus,
    subscription_id: Option<String>,
}

impl TelegramBridge {
    pub fn new(
        config: TelegramBridgeConfig,
        interaction: Arc<dyn TelegramInteractionClient>,
    ) -> TelegramBridgeResult<Self> {
        config.validate()?;
        Ok(Self {
            config,
            interaction,
            state: Mutex::new(TelegramBridgeState::default()),
        })
    }

    pub fn from_ws_client(
        config: TelegramBridgeConfig,
        interaction: InteractionWsClient,
    ) -> TelegramBridgeResult<Self> {
        Self::new(config, Arc::new(interaction))
    }

    pub fn config(&self) -> &TelegramBridgeConfig {
        &self.config
    }

    pub fn access(&self) -> &TelegramAccessPolicy {
        &self.config.access
    }

    pub async fn initialize(&self) -> TelegramBridgeResult<InteractionInitializeResult> {
        let client_info = InteractionClientInfo {
            name: "noloong-telegram".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            requested_authority: BTreeSet::from([
                InteractionAuthorityCapability::AgentRun,
                InteractionAuthorityCapability::AgentQueue,
                InteractionAuthorityCapability::ApprovalResolve,
            ]),
            requested_ux: InteractionUxCapabilities {
                raw_events: false,
                display_events: true,
                stream_text: true,
                edit_message: true,
                markdown: true,
                max_message_bytes: Some(self.config.max_outbound_chars),
            },
            metadata: Default::default(),
        };
        let result = self
            .request_as::<InteractionInitializeResult>(METHOD_INITIALIZE, json!(client_info))
            .await?;
        let profile_id = self
            .config
            .profile_id
            .clone()
            .or_else(|| {
                result
                    .profiles
                    .first()
                    .map(|profile| profile.profile_id.clone())
            })
            .ok_or(TelegramBridgeError::NoProfiles)?;
        self.state
            .lock()
            .expect("telegram bridge state lock poisoned")
            .profile_id = Some(profile_id);
        Ok(result)
    }

    pub async fn handle_text_message(
        &self,
        input: TelegramTextInput,
        bot_username: Option<&str>,
    ) -> TelegramBridgeResult<InteractionSessionDescriptor> {
        if !self.config.access.allows(input.chat_id, input.user_id) {
            return Err(TelegramBridgeError::Unauthorized);
        }
        if !self.config.access.accepts_text(&input, bot_username) {
            if input.text.trim().is_empty() {
                return Err(TelegramBridgeError::EmptyMessage);
            }
            return Err(TelegramBridgeError::NotAddressed);
        }

        let text = input.text_without_bot_mention(bot_username);
        if text.trim().is_empty() {
            return Err(TelegramBridgeError::EmptyMessage);
        }

        let key = TelegramSessionKey::new(input.chat_id, input.thread_id);
        let session = self.ensure_session(key, &input).await?;
        let message = AgentMessage::user(
            format!("telegram:{}:{}", input.chat_id, input.message_id),
            text,
        );
        let status = self.session_status(&key)?;
        let method = match status {
            InteractionSessionStatus::Running | InteractionSessionStatus::Paused => {
                METHOD_AGENT_FOLLOW_UP
            }
            InteractionSessionStatus::Idle
            | InteractionSessionStatus::Completed
            | InteractionSessionStatus::Aborted
            | InteractionSessionStatus::Failed => METHOD_AGENT_PROMPT,
        };
        let params = if method == METHOD_AGENT_PROMPT {
            json!({
                "sessionId": session.session_id,
                "input": {"type": "message", "message": message},
            })
        } else {
            json!({
                "sessionId": session.session_id,
                "message": message,
            })
        };
        let descriptor = self
            .request_as::<InteractionSessionDescriptor>(method, params)
            .await?;
        self.record_session_status(key, descriptor.status.clone());
        Ok(descriptor)
    }

    pub async fn resolve_approval(
        &self,
        session_id: &str,
        approval_id: &str,
        decision: ToolPermissionDecision,
    ) -> TelegramBridgeResult<InteractionSessionDescriptor> {
        self.request_as(
            METHOD_APPROVAL_RESOLVE,
            json!({
                "sessionId": session_id,
                "approvalId": approval_id,
                "decision": decision,
            }),
        )
        .await
    }

    pub fn subscribe_interaction_notifications(
        &self,
    ) -> broadcast::Receiver<InteractionWsNotification> {
        self.interaction.subscribe()
    }

    pub fn parse_display_notification(
        notification: InteractionWsNotification,
    ) -> TelegramBridgeResult<Option<InteractionDisplayNotification>> {
        if notification.method != DISPLAY_EVENT_NOTIFICATION {
            return Ok(None);
        }
        serde_json::from_value::<InteractionDisplayNotification>(notification.params)
            .map(Some)
            .map_err(|error| TelegramBridgeError::Decode(error.to_string()))
    }

    async fn ensure_session(
        &self,
        key: TelegramSessionKey,
        input: &TelegramTextInput,
    ) -> TelegramBridgeResult<InteractionSessionDescriptor> {
        if let Some(session_id) = self.session_id(&key) {
            let descriptor = self
                .request_as::<InteractionSessionDescriptor>(
                    METHOD_SESSION_GET,
                    json!({"sessionId": session_id}),
                )
                .await?;
            self.record_session_status(key, descriptor.status.clone());
            return Ok(descriptor);
        }

        let session_id = key.session_id();
        let profile_id = self.profile_id()?;
        let descriptor = self
            .request_as::<InteractionSessionDescriptor>(
                METHOD_SESSION_CREATE,
                json!({
                    "sessionId": session_id,
                    "profileId": profile_id,
                    "manifestPatches": [telegram_system_prompt_patch()],
                    "metadata": telegram_session_metadata(
                        input.chat_id,
                        input.thread_id,
                        input.chat_kind.as_str()
                    ),
                }),
            )
            .await?;
        self.record_session(
            key,
            descriptor.session_id.clone(),
            descriptor.status.clone(),
        );
        let subscription = self
            .request_as::<SubscriptionResult>(
                METHOD_DISPLAY_SUBSCRIBE,
                json!({
                    "sessionId": descriptor.session_id,
                    "ux": {
                        "displayEvents": true,
                        "streamText": true,
                        "editMessage": true,
                        "markdown": true,
                        "maxMessageBytes": self.config.max_outbound_chars,
                    }
                }),
            )
            .await?;
        if !self.record_subscription(key, subscription.subscription_id) {
            return Err(TelegramBridgeError::MissingSession(key.session_id()));
        }
        Ok(descriptor)
    }

    fn profile_id(&self) -> TelegramBridgeResult<String> {
        self.state
            .lock()
            .expect("telegram bridge state lock poisoned")
            .profile_id
            .clone()
            .or_else(|| self.config.profile_id.clone())
            .ok_or(TelegramBridgeError::NoProfiles)
    }

    fn session_id(&self, key: &TelegramSessionKey) -> Option<String> {
        self.state
            .lock()
            .expect("telegram bridge state lock poisoned")
            .sessions
            .get(key)
            .map(|session| session.session_id.clone())
    }

    fn session_status(
        &self,
        key: &TelegramSessionKey,
    ) -> TelegramBridgeResult<InteractionSessionStatus> {
        self.state
            .lock()
            .expect("telegram bridge state lock poisoned")
            .sessions
            .get(key)
            .map(|session| session.status.clone())
            .ok_or_else(|| TelegramBridgeError::MissingSession(key.session_id()))
    }

    fn record_session(
        &self,
        key: TelegramSessionKey,
        session_id: String,
        status: InteractionSessionStatus,
    ) {
        let mut state = self
            .state
            .lock()
            .expect("telegram bridge state lock poisoned");
        state.sessions.insert(
            key,
            TelegramRuntimeSession {
                session_id,
                status,
                subscription_id: None,
            },
        );
    }

    fn record_session_status(&self, key: TelegramSessionKey, status: InteractionSessionStatus) {
        if let Some(session) = self
            .state
            .lock()
            .expect("telegram bridge state lock poisoned")
            .sessions
            .get_mut(&key)
        {
            session.status = status;
        }
    }

    fn record_subscription(&self, key: TelegramSessionKey, subscription_id: String) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("telegram bridge state lock poisoned");
        let Some(session) = state.sessions.get_mut(&key) else {
            return false;
        };
        session.subscription_id = Some(subscription_id);
        session.subscription_id.is_some()
    }

    async fn request_as<T>(&self, method: &str, params: Value) -> TelegramBridgeResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let value = self.interaction.request_value(method, params).await?;
        serde_json::from_value(value)
            .map_err(|error| TelegramBridgeError::Decode(error.to_string()))
    }
}

fn telegram_system_prompt_patch() -> ManifestPatch {
    ManifestPatch::UpsertSystemPromptAddition {
        addition: SystemPromptAddition::new(
            TELEGRAM_SYSTEM_PROMPT_ADDITION_ID,
            "Current interaction channel: Telegram. User messages arrive from Telegram chats, and assistant replies are delivered back to Telegram automatically by the bridge. Keep responses suitable for Telegram: concise, split-safe, Markdown-friendly, and useful without requiring the user to see raw JSON-RPC events or host logs.",
        ),
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionInitializeResult {
    pub grant: noloong_agent::interaction::InteractionCapabilityGrant,
    pub profiles: Vec<InteractionProfileDescriptor>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionDisplayNotification {
    pub session_id: String,
    pub subscription_id: String,
    pub event: DisplayEvent,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SubscriptionResult {
    subscription_id: String,
}

#[cfg(test)]
mod tests {
    use super::{
        TELEGRAM_SYSTEM_PROMPT_ADDITION_ID, TelegramBridge, TelegramBridgeError,
        TelegramInteractionClient,
    };
    use crate::{
        access::{TelegramAccessPolicy, TelegramChatKind, TelegramTextInput},
        config::TelegramBridgeConfig,
    };
    use noloong_agent::{
        AgentManifest,
        interaction::{InteractionClientError, InteractionWsNotification},
    };
    use noloong_agent_core::AgentState;
    use serde_json::{Value, json};
    use std::{
        collections::VecDeque,
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn bridge_initializes_interaction() {
        let fake = Arc::new(FakeInteraction::default());
        fake.push_response(json!({
            "grant": {
                "authority": ["agent.run", "agent.queue", "approval.resolve"],
                "ux": {
                    "displayEvents": true,
                    "streamText": true,
                    "editMessage": true,
                    "markdown": true
                }
            },
            "profiles": [profile("default")]
        }));
        let bridge = test_bridge(Arc::clone(&fake), None);

        let result = bridge.initialize().await.unwrap();

        assert_eq!(result.profiles[0].profile_id, "default");
        let calls = fake.calls();
        assert_eq!(calls[0].0, "initialize");
        assert_eq!(
            calls[0].1["requestedAuthority"],
            json!(["agent.run", "agent.queue", "approval.resolve"])
        );
        assert_eq!(calls[0].1["requestedUx"]["displayEvents"], true);
    }

    #[tokio::test]
    async fn bridge_creates_and_subscribes_session() {
        let fake = Arc::new(FakeInteraction::default());
        fake.push_response(initialize_response());
        fake.push_response(session("telegram:42", "default", "idle"));
        fake.push_response(json!({"subscriptionId": "subscription-1"}));
        fake.push_response(session("telegram:42", "default", "running"));
        let bridge = test_bridge(Arc::clone(&fake), None);
        bridge.initialize().await.unwrap();

        let descriptor = bridge
            .handle_text_message(text_input(42, "hello"), Some("noloong_bot"))
            .await
            .unwrap();

        assert_eq!(descriptor.session_id, "telegram:42");
        let calls = fake.calls();
        assert_eq!(calls[1].0, "session/create");
        assert_eq!(calls[1].1["metadata"]["channel"], "telegram");
        assert_eq!(
            calls[1].1["manifestPatches"][0]["op"],
            "upsert_system_prompt_addition"
        );
        assert_eq!(
            calls[1].1["manifestPatches"][0]["addition"]["id"],
            TELEGRAM_SYSTEM_PROMPT_ADDITION_ID
        );
        assert_eq!(calls[2].0, "display/subscribe");
        assert_eq!(calls[3].0, "agent/prompt");
    }

    #[tokio::test]
    async fn bridge_routes_running_session_to_follow_up() {
        let fake = Arc::new(FakeInteraction::default());
        fake.push_response(initialize_response());
        fake.push_response(session("telegram:42", "default", "running"));
        fake.push_response(json!({"subscriptionId": "subscription-1"}));
        fake.push_response(session("telegram:42", "default", "running"));
        fake.push_response(session("telegram:42", "default", "running"));
        fake.push_response(session("telegram:42", "default", "running"));
        let bridge = test_bridge(Arc::clone(&fake), None);
        bridge.initialize().await.unwrap();

        bridge
            .handle_text_message(text_input(42, "first"), Some("noloong_bot"))
            .await
            .unwrap();
        bridge
            .handle_text_message(text_input(42, "second"), Some("noloong_bot"))
            .await
            .unwrap();

        let calls = fake.calls();
        assert_eq!(calls[3].0, "agent/follow_up");
        assert_eq!(calls[5].0, "agent/follow_up");
    }

    #[tokio::test]
    async fn bridge_rejects_unauthorized_message() {
        let fake = Arc::new(FakeInteraction::default());
        let bridge = test_bridge(fake, None);

        let error = bridge
            .handle_text_message(text_input(999, "hello"), Some("noloong_bot"))
            .await
            .unwrap_err();

        assert!(matches!(error, TelegramBridgeError::Unauthorized));
    }

    fn test_bridge(fake: Arc<FakeInteraction>, profile_id: Option<String>) -> TelegramBridge {
        TelegramBridge::new(
            TelegramBridgeConfig {
                bot_token: "token".into(),
                bot_username: None,
                interaction_ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
                interaction_bearer_token: None,
                profile_id,
                message_window_ms: 600,
                long_split_window_ms: 2_000,
                edit_throttle_ms: 750,
                max_outbound_chars: 3900,
                access: TelegramAccessPolicy::new([42], []),
                network: Default::default(),
                show_tool_status: true,
                locale: noloong_agent::Locale::En,
            },
            fake,
        )
        .unwrap()
    }

    fn text_input(chat_id: i64, text: &str) -> TelegramTextInput {
        TelegramTextInput {
            chat_id,
            thread_id: None,
            chat_kind: TelegramChatKind::Private,
            user_id: Some(7),
            message_id: 1,
            text: text.into(),
            is_reply_to_bot: false,
        }
    }

    fn initialize_response() -> Value {
        json!({
            "grant": {
                "authority": ["agent.run", "agent.queue", "approval.resolve"],
                "ux": {
                    "displayEvents": true,
                    "streamText": true,
                    "editMessage": true,
                    "markdown": true
                }
            },
            "profiles": [profile("default")]
        })
    }

    fn profile(profile_id: &str) -> Value {
        json!({
            "profileId": profile_id,
            "displayName": profile_id,
            "defaultManifestPatches": [],
            "metadata": {}
        })
    }

    fn session(session_id: &str, profile_id: &str, status: &str) -> Value {
        json!({
            "sessionId": session_id,
            "profileId": profile_id,
            "status": status,
            "manifest": AgentManifest::default(),
            "state": AgentState::default(),
            "metadata": {}
        })
    }

    struct FakeInteraction {
        calls: Mutex<Vec<(String, Value)>>,
        responses: Mutex<VecDeque<Value>>,
        notifications: broadcast::Sender<InteractionWsNotification>,
    }

    impl Default for FakeInteraction {
        fn default() -> Self {
            let (notifications, _) = broadcast::channel(16);
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(VecDeque::new()),
                notifications,
            }
        }
    }

    impl FakeInteraction {
        fn push_response(&self, value: Value) {
            self.responses
                .lock()
                .expect("fake response lock poisoned")
                .push_back(value);
        }

        fn calls(&self) -> Vec<(String, Value)> {
            self.calls.lock().expect("fake calls lock poisoned").clone()
        }
    }

    impl TelegramInteractionClient for FakeInteraction {
        fn request_value<'a>(
            &'a self,
            method: &'a str,
            params: Value,
        ) -> Pin<Box<dyn Future<Output = Result<Value, TelegramBridgeError>> + Send + 'a>> {
            Box::pin(async move {
                self.calls
                    .lock()
                    .expect("fake calls lock poisoned")
                    .push((method.into(), params));
                self.responses
                    .lock()
                    .expect("fake response lock poisoned")
                    .pop_front()
                    .ok_or(TelegramBridgeError::Interaction(
                        InteractionClientError::Closed,
                    ))
            })
        }

        fn subscribe(&self) -> broadcast::Receiver<InteractionWsNotification> {
            self.notifications.subscribe()
        }
    }
}
